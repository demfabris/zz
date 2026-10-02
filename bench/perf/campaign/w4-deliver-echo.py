import json
import os
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))

import isolate
import probe
from groups.echo import LETTERS, LINE, keystroke
from ptyclient import PtyClient

PANE = os.path.join(os.path.dirname(HERE), "pane", "echo.py")


def main():
    zz_bin, tmux_bin, out_path = sys.argv[1], sys.argv[2], sys.argv[3]
    runs = 150
    env = isolate.Env()
    muxes = [isolate.Zz(zz_bin, env), isolate.Tmux(tmux_bin, env)]
    result = {}
    try:
        for mux in muxes:
            mux.run("new-session", "-d", "-s", "e", "-x", "120", "-y", "40", f"exec python3 {PANE} 0", check=True)
            mux.run("set-option", "-g", "status", "off", check=True)
        time.sleep(1.0)
        for mux in muxes:
            client = PtyClient(mux.attach_argv("e"), mux.env, env, cols=120, rows=40)
            client.drain(1.5)
            s0, c0 = mux.sample(), probe.sample(client.pid)
            lat = []
            for i in range(runs):
                if i and i % LINE == 0:
                    client.write(b"\r")
                    client.drain(0.05)
                ms = keystroke(client, LETTERS[i % len(LETTERS)], i + 1)
                if ms is not None:
                    lat.append(ms)
                client.drain(0.03)
            s1, c1 = mux.sample(), probe.sample(client.pid)
            lat.sort()
            result[mux.name] = {
                "p50_ms_unreliable_under_load": lat[len(lat) // 2] if lat else None,
                "server_kinstr_per_key": (s1.instructions - s0.instructions) / runs / 1e3,
                "server_cpu_us_per_key": (s1.cpu_ns - s0.cpu_ns) / runs / 1e3,
                "client_kinstr_per_key": (c1.instructions - c0.instructions) / runs / 1e3,
                "client_cpu_us_per_key": (c1.cpu_ns - c0.cpu_ns) / runs / 1e3,
                "missed": runs - len(lat),
            }
            client.detach(mux.detach_keys)
    finally:
        result["strays"] = env.cleanup()
    with open(out_path, "w") as f:
        json.dump(result, f, indent=1)
    print(json.dumps(result, indent=1))


if __name__ == "__main__":
    main()
