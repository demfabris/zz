# TUI-007 attempt-01

Prompts, menus, popups and pane labels, measured and closed in one pass (cycle 5, overlays lane,
2026-09-10, alienware). Every run below ran at `36fb9f7c`, the commit that carries the fixture and
the code, with a clean tracked tree and `target/debug/zz` built from that tree.

| File | What it is |
| --- | --- |
| `environment.txt` | Both binaries' sha256, the zz revision and clean state, the pin, OS, TERM, shell, bash, locale |
| `tui-overlays-run-1.stdout.txt` | `compat/tui-overlays.sh`, exit 0: `all 33 asserted comparisons identical, 13 recorded not asserted` |
| `tui-overlays-run-1.stderr.txt` | stderr of run 1 (empty) |
| `tui-overlays-run-1.exit.txt` | exit status of run 1 |
| `tui-overlays-run-2.stdout.txt` | the same fixture again, exit 0, same summary |
| `tui-overlays-run-2.stderr.txt` | stderr of run 2 (empty) |
| `tui-overlays-run-2.exit.txt` | exit status of run 2 |
| `tui-overlays-run-3.stdout.txt` | the same fixture a third time, exit 0, same summary |
| `tui-overlays-run-3.stderr.txt` | stderr of run 3 (empty) |
| `tui-overlays-run-3.exit.txt` | exit status of run 3 |
| `tui-overlays-self-check.stdout.txt` | `compat/tui-overlays.sh --self-check`, exit 0: four sabotages caught, one equivalence passed |
| `tui-overlays-self-check.stderr.txt` | stderr of the self-check (empty) |
| `tui-overlays-self-check.exit.txt` | exit status of the self-check |
| `tui-screen-diff.stdout.txt` | `compat/tui-screen-diff.sh`, exit 0: `all 111 asserted checkpoints identical, 42 recorded not asserted` |
| `tui-screen-diff.stderr.txt` | stderr of that run |
| `tui-screen-diff.exit.txt` | exit status of that run |
| `attached-client.stdout.txt` | `ZZ_BIN=target/debug/zz TMUX_BIN=<pin> compat/attached-client.sh`, exit 0: `attached-client compatibility: PASS` |
| `attached-client.stderr.txt` | stderr of that run |
| `attached-client.exit.txt` | exit status of that run |
| `notes.md` | this file |

Environment for every fixture run: `PATH=/opt/homebrew/bin:$PATH`,
`ZZ_COMPAT_TMUX=/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux`,
`ZZ_COMPAT_CORPUS=/home/demfabris/dev/zz/compat/.cache/plugins`. An earlier attached-client call
without `ZZ_BIN` and `TMUX_BIN` stopped at the fixture's usage line; its files were overwritten by
the real run above.

## What the fixture asserts

33 comparisons of the whole decoded screen plus the cursor tuple, at named settled checkpoints:

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

13 of the 33 run in `text` mode: every glyph, every column and the cursor asserted, and only the
prompt or message row's style recorded, each with the reason `SIBLING:modes ...`. The raw TUI
draws that row in its own overlay appearance while the pin resolves `message-style` and
`message-command-style`, which TUI-004 publishes and consumes this cycle. Checked against run 1:
the only styled rows that differ in those cases are the message row (23 at 24 rows, 19 at 20,
29 at 30) and, for `prompt-status-top`, rows 0 and 1, where row 1's glyphs are identical and only
the SGR that capture-pane carries over from row 0 differs.

## Declared comparison rule

In the display-panes cases, and only there, the SGR immediately around each vertical divider
glyph is stripped from both captures. The divider's colour is the border record
`compat/tui-screen-diff.sh` keeps under `BORDER_STYLE_REASON`, not this surface.

## Findings outside this obligation, reported and not fixed

Measured 2026-09-10 while writing the display-panes case: an 80-column `-h` split grown to 100
columns splits 50|49 on the pin and 49|50 on zz, and shrinking back leaves zz's left pane
showing scrollback lines the pin's does not. Both are pane geometry and terminal resize, so the
display-panes resize changes the height only and the case re-marks after the round trip.
