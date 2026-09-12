# TUI-005 attempt-03 (cycle 6, copy lane)

Branch `campaign/tui-copy-2`, based on `origin/campaign/tui-cycle5-gated` (5d9bf198). Every proof in
this directory ran at 4f4f0b0b, the last code and fixture commit; the ledger commit on top of it
touches only `compat/tui/` and the generated report. Pin: tmux d77c9dc6 (next-3.8). Box: alienware,
CachyOS Linux (`environment.txt`).

This batch was a punch list of what cycle 5's re-review and gate left open. It resumed after the
first run of the cycle was killed by the OOM about twenty minutes in: that run had left the page
clamp fix, the send-keys dispatch and most of the fixture cases uncommitted in this worktree. They
were kept, rebuilt, measured, and split into one commit per punch-list item.

## Commits

- 235281e4 Keep the remembered column on a page move pinned at the bottom (item 1)
- fd6a719e Send keys into a copy-mode pane through the mode's table (item 2)
- bbee4948 Name the modes lane as owner of the current-match cases (item 3)
- 4f4f0b0b Compare pane_search_string at every copy-mode checkpoint (item 4)
- the ledger commit on top (item 5)

## 1. The vi page clamp

Pin: `window_copy_pageup1` and `window_copy_pagedown1` (window-copy.c:877-990) set
`data->cx = data->lastcx` and call `window_copy_cursor_end_of_line` only when
`(cx >= lastsx && cx != px) || cx > px`. The vi one-short limit is never applied on that path.
zz's `page_copy_cursor` (crates/zz-terminal/src/session.rs) always ended in `place_copy_cursor`,
which caps x at length-1 in vi. It now places the cursor at `last_cx` directly when the end-of-line
rule does not fire, and goes through `place_copy_cursor` only when it does.

Measured by the cycle-5 review's probe2 (`../attempt-02/gate-review-probe2.txt`): from column 1 of
`aaaaaaaaaaaa`, `C-u C-u C-d C-d C-d` and `5 k NPage` land the pin on 1,23 and zz on 0,23. Both
now land on 1,23.

- Unit test: zz-terminal
  `page_movement_pinned_at_the_bottom_keeps_the_remembered_column_unclamped`, both tables, from the
  kept column and from one past it.
- Fixture: `run_page_clamp` per table, seven cases each (`$table-clamp-*`), on a seed of 77
  numbered lines, then `aaaaaaaaaaaa`, `bbbbbbbbbbbbbbbbbbbb`, `cc`, an empty line and `SEEDEND`.
- Self-check: the page landing on the prompt row on both sides, then `h` on zz only, which puts it
  on column 0 where the old clamp did. Caught in cursor and facts.

## 2. send-prefix and send-keys into a pane in copy mode

Pin: server-client.c:1417 gives the prefix precedence inside a copy table. The second `C-b` runs
`send-prefix`, and `cmd_send_keys_inject_key` (cmd-send-keys.c:94-100) sends a key for a pane in a
mode through that mode's table: a bound key runs its binding with the pane as target, and an
unbound one is dropped. The same holds for `send-keys` without `-X` or `-K`.

zz's `send_prefix` and `send-keys` emit `MuxEffect::SendKeys` to the pane, and nothing moved:
after `C-b [`, `k k`, `C-b C-b` in vi, the pin sat at 6,21 scroll 22 and zz at 0,21 scroll 0.

`crates/zz-daemon/src/daemon.rs` now handles it in the SendKeys effect. `copy_mode_key_owners`
picks the caller's live copy session on the pane, or else every live copy session on it. Then
`inject_mode_table_keys` looks each key up in that session's copy table and runs the binding, or
drops the key. `paste-buffer` still writes to the pane, as `paste_send_pane` does on the pin.

The daemon test `daemon_keeps_pty_and_mux_state_across_interactive_detach` had fed the PTY under
copy mode through `send-keys`, which is the old leak. It now feeds it through `set-buffer` and
`paste-buffer -d`, which keeps what the test is about (the PTY advancing under a frozen copy view).

- Unit test: zz-daemon `send_keys_and_send_prefix_into_copy_mode_run_the_copy_table`. In vi,
  `send-keys Z k` moves up one, and `send-prefix` with the prefix set to `k` moves up another.
  After cancel, the `LIVE` line reaches the pane and no `Z` ever did.
- Fixture: per table, `$table-send-prefix-start`, `$table-send-prefix-runs-the-copy-binding`
  (`C-b C-b`: cursor-left in emacs, page-up in vi), `$table-send-keys-runs-the-copy-table`
  (`send-keys -t -N 2 Z <up>` from the command line: Z is unbound in both tables, so the cursor
  goes up twice) and `$table-send-keys-cancel` (its text channel shows nothing reached the shell).
- Self-checks: `C-b C-b` on the pin side only, caught in facts and view; `send-keys` into copy mode
  on the pin side and into the live shell on the zz side, caught in text.

Zone excursion: `knowledge/tmux/key-tables.md` said `<prefix> <prefix>` always delivers a literal
prefix to the pane. One paragraph now says a pane in copy mode gets it through the mode's table.

## 3. Sibling accounting

The 12 `MATCH_REASON` cases and `emacs-ordinary-pane-search-backspace` (whose reason is
`MATCH_REASON` plus the incremental-search note) are red on the rows channel only. The pin paints
the current match with `copy-mode-current-match-style` (bg #cd00cd), zz in reverse video. Text,
cursor, facts, view and buffer assert through all 13. The painting is in crates/zz-tui render.rs,
the modes lane's zone this cycle. The reason starts `SIBLING:modes ` with the cycle-5 gate's
measurement, and the copy gate flips the 13 after rebasing onto the main that carries modes.

## 4. pane_search_string

Cycle 5 removed `format:pane_search_string` from `formats.pane-runtime` once the pane's terminal
kept the last search. Its re-review found the fixture compared the format only at named
checkpoints. `FACTS_FORMAT` now carries `search=[#{pane_search_string}]`, so every case compares it,
at every checkpoint the fixture drives: inside the mode after each search, while the emacs
incremental prompt is typed and edited, after `n` and `N`, after `q`, on every re-entry, after
send-keys and send-prefix into the mode, and through the page clamp.

A fresh-entry group per table opens the search prompt after the pane already holds `needle`. The
emacs `C-r` binding passes `-I '#{pane_search_string}'`, but `prompt_create` keeps an incremental
prompt's expanded input as its last search and opens the buffer empty (prompt.c:162-164). Both
sides open `(search up) ` empty, as the cycle-5 gate had written in key-tables.md.

- Self-check: a search made on the zz side only, then `q` on both, is caught in the facts channel
  as well as in the formats check.
- `formats.pane-runtime`: the removal stays. The measurement is appended and the acceptance now
  names `pane_search_string` as answered. `pane_key_mode` keeps the per-viewer stance and stays the
  group's one item.
- Left for the orchestrator, outside this lane's gaps: `keys.copy-mode-binding-fidelity`'s
  resolution still says the `-I` value expands empty in zz and that the prompts open empty for
  that reason. They open empty on the pin too, for the prompt.c reason above. The
  `semantic:copy-mode-search-string-scope` text in `clients.interactive-refresh` still says zz
  answers the format as a constant empty and the string dies with the mode, which is no longer so
  since cycle 5.

## Code boundaries

No render.rs hunk: crates/zz-tui is untouched by this branch. Hunks by file and function:

- crates/zz-terminal/src/session.rs: `page_copy_cursor`, plus one unit test
- crates/zz-daemon/src/daemon.rs: `Shared::apply_effects` (the `MuxEffect::SendKeys` arm and the
  drain after the effect loop), the new `Shared::inject_mode_table_keys` and free
  `copy_mode_key_owners`, the detach test's input step, and one new unit test

The wire is unchanged. The GUI has no presentation change: none of crates/zz, zz-client or zz-tui
issues `send-keys`.

## Files

- `environment.txt`: host, OS, pin, toolchain, cargo wrapper
- `copy-mode-tip-1.txt`, `copy-mode-tip-2.txt`, `copy-mode-tip-3.txt` (with `.stderr.txt`): three
  corpus runs at 4f4f0b0b. 147 cases, all asserting at least one channel, all agreeing; 13
  recorded, all `SIBLING:modes`.
- `copy-mode-self-check-tip.txt` (with `.stderr.txt`): `--self-check`, 27 of 27 expectations met
- `screen-diff-tip.txt` (with `.stderr.txt`): `compat/tui-screen-diff.sh`, 111 asserted
  checkpoints identical
- `stock-keys-tip.txt` (with `.stderr.txt`): `compat/tui-stock-keys.sh`, 50 cases agree
- `attached-client-tip.txt` (with `.stderr.txt`): `compat/attached-client.sh`, see below
- `unit-tests-zz-terminal.txt`, `unit-tests-zz-daemon.txt`: `cargo test -p` per touched crate
- `clippy-zz-terminal.txt`, `clippy-zz-daemon.txt`: clippy `--all-targets --all-features -D warnings`
- `zz-integration-tests-tip.txt`: `cargo test -p zz` (lib, buffer_client_file_load,
  buffer_client_file_save, cli_binary)
- `proofs-at-tip.txt`: every proof command with its exit code and revision

## attached-client.sh

`attached-client-tip.txt` exits 1 after 19 seconds with `error: zz screen did not visibly become
copy-mode within 10 seconds`. That is the step the batch names as known at BASE: its copy-mode wait
still expects zz's old COPY status badge, which the modes landing replaced with the pin's in-pane
position indicator, and the modes lane fixes it. Nothing past that step ran, so nothing past it is
reported.

## History moved out of the ledger

The cycle-5 evidence_note (attempt-02's closures: half page and search landing, whole page, vi
rectangle newline, prefix precedence, selection_active and rectangle_toggle, the prompt repaint, the
search outliving the mode, the two settle races, and the cycle-5 gate's sibling flips) is kept in
`../attempt-02/notes.md` and the git history of `compat/tui/campaign.json` at 5d9bf198. The ledger
now carries only the current measurement.

## Cycle-6 copy gate, 2026-09-11

Revision bcf9d12328ed2ef15b56d9d4aa12c229273901c6, the copy lane rebased onto
origin/main fd3c64e4 plus two gate commits. Command lines, exit codes and start
times are in the `gate-*.txt` files beside this one; the reviewer's verdict and
what the gate did with it are in `review.md`.

THE REBASE. The five lane commits replayed with one conflict, the generated
`knowledge/tmux/tui-parity.md` status-count line, resolved by regenerating.
The rebased diff against origin/main is byte for byte the lane's own diff
against BASE, so nothing came along with it.

`git merge-tree --write-tree origin/main 8d766d0d` exits 1 with fifteen
conflicted paths, which is not a real prediction here: the modes gate put the
cycle-5 chain on main as new commits by rebasing it, so BASE's own hunks sit on
both sides with no ancestor between them.

THE FLIP. The thirteen cases that recorded `SIBLING:modes` now assert the rows
channel: the twelve MATCH_REASON search cases and
`emacs-ordinary-pane-search-backspace`. The modes lane landed
copy-mode-current-match-style in `crates/zz-tui` render.rs, so zz paints the
current match where it used to write reverse video. Three consecutive green
runs and a green `--self-check` at this revision, after which the fixture has
no recorded case at all: 147 of 147 assert every channel they drive.

ONE GATE COMMIT THE LANE DID NOT OWE. `knowledge/tmux/gaps.md` was stale AT
origin/main: main's own 268ccd9d edited `compat/tmux-gaps.json`
(config.discovery, presentation.native-status) without regenerating the report,
and `compat/run.sh` and `compat/check.sh` both refuse while the two disagree.
Checking out origin/main's two files alone into the gate worktree reproduces
`knowledge/tmux/gaps.md is stale`. Commit 1166303b regenerates it.

WHAT THE GATE DID NOT DO. `compat/tui-overlays.sh` and `compat/tui-choosers.sh`
are not on main at this tip; they belong to later lanes in the cycle-6 order.
`knowledge/tmux/key-tables.md`, the lane's declared zone excursion, was left
alone: no keys or modes lane touched that sentence, so there was no collision.
