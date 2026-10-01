import fnmatch
import glob
import json
import os
import signal
import subprocess
import sys
import time

worktree, brief, out, start, budget_min, idle_min, base, zone_file = sys.argv[1:9]
worktree = os.path.realpath(worktree)
start = int(start)
budget = int(budget_min) * 60
idle = int(idle_min) * 60
zone = ["knowledge/designs/daemon-perf-rebuild.md"]
if zone_file != "-":
    zone += [line.strip() for line in open(zone_file) if line.strip()]
sessions = os.path.expanduser("~/.codex/sessions")


def git(*args):
    return subprocess.run(["git", "-C", worktree, *args], capture_output=True, text=True).stdout


def processes():
    rows = subprocess.run(["ps", "-Ao", "pid=,ppid=,command="], capture_output=True, text=True).stdout
    table = []
    for row in rows.splitlines():
        parts = row.split(None, 2)
        if len(parts) == 3:
            table.append((int(parts[0]), int(parts[1]), parts[2]))
    return table


def cwds():
    found = {}
    if os.path.isdir("/proc/self"):
        for entry in os.listdir("/proc"):
            if entry.isdigit():
                try:
                    found[int(entry)] = os.path.realpath(os.readlink(f"/proc/{entry}/cwd"))
                except OSError:
                    pass
        return found
    pid = None
    for line in subprocess.run(["lsof", "-a", "-d", "cwd", "-Fpn"], capture_output=True, text=True).stdout.splitlines():
        if line.startswith("p"):
            pid = int(line[1:])
        elif line.startswith("n") and pid is not None:
            found[pid] = os.path.realpath(line[1:])
    return found


def kill(reason):
    with open(out + ".watchdog", "w") as handle:
        handle.write(f"{time.strftime('%Y-%m-%d %H:%M')} {reason}\n")
    if os.environ.get("LANE_WATCHDOG_DRYRUN"):
        print(reason)
        sys.exit(0)
    table = processes()
    me = os.getpid()
    roots = {pid for pid, _, command in table if "codex" in command and brief in command}
    targets = set(roots)
    changed = True
    while changed:
        changed = False
        for pid, ppid, _ in table:
            if ppid in targets and pid not in targets:
                targets.add(pid)
                changed = True
    for pid, cwd in cwds().items():
        if cwd == worktree or cwd.startswith(worktree + os.sep):
            targets.add(pid)
    targets.discard(me)
    targets.discard(os.getppid())
    for sig in (signal.SIGTERM, signal.SIGKILL):
        for pid in targets:
            try:
                os.kill(pid, sig)
            except OSError:
                pass
        time.sleep(5)
    with open(out + ".watchdog", "a") as handle:
        handle.write(f"killed {len(targets)} processes\n")
    sys.exit(0)


def codex_running():
    return any("codex" in command and brief in command for _, _, command in processes())


def helpers():
    count = 0
    for path in glob.glob(os.path.join(sessions, "*", "*", "*", "*.jsonl")):
        if os.path.getmtime(path) < start:
            continue
        try:
            meta = json.loads(open(path).readline()).get("payload", {})
        except (OSError, ValueError):
            continue
        if os.path.realpath(meta.get("cwd", "")) == worktree and "subagent" in json.dumps(meta.get("source")):
            count += 1
    return count


while True:
    time.sleep(int(os.environ.get("LANE_WATCHDOG_INTERVAL", "300")))
    if not codex_running() and not os.environ.get("LANE_WATCHDOG_DRYRUN"):
        sys.exit(0)
    now = time.time()
    if now - start > budget:
        kill(f"wall budget {budget_min} min spent")
    last = max(start, int(git("log", "-1", "--format=%ct").strip() or 0))
    if now - last > idle:
        kill(f"no new commit for {idle_min} min")
    if helpers():
        kill("a helper agent runs in the worktree")
    changed = set(git("diff", "--name-only", f"{base}..HEAD").split())
    status = git("status", "--porcelain", "--untracked-files=all").splitlines()
    untracked = [line[3:] for line in status if line.startswith("??")]
    changed |= {line[3:] for line in status}
    outside = sorted(path for path in changed if not any(fnmatch.fnmatch(path, glob_) for glob_ in zone))
    if outside:
        kill("outside the write zone: " + ", ".join(outside[:5]))
    added = git("diff", "--name-only", "--diff-filter=A", f"{base}..HEAD").split() + untracked
    if any(path.startswith("third_party/") or "/vendor/" in path for path in added):
        kill("a new file under third_party/ or a vendored crate")
    size = sum(os.path.getsize(os.path.join(worktree, path)) for path in untracked if os.path.isfile(os.path.join(worktree, path)))
    if len(untracked) > 50 or size > 100 * 1024 * 1024:
        kill(f"{len(untracked)} untracked files, {size // (1024 * 1024)} MB")
