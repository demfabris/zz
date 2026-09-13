# TUI-008 attempt-01, cycle 7, the input lane

Alienware, CachyOS, against pinned tmux d77c9dc6. `environment.txt` carries the
box, both binaries and the tip every file below was produced at.

## What this attempt built

`compat/tui-mouse.sh`, TUI-008's proof surface. It is the first thing in the
campaign to drive a POINTER at an attached client: real SGR 1006 mouse reports,
real bracketed pastes and real focus reports, written as bytes into each side's
stdin with `send-keys -H` on the outer pinned tmux, which is also the decoder.
Every case names the observable it compares, and none of them is the byte
stream: an active pane index, a paste buffer, `#{pane_in_mode}`, a window
index, a global option a user's own binding set, the SGR report a program in
the pane received off its own decoded screen, or the whole decoded screen.

## The files

- `environment.txt` — the box, the two binaries, the tip.
- `tui-mouse-run-1.txt`, `-2`, `-3` — three runs of the fixture at the tip.
- `tui-mouse-self-check.txt` — five one-sided sabotages and three controls.
- `attached-client.txt` — `compat/attached-client.sh`, PASS.
- `tui-screen-diff.txt`, `tui-screen-diff-self-check.txt`
- `tui-stock-keys.txt`, `tui-indicators.txt`, `tui-pane-geometry.txt`
- `tui-copy-mode.txt`, `tui-overlays.txt`
- `tui-choosers.txt` — green. An earlier run of it during this attempt took one
  red case, `filter-cleared`, on `#{pane_current_command}` reading `sh` on the
  pin and `bash` on zz for the same `/bin/sh` pane, which is bash on this box;
  it was green on the next run and green again at the final tip. Nothing in this
  lane's diff can reach that format.
- `corpus-known-terminal-runtime.txt` — the one corpus row that reads the mouse
  flags this lane changed (`known/known-terminal-runtime`), green: those are the
  PANE's flags, not the client's arming.
- `corpus-focus-rows.txt` — every corpus row naming `focus-events`
  (`smoke/hooks-pane-focus` and the three `honest-knobs-c1-*` rows), green.
- `status-row.txt` — under `LC_ALL=C LC_TIME=C`, the box's own locale being the
  known `%b` divergence.
- `cargo-checks.txt` — `cargo test` and `cargo clippy` for the four touched
  crates, and `cargo fmt --all -- --check`, whose one complaint is
  `crates/zz-tui/src/mode_view.rs`, a file this branch does not touch.

## What landed

1. **A bound mouse key runs its binding.** The client names a decoded pointer
   event the way `server_client_check_mouse` does, from the gesture, its button
   and where on the client's own screen it landed (`Pane`, `Border`, `Status`,
   `StatusLeft`, `StatusRight`, `StatusDefault`, `Control0`-`Control9`), and
   sends it as `InputMessage::MouseKey` with the pane and window the event
   carries. The daemon walks the table the client is in and then the session's
   root table, as `key_bindings_get` does, and runs the binding against that
   target. `bind -n MouseDown1Pane set-option -g @mousekey fired` now fires on
   both binaries: `click-user-binding` asserts.
2. **The pin's mouse arming.** `server_client_reset_state` picks between
   button-event and any-event tracking from the `mouse` option,
   `focus-follows-mouse`, the panes of the current window and the overlay on
   screen, and `tty_update_mode` clears all four modes before arming. The raw
   TUI armed `\e[?1003h\e[?1006h` for the whole attach. Eight rows of
   `tui-caps.sh` moved from recorded to asserted; see TUI-009 attempt-04.
3. **Focus reporting asked for, not assumed.** `tty_start_tty` writes `Enfcs`
   only while `focus-events` is on and its default is off; `TerminalGuard::enter`
   wrote `\e[?1004h` on every attach and now reads that server option once.

## The cases that still record, each with its cause

`tui-mouse.sh` asserts 13 checks and records 17, identical across three runs.

- `click-user-binding-target/context` — the invoking event travels with the
  command now, but no format reads it: the eight `mouse_*` names are
  unimplemented in `crates/zz-mux/src/formats.rs`, which this batch's zones do
  not open. Owner: whoever holds `formats.rs`. Pin `6,4,%0,`; zz empty.
- `wheel-up-pane` (2 checks), `drag-selects/buffer`, `multi-click` (2),
  `right-click-pane/screen`, `border-drag/pane-width`,
  `status-clicks/window-after-wheel-down`, `status-clicks/right-click-screen` —
  all eight are the pin's STOCK root bindings, which zz does not install:
  `list-keys -T root` still answers 27 rows on the pin and 0 on zz. Installing
  them needs `send -M`, `copy-mode -M`/`-e`/`-H`/`-u`/`-d`/`-S`,
  `resize-pane -M`, `move-pane -M`, `paste -p`, `select-pane -M`, `-t=`/`-t@`
  target spellings and `if -F` over `#{mouse_any_flag}`, which is the next
  obligation's shape, not a punch-list item's. Owner: TUI-008.
  Two of them carry a second cause of their own: `border-drag` cannot exist at
  all while `input.rs` has no border hit test, and `multi-click` cannot while
  `input.rs` sets `click_count` to 0 or 1 only, so zz-terminal's own multi-click
  path is unreachable from the raw TUI.
- `paste-into-copy-mode/after-cancel-screen` and `paste-under-menu/screen` —
  a bracketed paste is a direct write to the pane in the raw TUI
  (`input.rs handle_paste`), so the text lands in the program behind an overlay
  and appears the moment the overlay is left, while the pin's window mode and
  menu consume the paste key. Owner: TUI-008, in `input.rs`. The paste INTO a
  pane, into copy mode while the mode is up, into the command prompt, and the
  menu ITEM a paste's characters select are all asserted.
- `focus-events-off/received` — `focus-events` on the pin decides only whether
  the client arms `Enfcs`; a report that arrives anyway still reaches a pane
  that asked for it. `tty.rs` now matches the arming half, but the daemon still
  gates DELIVERY on the option, and that gate is the GUI's too
  (`crates/zz/src/terminal/view.rs` sends the same `Focus` action from window
  focus and has no arming of its own), so moving it into each client is a change
  these zones do not open. Owner: whoever holds the GUI's focus reporting.

## Sabotages

`--self-check` drives seven one-sided differences and three controls:
a one-sided paste; the SGR mouse extension dropped from one side's program; only
the focus-out report sent to one side; a `status-left` only one side carries; a
click aimed at the pane zz already sits in; and the same root mouse binding set
to a different value on one side. The three controls (a click, a paste and a
pair of focus reports, identical on both sides) stay quiet.
