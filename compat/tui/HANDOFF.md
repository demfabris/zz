# TUI parity handoff: cycle 9 stopped before its gates (2026-09-14, alienware)

Cycle 9 ran its three lanes and their reviews on the alienware box and was stopped deliberately
before any gate, so fabrico could move machines. Nothing is merged. Every lane's work is pushed to
a campaign branch, and the resume point is a well defined one: run the gates.

| Fact | Value |
| --- | --- |
| `origin/main` | the wire guard and this handoff, under fabrico's `v0.9.1` release at `416c0b4d` |
| Ledger | 8/12 baseline verified (TUI-001 to 005, 007, 009, 010); added scope 0/6 |
| Merged this cycle | nothing; no gate ran |
| `PROTOCOL_VERSION` | 102, and **102 is released**: zz 0.9.0 and 0.9.1 both shipped it. The first wire change to land opens 103 |
| Board | `F-TUI-CYCLE-9-LANES` claimed by `alienware/orchestrator`; MAIN and TRIAGE free |
| Runners | `run-9.js` (this cycle), `run-9b.js` (its second half), `run-10.js` (ready, and the last cycle) |
| Deferred | TUI-013 still waits for a macOS box; `compat/tui/run-2.js` is its ready runner |

## The four branches waiting for a gate

| Branch | Tip | What it carries |
| --- | --- | --- |
| `campaign/tui-mouse` | `142139fd` | TUI-008 at `active`. Both unlocks landed (`send-keys -M` re-encoding the invoking event, and `#{mouse_any_flag}` answering the pane's own tracking) plus the pin's multi-click sequence. Three checks still recorded: the two pointer menus and the paste under a menu |
| `campaign/tui-choosers-3` | `5f06595b` | TUI-006 at `review` with **all three clauses proved**, which is a baseline id ready to verify. TUI-014 at `active`: clause 1 done including the info preview, clauses 2 and 3 untouched |
| `campaign/tui-introspection` | `8d6944cf` | TUI-016 at `review`, TUI-017 clause 2 closed and clause 1 open by design |
| `campaign/tui-mouse-menus` | see below | The mouse lane's second half: the two pointer menus, the paste under a menu, and the three `mouse_*` formats they read |

## What each gate must know

The gate order is mouse, choosers, introspection, then the menus branch. Each rebases onto the main
the previous one pushed.

Every branch here is based on `879b68fc` and main has moved a long way past it: fabrico's two
releases, a GPUI and CEF refresh, a third-party notice, and a clippy and test pass that touched
`compat/tmux-gaps.json`, `crates/zz-daemon/src/daemon.rs` and `crates/zz-daemon/src/client.rs`,
which are campaign zones.

Predicted against `origin/main` at `e04bb980` with `git merge-tree --write-tree`, and the answer is
better than the drift suggests:

| Branch | Prediction |
| --- | --- |
| `campaign/tui-choosers-3` | clean |
| `campaign/tui-mouse` | conflicts in `knowledge/tmux/gaps.md` only |
| `campaign/tui-introspection` | conflicts in `knowledge/tmux/gaps.md` only |

`knowledge/tmux/gaps.md` is generated. Never hand-merge it: take either side, then
`python3 compat/tmux-tracker.py write-report` and commit what it produces. The file it is generated
from, `compat/tmux-gaps.json`, auto-merges on both branches, as do
`crates/zz-daemon/src/daemon.rs`, `crates/zz-protocol/src/message.rs`, `catalog.rs` and
`crates/zz-terminal/src/session.rs`. Re-predict at your own tip anyway, since each gate pushes main
under the next one.

1. **The wire version moved under this cycle.** zz 0.9.0 shipped `PROTOCOL_VERSION` 102 while three
   lanes were appending to 102. The mouse branch adds `view_action` and `press_action` to
   `InputMessage::MouseKey`; the choosers branch adds `CommandPromptState.pane` and
   `ProtocolMessage::ClientTerminalType`. All three still say 102 and all three name the v102 entry
   in the version history. The first gate to land a wire change sets the constant to 103, opens a
   v103 entry, and moves the two assertions that pin the number
   (`crates/zz-protocol/src/message.rs` and `crates/zz-protocol/tests/hunt_claims.rs`); later gates
   fold into 103 and move any v102 line their lane wrote. `compat/wire-version.py` runs inside
   `compat/check.sh` and fails a gate that forgets.
2. **The introspection review returned approve-with-fixes with a real blocker.** `show-messages -J`
   prints two rows where the pin prints one as soon as a format job is live, so TUI-016 clause 1's
   `-J` half has asserted evidence only for the empty table. Its must-fix: the recorded client
   naming decision describes the server log wrongly (the pin names any tty-bearing client by its
   tty, zz names none and spells one row by hostname). Its nits: two `notes.md` files cite a commit
   that is not reachable from any ref, and the diff adds 60 comment lines against this repo's rule
   against comments in code.
3. **Cycle 9's gates never run five fixtures.** Their stage 3 list predates `tui-mouse.sh`,
   `tui-client-commands.sh`, `tui-superset.sh`, `tui-launch-diff.sh` and
   `tui-output-backpressure.sh`. `lint-runner.py`'s `every-fixture` rule catches this for later
   runners, and `run-10.js` already names all fifteen, but a gate launched from `run-9.js` will not.
   Run those five by hand at the pushed tip, or take the close-out's rule: build zz at the final
   `origin/main` and run every fixture in the tree with its `--self-check`.
4. **TUI-012 is held on TUI-008.** Its proof block is filled and its status is `review`. Whichever
   gate verifies TUI-008 flips it.

## Cycle 10, written and ready

`run-10.js` passes all fifteen lint rules and its dry run is clean. Three lanes, gated in the order
stream, modes, capture:

- **stream**, TUI-018's bounded command-stream channel, designed before it is built because the
  clause asks for one channel rather than four flag fixes;
- **modes**, TUI-014's clauses 2 and 3, which cycle 9's choosers lane left untouched: clock-mode,
  switch-mode, customize-mode, suspend-client and server-access, in the order that lane worked out;
- **capture**, TUI-017's six rich capture transports beside TUI-015's lock decision. Its batch
  carries the pin's semantics for each of `-C`, `-F`, `-H`, `-L`, `-P` and `-R` read out of
  `cmd-capture-pane.c`, and says which one to weigh rather than imitate: `-R` prints tmux's internal
  grid, its history limit and its per-line `cellused` and `cellsize`, none of which zz's terminal
  engine has in that shape, and the clause allows a measured refusal.

Before launching it, pre-position the six worktrees the runner expects, because each lane's prompt
says the orchestrator already made one and a lane that finds nothing will improvise:

```sh
for w in stream modes capture; do
  git -C <checkout> worktree add --detach ../zz-tui-$w-10 origin/main
  git -C <checkout> worktree add --detach ../zz-tui-$w-10-review origin/main
done
```

Each lane builds into its own worktree's `target/`, which is a cold build of about 30 minutes and
150 GB apiece on this workspace. Point a lane at a finished lane's warm target instead where one
exists; `run-9b.js` shows the wording. The gates make their own worktree.

TUI-011 rides on the last gate. It is a baseline id whose clause 2 holds 37 recorded roster entries
that flip as TUI-014 through TUI-018 land, so that gate is what takes the baseline to 12/12.

## Resuming on another machine

**The runner is parameterised, and its defaults describe alienware.** `run-10.js` reads `args` for
`machine`, `boxNote`, `dev`, `holder`, `date`, `workerJobs` and `gateJobs`, and every default is
this Linux box. Pass your own or the lanes will be told the wrong things, and one of them does not
merely mislead: **the cargo memory wrapper is `systemd-run --user --scope`, which does not exist on
macOS.** Every cargo command in every prompt goes through it. On a mac, replace that wrapper in the
box note with something local, or drop the cap and keep the two-slot `flock`, which is what the
concurrency limit actually needs. The locale note, the five environmental corpus rows and the known
flakes are also measurements of this box, not of yours: re-measure them rather than inherit them.

```js
Workflow({ scriptPath: '<checkout>/compat/tui/run-10.js', args: {
  root: '<checkout>', dev: '<worktree parent>', holder: '<box>/orchestrator',
  machine: '<cores, RAM, OS>', date: '<today>', boxNote: '<your box, measured>',
} })
```

The campaign resumes from the repo and the board, not from a session. On a fresh box:

1. `compat/fetch-tmux.sh` and `compat/fetch-corpus.sh` populate the caches; `compat/check.sh` proves
   the checkout is sound and now also proves the wire version is honest.
2. `python3 compat/tui/tracker.py check` and `ready` read the real ledger state. Do not trust a
   remembered count.
3. Re-point the board holder: `export ZZ_BOARD_HOLDER=<box>/orchestrator`. `F-TUI-CYCLE-9-LANES` is
   held by `alienware/orchestrator` and its lease will lapse; claim it when it reads READY.
4. `knowledge/playbooks/tui-parity-campaign.md` carries every rule the cycles paid for, including
   the second-half lane and the close-out that runs every fixture.
5. A macOS box also unblocks TUI-013, which has been waiting on one since cycle 2. Its runner and
   its board recipe are in `compat/tui/README.md`.

Nothing on the alienware box is needed to continue: the worktrees under `~/dev/zz-tui-*` and their
build targets (about 900 GB across six of them) are local caches, not state.
