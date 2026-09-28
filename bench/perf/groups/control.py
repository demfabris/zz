import os
import select
import subprocess
import time

import fixtures


class Control:
    def __init__(self, ctx, mux, target):
        self.mux = mux
        self.proc = subprocess.Popen(
            mux.control_argv(target), env=mux.env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL
        )
        ctx.env.clients.add(self.proc.pid)
        self.ctx = ctx
        self.fd = self.proc.stdout.fileno()
        os.set_blocking(self.fd, False)
        self.pending = b""
        self.ends = 0
        self.total = 0
        self.tail = b""
        self.marker = None
        self.found = False
        self.first_output = None

    def pump(self, timeout):
        ready, _, _ = select.select([self.fd], [], [], timeout)
        if not ready:
            return False
        try:
            data = os.read(self.fd, 1 << 20)
        except BlockingIOError:
            return False
        if not data:
            return False
        self.total += len(data)
        window = self.tail + data
        if self.marker and self.first_output is None and b"%output " in window:
            self.first_output = time.perf_counter()
        if self.marker and self.marker in window:
            self.found = True
        self.tail = window[-64:]
        self.pending += data
        lines = self.pending.split(b"\n")
        self.pending = lines.pop()
        for line in lines:
            if line.startswith((b"%end ", b"%error ")):
                self.ends += 1
        return True

    def settle(self, quiet=0.3):
        while self.pump(quiet):
            pass

    def send(self, text):
        self.proc.stdin.write(text.encode())
        self.proc.stdin.flush()

    def wait_ends(self, count, timeout=30):
        deadline = time.perf_counter() + timeout
        while self.ends < count and time.perf_counter() < deadline:
            self.pump(0.05)
        return self.ends >= count

    def close(self):
        try:
            self.proc.stdin.close()
        except OSError:
            pass
        try:
            self.proc.wait(timeout=3)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            self.proc.wait()
        self.ctx.env.clients.discard(self.proc.pid)


def run(ctx):
    runs = ctx.pick(200, 50)
    for mux in ctx.muxes:
        ctx.session(mux, "c", 180, 50)
    time.sleep(0.5)
    ctls = {m.name: Control(ctx, m, "c") for m in ctx.muxes}
    for c in ctls.values():
        c.settle()
    lat = {m.name: [] for m in ctx.muxes}
    before = ctx.samples()
    for i in range(runs):
        for mux in ctx.order(i):
            c = ctls[mux.name]
            target = c.ends + 1
            t0 = time.perf_counter()
            c.send("display-message -p x\n")
            if c.wait_ends(target, 5):
                lat[mux.name].append((time.perf_counter() - t0) * 1000)
    after = ctx.samples()
    cpu = {n: (after[n].cpu_ns - before[n].cpu_ns) / runs / 1e6 for n in after}
    instr = {n: (after[n].instructions - before[n].instructions) / runs / 1e6 for n in after}
    ctx.add("control.latency", "ms", "wall", lat["zz"], lat["tmux"])
    ctx.add("control.cpu_per_cmd", "ms", "cpu", cpu["zz"], cpu["tmux"])
    ctx.add_instr("control.instr_per_cmd", "Minstr", instr["zz"], instr["tmux"])
    burst = {}
    for mux in ctx.muxes:
        c = ctls[mux.name]
        c.settle(0.1)
        target = c.ends + runs
        t0 = time.perf_counter()
        c.send("display-message -p x\n" * runs)
        ok = c.wait_ends(target, 60)
        burst[mux.name] = runs / (time.perf_counter() - t0) if ok else None
    ctx.add("control.burst_cmds_per_s", "cmd/s", "wall", burst["zz"], burst["tmux"], better="higher")
    path = fixtures.fixture("ascii", 16)
    size = os.path.getsize(path)
    rate = {m.name: [] for m in ctx.muxes}
    stream = {m.name: [] for m in ctx.muxes}
    for i in range(ctx.pick(2, 1)):
        for mux in ctx.order(i):
            c = ctls[mux.name]
            c.settle(0.1)
            marker = f"ZZPFDONE{i}"
            out = ctx.env.path(f"ctl-{mux.name}-{i}.txt")
            c.marker = marker.encode()
            c.found = False
            c.first_output = None
            c.tail = b""
            start_total = c.total
            t0 = time.perf_counter()
            mux.run("new-window", "-d", "-t", "c:", ctx.pane("timer.py", path, out, marker), check=True)
            deadline = t0 + 120
            while not c.found and time.perf_counter() < deadline:
                c.pump(0.05)
            if c.found and c.first_output is not None:
                rate[mux.name].append(size / (time.perf_counter() - c.first_output) / 1e6)
                stream[mux.name].append((c.total - start_total) / size)
            mux.run("kill-window", "-a", "-t", "c:0")
    ctx.add("control.output_mbps", "MB/s", "wall", rate["zz"], rate["tmux"], "16 MiB cat seen as %output, first %output line to the done marker", better="higher")
    ctx.add("control.output_bytes_per_byte", "x", "bytes_info", stream["zz"], stream["tmux"])
    for c in ctls.values():
        c.close()
