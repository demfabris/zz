# TUI-008 attempt-03, the cycle-9 mouse lane

Cycle 9 punch list. `environment.txt` first; every other file here is a real
run at this branch's tip on alienware against pinned tmux d77c9dc6. Attempt-02
and its gate carry the history; nothing here restates it.

## Files

- `environment.txt` — box, tip, base, binary hash, pin, toolchain, locale.
- `01-tui-mouse-run-1.txt`, `02-tui-mouse-run-2.txt`, `03-tui-mouse-run-3.txt` —
  compat/tui-mouse.sh three times, byte-identical at md5 `cd55b00d`, each
  `33 asserted checks, 3 recorded checks` and `all 33 asserted checks identical`.
- `04-tui-mouse-self-check.txt` — the fixture's `--self-check`: three controls
  that stay quiet and seventeen one-sided sabotages, each caught in its own
  channel. Three of the sabotages are new this attempt.
- `05-tui-stock-keys.txt`, `06-tui-stock-keys-self-check.txt` — 50 cases agree,
  7 recorded elsewhere; every sabotage caught.
- `07-attached-client.txt` — `attached-client compatibility: PASS`.
- `08-tui-copy-mode.txt` — 147 cases, 0 recorded. This is the fixture the
  select-word change had to keep green.
- `09-tui-caps.txt` — 366 asserted rows, 0 recorded. This is the fixture that
  asserts `#{mouse_any_flag}` on both sides, so the move from a constant zero to
  the pane's own tracking had to leave it identical.
- `10-corpus-delta.txt` — the delta corpus for the three commands this landing
  changes, 226 rows completed, with the result line of each.
- `11-client-non-utf8-cwd-absolute-binary.txt`,
  `12-client-non-utf8-cwd-relative-binary-diagnosis.txt` — see "the one red I
  chased" below.
- `13-cargo-clippy.txt` — clippy with `-D warnings` over the seven touched
  crates, exit 0.
- `14-cargo-fmt.txt` — `cargo fmt --all -- --check`, exit 0.
- `15-compat-check.txt` — compat/check.sh, exit 0, which includes
  compat/tui/verify-claims.py ("every verified obligation holds up") and the
  daemon's delegated-format inventory test.

## What moved

Ten recorded checks at the cycle-8 tip, three now. Every channel that flipped
gained a one-sided sabotage in the same commit.

1. `wheel-up-pane` (2 checks). `send-keys -M` hands the invoking event to the
   pane the way `window_pane_key(wp, tc, s, wl, m->key, m)` re-encodes `m`
   through `input_key_pane`, and `#{mouse_any_flag}` answers the pane's own
   tracking against `ALL_MOUSE_MODES` instead of the constant zero it was.
   With both, the pin's four stock pane-body root rows install and behave.
   Measured: `wheel-up-pane/pane-in-mode` and `/pane-mode` are `1` and
   `copy-mode` on both binaries, where zz answered `0` and empty.
2. `drag-selects` (3 checks). The copy tables' mouse names are reachable from a
   pointer: the client offers them while the pane the pointer landed on holds a
   mode or while the release ends a drag a binding claimed, and the daemon looks
   a mouse key up in `wme->mode->key_table(wme)` before the client's own table.
   Measured: `drag-selects/buffer` is `DRAGLIN`, `/pane-in-mode` `0` and
   `/selection` empty on both, where zz answered empty, `1` and `1`.
3. `multi-click` (2 checks). The click sequence lives on the client, which is
   where the pointer is: `CLIENT_DOUBLECLICK`, `CLIENT_TRIPLECLICK` and the
   300 ms `KEYC_CLICK_TIMEOUT`. Measured: `multi-click/double-buffer` is `alpha`
   and `/triple-buffer` is `MULTI alpha beta gamma` on both, where zz answered
   empty for both.

## Two things the multi-click chain needed, each measured on the pin

- `window_copy_command` moves the copy cursor to the event's cell before every
  `send -X` run from a mouse binding that is not a wheel. Without it the pin's
  `DoubleClick1Pane` chain selected a word at the pane's own cursor, not under
  the pointer.
- `window_copy_cursor_next_word_end` leaves the emacs cursor ONE CELL PAST the
  word, and the emacs end is exclusive, so the pair copies the whole word.
  zz left the cursor on the word's last cell and its copy dropped that cell.
  Measured on the pin with `mode-keys emacs`, `alpha beta gamma`, a `select-word`
  at column 7: the cursor lands at 10 and the buffer reads `beta`; zz left it at
  9 and the buffer read `bet`. Four zz tests had recorded the old cursor and
  were corrected against that measurement, including
  `word_selection_resolves_a_whitespace_cursor_outward_like_the_pin`, whose
  backward case the pin answers with cursor 6 and buffer `beta`.

## The one red I chased

`smoke/client-non-utf8-cwd` went red at this tip with `NON_UTF8_CWD=broken`
against the pin's `clean:12`. It is not a divergence: the fixture `cd`s into the
non-UTF-8 directory before it runs the binary, so a RELATIVE `ZZ_COMPAT_ZZ`
stops resolving. A one-shot diagnostic that printed the probe exit codes gave
`d127n127q127m127`, four `command not found`
(`12-client-non-utf8-cwd-relative-binary-diagnosis.txt`); the same row with an
absolute `ZZ_COMPAT_ZZ` is clean
(`11-client-non-utf8-cwd-absolute-binary.txt`), and running the fixture script
directly against this tip's binary answers `clean:12`. The diagnostic edit to
compat/scenarios/smoke/fixtures/client-non-utf8-cwd.sh was reverted before any
commit; `git status` is clean on compat/scenarios. Every chunk after that used
an absolute path.

## The other reds, none of them this lane's

- `known/known-main-preset-two-panes`, `known/known-pane-scrollbar-columns`,
  `known/known-spread-mixed` and `known/known-terminal-runtime` each hit their
  exact documented divergence and run.sh says so.
- `micro-flags` and `lane2-store` are two of this box's documented
  environmental rows.
- `smoke/status-background-jobs` is the timing-sensitive `#(date +%s%N)` status
  job the cycle-8 keys gate sampled nine times at origin/main on this box and
  found 2 green to 7 red.

## Coverage this attempt did not reach

- `command-item-format` is the delta row this box needs up to eight minutes for,
  which does not fit a single 590-second call; it is not run here. It is a
  `list-keys` row and its neighbours `list-keys-padding`, `copy-mode-bindings`
  and `copy-mode-stock-action-keys` are all clean at this tip.
- 167 rows were selected and 226 completed (the smoke set comes in whole with
  any delta), which leaves part of the smoke set unrun. Every smoke row whose
  text names `send-keys`, `send -`, `copy-mode`, `list-keys`, `bind-key`,
  `bind -`, `Mouse`, `Wheel`, `Click`, `select-word` or `paste-buffer` was run
  and is clean.

## Zone excursions

- `crates/zz-terminal/src/interaction.rs`: `CopyModeAction` gains a trailing
  `MouseCursor(PointerCellEvent)`, forced by the measurement above - the cursor
  move `window_copy_move_mouse` does must not disturb the selection, and every
  existing pointer action in copy mode resets it.
- `crates/zz-daemon/src/status.rs`: one match arm so `mouse_any_flag` answers
  from the delegated facts path, forced by
  `daemon_delegated_format_consumers_match_mux_inventory`, and the partition
  counts in `crates/zz-mux/src/compat_manifest_tests.rs` with the one claim in
  `crates/zz-mux/tests/hunt_claims.rs` that asserted `send-keys -M` was
  unsupported. All moved in the same commits.
- `compat/tmux-gaps.json` `formats.terminal-runtime`, which is not one of this
  batch's four gaps: `compat_manifest_tests` refuses a tracked item for a format
  zz answers, so `format:mouse_any_flag` had to move with the backing.

## An observation this lane did not chase

Under emacs `mode-keys`, a BACKWARD word selection ends on the word's last cell
and zz's exclusive emacs end drops it: the pin copies `beta` where zz would copy
`bet`. No fixture or corpus row drives it and it is not on this batch's punch
list, so it is named here rather than fixed or recorded as a case.
