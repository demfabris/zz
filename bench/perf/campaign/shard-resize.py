import json
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))

import isolate
from groups.echo import LETTERS, keystroke
from ptyclient import PtyClient

ECHO = os.path.join(os.path.dirname(HERE), "pane", "echo.py")
HISTORY = "python3 -c \"import sys; sys.stdout.write(''.join(f'{i:06d} ' + 'x' * 90 + chr(10) for i in range(20000)))\"; exec bash --norc"
VARIANTS = {"shell": "exec bash --norc", "scrollback": HISTORY}


def proc_status(pid):
    fields = {}
    with open(f"/proc/{pid}/status") as f:
        for line in f:
            key, _, value = line.partition(":")
            if key in ("VmRSS", "VmHWM"):
                fields[key] = int(value.split()[0]) * 1024
    return fields


def cpu_ns(path):
    with open(path) as f:
        parts = f.read().rsplit(")", 1)[1].split()
    return (int(parts[11]) + int(parts[12])) * 1_000_000_000 // os.sysconf("SC_CLK_TCK")


def shard_threads(pid):
    found = {}
    for tid in os.listdir(f"/proc/{pid}/task"):
        try:
            with open(f"/proc/{pid}/task/{tid}/comm") as f:
                name = f.read().strip()
        except OSError:
            continue
        if name.startswith("zz-pty-shard"):
            found[tid] = name
    return found


def shard_cpu(pid, tids):
    total = 0
    for tid in tids:
        try:
            total += cpu_ns(f"/proc/{pid}/task/{tid}/stat")
        except OSError:
            pass
    return total


def measure(binary, kind, variant, size, settle):
    env = isolate.Env()
    mux = isolate.Zz(binary, env) if kind == "zz" else isolate.Tmux(binary, env)
    mux.env["ZZ_PTY_SHARDS"] = "1"
    sent = 0
    try:
        mux.run("new-session", "-d", "-s", "nb", "-x", "120", "-y", "40", f"exec python3 {ECHO} 0", check=True)
        mux.run("set-option", "-g", "status", "off", check=True)
        mux.run("new-session", "-d", "-s", "big", "-x", "120", "-y", "40", VARIANTS[variant], check=True)
        big = PtyClient(mux.attach_argv("big"), mux.env, env, cols=120, rows=40)
        near = PtyClient(mux.attach_argv("nb"), mux.env, env, cols=120, rows=40)
        deadline = time.perf_counter() + settle
        while time.perf_counter() < deadline:
            big.drain(0.05)
            near.drain(0.05)
        base = []
        for _ in range(30):
            sent += 1
            ms = keystroke(near, LETTERS[sent % len(LETTERS)], sent, timeout=2.0)
            if ms is not None:
                base.append(ms)
            big.drain(0.005)
        pid = mux.pid()
        tids = shard_threads(pid) if kind == "zz" else {}
        with open(f"/proc/{pid}/clear_refs", "w") as f:
            f.write("5")
        before = proc_status(pid)
        cpu0 = cpu_ns(f"/proc/{pid}/stat")
        shard0 = shard_cpu(pid, tids)
        recorder = None
        if os.environ.get("PERF_OUT"):
            recorder = subprocess.Popen(
                ["perf", "record", "-q", "-F", "999", "--call-graph", "dwarf,16384", "-p", str(pid), "-o", os.environ["PERF_OUT"]],
                stdin=subprocess.DEVNULL,
            )
            time.sleep(1.0)
        t0 = time.perf_counter()
        proc = subprocess.Popen(
            mux.argv("resize-window", "-t", "big", "-x", str(size), "-y", str(size)),
            env=mux.env,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
        )
        returned = None
        samples = []
        quiet = 0
        while time.perf_counter() - t0 < 180:
            sent += 1
            ts = time.perf_counter() - t0
            ms = keystroke(near, LETTERS[sent % len(LETTERS)], sent, timeout=120.0)
            samples.append((ts * 1000, ms))
            big.drain(0.0)
            if returned is None and proc.poll() is not None:
                returned = (time.perf_counter() - t0) * 1000
            quiet = quiet + 1 if ms is not None and ms < 20 else 0
            if returned is not None and quiet >= 40 and time.perf_counter() - t0 > returned / 1000 + 0.5:
                break
            time.sleep(0.002)
        err = proc.stderr.read().decode().strip()
        if recorder:
            recorder.send_signal(2)
            recorder.wait()
        cpu1 = cpu_ns(f"/proc/{pid}/stat")
        shard1 = shard_cpu(pid, tids)
        after = proc_status(pid)
        typed = []
        t1 = time.perf_counter()
        pending = None
        keys = 0
        while time.perf_counter() - t1 < 3.0:
            if pending is None or pending.poll() is not None:
                pending = subprocess.Popen(
                    mux.argv("send-keys", "-t", "big", "x"),
                    env=mux.env,
                    stdin=subprocess.DEVNULL,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                )
                keys += 1
            sent += 1
            ms = keystroke(near, LETTERS[sent % len(LETTERS)], sent, timeout=120.0)
            typed.append(ms)
            big.drain(0.0)
            time.sleep(0.002)
        if pending:
            pending.wait()
        shard2 = shard_cpu(pid, tids)
        cpu2 = cpu_ns(f"/proc/{pid}/stat")
        geometry = mux.out("display-message", "-p", "-t", "big", "#{pane_width}x#{pane_height}")
        slow = [(ts, ms) for ts, ms in samples if ms is None or ms >= 20]
        stall_end = max((ts + (ms or 120000) for ts, ms in slow), default=0.0)
        lat = sorted(ms for _, ms in samples if ms is not None)
        row = {
            "mux": kind,
            "variant": variant,
            "size": size,
            "geometry": geometry,
            "resize_cmd_ms": returned,
            "neighbour_max_echo_ms": max(lat) if lat else None,
            "neighbour_p50_echo_ms": lat[len(lat) // 2] if lat else None,
            "neighbour_missed": sum(1 for _, ms in samples if ms is None),
            "busy_until_ms": stall_end,
            "baseline_echo_p50_ms": sorted(base)[len(base) // 2] if base else None,
            "typing_neighbour_max_echo_ms": max((ms for ms in typed if ms is not None), default=None),
            "typing_neighbour_missed": sum(1 for ms in typed if ms is None),
            "typing_keys": keys,
            "typing_server_cpu_ms": (cpu2 - cpu1) / 1e6,
            "typing_shard_cpu_ms": (shard2 - shard1) / 1e6 if tids else None,
            "server_cpu_ms": (cpu1 - cpu0) / 1e6,
            "shard_cpu_ms": (shard1 - shard0) / 1e6 if tids else None,
            "rss_before_mb": before["VmRSS"] / 2**20,
            "peak_rss_mb": after["VmHWM"] / 2**20,
            "rss_after_mb": after["VmRSS"] / 2**20,
            "error": err or None,
        }
        big.kill()
        near.kill()
        return row
    finally:
        env.cleanup()


def main():
    zz_bin, tmux_bin, out_path = sys.argv[1], sys.argv[2], sys.argv[3]
    sizes = [int(s) for s in (sys.argv[4] if len(sys.argv) > 4 else "2000,5000,10000").split(",")]
    variants = (sys.argv[5] if len(sys.argv) > 5 else "shell,scrollback").split(",")
    muxes = (sys.argv[6] if len(sys.argv) > 6 else "zz,tmux").split(",")
    rows = []
    for variant in variants:
        for size in sizes:
            for kind in muxes:
                binary = zz_bin if kind == "zz" else tmux_bin
                row = measure(binary, kind, variant, size, 2.5 if variant == "scrollback" else 1.0)
                print(json.dumps(row), flush=True)
                rows.append(row)
    with open(out_path, "w") as f:
        json.dump(rows, f, indent=1)


if __name__ == "__main__":
    main()
