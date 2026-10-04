import ctypes
import json
import os
import signal
import statistics
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
sys.path.insert(0, HERE)

import isolate
import probe
from groups.control import Control

echomap = __import__("w4-echomap")


class Ctx:
    pass


def open_counter(tid, config):
    attr = probe._PerfEventAttr(
        type=0,
        size=ctypes.sizeof(probe._PerfEventAttr),
        config=config,
        flags=probe._PERF_EXCLUDE_KERNEL | probe._PERF_EXCLUDE_HV,
    )
    fd = probe._libc.syscall(ctypes.c_long(probe._PERF_EVENT_OPEN), ctypes.byref(attr), ctypes.c_int(tid), ctypes.c_int(-1), ctypes.c_int(-1), ctypes.c_ulong(probe._PERF_FLAG_FD_CLOEXEC))
    if fd < 0:
        raise OSError(ctypes.get_errno(), "perf_event_open")
    return fd


def counters(sets):
    fds = {}
    if not hasattr(probe, "_PerfEventAttr"):
        return fds
    for g in sets.values():
        for pids in g.values():
            for pid in pids:
                for tid in os.listdir(f"/proc/{pid}/task"):
                    try:
                        fds[tid] = (open_counter(int(tid), 1), open_counter(int(tid), 0))
                    except OSError:
                        pass
    return fds


def read_counters(fds):
    return {tid: tuple(int.from_bytes(os.read(fd, 8), sys.byteorder) for fd in pair) for tid, pair in fds.items()}


def groups(mux, ctl):
    server = sorted(set(mux.server_pids()) | {mux.pid()})
    return {"server": server, "client": [ctl.proc.pid]}


def main():
    zz_bin, tmux_bin, out_path = sys.argv[1], sys.argv[2], sys.argv[3]
    runs = int(os.environ.get("RUNS", "400"))
    gap = float(os.environ.get("GAP", "0"))
    idle = float(os.environ.get("IDLE", "2"))
    only = os.environ.get("ONLY", "zz,tmux").split(",")
    env = isolate.Env()
    ctx = Ctx()
    ctx.env = env
    muxes = []
    if "zz" in only:
        muxes.append(isolate.Zz(zz_bin, env))
    if "tmux" in only:
        muxes.append(isolate.Tmux(tmux_bin, env))
    result = {"runs": runs, "gap": gap, "muxes": {}}
    try:
        for mux in muxes:
            mux.run("new-session", "-d", "-s", "c", "-x", "180", "-y", "50", check=True)
            mux.run("set-option", "-g", "default-size", "180x50")
        time.sleep(0.5)
        ctls = {m.name: Control(ctx, m, "c") for m in muxes}
        for c in ctls.values():
            c.settle()
        sets = {m.name: groups(m, ctls[m.name]) for m in muxes}
        idle0 = {n: echomap.snapshot(g) for n, g in sets.items()}
        time.sleep(idle)
        idle1 = {n: echomap.snapshot(g) for n, g in sets.items()}
        lat = {m.name: [] for m in muxes}
        recorder = None
        if os.environ.get("PERF_OUT") and "zz" in sets:
            recorder = subprocess.Popen(
                ["perf", "record", "-q", *os.environ.get("PERF_ARGS", "-F 25000 --call-graph dwarf,16384").split(), "-o", os.environ["PERF_OUT"], "-p", ",".join(str(p) for p in sets["zz"]["server"])],
                stdout=subprocess.DEVNULL,
            )
            time.sleep(1.5)
        fds = counters(sets)
        instr0 = read_counters(fds)
        t0 = time.perf_counter()
        for i in range(runs):
            for mux in (muxes if i % 2 == 0 else muxes[::-1]):
                c = ctls[mux.name]
                target = c.ends + 1
                start = time.perf_counter()
                c.send("display-message -p x\n")
                if c.wait_ends(target, 5):
                    lat[mux.name].append((time.perf_counter() - start) * 1e6)
                if gap:
                    time.sleep(gap)
        busy_s = time.perf_counter() - t0
        instr1 = read_counters(fds)
        if recorder:
            recorder.send_signal(signal.SIGINT)
            recorder.wait(timeout=120)
        after = {n: echomap.snapshot(g) for n, g in sets.items()}
        for mux in muxes:
            n = mux.name
            scale = busy_s / idle
            rows = []
            for name, procs in after[n].items():
                for pid, tids in procs.items():
                    for tid, now in tids.items():
                        mid = idle1[n].get(name, {}).get(pid, {}).get(tid)
                        if mid is None:
                            continue
                        first = idle0[n].get(name, {}).get(pid, {}).get(tid, mid)
                        row = {"proc": name, "thread": now["comm"], "tid": tid}
                        for field in ("vol", "invol", "syscr", "syscw", "run_ns", "slices"):
                            busy = now[field] - mid[field]
                            row[field] = (busy - (mid[field] - first[field]) * scale) / runs
                            row[field + "_raw"] = busy / runs
                        row["kinstr"] = (instr1[tid][0] - instr0[tid][0]) / runs / 1000 if tid in instr0 else 0
                        row["kcycles"] = (instr1[tid][1] - instr0[tid][1]) / runs / 1000 if tid in instr0 else 0
                        rows.append(row)
            values = sorted(lat[n])
            result["muxes"][n] = {
                "p50_us": statistics.median(values) if values else None,
                "mean_us": statistics.fmean(values) if values else None,
                "rows": rows,
            }
        for c in ctls.values():
            c.close()
        for mux in muxes:
            mux.kill()
    finally:
        result["strays"] = env.cleanup()
    with open(out_path, "w") as f:
        json.dump(result, f, indent=1)
    for name, data in result["muxes"].items():
        print(f"{name} p50 {data['p50_us']:.2f} us mean {data['mean_us']:.2f} us")
        for row in sorted(data["rows"], key=lambda r: (r["proc"], r["thread"])):
            if max(abs(row["vol"]), abs(row["syscr"]), abs(row["syscw"]), abs(row["run_ns"]) / 1000) < 0.02:
                continue
            print(
                f"  {row['proc']:6} {row['thread'][:16]:16} vol {row['vol']:6.2f} invol {row['invol']:5.2f} "
                f"syscr {row['syscr']:5.2f} syscw {row['syscw']:5.2f} run_us {row['run_ns'] / 1000:6.2f} slices {row['slices']:5.2f} kinstr {row['kinstr']:7.2f} kcyc {row['kcycles']:7.2f}"
            )


if __name__ == "__main__":
    main()
