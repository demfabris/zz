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


MAIN_LOCK = [
    comment(10, 8, "FRONT MAIN\nkind: lock\npriority: 1\ncontract: integration lock\nzones: main-lock"),
]


def test_withdraw_on_claimed_or_candidate_front_is_void():
    b = fold(
        BASE
        + [
            comment(4, 3, "CLAIM F-A\nholder: w/one\nlease: 6h"),
            comment(5, 4, "CANDIDATE F-A\nholder: w/one\nbase: abc\ncommit: def\nbranch: campaign/F-A"),
            comment(6, 5, "WITHDRAW F-A\nholder: good/triager\nreason: superseded"),
        ]
    )
    assert b.fronts["F-A"].state == "CANDIDATE", b.fronts["F-A"].state
    assert any("in state CANDIDATE ignored" in w for w in b.warnings), b.warnings


def test_repair_dissolves_candidate_and_keeps_active_claim():
    b = fold(
        BASE
        + MAIN_LOCK
        + [
            comment(11, 9, "CLAIM F-A\nholder: w/one\nlease: 6h"),
            comment(12, 10, "CANDIDATE F-A\nholder: w/one\nbase: abc\ncommit: def\nbranch: campaign/F-A"),
            comment(13, 11, "CLAIM MAIN\nholder: w/one\nlease: 2h"),
            comment(14, 12, "REPAIR F-A\nholder: w/one\nreason: gate red"),
        ]
    )
    f = b.fronts["F-A"]
    assert f.state == "CLAIMED", (f.state, b.warnings)
    assert f.candidate is None
    assert f.holder == "w/one"


def test_repair_after_lease_expiry_frees_the_front():
    b = fold(
        BASE
        + MAIN_LOCK
        + [
            comment(11, 9, "CLAIM F-A\nholder: w/one\nlease: 2m"),
            comment(12, 10, "CANDIDATE F-A\nholder: w/one\nbase: abc\ncommit: def\nbranch: campaign/F-A"),
            comment(13, 20, "CLAIM MAIN\nholder: w/two\nlease: 2h"),
            comment(14, 21, "REPAIR F-A\nholder: w/two\nreason: gate red"),
        ]
    )
    f = b.fronts["F-A"]
    assert f.state == "READY", (f.state, b.warnings)
    assert f.candidate is None


def test_bare_number_lease_reads_as_hours():
    b = fold(BASE + [comment(4, 3, "CLAIM F-A\nholder: w/one\nlease: 2")])
    f = b.fronts["F-A"]
    assert f.state == "CLAIMED", (f.state, b.warnings)
    assert f.expiry == board_mod.parse_time("2026-08-30T12:03:00Z"), f.expiry


def test_records_only_merge_lands_on_the_ledger():
    b = fold(
        BASE
        + MAIN_LOCK
        + [
            comment(11, 9, "CLAIM MAIN\nholder: w/one\nlease: 2h"),
            comment(12, 10, "INTEGRATED MAIN\nholder: w/one\nmerge: cafe1234"),
        ]
    )
    assert b.records and b.records[0]["merge"] == "cafe1234", b.records
    assert b.fronts["MAIN"].state == "CLAIMED", b.fronts["MAIN"].state


def test_integrated_main_from_non_holder_is_ignored():
    b = fold(
        BASE
        + MAIN_LOCK
        + [
            comment(11, 9, "CLAIM MAIN\nholder: w/one\nlease: 2h"),
            comment(12, 10, "INTEGRATED MAIN\nholder: w/two\nmerge: cafe1234"),
        ]
    )
    assert not b.records, b.records
    assert any("does not hold MAIN" in w for w in b.warnings), b.warnings


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
