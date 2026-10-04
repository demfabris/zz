import json
import os
import re
import signal
import statistics
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
sys.path.insert(0, os.path.join(os.path.dirname(HERE), "groups"))

import isolate
import probe
from attach import MARK_B, QUIET, content, setup
from ptyclient import PtyClient

FIELDS = re.compile(r"^(voluntary_ctxt_switches|nonvoluntary_ctxt_switches):\s+(\d+)", re.M)
TICK_NS = 1e9 / os.sysconf("SC_CLK_TCK")
COMMS = {}
KEYS = ("run_ns", "user_ns", "sys_ns", "slices", "vol", "invol", "syscr", "syscw", "minflt")


class Ctx:
    def session(self, mux, name, cols=180, rows=50, command=None):
        args = ["new-session", "-d", "-s", name, "-x", str(cols), "-y", str(rows)]
        if command:
            args.append(command)
        mux.run(*args, check=True)
        mux.run("set-option", "-g", "default-size", f"{cols}x{rows}")


def stat_fields(path):
    raw = open(path).read()
    return raw[raw.rindex(")") + 2 :].split()


def thread(base):
    rest = stat_fields(f"{base}/stat")
    status = dict(FIELDS.findall(open(f"{base}/status").read()))
    io = dict(line.split(": ") for line in open(f"{base}/io").read().splitlines())
    run = open(f"{base}/schedstat").read().split()
    return {
        "comm": open(f"{base}/comm").read().strip(),
        "run_ns": int(run[0]),
        "user_ns": int(rest[11]) * TICK_NS,
        "sys_ns": int(rest[12]) * TICK_NS,
        "slices": int(run[2]),
        "vol": int(status["voluntary_ctxt_switches"]),
        "invol": int(status["nonvoluntary_ctxt_switches"]),
        "syscr": int(io["syscr"]),
        "syscw": int(io["syscw"]),
        "minflt": int(rest[7]),
    }


def snapshot(pid):
    out = {}
    for tid in os.listdir(f"/proc/{pid}/task"):
        try:
            out[tid] = thread(f"/proc/{pid}/task/{tid}")
        except (FileNotFoundError, ProcessLookupError, ValueError):
            pass
    try:
        rest = stat_fields(f"/proc/{pid}/stat")
        out["process"] = {
            "comm": "process",
            "user_ns": int(rest[11]) * TICK_NS,
            "sys_ns": int(rest[12]) * TICK_NS,
            "minflt": int(rest[7]),
            "run_ns": probe.sample(pid).cpu_ns,
            "instr": probe.sample(pid).instructions,
        }
    except FileNotFoundError:
        pass
    return out


def delta(before, after):
    rows = {}
    for tid, now in after.items():
        then = before.get(tid, {k: 0 for k in now})
        name = now["comm"] if tid == "process" else f"{now['comm']}"
        row = rows.setdefault(name, {})
        for key, value in now.items():
            if key == "comm":
                continue
            row[key] = row.get(key, 0) + value - then.get(key, 0)
        row["threads"] = row.get("threads", 0) + (0 if tid == "process" else 1)
        if tid not in before and tid != "process":
            row["new"] = row.get("new", 0) + 1
    return rows


def median_rows(runs):
    names = set()
    for run in runs:
        names |= set(run)
    out = {}
    for name in sorted(names):
        keys = set()
        for run in runs:
            keys |= set(run.get(name, {}))
        out[name] = {k: statistics.median([run.get(name, {}).get(k, 0) for run in runs]) for k in sorted(keys)}
    return out


def mean_rows(runs):
    names = set()
    for run in runs:
        names |= set(run)
    out = {}
    for name in sorted(names):
        keys = set()
        for run in runs:
            keys |= set(run.get(name, {}))
        out[name] = {k: sum(run.get(name, {}).get(k, 0) for run in runs) / len(runs) for k in sorted(keys)}
    return out


def start_traced(tracer, mux, env, out):
    if mux.name == "zz":
        argv = [mux.binary, "--socket", mux.socket, "-f", env.config, "daemon"]
    else:
        argv = [mux.binary, "-L", mux.label, "-f", env.config, "-D"]
    proc = subprocess.Popen([tracer, out, *argv], env=dict(mux.env, **{k: "1" for k in ("TIMELINE", "FORKS") if os.environ.get(k)}), stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if mux.run("list-sessions").returncode in (0, 1) and mux.server_pids():
            break
        time.sleep(0.1)
    return proc


def dumps(path):
    if not os.path.exists(path):
        return []
    blocks, cur = [], None
    events = []
    for line in open(path):
        parts = line.split()
        if parts[0] == "dump":
            cur = []
        elif parts[0] == "end":
            if events:
                cur.append(("events", events))
            events = []
            blocks.append(cur)
            cur = None
        elif parts[0] == "ev":
            if cur is not None:
                events.append(parts[1:])
        elif cur is not None:
            COMMS[parts[0]] = parts[1]
            cur.append((parts[1], int(parts[2]), int(parts[3]), int(parts[4])))
    return blocks


def mark(proc, path):
    want = len(dumps(path)) + 1
    proc.send_signal(signal.SIGUSR1)
    deadline = time.monotonic() + 5
    while len(dumps(path)) < want and time.monotonic() < deadline:
        time.sleep(0.01)


def names():
    table = {}
    for line in open("/usr/include/asm/unistd_64.h"):
        parts = line.split()
        if len(parts) == 3 and parts[0] == "#define" and parts[1].startswith("__NR_"):
            table[int(parts[2])] = parts[1][5:]
    return table


def once(mux, env):
    client = PtyClient(mux.attach_argv("b"), mux.env, env, cols=180, rows=50)
    seen = client.read_until(content(MARK_B, 4), 5.0)
    quiet = client.drain_quiet(QUIET, 3.0)
    client.detach(mux.detach_keys)
    time.sleep(0.2)
    return seen is not None, quiet, client.total


def main():
    zz_bin, tmux_bin, out_path = sys.argv[1], sys.argv[2], sys.argv[3]
    runs = int(os.environ.get("RUNS", "10"))
    only = os.environ.get("ONLY", "zz,tmux").split(",")
    perf = os.environ.get("PERF")
    tracer = os.environ.get("TRACE")
    env = isolate.Env()
    muxes = []
    if "zz" in only:
        muxes.append(isolate.Zz(zz_bin, env))
    if "tmux" in only:
        muxes.append(isolate.Tmux(tmux_bin, env))
    result = {"runs": runs, "muxes": {}}
    try:
        ctx = Ctx()
        traced = {}
        for mux in muxes:
            if tracer:
                path = f"{out_path}.{mux.name}.trace"
                if os.path.exists(path):
                    os.unlink(path)
                traced[mux.name] = (start_traced(tracer, mux, env, path), path)
            setup(ctx, mux)
        time.sleep(1.0)
        rows = {m.name: [] for m in muxes}
        meta = {m.name: {"missed": 0, "loud": 0, "bytes": []} for m in muxes}
        perfs = {}
        if perf:
            for mux in muxes:
                path = f"{out_path}.{mux.name}.perf"
                perfs[mux.name] = (path, subprocess.Popen(["perf", "stat", "--per-thread", "-x", ",", "-o", path, "-e", perf, "-p", str(mux.pid())]))
            time.sleep(0.5)
        for i in range(runs):
            for mux in muxes if i % 2 == 0 else muxes[::-1]:
                pid = mux.pid()
                if mux.name in traced:
                    mark(*traced[mux.name])
                before = snapshot(pid)
                seen, quiet, total = once(mux, env)
                after = snapshot(pid)
                if mux.name in traced:
                    mark(*traced[mux.name])
                rows[mux.name].append(delta(before, after))
                meta[mux.name]["missed"] += not seen
                meta[mux.name]["loud"] += not quiet
                meta[mux.name]["bytes"].append(total)
                time.sleep(0.1)
        for name, (path, proc) in perfs.items():
            proc.send_signal(signal.SIGINT)
            proc.wait(10)
            result.setdefault("perf", {})[name] = open(path).read()
        table = names() if traced else {}
        for name, (proc, path) in traced.items():
            blocks = dumps(path)
            windows = blocks[1::2][: runs]
            per = {}
            timeline = []
            for block in windows:
                for item in block:
                    if item[0] == "events":
                        timeline.append(item[1])
                        continue
                    comm, nr, count, ns = item
                    key = f"{comm}/{table.get(nr, nr)}"
                    cur = per.setdefault(key, [0, 0])
                    cur[0] += count / len(windows)
                    cur[1] += ns / len(windows)
            result.setdefault("trace", {})[name] = per
            if timeline:
                tids = {}
                with open(f"{out_path}.{name}.timeline", "w") as f:
                    for n, events in enumerate(timeline):
                        t0 = int(events[0][0]) if events else 0
                        f.write(f"run {n}\n")
                        for t, tid, nr, a0, a1, a2, ret, link in events:
                            label = COMMS.get(tid, "?").replace("zz-pty-", "")[:12]
                            f.write(f"{(int(t) - t0) / 1000:10.1f} {label:12s} {table.get(int(nr), nr):16s} {a0:>6s} {a2:>8s} -> {ret:>8s} {link}\n")
        for mux in muxes:
            result["muxes"][mux.name] = {"median": median_rows(rows[mux.name]), "mean": mean_rows(rows[mux.name]), "meta": meta[mux.name], "runs": rows[mux.name]}
    finally:
        result["strays"] = env.cleanup()
    with open(out_path, "w") as f:
        json.dump(result, f, indent=1)
    for name, data in result["muxes"].items():
        print(f"== {name} {data['meta']['missed']} missed {data['meta']['loud']} loud bytes {statistics.median(data['meta']['bytes'])}")
        print(f"{'thread':18s} {'n':>3s} " + " ".join(f"{k:>9s}" for k in KEYS))
        for thread_name, row in sorted(data["median"].items(), key=lambda kv: -kv[1].get("run_ns", 0)):
            print(f"{thread_name:18s} {row.get('threads', 0):3.0f} " + " ".join(f"{row.get(k, 0):9.0f}" for k in KEYS))
    for name, per in result.get("trace", {}).items():
        print(f"== syscalls per attach {name} total {sum(v[0] for v in per.values()):.1f}")
        for key, (count, ns) in sorted(per.items(), key=lambda kv: -kv[1][0]):
            print(f"  {key:44s} {count:7.1f} {ns / 1000:9.1f} us")
    for name, text in result.get("perf", {}).items():
        print(f"== perf {name}")
        print(text)


if __name__ == "__main__":
    main()
