# TUI-004 attempt-05: window sizing under status rows, and the styled trim

Cycle 6, the mux lane, punch list items 1 to 3, plus one divergence found on
the way. Base `origin/main` 33ecbd86. Every `*-tip.txt` run here is at code
revision 451cf194 with a clean tree, against the binary whose sha256 is in
`environment.txt`. The commit that records this directory changes nothing but
evidence and the ledger.

## What asserts, and what remains

Asserts at the tip: `tui-screen-diff.sh` 135 asserted checkpoints identical
with 18 recorded, three runs with identical dispositions, `--self-check` green
with two new sabotages; `tui-pane-geometry.sh` 6 of 6; `status-row.sh` 14 of 14
under `LC_ALL=C LC_TIME=C` with none recorded; `tui-indicators.sh` 23 of 23
with 0 recorded; `attached-client.sh` PASS; `cargo test` green for zz-mux,
zz-daemon and zz; clippy clean on both touched crates.

What remains, and it is NOT inside a fixture: a client shorter than five rows
whose status block would leave the window a single row. See "Found on the way"
below.

The two checkpoints this lane owned flipped. `status-two-rows` and
`status-top` were recorded at 80x10 and 80x6 and now assert at all six sizes;
`styled-left-trim` was recorded at every size and now asserts at every size.
That is four plus six checkpoints moved from recorded to asserted, which is
125 asserted and 28 recorded at BASE against 135 and 18 here.

The 18 that remain are three cases at six sizes each, and none of them is
inside TUI-004's clause 2:

- `default-fg`, TUI-009's default-foreground divergence (its `next_action`
  names it: zz writes an explicit RGB foreground where the pin leaves the
  foreground default, and the decision belongs with
  `presentation:tui-status-row-theme-defaults`).
- `colour-classes`, TUI-009 clause 3's named/indexed/RGB divergence.
- `cursor-style-request`, the cursor channel: the pin forwards a DECSCUSR its
  pane asked for and zz's wire has nowhere to carry the request.

Their reasons in the fixture are untouched.

## Item 1: window sizing under status rows

The pin: `options.c` `options_push_changes` calls `recalculate_sizes()` after
every option write, `resize.c` `clients_calculate_size` takes each client's
`cy = loop->tty.sy - status_line_size(loop)`, and `recalculate_size` resizes
the window unless its `window-size` is manual. zz had the same subtraction in
`interactive_client_window_extent`, but nothing on the option path reached it:
the window's extent came from `set_pane_geometry` back-solving the layout from
the ACTIVE pane's reported geometry, and the active pane keeps its height when
the status block grows, so the row the pin takes came out of the client's
screen and not out of the layout.

What landed:

- `crates/zz-mux/src/status.rs` gains `StatusFormats::rows`, the status block's
  row count, `status_line_size`'s value.
- `crates/zz-mux/src/command.rs` `set_status_option` compares that count across
  the write and emits `MuxEffect::StatusRowsChanged` beside
  `StatusFormatsChanged` when it moved, through the new
  `status_option_execution`. Both the set and the unset path go through it.
- `crates/zz-mux/src/command.rs` gains `MuxEngine::resize_window_to_extent`: it
  refuses a manual window, no-ops when the extent already matches, and
  otherwise goes through `state.resize_window`, which clears
  `last_extent_probe`, so the next client report back-solves from the new
  layout instead of holding the old one.
- `crates/zz-daemon/src/daemon.rs` handles the effect on the paths that size an
  interactive client's windows. `attached_client_window_extent` is split so its
  candidate loop and its ceiling clamp are shared with a new
  `measured_client_window_extent`, which differs in one way that matters: it
  returns `None` when no attached client measured anything, where
  `attached_client_window_extent` falls back to `default-size`. The pin's
  `recalculate_size` does not resize a window no client sized, so the
  recalculation must not either. `recalculate_window_extents` then walks the
  session's windows at each window's own `window-size` and resizes, and the arm
  pushes the terminal resizes for whatever moved. It deliberately does NOT call
  `write_back_terminal_geometries`: that would back-solve from the client's
  stale report and undo the resize.
- `interactive_client_window_extent` now reads the row count through
  `StatusFormats::rows` instead of spelling the `enabled`/`lines` conditional
  out again.

Only the raw TUI reaches the client-size branch: `client_sizes` is written from
`InputMessage::ClientTerminalSize`, which `crates/zz-tui/src/app.rs` alone
sends, and from the `client-size-v1:` hello fact, which
`crates/zz-daemon/src/client.rs` attaches only for a `terminal_surface` client.
A GUI client measures through `terminal_geometries`, whose back-solve returns
the extent it already had, so `resize_window_to_extent` no-ops and the GUI's
presentation does not move.

`pin-and-zz-status-sizing.txt` drives both binaries from one outer pinned tmux
pane of exactly 80 columns by 10, 6 and 24 rows through the fixture's sequence
(status off, status on, split, status 2, position top, position bottom, status
on) and prints every pane height and window extent. Both sides agree on all 42
lines. The numbers the fixture's header quotes come from this file.

`crates/zz-daemon/src/daemon.rs`
`status_rows_resize_every_window_of_an_interactive_clients_session` is the unit
test: an interactive client at 80x10 with a split window at extent 9 and panes
4 and 4, `status 2` takes it to 8 with 3 beside 4, `status-position` moves
nothing, and `status on` restores 9 with 4 beside 4. Each number is the pin's
from the probe above.

## Item 2: styled-left-trim

`format-draw.c` `format_trim_left` and `format_trim_right` walk `format_width`'s
units: a `#[...]` section is copied through and costs the limit nothing, a run
of `#`s costs the columns it would draw once escaped (`format_leading_hashes`),
a UTF-8 character costs its display width, and a byte that opens no sequence
and is not printable ASCII costs nothing and is dropped. zz's `truncate_value`
counted every printable byte, so `#{T;=/#{status-left-length}:status-left}`
answered `#[fg=red,b` where the pin answered `#[fg=red,bold]LEFT`, and the
unterminated marker took the whole status-left band off the row.

`crates/zz-mux/src/formats.rs` now has `format_trim_left` and
`format_trim_right` written against those two functions, on the same
`leading_hashes` and `find_measured_style_end` helpers `styled_display_width`
(the pin's `format_width`, for the `W` modifier) already used. `truncate_value`
calls them and keeps the marker rule from `format.c`: the marker is appended
only when the trim changed the value. A right trim still hands back the whole
string when `format_width` does not exceed the limit, which includes the case
where an unterminated section makes `format_width` answer zero.

`pad_value` was left on `format_byte_width`, which is the plain walk, because
the pin pads through `utf8_padcstr` and `utf8_cstrwidth`, neither of which
knows about style sections.

`pin-and-zz-styled-trim.txt` asks both binaries the same twelve modifiers over
six values (`#[fg=red,bold]LEFT`, a value with two sections in the middle,
`####AB`, `###[fg=red]AB`, a section before three wide characters, and an
unterminated `#[fg=red`), with and without a `...` marker, left and right: 72
answers, 72 agree. 27 of them are pinned in the zz-mux unit test
`a_style_section_costs_a_trim_no_column_the_way_format_trim_left_does`.

## Found on the way: a window the pin takes down to one row

Probing item 1 at heights below the fixture's smallest (`80x6`) turned up a
divergence no punch-list item names. `pin-and-zz-short-client.txt` walks one
pane at 80 columns by 6, 5, 4, 3 and 2 rows against `status` 1 to 4 on both
binaries: 16 of the 20 agree, and the 4 that differ are exactly the rows where
the pin's window would come out one row tall (rows=5 status=4, rows=4 status=3,
rows=3 status=2, rows=2 status=1). zz answers the client's full height there.

Half of it was the daemon's, and landed. `resize.c` `recalculate_sizes_now`
raises `CLIENT_STATUSOFF` when `c->tty.sy <= s->statuslines`, so
`status_line_size` answers 0 and the window keeps every row; zz clamped the
subtraction to `rows - 1` instead, which took a row away where the pin takes
none. `interactive_client_window_extent` now carries the pin's rule, which
converged rows=3 status=3, rows=2 status=2 and rows=4 status=4 (zz answered 1
at each and now answers the pin's 3, 2 and 4), pinned by the zz-daemon unit
test `a_client_no_taller_than_its_status_block_keeps_every_row` against the
probe's numbers at a 3-row client. It changes nothing at any height a fixture
drives: it only fires when the client is no taller than its status block.

The other half is not this lane's. zz never takes a window down to a single
row: the raw TUI reports a full-height pane at those heights and
`set_pane_geometry` back-solves the extent from that report, so the four rows
above stay open. Closing them needs a rule in `crates/zz-tui`, which this
lane's zones exclude, and no compat fixture drives a client shorter than six
rows, so there is nowhere inside this lane's fixture zone to record it as a
case. `rows=2 status=1` also diverges at BASE and is untouched by this branch:
`status 1` is the value a fresh session already holds, so no status write
happens and no resize is attempted.

## The two sabotages

Both are in `compat/tui-screen-diff.sh`'s `--self-check`, and both work the
same way: give the two sides the same input and put one side back into the
behaviour that landed away.

- `status rows, one side keeps the window a row taller`. At 80x10 the upper
  pane is filled with eight lines before the split, both sides take
  `status 2`, and zz alone is pinned back to the window height it had with
  `resize-window -y 9`, which makes the window manual and holds it there. The
  upper pane's viewport is then one row taller than the box the client paints
  it into, and zz's first row reads FILL-6 where the pin's reads FILL-7. The
  fill is what makes it visible: without it, a pane whose content is shorter
  than either box shows the same cells at both heights, and the first version
  of this sabotage passed with no row difference at all
  (`screen-diff-self-check-item1-first-sabotage.txt`).
- `styled trim, one side keeps what a byte count leaves`. Both sides get
  `status-left = '#[fg=red,bold]LEFT'` and zz is then given `#[fg=red,b`, the
  value a byte-counting trim left behind.

## Files

- `environment.txt` — box, revision, both binaries' hashes, server hygiene.
- `pin-and-zz-status-sizing.txt` — item 1's probe, both binaries, three sizes.
- `pin-and-zz-styled-trim.txt` — item 2's probe, 72 answers, both binaries.
- `pin-and-zz-short-client.txt` — the short-client probe, 20 rows, both
  binaries, and what each of the four open ones needs.
- `screen-diff-tip-1.txt`, `screen-diff-tip-2.txt`, `screen-diff-tip-3.txt` —
  three runs at the tip, identical dispositions, 135 asserted and 18 recorded.
- `screen-diff-self-check-tip.txt` — `--self-check` at the tip, 16 expectations
  met, both new sabotages caught in the rows channel.
- `screen-diff-item1.txt` — the run after item 1 only, 129 asserted and 24
  recorded: the four status-row checkpoints flipped, styled-left-trim not yet.
- `screen-diff-self-check-item1-first-sabotage.txt` — the status-rows sabotage
  before the fill was added, failing for the reason described above.
- `pane-geometry-tip.txt`, `status-row-tip.txt`, `indicators-tip.txt`,
  `attached-client-tip.txt` — the other four proof surfaces at the tip.
- `zz-mux-tests-tip.txt`, `zz-daemon-tests-tip.txt`, `zz-tests-tip.txt` —
  `cargo test` for the two touched crates and for zz (cli_binary).
- `clippy-tip.txt` — clippy for the two touched crates.
- `trackers-tip.txt` — `tracker.py check` and `tmux-tracker.py check`.

## Not this branch's

`status-row.sh` under the box locale (`LC_TIME=pt_BR.UTF-8`) still differs on
`%b`: the pin expands it through libc `strftime` and zz through
locale-independent chrono. TUI-001 records it and it is environmental; the
`LC_ALL=C LC_TIME=C` control exits 0.

Nothing on the wire moved. `PROTOCOL_VERSION` is untouched, no message gained a
field, and `crates/zz-tui` has no change.
