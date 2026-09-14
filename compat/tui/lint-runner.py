#!/usr/bin/env python3
"""Check a TUI parity cycle runner against every lesson earlier cycles paid for.

Each rule below cost the campaign real time once. The point of this file is that
the next orchestrator does not have to remember them: run it before launching.

    python3 compat/tui/lint-runner.py compat/tui/run-N.js

Exit 0 when every rule holds, 1 otherwise. Add a rule the moment a cycle teaches
one; a lesson that lives only in prose gets forgotten, which is how three cycles
in a row shipped a lane whose fix sat outside its own zones.
"""
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def protocol_version():
    src = (ROOT / "crates/zz-protocol/src/message.rs").read_text(encoding="utf-8")
    match = re.search(r"pub const PROTOCOL_VERSION: u16 = (\d+)", src)
    return match.group(1) if match else None


def rule_wire(text):
    version = protocol_version()
    if version is None:
        return False, "could not read PROTOCOL_VERSION from crates/zz-protocol/src/message.rs"
    if f"PROTOCOL_VERSION is {version}" not in text:
        return False, f"the wire rule must name the tree's current version, {version}"

    root = Path(__file__).resolve().parent.parent.parent
    tags = subprocess.run(
        ["git", "-C", str(root), "tag", "--list", "v*", "--sort=-v:refname"],
        capture_output=True, text=True).stdout.split()
    released = None
    if tags:
        at_tag = subprocess.run(
            ["git", "-C", str(root), "show", f"{tags[0]}:crates/zz-protocol/src/message.rs"],
            capture_output=True, text=True).stdout
        found = re.search(r"pub const PROTOCOL_VERSION: u16 = (\d+);", at_tag)
        if found:
            released = found.group(1)

    shipped = released == version
    nxt = str(int(version) + 1)
    allowed = {version, nxt} if shipped else {version}
    forward = {v for v in re.findall(r"\bv(\d{3})\b", text)
               if int(v) > int(version) and v not in allowed}
    if forward:
        return False, f"names a version above the tree's {version}: {sorted(forward)}"

    if shipped:
        if re.search(r"STAYS " + version, text):
            return False, (f"the runner says the wire STAYS {version}, but {tags[0]} shipped "
                           f"{version}: a released version cannot take appends, so this cycle "
                           f"opens {nxt}")
        if nxt not in text:
            return False, (f"{tags[0]} shipped {version}, so the first wire append of this cycle "
                           f"opens {nxt}; the runner never says so")
        return True, f"wire rule opens {nxt} because {tags[0]} shipped {version}"
    return True, f"wire rule pinned at {version}, unreleased"


def rule_parses(text):
    """A runner that does not parse wastes a launch and a permission prompt.

    Earned by cycle 9: the compose script emitted `const SLOTS` twice, the lint
    passed because it only reads prose, and the Workflow tool rejected the
    script at launch. Checking the prose without checking the syntax is half a
    check.
    """
    import shutil
    import tempfile
    node = shutil.which("node")
    if node is None:
        return True, "node not available, syntax unchecked"
    dupes = [n for n in ("SLOTS", "LANES", "OPTS", "ORDER", "COMMON", "GATE_SCHEMA")
             if len(re.findall(rf"^const {n}\b", text, re.M)) > 1]
    if dupes:
        return False, f"declared more than once: {', '.join(dupes)}"
    with tempfile.NamedTemporaryFile("w", suffix=".mjs", delete=False) as fh:
        fh.write("const agent=async()=>({});const log=()=>{};const args=undefined;\n"
                 "async function __main(){\n"
                 + re.sub(r"^export const meta", "const meta", text, count=1, flags=re.M)
                 + "\n}\n")
        tmp = fh.name
    try:
        r = subprocess.run([node, "--check", tmp], capture_output=True, text=True, timeout=60)
    finally:
        os.unlink(tmp)
    if r.returncode != 0:
        first = (r.stderr or "").strip().splitlines()
        detail = next((l for l in first if "Error" in l or "error" in l), first[0] if first else "")
        return False, f"does not parse as JavaScript: {detail[:150]}"
    return True, "parses as JavaScript"


def rule_slots(text):
    match = re.search(r"const SLOTS = (\d+)", text)
    if not match:
        return False, "no SLOTS constant: the runner must cap concurrent agents"
    n = int(match.group(1))
    if not 1 <= n <= 3:
        return False, f"SLOTS is {n}; fabrico's ceiling for this box is 3"
    return True, f"SLOTS {n}"


def rule_cargo_wrapper(text):
    if "MemoryMax" not in text or "zz-cargo-slot-" not in text:
        return False, "every cargo command must go through the two-slot flock and a MemoryMax cap"
    return True, "cargo slot-and-cap wrapper present"


def rule_every_fixture(text):
    here = Path(__file__).resolve().parent.parent
    have = sorted(f.name for f in here.glob("tui-*.sh"))
    have += ["status-row.sh", "attached-client.sh"]
    stage = re.search(r"3\. Build zz.*?(?=\n4\. )", text, re.S)
    if stage is None:
        return False, "no gate stage that builds zz and runs the fixtures"
    missing = [n for n in have if n not in stage.group(0)]
    if missing:
        return False, "the gate's own fixture stage never runs " + ", ".join(missing)
    return True, f"the gate stage runs all {len(have)} proof fixtures in the tree"


RULES = [
    ("zones-around-obligations",
     "cycles 4 to 7: an obligation stayed open only because its last fix sat in a crate the lane "
     "did not hold, three times running",
     lambda t: (bool(re.search(r"zones? .{0,40}around .{0,20}obligation", t, re.I)),
                "COMMON must say a lane's zones are drawn around its obligation, not around a crate")),

    ("judged-on-flips",
     "cycle 4: three lanes merged and verified nothing because they measured divergences and "
     "recorded them instead of fixing them",
     lambda t: (bool(re.search(r"recorded case|flip .{0,30}assert|zero recorded", t, re.I)),
                "each batch must say the lane is judged on recorded cases flipped to asserted")),

    ("cheap-iteration",
     "cycle 8: a lane checked one count assertion by running a whole 893-test package suite behind "
     "a memory cap and two shared slots, so one edit-check loop cost minutes",
     lambda t: (bool(re.search(r"ITERATE (?:BEHIND|WITH) A (?:TEST )?FILTER", t)),
                "no worker-facing iteration rule: say ITERATE BEHIND A FILTER, naming "
                "cargo test -p <pkg> --lib <name> for the edit-check loop and the full package "
                "only before the commit that closes an item")),

    ("corpus-grep",
     "cycle 6: a corpus scenario still asserted the zz-only chooser chrome a lane had replaced, "
     "twice, and the gate was the first place the two met",
     lambda t: (bool(re.search(r"grep compat/scenarios", t)),
                "a lane that removes a zz-only screen string must grep compat/scenarios for it")),

    ("reviewer-runs-corpus",
     "cycle 6: neither the worker nor the reviewer ran a corpus row, so only the gate could catch it",
     lambda t: (bool(re.search(r"DELTA CORPUS|compat/run\.sh .{0,60}--delta", t)),
                "the reviewer method must run the delta corpus for the lane's touched commands")),

    ("held-proof",
     "cycle 6: a dependent obligation with complete proof would otherwise wait a whole cycle",
     lambda t: (bool(re.search(r"held on|held rule", t, re.I)),
                "a gate must fill the proof block and hold at review when only a dependency is missing")),

    ("gate-order",
     "cycle 5: one gate agent carrying five branches stopped partway and left main untouched",
     lambda t: (bool(re.search(r"const ORDER = \[", t)) and "gate:${" in t.replace("`", ""),
                "declare the gate order and give each branch its own gate agent")),

    ("last-gate-shared-corpus",
     "cycle 7: every gate re-ran the same keys and status rows, hours per cycle for nothing",
     lambda t: (bool(re.search(r"LAST gate in the order", t)),
                "only the last gate should run the shared keys and status scenario sets")),

    ("self-check",
     "every cycle: a fixture case that cannot fail proves nothing",
     lambda t: (t.count("--self-check") >= 3,
                "each lane must run its fixture's --self-check and add a sabotage per flipped case")),

    ("no-attribution",
     "the repo's own CLAUDE.md",
     lambda t: (bool(re.search(r"NO attribution trailers", t)),
                "delivery rules must forbid commit attribution trailers")),

    ("wire-version", "101 shipped in 0.8.0 mid-campaign and a stale rule would misdecode frames",
     rule_wire),
    ("parses", "cycle 9: a duplicated const passed the prose rules and was rejected at launch",
     rule_parses),
    ("agent-concurrency", "five uncapped lanes drove this box out of memory on 2026-09-11",
     rule_slots),
    ("cargo-caps", "the same crash: an uncapped cargo can take the whole machine down",
     rule_cargo_wrapper),
    ("every-fixture",
     "cycle 10: the gate's fixture list had gone three cycles without picking up tui-mouse.sh, "
     "tui-client-commands.sh or tui-superset.sh, so nothing ran them but the lane that owned them",
     rule_every_fixture),
]


def main(argv):
    if len(argv) != 2:
        print(__doc__)
        return 2
    path = Path(argv[1])
    text = path.read_text(encoding="utf-8")
    failures = []
    for name, why, check in RULES:
        ok, detail = check(text)
        print(f"{'ok  ' if ok else 'FAIL'}  {name}: {detail}")
        if not ok:
            failures.append((name, why, detail))
    print()
    if failures:
        print(f"{len(failures)} of {len(RULES)} rules fail for {path}:")
        for name, why, detail in failures:
            print(f"  {name}: {detail}")
            print(f"    earned by: {why}")
        return 1
    print(f"all {len(RULES)} rules hold for {path}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
