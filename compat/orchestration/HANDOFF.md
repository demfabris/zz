# Handoff for the tmux-compat campaign (cycle 12 is the last one)

Written 2026-09-03 ~00:40Z on the ubuntu box and rewritten 2026-09-04 after cycle 11 integrated.
Cycles 10 and 11 are both fully integrated. Meter 99.0%, THREE items left, all three in their own
group. One more cycle closes the registry.

THE CYCLE SHAPE CHANGED ON 2026-09-03 and this overrides every older script. Fabrico's instruction
is TWO work lanes per cycle, not three, and every agent is Opus 5 at `effort: 'xhigh'`: workers,
reviewers and the gate alike. The Fable reviewers and Fable gates of cycles 6 to 10 are history.
Copy `opus-compat-run-11.js`, not `opus-compat-run-10.js`.

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
  the corpus (212 scenarios after cycle 10), `smoke/` the ones with real pty clients.
- **The registry**, `compat/tmux-gaps.json`, lists every known difference in groups, each item with
  the exact proof that closes it. Closing an item means removing it after that proof; a difference
  we keep on purpose is relocated into an accepted group with the measured tmux behaviour and the
  reason. `knowledge/tmux/gaps.md` is generated from it (`compat/tmux-tracker.py write-report`).
- **The meter**, `compat/progress.py`, scores the registry against a list of 304 items frozen on
  2026-08-31 (`compat/progress-baseline.json`), so the percentage cannot be gamed by moving goalposts.
- **The board**, GitHub issue #7 driven by `compat/board.py`, is the work ledger: fronts are
  minted, claimed, gated and integrated as comments, with zone locks so parallel agents do not
  collide. `TMUX_COMPAT_TRACKER.md` at the repo root is the human-readable checkpoint.
- **The cycles**: each one runs three implementor agents in parallel worktrees, one adversarial
  reviewer per lane that tries to disprove the closes against the pin, and one gate that rebases,
  runs the workspace tests, clippy and the differential corpus, pushes main and ledgers the board.
  An orchestrator session writes each cycle's script from a fresh census. `CAMPAIGN-LOG.md` has
  the history; the `opus-compat-run-N.js` files beside it are the scripts.

What gets compared is the daemon, not the screen. Both binaries receive the same commands and
their observable answers are diffed (`list-panes`, `capture-pane`, `display-message` formats,
hooks, options, key tables, copy-mode state, control-mode output). The zz raw TUI draws a sidebar
and chrome rows around a pane, so it is never cell-compared to tmux's 80x24; it is used as a real
pty client to drive copy mode, prompts, choosers, mouse and paste, after which daemon facts are
read. The desktop GPUI app keeps its own look entirely.

## State now

| Fact | Value |
| --- | --- |
| `origin/main` | `3eda6ed` (cycle-11 choosers merge; formats lane `89f36ac`) plus the ledger recompute |
| Agreed-scope meter | 99.0% (301/304 items), 62/65 groups done, 3 partially burned; `python3 compat/progress.py` |
| Live registry | 3 open groups, 0 blocked, 3 items; 170 closed records, 42 accepted groups |
| Corpus | 215 scenarios / 2,636 steps, attached-client PASS (recorded on this box) |
| `PROTOCOL_VERSION` | 97 (hex hello frame 0x61, test `..._ninety_seven`); the next wire change bumps to 98 (0x62) |
| Unmerged work | None. `campaign/batch-choosers-copy-tail-opus-gated` (`3eda6ed`) is the tip that landed, pushed before the by-hand finish as insurance; it is history now |
| Board (issue #7) | MAIN and TRIAGE free. Every cycle-10 and cycle-11 lock front is INTEGRATED and released; the `F-SPLIT-MUX-*-V5` chain untouched |
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

## What is left: three items, one cycle

Cycle 11 integrated on 2026-09-04. The formats lane (`89f36ac`) closed
`formats.context-producer-fidelity` and `display-message.verbose-trace`; the choosers lane
(`3eda6ed`) closed `copy-mode.action-fidelity` and `flag:choose-tree:-y`, bumped the protocol to 97,
and landed the first five chooser keys on a real overlay-owned prompt. Both lanes finished inside
their hard budgets, which is the rule to keep.

| Group | Items | Standing |
| --- | --- | --- |
| `choosers.command-flags` | 1 | `semantic:chooser-key-vocabulary`. The prerequisite is DONE: the overlay-owned prompt exists and `t`, `T`, `C-t`, `x` and `X` are proved against the pin. What is left is the rest of `mode_tree_key`, sized in the group reason by the worker that stopped at its budget: `K`/`J`/`S-Up`/`S-Down` (`mode_tree_swap`), `O` and `r` (the per-mode order sequences and `sort_next_order`'s wrap), `F1`/`C-h` (the help screen the next key of any kind closes), `M--` and `M-+` (collapse or expand every top-level item, not the current one), the `m` mark, the `:` prompt (`(%u tagged) ` or `(current) `, which `-y` never answers because it does not pass `PROMPT_SINGLE`), and `window_buffer_key`'s `e`, `d`, `D` and `P`. Mechanical rather than exploratory now |
| `clients.path-encoding` | 1 | `semantic:client-environment-non-utf8`. Untouched and deliberately so: the reason prices the byte across four channels (the hello entry, `CommandInvocation.args` read as `&str` in 99 places behind 65 signatures in the mux alone, the environment store, `CommandResponse::Success.output`), and half-landing was refused twice because channels (i) and (iii) alone change nothing observable. One lane on its own. The probes in the reason are the acceptance test |
| `rendering.geometry-residue` | 1 | `semantic:attached-gui-pane-width`. Narrowed by the orchestrator on 2026-09-03 rather than closed blind, see below |

### The geometry decision, narrowed

The parked question was "which extent do the window formats report for a client that draws chrome".
Two of the three options are refuted by measurement already recorded in the group reason: carving
the PTY from the client's whole extent oversizes and clips the pane, and carving the window extent
from the drawn grid changes what `window_width` reports and breaks fixture rows derived from the
pin, which would be re-scoping a contract to match an implementation. So `window_width` and
`window_height` STAY the client's whole window, the way tmux's own `window_width` is the client's
tty.

What was never measured is the third option, and it reframes the item. The divergence is that an
Interactive client under latest, largest or smallest has its own per-pane report reach the PTY while
the format reports the engine's allocation. The question is therefore not which WINDOW extent to
report but which of the two PANE numbers is observable truth. tmux has no such split, because
`window_pane_resize` moves the screen and the PTY together, which argues the format should follow
the PTY. The probe: drive `TerminalView::update_geometry` in the `#[gpui::test]` harness that
already exists in `crates/zz/src/workspace/view.rs`, with a pixel box that is not a whole number of
cells, and compare `#{pane_width}` against the columns the PTY actually got. The pure measurement
`terminal_grid_size` in `crates/zz/src/terminal/element.rs` is already on main with unit tests.
Schedule that; do not decide it from prose.

### Cycle 12 shape

Two lanes, Opus 5 at xhigh, hard budgets, exactly one lane owning any protocol bump (97 to 98,
0x61 to 0x62, `..._ninety_eight`):

- Lane A: `clients.path-encoding` alone, the whole cycle if it needs it.
- Lane B: the chooser vocabulary tail plus the geometry probe above. Both are bounded and named.

That closes the registry. When it does, the campaign's exit is the meter at 304/304 with the three
accepted-native decisions standing, and the `F-SPLIT-MUX-*-V5` chain is the only board work left.

Launch shape: copy `opus-compat-run-11.js` (its `M` block carries the `boxNote`/`gitNote`
defaults), write the new lane batches and lock names, mint the lock fronts under TRIAGE with
pairwise-disjoint zones, commit the records under MAIN, claim the fronts (`--lease 6h`), and launch
with the machine facts as `args` (omit them on the ubuntu box). A 6h lease covers a two-lane cycle,
so no renewer process is needed; renew by hand at a check-in instead, which also removes the
`pkill -f` hazard that killed the orchestrator's own tool shell twice.

### If the gate dies mid-cycle

It happened in cycle 11: the gate agent hit a network timeout at 20:17Z on 2026-09-03 while
sharding the second lane's corpus, after it had already merged the first lane, rebased the second
onto main, applied both must-fixes and cleared the workspace suite and clippy. Recovery took about
an hour and rebuilt nothing. The order that worked, and the order to repeat:

1. Read `origin/main` and every `zz-gate-*` worktree before touching anything. The worktree holds
   the gate's fix commits and they are usually the expensive part.
2. Push the rebased tip as `campaign/<name>-gated` FIRST, as insurance, before running anything.
3. Verify the reviewer's must-fixes are actually present at that tip. The gate never reported, so
   its claims do not exist; check the code and the registry yourself.
4. Re-run the stages the gate had not finished, plus any stage whose inputs changed after it ran
   (cycle 11's clippy predated two fix commits, so clippy was re-run).
5. Then push, ledger, recompute, release, exactly as the gate's own steps say.

## Pages that carry live checkpoint numbers

An audit on 2026-09-03 found 47 stale facts across the bundle, a third of them the SAME checkpoint
paragraph copied into several pages and left five cycles behind. They are hand-written prose, not
generated, so nothing refreshes them: `compat/tmux-tracker.py write-report` regenerates only
`knowledge/tmux/gaps.md`. Fencing them for a generator was considered and rejected: the numbers sit
mid-paragraph in bespoke sentences, so a fence would mean restructuring eight pages to suit a tool,
and the campaign has one or two cycles left to amortise it. The cheap fix instead: the GATE already
recomputes `TMUX_COMPAT_TRACKER.md` from the merged registry in its step 7, so give it this list
and let it refresh these in the same pass.

Present-tense registry, corpus or partition numbers live in `knowledge/tmux/tmux-compat.md`,
`knowledge/tmux/status-line.md`, `knowledge/tmux/key-tables.md`, `knowledge/tmux/copy-mode.md`,
`knowledge/tmux/commands.md`, `knowledge/tmux/divergences.md`,
`knowledge/playbooks/compat-harness.md` and `knowledge/playbooks/tmux-compat-cohorts.md`. Leave
dated historical sentences alone ("at that checkpoint the tracker had ..."): they are correct about
the past and are not drift.

## The cycle, in general

If a run dies mid-way in the SAME session, `Workflow({scriptPath, resumeFromRunId})` replays
finished agents from the journal cache (`subagents/workflows/<runId>/journal.jsonl`, result key
`result`); across machines or sessions, export the cached reports into the script as constants the
way `opus-compat-run-6-continue.js` and `opus-compat-run-10b.js` do, and skip the agents they
replace with a stage switch in `args`. If the gate died after pushing some lanes, finishing the
rest by hand is often cheaper: the gate's fix commits sit in its `zz-gate-*` worktrees, and a
rebased tip should be pushed as `campaign/<name>-gated` before anything is removed.

Orchestrator loop per cycle: claim the three lock fronts under your holder identity (6h leases,
renew them and MAIN while the gate runs), launch, verify `origin/main`, the board records, and
`compat/progress.py` when the gate finishes, then write the next script from a fresh registry census
(protocol version, lock names, group lists, the mooted fronts for TRIAGE; the census is
`compat/tmux-gaps.json` gaps with status open or blocked, read every reason), mint the next lock
fronts under TRIAGE, commit the orchestration records under MAIN (a records-only push is ledgered as
`integrated MAIN --merge <sha>`), and repeat.

## Resuming on another machine

1. Clone `git@github.com:demfabris/zz.git` (HTTPS through `gh auth setup-git` where the SSH keys
   are missing). Toolchain per `mise.toml`; the campaign only needs debug builds, `cargo test`, and
   `cargo clippy`.
2. Populate the caches once: `compat/fetch-tmux.sh` builds the pinned tmux, `compat/fetch-corpus.sh`
   clones the plugin corpus, and any scenario through `compat/run.sh` triggers both. The readiness
   check is the `formats` scenario running clean:
   `ZZ_COMPAT_TMUX=<checkout>/compat/.cache/tmux-src/tmux ZZ_COMPAT_CORPUS=<checkout>/compat/.cache/plugins compat/run.sh --strict-geometry formats`
   (cold build, several minutes). Those two variables are preset in every prompt so sharded runs
   never race the clone.
3. `gh auth login -h github.com --insecure-storage` IN A SHELL ON THAT MACHINE (a login run on
   another machine authenticates that machine's gh; over SSH, `--insecure-storage` keeps the token
   in `hosts.yml` where a non-desktop shell finds it). `gh api user` must answer and
   `python3 compat/board.py status` must list the fronts. Pick a holder identity like
   `<host>/orchestrator` and use it for every board call (`ZZ_BOARD_HOLDER=<host>/orchestrator`).
4. Claude Code settings for an unattended run, in `~/.claude/settings.json`: the allow rule
   `"Bash(rm:*)"` under `permissions.allow`, and a PreToolUse hook on `Bash` running
   `python3 <checkout>/compat/orchestration/guard-rm-home.py` with a 10 s timeout. The hook denies
   the `rm -rf $HOME` shape with a rewrite hint, because Claude Code's built-in critical-path guard
   prompts on it and no allow rule bypasses that. Open `/hooks` once after editing so the session
   reloads the file.
5. Keep the machine awake for the gate: `caffeinate -is -w <claude pid>` on macOS; on Linux check
   `gsettings get org.gnome.settings-daemon.plugins.power sleep-inactive-ac-type` is `nothing`
   (`systemd-inhibit` from a non-tty shell did not stay up on the ubuntu box).
6. Optional: carry the orchestrator's memory over as described under "Moving the Claude Code
   session itself". The repository and the board are the durable state.
7. First task: the census below; the client gate that used to stand here is done.

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
  literal or a TERM-scrubbed harness. The allow rule and the hook (pointing at the checkout's
  `guard-rm-home.py`) are in place. No user tmux server or zz daemon was running during cycle 10.
- **macbook** (16 cores, 48 GB, macOS 27): ran cycles 6 (second half) to 9. `origin` is HTTPS
  through gh's credential helper (SSH security keys unavailable since cycle 7). `/bin/bash` is 3.2
  (no `mapfile`; helpers use `/opt/homebrew/bin/bash` or python3). APFS refuses non-UTF-8 file
  names (`smoke/client-non-utf8-cwd` guards for it). The user has a live tmux server (sessions
  `clairvo`, `home`, `zz`) and a live `/Applications/zz.app` daemon that no worker may kill.
  Worktrees `~/dev/zz-opus-dint`, `~/dev/zz-opus-panes`, `~/dev/zz-opus-termopts` are warm at the
  cycle-9 tips. The `boxNote` and `gitNote` args for it: bash 3.2 and APFS traps, HTTPS origin,
  never switch to SSH. Use `workerJobs: 8, workerThreads: 4, gateJobs: 16, gateThreads: 8, shards: 8`.
- **A worker's prompt** carries the box traps through `M.boxNote` and `M.gitNote`; a new machine
  with a new trap gets it in `args`, not in the script.

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
  and let the prompts carry the real file ownership. READY fronts whose zones overlap a claimed
  lock read `zones-busy` until the release; that is expected.
- `withdraw` and `front` need TRIAGE held; `integrated`, `repair`, and `rejected` need MAIN held.
  A records-only push (ledger, docs) is ledgered as `integrated MAIN --merge <sha>`.
- A withdrawn front that other fronts depend on reads as `deps-broken` for them: withdraw the
  dependent first, or remint as V(n+1).
- `board.py` stores a front's contract as a free string: when a contract group closes but its slugs
  move to another group, a RESIDUAL redirecting the claim is enough, no remint needed.
- Unknown zone names only warn; `python3 compat/board.py zones` lists the real ones.
- `python3 compat/board_test.py` is part of every gate; it leaves `compat/__pycache__/` behind.

## Moving the Claude Code session itself

The repository and the board are the durable state. If you also want the old session's transcript
and memory, Claude Code keeps them under `~/.claude/projects/<checkout path with slashes as
dashes>/`: the session's `.jsonl`, a same-named directory with subagent transcripts and workflow
journals, and `memory/` with the project's auto-memory. Copy them into the matching project
directory on the new machine (clone at the same absolute path or rename the directory to match),
then `claude --resume <session-id>` from that checkout. Workflow journal replay depends on the old
scratchpad paths, so launch the next script fresh rather than resuming an old run; the 10b script
already carries everything the client gate needs.

## Lore the prompts already encode

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
