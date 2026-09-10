# TUI-005 attempt-01

Pane copy mode and search, typed into both attached clients.

## What this attempt did

- Wrote `compat/tui-copy-mode.sh`, a new differential fixture that types the stock
  copy-table chords into both binaries' attached clients inside one outer pinned tmux
  and compares five channels per case.
- Fixed one defect in the raw client: the copy cursor is now the terminal cursor
  instead of a reverse-video cell (`crates/zz-tui/src/render.rs`).
- Measured four divergences it did not fix, each outside this obligation's zones, and
  reproduced the `terminal search is unsupported here` message TUI-005 opened on.

## Files

### The environment

- `environment.txt` — both binary sha256s, the branch and its base, the pin revision,
  the OS line, TERM, shell and locale.

### The fixture at the tip

- `copy-mode-tip-1.txt`, `copy-mode-tip-2.txt`, `copy-mode-tip-3.txt` — three runs of
  `compat/tui-copy-mode.sh`, the whole corpus, exit code recorded in
  `proofs-at-tip.txt`.
- `copy-mode-self-check-tip.txt` — `--self-check`: five one-sided sabotages, one per
  asserted channel plus a style-only difference that must land in the rows channel
  alone, and two equivalences the fixture must not report.
- `measured-cells.txt` — the decoded cells behind each recorded divergence, taken from
  `ZZ_COPY_CAPTURE_DIR` on the first tip run: the selection style, the paste-buffer
  bytes that agree through it, the position indicator at its default, and the search
  prompt row.

### The defect this obligation opened on

- `native-search-command.txt` — the `copy-mode-search-prompt` group's own output: zz
  exits 0 and puts `terminal search is unsupported here` on its last row, the pin
  exits 1 with `unknown command: copy-mode-search-prompt`. The stock chords do not
  travel that way on either binary, which is what the ordinary-pane search cases in
  `copy-mode-tip-*.txt` assert.

### Before the fix, for comparison

- `copy-mode-before-the-cursor-fix.txt` — the same fixture against the unmodified
  client. Every copy-mode checkpoint differs twice: an extra `\e[7m` run in the pane
  row and `cursor_flag 0` at column 79 against the pin's visible cursor on the copy
  cell.

### Regressions

- `screen-diff-tip.txt` — `compat/tui-screen-diff.sh`.
- `stock-keys-tip.txt` — `compat/tui-stock-keys.sh`.
- `stock-keys-flake.txt` — one run where four chooser cases reported
  `cmd=bash` against the pin's `cmd=sh`, which is the `#{pane_current_command}` settle
  that fixture's own `COMMAND_SETTLE_REASON` names.
- `stock-keys-baseline-without-the-render-change.txt` — the same fixture with
  `crates/zz-tui/src/render.rs` stashed and the binary rebuilt: green. Restoring the
  change and re-running was green again (`stock-keys-tip.txt`), so the four reds were
  the flake and not this branch.

### Code

- `unit-tests-tip.txt` — `cargo test -p zz-tui`.
- `clippy-tip.txt` — `cargo clippy -p zz-tui --all-targets --all-features -- -D warnings`.
- `zz-integration-tests-tip.txt` — `cargo test -p zz`, the integration-test rule for a
  diff that touches `crates/`.
- `tracker-check-tip.txt` — `python3 compat/tui/tracker.py check`.

- `proofs-at-tip.txt` — every command above with its exit code and the sha it ran at.

## What the fixture asserts and what it records

Every case names the channels it asserts; a channel it leaves out is printed with the
measurement behind it, so nothing is waived by omission. The recorded ones are the
view placement after a scroll, page or search; the prefix taking precedence over a
copy-table binding of the same key; the selection and position presentation; and the
search prompt's palette. `compat/tui/campaign.json` carries each measurement with its
numbers.
