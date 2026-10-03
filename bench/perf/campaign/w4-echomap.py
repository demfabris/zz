import json
import os
import random
import re
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))

import isolate
import probe
from groups.echo import LETTERS, LINE, TAG, text
from ptyclient import PtyClient

PANE = os.path.join(os.path.dirname(HERE), "pane", "echo.py")
FIELDS = re.compile(r"^(voluntary_ctxt_switches|nonvoluntary_ctxt_switches):\s+(\d+)", re.M)


CLOCK = time.CLOCK_UPTIME_RAW if sys.platform == "darwin" else time.CLOCK_MONOTONIC
OFFSET = time.clock_gettime_ns(CLOCK) - int(time.perf_counter() * 1e9)


def stamp(perf):
    return int(perf * 1e9) + OFFSET


def mac_threads(pid):
    import ctypes
    ru = probe._RusageV4()
    ti = probe._TaskInfo()
    if probe._libproc.proc_pid_rusage(int(pid), probe._RUSAGE_INFO_V4, ctypes.byref(ru)) != 0:
        return {}
    if probe._libproc.proc_pidinfo(int(pid), probe._PROC_PIDTASKINFO, ctypes.c_uint64(0), ctypes.byref(ti), ctypes.sizeof(ti)) != ctypes.sizeof(ti):
        return {}
    return {"task": {
        "comm": "task",
        "vol": ti.csw,
        "invol": ru.pkg_idle_wkups + ru.interrupt_wkups,
        "syscr": ti.syscalls_unix,
        "syscw": ti.syscalls_mach,
        "run_ns": probe._ticks(ru.user_time + ru.system_time),
        "slices": ru.instructions,
    }}


def threads(pid):
    if sys.platform == "darwin":
        return mac_threads(pid)
    out = {}
    try:
        tids = os.listdir(f"/proc/{pid}/task")
    except FileNotFoundError:
        return out
    for tid in tids:
        base = f"/proc/{pid}/task/{tid}"
        try:
            comm = open(f"{base}/comm").read().strip()
            status = dict(FIELDS.findall(open(f"{base}/status").read()))
            io = dict(line.split(": ") for line in open(f"{base}/io").read().splitlines())
            run = open(f"{base}/schedstat").read().split()
        except (FileNotFoundError, ProcessLookupError, PermissionError):
            continue
        out[tid] = {
            "comm": comm,
            "vol": int(status["voluntary_ctxt_switches"]),
            "invol": int(status["nonvoluntary_ctxt_switches"]),
            "syscr": int(io["syscr"]),
            "syscw": int(io["syscw"]),
            "run_ns": int(run[0]),
            "slices": int(run[2]),
        }
    return out


def snapshot(groups):
    return {name: {pid: threads(pid) for pid in pids} for name, pids in groups.items()}


def delta(before, after, keys):
    rows = []
    for name, procs in after.items():
        for pid, tids in procs.items():
            for tid, now in tids.items():
                then = before.get(name, {}).get(pid, {}).get(tid)
                if then is None:
                    continue
                row = {"proc": name, "pid": pid, "thread": now["comm"]}
                for field in ("vol", "invol", "syscr", "syscw", "run_ns", "slices"):
                    row[field] = (now[field] - then[field]) / keys
                rows.append(row)
    return rows


def keystroke(client, char, count, timeout=1.0):
    start = len(client.buf)
    token = TAG + b"%03d" % (count % 1000)
    t0 = time.perf_counter()
    client.write(bytes([char]))
    seen = client.read_until(lambda buf: token in text(buf[start:]), timeout)
    return (t0, seen)


def pin(groups, spec):
    for item in filter(None, spec.split(";")):
        name, cpus = item.split("=")
        if name == "bench":
            os.sched_setaffinity(0, {int(c) for c in cpus.split(",")})
            continue
        for pid in groups.get(name, []):
            subprocess.run(["taskset", "-a", "-p", "-c", cpus, str(pid)], capture_output=True, check=True)


def series(mux, client, runs, rng, counter):
    keys = []
    for i in range(runs):
        if i and i % LINE == 0:
            client.write(b"\r")
            client.drain(0.05)
        counter[0] += 1
        t0, seen = keystroke(client, LETTERS[i % len(LETTERS)], counter[0])
        keys.append({"n": counter[0], "t0": stamp(t0), "seen": stamp(seen) if seen else None})
        client.drain(rng.uniform(0.02, 0.05))
    return keys


def main():
    zz_bin, tmux_bin, out_path = sys.argv[1], sys.argv[2], sys.argv[3]
    runs = int(os.environ.get("RUNS", "200"))
    only = os.environ.get("ONLY", "zz,tmux").split(",")
    variants = [v for v in os.environ.get("VARIANTS", "idle:0,busy30:30").split(",")]
    extra = json.loads(os.environ.get("MUX_ENV", "{}"))
    spinners = [subprocess.Popen(["nice", "-n", "19", "sh", "-c", "while :; do :; done"]) for _ in range(int(os.environ.get("SPIN", "0")))]
    affinity = os.environ.get("AFFINITY", "")
    env = isolate.Env()
    muxes = []
    if "zz" in only:
        muxes.append(isolate.Zz(zz_bin, env))
    if "tmux" in only:
        muxes.append(isolate.Tmux(tmux_bin, env))
    result = {"runs": runs, "muxes": {}}
    try:
        for mux in muxes:
            mux.env.update(extra.get(mux.name, {}))
            out = result["muxes"][mux.name] = {}
            for variant in variants:
                name, hz = variant.split(":")
                mux.run("new-session", "-d", "-s", f"e{name}", "-x", "120", "-y", "40", f"exec python3 {PANE} {hz}", check=True)
            mux.run("set-option", "-g", "status", "off", check=True)
            time.sleep(1.0)
            rng = random.Random(7)
            for variant in variants:
                name, _ = variant.split(":")
                client = PtyClient(mux.attach_argv(f"e{name}"), mux.env, env, cols=120, rows=40)
                client.drain(1.5)
                server = sorted(set(mux.server_pids()) | {mux.pid()})
                panes = sorted(isolate.descendants(server) - set(server))
                groups = {"server": server, "client": [client.pid], "pane": panes}
                pin(groups, affinity)
                counter = [0]
                idle0 = snapshot(groups)
                t_idle = time.perf_counter()
                if not os.environ.get("NOIDLE"):
                    client.drain(runs * 0.035 + (runs // LINE) * 0.05)
                idle_s = max(time.perf_counter() - t_idle, 1e-9)
                idle1 = snapshot(groups)
                t_keys = time.perf_counter()
                keys = series(mux, client, runs, rng, counter)
                keys_s = time.perf_counter() - t_keys
                after = snapshot(groups)
                lat = sorted((k["seen"] - k["t0"]) / 1e6 for k in keys if k["seen"])
                out[name] = {
                    "p50_ms": lat[len(lat) // 2] if lat else None,
                    "p90_ms": lat[len(lat) * 9 // 10] if lat else None,
                    "p99_ms": lat[min(len(lat) - 1, len(lat) * 99 // 100)] if lat else None,
                    "missed": runs - len(lat),
                    "idle_s": idle_s,
                    "keys_s": keys_s,
                    "pids": groups,
                    "idle_per_key": delta(idle0, idle1, runs),
                    "keys_per_key": delta(idle1, after, runs),
                    "keys": keys,
                }
                client.detach(mux.detach_keys)
            mux.kill()
    finally:
        for spinner in spinners:
            spinner.kill()
        result["strays"] = env.cleanup()
    with open(out_path, "w") as f:
        json.dump(result, f, indent=1)
    for mux, variants_out in result["muxes"].items():
        for name, row in variants_out.items():
            print(mux, name, "p50", round(row["p50_ms"], 3), "p90", round(row["p90_ms"], 3), "p99", round(row["p99_ms"], 3), "missed", row["missed"])


if __name__ == "__main__":
    main()
