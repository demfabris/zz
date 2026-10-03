import json
import os
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))

import isolate
import probe
from groups.control import Control
from sockproxy import SockProxy


class Ctx:
    pass


def measure(ctx, mux, env=None, runs=400):
    saved = mux.env
    if env:
        mux.env = env
    ctl = Control(ctx, mux, "c")
    mux.env = saved
    ctl.settle()
    server0 = mux.sample()
    client0 = probe.sample(ctl.proc.pid)
    t0 = time.perf_counter()
    for _ in range(runs):
        target = ctl.ends + 1
        ctl.send("display-message -p x\n")
        ctl.wait_ends(target, 5)
    wall = (time.perf_counter() - t0) / runs * 1e3
    server1 = mux.sample()
    client1 = probe.sample(ctl.proc.pid)
    ctl.close()
    return {
        "wall_ms_per_cmd": wall,
        "server_cpu_us": (server1.cpu_ns - server0.cpu_ns) / runs / 1e3,
        "server_kinstr": (server1.instructions - server0.instructions) / runs / 1e3,
        "client_cpu_us": (client1.cpu_ns - client0.cpu_ns) / runs / 1e3,
        "client_kinstr": (client1.instructions - client0.instructions) / runs / 1e3,
        "server_wakeups": (server1.wakeups - server0.wakeups) / runs,
        "client_wakeups": (client1.wakeups - client0.wakeups) / runs,
    }


def main():
    zz_bin, tmux_bin, out_path = sys.argv[1], sys.argv[2], sys.argv[3]
    env = isolate.Env()
    zz = isolate.Zz(zz_bin, env)
    tmux = isolate.Tmux(tmux_bin, env)
    ctx = Ctx()
    ctx.env = env
    result = {}
    try:
        for mux in (zz, tmux):
            mux.run("new-session", "-d", "-s", "c", "-x", "180", "-y", "50", check=True)
        time.sleep(0.5)
        for mux in (zz, tmux):
            result[mux.name] = measure(ctx, mux)
        proxy_path = env.path("px.sock")
        proxy = SockProxy(proxy_path, zz.socket)
        try:
            ctl = Control(ctx, type("M", (), {"env": dict(zz.env, ZZ_SOCKET=proxy_path), "control_argv": zz.control_argv})(), "c")
            ctl.settle()
            start = proxy.counters()
            runs = 200
            for _ in range(runs):
                target = ctl.ends + 1
                ctl.send("display-message -p x\n")
                ctl.wait_ends(target, 5)
            end = proxy.counters()
            ctl.close()
            result["zz_wire_per_cmd"] = {k: (end[k] - start[k]) / runs for k in ("c2s", "s2c", "s2c_frames")}
        finally:
            proxy.close()
    finally:
        result["strays"] = env.cleanup()
    with open(out_path, "w") as f:
        json.dump(result, f, indent=1)
    print(json.dumps(result, indent=1))


if __name__ == "__main__":
    main()
