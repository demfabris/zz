# TUI-009 attempt-02: outer-terminal capabilities and fidelity

Cycle 5, the caps lane, on alienware against pinned tmux d77c9dc6, both
binaries attached inside one outer pinned tmux. attempt-01 is cycle 4's
measurement and stays as read-only history. Everything here ran at
`21c914e1` unless a file says otherwise.

## What changed and what it proves

**Clause 3, colour classes: asserted.** The pane body now keeps the colour
class the program wrote. `PackedStyle` carries a class per ground (resolved,
default, palette index, RGB) next to the RGB it always had. The foreground
index sits in the unused top byte of the foreground word, the background index
in the byte that used to be reserved, and the four class bits in the unused top
of `attributes`. `PackedStyle` is still 16 bytes, the C ABI layout is the same,
and `foreground()`, `background()`, `*_raw()` and `attributes()` return what
they returned before. The terminal worker classifies from libghostty's
`StyleColor` and the raw cell's background tag. A palette cell whose current
entry differs from the configured palette is classed RGB, which is what the pin
does after an OSC 4 (`tty_check_fg` over `colour_palette_get`). The wire keeps
the 16-byte style record and appends one class word per style after the kitty
placements of a full viewport and of a patch. That append is why
`PROTOCOL_VERSION` is 101. `write_sgr` spells the class the way `write_ground`
already did for chrome. The GUI still paints the RGB. Measured in
`01`-`03`: stock `\e[31m` and `\e[38;5;42m` on both sides (zz used to send
`\e[38;2;205;0;0m` and `\e[38;2;0;215;135m`). After OSC 4 on entry 1 the named
foreground and background go RGB. After OSC 4 on entry 42 the indexed cell does
too. After OSC 104 both go back to their index. Every cell of every stage is
asserted. `05` asserts `colour-classes` at all six sizes.

Two limits are named and not driven:

- An OSC 4 that sets an entry to exactly its configured value. libghostty-vt
  exposes the current and default palettes but no override mask.
- `pane-colours`, which zz applies to the pane's default palette.

Inverse cells keep the old resolved path.

**Default foreground: asserted.** A style that names no colour now resets the
ground (39/49) instead of painting zz's theme foreground.

- Old behaviour: `\e[38;2;216;222;233m\e[44m` under `status-style bg=colour4`.
- The pin's behaviour: `\e[44m`, the terminal's own foreground.
- Decided 2026-09-10 by the orchestrator under fabrico's TUI parity contract of
  2026-09-09; reversible.

The pane-border ground is the same kind of divergence. The divider painter
hands `model.appearance.background` to `write_colored_text`. It is measured,
and it is outside this lane's render.rs zone. Those checkpoints stay recorded
for the border colour anyway.

**Clause 2, the flags: asserted.** `-u` raises the existing `client-utf8-v1`
capability. `-2` and `-T` each send a `client-features-v1:<spec>` capability.
The daemon parses them with `tty_add_features`' rules and merges them in the
pin's table order. Under `TERM=xterm LANG=C`:

- `-u` takes `client_utf8` from 0 to 1 and adds `UTF-8` to `client_flags` on
  both sides.
- `-T sixel` adds `sixel` on both sides.
- `-2` leaves `256` present on both sides.

`delta-client_flags` and `delta-client_termfeatures` assert in every case
(reviewer nit d), along with a `flag-features` row.

Still recorded:

- The roster rows (`client_colours`, `client_termfeatures`, `client_theme`),
  under options.client-terminal-negotiation.
- `widths/non-utf8 line`, new this cycle: for a non-UTF-8 client the pin
  draws underscores (`tty_check_codeset`) and the raw TUI writes UTF-8. That
  fix belongs to render.rs's glyph path, outside this zone.

**Clause 1, extended keys: asserted.** tty.rs reads `extended-keys` over a
separate command connection before it enters the terminal (on the interactive
connection, a command's output turns into a view). For any value but `off` it
arms `\e[>4;2m` and disarms `\e[>4m`. terminal_event.rs decodes
`\e[27;<mod>;<key>~`. `pane_key_mode` reads VT10x/Ext 2/Ext 2 on both sides
under off/on/always. A C-Enter root binding fires on neither side under off and
on both under on and always.

Still recorded in clause 1:

- `wrap_flag`
- `mouse_all_flag` and `mouse_button_flag`
- `keypad_flag` and `keypad_cursor_flag`

Focus reporting stays named and not driven.

**Fixture hygiene.** No committed revision of tui-caps.sh ever used the leaked
daemon's `/tmp/zzcaps5.<pid>/home` layout. The committed fixture could reach a
default socket only through `run_cli`, which names no socket. Now:

- Every zz process gets `XDG_RUNTIME_DIR` inside the scratch directory, and the
  CLI diagnostics get `TMUX_TMPDIR` there as well.
- The cleanup stops a daemon on that scratch socket.
- The cleanup reaps every process whose environment names the scratch
  directory. That includes the daemon's own `--bootstrap-tray` helper, which
  `09` shows alive at the moment of the signal.

## Files

- `environment.txt`: both binary hashes, the revision and worktree state, the
  pin, the OS line, TERM, shell, bash and locale.
- `01-tui-caps-run-1.txt`, `02-tui-caps-run-2.txt`, `03-tui-caps-run-3.txt`:
  three runs, exit 0, 111 asserted rows identical and 31 recorded, with
  identical row dispositions.
- `04-tui-caps-self-check.txt`: twelve sabotages caught and the control quiet,
  exit 0.
- `05-tui-screen-diff.txt`: 123 asserted checkpoints identical and 30 recorded,
  with colour-classes and default-fg asserted at six sizes, exit 0.
- `06-tui-screen-diff-self-check.txt`: includes the two new sabotages
  (colour class, default foreground), exit 0.
- `07-tui-pane-geometry.txt`: 6/6, exit 0.
- `08-status-row-c-locale.txt`: `LC_ALL=C LC_TIME=C`, 14/14, exit 0.
- `09-fixture-hygiene.txt`: a normal run and a run interrupted with SIGINT,
  with the process sweep, the scratch directory, the sockets and
  `/tmp/zz-user` checked after each.
- `10-cargo-and-clippy.txt`: the test results of every touched crate, the zz
  cli_binary integration suite, zz-daemon's library and integration tests,
  clippy and rustfmt, each labelled with the revision it ran at.
- `11-tracker-check.txt`: both ledger validators.
