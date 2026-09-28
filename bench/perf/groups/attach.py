import time

from sockproxy import SockProxy

MARK_A = b"ZZPFMARKA"
MARK_B = b"ZZPFMARKB"
HOLD = "exec sleep 1000000"
QUIET = 0.2


def setup(ctx, mux):
    ctx.session(mux, "a", 180, 50, f"echo {MARK_A.decode()}; {HOLD}")
    ctx.session(mux, "b", 180, 50, f"echo {MARK_B.decode()}; {HOLD}")
    for _ in range(3):
        mux.run("split-window", "-d", "-t", "b:0", f"echo {MARK_B.decode()}; {HOLD}", check=True)
        mux.run("select-layout", "-t", "b:0", "tiled", check=True)


def content(marker, count):
    return lambda buf: buf.count(marker) >= count


def once(ctx, mux, target, marker, count, env=None):
    client = ctx.attach(mux, target, env=env)
    seen = client.read_until(content(marker, count), 5.0)
    ttfc = (seen - client.t0) * 1000 if seen else None
    if seen:
        at = 0
        for _ in range(count):
            at = client.buf.index(marker, at) + len(marker)
        before = at
    else:
        before = None
    return client, ttfc, before


def run(ctx):
    runs = ctx.pick(10, 4)
    for mux in ctx.muxes:
        setup(ctx, mux)
    time.sleep(1.0)
    for size, target, marker, count in (("p1", "a", MARK_A, 1), ("p4", "b", MARK_B, 4)):
        ttfc = {m.name: [] for m in ctx.muxes}
        tty = {m.name: [] for m in ctx.muxes}
        total = {m.name: [] for m in ctx.muxes}
        cpu = {m.name: [] for m in ctx.muxes}
        instr = {m.name: [] for m in ctx.muxes}
        missed = {m.name: 0 for m in ctx.muxes}
        loud = {m.name: 0 for m in ctx.muxes}
        for i in range(runs):
            for mux in ctx.order(i):
                s0 = mux.sample()
                client, ms, before = once(ctx, mux, target, marker, count)
                if not client.drain_quiet(QUIET, 3.0):
                    loud[mux.name] += 1
                sent = client.total
                client.detach(mux.detach_keys)
                time.sleep(0.2)
                s1 = mux.sample()
                if ms is None:
                    missed[mux.name] += 1
                else:
                    ttfc[mux.name].append(ms)
                    tty[mux.name].append(before)
                    total[mux.name].append(sent)
                cpu[mux.name].append((s1.cpu_ns - s0.cpu_ns) / 1e6)
                instr[mux.name].append((s1.instructions - s0.instructions) / 1e6)
                time.sleep(0.1)
        notes = [f"{n} never showed content {k}/{runs}" for n, k in missed.items() if k]
        quiet_notes = [f"{n} never went quiet for {QUIET * 1000:.0f} ms within 3 s, {k}/{runs}" for n, k in loud.items() if k]
        ctx.add(f"attach.ttfc.{size}", "ms", "wall", ttfc["zz"], ttfc["tmux"], notes)
        ctx.add(f"attach.tty_total.{size}", "B", "bytes", total["zz"], total["tmux"], [f"tty bytes from attach until {QUIET * 1000:.0f} ms of quiet"] + quiet_notes)
        ctx.add(f"attach.tty_bytes.{size}", "B", "bytes_info", tty["zz"], tty["tmux"], "tty bytes up to the last content marker; depends on draw order")
        ctx.add(f"attach.cpu.{size}", "ms", "cpu", cpu["zz"], cpu["tmux"])
        ctx.add_instr(f"attach.instr.{size}", "Minstr", instr["zz"], instr["tmux"])
    proxy_path = ctx.env.path("px.sock")
    proxy = SockProxy(proxy_path, ctx.zz.socket)
    try:
        env = dict(ctx.zz.env, ZZ_SOCKET=proxy_path)
        for size, target, marker, count in (("p1", "a", MARK_A, 1), ("p4", "b", MARK_B, 4)):
            conns, s2c, frames, c2s = [], [], [], []
            for _ in range(ctx.pick(3, 1)):
                start = proxy.counters()
                client, _, _ = once(ctx, ctx.zz, target, marker, count, env=env)
                client.drain(1.0)
                end = proxy.counters()
                client.detach(ctx.zz.detach_keys)
                time.sleep(0.2)
                conns.append(end["connections"] - start["connections"])
                s2c.append(end["s2c"] - start["s2c"])
                frames.append(end["s2c_frames"] - start["s2c_frames"])
                c2s.append(end["c2s"] - start["c2s"])
            note = "zz only: daemon to client socket bytes from spawn to 1 s after content"
            ctx.add(f"attach.conns.{size}", "count", "count", conns, None, "zz only: daemon connections opened by one attach")
            ctx.add(f"attach.wire_s2c.{size}", "B", "bytes", s2c, None, note)
            ctx.add(f"attach.wire_frames.{size}", "count", "count", frames, None, "zz only")
            ctx.add(f"attach.wire_c2s.{size}", "B", "bytes", c2s, None, "zz only")
    finally:
        proxy.close()
