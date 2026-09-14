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

- `command-item-format`, the delta row this box needs up to eight minutes for,
  was run alone at the end and is clean: 125 steps, no divergence in any
  channel. Its result line is the last one in `10-corpus-delta.txt`, so 227 rows
  completed in all.
- 167 rows were selected and 227 completed (the smoke set comes in whole with
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

# The cycle-9 fix pass, ubuntu box, 2026-09-14

The adversarial review of `142139fd` came back REJECT with two blockers, one
wire blocker and one honesty must-fix. A fresh agent took the branch over on
the ubuntu box and committed on top; nothing above this line was rebased,
amended or reordered. `environment.txt` carries this box's block at its end;
every file numbered 19 and above is a real run at `8bef1252`, the tip of the
two fix commits, and the ledger commit that follows them touches only
`compat/tui/` and the two generated reports.

## Files this pass added

- `19-probe-blockers-before.txt`, `20-probe-blockers-after.txt` — the two
  gestures the review names, driven at `142139fd` and at the fix. The probe is
  this fixture's own driver with `run_cases` replaced.
- `21-wire-version.txt` — `compat/wire-version.py`, read out of `origin/main`
  because this branch's base predates it, exit 0.
- `22`, `23`, `24-tui-mouse-run-*.txt` — three runs, byte-identical at md5
  `76e3cc87`, each `35 asserted checks, 3 recorded checks` and `all 35
  asserted checks identical`, where `142139fd` left 33 and 3.
- `25-tui-mouse-self-check.txt` — three quiet controls and NINETEEN one-sided
  sabotages, each caught in its own channel; two are new this pass.
- `26`, `27-tui-stock-keys*.txt` — 50 cases agree, 8 recorded elsewhere; every
  sabotage caught. See "the eighth recorded row" below.
- `28-tui-copy-mode.txt` — 147 cases, 0 recorded.
- `29-tui-caps.txt` — 366 asserted rows, 0 recorded.
- `30-attached-client.txt` — `attached-client compatibility: PASS`.
- `31-corpus-delta-non-smoke.txt` — the 21 non-smoke rows of the delta
  selection, every channel clean on every row.
- `32-cargo-clippy.txt`, `33-cargo-test.txt`, `34-compat-check.txt`.

## What the two blockers were, and the one fix

Both are the same hole. `server_client_handle_key` ends an unmatched mouse key
at `forward_key`, where `window_pane_key` re-encodes the event for the pane
and `input_key_mouse` writes it while that pane has a mouse mode armed. The
daemon consumed it instead.

1. A drag lost its release. With a pane on `\033[?1002h\033[?1006h`, the pin's
   pane printed `\e[<0;2;3M`, `\e[<32;8;3M`, `\e[<0;8;3m` and the tip's printed
   the first two. `MouseDown1Pane` and `MouseDrag1Pane` are stock root rows
   that run `send -M` while the pane tracks; the release is
   `MouseDragEnd1Pane`, which `key-bindings.c` installs in the two copy tables
   and in no root table, so root is the first and only table tried and the pin
   forwards it.
2. A double click reported out of order. The pin gave press, release, press,
   release; the tip gave press, release, release, press. `SecondClick1Pane` is
   bound nowhere in root either, so the pin's second press reaches the pane the
   moment it arrives, where the client was swallowing it and the 300 ms click
   timer delivered it behind its own release, through `DoubleClick1Pane`'s
   `send -M`.

The fix, in three hunks:

- `crates/zz-daemon/src/daemon.rs` `input_mouse_key` forwards
  `mouse.view_action` to the pane when no binding matched AND root was the
  first table tried — `mode_table` empty and the client's active table root —
  which is the pin's `first != table` guard. It forwards only a
  `TerminalViewAction::Mouse`, because `input_key_mouse` writes only while
  `ALL_MOUSE_MODES` is set and what the client encoded for its own pointer
  handling is not a report.
- `crates/zz-tui/src/input.rs` drops `MouseKeyRoute::Consume`: a click-sequence
  name with no binding now goes to the daemon like any other, and the daemon
  decides.
- `crates/zz-mux/src/command.rs` `send-keys -M` writes nothing when the
  invoking key is a `DoubleClick` name. That is `m->ignore`, which
  `server_client_check_mouse` sets on the replayed event and on nothing else
  (`server-client.c:838,908`, `input-keys.c:805`), and it is why the pin's
  pane sees four reports and not five. `window_copy_command`'s own cursor move
  reads `m` regardless of `ignore` (`window-copy.c:3725`), so the copy-mode
  half of the same binding is untouched and `multi-click` stays green.

## The two new cases and their sabotages

`app-mouse-drag` and `app-mouse-double-click` drive exactly those gestures with
`\033[?1002h\033[?1006h` armed on both sides and compare the reports the pane's
own program printed, read off the one row it prints them on. The double-click
case waits for the pin's report count to HOLD for twelve consecutive polls,
which is longer than `KEYC_CLICK_TIMEOUT`, so the reading is taken after the
replay has or has not happened rather than racing it.

Each has a one-sided sabotage that binds the name the pin leaves unbound, in
zz's root table only, to a silent `set-option`:

- `MouseDragEnd1Pane` bound on zz: `tmux: ^[[<0;2;3M^[[<32;8;3M^[[<0;8;3m`
  against `zz: ^[[<0;2;3M^[[<32;8;3M`.
- `SecondClick1Pane` bound on zz:
  `tmux: ^[[<0;5;3M^[[<0;5;3m^[[<0;5;3M^[[<0;5;3m` against
  `zz: ^[[<0;5;3M^[[<0;5;3m^[[<0;5;3m`.

Both name exactly the event the fix restored. The two cases read one screen row
rather than the four `program_output` reads, because a message an earlier
sabotage left on the row below would otherwise ride along in a channel that is
about the reports.

## The wire

`compat/wire-version.py` on `origin/main` found it: the branch appended
`view_action` and `press_action` to `InputMessage::MouseKey` while
`PROTOCOL_VERSION` stayed at 102, and 102 shipped in zz 0.9.0 and 0.9.1. The
number is 103 now, the two appends moved out of the v102 entry of
`knowledge/protocol/wire-protocol.md` into a v103 one, and THREE assertions
moved with it, not two: `crates/zz-protocol/src/message.rs`,
`crates/zz-protocol/tests/hunt_claims.rs` line 18 (whose test name carried the
number and was renamed) and the byte-for-byte hello frame in the same file,
which carries the version twice as `0x66` and now carries it as `0x67`.

## The eighth recorded row in tui-stock-keys

`application-reader` recorded this pass where attempt-03 had it `ok`, with the
fixture's own reason: zz reports `#{pane_current_command}` a beat later than
the pin when a pane's child starts. It is a recorded channel, so the run still
exits 0, and nothing in this pass's diff touches `pane_current_command`: the
three hunks are the daemon's mouse-key fallthrough, the client's mouse-key
route and `send-keys -M`. It is this box under four agents, not a regression.

## Coverage this pass did not reach

The delta selection for `send-keys`, `copy-mode` and `list-keys` is 167
scenarios, 146 of them the smoke set that comes in whole with any delta. The 21
non-smoke rows are in `31-corpus-delta-non-smoke.txt`, every channel clean. The
smoke set was run by attempt-03 at the pre-fix tip and is named in
`10-corpus-delta.txt`; this pass's three hunks cannot reach a corpus row at all,
because every one of them is on a path a decoded POINTER opens — `input_mouse_key`
runs only for `InputMessage::MouseKey`, which only a client with a pointer
sends, and the `send-keys -M` gate needs an invoking key named `DoubleClick`,
which only the click timer produces.

## The corpus rows this pass ran, after the first push

- `35-corpus-delta-smoke-matched.txt` — the 30 smoke rows of the delta
  selection whose text names `send-keys`, `send -`, `copy-mode`, `list-keys`,
  `bind-key`, `bind -`, `Mouse`, `Wheel`, `Click`, `select-word` or
  `paste-buffer`. Every channel clean on every row.
- `36-corpus-delta-slow-rows.txt` — `copy-mode-stock-action-keys` and
  `smoke/command-prompt-editing`, the two slow rows closest to this pass, both
  clean. `command-item-format`, the third slow row, was run by the first pass
  and is the last result line in `10-corpus-delta.txt`.
- `37`, `38-tui-mouse-*-at-the-ledger-tip.txt` — the fixture and its
  self-check re-run at `c7b924d5`, the ledger commit, which touches only
  `compat/tui/` and the generated report. The run is byte-identical to the
  three at `8bef1252`, same md5 `76e3cc87`.

53 of the selection's 167 rows ran here. Nothing in this pass's three hunks can
reach a corpus row: `input_mouse_key` runs only for `InputMessage::MouseKey`,
which only a client with a pointer sends, and the `send-keys -M` gate needs an
invoking key named `DoubleClick`, which only the click timer produces.
