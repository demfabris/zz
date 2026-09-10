# TUI-005 attempt-02

Pane copy mode and search, cycle 5, copy lane. This attempt closes the divergences
attempt-01 measured. The adversarial review of b050b375 rejected it on one blocker:
the search did not outlive the copy mode. The fix pass is 221484cd. b8eecde3 then
makes the fixture settle on the cursor tuple too (see the cursor settle race below).
The four copy-mode runs ran at b8eecde3; every other run in this directory ran at
221484cd, whose crates, binary and other fixtures b8eecde3 does not change.

## What changed

First landing (66b23b5f, fixture settle fix be67a0d5):

- `crates/zz-terminal/src/session.rs`: `page_copy_cursor` is `window_copy_pageup1`
  and `window_copy_pagedown1`. The view moves by `screen_size_y/2` or
  `screen_size_y-2` and the cursor keeps its screen row. `scroll_copy_view_to_cursor`
  is `window_copy_scroll_to`, used where a copy-mode search lands. `CopyModeFacts`
  carries `rectangle_toggle` and `selection_active`.
- `crates/zz-terminal/src/session/mode_revision.rs`: `format_selection` keeps the
  final newline in vi when the right edge passes the end of the last selected line
  (window-copy.c:5737).
- `crates/zz-protocol/src/key.rs`: the prefix wins over a copy-table binding of the
  same key (server-client.c:1417). The engine goes back to the mode table once the
  prefix is spent, and `cancel_prefix` does the same.
- `crates/zz-daemon/src/daemon.rs`: `cancel_prefix` calls the engine's
  `cancel_prefix` instead of dropping the table.
- `crates/zz-daemon/src/status.rs`, `crates/zz-mux/src/command.rs`,
  `crates/zz-mux/src/formats.rs`: `rectangle_toggle` and `selection_active` join the
  delegated copy-mode formats. `pane_search_string` moves from Pane/Empty to
  StatusHook.
- `crates/zz-tui/src/app.rs`: a closing prompt repaints the whole screen, so the
  emacs incremental search no longer leaves its prompt row behind.

Fix pass (221484cd), the review's blocker and must-fix 1:

- `crates/zz-terminal/src/session.rs`: the pane's terminal actor keeps the last
  copy-mode search, `wp->searchstr` (window-copy.c:4508). It is written where
  `run_copy_mode_search` sets the mode's search, with the regex flag demoted the way
  the pin demotes it, and published as `TerminalSession::pane_search_string`. A
  fresh entry seeds its search from it with the direction up (window-copy.c:566),
  and a `search_all` latch is `data->searchall`, so an entry's first search
  recounts its matches. `CopyModeFacts.search_string` is gone.
- `crates/zz-daemon/src/status.rs`: `pane_search_string` answers from the pane's
  terminal inside and outside the mode (format.c:2418).
- Unit tests: zz-terminal
  `the_pane_keeps_its_last_search_and_a_fresh_entry_searches_up_for_it` and
  zz-daemon `pane_search_string_outlives_the_copy_session_and_seeds_the_next_entry`.
- `compat/tui-copy-mode.sh`: `record_pane_search_string` is gone. Per table, the
  formats are now asserted after q and on the re-entry, and so is the n landing on
  that re-entry. Then a forward search goes through the command path on both sides,
  and the re-entry formats, the n landing (up, to needle-48) and the formats after q
  are asserted. Five new self-check sabotages. The header no longer calls the
  outside-mode answer an accepted stance.
- `compat/tmux-gaps.json`: formats.pane-runtime's reason carries the dated
  measurement for the removed `format:pane_search_string` item.
- `knowledge/tmux/divergences.md`, `knowledge/tmux/key-tables.md`: the
  pane_search_string lines describe the pane record.

## Files

### Environment

- `environment.txt`: both binary sha256s, the revision (221484cd, clean tree), the
  pin, the OS line, TERM, shell, bash, outer size and locale.

### The fixture at b8eecde3

- `copy-mode-tip-1.txt`, `copy-mode-tip-2.txt`, `copy-mode-tip-3.txt`: three corpus
  runs, exit 0 each, run concurrently. All 115 cases agree on every channel they
  assert, and all 31 recorded cases are `SIBLING:modes`. Each run's stderr is in the
  `.stderr.txt` file beside it.
- `copy-mode-self-check-tip.txt` (and `.stderr.txt`): `--self-check`, exit 0, 21 of
  21 expectations, run beside the three corpus runs. The five added in the fix pass:
  - formats: pane_search_string after q, searched on one side only.
  - formats: pane_search_string on a fresh entry, searched on one side only.
  - cursor, then facts: n on a fresh entry on the pin side only (two expectations).
  - facts: n (up) on one side and N (down) on the other on a fresh entry after a
    forward search.
- `cells/`: the decoded screens and the facts, view and buffer files of eleven
  checkpoints from run 1 (`ZZ_COPY_CAPTURE_DIR`):
  - both page-ups and the vi half page
  - the two prefix checkpoints
  - the vi rectangle copy (36 bytes on both; `od -c` offsets are octal, so
    `0000044` is 36)
  - the emacs search submit and the vi backward search
  - the vi re-entry, its n landing on line-30 filler-30, and the n landing after
    the forward-search re-entry on line-48 needle-48 at 8,9 on both sides (the
    current-match style is the only difference in the landings)

### Regressions at 221484cd

- `screen-diff-tip.txt` (and `.stderr.txt`): `compat/tui-screen-diff.sh`, exit 0,
  all 111 asserted checkpoints identical.
- `stock-keys-tip.txt` (and `.stderr.txt`): `compat/tui-stock-keys.sh`, exit 0, all
  50 cases agree.
- `zz-integration-tests-lib-and-buffers.txt` and `zz-integration-tests-tip.txt`:
  `cargo test -p zz` split in two under the call cap. The lib passed 634,
  `buffer_client_file_load` 6 and `buffer_client_file_save` 5, then `cli_binary` 125.

### Code

- `unit-tests-zz-terminal.txt`: 254 passed, 1 ignored.
- `unit-tests-zz-protocol.txt`, `unit-tests-zz-mux.txt`, `unit-tests-zz-tui.txt`:
  every test passed.
- `unit-tests-zz-daemon-status.txt`: `--lib status::`, 44 passed.
- `unit-tests-zz-daemon-tests-loaded-flake.txt`: `--tests --skip status::`, exit 101.
  One failure, `client_focus_closes_display_panes_and_preserves_chooser_modes`, on a
  status event carrying the wall clock. The other 818 lib tests and every
  integration binary passed.
- `unit-tests-zz-daemon-flake-solo.txt`: that test run exact and alone, exit 0. It
  also passed in the full run before 221484cd was committed, on the same crate
  content: the load-flake rule.
- `clippy-zz-terminal.txt`, `clippy-zz-protocol.txt`, `clippy-zz-mux.txt`,
  `clippy-zz-tui.txt`, `clippy-zz-daemon.txt`: exit 0 each.
- `proofs-at-tip.txt`: every command above with its exit code and revision.

### Earlier revisions, kept for history

- `copy-mode-cursor-settle-race-at-63acab72.txt`: one of four concurrent fixture runs
  at 63acab72, run beside a cargo test and clippy chain. It exited 1 on
  `emacs-prefix-takes-precedence` in the cursor channel alone: zz's outer cursor at
  2,23 against the pin's 9,22, with the rows, glyphs, logical position, view and
  buffer identical. 2,23 is where a repaint's last cell write leaves the cursor
  (the copy view's bottom row is the `$ ` prompt), and arming the prefix makes the
  raw TUI repaint identical cells. crates/zz-tui render.rs places the cursor from
  the pane viewport and does not read the armed prefix. So two identical plain
  captures straddled a frame the outer tmux read in two chunks. b8eecde3 makes
  `wait_settled` require the screen and the cursor tuple to hold still between two
  polls; the three corpus runs and the self-check at b8eecde3 are green.

- `copy-mode-settle-race-before-the-fix.txt`: a corpus run at 3ced0ea6. It reported
  `emacs-rectangle-past-end-of-line` with zz's copy cursor at column 5 against the
  pin's 20, because that checkpoint waited on no observable while counted
  cursor-rights were still going through the numeric prompt. be67a0d5 makes both
  rectangle checkpoints wait for `#{copy_cursor_x}` to reach the typed column.
- `copy-mode-before-the-prompt-repaint-fix.txt`: a corpus run of the attempt-01
  fixture revision against a binary without the `app.rs` repaint. zz's last row
  still reads `(search down) needle` against the pin's `line-23 filler-23`.
- `screen-diff-loaded-run-that-lost-its-daemon.txt` (and `.stderr.txt`): a
  screen-diff run at be67a0d5 beside `tui-stock-keys.sh` and `cargo test -p zz`. It
  exited 2 when its own daemon's socket file was gone. That fixture is outside this
  lane's diff and was green when run on its own.
