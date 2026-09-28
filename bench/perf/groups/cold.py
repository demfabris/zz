import os
import stat
import time

import timing


def infocmp_shim(ctx):
    real = "/usr/bin/infocmp"
    for d in ctx.env.env["PATH"].split(":"):
        if d and os.access(os.path.join(d, "infocmp"), os.X_OK):
            real = os.path.join(d, "infocmp")
            break
    shim_dir = ctx.env.path("shim")
    os.makedirs(shim_dir, exist_ok=True)
    log = ctx.env.path("infocmp.log")
    shim = os.path.join(shim_dir, "infocmp")
    with open(shim, "w") as f:
        f.write(f"#!/bin/sh\necho \"$*\" >> {log}\nexec {real} \"$@\"\n")
    os.chmod(shim, os.stat(shim).st_mode | stat.S_IXUSR)
    return shim_dir, log


def run(ctx):
    runs = ctx.pick(12, 4)
    walls = {m.name: [] for m in ctx.muxes}
    noterm = {m.name: [] for m in ctx.muxes}
    for i in range(runs):
        for mux in ctx.order(i):
            for bucket, env in ((walls, mux.env), (noterm, {k: v for k, v in mux.env.items() if k != "TERM"})):
                mux.kill()
                time.sleep(0.1)
                ms, code = timing.spawn_ms(mux.argv("new-session", "-d", "-s", "a", "-x", "180", "-y", "50"), env)
                if code == 0:
                    bucket[mux.name].append(ms)
                ctx.wait(mux.server_running, 2)
    ctx.add("cold.wall.new_session", "ms", "wall", walls["zz"], walls["tmux"])
    ctx.add("cold.wall.new_session_noterm", "ms", "wall", noterm["zz"], noterm["tmux"])
    shim_dir, log = infocmp_shim(ctx)
    forks = {}
    for mux in ctx.muxes:
        mux.kill()
        if os.path.exists(log):
            os.unlink(log)
        env = dict(mux.env, PATH=f"{shim_dir}:{mux.env['PATH']}")
        mux.run("new-session", "-d", "-s", "a", env=env, check=True)
        time.sleep(0.3)
        forks[mux.name] = sum(1 for _ in open(log)) if os.path.exists(log) else 0
        mux.kill()
    ctx.add(
        "cold.infocmp_forks",
        "count",
        "count",
        forks["zz"],
        forks["tmux"],
        "infocmp runs during a cold new-session -d with TERM set; each one is a fork+exec on the first client's path",
    )
