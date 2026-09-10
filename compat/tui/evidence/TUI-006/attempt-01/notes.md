# TUI-006 attempt-01

Cycle 5, the choosers lane. Every file here is a real run on alienware.

## Baseline, before any product change (zz at 3319ceba, unchanged origin/main)

- `environment.txt`: both binaries' sha256, the zz revision and its clean
  state, the pin, the OS line, TERM, shell, bash, outer sizes and locale.
- `run-01-origin-main-digest.txt`: `compat/tui-choosers.sh` at that revision.
  Exit 1: 39 asserted comparisons, 28 differ, 8 recorded (7 for a sibling
  lane). The raw stdout was 235589 bytes because every differing row prints
  styled, so this file is a digest of it made with:

  ```
  awk '/^(ok|DIFF|note|all|[0-9]+ of|chooser)/ {print; next}
       /^      case / {print; pairs=0; next}
       /^      [0-9]+ of [0-9]+ rows differ/ {print; next}
       /^        +[0-9]+ (tmux|zz):/ { if (pairs < 4) {print; pairs++}; next }
       /^      cursor/ {print}' run-01.txt
  ```

- `self-check-01-origin-main.txt`: `--self-check` at the same revision. The
  four sabotages are caught and the restoration equivalence passes; the
  chooser equivalence FAILS, as it must before the fix.

## At the code tip 9cf839a3 ("Draw the pin's mode tree for the choosers in the raw TUI")

Built in this worktree at that revision; the evidence commit that adds these
files changes only this directory, so the binary under test is the same.

- `environment-tip.txt`: the same facts at 9cf839a3, clean worktree.
- `run-02-tip.txt`, `run-03-tip.txt`, `run-04-tip.txt`: three runs of
  `compat/tui-choosers.sh`. Each exits 0: all 39 asserted comparisons
  identical, 8 recorded (7 for a sibling lane), empty stderr.
- `self-check-02-tip.txt`: `--self-check`, exit 0: the one-sided tree row,
  preview cell, tag mark and cursor column are each caught in their channel
  and both equivalences (the same window tree, the restored pane) pass.
- `captures/`: the styled decoded screens of run-02 at eight checkpoints,
  pin (`.tmux.txt`) and zz (`.zz.txt`) side by side, passed through `cat -v`,
  with the cursor tuple as the last line. Each pair is identical.
- `stock-keys-regression.txt`: `compat/tui-stock-keys.sh`, exit 0: all 50
  cases agree on every channel they assert, the eight chooser cases included.
- `screen-diff-regression.txt`: `compat/tui-screen-diff.sh`, exit 0: all 111
  asserted checkpoints identical.
- `attached-client-regression.txt`: `compat/attached-client.sh` with
  ZZ_BIN and TMUX_BIN set, exit 0, `attached-client compatibility: PASS`.
  This is clause 3's behavioural coverage (choosers, command-output
  navigation, the stock pane search prompt). It took 575 s of the 580 s call
  cap, so it is evidence at the code tip and not among the final-tip proofs.
- `zz-protocol-unit-tests.txt` (244 passed), `zz-client-unit-tests.txt` (91),
  `zz-tui-unit-tests.txt` (176), `zz-daemon-unit-tests.txt` (861):
  `cargo test -p <crate> --jobs 3 -- --test-threads=2`, all exit 0.
- `zz-integration-tests.txt`: `cargo test -p zz --jobs 3 -- --test-threads=2`,
  779 passed, exit 0 (the cli_binary suite is the 125-test binary in it).
- `clippy.txt`: `cargo clippy -p <crate> --all-targets --all-features --jobs 3
  -- -D warnings` for the four touched crates, each exit 0.

## What the sibling cases wait on

- `find-window-prompt`, `find-window-typed`: glyphs, columns and cursor
  asserted; the prompt row's style is the overlays lane's
  (message-command-style, TUI-007).
- `output-shown`, `output-search-prompt`, `output-search-typed`,
  `output-searched`, `output-selected`: the run-shell view surface is the
  modes lane's. The copied text already matches and `output-closed` asserts
  the return to the pane.
- `client-tree-open` is recorded but not a sibling case: zz does not implement
  choose-client (commands.native-client-tools, outside this obligation).
