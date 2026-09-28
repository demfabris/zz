import time


def run(ctx):
    seconds = ctx.pick(10.0, 5.0)
    for mux in ctx.muxes:
        ctx.session(mux, "i")
        for _ in range(19):
            mux.run("new-window", "-d", "-t", "i:", check=True)
    time.sleep(3.0)
    s0 = ctx.samples()
    t0 = time.perf_counter()
    time.sleep(seconds)
    elapsed = time.perf_counter() - t0
    s1 = ctx.samples()
    pct = {n: (s1[n].cpu_ns - s0[n].cpu_ns) / 1e9 / elapsed * 100 for n in s1}
    wake = {n: (s1[n].wakeups - s0[n].wakeups) / elapsed for n in s1}
    instr = {n: (s1[n].instructions - s0[n].instructions) / 1e6 / elapsed for n in s1}
    ctx.add("idle.cpu_pct.p20", "%", "cpu", pct["zz"], pct["tmux"])
    ctx.add_instr("idle.instr_per_s.p20", "Minstr/s", instr["zz"], instr["tmux"])
    ctx.add("idle.wakeups_per_s.p20", "1/s", "count", wake["zz"], wake["tmux"])
