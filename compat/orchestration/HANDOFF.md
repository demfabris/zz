# Paused handoff for the tmux-compat campaign (after cycle 9)

> 2026-09-02 ~18:30Z: RESUMED on the ubuntu box. Cycle 10 is running from
> `opus-compat-run-10.js` under holder `ubuntu/orchestrator`; the census below is the one it was
> written from. `CAMPAIGN-LOG.md` has the launch note; this file is rewritten at the next pause.

Written 2026-09-02 ~16:00Z on the macbook. Cycle 9 is DONE, all three lanes merged, and fabrico
asked for a pause: nothing is minted, claimed, or launched. The board is idle (MAIN and TRIAGE
free, no leases held by `macbook/orchestrator`), the lease renewer and `caffeinate` are stopped,
and local `main` matches `origin/main`.

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
  the corpus (199 scenarios at the pause), `smoke/` the ones with real pty clients.
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
| `origin/main` | `3726adf8` (cycle-9 ledger; lanes `5e6de65a` daemon, `4cb913dc` terminal, `6f307120` client) plus this records commit |
| Agreed-scope meter | 90.5% (275/304 items), 55/65 groups done, 5 partially burned; `python3 compat/progress.py` |
| Live registry | 10 open groups, 0 blocked, 30 items (29 inside the frozen scope, 1 added since); 162 closed records, 43 accepted groups; ledger settlement 95.3% (205/215) |
| Corpus | 199 scenarios / 2,527 steps, attached-client PASS, digest `385ca4f5` |
| `PROTOCOL_VERSION` | 95 (hex hello frame 0x5F, test `..._ninety_five`); the next wire-reachable change bumps to 96 (0x60) |
| Remote | `origin` is HTTPS (`https://github.com/demfabris/zz.git`, gh credential helper) since the SSH security keys became unavailable mid-cycle-7; switch back with `git remote set-url origin git@github.com:demfabris/zz.git` once the YubiKey is back |
| Unmerged work | none. `campaign/batch-terminal-knobs-opus-gated` (`c328ebd3`) is superseded by the cycle-9 re-land and can be deleted with the other stale `campaign/*` branches |
| Board (issue #7) | MAIN and TRIAGE free. READY: `F-PANE-COMMAND-COMPLETION` (design front for `split-window -W`) and the `F-SPLIT-MUX-*-V5` chain. Cycle-9 fronts INTEGRATED; `F-KEY-CONTROL-V3`, `F-PANE-BORDER-LINES-TILED`, `F-PANE-BORDER-ZORDER` withdrawn as mooted |

## What is left, and what a cycle 10 could take

The census below is `compat/tmux-gaps.json` gaps with status open (there are no blocked groups
left); every reason carries the measurement and the recipe for the next attempt.

| Group | Items | Standing |
| --- | --- | --- |
| `keys.copy-mode-binding-fidelity` | 16 | 14 wait on the accepted `command-prompt -P` decision, `#`/`*` on the search action family. Closes only if that decision changes |
| `display-popup.behavior-fidelity` | 4 | `kitty-images` has an exact recipe (a per-view viewport in `zz-terminal` so `publish_popup_terminal` carries placements without a resize, proved by a no-resize fixture comparing image ids; residual 5512260474). `border-drag`, `context-menu`, `to-pane` wait on a menu over a popup |
| `choosers.command-flags` | 2 | `choose-tree -y` and the mode-tree key vocabulary; the client lane wrote down what the vocabulary needs (`78f9acd2`) |
| `display-message.verbose-trace` | 2 | `-v` and the format trace; needs modifier arguments expanded at parse time |
| `clients.path-encoding` | 1 | `client-environment-non-utf8`; the daemon lane wrote down where the bytes are lost (`dca1f47f`) |
| `copy-mode.action-fidelity` | 1 | logical-line and mode-keys; blocked on cursor geometry (zz's cursor-right never wraps), recorded at `a25ad0a0` |
| `formats.context-producer-fidelity` | 1 | `notify_monitor_cb` only; the monitor subsystem record is `7919994e` |
| `control-mode.disconnect-cancels-command-queue` | 1 | design front; bounced three times, needs per-connection worker infrastructure |
| `pane.command-completion` | 1 | `split-window -W`; design front `F-PANE-COMMAND-COMPLETION` is READY |
| `rendering.geometry-residue` | 1 | `attached-gui-pane-width`, GUI-only; needs a way to drive the desktop client's measurement under test |

A cycle 10 with three lanes (popup kitty + choosers, `-v` trace + environment bytes + context
producer, the popup pointer trio behind a menu-over-popup) could close about nine items and land
the meter near 93%. The other twenty items do not move without a product decision (the `-P`
stance), a design front, or a GUI test harness, so the campaign is at the point where the next
step is fabrico's call rather than another autonomous cycle. To run one: write
`opus-compat-run-10.js` from `opus-compat-run-9.js` (same `M` block, three new lane batches, lock
names, protocol 95 to 96), mint the lock fronts under TRIAGE, commit the records under MAIN, claim
the fronts, launch with the `args` below, and run a lease renewer.

Launch shape (unchanged since cycle 6):

```js
Workflow({
  scriptPath: "<checkout>/compat/orchestration/opus-compat-run-<N>.js",
  args: {
    root: "/Users/demfabris/dev/zz", dev: "/Users/demfabris/dev", holder: "macbook/orchestrator",
    machine: "16-core, 48 GB macOS box (macbook)", cores: 16,
    workerJobs: 8, workerThreads: 4, gateJobs: 16, gateThreads: 8, shards: 8,
    protected: "the user's default tmux server (sessions clairvo, home, zz) and the /Applications/zz.app daemon on the default socket",
    bash: "/opt/homebrew/bin/bash"
  }
})
```

Omit `args` on this macbook. Before launching: claim the lock fronts under your holder identity
(`ZZ_BOARD_HOLDER=<holder> python3 compat/board.py claim <FRONT> --lease 6h`), check
`origin/main`, and start a lease renewer (a loop renewing the fronts and MAIN with `--lease 6h`
every ~100 minutes). Cycles 7 to 9 took about four hours and 2.6-2.8M subagent tokens each. After
the gate finishes, verify `origin/main`, the board records, and `compat/progress.py`, then write
the next cycle from a fresh registry census as described under the loop below. `CAMPAIGN-LOG.md`
has the per-cycle history.

## The cycle, in general

If a run dies mid-way in the SAME session, `Workflow({scriptPath, resumeFromRunId})` replays
finished agents from the journal cache (`subagents/workflows/<runId>/journal.jsonl`, result key
`result`); across machines or sessions, export the cached reports into a continuation script the
way `opus-compat-run-6-continue.js` did (it embeds worker and review reports as constants and skips
the agents they replace). If the gate died after pushing some lanes, finishing the rest by hand is
often cheaper: the gate's fix commits sit in its `zz-gate-*` worktrees.

Orchestrator loop per cycle: claim the three lock fronts under your holder identity (6h leases,
renew them and MAIN while the gate runs), launch, verify `origin/main`, the board records, and
`compat/progress.py` when the gate finishes, then write the next script from a fresh registry census
(protocol version, lock names, group lists, the mooted fronts for TRIAGE; the census is
`compat/tmux-gaps.json` gaps with status open or blocked, read every reason), mint the next lock
fronts under TRIAGE, commit the orchestration records under MAIN (a records-only push is ledgered as
`integrated MAIN --merge <sha>`), and repeat.

## Resuming on another machine

1. Clone `https://github.com/demfabris/zz.git` (SSH is fine wherever the keys work; the macbook
   sits on HTTPS only because its security keys went away). Toolchain per `mise.toml`; the campaign
   only needs debug builds, `cargo test`, and `cargo clippy`.
2. Populate the caches once: `compat/fetch-tmux.sh` builds the pinned tmux, `compat/fetch-corpus.sh`
   clones the plugin corpus, and any scenario through `compat/run.sh` triggers both. The readiness
   check is the `formats` scenario running clean:
   `ZZ_COMPAT_TMUX=<checkout>/compat/.cache/tmux-src/tmux ZZ_COMPAT_CORPUS=<checkout>/compat/.cache/plugins compat/run.sh --strict-geometry formats`
   (cold build, several minutes). Those two variables are preset in every prompt so sharded runs
   never race the clone.
3. `gh auth login` with repo scope; `python3 compat/board.py status` must list the fronts. Pick a
   holder identity like `<host>/orchestrator` and use it for every board call
   (`ZZ_BOARD_HOLDER=<host>/orchestrator`).
4. Claude Code settings for an unattended run, in `~/.claude/settings.json`: the allow rule
   `"Bash(rm:*)"` under `permissions.allow` (the auto-mode classifier prompts on `rm -rf` otherwise),
   and a PreToolUse hook on `Bash` running `python3 <checkout>/compat/orchestration/guard-rm-home.py`
   with a 10 s timeout. The hook denies the `rm -rf $HOME` shape with a rewrite hint, because
   Claude Code's built-in critical-path guard prompts on it and no allow rule bypasses that.
   Open `/hooks` once after editing so the session reloads the file.
5. Keep the machine awake for the four-hour gate: `caffeinate -is -w <claude pid>` on macOS,
   `systemd-inhibit` or suspend disabled on Linux. A sleeping machine kills the gate agent
   ("computer went to sleep"); the journal cache then lets `Workflow({scriptPath, resumeFromRunId})`
   replay finished agents in the same session.
6. Optional: carry the orchestrator's memory over as described under "Moving the Claude Code
   session itself". The repository and the board are the durable state; the memory only saves
   re-reading `CAMPAIGN-LOG.md`.
7. First task: read the census above and decide. Either write `opus-compat-run-10.js` from
   `opus-compat-run-9.js` (same `M` block, three new lane batches, new lock names, protocol 95 to
   96), mint the lock fronts under TRIAGE, commit the records under MAIN, claim, launch with the
   machine facts as `args`, and run a lease renewer; or settle the twenty stuck items first (the
   `-P` stance, the two design fronts, the GUI harness). Nothing in the scripts needs editing for
   a new machine: `args` carries the paths, holder, core etiquette, shard count, the
   protected-server sentence, and the bash for helpers. On Linux pass `bash: "/bin/bash"`.

## Machine notes

- **macbook** (16 cores, 48 GB, macOS 27): `origin` is HTTPS through gh's credential helper.
  `/bin/bash` is 3.2 (no `mapfile`; helpers use `/opt/homebrew/bin/bash` or python3). APFS refuses
  non-UTF-8 file names (`smoke/client-non-utf8-cwd` guards for it). The user has a live tmux server
  (sessions `clairvo`, `home`, `zz`) and a live `/Applications/zz.app` daemon that no worker may
  kill. Caches populated (`d77c9dc6`, eight plugins). Worktrees `~/dev/zz-opus-dint`,
  `~/dev/zz-opus-panes`, `~/dev/zz-opus-termopts` are clean and warm at the cycle-9 lane tips
  (`4858798a`, `f1dfad97`, `399629e9`); reuse with `git checkout --detach origin/main`. The gate's
  `zz-gate-*` worktrees and the shared `zz-gate-target` build dir are gone. The allow rule and the
  hook (pointing at `~/.claude/scripts/guard-rm-home.py`, same file) are in place; `caffeinate`
  was stopped at the pause.
- **ubuntu box**: ran cycles 5 and 6. Still holds the cycle-6 worktrees `zz-opus-panes`,
  `zz-opus-dint`, `zz-review-dint` with 15-29 GB `target/` dirs; reuse a worktree if
  `git status --short` is empty, otherwise remove it. Needs the settings from step 4 before an
  unattended run; the SSH remote worked there.
- **A worker's prompt** already carries the traps above for both boxes; a new machine with a new
  trap gets it added to `COMMON` in the next script.

## Board tool quirks

- `--holder` is a global flag (before the subcommand); `ZZ_BOARD_HOLDER` does the same job.
- `release` and `withdraw` require `--reason`; `note` takes `--note`; `candidate` takes `--commit
  --branch --base` plus repeatable `--proof`; `integrated` takes `--merge` and optional `--gate`;
  `front` takes `--contract --zones` plus `--priority --kind {work,lock} --deps --path --notes`.
- `renew <FRONT> --lease 2h`: a bare number is silently ignored, always give a unit; the new expiry
  is the comment time plus the lease, so a short renew can shorten a long lease.
- One zone, one claim, even for the same holder: claim one lock front per lane and let the gate
  ledger the fronts it moots. READY fronts whose zones overlap a claimed lock read `zones-busy`
  until the release; that is expected.
- `withdraw` and `front` need TRIAGE held; `integrated`, `repair`, and `rejected` need MAIN held.
  A records-only push (ledger, docs) is ledgered as `integrated MAIN --merge <sha>`.
- A withdrawn front that other fronts depend on reads as `deps-broken` for them: withdraw the
  dependent first (`F-PANE-BORDER-LINES-TILED` before `F-PANE-BORDER-ZORDER`), or remint as V(n+1).
- `board.py` stores a front's contract as a free string: when a contract group closes but its slugs
  move to another group, a RESIDUAL redirecting the claim is enough, no remint needed.
- Unknown zone names only warn; `python3 compat/board.py zones` lists the real ones.
- `python3 compat/board_test.py` is part of every gate.

## Moving the Claude Code session itself

The repository and the board are the durable state. If you also want the old session's transcript
and memory, Claude Code keeps them under `~/.claude/projects/<checkout path with slashes as
dashes>/`: the session's `.jsonl`, a same-named directory with subagent transcripts and workflow
journals, and `memory/` with the project's auto-memory. Copy them into the matching project
directory on the new machine (clone at the same absolute path or rename the directory to match),
then `claude --resume <session-id>` from that checkout. Workflow journal replay depends on the old
scratchpad paths, so launch the next cycle's script fresh rather than resuming an old run.

## Lore the prompts already encode

Flaky-under-load list (all pass exact-solo): `client_focus_closes_display_panes_and_preserves_chooser_modes`,
`event_hooks_fire_after_mutation_with_captured_formats` (automatic-rename race, `bash` where the
test expects `named`),
`history_request_is_guarded_clamped_and_returns_self_contained_rows`, copy-mode reconcile tests,
`daemon_native_split_resize_commits_exactly_and_rejects_stale_contexts`,
`nested_alias_queue_bubbles_shutdown_and_yield_to_its_parent`,
`control_sourced_run_shell_closes_before_raw_output_and_same_line_continues`,
`request_full_enqueues_only_the_requested_visible_pane`,
`display_menu_resize_lifecycle::a_resize_moves_the_menu_and_keeps_everything_else` (waiter wakes
before the chosen row's command runs; the test polls now; residual 5503619053),
zz-terminal `pty_output_drains_while_the_input_writer_is_backpressured`,
`wait_exit_holds_the_control_process_until_a_second_blank_line` (hangs under load; timeout-guard
cli_binary runs), `concurrent_default_interactive_attaches_share_session_zero` (headless "not a
terminal", may be misattributed), `smoke/source-replay-diagnostics` (pin-side crash under
concurrent scenario load; run it solo after sharded gates), and `smoke/client-non-utf8-cwd` on APFS.
`attached_client_extents_clamp_retained_and_default_dimensions` is OFF the list since `5e6de65a`: it
was never a load flake but a deterministic fixture race (the output-view pane exited and the watcher
killed its window on the first resize); the test now retains that pane.

Registry grammar: closing = removing the slug from the group's items (an emptied group moves to
`closed[]`); native decisions = relocate the slug into an accepted-native group with the measured
stance (precedent `1f24a1f1`); park = relocate into a `park`/`blocked` group with the recipe; flag
promotions move `catalog.rs` counters and `compat_manifest_tests.rs` partition counts together
(precedent `c6ce82c4`); a wrong close is reverted and the reason records the refuting measurement
(precedents `0fec342` + `9cab1fa`, `be0052bb` + `6ca61b1f`). `cargo test -p zz-mux` is mandatory
after any registry edit, and `cargo test -p zz-daemon --lib` whenever mux target resolution, effect
shapes, or layout change.

Reviewer catches worth remembering: proofs gathered before the final commit are worthless (cycle 4);
durable registry resolutions must carry every divergence the worker discloses; doc comments must
stay attached to their fn; a menu width rule that ignored the title seed (cycle 3); a mode reset
built on DECSTR, which the pinned libghostty ignores entirely (cycle 5); a fixture that configured
away the pin's gating (`focus-events on`) and so could not see that the pin emits two focus hooks
where zz emitted ten (cycle 5); a resolution claiming the mark survives cross-session `move-window`
when the pin never retargets `marked_pane.s` (cycle 5); a hook close built on a mechanism the pin
does not have (the notified client as format client, where the pin uses the activity-time best
client; a single-client differential could not see it) (cycle 6); bound-key chains preflighting the
invoker's read-only bit instead of the `-c` client's (cycle 6); a fixture step that landed in the
wrong snapshot on both binaries and so looked like a divergence (cycle 6). Gate lore:
`compat/run.sh --delta --list` once returned a stale selection, so list twice and reconcile against
`git diff --name-only`; grep the tree for conflict markers after every rebase (one got committed
into `catalog.rs`); a shard runner under bash 3.2 ran the whole corpus eight times at once; when a gate rebase
conflicts on `compat/tmux-gaps.json`, `compat/orchestration/gaps-merge.py BASE OURS THEIRS OUT`
merges the two lanes' closes by record id and exits 2 on a record both sides changed differently
(feed it `git show :1:compat/tmux-gaps.json`, `:2:`, `:3:`), then regenerate `gaps.md` and recount
`catalog.rs` and `compat_manifest_tests.rs`, which move every cycle.
