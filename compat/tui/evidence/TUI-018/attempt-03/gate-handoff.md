# TUI-018 gate handoff

This is the worker's assessment, not an independent adversarial approval.
Status remains review. The prior rejection is in ../attempt-02/review.md.

Source and fixture revision: 357330a8265601076ab0ab0f921a393ba352d640.
The runtime proof binary has SHA256
158a9408818cb7fa48837b4a277cc8a0290ece374efce1573e62c2d53716a598.
The notes explain its build, the frozen executable layout, and the comment-only
amendment after the fixture cleanup fix.

The three rejected shapes now assert:

- A spent source reader preserves later stdout and state after source, buffer,
  and PaneInput readers. A missing source path still aborts its group.
- File replay retains the command client's one bounded stream, through single
  and multiple readers, a preceding nonreader, and nested files. Daemon-start
  config still has no caller stream.
- Unterminated binary alias stdout retains the raw writer, including file replay
  and the C locale. A later print-only alias does not inherit an earlier raw
  writer's classification.

The stream fixture and the verifier's repeat each report 57 asserted, zero
recorded and four decided cap cases. The passing self-check catches 29 sabotages,
including all 16 new ones, and both equivalences. One of the 14 added assertions
is pane cleanup; the other 13 exercise the review shapes and their controls.
The shared client fixture's stream cases pass and its owner tally has no
TUI-018 records and unattributed=0. Its self-check and attached-client pass.

The overall gate is not green:

- The shared fixture fails three CLI error-status assertions, with zz returning
  2 where tmux returns 1. verify-claims therefore exits 1.
- The full package run fails two unchanged zz GUI tests; both fail alone too.
  Daemon, mux and protocol package tests, all CLI integration tests, clippy on
  all four crates with all targets and features, and cargo fmt pass.
- The fetched v0.10.0 tag releases protocol 103. wire-version.py rejects the
  inherited serde-skipped marker because message.rs changed after that tag.
  The encoding test passes, version 103 is retained as required, and the guard
  was not weakened.
- The full corpus outcome and exact row coverage are recorded in
  corpus-coverage.json and the three corpus-complete logs. All 222 rows and 2,639
  steps ran: 207 clean rows, three matching known tuples and 12 failures after
  retry. Failures include lock defaults, CLI error statuses, a tmux-side resurrect
  title assertion and a zz status-job DATE sample. No clean corpus claim is made.

TUI-018's two acceptance clauses have asserted caller-stream evidence and the
existing four explicit cap decisions. An independent gate must assess that proof
and the failed checks before changing status. This lane does not claim full
F-ALIASES-MULTI-BODY closure across CLI error paths, and has not read or changed
the board.
