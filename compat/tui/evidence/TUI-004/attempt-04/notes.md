# TUI-004 attempt-04: borders, match styles, real splits

Cycle 6, the modes lane, punch list items 0 to 4. Base 5d9bf198
(origin/campaign/tui-cycle5-gated). Every `*-tip.txt` run is at code revision
a25c5b39 with a clean tree, against the binary whose sha256 is in
`environment.txt`. The commit that adds this directory's tip runs changes
nothing but evidence and the ledger.

## What asserts, and what remains

Asserts at the tip: attached-client PASS; tui-indicators 23 of 23 with 0
recorded, three runs, the divider row asserted whole; tui-screen-diff 113
asserted checkpoints identical, both pane-border checkpoints and theme-light
among them at every size; tui-pane-geometry 6 of 6; status-row 14 of 14 under
`LC_ALL=C LC_TIME=C`; tui-stock-keys 50 cases agree; the throwaway copy-mode
flip 115 cases agree; the focus test 20 of 20 exact-solo.

Remains, and why TUI-004 stays `active`: tui-screen-diff's splits were never
real (below). With real splits, `status-two-rows` and `status-top` differ at
80x10 and 80x6 and are recorded. That is clause 2's "multiple rows", and the
fix is in zz-mux's window sizing, outside this lane's zones.
`styled-left-trim` also stays recorded (zz-mux format trim).

## Item 0: attached-client and the focus-test flake

`wait_for_visible_mode` gave zz its own pattern, the old ` COPY n/m ` status
badge, which cycle 5 replaced with the pin's in-pane `[n/m]`. Both sides now
wait for `\[[0-9]+/[0-9]+\]`, and `wait_for_ordered_current_lines` strips a
trailing `[n/m]` on both sides, not only the pin's. No other zz-only
expectation was false; the fixture passes to the end.

The flake, measured (`daemon-focus-flake.txt`): 0 of 20 failures at 3319ceba,
5 of 20 at BASE. Instrumented runs show the stray `StatusChanged` is published
by the `zz-pane-0` thread: the output-view terminal's watcher syncs the pane
title asynchronously and publishes the client's first status render. The test
never waited for it, and BASE's heavier status and mode work moved that
publication past the test's baseline. The product behaviour is right (a title
change re-renders the status), so the test waits for the fixture title, then
renders the status once, before its baseline. 40 of 40 after the change, 20 of
20 at the tip (`daemon-focus-flake-tip.txt`). An attempt to serialize status
refreshes under the renderer lock made it worse (10 of 40) and was dropped.

## Item 1: borders

Pin, read from screen-redraw.c and window-border.c: `redraw_draw_border_span`
starts from `grid_default_cell` and applies `window_pane_get_border_style`:
`pane-active-border-style` (default `fg=#{?pane_marked,thememagenta,...,themegreen}`)
for the client's active pane and `pane-border-style` (`fg=themelightgrey`) for
the others, with `redraw_mark_two_pane_colours` splitting a two-pane divider
by half. The raw TUI painted its own theme over an explicit ground.

Decision recorded under the contract: old behaviour, the raw TUI's own divider
colours (`\e[38;2;216;222;233m\e[48;2;16;19;24m`, active `\e[38;2;77;163;235m`);
the pin's measured behaviour, `\e[38;2;179;179;179m` and `\e[38;2;154;205;50m`
on the default ground; decided 2026-09-11 by the orchestrator under fabrico's
TUI parity contract of 2026-09-09; reversible.

- Wire (folded into 101): `StatusLine.pane_borders: Vec<PaneBorderPresentation { pane, style }>`
  after `modes`, capped at 256, each style empty or parsing. The daemon expands
  the two options per pane of the client's current window with the live engine
  (`border_presentations` in daemon.rs). GUI clients ignore it; the snapshot's
  `border_colour` fields are untouched.
- Raw TUI: dividers and pane status rows take the published style over the
  default ground; the border repaint is keyed on the published borders and the
  theme, so a `set-option` or a theme change repaints them.
- Found on the way: tui-screen-diff's `run_on_both_active` put `-t` after
  split-window's shell command. tmux stops option parsing at the first
  argument, so `-t %0` went to `/bin/sh`, the new pane exited, and every split
  in the file measured one pane (`pin-probes.txt`, probe 1 and 2). That is why
  the file's plain split never reproduced the divider difference tui-indicators
  saw. With the target first, the splits are real, and three more differences
  showed up:
  - The raw TUI spent a row of every pane's box on its pane status line. The pin
    (`layout_add_horizontal_border`) does that only for a pane on the window
    edge; the others draw their status line on the border row beside them.
    Fixed in layout.rs (`PaneRect.status_on_border`).
  - theme-light's divider kept the dark grey, because the repaint key had no
    theme. Fixed.
  - `status 2` with a split: recorded, see above.
- tui-screen-diff's settle accepts the marker from the active pane's own
  history when the pane is too short to keep it on screen (one content row at
  80x6 under pane-border-status). The screen still has to hold still between
  two polls.
- Sabotages: tui-screen-diff `border style, pane-border-style fg=red on one
  side` (rows); tui-indicators `divider, pane-border-style fg=red on one side`
  (rows).

## Item 2: match highlight painting

`window_copy_update_style` replaces a matched cell's attributes and colours
with `copy-mode-match-style`, or with `copy-mode-current-match-style` inside
the match the cursor is on; the selection merges on top at draw time. zz-terminal
already publishes `SearchMatch` and `SearchCurrent` overlay spans, and the raw
TUI painted them in reverse video.

- Wire (folded into 101): `ModePresentation.match_style` and
  `current_match_style` after `vi_keys`, expanded per mode like the position
  and selection styles.
- Raw TUI: a matched cell is reset and painted in its match style (the current
  match wins over a plain match), then the selection merges over it.
- Gap items: none to close. Both options are already in
  `TMUX_OPTION_CONSUMERS`, and neither is an item of any gap in
  compat/tmux-gaps.json (options.native-mode-styles lists neither), so
  compat/tmux-gaps.json is unchanged.
- compat/tui-copy-mode.sh is not edited. `copy-mode-match-flip-tip.txt` is a
  THROWAWAY FLIP: that fixture with its 13 MATCH_REASON cases asserting rows
  (six copy_case lines run for both tables, plus
  emacs-ordinary-pane-search-backspace through SEARCH_OPEN_MODE). 115 of 115
  cases agree. The copy lane's gate flips the 13 SIBLING:modes cases.

## Item 3: still-unproduced formats

Pin (`pin-probes.txt`, probe 3): `#{top_line_time}` is 0 on entering copy mode,
and becomes a timestamp once the view's top line is in history; the default
`copy-mode-position-format` then draws `10:55 [22/78]` where zz draws
`[22/78]`, because zz-terminal keeps no line times. No fixture asserts a
copy-mode cell at the default format: tui-indicators pins the format without
the time, tui-copy-mode pins it to `''`, attached-client only greps for
`[n/m]`, and no other fixture enters copy mode. No fixture reads
`#{copy_position}` through display-message. Neither format reaches an asserted
cell at a default setting; the time prefix is a real difference whose fix
needs line times in zz-terminal.

## Code, hunk by hunk

render.rs:
- `Renderer` fields: `border_chrome` gains the published borders and the theme;
  `match_mask`, `match_styles` (and their constructor lines).
- `paint_workspace`: the border repaint key; dividers painted through
  `write_border_text` when a pane's style is published; `match_styles` set and
  cleared beside `selection_style`.
- `paint_border_status_row`: takes the pane instead of a colour and paints
  through `write_styled_text_over` when a style is published.
- `write_match_sgr` (new method) and `blit_row`: match kinds from the
  `SearchMatch`/`SearchCurrent` overlays, the match kind in the SGR run key, the
  match style written at both SGR sites.
- New free functions `grounded`, `write_border_text`, `write_styled_text_over`,
  placed before `write_tmux_sgr`.

Elsewhere: layout.rs `PaneRect.status_on_border`, `content`, `status_row`,
`resolve`; state.rs `Model::pane_border_style`; mode_view.rs test literal;
zz-protocol message.rs and lib.rs (the two appends and their validation);
zz-daemon status.rs (`StatusRequest.pane_borders`, `expand_style` made
`pub(crate)`, `mode_presentation`), daemon.rs (`status_request`,
`border_presentations`, the focus test's wait);
knowledge/protocol/wire-protocol.md (the v101 entry).

## Files

| file | what ran |
|---|---|
| `environment.txt` | binaries, revision, clean state, pin, OS, TERM, shell, locale. Read first. |
| `proofs-at-tip.txt` | every proof command at the tip with its exit code. |
| `attached-client-item0.txt` | attached-client.sh after the item-0 change, before any code change. |
| `attached-client-tip.txt` | attached-client.sh at the tip: PASS. |
| `daemon-focus-flake.txt` | 20 solo runs at 3319ceba and at BASE, 40 after the test change, and the failing assertion. |
| `daemon-focus-flake-tip.txt` | 20 solo runs at the tip. |
| `indicators-item1.txt`, `indicators-self-check-item1.txt` | tui-indicators.sh on the item-1 working tree. |
| `indicators-run-1.txt` .. `-3.txt`, `indicators-self-check.txt` | tui-indicators.sh at the tip, three runs and the self-check. |
| `screen-diff-item1.txt` | tui-screen-diff.sh on the item-1 working tree: real splits, four status-2 DIFFs before they were recorded (exit 1). |
| `screen-diff-status-record.txt` | tui-screen-diff.sh once those four were recorded. |
| `screen-diff-tip.txt`, `screen-diff-self-check.txt` | tui-screen-diff.sh and its self-check at the tip. |
| `pane-geometry-tip.txt` | tui-pane-geometry.sh at the tip. |
| `status-row-tip.txt` | status-row.sh under `LC_ALL=C LC_TIME=C` at the tip. |
| `stock-keys-tip.txt` | tui-stock-keys.sh at the tip. |
| `copy-mode-match-flip-item2.txt` | the throwaway flip on the item-2 working tree. |
| `copy-mode-match-flip-tip.txt`, `copy-mode-match-flip-diff.txt` | the throwaway flip at the tip and its diff against the committed fixture. |
| `pin-probes.txt` | the split-window target order, and `#{top_line_time}` on entry and after page-up. |
| `unit-tests-protocol-tui.txt`, `zz-daemon-unit-tests.txt`, `zz-daemon-integration-tests.txt`, `zz-integration-tests.txt` | cargo test per touched crate, and `-p zz`. |
| `clippy-tip.txt`, `trackers-tip.txt` | clippy for the touched crates and both tracker checks. |

cargo fmt: the lines this branch wrote are formatted. `cargo fmt --check` still
reports a hunk in daemon.rs near `cancel_prefix` (from BASE) and two in
zz-tui/src/input.rs; neither is this branch's, and neither was reflowed.
