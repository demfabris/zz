# TUI-007 attempt-01

Prompts, menus, popups and pane labels (cycle 5, overlays lane, alienware). First measured and
closed on 2026-09-10 at `36fb9f7c`, then revised the same day after the review of `b451b74a` found
two divergences the fixture's even sizes and default styles never reached. Every run below, except
`tui-overlays-prefix`, ran at `e9600323` with the tracked tree clean outside this directory and
`target/debug/zz` built from that tree.

| File | What it is |
| --- | --- |
| `environment.txt` | Both binaries' sha256, the zz revision and clean state, the pin, OS, TERM, shell, bash, locale |
| `tui-overlays-prefix.stdout.txt` | the revised `compat/tui-overlays.sh` against the PRE-FIX binary built from `b451b74a` (sha256 in `environment.txt`), exit 2: the even-size cases all `ok`, then `centre-menu-fg`, `centre-menu-fg-down`, `centre-popup-fg`, `centre-popup-fg-typed`, `centre-menu-fgbg`, `centre-menu-fgbg-down`, `centre-popup-fgbg`, `centre-popup-fgbg-typed` and `centre-menu-mouse-opened` DIFF, and the click on the centred `-M` menu chooses another item on zz |
| `tui-overlays-prefix.stderr.txt` | its stderr: `the clicked third item of the centred menu on zz did not happen within 10 seconds` |
| `tui-overlays-prefix.exit.txt` | its exit status |
| `tui-overlays-run-1.stdout.txt` | `compat/tui-overlays.sh`, exit 0: `all 47 asserted comparisons identical, 13 recorded not asserted` |
| `tui-overlays-run-1.stderr.txt` | stderr of run 1 (empty) |
| `tui-overlays-run-1.exit.txt` | exit status of run 1 |
| `tui-overlays-run-2.stdout.txt` | the same fixture again, exit 0, same summary |
| `tui-overlays-run-2.stderr.txt` | stderr of run 2 (empty) |
| `tui-overlays-run-2.exit.txt` | exit status of run 2 |
| `tui-overlays-run-3.stdout.txt` | the same fixture a third time, exit 0, same summary |
| `tui-overlays-run-3.stderr.txt` | stderr of run 3 (empty) |
| `tui-overlays-run-3.exit.txt` | exit status of run 3 |
| `tui-overlays-self-check.stdout.txt` | `compat/tui-overlays.sh --self-check`, exit 0: six sabotages caught, one equivalence passed |
| `tui-overlays-self-check.stderr.txt` | stderr of the self-check (empty) |
| `tui-overlays-self-check.exit.txt` | exit status of the self-check |
| `tui-screen-diff.stdout.txt` | `compat/tui-screen-diff.sh`, exit 0: `all 111 asserted checkpoints identical, 42 recorded not asserted` |
| `tui-screen-diff.stderr.txt` | stderr of that run |
| `tui-screen-diff.exit.txt` | exit status of that run |
| `attached-client-part-1.stdout.txt` | `compat/attached-client.sh` part 1, exit 0, `attached-client compatibility: PASS`: the whole setup, then `probe_side` through `probe_choose_buffer_delete` and the first `assert_buffer_parity keep` (the overlay behaviour regressions: command prompt, confirm-before, display-menu, display-popup) |
| `attached-client-part-1.stderr.txt`, `attached-client-part-1.exit.txt` | its stderr and exit status |
| `attached-client-part-2.stdout.txt` | part 2, exit 0, PASS: the whole setup, then `probe_choose_tree` through `probe_command_output_navigation` and the second `assert_buffer_parity keep` |
| `attached-client-part-2.stderr.txt`, `attached-client-part-2.exit.txt` | its stderr and exit status |
| `attached-client-part-3.stdout.txt` | part 3, exit 0, PASS: the whole setup, then `probe_source_file_depth` through `probe_detach_client_tty` |
| `attached-client-part-3.stderr.txt`, `attached-client-part-3.exit.txt` | its stderr and exit status |
| `attached-client-bad-split.stdout.txt` | a first split that is NOT a valid run of the driver, kept so nothing is hidden: it started at `probe_display_panes_target_no_select` and so skipped `probe_choose_buffer_row` and `probe_choose_buffer_delete`, which consume the `drop` buffer, and `probe_command_output_navigation` then failed with `zz navigation buffer cleanup did not become keep ... last output: drop`. Part 2 starts at `probe_choose_tree` for that reason |
| `attached-client-bad-split.stderr.txt`, `attached-client-bad-split.exit.txt` | its stderr and exit status (1) |
| `notes.md` | this file |

Environment for every fixture run: `PATH=/opt/homebrew/bin:$PATH`,
`ZZ_COMPAT_TMUX=/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux`,
`ZZ_COMPAT_CORPUS=/home/demfabris/dev/zz/compat/.cache/plugins`; the attached-client parts also
take `ZZ_BIN=$PWD/target/debug/zz TMUX_BIN=<pin>`.

WHY ATTACHED-CLIENT RUNS IN PARTS. The whole driver outlasts one 585-second call on this box while
four other lanes build (the reviewer measured it stopping at the cap twice, after every probe up to
`probe_command_output_navigation` had passed). Each part is a copy of `compat/attached-client.sh`
placed in `compat/` (so it finds its helpers), with only lines from the top-level probe sequence
deleted: the setup, every function and the closing PASS line are unchanged. The three parts
cover every probe call; parts 1 and 2 overlap on the chooser probes. The copies were deleted
after the runs and are not committed.

## What the fixture asserts

47 comparisons of the whole decoded screen plus the cursor tuple, at named settled checkpoints:

- command prompt: opened, typed, a long input scrolled the way `prompt.c` `prompt_draw` scrolls
  it, the cursor moved back into it, 60x20 and back, `status-position top`, cancelled
- confirm-before: opened, resized to 60x20, refused
- display-menu (`-x 4 -y 12 -T`, shortcut annotations, a separator, a disabled row): opened, Down,
  Down past the separator, an unanswered `z`, a message over it, 100x30, Escape, the `s`
  shortcut, and a `-M` menu opened and chosen by a button-1 click
- display-popup (`-w 34 -h 9 -T -E`): opened, a line typed into the job, a message over it,
  100x30, closed
- display-panes over a horizontal split: the split alone, the labels, 80x30 clearing them, a
  digit selecting pane 1, `Z` closing them and reaching the pane
- at 79x23, an odd height: a centred display-menu (`-x C -y C`) opened and after Down, and a
  centred display-popup of the default size opened, typed into and closed, once with
  `menu-style`, `menu-selected-style`, `menu-border-style`, `popup-style` and
  `popup-border-style` set fg-only on both sides and once set fg plus bg; a centred `-M` menu
  opened and chosen by a click; display-panes with `display-panes-colour colour33` and
  `display-panes-active-colour colour124` on both sides, shown and closed by a digit

13 of the 47 run in `text` mode: every glyph, every column, the cursor and the style of every row
except the prompt/message row are asserted (under `status-position top` the row after it is left
out too, since it only carries the SGR capture-pane continues from row 0). Only that row's style is
recorded, each with the reason `SIBLING:modes ...`: the raw TUI draws it in its own overlay
appearance while the pin resolves `message-style` and `message-command-style`, which TUI-004
publishes and consumes this cycle.

## What the review found and what changed

- Odd-height centring. `cmd_display_menu_get_pos` centres and clamps on `tty->sy`, the client's
  full height; zz's `popup_client_geometry` used the window's height, which leaves out the status
  line, so at 79x23 a centred popup and a `-x C -y C` menu sat one row higher and a click on a
  centred `-M` menu chose another item. `popup_client_geometry` now takes the client's own size
  when the client reported one, and falls back to the window extent otherwise (the GUI).
  `tui-overlays-prefix` is the before.
- fg-only overlay styles. The pin leaves a style's missing background as the terminal's default
  ground; the raw TUI painted its appearance RGB there. The menu and popup paths now map a missing
  fg or bg to default. `tui-overlays-prefix` shows the fg cases red; the self-check's one-sided
  `menu-border-style` is the sabotage.
- display-panes colours are measured at non-default values on both sides, with a one-sided
  `display-panes-active-colour` sabotage, so a raw TUI that ignored the published values would
  fail.

## Declared comparison rule

In the display-panes cases, and only there, the SGR immediately around each vertical divider
glyph is stripped from both captures. The divider's colour is the border record
`compat/tui-screen-diff.sh` keeps under `BORDER_STYLE_REASON`, not this surface. The coloured
display-panes cases run after the centred menus for that reason: the rule would strip a menu
border's SGR too.

## Findings outside this obligation, reported and not fixed

Measured 2026-09-10 while writing the display-panes case: an 80-column `-h` split grown to 100
columns splits 50|49 on the pin and 49|50 on zz, and shrinking back leaves zz's left pane
showing scrollback lines the pin's does not. Both are pane geometry and terminal resize, so the
display-panes resize changes the height only, and the odd-size section closes the split before
it resizes.
