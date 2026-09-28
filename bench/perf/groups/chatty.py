import time

import probe

FLIP = "while :; do echo line $RANDOM padding padding padding padding; sleep 0.01; done"


def setup_steady(ctx, mux):
    ctx.session(mux, "po", 120, 30)
    for _ in range(10):
        mux.run("new-window", "-d", "-t", "po:", ctx.pane("printer.py", 100), check=True)


def setup_flip(ctx, mux):
    ctx.session(mux, "po", 120, 30)
    for _ in range(10):
        mux.run("new-window", "-d", "-t", "po:", check=True)


def start_flip(ctx, mux):
    for w in range(1, 11):
        mux.run("send-keys", "-t", f"po:{w}", FLIP, "Enter", check=True)


def setup_visible(ctx, mux):
    ctx.session(mux, "po", 180, 50)
    for _ in range(3):
        mux.run("split-window", "-d", "-t", "po:0", check=True)
        mux.run("select-layout", "-t", "po:0", "tiled", check=True)


def start_visible(ctx, mux):
    for p in range(4):
        mux.run("send-keys", "-t", f"po:0.{p}", FLIP, "Enter", check=True)


VARIANTS = [
    ("steady", setup_steady, None, False),
    ("flip", setup_flip, start_flip, False),
    ("hidden", setup_flip, start_flip, True),
    ("visible", setup_visible, start_visible, True),
]


def run(ctx):
    seconds = ctx.pick(10.0, 4.0)
    chosen = ["flip", "hidden"] if ctx.quick else [v[0] for v in VARIANTS]
    for name, setup, start, attached in VARIANTS:
        if name not in chosen:
            continue
        ctx.reset()
        for mux in ctx.muxes:
            setup(ctx, mux)
        time.sleep(1.5)
        if start:
            for mux in ctx.muxes:
                start(ctx, mux)
        clients = {}
        if attached:
            for mux in ctx.muxes:
                clients[mux.name] = ctx.attach(mux, "po:0")
        ctx.drain(list(clients.values()), 2.0)
        s0 = ctx.samples()
        cs0 = {n: probe.sample(c.pid).cpu_ns for n, c in clients.items()}
        t0 = time.perf_counter()
        got = ctx.drain(list(clients.values()), seconds)
        elapsed = time.perf_counter() - t0
        s1 = ctx.samples()
        pct = {n: (s1[n].cpu_ns - s0[n].cpu_ns) / 1e9 / elapsed * 100 for n in s1}
        instr = {n: (s1[n].instructions - s0[n].instructions) / 1e6 / elapsed for n in s1}
        ctx.add(f"chatty.cpu_pct.{name}", "%", "cpu", pct["zz"], pct["tmux"])
        ctx.add_instr(f"chatty.instr_per_s.{name}", "Minstr/s", instr["zz"], instr["tmux"])
        if attached:
            client_pct = {n: (probe.sample(c.pid).cpu_ns - cs0[n]) / 1e9 / elapsed * 100 for n, c in clients.items()}
            kbps = {n: got[id(c)] / 1024 / elapsed for n, c in clients.items()}
            ctx.add(f"chatty.tty_kibps.{name}", "KiB/s", "bytes", kbps["zz"], kbps["tmux"])
            ctx.add(f"chatty.client_cpu_pct.{name}", "%", "cpu", client_pct["zz"], client_pct["tmux"])
            for mux in ctx.muxes:
                clients[mux.name].detach(mux.detach_keys)
