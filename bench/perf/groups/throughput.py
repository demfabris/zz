import fcntl
import os
import pty
import struct
import termios
import time

import fixtures


def ceiling_seconds(path, cols=180, rows=50):
    start = time.perf_counter()
    pid, fd = pty.fork()
    if pid == 0:
        os.execv("/bin/cat", ["/bin/cat", path])
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
    try:
        while os.read(fd, 65536):
            pass
    except OSError:
        pass
    finally:
        os.close(fd)
    os.waitpid(pid, 0)
    return time.perf_counter() - start


def read_seconds(path, timeout, pump=None):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if os.path.exists(path):
            with open(path) as f:
                text = f.read().strip()
            if text:
                return float(text)
        if pump:
            pump()
        else:
            time.sleep(0.05)
    return None


def detached(ctx, kind, path, runs):
    size = os.path.getsize(path)
    rate = {m.name: [] for m in ctx.muxes}
    ceiling = []
    grid = {}
    for mux in ctx.muxes:
        ctx.session(mux, "t", 180, 50)
    time.sleep(0.5)
    for i in range(runs):
        ceiling.append(size / ceiling_seconds(path) / 1e6)
        for mux in ctx.order(i):
            out = ctx.env.path(f"tp-{kind}-{mux.name}-{i}.txt")
            mux.run("new-window", "-d", "-t", "t:", ctx.pane("timer.py", path, out), check=True)
            grid[mux.name] = mux.out("display-message", "-p", "-t", "t:1", "#{pane_width}x#{pane_height}")
            seconds = read_seconds(out, 300)
            if seconds:
                rate[mux.name].append(size / seconds / 1e6)
            mux.run("kill-window", "-t", "t:1")
            time.sleep(0.3)
    notes = [f"grid {n} {g}" for n, g in grid.items()]
    ctx.add(f"throughput.detached.{kind}", "MB/s", "throughput", rate["zz"], rate["tmux"], notes, better="higher")
    ctx.add(f"throughput.ceiling.{kind}", "MB/s", "ceiling", ceiling, None, "a bare reader of cat through a cooked 180x50 pty, in the same run", better="higher")
    ctx.ceilings[kind] = ceiling
    for mux in ctx.muxes:
        mux.kill()


def attached(ctx, path, runs):
    ms = {m.name: [] for m in ctx.muxes}
    tty = {m.name: [] for m in ctx.muxes}
    for mux in ctx.muxes:
        ctx.session(mux, "ta", 180, 50)
    time.sleep(0.5)
    clients = {m.name: ctx.attach(m, "ta", 180, 51) for m in ctx.muxes}
    ctx.drain(list(clients.values()), 1.0)
    for i in range(runs):
        for mux in ctx.order(i):
            client = clients[mux.name]
            out = ctx.env.path(f"tpa-{mux.name}-{i}.txt")
            before = client.total
            mux.run("new-window", "-t", "ta:", ctx.pane("timer.py", path, out), check=True)
            seconds = read_seconds(out, 300, pump=lambda: client.drain(0.02))
            if seconds:
                ms[mux.name].append(seconds * 1000)
                tty[mux.name].append(client.total - before)
            mux.run("kill-window", "-t", "ta:1")
            ctx.drain(list(clients.values()), 0.5)
    ctx.add("throughput.attached.ascii_ms", "ms", "throughput", ms["zz"], ms["tmux"])
    size = os.path.getsize(path)
    ctx.add("throughput.ceiling.ascii_ms", "ms", "ceiling", [size / (c * 1e6) * 1000 for c in ctx.ceilings.get("ascii", [])] or None, None, "the detached ASCII ceiling as the time to read this file")
    ctx.add("throughput.attached.tty_bytes", "B", "bytes_info", tty["zz"], tty["tmux"])
    for mux in ctx.muxes:
        clients[mux.name].detach(mux.detach_keys)


def run(ctx):
    runs = ctx.pick(3, 1)
    ascii_path = fixtures.fixture("ascii", 150)
    detached(ctx, "ascii", ascii_path, runs)
    if not ctx.quick:
        detached(ctx, "unicode", fixtures.fixture("unicode", 150), runs)
        attached(ctx, ascii_path, runs)
