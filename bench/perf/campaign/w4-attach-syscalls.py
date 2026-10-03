import ctypes
import json
import os
import statistics
import struct
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
sys.path.insert(0, os.path.join(os.path.dirname(HERE), "groups"))

import isolate
import probe
from attach import MARK_A, MARK_B, QUIET, content, setup
from ptyclient import PtyClient


def task(pid):
    ti = probe._TaskInfo()
    got = probe._libproc.proc_pidinfo(int(pid), probe._PROC_PIDTASKINFO, ctypes.c_uint64(0), ctypes.byref(ti), ctypes.sizeof(ti))
    if got != ctypes.sizeof(ti):
        raise ProcessLookupError(pid)
    s = probe.sample(pid)
    return {
        "unix": ti.syscalls_unix,
        "mach": ti.syscalls_mach,
        "csw": ti.csw,
        "user_ms": probe._ticks(ti.total_user) / 1e6,
        "system_ms": probe._ticks(ti.total_system) / 1e6,
        "cpu_ms": s.cpu_ns / 1e6,
        "minstr": s.instructions / 1e6,
        "wakeups": s.wakeups,
    }


def counts(directory, pid):
    if not directory:
        return {}
    path = os.path.join(directory, f"{pid}.bin")
    if not os.path.exists(path):
        return {}
    with open(path, "rb") as f:
        data = f.read()
    magic, nf, slots, used = struct.unpack_from("<4Q", data, 0)
    if magic != 0x5A5A5343:
        return {}
    off = 32
    names = [data[off + i * 24 : off + (i + 1) * 24].split(b"\0")[0].decode() for i in range(nf)]
    off += nf * 24
    size = 48 + nf * 8
    found = {}
    for s in range(used):
        base = off + s * size
        thread = data[base : base + 48].split(b"\0")[0].decode(errors="replace")
        values = struct.unpack_from(f"<{nf}Q", data, base + 48)
        for name, value in zip(names, values):
            if value:
                found[f"{thread}/{name}"] = found.get(f"{thread}/{name}", 0) + value
    return found


def delta(a, b):
    keys = set(a) | set(b)
    return {k: b.get(k, 0) - a.get(k, 0) for k in keys if b.get(k, 0) - a.get(k, 0)}


def median_map(rows):
    keys = set()
    for row in rows:
        keys |= set(row)
    return {k: statistics.median([row.get(k, 0) for row in rows]) for k in sorted(keys)}


def by_call(rows):
    merged = []
    for row in rows:
        m = {}
        for k, v in row.items():
            call = k.split("/", 1)[1]
            m[call] = m.get(call, 0) + v
        merged.append(m)
    return median_map(merged)


def main():
    zz_arg, tmux_bin, out_path = sys.argv[1:4]
    runs = int(sys.argv[4]) if len(sys.argv) > 4 else 10
    dylib = os.environ.get("SYSCOUNT_DYLIB")
    bins = [part.split("=", 1) if "=" in part else ("zz", part) for part in zz_arg.split(",")]
    envs = [isolate.Env() for _ in bins]
    for index, zenv in enumerate(envs):
        zenv.zz_socket = f"/tmp/{zenv.tag}-{index}.sock"
        zenv.env["ZZ_SOCKET"] = zenv.zz_socket
    zzs = []
    for (label, path), zenv in zip(bins, envs):
        mux = isolate.Zz(path, zenv)
        mux.name = label
        zzs.append(mux)
    env = envs[0]
    tmux = isolate.Tmux(tmux_bin, env)
    muxes = [*zzs, tmux]
    labels = [m.name for m in muxes]
    dirs = {}
    for mux in muxes:
        if dylib:
            d = os.path.join(os.environ.get("SYSCOUNT_KEEP") or mux.envobj.root, f"counts-{mux.name}")
            os.makedirs(d, exist_ok=True)
            mux.env["DYLD_INSERT_LIBRARIES"] = dylib
            mux.env["ZZ_SYSCOUNT_DIR"] = d
            for key in ("ZZ_SYSCOUNT_LOG", "ZZ_SYSCOUNT_BT"):
                if os.environ.get(key):
                    mux.env[key] = "1"
            dirs[mux.name] = d
        else:
            dirs[mux.name] = None
    for key, value in os.environ.items():
        if key.startswith("ZZ_TRACE"):
            for mux in zzs:
                mux.env[key] = value
    result = {"zz": dict(bins), "tmux": tmux_bin, "runs": runs, "interposed": bool(dylib)}
    try:
        for mux in muxes:
            setup(type("C", (), {"session": lambda self, m, n, c, r, cmd: m.run("new-session", "-d", "-s", n, "-x", str(c), "-y", str(r), cmd, check=True)})(), mux)
            mux.run("set-option", "-g", "default-size", "180x50")
        time.sleep(1.5)
        idle = {}
        for mux in muxes:
            pid = mux.pid()
            t0, c0 = task(pid), counts(dirs[mux.name], pid)
            time.sleep(2.0)
            t1, c1 = task(pid), counts(dirs[mux.name], pid)
            idle[mux.name] = {"per_s": {k: (t1[k] - t0[k]) / 2.0 for k in t0}, "calls_per_s": {k: v / 2.0 for k, v in sorted(delta(c0, c1).items())}}
        result["idle"] = idle
        for size, target, marker, count in (("p1", "a", MARK_A, 1), ("p4", "b", MARK_B, 4)):
            rows = {m.name: [] for m in muxes}
            calls = {m.name: [] for m in muxes}
            for i in range(runs):
                for mux in muxes[i % len(muxes):] + muxes[: i % len(muxes)]:
                    pid = mux.pid()
                    m0 = time.clock_gettime(time.CLOCK_MONOTONIC)
                    t0, c0 = task(pid), counts(dirs[mux.name], pid)
                    client = PtyClient(mux.attach_argv(target), mux.env, mux.envobj, cols=180, rows=50)
                    seen = client.read_until(content(marker, count), 5.0)
                    client.drain_quiet(QUIET, 3.0)
                    client.detach(mux.detach_keys)
                    time.sleep(0.2)
                    t1, c1 = task(pid), counts(dirs[mux.name], pid)
                    row = {k: t1[k] - t0[k] for k in t0}
                    row["ttfc_ms"] = (seen - client.t0) * 1000 if seen else None
                    row["mono"] = [m0, time.clock_gettime(time.CLOCK_MONOTONIC)]
                    rows[mux.name].append(row)
                    calls[mux.name].append(delta(c0, c1))
                    time.sleep(0.1)
            out = {}
            for mux in muxes:
                r = rows[mux.name]
                out[mux.name] = {
                    "median": {k: statistics.median([x[k] for x in r if x[k] is not None]) for k in r[0] if k != "mono"},
                    "rows": r,
                    "calls_by_thread": median_map(calls[mux.name]),
                    "calls": by_call(calls[mux.name]),
                }
            result[size] = out
    finally:
        for mux in muxes:
            mux.kill()
        for e in envs:
            e.cleanup()
    with open(out_path, "w") as f:
        json.dump(result, f, indent=1)
    for size in ("p1", "p4"):
        for name in labels:
            m = result[size][name]["median"]
            ratio = m["unix"] / max(result[size]["tmux"]["median"]["unix"], 1)
            print(f"{size} {name:5} unix {m['unix']:6.0f} x{ratio:4.2f} mach {m['mach']:4.0f} csw {m['csw']:4.0f} wake {m['wakeups']:4.0f} user {m['user_ms']:5.2f} sys {m['system_ms']:5.2f} cpu {m['cpu_ms']:5.2f} minstr {m['minstr']:6.2f} ttfc {m['ttfc_ms']:6.2f}")


if __name__ == "__main__":
    main()
