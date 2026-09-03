# Paused handoff for the tmux-compat campaign (after cycle 10)

Written 2026-09-03 ~00:40Z on the ubuntu box. Cycle 10's queue and copy lanes are merged and
ledgered (`origin/main` `2af51ff`, meter 96.1%). The third lane (client: the four popup items) is
reviewed, rebased and pushed but NOT gated: fabrico paused for a machine move while its gate was
twenty minutes in. The board is idle (MAIN and TRIAGE free, no lane locks held), no renewer runs,
and local `main` on this box matches `origin/main` plus the records commit that carries this file.

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
  the corpus (207 scenarios at the pause), `smoke/` the ones with real pty clients.
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
| `origin/main` | `2af51ff` (cycle-10 ledger; lanes `fd19cce` queue, `cd03bb8` copy) plus this records commit |
| Agreed-scope meter | 96.1% (292/304 items), 58/65 groups done, 5 partially burned; `python3 compat/progress.py`. The unmerged client lane holds four more items (`display-popup.behavior-fidelity`), which would make it 97.4% |
| Live registry | 7 open groups, 0 blocked, 12 items (6 groups / 8 items once the client lane lands); 166 closed records, 42 accepted groups |
| Corpus | 207 scenarios / 2,586 steps, attached-client PASS (recorded on this box) |
| `PROTOCOL_VERSION` | 96 (hex hello frame 0x60, test `..._ninety_six`), two v96 changelog bullets on main (`CommandQueueParked`, `CopyModeAction::Search`); the client lane adds a third v96 bullet (`PopupAction::Pointer`); the next wire change AFTER that bumps to 97 (0x61) |
| Unmerged work | `campaign/batch-client-choosers-popups-opus` (`ff66ddc`, the worker's tip) and `campaign/batch-client-choosers-popups-opus-gated` (`affcfc9`, the same four commits rebased onto `2af51ff`, no fixes applied). Review: approve-with-fixes, six must-fixes, embedded in `opus-compat-run-10b.js` |
| Board (issue #7) | MAIN and TRIAGE free. `F-PANE-COMMAND-COMPLETION` and `F-COPY-SEARCH-FORMATS-MONITORS` INTEGRATED; `F-CLIENT-CHOOSERS-POPUPS-V2` released with the pause note (reads READY); the `F-SPLIT-MUX-*-V5` chain untouched |
| Remotes | SSH (`git@github.com:demfabris/zz.git`) works on the ubuntu box; the macbook was switched to HTTPS through gh's credential helper in cycle 7 |

## First task on the next machine: land the client lane

Nothing needs rewriting. After the machine checklist below:

```js
Workflow({
  scriptPath: "<checkout>/compat/orchestration/opus-compat-run-10b.js",
  args: { stage: "gate", root: "<checkout>", dev: "<dir holding the checkout>", holder: "<host>/orchestrator",
          machine: "...", cores: N, workerJobs: ..., workerThreads: ..., gateJobs: ..., gateThreads: ..., shards: ...,
          protected: "...", bash: "/bin/bash", boxNote: "...", gitNote: "..." }
})
```

`stage: "gate"` skips the review agent and uses the embedded verdict; the gate fetches the
`-gated` branch, rebases it onto the current `origin/main`, applies the six must-fixes with the
reviewer's probes re-run, runs the workspace gate and the sharded delta corpus, pushes main,
ledgers `F-CLIENT-CHOOSERS-POPUPS-V2` (claim it under your holder first, `--lease 3h`), and adds
the client merge to the cycle-10 checkpoint table in `TMUX_COMPAT_TRACKER.md`. Omit the machine
args on the ubuntu box. About an hour; one Fable agent.

## What is left after that, and what a cycle 11 could take

The census is `compat/tmux-gaps.json` gaps with status open (nothing is blocked); every reason
carries the measurement and the recipe. Eight items in six groups once the client lane lands:

| Group | Items | Standing |
| --- | --- | --- |
| `choosers.command-flags` | 2 | The vocabulary (17 keys) plus `-y`. Cycle 10 found the prerequisite: the pin's `x`/`X`/`:` prompts belong to the mode overlay (`mode_tree_set_prompt`), and zz has no overlay-owned prompt; the popup context menu built in cycle 10 (a `MenuSession` marked `popup_owner`, inserted by the overlay) is the shape to copy. Four sizing corrections are in the reason (kill prompts carry their target, `-y` only answers `x`/`X`, `O` steps a per-mode order sequence, the buffer chooser adds `e`/`d`/`D`/`P`) |
| `copy-mode.action-fidelity` | 1 | Reopened at the cycle-10 gate: three mode-keys reads the eleven-place enumeration missed (the per-command search-mark clear class, 51 ALWAYS / 36 EMACS_ONLY / 7 NEVER; the incremental-origin re-latch; the emacs copy-selection trim) plus `previous_word`'s `stop_at_eol`. Fix shape in the reason: a per-action clear class on `CopyModeAction` applied against the live `mode_keys_vi` knob, and an emacs branch in `format_selection` |
| `display-message.verbose-trace` | 2 | `-v` and the trace; the full line grammar and two structural mismatches are in the reason (modifier-argument expansion must move into parsing; no `format_check_time`). Rebase on the merged copy lane, which touched `Expander::lookup` |
| `formats.context-producer-fidelity` | 1 | The `set-hook -B` monitor subsystem; the reason holds the complete measured shape (parse rules, one-second tick, baseline-then-fire, nine names via `hook_format_variables`) |
| `clients.path-encoding` | 1 | Environment bytes; priced in the reason at four channels (hello entry, `CommandInvocation.args` read as `&str` in 99 places behind 65 signatures in the mux, the environment store, `CommandResponse::Success.output`); the probes in the reason are the acceptance test. Half-landing was refused twice; it is one honest lane on its own |
| `rendering.geometry-residue` | 1 | GUI-only. Cycle 10 refuted the recipe: the gpui harness exists (`crates/zz/src/workspace/view.rs` drives `TerminalView::update_geometry` under `#[gpui::test]`), and the writeback is not the problem; what is needed is a product decision about which extent `window_width`/`window_height` report for a client that draws chrome inside the window it reports. The pure measurement `terminal_grid_size` with tests is on the client branch |

A cycle 11 with three lanes (choosers on the overlay-owned prompt; the mode-keys tail plus the
monitors plus the `-v` trace; the environment bytes) could close seven, leaving only the GUI
extent decision, which the orchestrator should take (record the stance, close or relocate the
item) rather than schedule. Give every group a HARD budget this time, foundation groups included:
the cycle-10 queue lane ran 4h15m on an open budget and made the whole cycle six hours. Put the
`FOREGROUND` rule from `opus-compat-run-10b.js` into every worker, reviewer and gate prompt: a
reviewer in cycle 10 ended its turn waiting on a background monitor and the pipeline dropped the
lane. Protocol: 96 to 97 (0x60 to 0x61, `..._ninety_seven`).

Launch shape (unchanged since cycle 6): write `opus-compat-run-11.js` from `opus-compat-run-10.js`
(same `M` block with the `boxNote`/`gitNote` defaults, three new lane batches, new lock names),
mint the lock fronts under TRIAGE with pairwise-disjoint zones, commit the records under MAIN,
claim the fronts (`--lease 6h`), launch with the machine facts as `args` (omit them on the ubuntu
box), and run a lease renewer (a loop renewing the fronts and MAIN with `--lease 6h` every ~100
minutes; kill it by pid, never by a pattern that matches your own shell). Cycle 10 took 5h54m and
2.43M subagent tokens for the main run plus 0.23M for the client review.

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
7. First task: the client gate above. Then the census.

## Machine notes

- **ubuntu box** (8 cores, 30 GB, Ubuntu 26.04, bash 5.3, btrfs): ran cycles 5, 6 (first half)
  and 10. SSH origin works. Worktrees `~/dev/zz-opus-dint`, `~/dev/zz-opus-panes`,
  `~/dev/zz-opus-termopts` are clean and warm at the cycle-10 lane tips (20-30 GB targets each;
  reuse with `git checkout --detach origin/main`); `~/dev/zz-review-client`, `~/dev/zz-review-dint`,
  `~/dev/zz-review-copy` are review scratch; `~/dev/zz-gate-client` holds the rebased client tip
  (`affcfc9`, same as the `-gated` branch) and `~/dev/zz-gate-target` (47 GB) is the shared gate
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
cli_binary runs), `concurrent_default_interactive_attaches_share_session_zero` (headless "not a
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
