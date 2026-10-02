import json
import os
import re
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))

import isolate
from groups.chatty import FLIP
from ptyclient import PtyClient

IDLE = re.compile(r"kevent|__psynch_cvwait|__select|poll|mach_msg|__workq_kernreturn|__semwait|_pthread_start|thread_start|start_wqthread|__ulock_wait|epoll_wait")


def thread_cpu(pid):
    out = subprocess.run(["ps", "-M", "-p", str(pid)], capture_output=True, text=True).stdout.splitlines()[1:]
    times = []
    for line in out:
        stamps = [t for t in line.split() if re.match(r"^\d+:\d+\.\d+$", t)][:2]
        if len(stamps) == 2:
            times.append(sum(int(t.split(":")[0]) * 60 + float(t.split(":")[1]) for t in stamps))
    return times


def top_of_stack(pid, seconds):
    out = subprocess.run(["sample", str(pid), str(seconds), "-mayDie"], capture_output=True, text=True).stdout
    threads = {}
    current = None
    for line in out.splitlines():
        m = re.match(r"^\s{4}(\d+) Thread_\d+(.*)$", line)
        if m:
            rest = m.group(2).strip()
            current = rest.split(":", 1)[1].strip() if ":" in rest else rest or "<unnamed>"
            current = re.sub(r"\d+$", "N", current)
            threads.setdefault(current, {"samples": 0, "busy": 0})
            threads[current]["samples"] += int(m.group(1))
            stack = []
            continue
        if current is None:
            continue
        if line.startswith("Total number in stack") or line.startswith("Sort by top of stack"):
            current = None
            continue
        m = re.match(r"^(\s*)([+!:| ]*)(\d+) (.+)$", line)
        if not m:
            continue
        depth = len(m.group(1)) + len(m.group(2))
        count = int(m.group(3))
        name = m.group(4).split("  (in ")[0].strip()
        while stack and stack[-1][0] >= depth:
            stack.pop()
        if stack:
            stack[-1][2] -= count
        short = re.sub(r"::h[0-9a-f]{16}$", "", name)
        if all(short != ancestor[3] for ancestor in stack):
            inclusive = threads[current].setdefault("_inclusive", {})
            inclusive[short] = inclusive.get(short, 0) + count
        node = [depth, name, count, short]
        stack.append(node)
        threads[current].setdefault("_nodes", []).append(node)
    leaves = {}
    for name, info in threads.items():
        self_counts = {}
        inclusive = info.pop("_inclusive", {})
        info["inclusive_top"] = sorted(((fn, c) for fn, c in inclusive.items() if not IDLE.search(fn)), key=lambda kv: -kv[1])[:60]
        for depth, fn, own, _ in info.pop("_nodes", []):
            if own > 0:
                self_counts[fn] = self_counts.get(fn, 0) + own
        busy = {fn: c for fn, c in self_counts.items() if not IDLE.search(fn)}
        info["busy"] = sum(busy.values())
        leaves[name] = sorted(busy.items(), key=lambda kv: -kv[1])[:15]
    return threads, leaves


def setup_visible(zz):
    zz.run("new-session", "-d", "-s", "po", "-x", "180", "-y", "50", check=True)
    for _ in range(3):
        zz.run("split-window", "-d", "-t", "po:0", check=True)
        zz.run("select-layout", "-t", "po:0", "tiled", check=True)
    time.sleep(1.0)
    for p in range(4):
        zz.run("send-keys", "-t", f"po:0.{p}", FLIP, "Enter", check=True)


def setup_flip(zz):
    zz.run("new-session", "-d", "-s", "po", "-x", "120", "-y", "30", check=True)
    for _ in range(10):
        zz.run("new-window", "-d", "-t", "po:", check=True)
    time.sleep(1.0)
    for w in range(1, 11):
        zz.run("send-keys", "-t", f"po:{w}", FLIP, "Enter", check=True)


def main():
    zz_bin, out_path = sys.argv[1], sys.argv[2]
    results = {}
    for name, setup, attached in (("visible", setup_visible, True), ("flip", setup_flip, False)):
        env = isolate.Env()
        zz = isolate.Zz(zz_bin, env)
        try:
            setup(zz)
            pid = zz.pid()
            client = PtyClient(zz.attach_argv("po:0"), zz.env, env) if attached else None
            if client:
                client.drain(2.0)
            else:
                time.sleep(2.0)
            s0 = zz.sample()
            c0 = thread_cpu(pid)
            t0 = time.perf_counter()
            deadline = t0 + 5.0
            while time.perf_counter() < deadline:
                if client:
                    client.drain(0.05)
                else:
                    time.sleep(0.05)
            elapsed = time.perf_counter() - t0
            c1 = thread_cpu(pid)
            s1 = zz.sample()
            per_thread = [round((b - a) / elapsed * 100, 3) for a, b in zip(c0, c1)]
            threads, leaves = top_of_stack(pid, 6)
            results[name] = {
                "cpu_pct": (s1.cpu_ns - s0.cpu_ns) / 1e9 / elapsed * 100,
                "minstr_per_s": (s1.instructions - s0.instructions) / 1e6 / elapsed,
                "wakeups_per_s": (s1.wakeups - s0.wakeups) / elapsed,
                "thread_cpu_pct_ps_order": per_thread,
                "sample_threads": threads,
                "sample_busy_leaves": leaves,
            }
        finally:
            results.setdefault(name, {})["strays"] = env.cleanup()
    with open(out_path, "w") as f:
        json.dump(results, f, indent=1)
    print(json.dumps(results, indent=1))


if __name__ == "__main__":
    main()
