import time

import probe

STATUS = "#(date +%s) #(echo zzpf) #(uname -s)"


def run(ctx):
    seconds = ctx.pick(10.0, 5.0)
    for mux in ctx.muxes:
        ctx.session(mux, "j", 180, 50)
        mux.run("set-option", "-g", "status-interval", "1", check=True)
        mux.run("set-option", "-g", "status-right", STATUS, check=True)
    clients = [ctx.attach(m, "j") for m in ctx.muxes]
    ctx.drain(clients, 2.5)
    pids = {m.name: m.pid() for m in ctx.muxes}
    seen = {n: probe.thread_ids(p) for n, p in pids.items()}
    initial = {n: set(v) for n, v in seen.items()}

    def tick():
        for n, p in pids.items():
            seen[n] |= probe.thread_ids(p)

    s0 = ctx.samples()
    t0 = time.perf_counter()
    got = ctx.drain(clients, seconds, tick=tick)
    elapsed = time.perf_counter() - t0
    s1 = ctx.samples()
    pct = {n: (s1[n].cpu_ns - s0[n].cpu_ns) / 1e9 / elapsed * 100 for n in s1}
    child = {n: (s1[n].child_cpu_ns - s0[n].child_cpu_ns) / 1e9 / elapsed * 100 for n in s1}
    created = {n: len(seen[n] - initial[n]) / elapsed for n in seen}
    note = "sampled every ~2 ms; a thread that lives shorter than that is missed"
    if not probe.thread_ids_unique():
        note += "; thread handles may be reused, so this is a lower bound"
    instr = {n: (s1[n].instructions - s0[n].instructions) / 1e6 / elapsed for n in s1}
    ctx.add("statusjob.cpu_pct", "%", "cpu", pct["zz"], pct["tmux"])
    ctx.add_instr("statusjob.instr_per_s", "Minstr/s", instr["zz"], instr["tmux"])
    ctx.add("statusjob.threads_per_s", "1/s", "threads", created["zz"], created["tmux"], note)
    ctx.add("statusjob.child_cpu_pct", "%", "cpu_info", child["zz"], child["tmux"], "reaped job children")
    kib = {m.name: got[id(c)] / 1024 / elapsed for m, c in zip(ctx.muxes, clients)}
    ctx.add("statusjob.tty_kibps", "KiB/s", "bytes", kib["zz"], kib["tmux"])
    for mux, client in zip(ctx.muxes, clients):
        client.detach(mux.detach_keys)
