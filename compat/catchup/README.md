# tmux catch-up campaign

Opened 2026-10-09. Three tracks, one ledger:

- **fix**: refusals whose reasons went stale (raw TUI colour negotiation, the last caller-stream
  refusals, `capture-pane -H`, chooser `-N`/`-NN`, tree-mode styles, small semantics).
- **pin**: move the tmux pin from d77c9dc6 (a pre-3.8 snapshot) to the 3.8 tag and adopt the drift.
- **float**: floating panes, built by making the daemon's per-client popup window-owned.

Findings and rulings: [knowledge/research/2026-10-09-tmux-compat-revisit.md](../../knowledge/research/2026-10-09-tmux-compat-revisit.md).
Work items, status, branches: `compat/catchup/ledger.json` through `ledger.py`.

| File | What it is |
|---|---|
| `ledger.json`, `ledger.py` | Items, deps, zones, acceptance, status. `status`, `ready`, `show ID`, `live`, `set`, `check`. |
| `lane.md` | Lane brief template. The orchestrator fills `{ID}`, `{WT}`, `{BUDGET}`, `{EXTRA}`. |
| `review.md`, `review.sh SLOT ID` | Codex quick review of a lane branch against main. |
| `cargo.sh` | Every cargo call goes through this: memory cap, job count and two cargo slots sized from RAM. |
| `wt.sh` | Lane worktrees `../zz-cu-<slot>` with a reflinked `target` and `compat/.cache`: `add`, `item`, `rm`, `list`, `prune`. |

## Resuming (any machine)

A fresh orchestrator session (or the same one after a stop) does this, in order:

1. `git -C ~/dev/zz pull --ff-only` on `main`. Everything durable is on `main` or on `origin/catchup/*`.
2. `python3 compat/catchup/ledger.py check && python3 compat/catchup/ledger.py status`.
3. Clean up stale state: `compat/catchup/wt.sh prune` removes every `zz-cu-*` worktree whose branch
   is not an active or review item. Then check for leaked cargo slots
   (`fuser ${XDG_RUNTIME_DIR:-/tmp}/zz-cargo-slot-*.lock`, then `ps -o pid,etime,args -p <pids>`; a
   holder that is not cargo, flock or systemd-run, or a cargo older than any plausible build, is a
   leak: kill that tree only), for zz daemons left by tests (`pgrep -af 'zz_cli|zz-daemon'`, compare
   `readlink /proc/<pid>/cwd` exactly, never by path prefix), and for big files in `/tmp`.
4. Pick capacity from [Machine capacity](#machine-capacity).
5. Items in `active` or `review` were interrupted. `wt.sh item <slot> <id>` checks out
   `catchup/<id>` (from origin if it is not local), and the lane is relaunched with `lane.md` and
   `{EXTRA}` saying "this is a relaunch; continue from the branch and the ledger notes".
6. Fill free slots from `ledger.py ready` in [priority order](#priority-order).

## Orchestrator loop

Lanes are Opus subagents (Agent tool, `model: opus`, background). Reviews are Codex. The
orchestrator never runs the full suite per item.

1. **Launch**: `compat/catchup/wt.sh item <slot> <id>`, then
   `ledger.py set <id> active --branch catchup/<id>`. Fill `lane.md` (budget 120 minutes for a fix
   item, 180 for a pin or float item; the design item has no compile) and launch the agent with
   that text as its prompt.
2. **Report in**: read the report. If clauses are unmet without a good reason, send the lane back
   (SendMessage) once. Then `ledger.py set <id> review` and push the branch
   (`git push -u origin catchup/<id>`, after the secret checks in [State](#state)).
3. **Review**: `compat/catchup/review.sh <slot> <id>` (run it in the background; it takes minutes and
   writes `~/.cache/zz-catchup/reviews/<id>-<n>.md`). `LGTM` goes to merge. Otherwise send the
   findings to the lane agent for one fix pass (30 minutes). Review again only if a P0 or P1 fix
   was more than a few lines; two reviews at most, then the orchestrator decides.
4. **Merge**: in the main checkout, only when `git status` there shows nothing that conflicts (other
   sessions share it; never stash or reset their work).
   - `git merge-tree --write-tree main catchup/<id>` first: it predicts conflicts in one command.
   - `git merge --no-ff catchup/<id>`.
   - A lane branched before a pin move merges `main` and re-verifies at the new pin first
     (refresh its `compat/.cache` from the main checkout), then goes to review.
   - Merge checks, narrow: `compat/catchup/cargo.sh clippy -p <touched crates> --all-targets
     --all-features -- -D warnings`, the item's own filtered tests, and `just compat check` if the
     registry, oracle or manifest tests changed.
   - `ledger.py set <id> merged --sha <merge sha>`, commit the ledger, push `main`.
   - Delete the branch locally and on origin. Start the next ready item in the same slot
     (`wt.sh item` switches the warm worktree to a new branch), or `wt.sh rm <slot>` if nothing is
     ready. No worktree outlives its work.
5. **Milestones**: at M1, M2 and M3 run the [full suite](#full-suite-milestones-only) once on `main`.

Orchestrator shell habits: never `pkill -f <pattern>` (it matches the shell running it; list pids
with `pgrep -f`, check `readlink /proc/<pid>/cwd`, then `kill` those pids), and chain a merge and
its checks with `&&` only, so a failed merge never starts checks on the unmerged tree.

Add a dated line to [Decisions](#decisions) for every call the orchestrator makes on fabrico's
behalf, and a rule to [Lane rules](#lane-rules) in the same commit as any new lesson.

## Priority order

1. `pin.move`: unblocks the whole pin track and `float.core`. Then `pin.tui-fixtures`, because the
   TUI fixtures are red at 3.8 until it lands and every lane that runs one sees the noise.
2. `fix.tui-colour`: a real bug users hit over ssh.
3. `float.design`: no compile, runs as a third agent beside two compiling lanes.
4. The other `fix.*` items, in ledger order.
5. `pin.*` once `pin.move` merges, `pin.layout-v2` first (`float.core` waits on it).
6. `float.core`, then `float.clients`, then `float.keys`.

## Lane rules

Each rule cost a campaign real time. The source is in brackets
(`compat/orchestration/CAMPAIGN-LOG.md` = LOG, `compat/tui/HANDOFF.md` = THO,
`knowledge/playbooks/tui-parity-campaign.md` = PB).

1. **Cargo only through `compat/catchup/cargo.sh`.** It caps memory, sets `--jobs` from RAM, and
   holds one of two cargo slots inside `flock -o`, so a killed lane's daemons cannot keep a slot
   locked. Scripts that call `cargo` themselves (`just compat check`, `compat/run.sh`) go through
   it too when run as `PATH=$PWD/compat/catchup/bin:$PATH <script>`; always run them that way. [Five lanes OOMed alienware for hours; a leaked slot fd stalled every lane for 7 h.]
2. **Iterate behind a filter**: `compat/catchup/cargo.sh test -p <crate> --lib <name>`. Run the full
   test package of each crate you touched once, before your final commit. Never
   `cargo test --workspace`. [A whole `zz-daemon` run per edit made every loop cost minutes.]
3. **Clippy on touched crates before the final commit**:
   `compat/catchup/cargo.sh clippy -p <crate> --all-targets --all-features -- -D warnings`.
4. **Harness: named scenarios only.** Build `zz_cli` through `cargo.sh` first, then
   `ZZ_COMPAT_ZZ=$PWD/target/debug/zz_cli ZZ_COMPAT_TMUX=$PWD/compat/.cache/tmux-src/tmux
   ZZ_COMPAT_CORPUS=$PWD/compat/.cache/plugins compat/run.sh --strict-geometry <scenario>...`.
   Without `ZZ_COMPAT_ZZ`, `run.sh` builds zz outside the slots [THO: an 8-minute unlocked build].
   No `--delta` runs: they always add 149 smoke rows and took hours [LOG]. No full corpus.
   New tmux behaviour the harness can see gets a scenario in `compat/scenarios`.
5. **A zz-only string you change or remove**: `rg` it in `compat/scenarios` and `compat/tui` first.
   [Twice a corpus row contradicted a lane and only the gate found it, PB.]
6. **Registry**: edit only the gap entries your item names, plus new ones it needs. After any edit
   to `compat/tmux-gaps.json`, `compat/tmux-oracle.json` or `compat_manifest_tests.rs`, run
   `PATH=$PWD/compat/catchup/bin:$PATH just compat check` (zz-mux lib tests plus three daemon
   tests, plus the layout converter tests).
7. **Wire**: `PROTOCOL_VERSION` 107 shipped in v0.16.0. If you change a serde type under
   `crates/zz-protocol/src` (not `catalog.rs` or `lib.rs`) and main still says 107, move it to 108,
   move both assertions (`message.rs`, `tests/hunt_claims.rs`) and open 108 in
   `knowledge/protocol/wire-protocol.md`. If main already says 108 and no tag shipped it, append
   under 108 (main is on unreleased 108 since native-agent-drivers and osc-7501 merged on
   2026-10-09). `python3 compat/wire-version.py` tells you. [A release froze the version lanes
   were appending to, three times.]
8. **Bash calls die at 600 s.** Run long builds and fixtures detached
   (`setsid nohup <cmd> > <log> 2>&1 &`, append an `EXIT $?` marker) and poll the log. Never end
   your turn waiting on a background task. [A reviewer that did was dropped, LOG.]
9. **Fixtures that start tmux or zz**: scrub HOME and the XDG dirs only (not `env -i`), start tmux
   with `-L zzprobe-$$ -f /dev/null`, put sockets directly under `/tmp` with short names.
   [A shared HOME made a fixture compare the pin with itself, LOG.]
10. **Proofs at tip**: every result in your report comes from your final commit.
11. **Zones are a hint, not a fence.** If a clause needs a file outside your item's zones, edit it
    and say so. [Three cycles left items open because the last fix sat in "someone else's" crate.]
12. **Fix, don't record.** A divergence goes into the registry or `divergences.md` only when a
    clause says so or fabrico ruled on it. [Cycle 4 merged three lanes that verified nothing.]
13. **Probes live in the repo** (tests or `compat/scenarios`), never only in `/tmp`, which is RAM
    and is lost on reboot. [A gate spent an hour rebuilding lost probes, LOG.]
14. **Leave nothing running**: kill the daemons and fixtures you started; no binary copies in `/tmp`.
15. **Stay in your worktree.** Start every Bash command with `cd <your worktree> &&` or use
    absolute paths; a `cd` inside a backgrounded subshell does not carry over. [fix.tui-colour ran a
    test batch in the shared main checkout this way, 2026-10-09.]
16. **Commits**: plain English, what changed and why; end with `Co-Authored-By: <a funny name>`
    (race-condition-slayer, deadbeef-hexlord); never Claude, Codex or an email. No comments in code.

## Full suite: milestones only

At M1, M2 and M3 (and before any release), the orchestrator runs once, on `main`, detached:

- `compat/catchup/cargo.sh test --workspace --all-features` (`timeout 9000`)
- both CI clippy commands from AGENTS.md
- `just compat check`
- `ZZ_COMPAT_ZZ=<prebuilt zz_cli> timeout 9000 compat/run.sh --strict-geometry --attached-client`
  (restamps `compat/results/summary.md` for CI; about 50 to 90 minutes)
- each `compat/tui-*.sh` once

A red row is re-run alone before anyone diagnoses it (`run.sh` already retries once). Known
load-only daemon flakes are listed in AGENTS.md. Never start a second full run beside the first.

## Machine capacity

| Machine | Compiling lanes | `--jobs` | Memory cap |
|---|---|---|---|
| 20 GB of RAM or less (alienware has 15) | 2 | 2 | 4G |
| 21 to 40 GB | 2 | 3 | 8G |
| more than 40 GB | 3 (`ZZ_CARGO_SLOTS=3`) | 4 | 12G |

`cargo.sh` picks jobs and cap from RAM by itself. Codex reviews and the design item do not compile,
so one of them may run beside the compiling lanes. On macOS there is no `systemd-run` or `flock -o`;
`cargo.sh` runs cargo directly there, so keep to the lane count above by hand.

## State

- **Durable**: `main` (ledger, docs, merged work) and `origin/catchup/<id>` (unmerged lane work).
  Only the orchestrator edits `ledger.json`, and only on `main`.
- **Machine-local**: `~/.cache/zz-catchup/` (filled prompts, review outputs, logs, and on alienware
  the measured 3.8 delta with its oracle tools). Never `/tmp`: it is RAM and a reboot empties it.
- **Before stopping or switching machines**: every lane branch committed (a WIP commit is fine) and
  pushed, ledger notes saying where each lane stands, `main` pushed.
- **Before every push** (public repo): `python3 compat/evidence-secrets.py`, then
  `git diff origin/<branch>..HEAD | rg -n 'gh[pousr]_[A-Za-z0-9]{20,}|sk-[A-Za-z0-9_-]{20,}|AKIA[0-9A-Z]{16}|BEGIN [A-Z ]*PRIVATE KEY'`
  must print nothing.

## Watching lanes

Stuck looks like: the same full command 4 or more times in a lane's last 60 calls, the same failure
signature 4 or more times, no transcript write for 20 minutes, two memory kills, or hours of calls
with no commit. A high raw failure rate alone means nothing. A stuck lane gets one redirect, then is
stopped with its work committed as WIP.

## Codex

Codex only reviews. `review.sh` runs `codex exec` read-only at high effort over `git diff main...HEAD`, with `review.md`
plus the ledger item as the prompt, from the lane's worktree. If Codex is out
(`^ERROR: You've hit your usage limit`, "model is at capacity"), run the same prompt through an Opus
subagent instead and note it in the ledger. Killing the orchestrator leaves a running `codex` child;
check `pgrep -af codex` on resume.

## Decisions

- 2026-10-09 fabrico: build floating panes; move the pin to 3.8; `zz share` and desktop menu input
  capture are deferred (`later.*` items).
- 2026-10-09 fabrico: no full tmux compat suite per change; Codex quick review after each item;
  orchestrator manages parallelism and worktrees; instructions must survive a machine switch.
- 2026-10-09 orchestrator: pin the `3.8` tag (7f2a35ad), not master. Master deleted popups and the
  `popup-*` options, which would break configs today; it is the next pin move, not this one.
- 2026-10-09 orchestrator: wave 1 launched on alienware: `pin.move` (slot a), `fix.tui-colour`
  (slot b), `float.design` (slot c, no compile).
- 2026-10-09 orchestrator, on the design's two open questions, after Codex's review: match 3.8 on
  both. Windows may hold only floating panes (a zz-only "keep one tile" rule needed more custom
  transfer rules than it saved), and the v2 layout writer keeps 3.8's structural position for
  floating leaves so `#{window_layout}` matches tmux.
- 2026-10-09 orchestrator: pin.move merged (338aad43a); the pin is tmux 3.8. Any checkout's
  `compat/.cache` must be refetched (`compat/fetch-tmux.sh`) before `just compat check` passes there;
  `wt.sh add` copies the main checkout's cache, so refresh that one first.
- 2026-10-09 orchestrator: fix.capture-links gets a third, final fix pass after two reviews (output
  marks on resize, repeated alternate-on, IL/DL) and merges without a third review; IL/DL may be
  recorded as an engine limit if it does not fit. Zz approximates tmux's per-row output flag with
  tracked pins, so its edge cases are bounded by budget, not chased to the end.
- 2026-10-09 orchestrator: lane worktrees are per slot (`zz-cu-a`, `zz-cu-b`, `zz-cu-c`) and switch
  branches between items, so a warm target is reused instead of re-reflinked per item.
