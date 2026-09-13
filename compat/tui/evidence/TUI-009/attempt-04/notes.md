# TUI-009 attempt-04, cycle 7, the input lane

Alienware, CachyOS, against pinned tmux d77c9dc6. `environment.txt` carries the
box, both binaries and the tip. attempt-01 to attempt-03 are read-only history;
attempt-03/notes.md holds cycle 6's work and its review.

This attempt is the punch list's fourth item and nothing else: the mouse rows,
the widths row and the extended-keys row. The roster half of this obligation is
untouched here.

## The files

- `environment.txt`
- `tui-caps-run-1.txt`, `-2`, `-3` — three runs at the tip, identical
  dispositions and identical recorded values.
- `tui-caps-self-check.txt` — 22 one-sided sabotages and 4 controls.

## What moved

`compat/tui-caps.sh` asserts **287** rows and records **15**, from 279 and 23 at
origin/main. The eight rows that moved are `mouse_all_flag` and
`mouse_button_flag` on `legacy`, `extended`, `extended-always` and
`silent/extended`.

Cause and fix: `server_client_reset_state` starts from the overlay's screen mode
when one is drawn and the active pane's otherwise, and only then does the
`mouse` option speak: with the option on and no overlay it clears all three
trackings and raises `MODE_MOUSE_ALL` only for a pane that asked for it,
`focus-follows-mouse` raises it too, and anything short of that settles on
`MODE_MOUSE_BUTTON`; a menu is the overlay that carries `MODE_MOUSE_ALL` of its
own (`menu.c` `menu_prepare`). `tty_update_mode` then clears `1006`, `1000`,
`1002` and `1003` before arming the pair it wants. `tty.rs` armed
`\e[?1003h\e[?1006h` for the whole attach and `app.rs sync_mouse_modes` decided
on a single boolean. `crate::tty::MouseArming` is now the pin's three states,
`mouse_mode_sequence` writes the pin's clear-then-arm, and
`app::desired_mouse_arming` is the rule above.

The fixture gained a sabotage for the channel: `focus-follows-mouse` turned on
for the pin alone raises `MODE_MOUSE_ALL` there and leaves `MODE_MOUSE_BUTTON`
on zz, and those two rows are the only ones that can report it; with the option
on both sides the same case is a control and stays quiet.

The capability matrix's `focus reporting` line also stopped being a named
not-equal: `tty_start_tty` writes `Enfcs` only while `focus-events` is on and
its default is off, and `TerminalGuard::enter` now reads that same server
option once instead of writing `\e[?1004h` on every attach. No pane format
publishes focus mode, so this decoder still cannot drive it; what each side does
with a report that arrives is driven by `compat/tui-mouse.sh` instead.

## The 15 rows that still record, each with its cause and its owner

**Clause 1, two rows.**

- `widths/non-utf8/line` — `tty_check_codeset` draws each non-ASCII cell as
  `data.width` underscores for a client without `CLIENT_UTF8`, and maps what it
  can to ACS first; `render.rs`'s glyph path writes the UTF-8 grapheme. The
  glyph path is in these zones and the fix is small there, but the raw TUI has
  no way to learn its own UTF-8 flag: the rule lives in
  `crates/zz-daemon/src/client.rs` (`client_utf8_capability` over
  `client_takes_utf8`) and neither it nor `client_terminal_flags` is exported,
  where the colour count next to them is (`client_terminal_colour_count`).
  Unblocking is one exported accessor beside that one; a full match also wants
  the ACS reverse map for the chrome the pin folds through the same function.
  Owner: `crates/zz-daemon/src/client.rs`, then TUI-009.
- `silent/extended pane_key_mode` — the pin writes `Eneks` only when the
  option is on AND its terminal carries the `extkeys` feature
  (`tty_term_string(term, TTYC_ENEKS)` is empty otherwise), which on a silent
  `TERM=xterm` it does not; `tty.rs` arms `\e[>4;2m` from the option alone in
  `TerminalGuard::enter`. Same shape of blocker as the row above: deciding it
  needs the terminal's feature set by TERM name, which
  `crates/zz-daemon/src/terminal_features.rs` computes and does not export by
  feature. Duplicating that table in `tty.rs` is the debt this lane declined.
  Owner: `crates/zz-daemon/src/terminal_features.rs`, then TUI-009. Closing it
  is what lets `option:extended-keys` leave `honest_knobs.rs` for
  `TMUX_OPTION_CONSUMERS` and the `compat_manifest_tests.rs` partition.

**Clause 2, thirteen rows.** `client_colours` on `facts/bare`, `facts/-2`,
`facts/-u`, `facts/-T` and `facts/utf8-locale`, and `client_termfeatures` on
those five plus `silent/bare`, `silent/-T` and `silent/-2`. Unchanged cause from
attempt-03: the daemon derives a client's roster from its TERM, its COLORTERM
and its flags alone, because `ClientHello` is sent before the terminal can
answer anything and no message carries a later answer. The fix is a wire append
carrying the features a client learned from its terminal after the hello, the
way `tty_update_features` reaches the pin's own client record, plus the
terminfo-derived base set per TERM for the three silent rows. It lands in
`crates/zz-daemon/src/client.rs`, `terminal_features.rs` and `daemon.rs`'s
roster, none of which this batch's zones open (this batch has `daemon.rs` for
mouse, paste and focus routing only). Owner: TUI-009, in a lane whose zones
carry the daemon's client roster.

**Clause 3** asserts with nothing recorded behind it, as it did at the end of
cycle 6.
