import time

import timing

OPTIONS = [
    'set -g status-left "#[fg=colour{n}]#{{session_name}} {n} "',
    'set -g status-right "#{{?client_prefix,P,}} %H:%M {n}"',
    "set -g history-limit {h}",
    "set -g escape-time {e}",
    "set -g display-time {h}",
    'set -g status-style "fg=colour{n},bg=default"',
    'set -g message-style "fg=colour{n}"',
    'setw -g window-status-format "#I:#W {n}"',
    'setw -g window-status-current-format "#[bold]#I:#W {n}"',
    "set -g renumber-windows {onoff}",
]
KEYS = "abcdefghijklmnopqrstuvwxyz0123456789"


def generate(path, lines=1000):
    out = []
    n = 0
    while len(out) < lines:
        kind = n % 10
        if kind < 4:
            out.append(f'set -g @plugin_opt_{n} "value {n} #{{session_name}}"')
        elif kind < 7:
            table = f"zzpf{n % 25}"
            key = KEYS[(n // 25) % len(KEYS)]
            out.append(f"bind-key -T {table} {key} display-message x{n}")
        else:
            template = OPTIONS[n % len(OPTIONS)]
            out.append(template.format(n=n % 256, h=2000 + n, e=10 + n % 50, onoff="on" if n % 2 else "off"))
        n += 1
    with open(path, "w") as f:
        f.write("\n".join(out[:lines]) + "\n")
    return path


def run(ctx):
    runs = ctx.pick(8, 3)
    path = generate(ctx.env.path("gen1000.conf"))
    for mux in ctx.muxes:
        ctx.session(mux, "s")
    time.sleep(0.5)
    notes = []
    for mux in ctx.muxes:
        proc = mux.run("source-file", path)
        errors = [l for l in (proc.stdout + proc.stderr).splitlines() if l.strip()]
        if proc.returncode or errors:
            notes.append(f"{mux.name} reported {len(errors)} lines on stderr/stdout, exit {proc.returncode}")
    wall = {m.name: [] for m in ctx.muxes}
    cpu = {m.name: [] for m in ctx.muxes}
    instr = {m.name: [] for m in ctx.muxes}
    for i in range(runs):
        for mux in ctx.order(i):
            s0 = mux.sample()
            ms, _ = timing.spawn_ms(mux.argv("source-file", path), mux.env)
            time.sleep(0.05)
            s1 = mux.sample()
            wall[mux.name].append(ms)
            cpu[mux.name].append((s1.cpu_ns - s0.cpu_ns) / 1e6)
            instr[mux.name].append((s1.instructions - s0.instructions) / 1e6)
    ctx.add("config.wall.source_1000", "ms", "wall", wall["zz"], wall["tmux"], notes)
    ctx.add("config.cpu.source_1000", "ms", "cpu", cpu["zz"], cpu["tmux"])
    ctx.add_instr("config.instr.source_1000", "Minstr", instr["zz"], instr["tmux"])
