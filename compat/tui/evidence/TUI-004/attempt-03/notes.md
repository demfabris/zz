# TUI-004 attempt-03: clause 3 closed whole

Cycle 5, the modes lane. Attempt-01 (cycle 3) holds clauses 1 and 2 and the
proof block; attempt-02 (cycle 4) holds the theme arm and the prefix and
message-cursor fixes. This attempt closes what cycle 4's gate held the record
on: the copy and view indicators asserted nothing, and the message and prompt
styles were recorded.

> Compare copy/view/prefix indicators and message/prompt restoration without
> waiving cells occupied by zz hints. Turn the three recorded theme differences
> into asserted comparisons.

The first push of this attempt (code f9bb4e53, evidence 5a85f44e) was rejected
in review over one blocker and two must-fixes. This directory was rewritten for
the rework: every run here is at code revision 64204561 with a clean tree,
against the binary whose sha256 is in `environment.txt`. The commit that adds
these files changes nothing but these files.

## What the review found, and what changed

**The blocker: a view on another pane swallowed every key.** f9bb4e53 moved the
command output into its target pane's rectangle but left it client-modal. The
TUI sent every key to it whenever one existed, and the daemon swallowed every
passed key for a client that had one. So `run-shell -t` on a pane in a window
the client was not showing (the reviewer's R9) left a surface that took every
key and drew nothing. That was a regression against origin/main, whose
full-screen overlay was at least visible. The same happened for a visible pane
that was not the active one (R10). The pin's view mode belongs to the pane:
server_client_handle_key takes a mode's key table only from the active pane, so
keys typed meanwhile run in the active pane.

Now:
- `Model::command_output_focus` returns the output's pane only while it is the
  active pane of the current window.
- `input.rs` routes keys, paste and mouse to the output only then. `render.rs`
  shows the view-search prompt and its cursor only then, and `mode_indicators`
  follows the same rule.
- In the daemon, a passed key is swallowed only when it targets the output's
  pane (`command_output_owns_pane`).
- The client's key table is handed back while keys target another pane
  (`follow_command_output_focus`, `CommandOutputSession.parked`). The copy
  table goes back on when a key targets the output's pane again. The GUI sends
  its keys to the output's pane, so it never parks and keeps its behaviour.

**Must-fix: the message-command-style sentence.** The options.native-overlay-styles
reason said the raw TUI paints a command-mode prompt in message-command-style.
It does not. The field is published on StatusLine and not consumed, because
CommandPromptState carries no command-mode flag. The reason now says so, with
the reviewer's measurement: the pin paints `:abc` in message-command-style
with the cursor at 3,23, and the raw TUI keeps message-style at 4,23. That
prompt is the overlays lane's gap.

**Must-fix: no committed fixture drew a selection.** `tui-indicators.sh` now
asserts three: the emacs table with the default style, the vi table, and
`copy-mode-selection-style bg=red,fg=white,bold` set on both sides. The
options.native-mode-styles closure cites them.

## The files

| file | what ran |
|---|---|
| `environment.txt` | both binaries' sha256, the revision and its clean state, the pin, the OS line, TERM, shell, bash, locale. Read this first. |
| `proofs-at-tip.txt` | every proof command with its exit code, in the order they ran. Read this second. |
| `indicators-run-1.txt`, `indicators-run-2.txt`, `indicators-run-3.txt` | `compat/tui-indicators.sh`, three consecutive runs: exit 0, all 23 asserted comparisons identical, 0 recorded. |
| `indicators-self-check.txt` | `compat/tui-indicators.sh --self-check`: exit 0, ten sabotages caught, the equivalence passed. |
| `status-row-tip.txt` | `compat/status-row.sh` under `LC_ALL=C LC_TIME=C`: exit 0, 14 of 14 identical, none recorded. |
| `screen-diff-tip.txt`, `screen-diff-self-check.txt` | `compat/tui-screen-diff.sh` and its `--self-check`: exit 0, 111 asserted checkpoints identical, 42 recorded; every sabotage caught. |
| `pane-geometry-tip.txt` | `compat/tui-pane-geometry.sh`: exit 0, 6 of 6. |
| `stock-keys-tip.txt` | `compat/tui-stock-keys.sh`, run with nothing else beside it: exit 0, all 50 cases agree on every asserted channel. |
| `copy-mode-regression-tip.txt` | `compat/tui-copy-mode.sh` as committed: exit 0, all 83 cases agree. |
| `copy-mode-throwaway-flip.txt` | A THROWAWAY FLIP, not a committed fixture: `compat/tui-copy-mode.sh` with every case recorded under `POSITION_REASON`, `SELECTION_REASON` or `PROMPT_REASON` asserted on all six channels. Exit 0, all 83 cases agree. The gate uses it to predict the copy lane's SIBLING:modes flips. |
| `copy-mode-throwaway-flip-diff.txt` | `diff -u` between the committed fixture and that throwaway copy: the nine flipped cases (eight one-line `copy_case` calls and the POSITION case's continuation line) and the `COMPAT_DIR` pin. |
| `zz-protocol-unit-tests.txt`, `zz-mux-unit-tests.txt`, `zz-tui-unit-tests.txt`, `zz-daemon-unit-tests.txt`, `zz-daemon-integration-tests.txt` | `cargo test -p <crate> --jobs 3 -- --test-threads=2` for every crate the branch touches. The daemon's library and its integration binaries ran as two calls. |
| `zz-integration-tests.txt` | `cargo test -p zz --jobs 3 -- --test-threads=2`, the integration-test rule. |
| `clippy-tip.txt` | `cargo clippy -p zz-protocol -p zz-mux -p zz-daemon -p zz-tui --all-targets --all-features --jobs 3 -- -D warnings`. |
| `trackers-tip.txt` | `python3 compat/tui/tracker.py check` and `python3 compat/tmux-tracker.py check`. |

## What changed, surface by surface

**Copy-mode position.** window_copy_write_line (window-copy.c:5228) draws
`copy-mode-position-format` into the pane's first row at the right in
`copy-mode-position-style`. The raw TUI painted a ` COPY 62/62 ` badge on the
status row instead. The daemon now expands the format per client against the
pane's mode, binding `#{copy_position}` and `#{copy_position_limit}` to the
view's oy and history size for that expansion. It publishes the result on
`StatusLine` as `modes: Vec<ModePresentation>`, together with the two resolved
styles. The raw TUI draws exactly the cells the format writes over the pane's
first row, and the status row carries no badge.

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
`message_command_style`, resolved per client and read by the daemon straight
from the session knobs. The TUI paints the message row, the command prompt and
the view-search prompt over the row underneath by that rule.
`message_command_style` is not consumed yet (see above).

**View surface.** run-shell output reaches the pin as view mode in the target
pane (cmd-run-shell.c:103). It opens at its top, `[78/78]` for 101 lines in 23
rows, with the cursor at the pane's origin; q or Enter leaves it. The raw TUI
used to draw a full-screen overlay with a ` command output ` rule. It now draws
the output inside the target pane's rectangle, sized to it, with the same
position cells, and takes keys only while that pane is the active one.
`blit_row` also clears a trailing run of ten or more blank default cells with
ECH, the way `tty_draw_line` does, which is what makes the row under `[0/0]`
decode the same on both sides.

**Refresh cost, found and fixed in the first push.** Keying the refresh on the
whole TerminalMode re-rendered the full status line on every copy-mode
keystroke. That throttled key input enough that the copy fixture's text-only
settles caught zz mid-typing in its search and rectangle cases. Mode changes
now refresh only the modes, from narrow facts and live option reads, on the
existing status sampler thread. The refresh is keyed on the mode kind, the
scrollbar and the search status.

## The fixture

`compat/tui-indicators.sh`, 80x24, the outer-pinned-tmux driver. It asserts
every case it runs, 23 in all.

New in the rework:
- `selection-emacs`, `selection-vi`, `selection-styled`: the selection drawn
  with send-keys -X (up to the marker line, to its start, begin, four cells
  right). The observable is each side's styled screen changing.
- `view-hidden-opened`, `view-hidden-typed`: R9. A second window named `hid`,
  run-shell on its pane, then `printf 'TYPED-%s\n' HIDDEN` typed through each
  client. Compared whole.
- `view-inactive-opened`, `view-inactive-typed`: R10. A `-v` split, run-shell
  on the inactive pane, the same typing. Every row is compared whole except the
  divider row, which is compared by glyph. In this driver a plain split's
  default border colours differ with or without a view, at origin/main
  3319ceba and at 2911d88c alike (the modes review's probe6 and probe7): the
  pin draws `\e[38;2;179;179;179m` then `\e[38;2;154;205;50m` on the default
  ground, zz draws `\e[38;2;216;222;233m\e[48;2;16;19;24m` then
  `\e[38;2;77;163;235m`. Even `pane-border-style fg=colour2` set on both sides
  gives `\e[32m\e[49m` against `\e[38;2;0;205;0m\e[48;2;16;19;24m`. That is the
  mechanism `BORDER_STYLE_REASON` in tui-screen-diff.sh describes, but no
  existing record covers this case: tui-screen-diff.sh's plain `split`
  checkpoint asserts identical and does not reproduce it. Why the two drivers
  disagree is not explained yet; it is raised for clause-2 triage (corrected by
  the cycle-5 gate). The row below the divider is captured on its own, because
  capture-pane -e carries the divider's SGR into that row's leading escapes.
  The header's CONTROLLED DYNAMIC VALUES declares both rules.

Each view case waits for the pin's pane to be in view mode and for zz's client
key table to read a copy table, which zz sets when it opens an output, before
anything is typed.

`--self-check` catches ten one-sided sabotages and passes the equivalence.
Three of the ten are new in the rework:
- one side selecting with the vi table
- one side opening the view on the ACTIVE pane of a hidden-window setup, where
  the typed keys do go into the view
- the same in a split, through the divider-aware comparison

Each sabotage sets its own option after attaching.

## Throwaway probes that are not committed

The rework's experiments ran as scratch copies of the fixture, not as proofs:
the border style with the default and with explicit styles, and the plain
split with the driver's attach options removed. Each confirmed that the split's
border colours differ in this driver regardless of the view. They are
described here only so the divider rule's reason can be traced.
