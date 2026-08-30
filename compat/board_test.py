#!/usr/bin/env python3
"""Offline fold regression tests for compat/board.py. Run: python3 compat/board_test.py"""

import importlib.util
import pathlib

spec = importlib.util.spec_from_file_location("board", pathlib.Path(__file__).with_name("board.py"))
board_mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(board_mod)


def comment(cid, minute, body):
    ts = f"2026-08-30T10:{minute:02d}:00Z"
    return {"id": cid, "created_at": ts, "updated_at": ts, "body": body}


def fold(comments):
    b = board_mod.Board()
    for c in sorted(comments, key=lambda c: c["id"]):
        b.apply(c)
    return b


BASE = [
    comment(1, 0, "FRONT TRIAGE\nkind: lock\npriority: 1\ncontract: front minting lock\nzones: triage-lock"),
    comment(2, 1, "FRONT F-A\npriority: 3\ncontract: gap.a\nzones: config-parser"),
    comment(3, 2, "CLAIM TRIAGE\nholder: good/triager\nlease: 2h"),
]


def test_withdraw_requires_triage_hold():
    b = fold(BASE + [comment(4, 3, "WITHDRAW F-A\nholder: bad/not-triager\nreason: nope")])
    assert b.fronts["F-A"].state == "READY", b.fronts["F-A"].state
    assert any("does not hold TRIAGE" in w for w in b.warnings), b.warnings


def test_withdraw_by_triage_holder_lands():
    b = fold(BASE + [comment(4, 3, "WITHDRAW F-A\nholder: good/triager\nreason: superseded")])
    assert b.fronts["F-A"].state == "WITHDRAWN", b.fronts["F-A"].state


def test_release_after_candidate_frees_zones_and_goes_stale():
    b = fold(
        BASE
        + [
            comment(4, 3, "CLAIM F-A\nholder: w/one\nlease: 6h"),
            comment(5, 4, "CANDIDATE F-A\nholder: w/one\nbase: abc\ncommit: def\nbranch: campaign/F-A"),
            comment(6, 5, "RELEASE F-A\nholder: w/one\nreason: yielding zones"),
            comment(7, 6, "FRONT F-B\npriority: 3\ncontract: gap.b\nzones: config-parser"),
            comment(8, 7, "CLAIM F-B\nholder: w/two\nlease: 6h"),
        ]
    )
    assert b.fronts["F-A"].state == "STALE-CANDIDATE", b.fronts["F-A"].state
    assert b.fronts["F-A"].candidate["commit"] == "def"
    assert b.fronts["F-B"].state == "CLAIMED", b.warnings


if __name__ == "__main__":
    failures = 0
    for name, fn in sorted(globals().items()):
        if name.startswith("test_") and callable(fn):
            try:
                fn()
                print(f"ok   {name}")
            except AssertionError as e:
                failures += 1
                print(f"FAIL {name}: {e}")
    raise SystemExit(1 if failures else 0)
