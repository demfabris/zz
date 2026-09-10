# TUI-009 attempt-01 — outer-terminal capabilities and fidelity

Cycle 4, the caps lane. Everything here was measured on alienware against
pinned tmux d77c9dc6 with both binaries attached inside one outer pinned tmux.
No Rust source changed on this branch: every finding needs a change outside
TUI-009's zones, and each one names the field or behaviour it would need.

## What landed

`compat/tui-caps.sh`, a new fixture. It copies the outer-pinned-tmux driver
shape (two windows in one outer pinned tmux, isolated `HOME` and
`XDG_CONFIG_HOME` per side, short `/tmp` sockets, bounded `wait_for` on an
observable, settle = a marker on the screen AND the screen unchanged between
two polls) and compares the half of the client/terminal contract that
`tui-screen-diff.sh` does not see: the modes a client arms in its outer
terminal, the capabilities it negotiates, the client facts its own daemon
records, and the colour class it puts on the wire.

The outer pinned tmux is the decoder here too. It decodes each inner client's
mode-setting sequences into its own pane state and publishes them as formats,
so spelling, order and batching collapse on both sides while the mode a side
did or did not arm does not. `#{pane_key_mode}` is the extended-key channel and
`capture-pane -p -e` is the colour-class channel.

63 asserted rows, 41 recorded rows, three green runs with identical row
dispositions. Every recorded row is explained in the fixture header, in
`09-capability-matrix.txt` and below; none is silently absent.

## The three findings

**1. `-u` is parsed and dropped.** The pin sets `CLIENT_UTF8`, so
`#{client_utf8}` goes 0 to 1 and `#{client_flags}` gains `UTF-8`. zz's CLI
accepts the flag at `crates/zz/src/lib.rs` `'2' | 'q' | 'u' | 'v' => {}` and
nothing happens. `-2` and `-T` are dropped the same way (`-T` reads its
argument and discards it), and on the pin `-T sixel` really does add `sixel` to
`#{client_termfeatures}`. Applying any of the three means the client telling the
daemon something about its terminal, and nothing on the wire says it today:
zz's daemon computes UTF-8 from the client's ENVIRONMENT
(`client_uses_utf8`: `TMUX` set, else `LC_ALL`/`LC_CTYPE`/`LANG` containing
utf-8), which is the pin's fallback without the flag that overrides it, and
builds the whole feature roster from `TERM` and `COLORTERM`
(`client_colour_count`, `client_term_features`). One bool on the attach message
fixes `-u`; a client-published feature list fixes `-2` and `-T`. Both are
`crates/zz-protocol` plus `crates/zz-daemon`, outside this obligation's zones,
and the first is a wire change. `-q` and `-v` are correct as they stand: `-q` is
a no-op in the pin too and `-v` is its log level. Full measurement in
`08-client-flag-dispositions.txt`.

The other half of that measurement says the gap is the flag and not the rule
behind it. Attached under `LANG=C.UTF-8` and `LC_ALL=C.UTF-8` with no flag at
all, BOTH sides come up UTF-8: `client_utf8` 1 and `client_flags` gaining
`UTF-8` on the pin and on zz alike, asserted in `facts/utf8-locale`. zz already
implements the pin's locale fallback exactly; what it is missing is the flag
that overrides it.

**2. zz gets no extended keys from a tmux-class outer terminal.** Under
`extended-keys on` the pin arms the Eneks capability, `\e[>4;2m`, and the
decoder records `Ext 2`. zz arms the Kitty keyboard protocol, `\e[>3u`, and the
pin's CSI dispatch table carries exactly one `'u'` entry with no intermediate
(`{ 'u', "", INPUT_CSI_RCP }`), so the request is ignored and the decoder
records `VT10x`. Meeting the pin means also arming modifyOtherKeys and decoding
the `\e[27;<mod>;<key>~` form it produces, which
`crates/zz-tui/src/terminal_event.rs` does not do — it decodes only the Kitty
`\e[<key>;<mod>u` form. `terminal_event.rs` is not in TUI-009's zone list, so
this is named rather than changed.

Two things narrow it. Under DEFAULT options the two sides agree: `extended-keys`
is `off` on both binaries, the pin arms nothing, and the fixture's legacy case
asserts `pane_key_mode` VT10x on both. The gap opens only for a user who turns
`extended-keys on`, and it opens the other way round from the usual shape — zz
is not missing a decode, it is asking for extended keys in a dialect the pin
does not speak. And `set -s extended-keys on` on zz visibly changes nothing,
which is the accepted stance on `options.client-terminal-negotiation` (the
option is store-only and the raw TUI arms its own fixed sequence set) confirmed
rather than contradicted; what this adds to that stance is the consequence,
which the gap does not record: on a tmux-class outer terminal zz's `\e[>3u` is
a no-op, so the raw TUI has no extended-key channel there at all.

**3. The colour class is lost in the terminal engine, not in the TUI.**
`crates/zz-terminal/src/model.rs` `PackedStyle` stores `foreground: u32` and
`background: u32` — a packed 24-bit RGB with no field for a palette index. Its
own doc comment says "resolved". `ATTR_EXPLICIT_RGB` keeps the explicit/named
bit but not which index was named, so the value cannot be spelled back. The
wire (`terminal_codec.rs`, `push_u32(output, style.foreground_raw())`) and the
renderer (`render.rs`, `"\x1b[38;2;{};{};{}m"`) are both faithful to a value
that was already classless. The same renderer proves it is not the problem:
chrome travels as `TmuxColour`, an enum with `Basic`, `Indexed` and `Rgb` arms,
and `write_ground` turns each one back into its own class. Full trace in
`07-colour-class-trace.txt`, including the fields it would take.

## What clause 3 earned anyway

Once an OSC 4 entry exists for colour 1, the pin stops emitting the index and
resolves the cell through the pane palette (`tty.c` over
`window_pane_get_palette`), so both sides put the identical RGB on the outer
terminal — for the foreground the entry paints AND for the background. The
fixture asserts all of it: `colours/palette named cell`,
`colours/palette named background` and `colours/palette rgb cell`. The indexed
cell the entry did not touch keeps each side's own class and is recorded beside
them, so the case cannot pass by accident. On the one channel where the class
could change what a user sees, zz and the pin agree.

## Recorded emission differences with no decoded-screen consequence

`compat/tui-screen-diff.sh` is green at this tip (111 asserted checkpoints
identical), so none of these moves a cell:

- `wrap_flag` — zz writes `\e[?7l` on entry and `\e[?7h` on exit; a renderer
  that positions every cell explicitly needs autowrap off to write the last
  cell of the last row without scrolling. The pin keeps autowrap on and tracks
  that cell in `tty.c`.
- `mouse_all_flag` / `mouse_button_flag` — zz arms `\e[?1003h` (any-event) for
  the whole attach; the pin arms `\e[?1002h` and raises `MODE_MOUSE_ALL` only
  while a menu is up. zz's TUI does consume motion
  (`input.rs menu_pointer_kind` maps `Moved` to `Motion`), so this is a scope
  difference: zz arms for the whole attach what the pin arms for the life of a
  menu. Narrowing it is `app.rs`, outside these zones.
- `keypad_flag` / `keypad_cursor_flag` — the pin sends smkx, zz sends neither.
  Confined: the outer terminal therefore sends normal-mode CSI arrows, which is
  exactly what `terminal_event.rs parse_csi` decodes.
- `client_theme` — the pin answers empty until its terminal replies, zz answers
  `dark` from attach. That is the recorded stance on
  `options.client-terminal-negotiation`, not a new finding; the row keeps it
  under this fixture's eye.

## Two channels that turned out to be assertable

`options.terminal-engine-limits` is accepted on the grounds that zz keeps its
engine's own Unicode widths and takes no override, which says nothing about
whether those widths agree with the pin's. They do, on the three categories
that usually disagree. With UTF-8 clients on both sides, a wide CJK pair, a base
letter with a combining acute and an astral emoji leave the outer cursor at the
same column (20,0) and produce the identical decoded line, asserted in
`widths/cursor` and `widths/line`. The channel is driven rather than named, and
the sixth sabotage adds one wide codepoint to the pin's sample alone to prove
the case can fail.

## A measurement that adds to a gap nobody owns this cycle

`options.client-terminal-negotiation` records that zz opens no terminfo
database and selects capabilities by TERM name. `06-pin-feature-negotiation.txt`
adds the other half: the pin's client feature list is not read from the
terminfo entry either — attached to the same outer pinned tmux under
`vt100`, `xterm`, `xterm-256color` and `screen-256color`, the pin answers the
IDENTICAL list every time, including `256` and `RGB` on `vt100`, whose entry has
neither. It is negotiated from the terminal's replies (`tty-keys.c` adds sixel,
margins, rectfill and clipboard straight from the DA parameters). This does not
contradict the recorded decision, so the gap is left alone; it is written down
here because it is why `-2`'s `256` is redundant on any terminal that answers,
and why `client_colours` reads 16777216 against zz's 16 on `TERM=xterm`.

## Files

- `environment.txt` — both binary hashes, the base revision and worktree state,
  the pin and its commit, OS, shell, locale, TERM, outer size.
- `01-tui-caps-run-1.txt`, `02-tui-caps-run-2.txt`, `03-tui-caps-run-3.txt` —
  three runs of the new fixture, exit 0, 58 asserted rows identical, byte-stable
  row dispositions across runs.
- `04-tui-caps-self-check.txt` — the five sabotages and the control, exit 0.
- `05-tui-screen-diff.txt` — the regression run, exit 0.
- `06-pin-feature-negotiation.txt` — the pin's feature list under four TERM
  names, and `06-pin-feature-negotiation.probe.txt`, the script that produced it.
- `07-colour-class-trace.txt` — one named cell traced from the program that
  wrote it to the bytes the TUI emits, with the fields a fix would need.
- `08-client-flag-dispositions.txt` — `-2`, `-u` and `-T`: parsing, diagnostics,
  applying, and what each would take.
- `09-capability-matrix.txt` — the declared matrix with its measured values and
  the source citations behind every recorded row.
- `10-tracker-check.txt` — the ledger validator at the final tip.

## The sabotages

`--self-check` drives the same driver with one deliberate one-sided difference
and requires an ASSERTED row to report it. A recorded row can never satisfy a
sabotage, and a row that is recorded in the case that measures a divergence
still asserts in every other case, which is what makes the third sabotage below
work.

- control, nothing sabotaged: no asserted row differs.
- a one-sided flag: `-u` given to the pin alone. `client_utf8` and
  `client_flags` assert in this case and catch it.
- a one-sided palette entry: `rgb:00/ff/00` on zz against `rgb:00/00/ff` on the
  pin. The palette-resolved cells differ and are caught.
- the legacy case driven with extended keys on the pin alone: `pane_key_mode`
  asserts under `extended-keys off` (VT10x on both) and catches `Ext 2`.
- a one-sided unknown CLI option: the diagnostics comparison catches it.
- a one-sided wide codepoint: one extra wide character on the pin's sample
  moves its cursor away from zz's and changes its decoded line.
