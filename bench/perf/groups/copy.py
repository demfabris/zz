import shlex
import time

import isolate
from groups.control import Control

COLS = 180
ROWS = 50
MIB = 1 << 20


def history_file(path):
    stripe = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
    with open(path, "w", encoding="ascii") as f:
        for row in range(isolate.HISTORY_LIMIT + ROWS):
            prefix = f"{row:08d} "
            text = (stripe * ((COLS + len(stripe) - 1) // len(stripe)))[:COLS - len(prefix)]
            ending = "\n" if row + 1 < isolate.HISTORY_LIMIT + ROWS else ""
            f.write(prefix + text + ending)


def pane_facts(mux):
    return tuple(int(value) for value in mux.out(
        "display-message", "-p", "-t", "copy:0.0",
        "#{pane_width} #{pane_height} #{history_size} #{pane_in_mode}",
    ).split())


def check_pane(mux, mode):
    facts = pane_facts(mux)
    expected = (COLS, ROWS, isolate.HISTORY_LIMIT, mode)
    if facts != expected:
        raise RuntimeError(f"{mux.name} copy pane has width, height, history, mode {facts}, expected {expected}")


def enter(control):
    target = control.ends + 1
    errors = control.errors
    started = time.perf_counter_ns()
    control.send("copy-mode -t copy:0.0\n")
    if not control.wait_ends(target, 30):
        raise RuntimeError(f"{control.mux.name} copy-mode entry timed out")
    elapsed = (time.perf_counter_ns() - started) / 1e6
    if control.errors != errors:
        raise RuntimeError(f"{control.mux.name} copy-mode entry returned %error")
    return elapsed


def run(ctx):
    path = ctx.env.path("copy-history.txt")
    history_file(path)
    values = {name: {mux.name: [] for mux in ctx.muxes} for name in ("footprint", "rss", "cpu", "wall", "instr")}
    for iteration in range(ctx.pick(7, 3)):
        ctx.reset()
        for mux in ctx.order(iteration):
            ctx.session(mux, "copy", COLS, ROWS, f"cat {shlex.quote(path)}; exec sleep 1000000")
        for mux in ctx.muxes:
            if not ctx.wait(lambda mux=mux: pane_facts(mux)[2] == isolate.HISTORY_LIMIT, 60, 0.1):
                raise RuntimeError(f"{mux.name} copy pane did not reach {isolate.HISTORY_LIMIT} history rows")
        controls = {}
        try:
            for mux in ctx.order(iteration):
                controls[mux.name] = Control(ctx, mux, "copy")
                controls[mux.name].settle()
                check_pane(mux, 0)
            time.sleep(5.0)
            for mux in ctx.order(iteration):
                control = controls[mux.name]
                control.settle(0.05)
                before = mux.sample()
                wall = enter(control)
                after = mux.sample()
                check_pane(mux, 1)
                values["wall"][mux.name].append(wall)
                values["footprint"][mux.name].append((after.footprint - before.footprint) / MIB)
                values["rss"][mux.name].append((after.rss - before.rss) / MIB)
                values["cpu"][mux.name].append((after.cpu_ns - before.cpu_ns) / 1e6)
                values["instr"][mux.name].append((after.instructions - before.instructions) / 1e6)
        finally:
            for control in controls.values():
                control.close()
    notes = "First entry per fresh server; 10000 dense history rows at 180 columns in a 180x50 pane; 5 s idle before entry; persistent control client"
    for name, unit, kind in (("footprint", "MiB", "mem"), ("rss", "MiB", "rss"), ("cpu", "ms", "cpu"), ("wall", "ms", "wall")):
        ctx.add(f"mem.copy_{name}.scroll180", unit, kind, values[name]["zz"], values[name]["tmux"], notes)
    ctx.add_instr("mem.copy_instr.scroll180", "Minstr", values["instr"]["zz"], values["instr"]["tmux"], notes)
