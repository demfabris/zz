# TUI-004 review — canvas lane (campaign/tui-canvas-sidebar)

Reviewer: Opus 5, independent review worktree `/home/demfabris/dev/zz-tui-canvas-review`
at `cdfd97be9e5ac60f1c17bad00e41507419ac74fc`.
Verdict: **approve**. Two nits, no must-fix, no blocker.

## Reviewer verdict, verbatim

```json
{
  "lane": "canvas",
  "verdict": "approve",
  "confirmed_defects": [
    {
      "obligation": "TUI-004",
      "severity": "nit",
      "description": "environment.txt and proofs-at-tip.txt both record `tip: 5e96347476b954ee3cee783ca594797fd2a4355f`, which is the PARENT of the branch tip cdfd97be9e5ac60f1c17bad00e41507419ac74fc. A reader chasing the recorded revision lands one commit short of the branch. I verified this is harmless: `git diff --name-only 5e963474 cdfd97be` returns evidence files under compat/tui/evidence/TUI-004/attempt-01/ and nothing else, so no fixture, code or ledger byte moved after the proofs ran, and my own re-runs at cdfd97be reproduce the committed numbers line for line. The cause is structural (a file cannot name the commit that contains it), not dishonesty.",
      "suggested_fix": "Add one line to proofs-at-tip.txt: 'recorded at 5e963474; the commit that carries this file, cdfd97be, adds evidence text only and changes no fixture, code or ledger byte.' No re-run needed."
    },
    {
      "obligation": "TUI-004",
      "severity": "nit",
      "description": "The theme-arm measurement calls options.theme-palette an OPEN gap, in two places: TUI-004's evidence_note (\"All 21 names are items of the OPEN gap options.theme-palette\") and the dated TUI measurement appended to tui.status-row's reason (\"All 21 names are items of the OPEN gap options.theme-palette\"). The registry says that gap's status is `accepted`, not open. Everything else in the measurement checks out against the pinned C and the live test, and the mislabel does not weaken the conclusion - it strengthens it, since taking the 21 names as consumers would reopen an ACCEPTED decision rather than merely close an open one.",
      "suggested_fix": "Replace 'the OPEN gap options.theme-palette' with 'the accepted gap options.theme-palette' in both places, then re-run python3 compat/tui/tracker.py write-report and python3 compat/tmux-tracker.py write-report."
    }
  ]
}
```

### Reviewer checks_run, verbatim

```
GIT_TERMINAL_PROMPT=0 git -C /home/demfabris/dev/zz fetch origin +refs/heads/main:refs/remotes/origin/main '+refs/heads/campaign/*:refs/remotes/origin/campaign/*'
git -C /home/demfabris/dev/zz worktree add /home/demfabris/dev/zz-tui-canvas-review cdfd97be9e5ac60f1c17bad00e41507419ac74fc
find crates -name '*.rs' -exec touch {} + ; find . -maxdepth 2 -name 'Cargo.*' -exec touch {} +
CARGO_TARGET_DIR=/home/demfabris/dev/zz-tui-canvas/target cargo build -p zz --jobs 5  -> exit 0 (1m43s)
ZZ_COMPAT_TMUX=<pin> ZZ_BIN=<target>/debug/zz compat/tui-screen-diff.sh  -> exit 0, all 111 asserted checkpoints identical, 42 recorded (run 1)
ZZ_COMPAT_TMUX=<pin> ZZ_BIN=<target>/debug/zz compat/tui-screen-diff.sh  -> exit 0, all 111 asserted checkpoints identical, 42 recorded (run 2)
ZZ_COMPAT_TMUX=<pin> ZZ_BIN=<target>/debug/zz compat/tui-screen-diff.sh --self-check  -> exit 0, 12 expectations met, every sabotage caught in its own channel
ZZ_COMPAT_TMUX=<pin> ZZ_BIN=<target>/debug/zz compat/tui-pane-geometry.sh  -> exit 0 three times, 6 asserted measurements identical, 120x24 columns both 120
LC_ALL=C LC_TIME=C ZZ_COMPAT_TMUX=<pin> compat/status-row.sh <zz> <pin>  -> exit 0, 11 comparisons identical, 3 rows recorded
compat/attached-client.sh <zz> <pin>  -> exit 0, attached-client compatibility: PASS
CARGO_TARGET_DIR=<worker target> cargo test -p zz-tui --jobs 5 -- --test-threads=2  -> exit 0, 170 passed 0 failed
timeout 2400 cargo test -p zz --jobs 5 -- --test-threads=2  -> exit 0, 630 lib + 125 cli_binary + the rest, no failures (integration-test rule)
cargo clippy -p zz-tui --all-targets --all-features --jobs 5 -- -D warnings  -> exit 0
cargo clippy -p zz --all-targets --all-features --jobs 5 -- -D warnings  -> exit 0
python3 compat/tui/tracker.py check  -> exit 0; python3 compat/tmux-tracker.py check  -> exit 0
python3 compat/tui/tracker.py write-report && python3 compat/tmux-tracker.py write-report && git status --short  -> empty (reports regenerated, not hand-edited)
MY OWN SABOTAGE: patched sidebar.rs visible() back to `columns >= MIN_MANUAL_COLUMNS && (self.shown || columns >= 109)`, rebuilt, ran compat/tui-pane-geometry.sh  -> exit 1, 'DIFF  120x24 columns: tmux 120, zz 91'
MY OWN SABOTAGE: same build, compat/tui-screen-diff.sh  -> exit 2, 37 DIFF lines covering every 109x24 and 120x24 checkpoint, the '120x24-from-80x24 resized' case, and the text-mode pane-border checkpoints; the sidebar case then failed its withdrawal wait
git checkout -- crates/zz-tui/src/sidebar.rs && cargo build -p zz --jobs 5  -> exit 0, worktree clean at cdfd97be
WIDTH PROBE: throwaway copy of tui-pane-geometry.sh with SIZES=(109x24 119x24 120x24 200x24)  -> exit 0, all 8 asserted measurements identical (zz hands the pane 109/119/120/200 like the pin)
ORACLE: pin -L zzprobe-$$ -f /dev/null, status-left-length 10 + status-left '#[fg=red,bold]LEFT', display-message -p '#{T;=/#{status-left-length}:status-left}'  -> pin '#[fg=red,bold]LEFT'
ORACLE: same probe against zz on a /tmp socket  -> '#[fg=red,b'; styled status-right loses its text the same way; #{E:dark-theme-green} and #{theme} both empty at the tip
ORACLE: pin display-message -p '#{client_theme}' from a one-shot client  -> empty (confirms the terminal light/dark report is undrivable, as recorded)
ORACLE: grep -oE '"(dark|light)-theme-[a-z-]+"' options-table.c | sort -u | wc -l  -> 20; COLOUR_THEME_COUNT 10 in tmux.h; server_client_update_theme_colours at server-client.c:1173 picks dark or light per client (confirms the 21-name correction)
ORACLE: crates/zz-mux/src/compat_manifest_tests.rs asserts consumers.len()==118, tracked.len()==62, consumers.is_disjoint(&tracked), consumers.union(&tracked)==catalog(180), scope_counts==[15,44,41,18]; options.theme-palette holds exactly 21 theme items (confirms the partition obstacle)
ZONE: git diff --name-only origin/main...HEAD filtered against the declared zones  -> ALL IN ZONE (35 files)
WIRE: git diff --stat origin/main...HEAD -- crates/zz-protocol crates/zz-daemon crates/zz-mux crates/zz-client  -> empty; PROTOCOL_VERSION still 99; TMUX_OPTION_CONSUMERS.len() still 118
LEDGER: scripted field diff of compat/tui/campaign.json  -> only TUI-004 changed (status different->review, evidence_note, next_action); proof stays null
REGISTRY: scripted field diff of compat/tmux-gaps.json  -> only tui.sidebar-auto-hide (resolution pure append + one evidence file ref) and tui.status-row (reason pure append); items, scope and status byte-identical on both; GUI scope explicitly retained
HYGIENE: git check-ignore -v on the evidence dir  -> exit 1 (nothing ignored); zero .log files; git log origin/main..HEAD grep for attribution  -> none
diff of my run-1 ok/note/DIFF lines against the committed screen-diff-tip-1.txt  -> IDENTICAL
pgrep -fl zzprobe / tmux / zzgeo / zzscreen after the run  -> no stray servers, no stray sockets
```

## Gate actions (alienware/orchestrator, 2026-09-10)

The canvas branch was gated FIRST, because its fixture-mode flips define the screen
contract the other two lanes are judged by. It rebased onto `origin/main` as a
fast-forward, so the reviewed tip and the gated tip are the same tree.

Both nits applied, neither needing a re-run:

1. **Recorded tip one commit short.** `proofs-at-tip.txt` now carries the line the
   reviewer asked for, naming `cdfd97be` as the commit that carries the file and
   stating that it adds evidence text only. Verified again at the gate:
   `git diff --name-only 5e963474 cdfd97be` lists only files under
   `compat/tui/evidence/TUI-004/attempt-01/`.
2. **`options.theme-palette` mislabelled OPEN.** Replaced with `accepted` in both
   places the reviewer named — TUI-004's `evidence_note` and the dated TUI
   measurement appended to `tui.status-row`'s `reason`. Both generated reports were
   regenerated with their trackers afterwards and both checks are green.

The reviewer's decisive sabotage was reproduced at the gate by a different route:
`compat/tui-pane-geometry.sh` asserts 120 columns three consecutive times at the
merged tip, and `compat/tui-screen-diff.sh` asserts all 111 checkpoints including
every 109x24 and 120x24 one. Those assertions stayed green through the keys and flow
merges as well, which is what the flip was for.

The theme arm did not land and `PROTOCOL_VERSION` stays 99. The gate agrees with the
lane's reasoning: the consumer half and the wire half are one landing, all 21 names
are items of a gap this lane does not own, and `crates/zz-mux/src/compat_manifest_tests.rs`
enforces the partition in code. Half-landing it would have closed 21 items of someone
else's record. The measurement is worth more than the half-landing would have been and
is recorded for whoever takes it with `options.theme-palette` in scope.

Clause 3 stays open and TUI-004 is verified only for clauses 1 and 2 plus the
sidebar decision; see `evidence_note` and `clauses_open`.
