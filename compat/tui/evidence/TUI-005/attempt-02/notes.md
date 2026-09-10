# TUI-005 attempt-02

Pane copy mode and search, cycle 5, copy lane. This attempt closes the divergences
attempt-01 measured and left. It does not measure them again.

## What changed

- `crates/zz-terminal/src/session.rs`: `page_copy_cursor` is `window_copy_pageup1` and
  `window_copy_pagedown1`. The view moves by `screen_size_y/2` or `screen_size_y-2`
  and the cursor keeps its screen row. `scroll_copy_view_to_cursor` is
  `window_copy_scroll_to`, used where a copy-mode search lands. `CopyModeFacts` now
  carries `rectangle_toggle`, `selection_active` and `search_string`.
- `crates/zz-terminal/src/session/mode_revision.rs`: `format_selection` keeps the final
  newline in vi when the right edge passes the end of the last selected line
  (window-copy.c:5737).
- `crates/zz-protocol/src/key.rs`: the prefix wins over a copy-table binding of the same
  key (server-client.c:1417). The engine goes back to the mode table once the prefix
  is spent, and `cancel_prefix` does the same.
- `crates/zz-daemon/src/daemon.rs`: `cancel_prefix` calls the engine's `cancel_prefix`
  instead of dropping the table.
- `crates/zz-daemon/src/status.rs`, `crates/zz-mux/src/command.rs`,
  `crates/zz-mux/src/formats.rs`: `rectangle_toggle` and `selection_active` join the
  delegated copy-mode formats. `pane_search_string` moves from Pane/Empty to
  StatusHook.
- `crates/zz-tui/src/app.rs`: a closing prompt repaints the whole screen, so the emacs
  incremental search no longer leaves its prompt row behind.
- `compat/tui-copy-mode.sh`:
  - The VIEW, PAGE, RECTANGLE and PREFIX reasons are gone, and those cases assert.
  - `assert_mode_formats` compares the three formats at twelve checkpoints.
  - The SELECTION, PROMPT and POSITION reasons, plus the new MATCH reason, start with
    `SIBLING:modes `.
  - Six new self-check sabotages.

## Files

### Environment

- `environment.txt`: both binary sha256s, the revision (66b23b5f, clean tree), the pin,
  the OS line, TERM, shell, bash, outer size and locale.

### The fixture at the tip (66b23b5f)

- `copy-mode-tip-1.txt`, `copy-mode-tip-2.txt`, `copy-mode-tip-3.txt`: three corpus
  runs, exit 0 each. All 97 cases agree on every channel they assert, and the 27
  recorded cases are all `SIBLING:modes`. Each run's stderr is in the `.stderr.txt`
  file beside it.
- `copy-mode-self-check-tip.txt` (and `.stderr.txt`): `--self-check`, exit 0, 16 of 16
  expectations met. The six added this attempt:
  - view alone: a half page on one side, twelve counted cursor-ups on the other.
  - facts: a page on one side, twenty-four counted cursor-ups on the other.
  - buffer: the vi rectangle final newline, with the pin's mode-keys switched to
    emacs before the copy (35 bytes against 36).
  - facts: C-b as page-up on one side, as the prefix on the other.
  - formats: a search, the rectangle toggle and a selection on one side only.
  - text: a search prompt open on one side only.
- `cells/`: the decoded screens and the facts, view and buffer files of eight
  checkpoints from run 1 (`ZZ_COPY_CAPTURE_DIR`):
  - both page-ups (line-38 at row 21 / scroll 22 on both)
  - the vi half page
  - the two prefix checkpoints
  - the vi rectangle copy (36 bytes on both; `od -c` offsets are octal, so `0000044`
    is 36)
  - the emacs search submit and the vi backward search (the current-match style is
    the only difference)

### Before the prompt repaint fix

- `copy-mode-before-the-prompt-repaint-fix.txt`: a corpus run of the PREVIOUS fixture
  revision (the attempt-01 file) against a binary that already had the engine fixes
  but not the `app.rs` repaint. Its `emacs-ordinary-pane-search-submit` block shows
  zz's last row still reading `(search down) needle` against the pin's
  `line-23 filler-23`. That revision did not assert the text channel there, so it
  counted the case as recorded.

### Regressions at the tip

- `screen-diff-tip.txt` (and `.stderr.txt`): `compat/tui-screen-diff.sh`, exit 0, all
  111 asserted checkpoints identical.
- `stock-keys-tip.txt` (and `.stderr.txt`): `compat/tui-stock-keys.sh`, exit 0, all 50
  cases agree.
- `zz-integration-tests-tip.txt`: `cargo test -p zz --jobs 3 -- --test-threads=2`,
  exit 0, `cli_binary` 125 passed.

### Code

These runs used the working tree before the commit. Its content for each crate is
66b23b5f's.

- `unit-tests-zz-terminal.txt`: `cargo test -p zz-terminal`, 253 passed.
- `unit-tests-zz-protocol.txt`: `cargo test -p zz-protocol`, 222 lib tests plus the
  integration files.
- `unit-tests-zz-mux.txt`: `cargo test -p zz-mux`, 524 lib tests plus the integration
  files.
- `unit-tests-zz-tui.txt`: `cargo test -p zz-tui`, 176 passed.
- `unit-tests-zz-daemon-status.txt` and `unit-tests-zz-daemon-tests.txt`: `cargo test -p
  zz-daemon` split in two to stay under the call cap. `--lib status::` passed 44,
  including the new format test and the corrected delegated inventory count. `--tests
  --skip status::` passed the other 818 lib tests and every integration binary.
- `clippy-zz-terminal.txt`, `clippy-zz-protocol.txt`, `clippy-zz-mux.txt`,
  `clippy-zz-tui.txt`, `clippy-zz-daemon.txt`: `cargo clippy -p <crate> --all-targets
  --all-features --jobs 3 -- -D warnings`, exit 0 each.
- `proofs-at-tip.txt`: every command above with its exit code and the revision it ran
  at.

## Seen and not counted

An early run under five-lane load reported `emacs-rectangle-past-end-of-line` with zz's
copy cursor at column 15 against the pin's 20. The fourth counted cursor-right had not
landed when the screen settled. The next four corpus runs, three of them at the tip,
were green on that case. That run's output file was overwritten by another lane
sharing the scratchpad and is not kept.
