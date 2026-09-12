# TUI-007 attempt-02

Prompts, menus, popups and pane labels (cycle 6, overlays lane, alienware). Attempt-01 closed the
surfaces themselves and left thirteen cases recorded `SIBLING:modes`, because the prompt and
message row's style was TUI-004's. TUI-004 landed. This attempt flips those thirteen, fixes the
four that did not simply start matching, and takes the fixture to zero recorded cases.

Every run below except `tui-overlays-prefix` ran at `b1747c04` with the tracked tree clean outside
this directory and `target/debug/zz` built from that tree.

| File | What it is |
| --- | --- |
| `environment.txt` | both binaries' sha256, the zz revision and clean state, the prefix binary, the pin, OS, TERM, shell, bash, locale |
| `tui-overlays-prefix.stdout.txt` | this attempt's `compat/tui-overlays.sh` against the `crates/` tree of START `7c692222`, exit 1: `11 of 48 asserted comparisons differ, 0 recorded` - `prompt-trailing-space`, `confirm-opened`, `confirm-resized`, `popup-opened`, `popup-typed`, `popup-under-message`, `popup-resized`, `centre-popup-fg`, `centre-popup-fg-typed`, `centre-popup-fgbg`, `centre-popup-fgbg-typed`. That is the whole of what the three landings fix |
| `tui-overlays-prefix.stderr.txt`, `tui-overlays-prefix.exit.txt` | its stderr (empty) and exit status |
| `tui-overlays-run-1.stdout.txt` | `compat/tui-overlays.sh`, exit 0: `all 48 asserted comparisons identical, 0 recorded not asserted` |
| `tui-overlays-run-1.stderr.txt`, `tui-overlays-run-1.exit.txt` | stderr (empty) and exit status of run 1 |
| `tui-overlays-run-2.stdout.txt` | the same fixture again, exit 0, same summary |
| `tui-overlays-run-2.stderr.txt`, `tui-overlays-run-2.exit.txt` | stderr (empty) and exit status of run 2 |
| `tui-overlays-run-3.stdout.txt` | the same fixture a third time, exit 0, same summary |
| `tui-overlays-run-3.stderr.txt`, `tui-overlays-run-3.exit.txt` | stderr (empty) and exit status of run 3 |
| `tui-overlays-self-check.stdout.txt` | `compat/tui-overlays.sh --self-check`, exit 0: eight sabotages caught, one equivalence passed |
| `tui-overlays-self-check.stderr.txt`, `tui-overlays-self-check.exit.txt` | stderr (empty) and exit status |
| `tui-screen-diff.stdout.txt` | `compat/tui-screen-diff.sh`, exit 0: `all 111 asserted checkpoints identical, 42 recorded not asserted` |
| `tui-screen-diff.stderr.txt`, `tui-screen-diff.exit.txt` | its stderr and exit status |
| `attached-client.stdout.txt` | `compat/attached-client.sh`, empty: the driver stops in its first probe |
| `attached-client.stderr.txt` | `error: zz screen did not visibly become copy-mode within 10 seconds`, plus the daemon ring log. This is the one red BASE is known to carry and it is not this lane's: `wait_for_visible_mode` still looks for zz's old `COPY` status badge, which the modes landing replaced with the pin's in-pane position indicator. `origin/main` already carries the modes lane's fix to that function; this branch sits on BASE, so it does not. Nothing runs past that step, so there is nothing past it to report |
| `attached-client.exit.txt` | its exit status (1) |
| `cargo-test-zz-tui.txt` | `cargo test -p zz-tui`, 192 passed |
| `cargo-clippy-zz-tui.txt` | `cargo clippy -p zz-tui --all-targets --all-features -- -D warnings`, clean |
| `cargo-test-zz-mux.txt` | `cargo test -p zz-mux`, 524 + 105 passed |
| `cargo-clippy-zz-mux.txt` | `cargo clippy -p zz-mux --all-targets --all-features -- -D warnings`, clean |
| `cargo-test-zz.txt` | `cargo test -p zz`, 634 unit and 125 `cli_binary` tests passed |
| `notes.md` | this file |

Environment for every fixture run: `PATH=/opt/homebrew/bin:$PATH`,
`ZZ_COMPAT_TMUX=/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux`,
`ZZ_COMPAT_CORPUS=/home/demfabris/dev/zz/compat/.cache/plugins`; `attached-client.sh` also takes
`ZZ_BIN=$PWD/target/debug/zz TMUX_BIN=<pin>`. Every cargo command went through the box's two-slot
lock at `MemoryMax=5G`, `--jobs 4 -- --test-threads=3`.

## What changed, and what each change is worth

1. `Keep popup-style under a popup's blank rows instead of erasing them` (`6e6b7daf`). At the
   merged tip the fixture's eight popup cases differed, having been identical three times at the
   lane tip before the merge. The DIFF bytes named it: zz ended a popup's blank rows with an ECH
   after an SGR reset, so the run came back on the terminal's default ground where the pin's
   `popup_draw_cb` gives the job's default cells popup-style. That erase is the modes landing's
   trailing clear in `render.rs` `blit_row`; it now stands down when the renderer is drawing on an
   explicit ground. `popup_blank_rows_keep_the_popup_style_instead_of_an_erase` pins it.
2. `Paint the confirm prompt like a message and leave trailing spaces to the fill` (`8b510681`).
   Two measurements. The pin draws confirm-before through `status_prompt_redraw`, the same message
   area as a command prompt and a client message, so zz's second overlay - a `StatusOverlay::Row`
   that painted the whole row in the terminal appearance's inverted colours and read no option -
   is gone and confirm goes through the one message painting. And a space at the END of a message
   string or of the prompt's input is not painted in message-style: the pin's decoded row carries
   the message-style run up to the last non-blank glyph and then the fill's own cell, while zz
   painted the spaces inside the run. `trailing_prompt_spaces_are_left_to_the_fill` pins the
   second.
3. `Assert every overlay case now that the modes landing paints the message row` (`f87aedc8`).
   The `text` verdict mode and its row exclusion are gone; every case is `same`, whole screen and
   cursor. Two new measurements come with the flip: `prompt-trailing-space` types two spaces at the
   end of a command, and the two `*-under-message` cases pass a message that ends in two spaces.
4. `Close option:message-style for the raw TUI` (`30134c35`).
5. `Make StatusOverlay a struct now that it has one shape` (`b1747c04`). Confirm-before was the
   second variant; with it gone the enum carried one and every use of it was an irrefutable
   pattern. No behaviour changes.

## The thirteen SIBLING:modes cases

Nine matched at the merged tip and were flipped as they stood: `prompt-opened`, `prompt-typed`,
`prompt-wrapped`, `prompt-cursor-home`, `prompt-resized`, `prompt-resized-back`,
`prompt-status-top`, `menu-under-message`, `menu-resized`. Four needed the landings above:
`confirm-opened` and `confirm-resized` (the confirm overlay), `popup-under-message` and
`popup-resized` (the trailing spaces, on top of the popup erase). `tui-overlays-prefix` shows all
four failing without them. No case is recorded for any reason.

## The sabotages

`--self-check` now plants eight one-sided differences and one equivalence. The six from attempt-01
are unchanged except that the `menu-border-style` one no longer excludes the message row - there is
nothing left to exclude. Two are new, and they are what holds the flip:

- `message, a message-style only one side sets`: `message-style bg=colour94` on zz only, under a
  client message. Expected `rows:23` - the message row and nothing else, cursor included. Measured
  pin `38;2;13;13;13m 48;2;184;134;11m` against zz `48;5;94m`.
- `prompt, a message-style only one side sets`: the same option, under the command prompt.
  Expected `rows:23` as well.

An exclusion sneaking back into `compare_rows` would make both of these report zero rows and fail.

The message sabotage clears its message with `C-l`, not `Escape`. `server-client.c:1624` clears the
message and then passes the key on: with no overlay open it reaches the pane, and an `Escape` there
leaves the pane's shell half-way into a meta sequence, so the next case's marker never runs. Every
other `Escape` in this fixture is answered by an open surface. Measured while writing the two
cases: the run stalled at `pstyle settled on the zz screen did not settle within 10 seconds`, on
both sides identically.

## Gap

`option:message-style` left `options.native-overlay-styles` for a closed `options.tui-message-style`
with the 2026-09-11 measurement, and joined `TMUX_OPTION_CONSUMERS`, 144 to 145 (the
`compat_manifest_tests.rs` partition moved in the same commit: session scope 46 to 47, tracked
option items 36 to 35). `option:message-command-style` stays open with the modes lane's 2026-09-10
measurement: `CommandPromptState` carries no command-mode flag, so the raw TUI cannot tell a prompt
in vi command mode and paints message-style there too. Nothing else in that gap moved, and the GUI
keeps its native overlay presentation.

## Not measured

Nothing on the wire changed this attempt. `message-command-style` under `status-keys vi` is not
exercised by this fixture and is not claimed. `attached-client.sh` does not run past its first
probe on this branch, for the reason above.

## Carried from attempt-01's evidence note

Kept verbatim so the cycle-5 measurement, its wire and gap bookkeeping and the gate's
decision survive the note being trimmed to this cycle.

CYCLE 5, ATTEMPT-01, REVISED AFTER THE REVIEW OF b451b74a. New fixture compat/tui-overlays.sh, the outer-pinned-tmux driver of tui-indicators.sh, whole screen plus the cursor tuple at named settled checkpoints. At e9600323 it reports all 47 asserted comparisons identical and 13 recorded, three runs out of three, and --self-check catches six sabotages (a menu item only one side has, a keystroke only one side's covered pane receives, a popup border only one side draws rounded, a prompt cursor only one side moves, an fg-only menu-border-style only one side sets, compared the way text mode compares with the message row left out, and a display-panes-active-colour only one side sets) and passes one equivalence. Evidence compat/tui/evidence/TUI-007/attempt-01/. WHAT ASSERTS. Command prompt: opened, typed, a long input scrolled the way prompt.c prompt_draw scrolls it, the cursor moved back into it, 60x20 and back, status-position top, cancelled. Confirm-before: opened, resized, refused. Display-menu (-x 4 -y 12, -T, shortcut annotations, a separator and a disabled row): opened, Down, Down past the separator, an unanswered z, a message over it, 100x30, Escape, the s shortcut, and a -M menu chosen by a button-1 click. Display-popup (-w 34 -h 9 -T -E over a job): opened, a line typed into the job, a message over it, 100x30, closed. Display-panes over a horizontal split: the split, the labels, a resize clearing them, digit 1 selecting pane 1, Z closing them and reaching the pane. At 79x23, an odd height: a centred display-menu (-x C -y C) opened and after Down, a centred -M menu opened and chosen by a click, and a centred display-popup of the default size opened, typed into and closed, first with menu-style, menu-selected-style, menu-border-style, popup-style and popup-border-style set fg-only on both sides and then fg plus bg; display-panes with display-panes-colour colour33 and display-panes-active-colour colour124 on both sides, shown and closed by a digit. Nothing typed into a covered pane reaches it: the restoration comparisons after the menu's z, the popup's line and the labels' Z cover the pane each surface sat on. THE 13 RECORDED CASES are text mode (the 7 prompt-* cases, confirm-opened, confirm-resized, menu-under-message, menu-resized, popup-under-message, popup-resized): every glyph, every column, the cursor and the style of every row but the prompt/message row are asserted (under status-position top the row after it is left out too, since it only carries the SGR capture-pane continues from row 0); only that row's STYLE is recorded, with a reason starting SIBLING:modes, because message-style and message-command-style are TUI-004's this cycle. WHAT THE REVIEW OF b451b74a FOUND AND WHAT CHANGED. (a) Odd-height centring: the pin's cmd_display_menu_get_pos centres and clamps on tty->sy, the client's full height, while zz's popup_client_geometry used the window's height, which leaves out the status line. At 79x23 zz put a centred popup and a -x C -y C menu one row higher (pin rows 6..14, zz 5..13), and a click on a centred -M menu chose a different item. popup_client_geometry now takes the client's own size from client_sizes when the client reported one (the raw TUI and other terminal clients), and falls back to the window extent otherwise, so the GUI is unchanged. This is a one-call-site change in crates/zz-daemon, outside this lane's wire-append zone, made because the orchestrator directed every blocker fixed. Before the fix, tui-overlays-prefix.stdout.txt (the new fixture against the b451b74a binary) shows 9 DIFFs and the click missing. (b) User overlay styles with a foreground and no background: the pin leaves the terminal's default ground (49) under the cell, and the raw TUI painted its appearance RGB (48;2;16;19;24). overlay::grounded now maps a missing fg or bg to default for the menu border, title, padding and blank rows, and for the popup fill, border and title; write_sgr is unchanged. (c) display-panes-colour and display-panes-active-colour are now measured at non-default values on both sides, plus a one-sided sabotage, so the published values are shown to be drawn, not the fallback. (d) Text mode now asserts every row style except the message row's. NOT MEASURED against the pin, unit-tested on zz's side only: the display-panes plain-number path for a pane smaller than six columns per digit or five rows, and the letter below the digits for panes 10 to 34. OLD BEHAVIOUR AND FIX, each measured 2026-09-10 against the pin and fixed in crates/zz-tui/src/overlay.rs with hooks in render.rs, input.rs and app.rs: (1) prompt input never scrolled; zz cut it at the right edge where prompt_draw keeps the cursor on the last column. (2) confirm-before hid its cursor at 79,23 where status_prompt_cursor shows it after the prompt at 18,23, and a frame-only repaint after a resize hid it again. (3) menu and popup titles were padded with spaces where screen_write_box leaves the border line. (4) a disabled menu row's padding was dim where screen_write_menu dims only the name. (5) the hidden menu cursor sat at 23,11 where menu_mode_cb puts it two columns into the selected row. (6) popup content took the terminal's default colours where popup_draw_cb gives the job's default cells popup-style. (7) the popup cursor stayed visible under a message. (8) display-panes drew a small tag at each pane's top left where cmd_display_panes_draw_pane draws window_clock_table digits centred in the pane on a ground of display-panes-colour or display-panes-active-colour, display-panes-format on the pane's top row, and parks the hidden cursor at 0,0. (9) a key a client-local overlay answered never reached the daemon, so a message stayed up and froze pane output, where server_client_handle_key clears the message before any overlay sees the key. (10) a size change left display-panes up where server-client.c:2465 clears an overlay with no resize callback; closing it with DisplayPanesAction::Close would have typed an Escape into the pane, so the TUI sends a new Dismiss. WIRE: PROTOCOL_VERSION 100 to 101 for three pure appends, InputMessage::DismissClientMessage, DisplayPanesAction::Dismiss and DisplayPanesState colour and active_colour, consumer halves in the same push. GAPS: option:display-panes-colour and option:display-panes-active-colour closed as options.display-panes-colours, TMUX_OPTION_CONSUMERS 139 to 141; options.native-overlay-styles keeps its GUI decision and its presentation items with the dated measurement appended. The three clients.tui-*-overlay gaps were already closed; compat/attached-client.sh keeps their behavioural cases and passes at e9600323, run in three parts because the whole driver outlasts one 585-second call on this loaded box, and compat/tui-screen-diff.sh stays green, 111 asserted and 42 recorded. DECLARED IN THE FIXTURE HEADER: in the display-panes cases only, the SGR around the divider glyph is stripped on both sides, because the divider's colour is the border record tui-screen-diff.sh keeps under BORDER_STYLE_REASON. FINDINGS OUTSIDE THIS OBLIGATION, reported and not fixed: an 80-column -h split grown to 100 columns splits 50|49 on the pin and 49|50 on zz, and shrinking back leaves zz's left pane showing scrollback lines the pin's does not; the display-panes resize therefore changes the height only. RESIDUE NOT DRIVEN: under display-message -N the daemon ignores the dismissal as the pin does, but the TUI still hands the key to its menu where the pin swallows it; the TUI has no field that says a message ignores keys. GATE DECISION (cycle 5, 2026-09-11): the gate accepts the one-call-site excursion in crates/zz-daemon/src/daemon.rs popup_client_geometry, outside the lane's wire-append zone. It reads the client's reported full size from client_sizes, falling back to window_extent, for the centre and bottom clamp, as cmd_display_menu_get_pos does with tty->sy and tty->sx. Only the raw TUI fills client_sizes (ClientTerminalSize and the hello's client-size-v1), control clients return early through control_client_geometry, and the GUI never fills it, so GUI geometry is unchanged. popup_geometry_takes_the_clients_full_terminal_height_like_tty_sy pins it, and tui-overlays-prefix.stdout.txt shows the 9 centre-* cases failing without it.
