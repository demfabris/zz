# TUI-014 attempt-01, cycle 8

choose-client, the pin's client mode, drawn by the raw TUI.

## What is in this directory

- `environment.txt` — the box, the revision the runs were taken at, both
  binaries with the zz build's sha256, and the pin.
- `choosers-run-1.txt`, `choosers-run-2.txt`, `choosers-run-3.txt` — three
  consecutive runs of `compat/tui-choosers.sh` at the tip. Runs 1 and 3 end
  `all 73 asserted comparisons identical, 3 recorded not asserted (0 for a
  sibling lane)`, exit 0. Run 2 ends with one differing comparison,
  `filter-cleared`, and that row is not this landing's: it is a window-tree
  pane row where `#{pane_current_command}` answers `bash` on zz and `sh` on the
  pin. zz resolves that name from the pty's foreground process group through
  sysinfo in `terminal_current_command` (crates/zz-daemon/src/daemon.rs), a
  path this branch does not touch; the same row is green in runs 1 and 3 and in
  four earlier runs today, and the box was carrying two other lanes' cargo jobs
  while run 2 was taken. The seventeen client-mode comparisons are green in all
  three runs.
- `choosers-self-check.txt` — `compat/tui-choosers.sh --self-check`, exit 0.
  Nine sabotages, each caught in its own channel, and two equivalences that
  report nothing. The new one is `client info`: `i` typed into the pin's client
  tree alone swaps `window_client_draw` for `window_client_draw_info`, a
  surface no other sabotage in the file reaches, and it is caught in the rows
  with the client mask on, so the difference it reports is not the two clients'
  names.
- `client-commands-run-1.txt`, `-2`, `-3` — three runs of
  `compat/tui-client-commands.sh`, each `all 61 asserted comparisons identical,
  37 recorded not asserted (0 for a sibling lane)`, exit 0. The three new
  asserted cases are `client-tree-unknown-flag`, `client-tree-bad-sort` and
  `client-tree-usage`; `client-tree-open` stays recorded and its reason moved
  from commands.native-client-tools to clients.interactive-refresh.
- `client-commands-self-check.txt` — exit 0, every sabotage caught.
- `attached-client.txt` — `compat/attached-client.sh`, PASS.
- `tui-screen-diff.txt`, `tui-screen-diff-self-check.txt` — exit 0 each,
  `all 147 asserted checkpoints identical, 6 recorded not asserted`.
- `corpus.txt` — 23 corpus rows under `--strict-geometry`, every one clean:
  the `list-keys` and prefix-table rows the new stock `D` binding reaches
  (list-keys-padding, prefix2, strict-key-validation, command-alias,
  census-hooks, formats-values, honest-knobs-c1-readback,
  smoke/keys-prefix-stock, the three smoke/config-discovery rows and the four
  smoke/plugin-runtime rows that read the prefix table), the four chooser rows,
  the two positional-inventory rows, `capture-pane`, and
  `smoke/command-flag-errors`, whose tsv fixture and probe counts move with the
  new command.
- `cargo-zz.txt` — `cargo test -p zz`, exit 0, including the 125 `cli_binary`
  tests.

## What landed

`choose-client` leaves `UNIMPLEMENTED_TMUX_COMMANDS` and becomes
`window_client_mode` over the mode tree that already draws `choose-tree`.

- `ChooseTreeKind::Clients` and `ChooseTreeTarget::Client(ClientId)` are tail
  appends; so are `ChooserPreview::Client` and `ChooserPreview::Markup`, and
  `ChooseTreeAction::ClientDetach`, `ClientDetachTagged` and `ClientInfo`,
  which the daemon resolves `d`, `D` and `i` to inside the client mode because
  the pin keeps those keys in the mode and not in a key table.
- The daemon builds one `ClientChooserRow` per attached client, with the pin's
  `WINDOW_CLIENT_DEFAULT_FORMAT` (or `-F`) and the `-f` filter expanded in that
  client's own format tree, the only place `#{t/p:client_activity}` and the
  rest of the client formats resolve. `window_client_order_seq` is
  `WINDOW_CLIENT_ORDER_SEQ` in zz-mux and `O` steps it.
- The preview is `window_client_draw`: the chosen client's current pane in
  `sy - 2 - lines` rows, a rule, and a copy of that client's own status rows
  underneath, composed at the client's own width through
  `zz_client::compose_status_row` and clipped to the box. `ServerState` keeps
  the rows last published to each client for it. `i` swaps in the pin's info
  lines, expanded the same way.
- `bind-key -T prefix D choose-client -Z` is the pin's stock binding and is now
  zz's too.

## Zone excursions

- `crates/zz-tui/src/render.rs`: one match arm in `paint_chooser` for
  `ChooseTreeKind::Clients`, forced by the appended variant. That function is
  the fallback chrome the raw TUI paints when the daemon has published no
  presentation yet; the mode tree itself is drawn in
  `crates/zz-tui/src/render/chooser.rs`, which the lane holds.
- `crates/zz-protocol/src/key.rs`: one line, the stock `prefix D` binding,
  forced by the case the punch list names - prefix D is how the fixture opens
  the client tree.
- `compat/tmux-gaps.json` `keys.default-prefix`: `key:prefix:D` removed,
  forced by `compat_manifest_tests::option_format_hook_and_default_key_items_match_pinned_inventories`,
  which fails with `implemented default key has a stale item: key:prefix:D` the
  moment the binding exists. The gap keeps every other item and its decision.
- `crates/zz-mux/src/compat_manifest_tests.rs`,
  `crates/zz-protocol/src/catalog.rs` and `crates/zz-daemon/src/daemon.rs`
  inventory counts, and `compat/scenarios/smoke/fixtures/command-flag-errors.tsv`
  and `.sh`: the partition tests count implemented commands, and one more
  command moves eleven of those numbers.

## What did not land

Clause 2 in full: `clock-mode`, `customize-mode`, `switch-mode`,
`suspend-client` and `server-access` are still hard-rejected and still
recorded in `compat/tui-client-commands.sh` under
commands.native-client-tools, which keeps those four items. Clause 3 keeps
`client-tree-open` recorded, now under clients.interactive-refresh: zz's
chooser is per client, so a clientless CLI answers
`choose-client requires an interactive client` at exit 1, which that accepted
gap already covers in writing for `choose-tree` and `choose-buffer`. The `d`,
`D`, `x` and `X` detach keys and the default `detach-client -t '%%'` template
are implemented but not asserted whole-screen: a scene with one client per side
cannot survive its own detach, so `client-run-*` proves the Enter path and the
`%%` substitution through a template that does not detach.

## GATE ADDENDUM, 2026-09-14, cycle 6 commands

### The two files the lane's listing above missed (reviewer nit 5)

- `cargo-crates.txt` — the lane's `cargo test -p zz-protocol -p zz-mux -p zz-tui`,
  exit 0.
- `check-sh.txt` — the lane's `compat/check.sh`, exit 0.

### Five more zone excursions the lane's section above did not name (reviewer nit 7)

Each is forced and small; two of the five are already in TUI-014's `sources`.

- `crates/zz-daemon/src/keys.rs`: the new `client_mode_key_action`. Forced
  because the pin keeps `d`, `D` and `i` inside the client mode rather than in a
  key table (window-client.c `window_client_key`), so the daemon has to resolve
  them where it resolves the mode tree's own keys.
- `crates/zz-mux/src/sort.rs`: `WINDOW_CLIENT_ORDER_SEQ`. Forced by the four
  sort orders `O` steps through; the pin's own `window_client_order_seq` lives
  beside its other order sequences and zz's mirror of it lives beside theirs.
- `crates/zz-mux/src/lib.rs`: the re-export of that constant, one line, forced
  by the daemon needing it.
- `crates/zz-mux/tests/hunt_claims.rs`: `COMMAND_SPECS` 78 -> 79 and an
  `info_preview` field on the chooser claim. Forced by the byte pins, which fail
  the moment a command spec or a chooser field appears.
- `crates/zz/src/chooser/tree.rs`: two match arms for the appended
  `ChooseTreeKind` variant in the GUI. Forced by exhaustiveness - the crate does
  not compile without them.

### The gate's own runs

All at the gate tip in /home/demfabris/dev/zz-gate-tui7 against the pin
d77c9dc6 and the zz build whose sha256 is in `environment.txt`.

- `review.md` — the reviewer's verdict verbatim, what the gate did with each
  blocker, must-fix and nit, and what the gate measured that the review did not.
- `gate-choosers-run-1.txt`, `-2`, `-3` — three runs at the gate tip, each exit 0
  and `all 73 asserted comparisons identical, 3 recorded not asserted (0 for a
  sibling lane)`. The `filter-cleared` flake the lane saw in its run 2 did not
  appear in any of the three.
- `gate-choosers-self-check.txt` — exit 0.
- `gate-client-commands.txt`, `gate-client-commands-self-check.txt` — both after
  the gate's correction to `CLIENT_TREE_CLIENTLESS`; 61 asserted identical, 37
  recorded, and every sabotage caught in its own channel.
- The stage-3 list, every one exit 0: `gate-tui-screen-diff.txt`,
  `gate-tui-screen-diff-self-check.txt`, `gate-tui-pane-geometry.txt`,
  `gate-status-row.txt` (LC_ALL=C LC_TIME=C), `gate-tui-stock-keys.txt`,
  `gate-tui-stock-keys-self-check.txt`, `gate-tui-indicators.txt`,
  `gate-tui-indicators-self-check.txt`, `gate-tui-copy-mode.txt`,
  `gate-tui-copy-mode-self-check.txt`, `gate-tui-caps.txt`,
  `gate-tui-caps-self-check.txt`, `gate-tui-overlays.txt`,
  `gate-tui-overlays-self-check.txt`, `gate-attached-client.txt`.
- `gate-tui-mouse.txt` — not on the stage-3 list, run because this lane edits
  `crates/zz-protocol/src/key.rs` and that fixture is the keys lane's. Exit 0,
  `all 26 asserted checks identical`, 10 recorded: unchanged from the keys gate,
  so TUI-008 is still active and TUI-012 stays at review.
- `gate-cargo.txt` — the three package test runs, clippy and fmt, with the
  result lines and the three count assertions the rebase had to resolve by hand.
- `gate-corpus.txt` — the 164-row selection, the coverage proof, the two
  environmental divergences with what each actually is, and every row's summary
  line.
- `gate-probe-info.sh`, `gate-probe-info-1.txt`, `-2`, `-3` — the reviewer's own
  info-view probe with ZZ_BIN pointed at the gate build, run three times. This
  is the measurement behind the clause-1 correction, and behind the one
  divergence of theirs that did not reproduce.
- `gate-probe-clientless.sh`, `gate-probe-clientless.txt` — the clientless
  `choose-client` / `choose-tree` / `choose-buffer` comparison behind the
  `clients.interactive-refresh` widening, including the `#{pane_in_mode}`
  readings that show the pin opens no mode for choose-client.
