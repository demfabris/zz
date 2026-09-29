import time

MIB = 1 << 20


def record(ctx, phase, samples, notes=None):
    ctx.add(f"mem.footprint.{phase}", "MiB", "mem", samples["zz"].footprint / MIB, samples["tmux"].footprint / MIB, notes)
    ctx.add(f"mem.rss.{phase}", "MiB", "rss", samples["zz"].rss / MIB, samples["tmux"].rss / MIB)
    ctx.add(f"mem.threads.{phase}", "count", "threads", samples["zz"].threads, samples["tmux"].threads)


def scroll(ctx, cols, rows):
    ctx.reset()
    for mux in ctx.muxes:
        ctx.session(mux, "m", cols, rows, "exec sleep 1000000")
        for _ in range(20):
            mux.run("new-window", "-d", "-t", "m:", "seq 1 12000; exec sleep 1000000", check=True)

    last = {}

    def filled(mux):
        rows = mux.out("list-panes", "-s", "-t", "m", "-F", "#{window_index} #{history_size}").splitlines()
        sizes = tuple(int(r.split()[1]) for r in rows if r.split()[0] != "0")
        settled = len(sizes) == 20 and min(sizes) >= 5000 and last.get(mux.name) == sizes
        last[mux.name] = sizes
        return settled

    ok = {m.name: ctx.wait(lambda m=m: filled(m), 60, 0.5) for m in ctx.muxes}
    time.sleep(5.0)
    notes = [f"{n} history not full after 60 s" for n, v in ok.items() if not v]
    for mux in ctx.muxes:
        notes.append(f"{mux.name} pane {mux.out('display-message', '-p', '-t', 'm:1', '#{pane_width}x#{pane_height}')}")
    record(ctx, f"scroll{cols}", ctx.samples(), notes)


def run(ctx):
    for mux in ctx.muxes:
        ctx.session(mux, "m")
    time.sleep(2.0)
    record(ctx, "p1", ctx.samples())
    for mux in ctx.muxes:
        for _ in range(19):
            mux.run("new-window", "-d", "-t", "m:", check=True)
    time.sleep(3.0)
    record(ctx, "p20", ctx.samples())
    if ctx.quick:
        return
    clients = [ctx.attach(mux, "m") for mux in ctx.muxes]
    ctx.drain(clients, 3.0)
    record(ctx, "tui20", ctx.samples())
    for mux, client in zip(ctx.muxes, clients):
        client.detach(mux.detach_keys)
    scroll(ctx, 180, 50)
    scroll(ctx, 80, 24)
