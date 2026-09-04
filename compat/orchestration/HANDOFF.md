# Handoff for the tmux-compat campaign (registry closed; next work is in CAMPAIGN-REVIEW.md)

Written 2026-09-03 ~00:40Z on the ubuntu box and rewritten 2026-09-04 after cycle 13 integrated.
THE REGISTRY IS EMPTY: meter 100.0% (304/304), 65/65 groups, 0 open, 0 blocked, 174 closed records,
42 accepted groups. The implementation phase of the campaign as scoped on 2026-08-31 is done.

It is not the end of the work, and the tracker's own headline says why: the persisted attached-client
row reads PASS but the fixture does not complete on this box, so the practical exit gate is not met.
A retrospective run the same day (`CAMPAIGN-REVIEW.md`, eight Fable reviewers and a critic) found
fifteen ranked gaps the registry could never see because its list was drawn after argv parsing and
below the screen. THE NEXT ORCHESTRATOR STARTS THERE: the instrument pass in that report comes
before any cycle 14.

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
| `origin/main` | `0092abf` (cycle-13 ledger recompute; lanes `fd2e790` geometry, `37e8df0` consumers) |
| Agreed-scope meter | 100.0% (304/304 items), 65/65 groups done, 0 partially burned; `python3 compat/progress.py`. Beside it publish the honest denominator the retrospective asks for: 304 of 757 identified items, because the 42 accepted groups hold 453 more |
| Live registry | 0 open groups, 0 blocked, 0 items; 174 closed records, 42 accepted groups |
| Corpus | 220 scenarios / 2,648 steps; the stored attached-client row reads PASS but the fixture does NOT complete on this box (see the retrospective's finding 11 and the instrument pass) |
| `PROTOCOL_VERSION` | 98 (hex hello frame 0x62, test `..._ninety_eight`); the next wire change bumps to 99 (0x63) |
| Unmerged work | None |
| Board (issue #7) | MAIN and TRIAGE free. Every lock front from cycles 10 to 13 is INTEGRATED and released; the `F-SPLIT-MUX-*-V5` chain untouched and never part of this campaign |
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
synchronous with a 2 s cap; a custom key table is never left; the attached fixture is red under a
stored PASS; control-mode notifications were never diffed; `refresh-client -S` errors; the raw TUI
keeps a 29-column sidebar at 80 columns; the CLI output writer changes bytes.

### The instrument pass comes first

Not a cycle: one owner, hours each, no reviewers. Until it is done every attached-only close is
unverified on this box and no status-rendering or entrypoint claim can be proved at all.

1. Fix the attached-client fixture here (`probe_command_output_navigation`, `probe_command_prompt`)
   and make `compat/run.sh --check-summary` refuse a `PASS` footer that carries no commit stamp or
   predates the tip. `run.sh:311` only re-reads the footer today.
2. A launcher mode in `compat/diff-scenario.sh`: the zz side runs the installed layout, no
   `--socket`, `ZZ_SOCKET` unset, a scrubbed PATH with the pin tmux first.
3. A row-level TUI differential: zz-tui below 50 columns or with the sidebar off, `status off` on
   the recorder, diff the last row's `capture-pane -p -e` bytes over a small format corpus.

### Then three two-lane cycles

The review lays them out with branch names, zones, budgets and what each closes: cycle 14 (the
keys contract: prefix table split, shift modifier, table lifecycle; buffers and VT facts:
`save-buffer -`, the four terminal formats, CLI bytes, `refresh-client -S`), cycle 15 (config
discovery and the pane PATH decision; background status jobs and the control-notify fixture), cycle
16 (the desktop status row and drag-to-CLIPBOARD; the proof debt: per-plugin runtime fixtures, the
census scenarios, the harness holes, slugs for the prose-only divergences). Read the report's
"Next cycles" section before writing `opus-compat-run-14.js`; copy `opus-compat-run-13.js` for the
shape. Same rules: two lanes, Opus 5 at xhigh throughout, hard budgets in minutes, the FOREGROUND
rule, exactly one lane owning any protocol bump unless both must, in which case say so and let the
gate reconcile as cycles 10 and 12 did.

### Two things the closing cycles taught

A measurement beats a narrowing. The orchestrator narrowed the geometry item from prose on
2026-09-03 and got the direction wrong; the probe scheduled instead of a decision found `pane_width`
is one of a tiled family, so the PTY follows the layout, not the format the PTY. When an item
resists a decision, write the probe into the prompt and say "measure first".

A gate may spend meter points to stay honest. Cycle 12's gate moved an item out of an accepted
group because its own cycle had falsified the acceptance clause, and said so in its report rather
than banking 99.7%. Keep telling gates that a falsified premise under an accepted group is a
finding, and keep the reviewer rule that a divergence disclosed only in a worker's notes is a
must-fix until it has a slug.

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
