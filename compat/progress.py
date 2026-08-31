#!/usr/bin/env python3
"""Agreed-scope progress for the tmux compat campaign.

The ledger settlement percentage moves with discovery: every residual folded in
grows the denominator, so a productive day can read as standing still. This
meter freezes the agreed scope once (`freeze`) and reports burn-down against
that fixed basket. Item slugs are globally unique and survive group splits, so
an item counts as done exactly when it has left every unresolved group. Scope
discovered after the freeze is reported separately and never dilutes the number.

Usage:
  python3 compat/progress.py freeze   # write compat/progress-baseline.json (once per agreed scope)
  python3 compat/progress.py          # report against the frozen baseline
"""

import json
import pathlib
import sys
from datetime import date

HERE = pathlib.Path(__file__).resolve().parent
REGISTRY = HERE / "tmux-gaps.json"
BASELINE = HERE / "progress-baseline.json"


def load_registry():
    return json.loads(REGISTRY.read_text())


def freeze():
    if BASELINE.exists():
        sys.exit(
            f"{BASELINE.name} exists; a baseline is frozen once per agreed scope "
            "(delete it deliberately to re-freeze)"
        )
    reg = load_registry()
    unres = [g for g in reg["gaps"] if g["status"] in ("open", "blocked")]
    payload = {
        "frozen_on": date.today().isoformat(),
        "note": "agreed scope: every group unresolved at freeze time; an item is done when it has left every unresolved group",
        "groups": {
            g["id"]: {"decision": g["decision"], "ease": g["ease"], "items": g["items"]}
            for g in unres
        },
    }
    BASELINE.write_text(json.dumps(payload, indent=1) + "\n")
    n = sum(len(v["items"]) for v in payload["groups"].values())
    print(f"froze {len(payload['groups'])} groups / {n} items as the agreed scope ({payload['frozen_on']})")


def report():
    if not BASELINE.exists():
        sys.exit("no baseline; run: python3 compat/progress.py freeze")
    base = json.loads(BASELINE.read_text())
    reg = load_registry()
    open_items = {i for g in reg["gaps"] if g["status"] in ("open", "blocked") for i in g["items"]}
    base_groups = base["groups"]
    base_items = {i for v in base_groups.values() for i in v["items"]}
    done_items = {i for i in base_items if i not in open_items}
    group_done = {gid: [i for i in v["items"] if i not in open_items] for gid, v in base_groups.items()}
    full = sorted(gid for gid, d in group_done.items() if len(d) == len(base_groups[gid]["items"]))
    partial = sorted(gid for gid, d in group_done.items() if 0 < len(d) < len(base_groups[gid]["items"]))
    pct = 100.0 * len(done_items) / len(base_items) if base_items else 100.0
    print(f"agreed scope frozen {base['frozen_on']}: {len(base_groups)} groups / {len(base_items)} items")
    print(
        f"PROGRESS: {pct:.1f}% ({len(done_items)}/{len(base_items)} items) | "
        f"groups done {len(full)}/{len(base_groups)}, partially burned {len(partial)}"
    )
    parks = [gid for gid, v in base_groups.items() if v["decision"] == "park"]
    parks_done = [g for g in parks if len(group_done[g]) == len(base_groups[g]["items"])]
    print(f"park dispositions: {len(parks_done)}/{len(parks)}")
    new_items = open_items - base_items
    if new_items:
        holders = sorted(
            g["id"]
            for g in reg["gaps"]
            if g["status"] in ("open", "blocked") and any(i in new_items for i in g["items"])
        )
        print(f"scope added since freeze (tracked, not diluting the %): {len(new_items)} items across {len(holders)} groups")
    for gid in full:
        print(f"  done     {gid}")
    for gid in partial:
        print(f"  partial  {gid} ({len(group_done[gid])}/{len(base_groups[gid]['items'])})")


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "freeze":
        freeze()
    elif len(sys.argv) == 1:
        report()
    else:
        sys.exit(__doc__.strip())
