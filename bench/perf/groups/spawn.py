import time

import timing

VARIANTS = [
    ("split_shell", ["split-window", "-d", "-t", "s:0"]),
    ("split_empty_P", ["split-window", "-d", "-P", "-F", "#{pane_id}", "-t", "s:0", ""]),
    ("new_window", ["new-window", "-d", "-t", "s:"]),
]
SETTLE = 0.1


def run(ctx):
    runs = ctx.pick(10, 4)
    for mux in ctx.muxes:
        ctx.session(mux, "s")
    time.sleep(0.5)
    kill_wall = {m.name: [] for m in ctx.muxes}
    kill_cpu = {m.name: [] for m in ctx.muxes}
    kill_instr = {m.name: [] for m in ctx.muxes}
    for variant, args in VARIANTS:
        wall = {m.name: [] for m in ctx.muxes}
        cpu = {m.name: [] for m in ctx.muxes}
        instr = {m.name: [] for m in ctx.muxes}
        bad = {m.name: 0 for m in ctx.muxes}
        for i in range(runs):
            for mux in ctx.order(i):
                s0 = mux.sample()
                ms, code = timing.spawn_ms(mux.argv(*args), mux.env)
                time.sleep(SETTLE)
                s1 = mux.sample()
                wall[mux.name].append(ms)
                cpu[mux.name].append((s1.cpu_ns - s0.cpu_ns) / 1e6)
                instr[mux.name].append((s1.instructions - s0.instructions) / 1e6)
                bad[mux.name] += code != 0
                if variant == "new_window":
                    mux.run("kill-window", "-a", "-t", "s:0")
                    continue
                panes = mux.out("list-panes", "-t", "s:0", "-F", "#{pane_id}").split()
                for pane in panes[1:]:
                    s2 = mux.sample()
                    kms, _ = timing.spawn_ms(mux.argv("kill-pane", "-t", pane), mux.env)
                    time.sleep(SETTLE)
                    s3 = mux.sample()
                    kill_wall[mux.name].append(kms)
                    kill_cpu[mux.name].append((s3.cpu_ns - s2.cpu_ns) / 1e6)
                    kill_instr[mux.name].append((s3.instructions - s2.instructions) / 1e6)
        notes = [f"{n} exited nonzero {k}/{runs}" for n, k in bad.items() if k]
        ctx.add(f"spawn.wall.{variant}", "ms", "wall", wall["zz"], wall["tmux"], notes)
        ctx.add(f"spawn.cpu.{variant}", "ms", "cpu", cpu["zz"], cpu["tmux"])
        ctx.add_instr(f"spawn.instr.{variant}", "Minstr", instr["zz"], instr["tmux"])
    ctx.add("spawn.wall.kill_pane", "ms", "wall", kill_wall["zz"], kill_wall["tmux"])
    ctx.add("spawn.cpu.kill_pane", "ms", "cpu", kill_cpu["zz"], kill_cpu["tmux"])
    ctx.add_instr("spawn.instr.kill_pane", "Minstr", kill_instr["zz"], kill_instr["tmux"])
