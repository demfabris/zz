#!/usr/bin/env python3
"""Re-measure a verified TUI obligation instead of trusting what a lane claimed.

    python3 compat/tui/verify-claims.py                # structural checks only
    python3 compat/tui/verify-claims.py --run TUI-009  # also run that id's fixtures

`tracker.py check` validates the shape of a record. It cannot tell whether the
fixture behind a `verified` claim still holds recorded cases, and twice a lane
reported a clause proved while its own fixture recorded cases against it. Cycle
8's reviewer caught one by re-running the fixture by hand; this does it
mechanically at every close-out.

With --run, each named obligation's fixtures are executed and their own tally
line is parsed. A non-zero recorded count under a `verified` obligation is a
defect in the ledger, not in the fixture.
"""
import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LEDGER = ROOT / "compat/tui/campaign.json"

# Which fixture actually carries each obligation's cases. A `verified` id whose
# fixture is not listed here cannot be re-measured, which is itself worth saying.
FIXTURES = {
    "TUI-001": ["compat/tui-pane-geometry.sh", "compat/status-row.sh"],
    "TUI-002": ["compat/tui-screen-diff.sh"],
    "TUI-003": ["compat/tui-stock-keys.sh"],
    "TUI-004": ["compat/tui-indicators.sh", "compat/tui-screen-diff.sh"],
    "TUI-005": ["compat/tui-copy-mode.sh"],
    "TUI-006": ["compat/tui-choosers.sh"],
    "TUI-007": ["compat/tui-overlays.sh"],
    "TUI-008": ["compat/tui-mouse.sh"],
    "TUI-009": ["compat/tui-caps.sh"],
    "TUI-010": ["compat/tui-output-backpressure.sh"],
    "TUI-011": ["compat/tui-client-commands.sh"],
    "TUI-012": ["compat/tui-superset.sh"],
    "TUI-014": ["compat/tui-client-commands.sh", "compat/tui-choosers.sh"],
    "TUI-018": ["compat/tui-command-streams.sh", "compat/tui-client-commands.sh"],
}

TALLY = re.compile(r"(\d+)\s+recorded not asserted|(\d+)\s+assert none and are recorded in full")


def git(*args):
    return subprocess.run(["git", "-C", str(ROOT), *args],
                          capture_output=True, text=True).stdout.strip()


def structural(items, problems):
    by_id = {i["id"]: i for i in items}
    head = git("rev-parse", "HEAD")
    for item in items:
        if item["status"] != "verified":
            continue
        pid = item["id"]
        proof = item.get("proof")
        if not proof:
            problems.append(f"{pid}: verified with no proof block")
            continue
        rev = proof.get("revision", "")
        anc = subprocess.run(["git", "-C", str(ROOT), "merge-base", "--is-ancestor", rev, head])
        if anc.returncode != 0:
            problems.append(f"{pid}: proof.revision {rev[:12]} is not an ancestor of HEAD, "
                            f"so the proof was never on this history")
        for dep in item.get("depends_on", []):
            if by_id.get(dep, {}).get("status") != "verified":
                problems.append(f"{pid}: verified but its dependency {dep} is "
                                f"{by_id.get(dep, {}).get('status', 'missing')}")
        for art in proof.get("artifacts", []):
            if not (ROOT / art).exists():
                problems.append(f"{pid}: proof artifact missing on disk: {art}")
        if pid not in FIXTURES:
            problems.append(f"{pid}: verified but no fixture is mapped for it here, "
                            f"so its claim cannot be re-measured; add it to FIXTURES")


def run_fixtures(ids, items, problems, zz=None):
    by_id = {i["id"]: i for i in items}
    env = dict(os.environ)
    env.setdefault("ZZ_COMPAT_TMUX", str(ROOT / "compat/.cache/tmux-src/tmux"))
    env.setdefault("ZZ_COMPAT_CORPUS", str(ROOT / "compat/.cache/plugins"))
    env.setdefault("TMUX_BIN", str(ROOT / "compat/.cache/tmux-src/tmux"))
    binary = Path(zz) if zz else ROOT / "target/debug/zz"
    if not binary.exists():
        problems.append(f"no zz binary to compare: {binary} does not exist. The fixtures default "
                        f"to REPO/target/debug/zz and ignore ZZ_COMPAT_ZZ; pass --zz with a build "
                        f"of the revision under test.")
        return
    env["ZZ_BIN"] = str(binary)
    print(f"  comparing {binary}")
    for pid in ids:
        item = by_id.get(pid)
        if item is None:
            problems.append(f"{pid}: not in the ledger")
            continue
        for rel in FIXTURES.get(pid, []):
            path = ROOT / rel
            if not path.exists():
                problems.append(f"{pid}: fixture {rel} does not exist")
                continue
            print(f"  running {rel} for {pid} ...", flush=True)
            r = subprocess.run(["bash", str(path)], capture_output=True, text=True,
                               cwd=str(ROOT), env=env, timeout=1800)
            tail = (r.stdout or "").strip().splitlines()
            last = tail[-1] if tail else "(no output)"
            if r.returncode != 0:
                err = (r.stderr or "").strip().replace("\n", " ")[:200]
                problems.append(f"{pid}: {rel} exited {r.returncode}: {last[:120]} {err}")
                continue
            recorded = 0
            for m in TALLY.finditer(r.stdout or ""):
                recorded += int(m.group(1) or m.group(2) or 0)
            state = item["status"]
            print(f"    {last[:150]}")
            if recorded and state == "verified":
                problems.append(f"{pid} is VERIFIED but {rel} still reports {recorded} "
                                f"recorded case(s): {last[:150]}")
            elif recorded:
                print(f"    ({state}, {recorded} recorded, which is consistent)")


def main(argv):
    ap = argparse.ArgumentParser()
    ap.add_argument("--run", nargs="*", metavar="ID",
                    help="also execute these obligations' fixtures and read their tally")
    ap.add_argument("--zz", metavar="PATH",
                    help="the zz binary to compare (the fixtures default to REPO/target/debug/zz "
                         "and ignore ZZ_COMPAT_ZZ, so an orchestrator worktree with no target "
                         "directory must pass this)")
    args = ap.parse_args(argv[1:])
    data = json.loads(LEDGER.read_text(encoding="utf-8"))
    items = data["items"]
    problems = []
    verified = [i["id"] for i in items if i["status"] == "verified"]
    print(f"verified: {', '.join(verified) or 'none'}")
    structural(items, problems)
    if args.run is not None:
        targets = args.run or verified
        print(f"re-measuring: {', '.join(targets)}")
        run_fixtures(targets, items, problems, args.zz)
    print()
    if problems:
        print(f"{len(problems)} problem(s):")
        for p in problems:
            print(f"  {p}")
        return 1
    print("every verified obligation holds up")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
