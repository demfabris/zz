# TUI-014 fix pass for the verifying review, 2026-09-17 (lane modes-opus)

Branch `campaign/tui-customize-4`, rebased onto `origin/main` `066b862a` (release zz 0.11.0).
The runtime under test is `target/debug/zz_cli` at `926b01b0`, copied and hashed before any run;
`environment.txt` carries both binaries' hashes and the fixture environment.

## What the review rejected, and what it measures now

**A long customize prompt no longer disconnects any client.** `ModePrompt::draw`
(`crates/zz-mux/src/command/mode_prompt.rs`) ports `prompt_draw`: the row carries the expanded
prompt string and only the slice of the value that fits, scrolled so the cursor stays visible, and
the daemon sends that row plus its cursor column in `PaneMode::Customize`. `ChooseTreeState.prompt`
stays empty, so nothing user-controlled crosses `MAX_CHOOSE_ITEM_TEXT_BYTES` any more; the other
fields customize and switch fill (item labels, row text, preview lines, titles, the switch rows and
prompt) are unbounded on the wire, and the row shortcut keys are the only capped field left, at
three bytes. `DaemonClient::recv` now logs an undecodable control frame and reads the next one
instead of ending the connection, with a unit test that feeds an over-cap `ChooseTreeState` frame
followed by a good one. 13 `customize-long-*` cases assert the whole screen and cursor for
`/status-format Enter` then a lone Enter, the Left and Home scroll, the child prompt, and
`list-clients` with a second raw client attached to another session throughout.

**switch-mode has its own keys and filter.** `SwitchMode`
(`crates/zz-mux/src/command/switch_mode.rs`) keeps `current`, `offset`, the prompt buffer and the
filter on the pane's mode stack. `window_switch_key`'s map is reproduced: `C-p`/`C-k` up,
`C-n`/`C-j` down, Up, Down, PPage and NPage through `prompt_check_move`, Home, End and the emacs
editing keys into the `(search)` prompt, every buffer change rebuilding the match list with the
selection back at the top, Enter running the template on the current match and doing nothing when
nothing matches. `fuzzy_match_columns` (`crates/zz-mux/src/formats.rs`) ports `fuzzy_match`,
including the style-skipping scan and the alignment trim, and returns the columns the client
repaints with `switch-mode-match-style`, which joined `TMUX_OPTION_CONSUMERS`. 42 `switch-keys-*`
cases assert movement, wrapping, page keys, prompt cursor keys, the `z` and `zu` filters, BSpace,
a subsequence match, smart case, no-match Enter, `C-w`, `C-t`, the filtered and moved Enter
targets, the `-w` window list, a custom match style and the default `switch-client` target.
`semantic:switch-mode-vocabulary` leaves `clients.interactive-refresh`; the mouse half is
registered as `semantic:pane-mode-mouse`, which is unbuilt for both modes.

**The customize tree is a port, not an imitation.** `crates/zz-mux/src/command/customize.rs` now
follows `mode-tree.c` and `window-customize.c`: `current`, `offset` and `height` with the pin's
build, `mode_tree_set_current` and `check_selected` arithmetic; array children named `name[key]`
with the parent's format expansion, so a pane-scope entry keeps its `(pane 0)` marker; the
`This is an array option, key N.` description; `u`, `a`, `d`, `D`, `U` with the pin's single-key
confirmations; Right expanding then descending, Left collapsing or rising, `M--` and `M-+` keeping
the current line, the three-state `v` cycle, keyboard `t` not moving, `T`/`C-t`, search with `n`
and `N` over the whole tree, filter and `c`, `H`, help on `C-h`, and the `s`/`w`/`S`/`W`/Enter
scope choices with flag and choice cycling. Previews are laid out through a port of
`screen_write_text` and arrive as markup lines, with the drawn-as-parent title in the selected
item's `detail`. `parse_styled_segments` now draws the text after `#[ignore]` literally the way
`format_draw` does, so `pane-border-format` and `status-format` values show their markup instead of
rendering it. 95 `customize-screen-*` cases and 20 `customize-array*` cases assert the whole decoded screen for all of it.

**Nits.** The raw TUI no longer clears switch rows or moves the cursor to imitate the outer tmux's
cell allocation; switch screens are asserted on decoded cells and the cursor instead, and the
closed `clients.switch-mode-style-tail` record says so. `client-tree-open`'s reason now states the
measured behaviour: with the fixture's client attached the pin exits 0 and opens client-mode on the
target pane, while zz exits 1 with its attached-client error and opens nothing.
`capture_seconds_pair` decodes the seconds digit out of both clock faces and only accepts a pair
once both show the second the wait started in.

## Measurements

Every fixture ran from the worktree with the frozen binary and the minimal environment
`environment.txt` names. Full output and exit statuses are in `laneA/`, `laneB/`, `laneC/` and
`copy-retry/`; `results.json` collects them.

- `compat/tui-client-commands.sh` five times (client-1 to client-5, three in one lane and two in
  another): each exits 0 with all 495 asserted comparisons identical and 30 recorded, owners
  `TUI-015=3 TUI-017=4 decided:TUI-014=3 decided:TUI-015=4 decided:TUI-016=1 decided:TUI-017=6
  gap:clients.interactive-refresh=8 gap:protocol.socket-acl=1 unattributed=0`. TUI-014 owns no
  ordinary record.
- `compat/tui-client-commands.sh --self-check`: exit 0, 158 expectations met, every sabotage caught
  in its own channel and both equivalences passed.
- `compat/tui-choosers.sh` (78 asserted, 0 recorded), `compat/tui-copy-mode.sh` (147),
  `compat/tui-screen-diff.sh` (147 asserted, 6 recorded), `compat/tui-overlays.sh` (48) and each
  `--self-check`: exit 0. `compat/attached-client.sh`: PASS.
- One `tui-copy-mode` run failed `emacs-rectangle-off` and `vi-rectangle-off` while three fixture
  lanes and a cargo test build shared the box; the retained retry alone (`copy-retry/`) passes all
  147. The two zz-daemon tests that failed in the same window pass solo
  (`tests-daemon.txt`, `tests-daemon-solo.txt`).
- `python3 compat/tui/verify-claims.py --run TUI-014`: exit 0, re-measuring 495 asserted / 30
  recorded with none of them TUI-014's, and 78 / 0 for the choosers.
- Delta corpus, `origin/main...HEAD` with the commands this lane touched: 204 rows, 193 clean, the
  two documented `known/` rows, the six inherited baseline divergences that the attempt-17 run of
  the same selection also carried, and three rows that need the binary to be named `zz_cli`
  (`smoke/config-discovery-launcher`, `smoke/launcher-installed-layout`, `smoke/pane-tmux-path`),
  all three clean when re-run with the identical binary under that name. No unrun rows.
  See `corpus-results.json`, `delta-corpus.txt` and `delta-corpus-launcher.txt`.
- Tests: zz-protocol, zz-mux and zz-tui with `--all-features`, 1119 passed, 0 failed; zz-daemon lib
  986 tests. Clippy for zz-protocol, zz-mux, zz-daemon, zz-tui and zz-cli with `--all-targets
  --all-features -D warnings`: exit 0. `cargo fmt --all -- --check`: exit 0. `compat/check.sh` with
  every nested cargo routed through `/tmp/zz-cargo.sh`: exit 0, including `wire-version.py`
  (105 unreleased), `evidence-secrets.py` and both trackers.

## Limits, retained

- Mouse input inside switch-mode and customize-mode is unbuilt (`semantic:pane-mode-mouse`).
- A customize edit prompt has no history (`Up`/`Down`), no `Tab` completion, no `C-y` paste from
  the top paste buffer, and no vi `status-keys` table; the search and filter prompts are unaffected.
- Rows are rebuilt from live state on every snapshot, so an option changed from outside the mode
  shows at once where the pin shows it after its next build.
- The pin crashes when its customize preview draws a `pane-colours` array child
  (`options_get_number` on an array), so the pane-scope array cases open the mode with `-N`.
- `new-session -x/-y` sets `default-size` on the pin's new session and not on zz's, and zz's
  `history-limit` default is 10000 against the pin's 2000; both are outside this obligation, so the
  customize scene sets them on both sides before comparing trees.
- Default key tables differ by accepted `keys.*` gaps, so the key-binding cases use a table the
  fixture binds itself rather than `root` or `prefix`.
