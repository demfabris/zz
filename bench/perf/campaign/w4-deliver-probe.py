import json
import os
import re
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))

import isolate
import probe
from groups.control import Control
from ptyclient import PtyClient


def thread_names(pid):
    if probe.MACOS:
        out = subprocess.run(["sample", str(pid), "1", "-mayDie"], capture_output=True, text=True).stdout
        names = []
        for line in out.splitlines():
            m = re.match(r"^\s*\d+\s+Thread_\d+(.*)$", line)
            if m:
                rest = m.group(1).strip()
                names.append(rest.split(":", 1)[1].strip() if ":" in rest else rest or "<unnamed>")
        return names
    names = []
    for tid in os.listdir(f"/proc/{pid}/task"):
        with open(f"/proc/{pid}/task/{tid}/comm") as f:
            names.append(f.read().strip())
    return names


def summarize(names):
    counts = {}
    for name in names:
        key = re.sub(r"\d+$", "N", name)
        counts[key] = counts.get(key, 0) + 1
    return dict(sorted(counts.items()))


def instr_over(mux, seconds, clients=()):
    before = mux.sample()
    deadline = time.perf_counter() + seconds
    while time.perf_counter() < deadline:
        for c in clients:
            c.drain(0.01)
        if not clients:
            time.sleep(0.05)
    after = mux.sample()
    return {
        "minstr": (after.instructions - before.instructions) / 1e6,
        "cpu_ms": (after.cpu_ns - before.cpu_ns) / 1e6,
        "threads": after.threads,
    }


def main():
    zz_bin = sys.argv[1]
    out_path = sys.argv[2]
    env = isolate.Env()
    zz = isolate.Zz(zz_bin, env)
    result = {"binary": zz_bin, "host": os.uname().nodename}
    try:
        zz.run("new-session", "-d", "-s", "m", "-x", "180", "-y", "50", check=True)
        time.sleep(2.0)
        pid = zz.pid()
        result["p1"] = {"threads": zz.sample().threads, "names": summarize(thread_names(pid))}
        for _ in range(19):
            zz.run("new-window", "-d", "-t", "m:", check=True)
        time.sleep(3.0)
        result["p20"] = {"threads": zz.sample().threads, "names": summarize(thread_names(pid))}

        class Ctx:
            pass

        ctx = Ctx()
        ctx.env = env
        ctl = Control(ctx, zz, "m")
        ctl.settle()
        time.sleep(1.0)
        result["p20_control"] = {"threads": zz.sample().threads, "names": summarize(thread_names(pid))}
        zz.run("send-keys", "-t", "m:0", "for i in $(seq 1 50); do echo line$i; done", "Enter", check=True)
        ctl.settle()
        result["p20_control_output"] = {"threads": zz.sample().threads, "names": summarize(thread_names(pid)), "control_bytes": ctl.total}
        ctl.close()
        time.sleep(1.0)
        result["p20_after_control"] = {"threads": zz.sample().threads}

        zz.run(
            "new-session",
            "-d",
            "-s",
            "f",
            "-x",
            "120",
            "-y",
            "40",
            "while :; do printf 'tick %s\\n' $RANDOM; sleep 0.02; done",
            check=True,
        )
        time.sleep(1.0)
        fanout = {}
        fanout["clients0"] = instr_over(zz, 5.0)
        a = PtyClient(zz.attach_argv("f"), zz.env, env, cols=120, rows=40)
        a.drain(2.0)
        fanout["clients1"] = instr_over(zz, 5.0, [a])
        b = PtyClient(zz.attach_argv("f"), zz.env, env, cols=120, rows=40)
        b.drain(2.0)
        fanout["clients2"] = instr_over(zz, 5.0, [a, b])
        fanout["clients1_minus_0"] = fanout["clients1"]["minstr"] - fanout["clients0"]["minstr"]
        fanout["clients2_minus_1"] = fanout["clients2"]["minstr"] - fanout["clients1"]["minstr"]
        fanout["frames_per_s_nominal"] = 50
        result["fanout"] = fanout
        result["tui2_threads"] = {"threads": zz.sample().threads, "names": summarize(thread_names(pid))}
    finally:
        result["strays"] = env.cleanup()
    with open(out_path, "w") as f:
        json.dump(result, f, indent=1)
    print(json.dumps(result, indent=1))


if __name__ == "__main__":
    main()
