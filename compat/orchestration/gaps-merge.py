#!/usr/bin/env python3
"""Three-way merge of compat/tmux-gaps.json at the record level.

usage: gaps-merge.py BASE OURS THEIRS OUT
Records are keyed by id inside gaps[] and closed[]; a record changed on one side
only takes that side, a record changed identically on both is fine, and a record
changed differently on both is a conflict that fails the merge loudly.
"""
import json, sys

base, ours, theirs, out = sys.argv[1:5]
B, O, T = (json.load(open(p)) for p in (base, ours, theirs))
result = {}
for key in ("schema", "pin"):
    assert B[key] == O[key] == T[key], key
    result[key] = O[key]
result["updated_on"] = max(O["updated_on"], T["updated_on"])
conflicts = []

def merge_array(name, key):
    b = {r[key]: r for r in B.get(name, [])}
    o = {r[key]: r for r in O.get(name, [])}
    t = {r[key]: r for r in T.get(name, [])}
    merged = {}
    for rid in sorted(set(b) | set(o) | set(t)):
        rb, ro, rt = b.get(rid), o.get(rid), t.get(rid)
        if ro == rb:
            pick = rt
        elif rt == rb:
            pick = ro
        elif ro == rt:
            pick = ro
        else:
            conflicts.append(f"{name}:{rid}")
            pick = ro
        if pick is not None:
            merged[rid] = pick
    return merged

gaps = merge_array("gaps", "id")
closed = merge_array("closed", "id")
result["gaps"] = [gaps[k] for k in sorted(gaps)]
result["closed"] = [closed[k] for k in sorted(closed)]
kd_key = None
sample = (B.get("known_differentials") or O.get("known_differentials") or T.get("known_differentials") or [{}])[0]
for cand in ("id", "scenario", "name"):
    if cand in sample:
        kd_key = cand
        break
if kd_key:
    kd = merge_array("known_differentials", kd_key)
    order = [r[kd_key] for r in O.get("known_differentials", [])] + [r[kd_key] for r in T.get("known_differentials", []) if r[kd_key] not in {x[kd_key] for x in O.get("known_differentials", [])}]
    result["known_differentials"] = [kd[k] for k in order if k in kd]
else:
    assert O.get("known_differentials") == T.get("known_differentials")
    result["known_differentials"] = O.get("known_differentials", [])
if conflicts:
    print("CONFLICTS:", conflicts)
    sys.exit(2)
open(out, "w").write(json.dumps(result, indent=2, ensure_ascii=True) + "\n")
print(f"merged gaps={len(result['gaps'])} closed={len(result['closed'])} known={len(result['known_differentials'])}")
