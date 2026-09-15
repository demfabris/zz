# TUI-008 attempt-05: the screen under the pointer

The mouse-context lane, the ubuntu box, 2026-09-15, branched from 6fbdf4c3 (the tip of
campaign/tui-mouse-menus-2, whose menus half was reviewed in attempt-04 and gated separately).
It closes what attempt-04's review left open: `formats.mouse-context`'s three grid-reading
names, `#{mouse_word}`, `#{mouse_line}` and `#{mouse_hyperlink}`.

Pin: tmux d77c9dc6 (next-3.8) at /home/demfabris/dev/zz/compat/.cache/tmux-src/tmux.
Every compat invocation ran with ZZ_COMPAT_TMUX and ZZ_COMPAT_CORPUS pointed at that checkout's
caches; see environment.txt.

## What the pin does

`format.c`'s `format_cb_mouse_word`, `format_cb_mouse_line` and `format_cb_mouse_hyperlink` need
`ft->m.valid`, resolve the event's pane with `cmd_mouse_pane` and its cell with `cmd_mouse_at`,
and then read the pane's CURRENT grid: `format_grid_word(gd, x, gd->hsize + y)`,
`format_grid_line(gd, gd->hsize + y)` and `format_grid_hyperlink(gd, x, gd->hsize + y, wp->screen)`.
A pane with a mode up answers off the copy mode's own backing instead
(`window_copy_get_word`/`_line` index it at `gd->hsize + y - data->oy`).

- `format_grid_word` walks back to the start of the word under the cell, crossing a wrap into the
  scrollback, then collects forward to the next separator. It skips padding halves, honours
  `word-separators` (the session option, plus tab and space), and a cell that IS a separator
  collects the word that FOLLOWS it.
- `format_grid_line` is one row, trailing blanks trimmed, wraps NOT followed.
- `format_grid_hyperlink` steps left off a padding half and answers that cell's OSC 8 URI.

## The oracle (punch-list item 1)

oracle-probe.sh drives ONE side: a throwaway server, an 80x24 pane running a program that prints
the sample and parks, an outer pinned tmux that owns the terminal and injects real SGR reports with
`send-keys -H`, and `bind-key -n MouseDown3Pane set-option -gF @mc
'<#{mouse_word}|#{mouse_line}|#{mouse_hyperlink}|#{mouse_x},#{mouse_y}|#{mouse_status_line}|#{mouse_status_range}|#{mouse_pane}>'`
read back with `show-options -gqv`. A right press within `KEYC_CLICK_TIMEOUT` of the previous one
is a `SecondClick3Pane` that nothing binds - `server_client_check_mouse` resets the sequence on the
BUTTON, not on the cell - so every probe sends a left click first.

Runs: 01 (pin, default separators), 02 (pin, `word-separators` = `_x`), 03 (zz BEFORE the change),
06 (zz after), 08 (zz, `word-separators` = `_x`).

The pin's table, default `word-separators`, sample rows
`alpha beta gamma` / a 139-cell word that wraps over two rows / `LINKTEXT` under an OSC 8 link /
`pre<TAB>here post` / `wide CJK<3 wide chars> tail` / a blank row / 73 spaces then `ENDWORD` /
`dash-joined_word end`:

| cell | mouse_word | mouse_line | mouse_hyperlink | zz |
|---|---|---|---|---|
| over `beta` | `beta` | the whole row | empty | same |
| the row's first cell | `alpha` | the whole row | empty | same |
| column 80, past the text | empty | the whole row | empty | same |
| the space before `beta` | `beta` | the whole row | empty | same |
| the wrap's head | the whole 139-cell word | the head row's 80 cells | empty | same |
| the head's last column | the whole word | the head row | empty | same |
| the wrap's tail | the whole word | the tail's 59 cells | empty | same |
| over the OSC 8 link | `LINKTEXT` | `LINKTEXT` | `https://example.com/page` | same |
| past the link's end | empty | `LINKTEXT` | empty | same |
| either half of a wide cell | `CJK<3 wide chars>` | the whole row | empty | same |
| a blank row | empty | empty | empty | same |
| a word ending at column 80 | `ENDWORD` | leading blanks kept | empty | same |
| over `dash` | `dash` | the whole row | empty | same |
| over `joined_word` | `joined_word` | the whole row | empty | same |
| `word-separators` `_x`, over `dash-joined` | `dash-joined` | | | same |
| `word-separators` `_x`, over `joined_word` | `word` | | | same |
| `word-separators` `_x`, the wrap's head | `WRAPPEDHEAD` | | | same |
| `word-separators` `_x`, the wrap's tail | empty | | | same |
| a row below the content | empty | empty | empty | same |

zz answered EMPTY for all three at every one of those cells before this landing (03).

Two cells in the table do NOT agree and neither is owned by these formats:

- THE TAB. The pin's grid keeps a tab as a cell of its own (`GRID_FLAG_TAB`) and libghostty leaves
  the columns a tab skipped unwritten. Over `pre<TAB>here post` the pin answers `mouse_line` with a
  real tab byte and zz answers `prehere post`; over the tab cell itself `mouse_word` is `here` on
  the pin (the separator collects the word that follows) and empty on zz (the next cell is
  unwritten, so it is a separator too). `capture-pane -p` shows the same two grids on both
  binaries - `pre^Ihere post` against `pre` and five spaces - so this is the engines' tab storage,
  visible through a different surface, and not these readers. It is carried forward in TUI-008's
  next_action and named in the gap's own reason.
- THE STATUS ROW. `format_cb_mouse_x` and `format_cb_mouse_y` fall back to the event's own row when
  `cmd_mouse_at` fails, so a click on the status row answers `0,0` on the pin and `0,23` on zz,
  which computes from the pane rectangle. That is a residue of `format:mouse_x` and
  `format:mouse_y`, closed on 2026-09-13, not of this landing. The three names closed here agree on
  that click anyway: the daemon declines to read the grid for an event outside the pane's own
  rectangle, which is `cmd_mouse_at`'s test. `#{mouse_status_line}` and `#{mouse_status_range}` are
  measured at the same clicks and STAY OPEN: the pin answers `0`/`left` on status-left,
  `0`/`window` on a window's name and `0`/empty on status-right, and zz answers empty for both at
  all three.

## The code (punch-list items 2 and 3)

- crates/zz-terminal/src/session.rs: the three `mode_format_*` readers the frozen copy-mode
  revision used became `format_grid_*` over a `FormatGrid` trait - the rows, the cells, the widths
  and the wrap flag and nothing else. `ModeRevision` implements it, so copy mode reads exactly what
  it read before; a new `LiveGrid` implements it over the live terminal through
  `Point::Screen`, which is `gd->hsize + y` addressing and lets the word walk cross a wrap into the
  scrollback. `pointer_context` answers all three for one cell, off the copy mode's revision when
  the pane has one and off the live grid otherwise, with the OSC 8 URI read through the same
  `hyperlink_uri_bytes` the hover path uses (split into a point-addressed twin). It is reached
  synchronously through `Command::PointerContext` and `TerminalSession::pointer_context`, the same
  request/reply shape `capture` uses, with the same timeout.
- crates/zz-daemon/src/daemon.rs: `mouse_format_variables` hands back the pane worker and the
  event's cell alongside the variables it already published, and the mouse-invoked command path
  takes the worker read OUTSIDE the server lock, so the three names reach the command's format tree
  beside `mouse_pane`, `mouse_x` and `mouse_y`. No probe leaves for an event outside the pane's
  rectangle.
- crates/zz-daemon/src/status.rs: the delegated-format hook consumes the three names, the way it
  consumes `mouse_pane`, `mouse_x` and `mouse_y`.
- crates/zz-mux/src/formats.rs: the three registrations moved from the constant backing to the
  delegated one, with compat_manifest_tests.rs's partition counts (43/99/56, 155 nonconstant) in
  the same commit as the gap items.
- The GUI is untouched: no hunk under crates/zz, and `cargo test -p zz` is green.

Unit tests in zz-terminal, one per row of the oracle table: `pointer_formats_*`,
`pointer_word_crosses_a_wrap_where_the_line_does_not`,
`pointer_hyperlink_answers_the_osc_8_uri_of_the_cell`,
`pointer_word_reads_the_same_text_from_both_halves_of_a_wide_cell`,
`pointer_line_keeps_leading_blanks_and_trims_trailing_ones`,
`pointer_word_honours_the_word_separators_option`.

## The fixture (punch-list item 4)

compat/tui-mouse.sh gains two cases and six sabotages, and keeps
right-click-pane/screen at its blank cell:

- `right-click-pane/over-a-word`, ASSERTED: the identical stock gesture as right-click-pane/screen
  (`send-keys -H`, a real `\e[<2;col;rowM`, `capture-pane -p -e`, 80x24) at a cell whose row
  carries text. The pin's `DEFAULT_PANE_MENU` renders `Copy Line` and its three word items off
  these names there, so the menu is fourteen rows where the blank-cell menu is twelve. Channel: the
  whole decoded screen.
- `mouse-context/over-a-word`, `/wrapped-head`, `/wrapped-tail`, `/hyperlink` and `/blank-cell`,
  ASSERTED: `bind-key -n C-MouseDown3Pane set-option -gF @mousectx
  '[#{mouse_word}][#{mouse_line}][#{mouse_hyperlink}]'` read back from a real SGR report with the
  control bit (button 18). A key of its own, bound by neither binary, so the stock
  `MouseDown3Pane` the two menu cases need stays exactly as `key-bindings.c` installed it. The
  brackets keep the value non-empty whatever the names answer, so "the binding ran" and "the
  binding answered something" stay different questions.

Sabotages, each reporting in its own channel: zz's own `word-separators` under the pane menu (the
menu's word items change and `Copy Line` stays on both, so the case's waits are unmoved), and one
per format channel, that probe alone aimed at a different cell on zz.

The menu case's sample deliberately carries NO hyperlink row. Measured here: the pin's client
re-emits the OSC 8 sequence when it draws that cell (`^[]8;id=tmux1;https://example.com/page^[\`
around the text) and the raw TUI draws the underline without it, so a screen-channel case whose
sample carries a link compares that and not the menu. That is the raw TUI's cell writer
(crates/zz-tui), outside this obligation's zones; it is carried forward in next_action. The format
probes compare an option's value rather than the screen, so they keep the link row and assert the
URI.

The tab row is absent from both samples for the reason above.

## Punch-list item 6, the backward emacs word selection

Left parked, and it is not this lane's code: the pin copies `beta` where zz copies `bet` under
emacs mode-keys, and that is decided by copy mode's own cursor readers
(`window_copy_cursor_next_word_end` and the `reader_*`/`revision_*` family in session.rs), not by
`format_grid_word`, which this lane generalised. Nothing in this diff moves it.

## Files

- environment.txt - the box, the binaries, the pin, RUN_ENV
- oracle-probe.sh - the one-sided oracle, re-runnable
- 01-oracle-pin-default-separators.txt, 02-oracle-pin-word-separators-underscore-x.txt
- 03-oracle-zz-before.txt (zz at 6fbdf4c3: empty everywhere)
- 06-oracle-zz-after.txt, 08-oracle-zz-word-separators-underscore-x.txt
- 04-cargo-test-zz-terminal.txt, 05-clippy-zz-terminal.txt
- 07-clippy-zz-daemon.txt, 11-cargo-test-zz-mux.txt, 12-clippy-zz-mux.txt
- 13-cargo-test-zz-daemon.txt (the run that caught the delegated-consumer count; the tip run is 25)
- 09-tui-mouse-run-1.txt, 10-tui-mouse-self-check.txt (taken while the fixture was being built)
- 14-tui-copy-mode.txt, 15-tui-copy-mode-self-check.txt
- 16-tui-caps.txt, 17-tui-stock-keys.txt, 18-tui-stock-keys-self-check.txt
- 19-attached-client.txt
- 20-corpus-delta-formats.txt, 21-corpus-delta-menus.txt
- 22-cargo-test-zz.txt, 23-verify-claims-TUI-008.txt, 24-compat-check.txt
- 25-*, 26-*: the proofs re-taken at the final tip
