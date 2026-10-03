import bisect
import glob
import json
import os
import statistics
import sys

ZZ_CHAIN = [
    ("client key read", "probe", 1, "client"),
    ("client socket write", "probe", 2, "client"),
    ("loop wake", "probe", 10, "server"),
    ("loop socket read", "probe", 11, "server"),
    ("loop dispatch key", "probe", 12, "server"),
    ("loop key queued + wake", "probe", 13, "server"),
    ("shard wake (key)", "probe", (20, 29), "server"),
    ("shard key command", "probe", 21, "server"),
    ("shard PTY write", "probe", 22, "server"),
    ("pane read", "sct", (1,), "pane"),
    ("pane write", "sct", (2,), "pane"),
    ("gather wake", "probe", 30, "server"),
    ("gather read", "probe", 31, "server"),
    ("gather send + wake", "probe", 32, "server"),
    ("shard wake (data)", "probe", (20, 29), "server"),
    ("shard data", "probe", 23, "server"),
    ("shard parse done", "probe", 24, "server"),
    ("frame encode start", "probe", 25, "server"),
    ("frame encoded", "probe", 26, "server"),
    ("socket write (direct)", "probe", (27, 14), "server"),
    ("publish returned", "probe", 28, "server"),
    ("client socket read", "probe", (3, 5), "client"),
    ("client tty write", "probe", 4, "client"),
]

ZZ_MAC_CHAIN = [
    ("client key read", "probe", 1, "client"),
    ("client socket write", "probe", 2, "client"),
    ("loop wake", "probe", 10, "server"),
    ("loop socket read", "probe", 11, "server"),
    ("loop dispatch key", "probe", 12, "server"),
    ("loop key queued + wake", "probe", 13, "server"),
    ("shard wake (key)", "probe", (20, 29), "server"),
    ("shard key command", "probe", 21, "server"),
    ("shard PTY write", "probe", 22, "server"),
    ("pane read", "sct", (1,), "pane"),
    ("pane write", "sct", (2,), "pane"),
    ("shard wake (data)", "probe", (20, 29), "server"),
    ("shard PTY read", "probe", 31, "server"),
    ("shard parse done", "probe", 24, "server"),
    ("frame encode start", "probe", 25, "server"),
    ("frame encoded", "probe", 26, "server"),
    ("socket write (direct)", "probe", (27, 14), "server"),
    ("publish returned", "probe", 28, "server"),
    ("client socket read", "probe", (3, 5), "client"),
    ("client tty write", "probe", 4, "client"),
]

TMUX_CHAIN = [
    ("server wake (key)", "sct", (7, 8, 9), "server"),
    ("server tty read", "sct", (1, 3), "server"),
    ("server PTY write", "sct", (2, 4), "server"),
    ("pane read", "sct", (1,), "pane"),
    ("pane write", "sct", (2,), "pane"),
    ("server wake (data)", "sct", (7, 8, 9), "server"),
    ("server PTY read", "sct", (1, 3), "server"),
    ("server tty write", "sct", (2, 4), "server"),
]


def load_probe(directory):
    events = []
    for path in glob.glob(os.path.join(directory, "probe-*.txt")):
        for line in open(path):
            pid, tid, stage, ns = map(int, line.split())
            events.append((ns, "probe", stage, pid, tid, 0, 0))
    return events


def load_sct(directory):
    events = []
    for path in glob.glob(os.path.join(directory, "sct-*.txt")):
        pid = int(os.path.basename(path)[4:-4])
        for line in open(path):
            tid, call, fd, ret, t0, t1 = map(int, line.split())
            events.append((t1, "sct", call, pid, tid, fd, ret))
    return events


def role(pid, pids):
    for name, members in pids.items():
        if pid in members:
            return name
    return "other"


def walk(chain, events, times, pids, key, pane_pid, until):
    t = key["t0"]
    stages = []
    for name, kind, codes, who in chain:
        codes = codes if isinstance(codes, tuple) else (codes,)
        hit = None
        for ev in events[bisect.bisect_left(times, t):]:
            ns, ekind, code, pid, tid, fd, ret = ev
            if ns > until:
                break
            if ekind != kind or code not in codes:
                continue
            if who == "pane" and pid != pane_pid:
                continue
            if who != "pane" and role(pid, pids) != who:
                continue
            if kind == "sct" and code in (1, 3) and ret <= 0:
                continue
            if kind == "sct" and code in (7, 8, 9) and ret <= 0:
                continue
            hit = ev
            break
        if hit is None:
            stages.append((name, None, None))
            continue
        t = hit[0]
        stages.append((name, t - key["t0"], hit[4]))
    return stages


def summarize(chain, rows):
    out = []
    for index, (name, *_rest) in enumerate(chain):
        values = [row[index][1] for row in rows if row[index][1] is not None]
        if not values:
            out.append((name, None, None, None, 0))
            continue
        values.sort()
        out.append((name, statistics.median(values) / 1e3, values[int(len(values) * 0.99) - 1 if len(values) > 1 else 0] / 1e3, None, len(values)))
    return out


def main():
    run_json, directory, mux, variant = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
    data = json.load(open(run_json))
    row = data["muxes"][mux][variant]
    pids = {name: set(members) for name, members in row["pids"].items()}
    events = sorted(load_probe(directory) + load_sct(directory))
    if not pids["server"]:
        pids["server"] = {ev[3] for ev in events if ev[1] == "probe" and 10 <= ev[2] <= 40} - pids["client"]
    times = [ev[0] for ev in events]
    chain = TMUX_CHAIN
    if mux == "zz":
        chain = ZZ_CHAIN if any(ev[1] == "probe" and ev[2] == 30 for ev in events) else ZZ_MAC_CHAIN
    pane_pid = None
    keys = [k for k in row["keys"] if k["seen"]]
    for pane in sorted(pids["pane"]):
        try:
            cmd = open(f"/proc/{pane}/cmdline").read()
        except OSError:
            cmd = ""
        if cmd.endswith(" 0\0") or cmd.endswith("\x000\x00"):
            pane_pid = pane
    if pane_pid is None:
        counts = {}
        k = keys[len(keys) // 2]
        for ev in events:
            if k["t0"] <= ev[0] <= k["seen"] and ev[1] == "sct" and ev[3] in pids["pane"]:
                counts[ev[3]] = counts.get(ev[3], 0) + 1
        pane_pid = max(counts, key=counts.get) if counts else -1
    rows = []
    for k in keys:
        rows.append(walk(chain, events, times, pids, k, pane_pid, k["seen"] + 2_000_000))
    deltas = []
    previous = 0.0
    total = [(k["seen"] - k["t0"]) / 1e3 for k in keys]
    print(f"{mux} {variant}: keys {len(keys)} median total {statistics.median(total):.1f} us, pane pid {pane_pid}")
    print(f"{'stage':32} {'at p50':>8} {'step p50':>9} {'step p99':>9} {'n':>4}")
    prev_index = None
    for index, (name, *_r) in enumerate(chain):
        at = [r[index][1] / 1e3 for r in rows if r[index][1] is not None]
        if prev_index is None:
            steps = at
        else:
            steps = [(r[index][1] - (r[prev_index][1] or 0)) / 1e3 for r in rows if r[index][1] is not None and r[prev_index][1] is not None]
        if not at:
            print(f"{name:32} {'-':>8}")
            continue
        steps.sort()
        p99 = steps[min(len(steps) - 1, int(len(steps) * 0.99))]
        print(f"{name:32} {statistics.median(at):8.1f} {statistics.median(steps):9.1f} {p99:9.1f} {len(at):4}")
        deltas.append({"stage": name, "at_p50_us": statistics.median(at), "step_p50_us": statistics.median(steps), "step_p99_us": p99, "n": len(at)})
        prev_index = index
    tail = sorted(total[i] - (rows[i][prev_index][1] or 0) / 1e3 for i in range(len(rows)) if rows[i][prev_index][1] is not None)
    if tail:
        print(f"{'bench sees echo':32} {statistics.median(total):8.1f} {statistics.median(tail):9.1f} {tail[min(len(tail) - 1, int(len(tail) * 0.99))]:9.1f} {len(tail):4}")
    tids = {}
    for r in rows[len(rows) // 2: len(rows) // 2 + 1]:
        for name, at, tid in r:
            tids[name] = tid
    print("threads on one key:", json.dumps(tids))
    if len(sys.argv) > 5:
        json.dump({"mux": mux, "variant": variant, "stages": deltas, "total_p50_us": statistics.median(total)}, open(sys.argv[5], "w"), indent=1)


if __name__ == "__main__":
    main()
