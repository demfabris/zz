# Handoff for the tmux-compat campaign (registry closed; instrument pass complete locally)

Updated 2026-09-05 UTC on the macbook after finishing the instrument pass on local branch
`codex/attached-client-instrument`, based on published `f39ab1e0`. The full strict corpus and
attached-client fixture pass at `f80405390af4`, and `compat/run.sh --check-summary` passes here.
Read "The instrument pass, as left" and `CAMPAIGN-LOG.md`'s last entry before resuming. These
commits have not been pushed or integrated into main; main's old summary remains unstamped.
THE REGISTRY IS EMPTY: meter 100.0% (304/304), 65/65 groups, 0 open, 0 blocked, 174 closed records,
42 accepted groups. The implementation phase of the campaign as scoped on 2026-08-31 is done.

The broader campaign still has work. The 2026-09-04 retrospective (`CAMPAIGN-REVIEW.md`, eight
Fable reviewers and a critic) found fifteen ranked gaps outside the registry's original scope.
The instrument pass now provides a current attached-client proof on this branch. Delivery of these
commits comes next, followed by cycle 14 from the report's "Next cycles" section. Cycles 14 to 16
have not started, and the practical exit gate still requires their daily-use findings to be settled.

## Current workflow (2026-09-05)

Fabrico's instruction is to implement the agreed work, then run validation after it is done.
Work directly in the current session. The campaign does not prescribe a model, reasoning effort,
delegation, agent roles, lane count, orchestration tool, or time budget. The old run scripts and
dated reports describe previous runs; they are historical references, not templates for new work.

1. Read the current checkpoint and choose a bounded implementation batch from "Next cycles" in
   `CAMPAIGN-REVIEW.md`. Keep its acceptance criteria and board ownership explicit.
2. Finish the batch, including fixtures and any integration conflicts. Use small pin probes or
   local unit checks when needed to resolve a concrete implementation question. Defer corpus,
   attached-client, and workspace-wide validation until the batch is complete.
3. Combine all changes for the batch into one candidate and inspect the diff. Commit the code
   before the final harness run so its stamp names the tested revision. Keep the code and harness
   fixed while validation runs.
4. Run the required workspace checks and one full `just compat --strict-geometry --attached-client`
   on that completed candidate. Confirm full scenario counts, the registered known rows, a clean
   attached-client PASS stamp, and `just compat --check-summary` before declaring it validated.
5. If validation fails, finish the repairs before rerunning the affected checks. A failed or
   invalidated full run still requires a successful full run before recording a current PASS.
   Preserve valid results for unchanged inputs; documentation-only changes and publishing the
   unchanged tested history do not require another full run.
6. Update the registry, summary, tracker and log with the actual results. Publish the completed
   batch together when authorized, holding MAIN. Separate commits within the batch do not each
   need a full run or a separate publication gate.

The stamp rule stays strict. A partial run or stale PASS cannot stand in for final validation.

The standing instruction from fabrico (2026-09-02) is "turn on the goal and go all the way": run
cycles until the registry is closed, taking the product decisions the earlier handoff had parked
and recording each one in the registry as reversible. Two were taken in cycle 10 (`command-prompt
-P` accepted as a client-owned presentation hint; `scroll-to-mouse` settled as a scrollbar-only
name zz has no slider grab for), both with the sentence "decided 2026-09-02 by the orchestrator
under fabrico's instruction to close the campaign; reversible" in their records.

## What this campaign is

The goal is `alias tmux=zz`: a tmux user brings their config, plugins, scripts and habits to zz and
nothing breaks. zz is a superset of tmux (a GPU desktop app with browser and agent panes over a
tmux-shaped daemon), and nobody switches multiplexers if their setup stops working, so
compatibility is the adoption story. It is also the cheapest bug finder there is: every
difference from tmux is either a defect or a decision we have to own in writing.

The pieces, in the order a new reader meets them:

- **The oracle** is a pinned tmux build (`d77c9dc6`, next-3.8) under `compat/.cache/tmux-src/`.
  Real tmux is the truth, not the man page; workers read its C source freely.
- **The harness**, `compat/run.sh`, runs the same scenario script against tmux and zz and diffs the
  answers: layout geometry, format expansions, command output, hooks. `compat/scenarios/` holds
  the corpus (221 scenarios at this checkpoint), `smoke/` the ones with real pty clients.
- **The registry**, `compat/tmux-gaps.json`, lists every known difference in groups, each item with
  the exact proof that closes it. Closing an item means removing it after that proof; a difference
  we keep on purpose is relocated into an accepted group with the measured tmux behaviour and the
  reason. `knowledge/tmux/gaps.md` is generated from it (`compat/tmux-tracker.py write-report`).
- **The meter**, `compat/progress.py`, scores the registry against a list of 304 items frozen on
  2026-08-31 (`compat/progress-baseline.json`), so the percentage cannot be gamed by moving goalposts.
- **The board**, GitHub issue #7 driven by `compat/board.py`, is the work ledger: fronts are
  minted, claimed, gated and integrated as comments, with zone locks so parallel agents do not
  collide. `TMUX_COMPAT_TRACKER.md` at the repo root is the human-readable checkpoint.
- **The cycles** group related implementation work. Finish the chosen batch before running its
  final validation. `CAMPAIGN-LOG.md` and the old run scripts preserve previous runs.

What gets compared is the daemon, not the screen. Both binaries receive the same commands and
their observable answers are diffed (`list-panes`, `capture-pane`, `display-message` formats,
hooks, options, key tables, copy-mode state, control-mode output). The zz raw TUI draws a sidebar
and chrome rows around a pane, so it is never cell-compared to tmux's 80x24; it is used as a real
pty client to drive copy mode, prompts, choosers, mouse and paste, after which daemon facts are
read. The desktop GPUI app keeps its own look entirely.

## State now

| Fact | Value |
| --- | --- |
| `origin/main` | `f39ab1e0` when this instrument branch started; resolve the live remote before delivery |
| Agreed-scope meter | 100.0% (304/304 items), 65/65 groups done, 0 partially burned; `python3 compat/progress.py`. Beside it publish the honest denominator the retrospective asks for: 304 of 757 identified items, because the 42 accepted groups hold 453 more |
| Live registry | 0 open groups, 0 blocked, 0 items; 174 closed records, 42 accepted groups |
| Corpus | 221 scenarios / 2,655 steps, 3 exact registered known rows; attached-client PASS recorded at `f80405390af4` on the macbook; `--check-summary` passes on this branch |
| `PROTOCOL_VERSION` | 98 (hex hello frame 0x62, test `..._ninety_eight`); the next wire change bumps to 99 (0x63) |
| Unmerged work | Local `codex/attached-client-instrument` in `~/dev/zz-attached-instrument`; implementation and proof records await delivery |
| Board (issue #7) | Instrument work used MAIN with holder `macbook/attached-instrument-0904`; read the live board for lease state. Cycles 10 to 13 are INTEGRATED; the `F-SPLIT-MUX-*-V5` chain remains outside this campaign |
| Remotes | SSH (`git@github.com:demfabris/zz.git`) works on the ubuntu box; the macbook was switched to HTTPS through gh's credential helper in cycle 7 |

## The client lane landed (2026-09-03)

`opus-compat-run-10b.js` ran its `stage: "gate"` on the ubuntu box and the lane is merged at
`9ddeae0`. All six reviewer must-fixes went in first, each with the reviewer's measurement
reproduced by reverting the fix: `menu_add_item` drops a row whose format expands empty instead of
turning it into a separator; a popup-owned `MenuSession` leaves with the popup; `popup_pointer`
records the previous report on every report the way `tty_keys_mouse` refreshes `tty->mouse_last_*`;
`Fill Space` stops resizing the popup's job; neither `Fill Space` nor `Centre` rewrites
`ppx`/`ppy`/`psx`/`psy`; and the `ProtocolMessage::Popup` schema row gained its v96 `Pointer`
variant. New scenario `smoke/display-popup-menu-policy` pins the last three on both binaries.

Three corpus rows fail on this box and are NOT any lane's: `smoke/format-modifier-interrogate`,
`smoke/pane-engine-knobs-input` and `smoke/remain-on-exit-format`. The client gate re-ran all three
alone and again in a baseline worktree at `origin/main`, where they fail identically, two of them
on the pin side only. Treat them as this box's terminfo and signal-name environment.

## What is left: nothing in the registry, fifteen things outside it

Cycle 12 (`595616b`, `12b4776`) closed `clients.path-encoding` in full and the chooser vocabulary,
and measured the geometry residue rather than closing it; cycle 13 (`fd2e790`, `37e8df0`) closed
that residue in the direction the measurement pointed and the three byte consumers cycle 12's
reviewer found. `CAMPAIGN-LOG.md` has both entries. Every frozen-scope item is closed.

`CAMPAIGN-REVIEW.md` is the census now. Its ranked findings, in the order a switcher meets them: the
desktop app discards the tmux status line entirely; `~/.tmux.conf` is never read at boot and the
harness never measured discovery because it injects every config with `-C source-file`; pane
processes have no `tmux` on PATH, so the plugin corpus was proven under a wrapper the product does
not ship; the default prefix table kills without `confirm-before`; `S-Left` and its siblings can
never fire; `#{history_size}` answers 0 so tmux-resurrect silently loses scrollback; `save-buffer -`
is refused though oh-my-tmux in the corpus uses it; mouse-key bindings never fire; status `#()` is
synchronous with a 2 s cap; a custom key table is never left; the attached fixture was red under a
stored PASS (fixed by the instrument pass below); control-mode notifications were never diffed;
`refresh-client -S` errors; the raw TUI keeps a 29-column sidebar at 80 columns; the CLI output
writer changes bytes.

### The instrument pass, as left (completed 2026-09-05 UTC on the macbook)

One owner, no cycle or reviewers. All three instruments are complete on this local branch.
`CAMPAIGN-LOG.md`'s last entry records the pin probes, implementation, and full-run proof.

DONE. `compat/run.sh` stamps the summary footer with `Recorded at: <commit>` on a full run and
`--check-summary` refuses a footer with no stamp, a `-dirty` stamp, a stamp that is not an
ancestor of HEAD, or a stamp behind which `compat/attached-client.sh` or `crates/` changed. The
stored footer on this branch records PASS at `f80405390af4` after a full strict corpus plus fixture
run on the macbook. `--check-summary` passes. Main remains red until this branch is delivered.
Record one full run for the completed batch whose code will be published.
`ZZ_COMPAT_ZZ=<path to a built zz>` skips `run.sh`'s own build.

DONE. `compat/diff-scenario.sh` has a `launcher:` header: the zz side runs the `zz_cli` launcher
from PATH with no `--socket` and no `ZZ_SOCKET`, on the default socket under a scratch
`XDG_RUNTIME_DIR`, with no harness `tmux` wrapper. `smoke/launcher-installed-layout` proves it
(7 steps clean, summary row added). Findings 2 and 3 of the review can now be written as
scenarios. It needs `zz_cli` beside `zz` (`cargo build -p zz` builds both).

DONE, MEASURED, NOT ACTED ON. `compat/status-row.sh` runs both binaries attached inside an outer
pinned tmux at 79x24 and diffs the last row's bytes with escapes after each status option. First
run: 9 of 9 rows differ, and each difference is a finding for the desktop-status-row lane
(truecolor SGR where the pin emits named colours, different default status colours, the default
`status-right` showing the shell name where the pin shows the hostname, `#{window_height}` 22
versus 23, a ` Ctrl-\ detach` hint the pin never draws). It exits 1 on any difference and prints
both rows in `od -c` and `%q` form.

DONE. The attached-client fixture. `probe_side` now waits for terminal readiness before its first
key. `probe_command_prompt` waits for `(rename-window) main` before sending the rename keys.
The command-output view remains client-local: `#{pane_in_mode}` stays 0 on zz, and
`#{client_key_table}` remains its observable.

The Escape probe found that zz closed output when `/` opened the search prompt, before Escape.
The pin keeps output behind the prompt. The daemon now preserves that output while raising a
command prompt, and the fixture checks its key table as soon as the prompt opens. A second pin
comparison found reverse search staying at row 65 instead of returning to row 35: the output
worker had never received `mode-keys`. It now receives the initial engine knobs and live changes.
The extended daemon regression covers both settings and search cancellation with the same output
view. The focused readiness, rename, and full output-navigation probes pass on both binaries.

Launcher mode now keeps its runtime under a short `/tmp/zzcl.XXXXXX` directory, so a long worktree
path cannot exceed the Unix socket path limit. The launcher still discovers its default socket
through `XDG_RUNTIME_DIR`, and cleanup removes the isolated runtime after its servers exit.

The first full run also exposed a lost-step race in the PTY drivers. They could read an empty
step file and acknowledge it before the shell wrote its command. All three drivers now wait for
the terminating newline; a forced empty/partial-write probe failed before and passed after the
change. See the campaign log for the pin reproduction and the full-run retry.

The byte-filename scenario now follows the existing filesystem capability guard used by the cwd
scenario. APFS rejects raw 0xff names before either engine runs, so this box reports
`clean:4/utf8-control` for the existing-file probes and still exercises the raw missing-byte path.
Filesystems that accept those names retain the raw-byte checks.

The full `just compat --strict-geometry --attached-client` retry passed all 221 scenarios / 2,655
steps with the 3 exact registered known rows and attached-client PASS at `f80405390af4`. Workspace
tests, all-feature clippy, and the compatibility checks passed. Protocol stays v98. Delivery is
pending; after these commits reach main, resume cycle 14. Do not repeat the old Escape investigation.

Keep failed fixture cleanup scoped: identify any surviving `/tmp/zza-*.sock` daemon by its exact
socket and PID, then reap that PID. Never use `pgrep` or `pkill -f` with a pattern in your own shell.

### Remaining implementation batches

The review lays them out by scope and what each closes: cycle 14 (the
keys contract: prefix table split, shift modifier, table lifecycle; buffers and VT facts:
`save-buffer -`, the four terminal formats, CLI bytes, `refresh-client -S`), cycle 15 (config
discovery and the pane PATH decision; background status jobs and the control-notify fixture), cycle
16 (the desktop status row and drag-to-CLIPBOARD; the proof debt: per-plugin runtime fixtures, the
census scenarios, the harness holes, slugs for the prose-only divergences). Read the report's
"Next cycles" section before implementing. The groupings express dependencies and ownership;
they do not require separate agents or separate full runs. Reconcile any protocol change across
the completed batch before validation.

### Two things the closing cycles taught

A measurement beats a narrowing. The orchestrator narrowed the geometry item from prose on
2026-09-03 and got the direction wrong; the probe scheduled instead of a decision found `pane_width`
is one of a tiled family, so the PTY follows the layout, not the format the PTY. When an item
resists a decision, write a small probe and measure first.

A gate may spend meter points to stay honest. Cycle 12's gate moved an item out of an accepted
group because its own cycle had falsified the acceptance clause, and said so in its report rather
than banking 99.7%. Keep telling gates that a falsified premise under an accepted group is a
finding. Record every discovered divergence in the registry with a slug and evidence.

### Resuming interrupted work

Inspect the existing worktree, commits, board notes and logs before rebuilding anything. Preserve
completed implementation and verify which checks actually finished. Resume unfinished work;
rerun validation only when it is incomplete, failed, or its inputs changed. An interrupted full
corpus cannot produce a PASS stamp. Record the tested revision and remaining work in the handoff.

## Pages that carry live checkpoint numbers

An audit on 2026-09-03 found 47 stale facts across the bundle, a third of them the SAME checkpoint
paragraph copied into several pages and left five cycles behind. They are hand-written prose, not
generated, so nothing refreshes them: `compat/tmux-tracker.py write-report` regenerates only
`knowledge/tmux/gaps.md`. Fencing them for a generator was considered and rejected: the numbers sit
mid-paragraph in bespoke sentences. Refresh them when updating `TMUX_COMPAT_TRACKER.md` from the
completed batch's registry and validation results.

Present-tense registry, corpus or partition numbers live in `knowledge/tmux/tmux-compat.md`,
`knowledge/tmux/status-line.md`, `knowledge/tmux/key-tables.md`, `knowledge/tmux/copy-mode.md`,
`knowledge/tmux/commands.md`, `knowledge/tmux/divergences.md`,
`knowledge/playbooks/compat-harness.md` and `knowledge/playbooks/tmux-compat-cohorts.md`. Leave
dated historical sentences alone ("at that checkpoint the tracker had ..."): they are correct about
the past and are not drift.

## Coordination

Use the board for claims, ownership and delivery records. Claim only the fronts needed for the
agreed batch, renew their leases while working, and hold MAIN for final integration and publication.
Use TRIAGE when minting or withdrawing fronts. After delivery, verify the remote revision and
record the batch's results, remaining work and released claims. Choose further work from the live
registry and retrospective findings within the user's requested scope.

## Resuming on another machine

1. Clone `git@github.com:demfabris/zz.git` (HTTPS through `gh auth setup-git` where the SSH keys
   are missing). Toolchain per `mise.toml`; the campaign only needs debug builds, `cargo test`, and
   `cargo clippy`.
2. Populate the caches once: `compat/fetch-tmux.sh` builds the pinned tmux, `compat/fetch-corpus.sh`
   clones the plugin corpus. At the start of final validation, check this machine with `formats`:
   `ZZ_COMPAT_TMUX=<checkout>/compat/.cache/tmux-src/tmux ZZ_COMPAT_CORPUS=<checkout>/compat/.cache/plugins compat/run.sh --strict-geometry formats`
   (cold build, several minutes). Populate caches before running concurrent commands against them.
3. Check `gh api user` on this machine and use `gh auth login -h github.com` if needed. Authentication
   on another machine does not authenticate this one. `gh api user` must answer and
   `python3 compat/board.py status` must list the fronts. Pick a holder identity like
   `<host>/<name>` and use it for every board call (`ZZ_BOARD_HOLDER=<host>/<name>`).
4. Identify and protect existing tmux and zz servers. Use short isolated test socket paths under
   `/tmp`; never kill a process by a command-line pattern that matches the invoking shell.
5. Keep the machine awake during final validation when needed, for example
   `caffeinate -is -w <validation-pid>` on macOS.
6. Resume from the repository, board and saved worktree. Prior chat transcripts are optional.

## Machine notes

- **ubuntu box** (8 cores, 30 GB, Ubuntu 26.04, bash 5.3, btrfs): ran cycles 5, 6 (first half)
  and 10. SSH origin works. Worktrees `~/dev/zz-opus-dint`, `~/dev/zz-opus-panes`,
  `~/dev/zz-opus-termopts` are clean and warm at the cycle-10 lane tips (20-30 GB targets each;
  reuse with `git checkout --detach origin/main`); `~/dev/zz-review-client`, `~/dev/zz-review-dint`,
  `~/dev/zz-review-copy` are review scratch; the client gate's `~/dev/zz-gate-client` worktree was
  removed after the merge, and `~/dev/zz-gate-target` (47 GB) is the shared gate
  build dir with a reflinked ghostty source at `zz-gate-target/ghostty-src` for
  `GHOSTTY_SOURCE_DIR`; the queue and copy gate worktrees were removed. Three compat rows are red
  here before any lane and pass `--check-summary` only because the stored summary tolerates them:
  `smoke/remain-on-exit-format` (the fixture wants `term` where Linux answers signal 15),
  `smoke/format-modifier-interrogate` (the harness's outer TERM carries `smxx`),
  `smoke/pane-engine-knobs-input` (pin-side flake under load). The first two need a Linux-aware
  literal or a TERM-scrubbed harness. No user tmux server or zz daemon was running during cycle 10;
  inspect the current processes before testing.
- **macbook** (16 cores, 48 GB, macOS 27): ran cycles 6 (second half) to 9. `origin` is HTTPS
  through gh's credential helper (SSH security keys unavailable since cycle 7). `/bin/bash` is 3.2
  (no `mapfile`; helpers use `/opt/homebrew/bin/bash` or python3). APFS refuses non-UTF-8 file
  names (`smoke/client-non-utf8-cwd` guards for it). The user has a live tmux server (sessions
  `clairvo`, `home`, `zz`) and a live `/Applications/zz.app` daemon that no worker may kill.
  Worktrees `~/dev/zz-opus-dint`, `~/dev/zz-opus-panes`, `~/dev/zz-opus-termopts` are warm at the
  cycle-9 tips. Preserve the HTTPS origin. Use the machine's available resources and cap
  workspace test concurrency at eight threads on this box to avoid timing failures under load.
- Record new machine constraints in the handoff. Keep machine-specific paths and concurrency
  settings outside reusable test logic.

## Board tool quirks

- `--holder` is a global flag (before the subcommand); `ZZ_BOARD_HOLDER` does the same job.
- `release` and `withdraw` require `--reason`; `note` takes `--note`; `candidate` takes `--commit
  --branch --base` plus repeatable `--proof`; `integrated` takes `--merge` and optional `--gate`;
  `front` takes `--contract --zones` plus `--priority --kind {work,lock} --deps --path --notes`.
- `front --priority` takes an INTEGER (`--priority 3`), not the `p3` the status listing prints.
- `renew <FRONT> --lease 2h`: a bare number is silently ignored, always give a unit; the new expiry
  is the comment time plus the lease, so a short renew can shorten a long lease. A renew on a
  front the holder does not hold posts a harmless RENEW comment.
- One zone, one claim, even for the same holder: mint the lock fronts with pairwise-disjoint zones
  and record the actual file ownership. READY fronts whose zones overlap a claimed
  lock read `zones-busy` until the release; that is expected.
- `withdraw` and `front` need TRIAGE held; `integrated`, `repair`, and `rejected` need MAIN held.
  A records-only push (ledger, docs) is ledgered as `integrated MAIN --merge <sha>`.
- A withdrawn front that other fronts depend on reads as `deps-broken` for them: withdraw the
  dependent first, or remint as V(n+1).
- `board.py` stores a front's contract as a free string: when a contract group closes but its slugs
  move to another group, a RESIDUAL redirecting the claim is enough, no remint needed.
- Unknown zone names only warn; `python3 compat/board.py zones` lists the real ones.
- `python3 compat/board_test.py` is part of every gate; it leaves `compat/__pycache__/` behind.

## Historical lessons

Two lessons from the 10b gate, both cheap to avoid and expensive to hit. A review's probes must
live in the repo or on a branch: 10b's reviewer left eight ready-made probe scripts in a session
scratchpad under `/tmp`, the machine move erased them, and the gate spent its first hour rebuilding
every measurement from the review's prose. And a ledger recomputes from the registry, never from a
worker report: the client lane's report claimed the open-item count went 457 to 453 where the
registry said 438 to 435. Before gating a branch that has sat while main moved, run
`git merge-tree --write-tree origin/main <tip>` first; it predicted 10b's single conflict exactly
and costs one command.

Flaky-under-load list (all pass exact-solo): `client_focus_closes_display_panes_and_preserves_chooser_modes`
(also fails about one run in three exact-solo), `event_hooks_fire_after_mutation_with_captured_formats`
(automatic-rename race), `history_request_is_guarded_clamped_and_returns_self_contained_rows`,
copy-mode reconcile tests, `daemon_native_split_resize_commits_exactly_and_rejects_stale_contexts`,
`nested_alias_queue_bubbles_shutdown_and_yield_to_its_parent`,
`control_sourced_run_shell_closes_before_raw_output_and_same_line_continues`,
`request_full_enqueues_only_the_requested_visible_pane`,
`display_menu_resize_lifecycle::a_resize_moves_the_menu_and_keeps_everything_else`,
zz-terminal `pty_output_drains_while_the_input_writer_is_backpressured`,
`wait_exit_holds_the_control_process_until_a_second_blank_line` (hangs under load; timeout-guard
cli_binary runs), `concurrent_default_interactive_attaches_atomically_share_session_zero` (renamed in 96ab56b, so an older prompt's copy of the short name finds nothing; headless "not a
terminal", may be misattributed), `smoke/source-replay-diagnostics` (pin-side crash under
concurrent scenario load; run it solo after sharded gates), `smoke/pane-engine-knobs-input`
(pin-side under shard load), `behavior-options` (one TOPO row under shard load), and
`smoke/client-non-utf8-cwd` on APFS.

Registry grammar: closing = removing the slug from the group's items (an emptied group moves to
`closed[]`); native decisions = relocate the slug into an accepted-native group with the measured
stance (precedent `1f24a1f1`); a promoted flag cannot keep a `flag:` item anywhere (the manifest
test `command_and_flag_gaps_match_the_pinned_oracle` refuses it), so an accepted group whose only
item is a promoted flag closes with the decision trail in its resolution (precedent `7f26fc6`);
park = relocate into a `park`/`blocked` group with the recipe; flag promotions move `catalog.rs`
counters and `compat_manifest_tests.rs` partition counts together (precedent `c6ce82c4`); a wrong
close is reverted and the reason records the refuting measurement (precedents `0fec342` +
`9cab1fa`), or, when the revert would take working code with it, the group is reopened with the
measurement and the fix shape (precedent `cd03bb8`). `cargo test -p zz-mux` is mandatory after any
registry edit, and `cargo test -p zz-daemon --lib` whenever mux target resolution, effect shapes,
or layout change.

Reviewer catches worth remembering: proofs gathered before the final commit are worthless (cycle 4);
durable registry resolutions must carry every divergence the worker discloses; doc comments must
stay attached to their fn; a menu width rule that ignored the title seed (cycle 3); a mode reset
built on DECSTR, which the pinned libghostty ignores entirely (cycle 5); a fixture that configured
away the pin's gating (`focus-events on`) (cycle 5); a hook close built on a mechanism the pin
does not have (the notified client as format client) (cycle 6); bound-key chains preflighting the
invoker's read-only bit instead of the `-c` client's (cycle 6); a popup Kitty close whose fixture
resized the client before every snapshot (cycle 9); an "all branches enumerated" close that the
new search engine made refutable, and prompt bindings that dropped the armed count prefix (cycle
10); an instant-exit `-W` racing its own waiter removal, and a non-zero `-W` status skipping the
after-hook (cycle 10). Gate lore: `compat/run.sh --delta --list` once returned a stale selection,
so list twice and reconcile against `git diff --name-only`; grep the tree for conflict markers
after every rebase; a shard runner under bash 3.2 ran the whole corpus eight times at once; when a
gate rebase conflicts on `compat/tmux-gaps.json`, `compat/orchestration/gaps-merge.py BASE OURS
THEIRS OUT` merges the two lanes' closes by record id and exits 2 on a record both sides changed
differently (feed it `git show :1:compat/tmux-gaps.json`, `:2:`, `:3:`), then regenerate `gaps.md`
and recount `catalog.rs` and `compat_manifest_tests.rs`, which move every cycle; seed a gate's
shared target dir by reflink from a warm worktree and point `GHOSTTY_SOURCE_DIR` at a
same-filesystem copy so libghostty never touches the network.
