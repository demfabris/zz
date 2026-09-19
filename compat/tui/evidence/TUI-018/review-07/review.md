# Seventh-review verdict: ACCEPT (review-07, 2026-09-19)

Branch `campaign/tui-stream-alias-7` at `c65065c1`, reviewed against the pin
`d77c9dc6` with binaries built by this review, not the lane's:

- tip `target/debug/zz_cli`: `56fcf707376c58bfcb9b08d5d193ea0272f69534e0f385046f524b655a5612a0`
- pre-fix (5 code files at `0560d58e`): `d548c98e0e1d2e495c7c316e83adb274355de50012dd73b64e8722e524d7812f`
- pin `compat/.cache/tmux-src/tmux`: `df2cafcb212e8b69677dd57c2bd0c3bb59592eaf385d974ec0124526d6b63ee5`

## What was re-measured

- `compat/tui-command-streams.sh` 3x on the tip binary: `all 208 asserted
  comparisons identical, 0 recorded not asserted, 5 decided` EXIT 0 each, zero
  `env` lines. `--self-check`: every sabotage caught, both equivalences, EXIT 0.
  (`target/review-07/streams-{1,2,3}.txt`, `self-check.txt`)
- Sabotage by experiment: the 6 new cells against the pre-fix binary fail 6/6
  (4 pane-death cells on exit+stdout+state: zz rc 0, `tail-out`, `@after=yes`;
  split-print-partial on state; split-print-term on stdout+state labelled `env`
  but still failed). `target/review-07/sabotage-matrix-prefix.txt`. Restored
  tree rebuilds byte-identical (`56fcf707`) and the matrix goes 7/0 green.
- Own probes P1-P6 (`review-07/probes/`, tip) against the pin:
  - P1 four pane-death shapes: rc 1, empty stdout/stderr, tail dropped,
    `@after` unset, intermediate screen identical. Only `cursor_flag` differs
    (zz 1, pin 0) — identical on the pre-fix binary (`probes-prefix/`).
  - P2a kill between `-P` release and first chunk: both exit 1 with the `-P`
    line present. P2b kill-after-EOF: identical 8/2 distributions, rc 0,
    tail runs. P2c SIGTERM-after-kill: both rc 0 with `-P` line, tail dropped.
  - P3 zoomed split: zoom 1->0 both, geometry/screens identical.
  - P4 chain cross-talk (`-P` release + raw `save-buffer -` both orders,
    middle failure, stream+failure+tail): rc/stdout/stderr identical.
  - P5 wait skip: `-I -P` skips the 2 s wait (pre-fix 2825-2923 ms, tip
    779-890 ms loaded) with pid/tty formats identical; plain `-d -P ''` and
    `-E -P` still wait (registered gap, not a regression). `pane_current_command`
    on empty panes differs (P5a pin `''`, P5c pin `bash`/default-command, zz
    empty) — identical on the pre-fix binary; the pin's value is sticky and
    follows `default-command`.
  - P6 latency: split-empty-P 2230-2368 ms vs pin 2-3 ms; round trip ~400 ms
    loaded vs pin 2 ms (lane: 88-142 ms on a quieter box; this box sat at
    load ~5 with parallel lanes). Shape and localization confirmed:
    `-P`-empty minus no-`-P` ~= the 2 s deadline.
- Delta corpus, same 220 rows / 3 shards: 211 green + 3 known-exact on tip;
  6 deterministic reds byte-identical on the pre-fix binary (inherited);
  source-replay-diagnostics, status-background-jobs and continuum flip on
  both binaries across runs (inherited intermittents). Zero caused here.
- Tests: zz-protocol 232+23 green; zz-client green; zz-mux 537+ green;
  zz-daemon lib 984 green with `agent_stream_soak_slow_client` failing
  1501-vs-1500 exactly as attempt-10/11 recorded on fresh main; zz-cli
  135+1 with the recorded `control_sourced_run_shell` race (this pass does
  not touch control mode). Clippy `-D warnings`, `cargo fmt --check`,
  `compat/check.sh`, both trackers, `wire-version.py`, `evidence-secrets.py`
  all green.
- Wire: two `EventPayload` variants are the enum tail after
  `ChooserPresentation`, one v105 entry, nothing reordered, `105 unreleased`
  per the guard. Events are unicast to Command-kind clients only; ClientCore
  and control mode ignore them; attached-client PASS.
- Merge `0560d58e` recomputed byte-identical: a pure merge. Gate preview
  (`merge-tree origin/main HEAD`): 2 conflicts, both mechanical —
  `campaign.json` (record-merge: main verified TUI-015/017, branch proves
  TUI-018) and generated `tui-parity.md` (regenerate). No code conflicts.

## Findings (none block TUI-018)

1. must-fix, pre-existing, formats owner: `#{pane_current_command}` on empty
   panes (`crates/zz-mux/src/formats.rs:568`). Fix direction: fall back to the
   pane's start/default command basename the way the pin does.
2. must-fix, pre-existing, formats owner: `cursor_flag` hardcoded `One`
   (`crates/zz-mux/src/formats.rs:530`) answers 1 on empty panes where the pin
   answers 0. Fix direction: report real cursor visibility.
3. nit: the `env` message asserts cause ("starvation on this box and not a
   parity difference"); the sabotage run shows it can label a genuine
   behavioral difference. It still fails correctly. Soften to the symptom.
4. nit: TUI-018 `sources` omits `knowledge/protocol/wire-protocol.md`, changed
   by this pass. Gate to amend.

## Clause assessment

TUI-018 clause 1 (bounded channel carrying the three forms, refusals with
workloads) and clause 2 (exact stdout/stderr/exit/state per form) both hold at
this tip: 208 asserted cells green 3x plus self-check, no TUI-018 record in the
shared fixture, both sixth-review findings closed with sabotages that fail
without the fix. The two must-fix residuals above are mux-format divergences
reproducible with no stream involvement and owned outside this obligation.
