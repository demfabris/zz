# TUI-004 attempt-03: clause 3 closed whole

Cycle 5, the modes lane. Attempt-01 (cycle 3) holds clauses 1 and 2 and the
proof block; attempt-02 (cycle 4) holds the theme arm and the prefix and
message-cursor fixes. This attempt closes what cycle 4's gate held the record
on: the copy and view indicators asserted nothing, and the message and prompt
styles were recorded.

> Compare copy/view/prefix indicators and message/prompt restoration without
> waiving cells occupied by zz hints. Turn the three recorded theme differences
> into asserted comparisons.

Every run here is at code revision f9bb4e53 with a clean tree, against the
binary whose sha256 is in `environment.txt`. The commit that adds this directory
changes nothing but these files.

## The files

| file | what ran |
|---|---|
| `environment.txt` | both binaries' sha256, the revision and its clean state, the pin, the OS line, TERM, shell, bash, locale. Read this first. |
| `proofs-at-tip.txt` | every proof command with its exit code, in the order they ran. Read this second. |
| `indicators-run-1.txt`, `indicators-run-2.txt`, `indicators-run-3.txt` | `compat/tui-indicators.sh`, three consecutive runs: exit 0, all 16 asserted comparisons identical, 0 recorded. |
| `indicators-self-check.txt` | `compat/tui-indicators.sh --self-check`: exit 0, seven sabotages caught, the equivalence passed. |
| `status-row-tip.txt` | `compat/status-row.sh` under `LC_ALL=C LC_TIME=C`: exit 0, 14 of 14 identical, none recorded. |
| `screen-diff-tip.txt`, `screen-diff-self-check.txt` | `compat/tui-screen-diff.sh` and its `--self-check`: exit 0, 111 asserted checkpoints identical, 42 recorded; every sabotage caught. |
| `pane-geometry-tip.txt` | `compat/tui-pane-geometry.sh`: exit 0, 6 of 6. |
| `stock-keys-tip.txt` | `compat/tui-stock-keys.sh`, run solo: exit 0, all 50 cases agree on every asserted channel. |
| `copy-mode-regression-tip.txt` | `compat/tui-copy-mode.sh` as committed: exit 0, all 83 cases agree. A regression check: it is the fixture that found the refresh cost described below. |
| `copy-mode-throwaway-flip.txt` | A THROWAWAY FLIP, not a committed fixture: `compat/tui-copy-mode.sh` with every case recorded under `POSITION_REASON`, `SELECTION_REASON` or `PROMPT_REASON` asserted on all six channels. Exit 0, all 83 cases agree. The gate uses it to predict the copy lane's SIBLING:modes flips. |
| `copy-mode-throwaway-flip-diff.txt` | `diff -u` between the committed fixture and that throwaway copy: the nine flipped `copy_case` lines and the `COMPAT_DIR` pin. |
| `zz-protocol-unit-tests.txt`, `zz-mux-unit-tests.txt`, `zz-tui-unit-tests.txt`, `zz-daemon-unit-tests.txt` | `cargo test -p <crate> --jobs 3 -- --test-threads=2` for every touched crate. |
| `zz-integration-tests.txt` | `cargo test -p zz --jobs 3 -- --test-threads=2`, the integration-test rule. |
| `clippy-tip.txt` | `cargo clippy -p zz-protocol -p zz-mux -p zz-daemon -p zz-tui --all-targets --all-features --jobs 3 -- -D warnings`. |
| `trackers-tip.txt` | `python3 compat/tui/tracker.py check` and `python3 compat/tmux-tracker.py check`. |

## What changed, surface by surface

**Copy-mode position.** window_copy_write_line (window-copy.c:5228) draws
`copy-mode-position-format` into the pane's first row at the right in
`copy-mode-position-style`. The raw TUI painted a ` COPY 62/62 ` badge on the
status row instead. The daemon now expands the format per client against the
pane's mode, binding `#{copy_position}` and `#{copy_position_limit}` to the
view's oy and history size for that expansion, and publishes it on `StatusLine`
as `modes: Vec<ModePresentation>` with the two resolved styles. The raw TUI
draws exactly the cells the format writes over the pane's first row, and the
status row carries no badge.

**Selection.** Selected cells take `copy-mode-selection-style` the way
`screen_select_cell` merges it: the style's colours win where they are set,
and the cell's own colours show through where they are default. For emacs keys
the TUI drops the bottom-right-most cell of a non-rectangle selection, as
`screen_check_selection` does, working from the selection spans and the copy
cursor. `ModePresentation.vi_keys` tells it which table applies.

**Message and prompt.** `status_message_redraw` and `prompt_draw` draw through
`format_draw` in `message-style`. When the style names a fill, the whole row is
cleared to that colour with the default foreground; with no fill, the row
underneath stays. `StatusLine` now carries `message_style` and
`message_command_style` resolved per client, read by the daemon straight from
the session knobs. The TUI paints the message row, the command prompt and the
view-search prompt over the row underneath by that rule.

**View surface.** run-shell output reaches the pin as view mode in the target
pane (cmd-run-shell.c:103). It opens at its top, `[78/78]` for 101 lines in 23
rows, with the cursor at the pane's origin; q or Enter leaves it. The raw TUI
drew a full-screen overlay with a ` command output ` rule. It now draws the
output inside the target pane's rectangle, sized to it, with the same position
cells. `blit_row` also clears a trailing run of ten or more blank default cells
with ECH, the way `tty_draw_line` does, which is what makes the row under
`[0/0]` decode the same on both sides.

**Refresh cost, found and fixed.** Keying the refresh on the whole TerminalMode
re-rendered the full status line on every copy-mode keystroke. That throttled
key input enough that the copy fixture's text-only settles caught zz mid-typing
in its search and rectangle cases. Mode changes now refresh only the modes,
from narrow facts and live option reads, on the existing status sampler thread.
They are keyed on the mode kind, the scrollbar and the search status. The
failing outputs from before this fix were scratch runs and are not committed;
`copy-mode-regression-tip.txt` is the committed run at the tip.

## The fixture

`compat/tui-indicators.sh`, 80x24, the outer-pinned-tmux driver.

It now asserts every case it runs, including three new long-output view cases:
`view-long-shown`, `view-long-down` and `view-long-restored`.

`--self-check` adds four sabotages to cycle 4's three, one per new channel:
- one side's `copy-mode-position-style` changed in copy mode
- one side's `copy-mode-position-format` changed on the view surface
- one side's `message-style` changed under a message
- one side's `message-style` changed under the prompt

Each sabotage sets its own option after attaching. Killing the last session
restarts either server, so a server-global option does not survive into the
next case. The equivalence case still has to report nothing.
