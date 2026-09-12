# TUI-009 attempt-03: what the raw TUI's terminal says, and what it writes back

Cycle 6, the caps lane, on alienware against pinned tmux d77c9dc6. Both
binaries attach inside one outer pinned tmux, except the silent-terminal cases,
which run through the relay described in compat/tui-caps.sh. attempt-01 and
attempt-02 are read-only history. Every file from `01` to `13`, and `17` and
`18`, ran at `3f6dc460` with a clean worktree. `14` to `16` are real runs from
earlier in the attempt and each one names the tree it ran on.

`compat/tui-caps.sh` asserts **243 rows** and records **23**, three runs with
identical dispositions and identical recorded values, and its `--self-check`
catches **21 one-sided sabotages** with **3 controls quiet**. At the start of
this attempt it asserted 210 and recorded 56.

## The punch list

**1. The regression that skipped this branch: fixed.**
`smoke/pane-colours-palette` was green at origin/main and red here (7 of 18
zz-side checks). `daemon.rs` `pane_palette` wrote the option into the pane's
default palette and `palette_class` compared a cell against that same default,
so an option entry never looked changed and the cell went out as `\e[31m`
where the pin writes `38;2;18;52;86`. The daemon now hands the terminal worker
the class of every entry it overlays (`TerminalAppearance::palette_classes`:
RGB for `#rrggbb`, the colour's own index for `colourN`), the worker's
`Classifier` uses it while the entry still equals the default, an OSC 4 over
the entry still goes RGB, and OSC 104 returns to the option. `07` is the
corpus row, 0 divergences.

**2. The -2 screen effect on a silent terminal: fixed and asserted.**
This was the one item the cycle-5 record left measured and unfixed. The pin
turns an RGB colour into its nearest 256 colour when the terminal has no RGB
(`colour_find_rgb`) and a 256 colour into its nearest of sixteen when it has
fewer (`colour_256to16`), in `tty_check_fg` and `tty_check_bg`. The raw TUI
never downgraded. Matching it needed the client to know what its terminal
takes, which is what the old record called
`options.client-terminal-negotiation` and left for later:

- `TerminalGuard::enter` now writes `\e[c\e[>c\e[>q`, the three requests
  `tty_send_requests` writes, where it used to write the primary one alone.
- `terminal_event.rs` decodes the secondary DA reply and the XTVERSION reply
  instead of leaving their bytes to be typed into a pane, and `tty.rs` raises
  what the terminal takes from the name they carry, exactly as
  `tty_keys_device_attributes2` and `tty_keys_extended_device_attributes` hand
  `tty_default_features` a name: `T` and `M` and the seven XTVERSION names
  carry `TTY_FEATURES_BASE_MODERN_XTERM`, which is 256 and RGB; `U` carries
  256 alone.
- `render.rs` `write_palette_ground` and `write_rgb_ground` drop a colour the
  terminal cannot take, through ports of `colour_find_rgb`, `colour_to_6cube`,
  `colour_dist_sq` and `colour_256to16`. A change in what the terminal takes
  invalidates the screen, which is what `tty_update_features` does.
- The colour count itself moved out of `daemon.rs` into
  `crates/zz-daemon/src/terminal_features.rs`, so the daemon's roster and a
  client's own writer read one rule. A terminal with no 256, no RGB and no
  `*-16color` name now carries the **eight** colours terminfo gives `xterm`
  and `screen`, where the roster assumed sixteen; that is what
  `#{client_colours}` reports for the pin on the same terminal.

Measured: on the silent terminal under TERM=xterm both sides write the
`38;5;42` cell as `32` and the `38;2;10;20;30` cell as `30`; under `-2` both
write `38;5;42` and `38;5;233`. Under the outer tmux, which answers, both keep
their classes, and all 123 checkpoints of `compat/tui-screen-diff.sh` stay
identical. The sabotages are `-2` on one side alone, in both directions, with
a control.

The product decision this carries: the raw TUI used to write every cell's own
class at every terminal, so an RGB cell left as `38;2;r;g;b` even where the
terminal takes eight colours. It now writes what its terminal takes, which
means a terminal that sets no truecolor `COLORTERM` and answers no request
loses truecolor in the raw TUI, exactly as it does under the pin. Decided
2026-09-11 by the orchestrator under fabrico's TUI parity contract of
2026-09-09; reversible.

**3. What else the cycle-5 record left open.**

- **-T RGB: fixed and asserted** in the earlier half of this attempt.
  `client_term_features` added `256` whenever the colour count reached 256 and
  a requested RGB raised that count, so `-T RGB` gave `256,RGB` where
  `tty_feature_rgb` adds only `RGB`. The roster now takes its derived colour
  bits from TERM and COLORTERM alone. The daemon test
  `requested_colour_features_join_the_roster_alone` pins it.
- **OSC 10/11 and window-style: driven and fixed** in the earlier half. The
  pin paints every cell that names no colour, and every clear, in the pane's
  default ground (`tty_default_colours`); the raw TUI wrote `39` and `49`
  whatever the pane's ground. Now the daemon hands the worker the class of the
  window-style ground, the worker classes an uncoloured ground as RGB when the
  terminal's current ground differs from the configured one, the viewport's
  style 0 carries the same class, and the raw TUI's trailing clear sets that
  background before it erases. Measured: OSC 10 as `38;2;255;0;0`, OSC 11 as
  `48;2;0;0;128`, `fg=colour2,bg=colour4` as `32`/`44`,
  `fg=#102030,bg=colour200` as RGB/`48;5;200`, and OSC 110, OSC 111 and
  unsetting all back to `39`/`49`. Decided 2026-09-11 by the orchestrator
  under fabrico's TUI parity contract of 2026-09-09; reversible.
- **Found on the way, earlier in the attempt: OSC 110 and OSC 111 froze the
  default grounds.** libghostty resets a dynamic colour to the default of that
  moment and keeps it as an override, so a later theme or window-style change
  never reached a pane whose program had reset its colours, in the GUI too.
  `apply_terminal_appearance` now re-resets a ground that was following its
  default, and a real OSC override survives as the pin's does. `14` is the
  fixture catching the old behaviour and `16` the libghostty probe.
- **Extended keys: the exact remaining difference, unchanged.** Under the
  outer tmux both sides arm and every extended-keys row asserts. On the silent
  terminal with `extended-keys on`, `pane_key_mode` is `VT10x` on the pin and
  `Ext 2` on zz: `tty_update_features` writes Eneks only for a terminal
  carrying `extkeys`, which `terminal-features` does not give `xterm*` and no
  reply supplied, while `tty.rs` arms whenever the option is not off. The
  arming decision is taken in `TerminalGuard::enter`, before any reply can
  arrive; deferring it until the terminal has answered is the remaining work
  and it is named in `next_action`. Recorded as
  `silent/extended pane_key_mode`.

## Found inside the obligation and fixed, beyond the punch list

Each of these was a recorded row of clause 1 or clause 2 that this lane's own
zones could close.

- **Autowrap.** `TerminalGuard::enter` wrote `\e[?7l` and restored `\e[?7h`;
  `tty_start_tty` never touches DECAWM and tracks the last cell itself. Both
  removed, and the renderer places the cursor absolutely before every row it
  writes, so nothing wraps. `wrap_flag` asserts in all four cases. The
  sabotage turns autowrap off on zz's outer pane just before its client
  reaches it, with every other mode row recorded so wrap_flag is the only row
  that can report.
- **The keypad.** The pin writes `smkx` on start and `rmkx` on stop and
  decodes the sixteen sequences `tty_default_raw_keys` names; the raw TUI
  wrote neither and decoded none. `tty.rs` now writes `\e[?1h\e=` and
  `\e[?1l\e>`, and `ss3_key` decodes the keypad to the character on the key,
  which is what `input-keys.c` sends a pane out of application-keypad mode.
  `keypad_flag` and `keypad_cursor_flag` assert in all four cases. The
  sabotage detaches zz alone: those two rows are 1 only while a client that
  armed smkx is attached.
- **The theme.** `c->theme` stays `THEME_UNKNOWN` until the terminal answers,
  and `format_cb_client_theme` gives nothing for it; a zz client always
  carried one and answered `dark` from the moment it attached. `tty.rs` now
  writes `\e[?2031h\e[?996n` on entry and `\e[?2031l` on exit, the parser
  decodes `\e[?997;1n` and `\e[?997;2n`, `input.rs` hands the daemon the
  scheme it was told, and the connect path reports no scheme until then. All
  eight `client_theme` rows assert. The sabotage gives the pin's outer window
  a background its decoder can answer an OSC 11 query with (`input_osc_11`
  declines while `window_pane_get_bg` is -1, which is why neither side learns
  anything here otherwise), and the pin's client_theme leaves empty while
  zz's stays.
  - The product decision this carries: a zz client used to carry a theme
    always and answer `dark` from the moment it attached, which is the stance
    the gap accepted on 2026-09-07; a raw TUI client now reports nothing until
    its terminal answers, and dark or light on the answer. Decided 2026-09-11
    by the orchestrator under fabrico's TUI parity contract of 2026-09-09;
    reversible.
  - This closes `semantic:harness-theme-steering` on
    `options.client-terminal-negotiation` for the raw TUI, with the dated
    measurement in the gap's reason. The gap keeps its decision for the
    desktop client, which is the terminal and reads its own appearance, and
    for the seven items it still holds. Its third acceptance clause is
    rewritten to say so.
  - `compat/scenarios/smoke/fixtures/format-listing.sh` recorded the old
    stance with a per-side branch. It no longer branches: both sides answer
    the empty string before any reply and `dark` after it, and
    `smoke/format-listing` is 0 divergences (`09`).

## What is still recorded, and why

23 rows, in two families and two singletons.

- `mouse_all_flag` and `mouse_button_flag`, four cases each: zz arms
  `\e[?1003h` for the whole attach where the pin arms `\e[?1002h` and raises
  MODE_MOUSE_ALL only while a menu is up. The two rows move together and are
  one divergence, whose fix is per-menu arming in `crates/zz-tui/src/app.rs`,
  owned with the overlays this cycle and outside this lane's zones.
- `client_colours` (five cases) and `client_termfeatures` (eight): the roster
  half of `options.client-terminal-negotiation`. The raw TUI's own cell writer
  now reads the terminal's replies, but the daemon derives a client's roster
  from its TERM, its COLORTERM and its flags alone, because the hello is sent
  before any reply can arrive and nothing carries a later one. Under the outer
  tmux the pin's roster is the `tmux` entry of `tty-features.c` and zz's is
  its own fixed list; on a silent terminal the pin's is what terminfo gives
  `xterm` and zz's is again the fixed list. `client_colours` asserts on the
  silent terminal, where there is no reply for either side to learn from.
- `widths/non-utf8 line`: under LANG=C with no `-u` the pin draws each
  non-ASCII cell as underscores (`tty_check_codeset`) and the raw TUI writes
  the UTF-8 glyph. That is `render.rs`'s glyph path, outside this lane's zone
  for that file.
- `silent/extended pane_key_mode`: above.

Clause 3 asserts with no recorded row. Clause 1 holds the mouse rows, the
widths row and the silent pane_key_mode row; clause 2 holds the roster rows.
TUI-009 stays `active`.

## Code, by place

- `crates/zz-daemon/src/terminal_features.rs`, new: the feature table,
  `terminal_feature_bit`, `terminal_features_list`, `terminal_feature_mask`
  and `terminal_colour_count`, moved out of `daemon.rs` unchanged except for
  the colour rule's eight-colour floor and its `*-16color` case. Always
  compiled, so a client build without the daemon feature reads the same rule.
- `crates/zz-daemon/src/daemon.rs`: `client_colour_count_with` and
  `client_features_fact` call it; `pane_palette` and
  `pane_terminal_appearance` carry each colour's class; `client_term_features`
  derives its colour bits through `client_colour_count_with(.., 0)`; the test
  `requested_colour_features_join_the_roster_alone`.
- `crates/zz-daemon/src/client.rs`: `client_terminal_colour_count` for a
  client asking about its own terminal;
  `connect_endpoint_with_prompts_and_terminal` takes an optional scheme and
  `connect_endpoint_without_theme` and `connect_terminal_surface_without_theme`
  pass none.
- `crates/zz-terminal/src/appearance.rs`: `palette_classes` and
  `default_classes`, both `serde(skip)`, so the GUI sees nothing new.
- `crates/zz-terminal/src/session.rs`: `ClassHints` and `Classifier` replace
  `ground_class` and `palette_class`, kept in `ViewportDictionary`;
  `build_snapshot` and `capture_history` class every ground and style 0
  through them; `apply_terminal_appearance` does the OSC reset follow-up.
- `crates/zz-tui/src/tty.rs`: `TERMINAL_REQUESTS`, `KEYPAD_TRANSMIT` and
  `KEYPAD_LOCAL`, `THEME_SUBSCRIBE` and `THEME_UNSUBSCRIBE`, the
  `TERMINAL_COLOURS` cell and the two reply notes; `enter` writes the requests
  and no longer writes `\e[?7l`, `Drop` no longer writes `\e[?7h`.
- `crates/zz-tui/src/terminal_event.rs`: `SecondaryDeviceAttributes`,
  `ExtendedDeviceAttributes`, `DarkTheme` and `LightTheme`; a device control
  string branch in `parse_escape`, bounded at 256 bytes the way the control
  sequence branch is bounded at 64, so an unterminated one is read as the
  escape it would have been rather than buffered; the keypad arms of
  `ss3_key`.
- `crates/zz-tui/src/input.rs`: the four new events, three of them terminal
  facts and one a `set_color_scheme`.
- `crates/zz-tui/src/render.rs`, by function: `Renderer::new` and the new
  `Renderer::note_terminal_colours` (the field and the invalidation);
  `Renderer::paint` and `Renderer::paint_frames` (one call each, first line);
  `Renderer::blit_row`'s trailing clear (the earlier half of the attempt);
  `write_palette_ground` and `write_rgb_ground` (the downgrade); the new free
  functions `downgrade_palette`, `colour_256to16`, `colour_find_rgb`,
  `colour_to_6cube` and `colour_distance`. Nothing else in the file moved.
- `crates/zz-tui/src/lib.rs` and `app.rs`: the connect call sites, three
  tokens each, and their imports.
- The wire is unchanged: no message, field or variant was added or altered,
  and PROTOCOL_VERSION stays 101.

This cycle's work landed in three commits: the terminal's own answers and the
cell writer that reads them in the first, the theme subscription and the gap it
closes in the second, the device control string's bound in the third. Autowrap,
the keypad and the colour rule sit in the first, with the negotiation they
share `tty.rs` and `compat/tui-caps.sh` with; splitting those out would have
meant a half-reverted `tty.rs` and a half-reverted fixture in the same commit,
which is worse to read than the one they are in.

## Files

- `environment.txt`: the revision and worktree state, both binary hashes, the
  pin, the OS line, TERM, shell, python, locale and the cargo wrapper.
- `01`, `02`, `03`: three `compat/tui-caps.sh` runs, exit 0, 243 asserted rows
  identical and 23 recorded, with identical row dispositions and identical
  recorded values.
- `04`: `--self-check`, 21 sabotages caught, 3 controls quiet, exit 0.
- `05`: `compat/tui-screen-diff.sh`, 123 asserted checkpoints identical, 30
  recorded, exit 0.
- `06`: its `--self-check`, every sabotage caught and every equivalence
  passed, exit 0.
- `07`: `compat/run.sh smoke/pane-colours-palette`, 0 divergences, exit 0.
- `08`: `compat/status-row.sh` under `LC_ALL=C LC_TIME=C`, 14/14, exit 0.
- `09`: the corpus rows this landing can reach: `smoke/terminal-facts`,
  `smoke/format-listing`, `renderer-styles`, `pane-selection-input-style` and
  `pane-spawn-style-title-v2`, 0 divergences each, exit 0.
- `10`: `compat/tui-copy-mode.sh`, exit 0; its recorded cases are the
  SIBLING:modes search-match rows.
- `11`: `compat/attached-client.sh`, exit 1 at the known copy-mode badge wait,
  which is the modes lane's; nothing past that step.
- `12`: clippy for zz-tui, zz-daemon, zz-terminal, zz-protocol and zz, and
  `cargo test` for all five, `cli_binary` included.
- `13`: the ledger validators.
- `14`: before the OSC reset fix, the window-style stages painting stale theme
  grounds.
- `15`: the two runs that hit the shared widths marker, before each widths
  case printed its own.
- `16`: libghostty's effective and default foreground across OSC 10, OSC 110
  and an appearance change, before the fix.
- `17`: `compat/tui-stock-keys.sh`, which the keypad arming could have moved,
  exit 0.
- `18`: `compat/tui-indicators.sh` and `compat/tui-pane-geometry.sh`, exit 0
  each.
- `19`: the tip after the ledger and evidence commit, whose code and fixtures
  are identical to `0ff4a261` (the cycle-6 caps review caught this line naming
  `3f6dc460`, which is a commit earlier: `0ff4a261` changed
  `crates/zz-tui/src/terminal_event.rs` after it, 1 file and 24 insertions, and
  file `19` itself already named `0ff4a261` correctly): `compat/tui-caps.sh`,
  its `--self-check`,
  `smoke/pane-colours-palette` and both ledger validators, all green there
  too.

# The cycle-6 caps gate

Rebased as local `gate-caps` onto origin/main `33ecbd86` (the cycle-6 copy
gate) in /home/demfabris/dev/zz-gate-tui6. The reviewer's verdict, its
checks_run, what this gate did with each finding and how the suspicions were
weighed are in `review.md`. Everything this gate ran is in `gate-01` to
`gate-16`, each carrying its revision, its command, its exit code and the wall
clock it was taken at, so no two captures are byte-identical.

## What moved on top of the lane

Three commits from the earlier, reboot-killed run of this gate, each checked
against the review and re-proved here:

- `dfeff8cf` the sub-16-colour guard in `downgrade_palette`, with
  `\e[38;5;12mA` and `\e[38;5;9mQ` permanently in the colour sample.
- `0394293c` the two wire appends named in `wire-protocol.md`, and the
  `style_classes` array in both payload tables in `terminal-lanes.md`.
- `82d83713` the three comment blocks in `terminal_event.rs` promoted to `///`
  documentation, and a const assertion guarding the attribute bits.

Two of this run's own:

- `6bae21ff` **PROTOCOL_VERSION 102**. The review judged the appends against
  101 and found them clean as pure appends, which they are; but 101 shipped in
  zz 0.8.0 at `fd3c64e4` before this lane landed, so a released client would
  pass the envelope check and misdecode every frame's style dictionary. The
  v101 entry closes with the tag it shipped in and a v102 entry opens with this
  lane's two appends; `git log fd3c64e4..origin/main -- crates/zz-protocol` is
  empty, so v102 is this lane's alone. Whole story in `gate-16`.
- `5c28f025` the delayed live job in `smoke/jobs-command-environment` gets the
  same four seconds its two siblings in that file already get. Whole story in
  `gate-15`.

## The numbers at the merged tip

The fixtures read differently here than they did on the lane's own base,
because origin/main now carries the modes and copy lanes:

| fixture | lane tip | gate tip |
|---|---|---|
| `tui-caps.sh` | 243 asserted / 23 recorded | **279 / 23** |
| `tui-caps.sh --self-check` | 21 sabotages, 3 controls | unchanged |
| `tui-screen-diff.sh` | 123 asserted / 30 recorded | **137 / 16** |
| `tui-copy-mode.sh` | 115 cases, 13 recorded | **147 cases, 0 recorded** |
| `tui-stock-keys.sh` | 50 agree, 18 recorded | 50 agree, **12** recorded |
| `attached-client.sh` | exit 1 at the copy-mode step | **PASS** |

The caps rows go 243 to 279 because the two aixterm cells now ride in every
colour stage. The screen-diff rows go 123/30 to 137/16 because the modes lane's
border checkpoints are on main and twelve colour checkpoints flipped from
recorded to asserted. copy-mode and attached-client are the copy and modes
lanes landing. The stock-keys recorded count is this box's known wobble.

## The 23 recorded rows, still recorded

Unchanged in count and in identity from what the lane and the reviewer both
report, and reproduced at this tip in `gate-01`:

- 8 mouse rows, `mouse_all_flag` and `mouse_button_flag` on legacy, extended,
  extended-always and silent/extended. zz arms `\e[?1003h` for the whole
  attach; the pin arms `\e[?1002h` and raises MODE_MOUSE_ALL only while a menu
  is up. Per-menu arming in `crates/zz-tui/src/app.rs`, the overlays lane's.
- 5 `client_colours` and 8 `client_termfeatures`, the roster half of
  `options.client-terminal-negotiation`. The daemon derives a client's roster
  from TERM, COLORTERM and flags alone because the hello precedes any reply.
  Needs a wire append with its consumer, which is now a v102 conversation.
- `widths/non-utf8/line`. `render.rs`'s glyph path, excluded from this batch's
  zones for that file.
- `silent/extended pane_key_mode`. The arming has to be deferred to the reply
  path, whose only interleave-safe channel is the renderer's control queue in
  `app.rs`.

## What this gate ran

- `gate-01` .. `gate-12`: every TUI fixture on main at this tip, with its
  `--self-check` where it has one, plus `attached-client.sh`. All exit 0.
  `tui-overlays.sh` and `tui-choosers.sh` are later lanes in the cycle-6 order
  and are not on main at this tip.
- `gate-13`: the five touched packages and workspace clippy, all exit 0.
- `gate-14`: the 182-row corpus selection, every row with a result.
- `gate-15`, `gate-16`: the two findings of this gate's own.
