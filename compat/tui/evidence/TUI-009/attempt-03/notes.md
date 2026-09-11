# TUI-009 attempt-03: the pane-colours regression, default grounds, -T RGB and a silent terminal

Cycle 6, the caps lane, on alienware against pinned tmux d77c9dc6. Both
binaries attach inside one outer pinned tmux, except for the silent-terminal
cases described below. attempt-01 and attempt-02 are read-only history.
Everything from `01` to `13` ran at `95d3644e` with a clean worktree. The files
`14` to `16` are real runs from earlier in the attempt, and each one names the
tree it ran on.

## The punch list

**1. The regression that skipped this branch: fixed.** smoke/pane-colours-palette
was green at origin/main and red here (7 of 18 zz-side checks). The attached
client never wrote the pane-colours RGB. `daemon.rs` `pane_palette` writes the
option into the pane's default palette. `palette_class` compared a cell
against that same default palette, so an option entry never looked changed.
The cell went out as `\e[31m` where the pin writes `38;2;18;52;86`. Now:

- The daemon hands the terminal worker the class of every entry it overlays.
  RGB for `#rrggbb`, the colour's own index for `colourN`
  (`TerminalAppearance::palette_classes`).
- The worker's `Classifier` uses that class when the current entry still
  equals the default.
- An OSC 4 over the entry still goes RGB, and OSC 104 returns to the option.

`07` is the corpus row, exit 0. tui-caps.sh gains the pane stages (an RGB and
an indexed entry, an OSC 4 over the option and OSC 104 back to it, an
untouched entry, the array unset) and two sabotages. Clause 3 is re-proved
whole in `01`-`06`.

**2. What the cycle-5 record left open.**

- **-T RGB: fixed and asserted.** `client_term_features` added `256`
  whenever the client's colour count reached 256, and a requested RGB raises
  the count to 16777216. So `-T RGB` gave `256,RGB` where the pin's
  `tty_feature_rgb` adds only `RGB`. The roster now takes its derived colour
  bits from TERM and COLORTERM alone. `client_colours` still counts the
  requested features. The daemon test
  `requested_colour_features_join_the_roster_alone` failed under the old rule
  (the roster carried 256 beside RGB) and passes now.
  - The outer pinned tmux cannot show this. It answers every query, and the
    pin learns 256 and RGB from the reply. So tui-caps.sh gains a **silent
    terminal**: a relay that runs the client on its own pty, copies
    everything it writes to the outer pane (still the decoder), and sends
    nothing back. That is what script(1) is.
  - There, neither baseline carries 256 or RGB. `silent/-T
    delta-client_termfeatures` is `RGB` on both sides and `silent/-2` is
    `256`. The sabotage hands the pin `-T 256,RGB` against zz's `-T RGB`,
    which is the shape of the old bug, and it is caught.
- **The -2 screen effect: measured and recorded, not fixed.**
  - On the silent terminal under TERM=xterm, the pin writes the `38;5;42`
    cell as `32` and the RGB cell as `30`. Under `-2` it writes the RGB cell
    as `38;5;233`.
  - The mechanism is tty.c `tty_check_fg` over `colour_find_rgb` and
    `colour_256to16`. The raw TUI writes both cells unchanged
    (`colours/silent`, `colours/silent-2`, recorded).
  - A downgrade from TERM alone would break every terminal that answers,
    because the pin learns RGB there. The fixture's own colour case under
    xterm-256color would lose its RGB cell. Matching needs the raw TUI to
    learn its terminal's features from the replies, which is
    options.client-terminal-negotiation.
- **Extended keys: the exact remaining difference.**
  - Under the outer tmux, both sides arm and every extended-keys row asserts,
    as in attempt-02.
  - On the silent terminal with `extended-keys on`, `pane_key_mode` is
    `VT10x` on the pin and `Ext 2` on zz. The pin writes Eneks only for a
    terminal carrying extkeys (tty.c `tty_update_features`), and
    `terminal-features` gives xterm* none. tty.rs arms whenever the option is
    not off.
  - Recorded as `silent/extended pane_key_mode`. It needs the same reply
    negotiation.
- **OSC 10/11 and window-style: driven and fixed.**
  - The pin paints every cell that names no colour, and every clear, in the
    pane's default ground (tty.c `tty_default_colours`).
  - Old behaviour: the raw TUI wrote `39` and `49` for all of them.
  - Now:
    - The daemon hands the worker the class of the window-style ground
      (`TerminalAppearance::default_classes`).
    - The worker classes an uncoloured ground as RGB when the terminal's
      current ground differs from the configured one (an OSC 10/11
      override). Otherwise it takes the window-style class, or default.
    - The viewport's style 0 carries the same classes.
    - The raw TUI's trailing clear sets that background before it erases.
  - Measured: OSC 10 as `38;2;255;0;0`, OSC 11 as `48;2;0;0;128`,
    `fg=colour2,bg=colour4` as `32`/`44`, `fg=#102030,bg=colour200` as
    RGB/`48;5;200`. OSC 110, OSC 111 and unsetting all go back to `39`/`49`.
  - Decided 2026-09-11 by the orchestrator under fabrico's TUI parity contract
    of 2026-09-09; reversible.
- **Found on the way: OSC 110 and OSC 111 froze the default grounds.**
  libghostty resets a dynamic colour to the default *of that moment* and
  keeps it as an override (`16`). So a later theme or window-style change
  never reached a pane whose program had reset its colours, in the GUI too.
  `14` is the fixture catching it: the window-style stages after the OSC
  stages painted zz's stale theme grounds.
  - `apply_terminal_appearance` now notes which ground was following its
    default. After setting the new default it writes OSC 110 or 111 again for
    that ground.
  - A real OSC override survives an appearance change, as the pin's does.
  - The unit test
    `an_osc_reset_leaves_the_default_grounds_following_the_appearance` pins
    both halves.
- **Named, not driven:**
  - An OSC 4 or OSC 10 that sets exactly the configured value. libghostty
    exposes no override mask.
  - OSC 10 under a window-style foreground. The pin's window-style wins over
    the pane's OSC 10 colour. zz applies window-style as the terminal's
    configured default and OSC 10 overrides it, in the GUI too.

**3. Ledger.** TUI-009 stays `active`. Clause 3 asserts with no recorded row.
Clauses 1 and 2 still hold recorded rows this lane cannot close in its zones:

- `wrap_flag`
- `mouse_all_flag` and `mouse_button_flag` (app.rs)
- `keypad_flag` and `keypad_cursor_flag`
- `silent/extended pane_key_mode`
- the roster rows
- `widths/non-utf8 line` (render.rs's glyph path)
- `colours/silent` and `colours/silent-2`

No compat/tmux-gaps.json item is closed: this landing makes none of the named
gaps' items match that did not match before.

## Fixture hygiene found this attempt

Every widths case printed `W1` into the same inner pane, and the pane keeps
the previous case's output across the re-attach. A case could settle on the
old line before its own command ran, and then read the cursor off the typed
command. That is `widths/non-utf8/cursor tmux 20,0, zz 0,2` in two of five
runs (`15`). Each case now prints and waits for its own number.

## Code, by place

- `crates/zz-daemon/src/daemon.rs`:
  - `pane_palette` and `pane_terminal_appearance` carry each colour's class
    through the new `tmux_colour_class`.
  - `client_term_features` derives its colour bits through
    `client_colour_count_with(.., 0)`.
  - The new test `requested_colour_features_join_the_roster_alone`.
- `crates/zz-terminal/src/appearance.rs`: `palette_classes` and
  `default_classes`, both `serde(skip)`, so the GUI sees nothing new.
- `crates/zz-terminal/src/session.rs`:
  - `ClassHints` and `Classifier` replace `ground_class` and `palette_class`,
    with the hints kept in `ViewportDictionary`.
  - `build_snapshot` and `capture_history` class every ground through them
    and class style 0.
  - `apply_terminal_appearance` does the OSC reset follow-up.
  - Three unit tests.
- `crates/zz-tui/src/render.rs`: one hunk, in `Renderer::blit_row`, the
  trailing clear. It writes the default style's background class before the
  ECH, and only when that class is not plain default, so the bytes are
  unchanged when no pane ground is set.
- The wire is unchanged. The class word per style already existed. Style 0
  now carries a class too.

## Files

- `environment.txt`: the revision and worktree state, both binary hashes,
  the pin, the OS line, TERM, shell, python and locale.
- `01-tui-caps-run-1.txt`, `02-tui-caps-run-2.txt`, `03-tui-caps-run-3.txt`:
  three runs, exit 0, 210 asserted rows identical and 56 recorded, with
  identical row dispositions and identical recorded values.
- `04-tui-caps-self-check.txt`: 16 sabotages caught, both controls quiet,
  exit 0.
- `05-tui-screen-diff.txt`: 123 asserted checkpoints identical, 30 recorded,
  exit 0.
- `06-tui-screen-diff-self-check.txt`: every sabotage caught and every
  equivalence passed, exit 0.
- `07-pane-colours-palette.txt`: `compat/run.sh smoke/pane-colours-palette`,
  0 divergences, exit 0.
- `08-status-row-c-locale.txt`: `LC_ALL=C LC_TIME=C`, 14/14, exit 0.
- `09-related-corpus-rows.txt`: the rows that set window-style
  (pane-selection-input-style, renderer-styles, pane-spawn-style-title-v2) or
  read `client_termfeatures` (smoke/terminal-facts, smoke/format-listing),
  0 divergences, exit 0.
- `10-tui-copy-mode.txt`: `capture_history` changed, so this was re-run.
  Exit 0, and the recorded cases are the SIBLING:modes search-match rows.
- `11-attached-client.txt`: stops at the known copy-mode badge wait (the
  modes lane's), exit 1. Nothing past that step.
- `12-cargo-and-clippy.txt`: `cargo test` for zz-tui, zz-terminal, zz-daemon
  and zz (cli_binary included), and clippy for the three touched crates, all
  at `95d3644e`.
- `13-tracker-check.txt`: the ledger validator.
- `14-before-osc-reset-window-style.txt`: before the OSC reset fix, the
  window-style stages painting stale theme grounds.
- `15-before-widths-marker.txt`: the two runs that hit the shared widths
  marker.
- `16-osc-reset-probe.txt`: libghostty's effective and default foreground
  across OSC 10, OSC 110 and an appearance change, before the fix.
