# TUI-010 review — flow lane (campaign/tui-client-lifecycle)

Reviewer: Opus 5, independent review worktree `/home/demfabris/dev/zz-tui-flow-review`
at `67e57c41ddb87b1614274fc575aaeecd5c5c12e7`.
Verdict: **approve**. Two nits, no must-fix, no blocker.

## Reviewer verdict, verbatim

```json
{
  "lane": "flow",
  "verdict": "approve",
  "confirmed_defects": [
    {
      "obligation": "TUI-010",
      "severity": "nit",
      "description": "The worker rewrote the `sources` array of its own TUI-010 record (dropped crates/zz-tui/src/tty.rs and crates/zz-tui/src/state.rs, added crates/zz-tui/src/render.rs, compat/tui-output-backpressure.sh, compat/tui-output-relay.py). The batch prompt limits a worker to status, evidence_note and next_action. Proved by diffing the two ledgers field by field: only TUI-010 changed, and its changed fields are ['status','sources','evidence_note','next_action']; proof is still null, acceptance is byte-identical, TUI-001 and TUI-002 are still `verified` and untouched, and baseline/milestones/tmux_commit are unchanged. The edit is factually right (the diff really does touch render.rs and really does not touch tty.rs or state.rs), so this is a process nit, not a content defect.",
      "suggested_fix": "Gate decision: either keep the corrected `sources` (it matches the diff) and note that the lane wrote a fourth field, or restore origin/main's array and let the gate rewrite it alongside the proof block."
    },
    {
      "obligation": "TUI-010",
      "severity": "nit",
      "description": "The block-clear timer is one interval less faithful to the pin than the resolution claims. tmux tty.c tty_block_maybe sets `tty->discarded = 0` when it starts a block (compat/.cache/tmux-src/tty.c:241), so the pin's first 100 ms interval counts only bytes offered after the drop and can clear the block after one interval. crates/zz-tui/src/writer.rs `submit` instead does `state.discarded = state.queued + bytes.len();`, folding the whole ~4 MiB dropped queue into the first interval. BLOCK_STOP is QUEUE_BUDGET/64 = 65536, so `discarded >= BLOCK_STOP` is always true on the first tick and run_block_timer always takes the `continue` branch: a zz block survives at least two intervals where the pin's can end after one. Failure scenario: a viewer that resumes reading in the same instant a block starts waits >=200 ms for the full repaint instead of >=100 ms. Not user-visible at this scale and input stays live throughout, which is why it is a nit and not a must-fix; the 64:1 ratio claim in the resolution and the fixture header is otherwise exact (TTY_BLOCK_START 1+sx*sy*8, TTY_BLOCK_STOP 1+sx*sy/8, TTY_BLOCK_INTERVAL 100000us, all read at the pin).",
      "suggested_fix": "Either set `state.discarded = 0` at block start to match tty_block_maybe, or say in the writer.rs comment and the gap resolution that the block start deliberately counts its own drop, so a future reader does not read it as the pin's rule."
    }
  ]
}
```

### Reviewer checks_run, verbatim

```
GIT_TERMINAL_PROMPT=0 git -C /home/demfabris/dev/zz fetch origin +refs/heads/main:refs/remotes/origin/main '+refs/heads/campaign/*:refs/remotes/origin/campaign/*'  -> origin/main bfd05821, origin/campaign/tui-client-lifecycle 67e57c41 (matches the report's FINAL TIP)
git worktree add /home/demfabris/dev/zz-tui-flow-review 67e57c41 ; git status --short  -> clean at the tip
git diff --stat origin/main...HEAD  -> 28 files, all inside the lane's declared zones (compat/tui/, compat/attached-client.sh, two new compat/ fixture files, crates/zz-tui/src/{writer,render,app}.rs, compat/tmux-gaps.json, plus the two generated knowledge/tmux/*.md)
git diff --stat origin/main...HEAD -- crates/zz-protocol crates/zz-daemon crates/zz-mux crates/zz-client crates/zz/tests  -> EMPTY (wire rule clean, PROTOCOL_VERSION not touched)
touch crates/**/*.rs Cargo.* ; CARGO_TARGET_DIR=/home/demfabris/dev/zz-tui-flow/target cargo build -p zz --jobs 5  -> exit 0 in 1m30s (my own binary, sha256 10234e50..., different from the recorded 900bec3e as expected on this box)
ZZ_COMPAT_TMUX=<pin> compat/tui-output-backpressure.sh --undrained 45 <my tip build>  -> exit 0, all 9 assertions passed, 3 recorded. tmux drained 0.023 undrained 0.024 growth 0 kB; zz drained 0.028 undrained 0.023 growth 6616 kB (report claimed 0.033/0 and 0.025/6572)
ZZ_COMPAT_TMUX=<pin> compat/tui-output-backpressure.sh --side zz --undrained 45 target/debug/zz-prefix-main  -> exit 1, 'FAIL zz undrained key did not act within 1.000 s after 45 s undrained (got none)', growth 6144 kB. Reproduces evidence 01 exactly
MY OWN SABOTAGE: throwaway copy of compat/ with tui-output-relay.py's SIGUSR1 handler setting hold=False, so the terminal never stops reading  -> exit 2, 'error: zz pty never filled while the relay held it; last avail=0'. The undrained window is real backpressure, not a no-op, and the fixture names the location
ZZ_COMPAT_TMUX=<pin> compat/tui-output-backpressure.sh --self-check --undrained 3 --key-timeout 4  -> exit 0, both sabotages caught (late paint in the screen channel and the escape-tail channel; halved deadline fires the response bound)
RECONSTRUCTED SABOTAGE A (window-size forced to smallest before the latest-rule case) on a throwaway copy, --lifecycle  -> exit 1, 'error: tmux latest simultaneous sizing follows the second client did not become 70x30 within 10 seconds; last output: 70x20' (matches the worker's reported string)
RECONSTRUCTED SABOTAGE B (second client attached without -r plus the readonly-flag gate neutered), --lifecycle  -> exit 1, 'error: tmux pane unexpectedly contained ATTACHED_READONLY_TYPED' (matches the worker's reported string)
RECONSTRUCTED SABOTAGE C (client reattaches at 70x20 instead of 80x24), --lifecycle  -> exit 1, 'error: tmux session attached client sizes did not become 80x24 within 10 seconds; last sizes: 70x20'
ZZ_COMPAT_TMUX=<pin> compat/attached-client.sh --lifecycle <tip> <pin>  -> exit 0, 'client lifecycle compatibility: PASS' (24s)
ZZ_COMPAT_TMUX=<pin> compat/attached-client.sh <tip> <pin>  -> exit 0, 'attached-client compatibility: PASS' (6m18s, full run)
ZZ_COMPAT_TMUX=<pin> compat/tui-screen-diff.sh <tip> <pin>  -> exit 0, all 33 asserted checkpoints identical, 32 recorded
ZZ_COMPAT_TMUX=<pin> compat/tui-pane-geometry.sh <tip> <pin>  -> exit 0, all 5 asserted measurements identical
ZZ_BIN=<tip> ZZ_COMPAT_TMUX=<pin> ZZ_COMPAT_CORPUS=<corpus> compat/run.sh --strict-geometry smoke/tui-client-input-backpressure  -> exit 0, 0 divergences on every channel
CARGO_TARGET_DIR=<worker> cargo test -p zz-tui -p zz-terminal --jobs 5 -- --test-threads=2  -> exit 0, 245 + 171 passed, first try, no flake
CARGO_TARGET_DIR=<worker> cargo clippy -p zz-tui --all-targets --all-features --jobs 5 -- -D warnings  -> exit 0
CARGO_TARGET_DIR=<worker> timeout 2400 cargo test -p zz --jobs 5 -- --test-threads=2  -> exit 0 in 2m29s, 630 + 125 + the rest passed, no known-flake test fired
python3 compat/tui/tracker.py check  -> exit 0 ; python3 compat/tmux-tracker.py check  -> exit 0
python3 compat/tui/tracker.py write-report && python3 compat/tmux-tracker.py write-report && git status --short  -> EMPTY: both generated reports regenerate byte for byte, not hand-edited
ORACLE: read options-table.c:1778-1788 at the pin  -> window-size .default_num = WINDOW_SIZE_LATEST. The worker's premise correction is right and the brief's 'sizes to the smallest' was wrong
ORACLE: read tty.c:81-83, tty_block_maybe (tty.c:218-245) and tty_timer_callback at the pin  -> TTY_BLOCK_INTERVAL 100000us; START 1+sx*sy*8 (15361 at 80x24, as the fixture header states); STOP 1+sx*sy/8, ratio 64; the block drains the whole out buffer and arms a timer; the timer sets CLIENT_ALLREDRAWFLAGS, and under STOP clears TTY_BLOCK and calls tty_invalidate. The drop-and-redraw shape in writer.rs/render.rs/app.rs matches
CODE READ: every hunk of writer.rs, render.rs and app.rs. submit() never waits on either path; abandon() sets closed=true so run_block_timer returns without waking; MainEvent::Repaint can only be handled inside the loop, so no repaint can land after the guard restores; flush_output's Dropped arm calls invalidate() which clears painted/headers/picker_cards/sidebar_rows/status_rows/status_geometry/damage/browser_painted/border_chrome and kitty.invalidate(), and the repaint is paint(&model, true) from the model. Dropped DATA does not mean dropped STATE
CODE READ: emit_queued_control()/control_replay lifecycle across paint() and paint_frames()  -> exactly one emit per flush, control_replay taken fresh each flush, output.clear() never precedes an emit within the same call, so no duplicated or silently-lost control bytes
python3 field-by-field diff of compat/tui/campaign.json against origin/main  -> only TUI-010 changed; proof still null; acceptance identical; TUI-001/TUI-002 still verified and untouched; baseline, milestones and tmux_commit unchanged
python3 field-by-field diff of compat/tmux-gaps.json against origin/main  -> only tui.client-output-queue-budget moved from gaps[] to closed[]; no other gap or closed record changed; the new closed entry's key set is exactly CLOSED_FIELDS {id,title,closed_on,evidence,resolution}, matching all 199 existing closed entries, and closed[] stays sorted by id
git check-ignore -v compat/tui/evidence/TUI-010/attempt-01/*  -> exit 1, nothing ignored; no .log file anywhere in the diff; all 19 evidence files are real captures with plausible sizes and contents I re-derived
git log --format=%B origin/main..HEAD | grep -iE 'co-authored|generated with|claude|noreply@'  -> none (no attribution trailers)
Evidence cross-check: 03 vs 04 (before/after the drop path) show the same two status-row bytes, confirming the recorded divergence predates this change; my own run reproduced restores=1 tail=40 escapes=0 for zz and restores=0 for the pin
Sweep: ps -eo pid,args | grep -E 'zzbp|zztr|tui-output-relay'  -> no leftover fixture processes; no socket of mine left in /tmp/tmux-1000; /run/user/1000/zz absent; the user's default sockets never touched; every server I started was -f /dev/null on a throwaway /tmp socket with scrubbed HOME and XDG_CONFIG_HOME
```

## Gate actions (alienware/orchestrator, 2026-09-10)

The flow branch was gated THIRD, rebased onto the accumulated canvas+keys tip
`3e7019fc`. Two rebase conflicts, both in generated reports
(`knowledge/tmux/gaps.md`, `knowledge/tmux/tui-parity.md`), resolved by running each
tracker's `write-report`, never by hand. `crates/zz-tui/src/render.rs` auto-merged
against both the canvas cursor work and the keys hint change, as the reviewer
predicted: the flow hunks sit near line 492 and the others are far below.
`compat/tmux-gaps.json` and `compat/tui/campaign.json` merged by record id with each
lane keeping only its own.

Both nits were decisions for the gate, and both are recorded rather than papered over:

1. **The lane wrote a fourth ledger field (`sources`).** Gate decision: **keep the
   corrected array**. The reviewer verified it against the diff and it is factually
   right — the branch does touch `crates/zz-tui/src/render.rs` and does not touch
   `tty.rs` or `state.rs`, and the two new fixture files are genuinely sources of this
   obligation. Restoring a stale array to satisfy the letter of the field list would
   make the record less true. Noted here as a process deviation so it is not read as
   precedent: a worker may write status, evidence_note and next_action.
2. **The block-clear timer outlives the pin's by one interval.** Gate decision: **do
   not change behaviour at the gate; record the measurement**. The reviewer is right
   about the mechanism — `tty_block_maybe` zeroes `tty->discarded` at block start
   (tty.c:241) while `crates/zz-tui/src/writer.rs` folds the dropped queue into the
   first interval, so `discarded >= BLOCK_STOP` is certain on the first tick and a zz
   block lasts at least two 100 ms intervals. It is a nit: bounded, invisible at this
   scale, and input stays live throughout. Changing it is a behaviour change that
   would need the backpressure fixture re-proved against a new timing claim, which is
   a lane's work and not a gate's. The measurement is recorded in TUI-010's
   `evidence_note` so the "64:1 ratio" sentence is not read as a claim that the block
   start matches the pin's rule.

At the gate tip every TUI-010 proof was re-run and is green:
`compat/tui-output-backpressure.sh --undrained 45` exit 0 (all 9 assertions passed,
3 recorded), its `--self-check` exit 0, `compat/attached-client.sh --lifecycle` exit 0,
the full `compat/attached-client.sh` exit 0, and
`compat/run.sh --strict-geometry smoke/tui-client-input-backpressure` clean on every
channel inside the delta corpus.

The status-row divergence this fixture records is left routed, not closed: it is a new
occurrence of a known divergence under the DEFAULT status-style rather than the
non-default one the standing `tui.status-row` record names, it only appears on a full
repaint, and `compat/tui-screen-diff.sh` is green and does not see it. The lane
correctly did not edit a record the canvas lane owns. The gate carries it into the
cycle residual.

The server-death half of transport recovery (pin exits within 0.05 s, zz within 0.1 s)
came from an ad-hoc probe that is not on the branch. It stays framed as recorded, not
asserted, and the gate did not promote those numbers into the proof block.
