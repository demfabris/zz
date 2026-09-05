# Handoff for the tmux-compat campaign: cycle 14 integrated

Updated 2026-09-05 after the Ubuntu cycle-14 gate. Start here; the older instrument-pass and
machine-move sections below describe the cycle-13 pause and no longer define current state.

Instrument landed at `f6348f19`, keys at `533d253c`, then buffers at `c2827417`. One Codex
gpt-6-astra gate ran alone at medium reasoning on the default tier (the workers started at high
reasoning on the fast tier; fabrico switched the cycle to medium/default at 14:45, mid-cycle). This order put the repaired attached fixture on
main before the stamped run and the keys focus-test correction on main before the buffers suite.
All reviewer must-fixes were applied and their probes re-run. Protocol remains v98.

The full run passed all 231 scenarios / 2,742 steps, with four registered known rows and the
attached-client fixture PASS recorded at `c282741787c9`. The harness printed `summary current`.
The SHA-256 of `compat/results/summary.md` is
`eb015c8382850aac3a8d2355fab28296667088c7c67592e8cd6e2c36639a8c2b`.
Owner commits `e9d23ec9`, `5dad18fe`, `b75bc348`, `4cee170c`, and `75d9ca24` landed during the full run. The records rebased conflict-free onto `75d9ca24`; the attached fixture is unchanged. The owner's `75d9ca24` makes post-stamp crate changes a warning, so the full run remains recorded at `c282741787c9`, with three owner crate commits afterward.

All three former environment-red rows now pass. The fixture's native-search proof does not close
the separately measured stock-search output-loss defect.

The frozen meter remains 100.0% (304/304 items, 65/65 groups). The live registry has 46 groups
holding 434 items: 4 OPEN, 0 BLOCKED, 42 ACCEPTED; 180 closed records. Ledger settlement is
222/226 (98.2%). Six post-freeze items remain open: mixed CLI output queues, command-output stock
search prompts, Linux dead-signal names, and prefix `f`, `M-n`, `M-p`. Read their acceptance and
measurements in `compat/tmux-gaps.json` before allocating the next work. `clients.interactive-refresh`
and `formats.terminal-runtime` remain accepted, with explicit refresh and placeholder dispositions.

The three cycle-14 lane locks are integrated and released. The records commit follows the stamped
tip; MAIN and TRIAGE are released after its ledger and residual pass. The F-SPLIT-MUX-*-V5 chain
stays untouched. Original campaign branch tips stay on origin; rebased tips landed on main without
force pushes. Only this gate's instrument/keys/buffers worktrees are removed; the shared build
directory, prior gate worktrees, and worker/reviewer worktrees remain.

Next-cycle scope still comes from `CAMPAIGN-REVIEW.md` plus the four live groups. Older descriptions
of an empty registry or a missing attached stamp are historical. The explicit cycle prompt owns
agent choice, lane order, concurrency, and resource limits; do not inherit those from older scripts.

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
| `origin/main` | `75d9ca24` (owner commits after `c2827417`) plus the rebased cycle-14 records commit |
| Agreed-scope meter | 100.0% (304/304 items), 65/65 groups done; six live post-freeze items across four groups |
| Live registry | 46 active groups / 434 items; 4 open, 0 blocked, 42 accepted; 180 closed records |
| Corpus | 231 scenarios / 2,742 steps / four known rows; attached-client PASS recorded at `c282741787c9` |
| `PROTOCOL_VERSION` | 98 (0x62); no cycle-14 wire change |
| Unmerged cycle-14 work | None |
| Board | All three cycle-14 locks integrated and released; MAIN and TRIAGE released after the records/residual pass; F-SPLIT-MUX-*-V5 unchanged |
| Remotes | SSH origin unchanged; all cycle-14 pushes succeeded without force |

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

## Historical cycle-13 retrospective: fifteen things outside the frozen registry

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

### The instrument pass, as left (2026-09-04 evening)

Not a cycle: one owner, hours each, no reviewers. Two of three instruments landed; the third is
measured and half fixed. `CAMPAIGN-LOG.md`'s last entry has the full measurements.

DONE. `compat/run.sh` stamps the summary footer with `Recorded at: <commit>` on a full run and
`--check-summary` refuses a footer with no stamp, a `-dirty` stamp, a stamp that is not an
ancestor of HEAD, or a stamp behind which `compat/attached-client.sh` itself changed; commits that touch
`crates/` after the stamp only print a drift warning (relaxed 2026-09-05 at fabrico's instruction: the gate
re-records after every code merge, and ordinary commits to main must not force a 90-minute rerun). The
stored footer has NO stamp, so `--check-summary` is RED on main right now and every gate's records
stage will fail until a full run with the fixture passing is recorded
(`compat/run.sh --attached-client`, full corpus plus fixture, about 30 minutes). Every gate that
merges code must re-record it. `ZZ_COMPAT_ZZ=<path to a built zz>` skips `run.sh`'s own build.

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

HALF DONE. The attached-client fixture. One fix is in and verified (the command-output search
prompt: the TUI now draws the pin's `(search down)`, and the fixture matched a bare `/` on the zz
side; both sides now wait for the same string). Three more failures are MEASURED and not fixed:
the command-output view is client-local (`#{pane_in_mode}` stays 0, `#{client_key_table}` is the
observable, do not swap it), and one run saw that table drop to `root` after Escape under vi
where a direct probe keeps it open, unresolved; `probe_command_prompt` on the tmux side raced its
own BSpace keys against the prompt opening (answered `mainprompted`), a settle after `C-b ,`
fixes it; `probe_side` once never saw `ATTACHED_ROOT_OK` because the first keys beat the zz
client's readiness. A run takes about ten minutes and, when it dies, leaves zz daemons on
`/tmp/zza-*.sock` that the trap does not reach: reap them by pid from the socket name. Run it as
`compat/attached-client.sh <zz> <pin tmux>` with a zz built at HEAD; on this box the warm build
is `~/dev/zz-gate-target/debug/zz`.

Order for whoever resumes: (1) the two races (a settle before the rename keys, a readiness wait
before `probe_side`'s first key), (2) the Escape question by probe, (3) a full
`compat/run.sh --attached-client` to stamp the footer, then the gate is green again.

### The cycle shape on Codex (from cycle 14)

Fabrico's instruction on 2026-09-05: every agent is Codex `gpt-6-astra`, reasoning `medium`,
service tier `default` (cycle 14 launched at high/fast and was switched mid-way; do not go back to
the fast tier unless told). The orchestrator is `compat/orchestration/codex-compat-run-14.py`: copy
it for cycle 15, keep the prompts' rules, rewrite the lane batches from the registry census and
`CAMPAIGN-REVIEW.md`.

- `python3 compat/orchestration/codex-compat-run-14.py --run-dir ~/dev/zz-run-N` runs the workers
  in parallel (each in its warm worktree, which the script checks out detached at `origin/main`),
  one reviewer behind each, then the serial gate. `--stage work --lane <key>` and `--stage gate`
  split it; any `<run-dir>/<label>.json` already present is reused, so a rerun resumes. The script
  renews the lock fronts and MAIN hourly; mint the lock fronts under TRIAGE and claim them (8 to
  10 h) before launching.
- Codex specifics: the prompt goes in on stdin (`-`) with stdin otherwise closed, or `codex exec`
  hangs on "Reading additional input from stdin"; `--output-schema` makes the JSON report the final
  message and `-o` writes it; `-C <dir>` sets the workspace. A run's session id is printed at the
  top of its log, and `codex exec resume <id> [flags] "<prompt>"` continues it with full context:
  that is how fix passes and re-reviews ran in cycle 14 (worker rejected, resume the worker with
  the blockers, move the review worktree to the new tip, resume the reviewer). `resume` takes no
  `-C`, `-s` or `--color`: run it from the worktree and pass `-c 'sandbox_mode="danger-full-access"'`.
- Reviewers apply the flake rule inconsistently: cycle 14's first two rejections read the known
  focus-test flake as a red tip. The prompts now spell the rule out; keep them doing so.
- Killing the orchestrator process leaves its `codex exec` child running and it still writes its
  `-o` report; a chained `until [ -f <label>.json ]` launcher picks the stage up from there. Never
  `pkill -f` a pattern that appears in your own shell's command line.
- The Claude Code harness kills background shell tasks when free memory dips during concurrent
  links; Monitor tasks survive. Three lanes building at once on the 8-core box is the ceiling.

### Cycle 15

From the review's "Next cycles" plus the four groups cycle 14 left open. Lane A: config discovery
and the pane PATH decision (findings 2 and 3), plus the remainder of `keys.prefix-stock-commands`
(`f` needs find-window to be a real mux verb; `M-n` and `M-p` need next-window -a and
previous-window -a to honour activity, not only bells). Lane B: background status jobs and the
control-notify fixture (findings 9 and 12), plus the three measured defects
`clients.cli-output-mixed-queue`, `clients.command-output-pane-prompt` and
`formats.dead-signal-platform-name`. Read each open group's reason first; they carry the probes.
Cycle 16 stays as the review lays it out (the desktop status row and the proof debt).

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

## Historical machine notes (through cycle 13)

- **ubuntu box** (8 cores, 30 GB, Ubuntu 26.04, bash 5.3, btrfs): ran cycles 5, 6 (first half)
  and 10. SSH origin works. Worktrees `~/dev/zz-opus-dint`, `~/dev/zz-opus-panes`,
  `~/dev/zz-opus-termopts` sit on the cycle-14 campaign branches, warm (20-30 GB targets each;
  reuse with `git checkout --detach origin/main`, which the Codex script does itself);
  `~/dev/zz-review-client`, `~/dev/zz-review-dint`, `~/dev/zz-review-copy` are review scratch
  (`-copy` has no build: reviewers of compat-only lanes use the worker's binary); cycle-14 logs,
  prompts and JSON reports are in `~/dev/zz-run-14`; the client gate's `~/dev/zz-gate-client` worktree was
  removed after the merge, and `~/dev/zz-gate-target` (47 GB) is the shared gate
  build dir with a reflinked ghostty source at `zz-gate-target/ghostty-src` for
  `GHOSTTY_SOURCE_DIR`; the queue and copy gate worktrees were removed. The three compat rows that were red
  here through cycle 13 (`smoke/remain-on-exit-format`, `smoke/format-modifier-interrogate`,
  `smoke/pane-engine-knobs-input`) pass since the instrument lane landed at `f6348f19`. The allow rule and the hook (pointing at the checkout's
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
