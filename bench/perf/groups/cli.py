import time

import timing

CHAIN = [
    "display-message", "-p", "#{pane_id}", ";",
    "list-panes", "-t", "s", ";",
    "show-options", "-gqv", "status", ";",
    "has-session", "-t", "s", ";",
    "select-pane", "-t", "s:0.0",
]

VERBS = [
    ("version", ["-V"]),
    ("display", ["display-message", "-p", "#{pane_id}"]),
    ("list_panes", ["list-panes", "-t", "s"]),
    ("show_options", ["show-options", "-gqv", "status"]),
    ("has_session", ["has-session", "-t", "s"]),
    ("send_keys", ["send-keys", "-t", "s:0.0", ""]),
    ("select_pane", ["select-pane", "-t", "s:0.0"]),
    ("list_keys", ["list-keys"]),
    ("chain5", CHAIN),
]

LISTS = [
    ("list_panes_a", ["list-panes", "-a"]),
    ("list_windows_a", ["list-windows", "-a"]),
    ("list_sessions", ["list-sessions"]),
]


def measure(ctx, verb, args, size, runs, warm, cpu=True):
    walls = {m.name: [] for m in ctx.muxes}
    bad = {m.name: 0 for m in ctx.muxes}
    argv = {m.name: m.argv(*args) for m in ctx.muxes}
    before = None
    for i in range(warm + runs):
        if i == warm and cpu:
            before = ctx.samples()
        for mux in ctx.order(i):
            ms, code = timing.spawn_ms(argv[mux.name], mux.env)
            if i >= warm:
                walls[mux.name].append(ms)
                bad[mux.name] += code != 0
    notes = [f"{name} exited nonzero {n}/{runs}" for name, n in bad.items() if n]
    ctx.add(f"cli.wall.{verb}.{size}", "ms", "wall", walls["zz"], walls["tmux"], notes)
    if cpu:
        after = ctx.samples()
        per = {n: (after[n].cpu_ns - before[n].cpu_ns) / runs / 1e6 for n in after}
        ctx.add(f"cli.cpu.{verb}.{size}", "ms", "cpu", per["zz"], per["tmux"])
        if after["zz"].instructions:
            instr = {n: (after[n].instructions - before[n].instructions) / runs / 1e6 for n in after}
            ctx.add(f"cli.instr.{verb}.{size}", "Minstr", "instr", instr["zz"], instr["tmux"])


def run(ctx):
    runs = ctx.pick(60, 20)
    warm = 5
    for mux in ctx.muxes:
        ctx.session(mux, "s")
    time.sleep(0.5)
    for verb, args in VERBS:
        measure(ctx, verb, args, "p1", runs, warm, cpu=verb != "version")
    for mux in ctx.muxes:
        for _ in range(19):
            mux.run("new-window", "-d", "-t", "s:", check=True)
    time.sleep(1.0)
    for verb, args in VERBS:
        if verb != "version":
            measure(ctx, verb, args, "p20", runs, warm)
    for mux in ctx.muxes:
        mux.run("kill-window", "-a", "-t", "s:0", check=True)
        for n in range(1, 20):
            mux.run("new-session", "-d", "-s", f"s{n}", check=True)
    time.sleep(1.0)
    for verb, args in LISTS:
        measure(ctx, verb, args, "s20", runs, warm)
