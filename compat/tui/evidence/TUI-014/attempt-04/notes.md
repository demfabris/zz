# TUI-014 attempt-04, cycle 10, modes lane 2

The batch was clause 2's four remaining commands: clock-mode, switch-mode,
customize-mode and suspend-client. **Two of the four landed — clock-mode and
switch-mode** — on a piece attempt-03 said was missing and named exactly: a
server-owned pane mode. customize-mode and suspend-client did not, so TUI-014
stays `active` and compat/tui-client-commands.sh still records two of its cases.

## The shape

zz had one mode per pane only in the sense that copy mode hangs off each
*client's* terminal view. attempt-03's diagnosis was right and this lane verified
it before building: `#{pane_in_mode}` and `#{pane_mode}` were answered from
`facts.copy_modes`, a `BTreeMap<PaneId, Vec<(clientName, CopyModeFacts)>>`, so a
clientless `list-panes -a` could only ever see a mode some client was holding;
every chooser is keyed by `ClientId`; and no key route ended a pane mode.

What landed is one per-pane mode the **daemon** owns:

- `ServerState.pane_modes: BTreeMap<PaneId, PaneModeRequest>` is the mode list.
  `MuxEffect::PaneModeChanged { pane, mode }` writes it, and `PanesRemoved`
  drops it.
- It reaches clients on the **pane snapshot**: `PaneSnapshot` gains one trailing
  `mode: Option<PaneMode>`, stamped by `stamp_pane_modes` beside the existing
  `stamp_pane_border_chrome`. That is why it needed no fan-out of its own: every
  client attached to the window already gets the window's panes, a client that
  attaches later gets the mode with its first snapshot, and a resize carries it.
- `#{pane_in_mode}` and `#{pane_mode}` answer from `facts.pane_modes`, a new
  `Arc<BTreeMap<PaneId, &'static str>>` built beside `copy_modes`. A server-owned
  mode outranks a client's copy session the way `window_pane_set_mode` pushes
  ahead of it in `wp->modes`.
- Keys: `input_key`'s `KeyDecision::Pass` arm hands the key to `pane_mode_key`
  before `dispatch_input_key`. That is `server_client_key_callback`'s own order —
  a bound key runs its command and leaves the mode up, a key no table claimed
  reaches `window_pane_key`.
- The clock's second: `arm_pane_mode_clock` is one thread that sleeps to the next
  whole second and republishes the snapshot while any pane holds a clock, copied
  from `arm_copy_mode_refresh`'s shape and from `window_clock_start_timer`'s own
  `1000000 - ts.tv_nsec / 1000`.

**What a GUI client sees when a pane enters one of these modes: nothing.**
`PaneSnapshot.mode` is `#[serde(default)]`, the GPUI, iOS and web clients never
read it, and no other field moves, so the pane keeps its own presentation there.
The raw TUI is the only consumer. `cargo clippy -p zz --all-targets -D warnings`
and `cargo test -p zz` are clean at the tip.

## clock-mode

`window_clock_draw_screen` reproduced: the pane is cleared to the default cell,
the time is formatted on the server from `clock-mode-style` (`%H:%M`,
`%H:%M:%S`, or `%l:%M ` / `%l:%M:%S ` with `AM`/`PM` appended from `tm_hour`
rather than a locale's `%p`), and each character is drawn as a 5x5 block from
`window_clock_table` with the resolved `clock-mode-colour` as BOTH grounds, at
`x = sx/2 - 3*len` and `y = sy/2 - 3` with a six-column pitch. The cursor lands
past the last set cell of the last glyph and is hidden, which is 54,12 at 80x24.
Any key no table claimed ends it, which is `window_clock_key`.

The time travels rather than the style that made it: `PaneMode::Clock { time,
colour }`. The server owns the clock the way the pin does, so every client
attached to the window turns the face over together on the same boundary.

`clock-mode-colour` and `clock-mode-style` were typed storage nothing read.
They are read now, so both names moved into `TMUX_OPTION_CONSUMERS` and both
`option:` items left `options.native-mode-styles`; the partition in
`crates/zz-mux/src/compat_manifest_tests.rs` moved with them (consumers 146 ->
148, window scope 43 -> 45, tracked options 34 -> 32).

## switch-mode

`window_switch_mode` reproduced for its opening surface and its teardown.
`switch_rows` expands one row per session, or per window under `-w`, through the
pin's own `WINDOW_SWITCH_DEFAULT_FORMAT` in `chooser_presentation.rs` — the same
`expand_row` the choosers use, with `scope_variables` deciding the
`#{?window_format,...}` branch — and sends each row with its `#[...]` markup
intact. The client draws them through the mode tree's own `Grid`, the current row
over a line cleared to `mode-style`'s background, and `prompt_draw`'s `(search) `
on the pane's last row in `message-style` with the cursor at 9,22 shown.

Two things had to be learned the hard way and are worth carrying forward:

1. **`noattr` on a base cell is not a veto.** zz expands `#{E:mode-style}` to
   `noattr,bg=themeyellow,fg=themeblack`, and `write_tmux_sgr` suppresses every
   attribute when `noattr` is on. A row's own `#[dim]` then vanished. `sgc` in
   the pin is a `grid_cell`, not a style string: as a base cell `noattr` says the
   cell carries no attributes, not that the row may not add one. `base_cell`
   drops it before the markup layers over it, and
   `a_switch_row_keeps_its_dim_runs_over_the_selection_style` pins that.
2. **A pane's trailing cells are a clear, not spaces.** `capture-pane -e` walks
   the grid with one persisting cell, so a row whose tail zz wrote as explicit
   spaces and the pin left to `screen_write_clearendofline` produced different
   captures even though the cells decoded the same. `Grid::emit_into` now takes
   a `Trailing`: the mode tree keeps `EL` over the whole client, and a pane's
   rect finds the trailing run of blank cells with no foreground of their own,
   writes that run's own paint, and uses `EL` only when the rect reaches the
   terminal's right edge. The chooser's path is unchanged and
   compat/tui-choosers.sh still ends `all 78 asserted comparisons identical`.

What did NOT land inside the mode is registered as the new item
`semantic:switch-mode-vocabulary` under `clients.interactive-refresh`: there is
no movement, so the current row is always the first; typing changes nothing where
the pin filters incrementally through `fuzzy_match` and repaints matches with
`switch-mode-match-style`; and the mouse rows and wheel are unbuilt. Escape,
`C-[`, `C-c` and `C-g` end the mode, Enter runs `switch-client -Z` for the
current row, and every other key is swallowed into the prompt the way
`window_switch_key` swallows it — which is why zz does not lose the mode on a
keystroke the pin keeps it through.

`switch-mode -w`'s window rows are the one cell-run residue, recorded as the
fixture case `switch-mode-windows` with its measurement: every cell matches, but
the pin leaves the columns past a window row carrying the cell that drew its last
column while zz leaves them at the default cell, so `capture-pane -e` prints the
same reset one line later. The session rows `switch-mode` opens by default are
identical on all five channels and are asserted.

## Why customize-mode and suspend-client did not land

Budget, and in that order. clock-mode took the shared pane mode with it and
switch-mode took the row expansion and two capture-spelling bugs with it.

customize-mode is the next cheapest: `#{pane_mode}` is `options-mode`, its
opening screen is eight collapsed rows over an empty `Server Options` preview
box, and that is `mode_tree_draw`, which zz already reproduces cell for cell in
`crates/zz-tui/src/render/chooser.rs`. What it needs is that renderer made
rect-aware and a `ChooserPresentation` carried per pane rather than per client —
the `Trailing::Pane` work above is half of it. attempt-03's evidence_note holds
its measured opening screen and this lane did not re-measure it.

suspend-client needs no mode at all; attempt-03's measurement of it stands
unchanged and is not re-derived here.

## The fixture

compat/tui-client-commands.sh flips `clock-mode-open` and `switch-mode` from
recorded to asserted and adds `clock-mode-twelve` beside them, which carries an
explicit `clock-mode-colour` of `#ff00aa` and `clock-mode-style 12` so the option
half is asserted rather than claimed. `align_clock_face` waits for the wall
clock's second to turn over before the comparison, because both servers redraw on
that boundary. That is enough for a face that changes once a minute; the two
`-with-seconds` faces change under the capture pair itself once the box is
loaded, so they are measured by this lane's own probe and not asserted — the
seconds case was green solo and went red at a load average near nine.

`restore_case` and `end_mode` now end a mode on either side, because zz opens one
too. `case_owner` gained `switch-mode-windows` under
`gap:clients.interactive-refresh` and `server-access-add` under
`gap:protocol.socket-acl`, so the fixture's own tally reads `unattributed=0` and
`TUI-014=2` — `customize-mode-open` and `suspend-client`, the two commands this
lane did not build.

The `--self-check` gains two sabotages: a clock open on the zz pane alone and a
switch mode open on the zz pane alone, each expected to report screen AND state
and nothing else, each withdrawn with the one key that ends it.

## Files

- `environment.txt` — the box, the revision, both binaries' sha256, RUN_ENV.
- `client-commands-run-1/2/3.txt` — compat/tui-client-commands.sh three times at
  the final tip, each `all 95 asserted comparisons identical, 27 recorded not
  asserted (0 for a sibling lane, owners TUI-014=2 TUI-015=4 TUI-017=6
  TUI-018=4 decided:TUI-016=1 gap:clients.interactive-refresh=9
  gap:protocol.socket-acl=1 unattributed=0)`, exit 0.
- `client-commands-self-check.txt` — `--self-check`, exit 0, nine ok lines.
- `choosers-run-1/2/3/4/5.txt` — compat/tui-choosers.sh five times. Runs 1, 2, 4
  and 5 end `all 78 asserted comparisons identical, 0 recorded not asserted` at
  exit 0. Run 3 is red in `zoom-tree-open` with zz's whole screen blank at a load
  average of 8.7, which is the load flake the box is known for; run 4 is the
  exact-solo rerun at 6.4 and green, and run 5 at 3.1 is green too. Kept rather
  than dropped. verify-claims.py's own first pass hit the same flake once at a
  higher load and its rerun at 3.8 is the committed one.
- `choosers-self-check.txt` — `--self-check`, exit 0.
- `screen-diff.txt` — compat/tui-screen-diff.sh, `all 147 asserted checkpoints
  identical, 6 recorded not asserted`, exit 0.
- `copy-mode.txt`, `copy-mode-self-check.txt` — compat/tui-copy-mode.sh, 147
  cases asserting at least one channel and 0 recorded in full, and its
  self-check, both exit 0. Copy mode is the mode this lane generalised and
  TUI-005 is verified, so this is the regression guard.
- `attached-client.txt` — `attached-client compatibility: PASS`, exit 0. It is
  green on this box now that fbeb4aa4 is on main.
- `corpus-command-rows.txt` — compat/run.sh --strict-geometry over
  command-flag-errors, cli-chain-parse-abort, config-discovery,
  config-discovery-explicit and config-discovery-launcher, exit 0.
- `corpus-store-rows.txt` — the same over lane2-store (193 steps),
  control-notify, copy-mode-kill-on-exit, args-parse-choosers and
  args-parse-set-option, exit 0. Those ten rows are every row that names one of
  the four commands or the string `unsupported command`.
- `cargo-protocol-mux.txt`, `cargo-daemon.txt`, `cargo-zz.txt`,
  `cargo-tui-client.txt` — the test runs, exit 0 each. zz-daemon ran with an
  empty HOME and **without** `--skip russh_socks`, which main's test resolver
  fixed.
- `clippy-libs.txt`, `clippy-zz.txt` — `-D warnings` over zz-protocol, zz-mux,
  zz-daemon, zz-tui, zz-client, zz-client-ffi and zz, exit 0 each. zz-daemon's
  seven russh_prompt errors are repaired on the main this branch sits on.
- `fmt.txt` — `cargo fmt --all -- --check`, exit 0.
- `check.txt` — RUN_ENV compat/check.sh from this worktree.
- `mode-probe.sh` — the two-sided driver this lane iterated with: an outer pinned
  tmux driving one attached pin client and one attached zz client at 80x24,
  printing both screens, cursors and pane-mode states with a diff. Throwaway
  servers on short /tmp sockets, isolated HOME and XDG_CONFIG_HOME per side,
  `-f /dev/null`, and a trap that reaps all three. Kept so it can be re-run
  verbatim.
- `probe-clock-mode.txt`, `probe-switch-mode.txt` — that driver's output for the
  two commands at the final tip. Both BEFORE and AFTER sections end `STYLED
  SCREENS IDENTICAL` and `STATE IDENTICAL` with equal cursors, which is the mode
  up. The clock's RESTORED section is identical too; the switch probe's is not,
  because the probe's own teardown sends one key with a fixed pause and the pin's
  switch mode was still up when it captured. The fixture ends it properly, waits
  for `#{pane_in_mode}` to fall and asserts the restored screen, which is the
  green `switch-mode-closed` case in every client-commands run.
- `verify-claims.txt` — the gate's own re-measurement of TUI-014 against this
  binary: both fixtures green and `(active, 2 recorded for TUI-014, which is
  consistent)`, ending `every verified obligation holds up` at exit 0.

## Count churn a reader will meet

Two commands left `UNIMPLEMENTED_TMUX_COMMAND_SPECS`, and the same eleven totals
followed them: `catalog.rs` (implemented 87, flag shapes 528 / none 297 /
required 223, supported 501, usage overrides 20, unimplemented specs 5),
`compat_manifest_tests.rs` (rows 87, implemented 76, unimplemented 4, plus the
option partition above), `hunt_claims.rs` (`COMMAND_SPECS` 82), `daemon.rs`
(specs 87, positional specs 76, spellings 161, diagnostic cases 644, required
cases 416, prefix cases 542) and the corpus fixture
`compat/scenarios/smoke/fixtures/command-flag-errors.{tsv,sh}` (canonical 87,
required 82, failure probes 532, probe count 535, sentinel `clean:535`).
`switch-mode` also joins `COMMAND_ARGS_PARSE_SPECS` and
`COMMAND_ARGS_PARSE_BEHAVES` with the pin's own `commands-or-string` rule.

Three daemon and mux tests used one of the two commands as their example of an
unsupported one and now use `customize-mode` or `link-window`, which are still
unsupported, so what they test is unchanged: the startup-cause test, the
tmux-import test, and `static_command_chain_validates_unimplemented_tmux_syntax`.
`compat/scenarios/smoke/fixtures/config-discovery-import.sh` did the same and now
imports `customize-mode`.

## Zone excursions

One, named here with what forced it. `crates/zz-protocol/src/snapshot.rs` is not
in this lane's zone, and `PaneSnapshot` is where the mode had to go: the fixture's
state channel reads a **clientless** `list-panes -a`, and the screen has to be the
same for a client that attaches after the mode opened. An `EventPayload` variant
would have needed a fan-out per window plus an attach path and would still have
raced a reattach. The trailing field costs `mode: None` in fifteen struct
literals, all of them test helpers, across zz-client, zz-client-ffi, zz-tui, zz,
zz-mux and zz-protocol's own hunt_claims.

## render.rs hunks, by function

- `Renderer` and `Renderer::with_sink`: one field, `pane_modes_painted`.
- `paint_workspace`, the `PaneKindSnapshot::Terminal` arm: the pane's mode is
  drawn over its rect instead of the terminal, and the paint that follows the
  mode ending forces that pane whole.
- `paint_workspace`'s tail: `pane_modes_painted` joins the retain list.
- `place_pane_cursor`: a pane in a mode takes the mode's own cursor and its
  visibility.
- `mod pane_mode;` beside `mod chooser;`.

Nothing else in render.rs moved, and no code was reflowed or reordered.
