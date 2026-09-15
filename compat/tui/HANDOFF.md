# TUI parity handoff: cycle 9 resumed on the ubuntu box, paused mid-flight to move again (2026-09-14)

Cycle 9 was stopped on alienware before its gates. The ubuntu box picked it up the same evening,
ran the fix passes, the missing review and the first two gates through the Agent tool (one Opus 5
agent per lane, prompts adapted from `run-9.js`, `run-9b.js` and `run-10.js`), and was paused on
fabrico's word ("pause as the agents come in, we'll take this to another machine") once the choosers
gate had pushed main. Nothing was killed: every agent that was running finished and its result is
below. The resume point is a gate order, not a lane: two branches landed on main, one is reviewed
and waiting with a blocker, one has its review fixes landed and waits for its gate, and one cycle 10
lane has pushed unreviewed.

| Fact | Value |
| --- | --- |
| `origin/main` | `d41b815f`, the mouse gate's push (the choosers gate pushed `3ecd4702` before it) |
| Ledger | **9/12 baseline verified** (TUI-001 to 007, 009, 010); TUI-008 active, TUI-011 and TUI-012 at review; added scope 0/6 (TUI-016 at review on its branch) |
| `PROTOCOL_VERSION` | **103 on main, unreleased** (0.9.1 shipped 102; the choosers gate opened 103). Every later append folds into 103; `compat/wire-version.py` enforces it inside `compat/check.sh` |
| Board | `F-TUI-CYCLE-9-LANES` RELEASED at the pause (cycle 10's stream lane ran under it too); MAIN and TRIAGE free. Issue 7 carries every gate note |
| Runners | `run-10.js` (cycle 10, lint-clean, modes batch now names the right gap groups, wire rule pinned at 103); `run-2.js` (the deferred macOS TUI-013 run) |
| Landed this resume | main `45481a72` (restores the zz cdylib that `6515adcd` removed; Windows loads `zz.dll`), main `3ecd4702` (choosers gate: TUI-006 verified), main `d41b815f` (mouse gate: TUI-008's first half, 22 mouse gap items closed, one v103 entry carrying both lanes' appends; TUI-008 stays active) |

## The branches, in gate order

| Branch | Tip | State | What its gate does |
| --- | --- | --- | --- |
| `campaign/tui-mouse` | `bad0261f` | **GATED, on main at `d41b815f`.** Re-review approve-with-fixes after the reject (three blockers verified fixed by the reviewer's own probes); the must-fix and nit applied at the gate; TUI-008 active with proof null, three checks record, `next_action` corrected | Nothing. The menus branch rebases onto this: its eight commits sit on `bad0261f`, whose commits are on main under new shas, so `git rebase origin/main` skips the patch-identical ones; expect conflicts in `compat/tui-mouse.sh` (PASTE_MENU_REASON) and TUI-008's record, where the menus branch wins |
| `campaign/tui-mouse-menus-2` | `6fbdf4c3` on `bad0261f` | Reviewed: **approve-with-fixes with ONE BLOCKER**. TUI-008 claims review at `39 asserted, 0 recorded`, but right-click-pane/screen asserts only because it aims at a blank cell; one row up, over text, 12 of 24 rows differ (zz answers empty for `mouse_word`, `mouse_line`, `mouse_hyperlink`). The verdict is in `compat/tui/evidence/TUI-008/attempt-04/review.md` | Rebase onto main; add the recorded check `right-click-pane/over-a-word` (MODE=record, reason naming formats.mouse-context's three formats), which puts the fixture at 1 recorded and keeps TUI-008 at **review**; apply the must-fix (display-menu `-x M`/`-y M`/`-x W`/`-y W` with no invoking event answer the screen centre where the pin answers 0 and the window's status range) and the four nits; do NOT verify TUI-008, do NOT flip TUI-012 |
| `campaign/tui-introspection` | `ec813757` | Fix pass landed all four alienware review findings (one format job per attached client like the pin; the server log names tty-bearing clients by tty; 58 comment lines stripped; evidence shas corrected). TUI-016 at review on both clauses (`all 80 asserted comparisons identical, 29 recorded`), TUI-017 active with clause 1 open | Verify the four fixes with their probes, verify TUI-016, run the keys and status corpus sets plus `smoke/tui-client-input-backpressure` (it is cycle 9's last gate), leave TUI-012 held unless TUI-008 verified earlier |
| `campaign/tui-stream` | `aa65960f` on `3ecd4702` | Cycle 10's stream lane, finished, UNREVIEWED. TUI-018 at review: one bounded stdin channel (`CommandInvocation.stdin`, a 103 tail append, one reader bounded by MAX_AGENT_SEND_BYTES, one resolver `zz_daemon::command_stdin_sink`, three sinks: Argument, Config, PaneInput) carries `source-file -`, `display-message -I` and `split-window -I`; `compat/tui-command-streams.sh` (new, registered in verify-claims) ends `all 33 asserted comparisons identical, 0 recorded not asserted, 4 decided`; `compat/tui-client-commands.sh` flips four stream cases (`65 asserted, 33 recorded`). Four items of `protocol.binary-streams` closed with dated measurements; the 1 MiB cap is a recorded decision (the pin streams 16 KiB chunks with no limit). Zone excursion: six lines in `crates/zz-tui/src/render.rs` `place_viewport_cursor` (an empty pane now carries the pin's screen mode, MODE_CRLF on and cursor off, so `-E` panes match too) | Adversarial review first (`run-10.js` REVIEW_EXTRA.stream), then its gate; `verify-claims.py` needs its FIXTURES entry |

Every branch above merges cleanly or with the conflicts named under "What each gate must know".
Predict again at your own tip with `git merge-tree --write-tree origin/main <tip>`.

## What closes TUI-008 and TUI-012

The menus gate leaves TUI-008 at review with one recorded check. Closing it is a worker item, not a
gate item: the daemon needs a synchronous read of the live grid under a pointer cell to answer
`mouse_word`, `mouse_line` and `mouse_hyperlink` (zz-terminal already answers the same three for a
frozen copy-mode revision in `mode_format_word` and `mode_format_line`; the shape is named in
TUI-008's `next_action`). One lane, zones around crates/zz-terminal/src/session.rs and
interaction.rs, crates/zz-daemon/src/daemon.rs (the mouse and popup paths), crates/zz-mux/src/formats.rs,
compat/tui-mouse.sh and the TUI-008 record; it flips `over-a-word`, closes the five
formats.mouse-context items with dated SGR measurements, and its gate verifies TUI-008 and then
TUI-012 (a fresh three-run `compat/tui-superset.sh` plus `--self-check` at its own tip, appended to
TUI-012's proof; its revision `e9199e38` is far behind). That takes the baseline to 11/12.

## What each gate must know

1. **Wire.** Main is at 103 with one v103 entry in `knowledge/protocol/wire-protocol.md` carrying
   the choosers appends (`CommandPromptState.pane`, `ProtocolMessage::ClientTerminalType`) and, once
   the mouse gate lands, the mouse appends (`view_action`, `press_action` on `InputMessage::MouseKey`).
   The menus branch appends `status_range_start: Option<u16>` at the tail of `MouseKey` with its own
   v103 line; the introspection branch has no wire change (`catalog.rs` only); the stream branch
   likely appends. On every rebase: keep ONE v103 entry with every line, keep the constant at 103,
   and remember `crates/zz-protocol/tests/hunt_claims.rs` pins the number THREE times (the named
   test, the assertion, and the hello frame where the version appears twice as bytes).
2. **Conflicts.** `knowledge/tmux/gaps.md` and `knowledge/tmux/tui-parity.md` are generated: take
   either side and run `python3 compat/tmux-tracker.py write-report` and `python3
   compat/tui/tracker.py write-report`. `compat/tui/campaign.json` and `compat/tmux-gaps.json` merge
   by record and by item; serialise with `ensure_ascii=False` so no other record's bytes move (two
   lanes re-serialised the whole file with escapes this cycle). `knowledge/index.md` and
   `knowledge/protocol/index.md` carry the wire version in one word each.
3. **The menus branch supersedes the mouse record.** The mouse gate corrects TUI-008's
   `evidence_note` and `PASTE_MENU_REASON` (the paste-under-menu divergence the first half described
   never reproduced: seven of seven runs identical, and the menus review proved it independently
   with a tail carrying a second menu key). The menus branch rewrites the same record and flips the
   case; where they conflict, the menus branch's `compat/tui-mouse.sh` and TUI-008 record win.
4. **Cycle 9's last gate runs the shared corpus sets** (every keys and status scenario and
   `smoke/tui-client-input-backpressure`), which is the introspection gate. Earlier gates run the
   delta for their own touched commands only.
5. **TUI-014's five commands live in three gaps**, not one: clock-mode, customize-mode and
   suspend-client in `commands.native-client-tools`, `command:switch-mode` in
   `clients.interactive-refresh`, `command:server-access` in `protocol.socket-acl`. `run-10.js`'s
   modes batch now says so; the cycle 9 punch list did not, which is one reason that lane closed
   nothing there.

## Reds at origin/main on the ubuntu box that alienware never saw

The choosers gate built zz at `origin/main` (`45481a72`) in its own worktree and reproduced three
reds there, so none was charged to a lane. Every earlier gate on alienware had all three green.
**The next box measures these before it charges anyone**: red there too means a main regression
that needs its own lane; green there means an ubuntu box difference to record in this file.

- `compat/attached-client.sh` fails in `probe_command_output_navigation`: `n` does not advance to
  the next match in retained command output (`zz current screen did not show ATTACHED_NAV_65
  ATTACHED_NAV_MATCH within 10 seconds`). That probe rebinds `/` to zz's native
  copy-mode-search-prompt on the zz side, so it drives the replacement binding TUI-006's clause 2
  excludes by its own words; the stock-binding half is asserted in `compat/tui-choosers.sh`. The
  reasoning is in TUI-006's proof block so it can be overruled with full information. The fixture is
  a declared source of six verified obligations, and the menus review and the mouse fix pass BOTH
  saw it PASS on this box the same day, so it is at least intermittent here.
- `compat/tui-overlays.sh --self-check` never settles on its closing equivalence (`equal settled on
  the zz screen did not settle within 10 seconds`), five tries, all seven sabotages caught. The
  mouse gate found the cause: the control's C-l reaches the pane instead of dismissing the
  display-message on BOTH binaries, and the pane's `/bin/sh` has no line editing, so the literal
  `^L` prefixes the next mark's printf and MARK-equal never prints. `/bin/sh` is dash on Ubuntu and
  bash on CachyOS, which is the likeliest box difference; the fixture should not depend on it.
- Three zz-daemon `russh_socks` loopback tests fail with ConnectionReset (module and Cargo.lock
  byte-identical to main), deterministic solo and at origin/main; both gates ran `cargo test -p
  zz-daemon -- --skip russh_socks::tests`. They belong to main's SSH work and want an owner.
- Note from the mouse gate: nobody had run the whole fixture tree at main for a while;
  `tui-launch-diff.sh` reports 4 recorded where it reported 3 before the choosers merge, exit 0.

## Residuals, none charged to a lane, all in the board notes

- Stale `(N results)` in the copy-mode position indicator on TUI-005's surface: the daemon
  re-expands the indicator only when its memo key moves, and `viewport.search` in
  `crates/zz-terminal/src/session.rs` `copy_mode_facts` is built from `view.search` unconditionally.
  One-line fix suggested by the choosers reviewer: `.filter(|_| mode.search_marks)` on that field,
  plus a fixture case (no fixture can see it today: `tui-copy-mode.sh` pins
  `copy-mode-position-format` to empty and `tui-choosers.sh` to a form with no results clause).
- The backward emacs word selection (pin copies `beta`, zz copies `bet`), parked in TUI-008's
  `next_action` with the pin's measurement; no fixture or corpus row drives it.
- A triple click and a wheel under application tracking are driven by no fixture case (both agree
  in the mouse reviewer's own probe).
- `display-menu -x M/-y M/-x W/-y W` without an invoking event (the menus must-fix above).
- The select-word change in `crates/zz-terminal/src/session.rs` (emacs cursor one cell past the
  word) reaches every client including the GUI; declared in TUI-008's record and reversible.
- `command:switch-mode` and `command:server-access` group attribution (fixed in `run-10.js`).
- `cli_binary daemon_autostart::nested_attach_inside_a_pane_prints_the_pinned_refusal` and the
  application-reader `#{pane_current_command}` record in `tui-stock-keys.sh` are load-dependent
  (the latter reads 7, 8 or 9 recorded depending on load).

## CI

Main has had no green CI run since 2026-08-14. `45481a72` fixed the Windows leg (the cdylib the
launcher loads had been deleted). The Linux leg fails `compat/run.sh --check-summary` because
`compat/results/summary.md` was last stamped at `996a8d0d` and the scenarios and
`compat/attached-client.sh` have moved since; only a full `compat/run.sh` with `--attached-client`
at a commit reachable from HEAD restamps it (about 90 minutes). That belongs to the close-out, after
the last gate, together with running every fixture in the tree with `--self-check`.

## How this resume ran, and what to copy

- **Agent tool, not the Workflow tool**, on fabrico's ask ("use opus 5 subagents"). One
  general-purpose agent per lane with `model: opus`, prompts lifted from the runners with a box note
  for this machine. What changed from the runner shape and is worth keeping: a gate does its rebase,
  tests and fixtures WITHOUT waiting for MAIN and claims MAIN only for the final fetch, rebase and
  push (the choosers gate held MAIN for four hours doing work that needed no lock); each gate gets
  its own worktree (the finished lane's, with its warm target), never a shared gate worktree; a fix
  pass commits on top of the rejected tip and never rebases, so the second-half lane sitting on the
  same tip can rebase onto it; a second-half lane pushes a NEW branch name rather than force-pushing.
- **Warm targets by reflink.** Each worktree got `cp -a --reflink=always ~/dev/zz/target
  <worktree>/target` (instant on btrfs, deps warm, workspace crates rebuild once in 5 to 15
  minutes). The cost: every rebuild diverges the copy by 15 to 20 GB, four lanes filled the disk in
  two hours, and the fix was deleting a stale 98 GB cache, each finished lane's target, and
  `target/release`, `target/ui-showcase` (deleted from the repo in `8ba0dbf1`) from every copy.
  Delete a lane's target the moment its agent finishes and its next consumer builds elsewhere.
- **Never symlink `compat/.cache` into a worktree.** The menus lane did, `compat/check.sh` called
  `compat/fetch-tmux.sh`, the stamp did not match through the link and it REBUILT the shared pin
  under three other agents (same commit, one corpus chunk and one fixture run died and were
  re-run). Export `ZZ_COMPAT_TMUX` and `ZZ_COMPAT_CORPUS` for `compat/check.sh` in a worktree, or
  run it from the shared checkout read-only.
- **The ubuntu box**: Ubuntu 26.04.1, 8 cores, 30 GB plus 16 GB swapfile and 7.5 GB zram, btrfs,
  bash 5.3, python 3.14, en_US.UTF-8. Cargo wrapper: `S=$((RANDOM % 2)); systemd-run --user --scope
  -q -p MemoryMax=8G -p MemorySwapMax=4G flock -w 540 /tmp/zz-cargo-slot-$S.lock cargo <args>
  --jobs 3` (10G and `--jobs 4` for a gate). Four agents at a time was the ceiling that held; a gate
  here takes three to four hours because it runs every fixture plus a corpus delta on half
  alienware's cores. `daemon::tests::explicit_boot_configs_replace_default_discovery_and_load_in_order`
  needs `HOME=/tmp/zz-emptyhome XDG_CONFIG_HOME=/tmp/zz-emptyhome/config`. Worktrees left in
  `~/dev`: `zz-tui-mouse` (mouse gate), `zz-tui-menus-9`, `zz-tui-introspection` (no target),
  `zz-tui-stream-10`, `zz-gate-tui9`; all caches, branches are on origin.

## Resuming on another machine

The campaign resumes from the repo and the board, not from a session. On a fresh box:

1. `compat/fetch-tmux.sh` and `compat/fetch-corpus.sh` populate the caches; `compat/check.sh`
   proves the checkout is sound and the wire version honest (103, unreleased).
2. `python3 compat/tui/tracker.py check` and `ready` read the real ledger state. Do not trust a
   remembered count.
3. `export ZZ_BOARD_HOLDER=<box>/orchestrator`; `python3 compat/board.py status`; claim
   `F-TUI-CYCLE-9-LANES` (released at the pause) or mint a cycle 10 front under TRIAGE once cycle 9's
   branches have landed.
4. Measure the three ubuntu-box reds above at `origin/main` before any gate runs.
5. Gate in the order this file's table gives: mouse (if still waiting), menus, introspection; review
   and gate stream; then `run-10.js` for modes and capture with `args` for your box (`root`, `dev`,
   `holder`, `machine`, `date`, `boxNote`; on macOS replace the `systemd-run` wrapper, which does not
   exist there, with a plain two-slot `flock`). Pre-position the worktrees the runner expects.
6. The TUI-008 closing lane described above, then its gate for TUI-008 and TUI-012; the cycle 10
   capture gate for TUI-011 and the twelve-item baseline; the close-out with every fixture and the
   corpus restamp.
7. A macOS box also unblocks TUI-013 (`run-2.js`, recipe in `compat/tui/README.md`).
