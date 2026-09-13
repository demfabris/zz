---
type: Playbook
title: Running the TUI parity campaign
description: "Run the TUI parity campaign: the ledger, the proof surfaces, and the worker, reviewer, and gate cycle that turns measurements into verified obligations."
resource: compat/tui/run-1.js
tags: [tui, tmux, compatibility, campaign, playbook]
timestamp: 2026-09-09T00:00:00-03:00
---

# Outcome

Make the plain zz TUI honor the [TUI parity contract](/designs/tui-parity.md) one bounded
obligation at a time, with every closure proved against the pinned tmux by real attached clients
and reviewed by an agent that did not write it. The campaign borrows its shape from the
[tmux compat cohorts](/playbooks/tmux-compat-cohorts.md): a ledger with acceptance clauses as the
contract, differential fixtures as the proof, a runner that drives one worker lane, one adversarial
reviewer and one serialized gate, and the shared board for locks and the integration ledger.

# Records

`compat/tui/campaign.json` owns the obligations. Each carries acceptance clauses, dependencies,
references into `compat/tmux-gaps.json`, an `evidence_note` (the current measurement), a
`next_action`, and a `proof` block once verified. The generated
[report](/tmux/tui-parity.md) is the readable view; regenerate it, never edit it.

```sh
python3 compat/tui/tracker.py check
python3 compat/tui/tracker.py write-report
python3 compat/tui/tracker.py ready
```

`check` validates fields, the pin against `compat/tmux-oracle.json`, gap references, source and
artifact paths, dependency cycles, and report freshness; `compat/check.sh` runs it beside the tmux
tracker. Statuses are `unmeasured`, `different`, `active`, `review`, `blocked`, `verified`. Only
`verified` counts. The baseline of twelve ids is frozen; additions report separately and never
dilute the number, the way `compat/progress.py` treats the tmux registry.

Who writes what: a worker writes `status`, `evidence_note` and `next_action` of the obligations
it owns and nothing else. The gate writes the `proof` block and sets `verified`, because
`proof.revision` must name the candidate commit the gate re-ran the proof on. A worker never sets
`verified`. Existing tmux gaps are not reopened by this campaign. A landing that makes the raw TUI
honour an item of a gap closes that item with a dated measurement (cycle 4 closed
`options.theme-palette` this way; an accepted native-presentation gap keeps its decision for the
GUI, per the contract's 2026-09-10 triage). Any other contradicting measurement is appended to the
gap's reason, dated.

Evidence lives at `compat/tui/evidence/<ID>/<attempt>/`: `environment.txt` (both binary hashes,
zz revision and dirty state, the pin, OS, TERM, shell, outer terminal size), fixture output and
captures as `.txt` (the global ignore drops `*.log`), `notes.md`, and the gate's `review.md`.
Real runs only; a file describing a check that did not run is a defect.

# Proof surfaces

Every attached proof runs both binaries inside an outer pinned tmux with isolated `HOME` and
`XDG_CONFIG_HOME` per side, short `/tmp` sockets and bounded waits on observables. The outer tmux
is the decoder: `capture-pane -e` re-emits attributes from its grid, so attribute order, batching,
redundant resets and cursor-movement spelling collapse on both sides, while the colour class of a
cell (named, indexed, RGB) is preserved and compared, as the closed class-1 finding under
`tui.status-row` established. Cursor position and geometry come from `display-message -p` on the
outer pane. Whether cursor shape can be read that way is measured, not assumed.

- `compat/tui-pane-geometry.sh`: pane columns and rows at 80, 100 and 120, all asserted since cycle 3.
- `compat/status-row.sh`: the last row's bytes after each status option at 79 columns.
- `compat/attached-client.sh`: the large attached fixture; `compat/run.sh --attached-client`.
- `compat/tui-screen-diff.sh`: the whole-screen comparison at named checkpoints, with a
  `--self-check` that proves it fails on a one-sided glyph, colour, cursor or geometry change and
  passes on an equivalent spelling. TUI-002 builds it; later obligations extend its cases.
- `compat/tui-stock-keys.sh` (TUI-003): stock bindings typed through real stdin.
- `compat/tui-indicators.sh` (TUI-004), `compat/tui-copy-mode.sh` (TUI-005) and
  `compat/tui-caps.sh` (TUI-009): one fixture per obligation since cycle 4, so parallel lanes do
  not rebase over one file.

The corpus runner and the [harness playbook](/playbooks/compat-harness.md) still apply for anything
a detached differential can observe. A detached query never proves what an attached client draws.

# The cycle

`compat/tui/run-1.js` is the template, copied from the tmux campaign's `opus-compat-run-18.js`:

1. **Work.** One Opus lane in a fresh worktree from `origin/main` takes the batch (cycle 1:
   TUI-001, then TUI-002), each obligation under a hard budget, measures first, pushes
   `campaign/<name>`, and reports through a schema (branch, per-obligation status and proofs).
2. **Review.** One adversarial reviewer verifies every claimed clause against the pin, re-runs the
   proofs at the tip, adds a sabotage of its own, and returns approve, approve-with-fixes or
   reject. A reject gets one fix pass by a fresh agent and a re-review.
3. **Integrate.** The gate rebases, runs the workspace tests and clippy only where `crates/`
   changed, always runs the fixtures and both trackers, writes the records commit (proof blocks,
   `review.md`, `verified`), pushes main, ledgers the lock front on the board, posts residuals
   under TRIAGE, and reports.

Before launching a cycle, run `python3 compat/tui/lint-runner.py compat/tui/run-N.js`. It holds one
rule per lesson an earlier cycle paid for, each with the cycle that earned it, and it exits non-zero
until the new runner carries them. Add a rule the moment a cycle teaches one. Every lesson this
campaign wrote down in prose alone was forgotten at least once: three cycles running shipped a lane
whose remaining fix sat in a crate the lane did not hold, and cycle 4's "a lane is judged on
recorded cases flipped to asserted" had to be relearned twice. A rule in the lint cannot be
forgotten, and a rule that only lives in a page will be.

`compat/tui/README.md` carries the launch recipe (preflight, mint the lock front, the `Workflow`
call, verification) and the current state. The orchestrator writes the next runner from a fresh
`ready` listing after each gate, the way `compat/orchestration/HANDOFF.md` does.

An obligation reaches `verified` only when every acceptance clause has evidence against the pin at
the recorded revision, the fixture that proves it demonstrably fails when it should, and an
independent review approved it. A timeout, a missing capability, a waived comparison or a skipped
case is recorded, never counted. Split a large obligation into child ids when measurement warrants
it; the parent stays and depends on the children.

Since cycle 3 a cycle runs several lanes in parallel under one consolidated lock front (the board
refuses sibling fronts whose zones overlap). When one lane's fixture case can only pass after a
sibling lane's landing, the lane records it with a reason starting `SIBLING:<lane>`; the gate,
merging in a declared order, flips each such case to asserted after that sibling merges and keeps
the flip only if the fixture and its `--self-check` stay green. This lets a dependent obligation
start in the same cycle as its dependency and verify in the same records commit.

Since cycle 6 each branch has its own gate agent. The gates run in a fixed order, and each one
pushes main when its branch is green, so a later lane rebases onto what the earlier ones landed.
Cycle 5 showed why: one gate agent carrying five branches stopped partway and left nothing on
main.

A dependent obligation whose own proof is complete does not wait for another cycle. When every
clause of an obligation asserts but a dependency is not yet verified, its gate writes the evidence
and fills the proof block, leaves the status at `review` with an `evidence_note` that leads
`proof complete at <sha>; held on <ids>`, and the gate that later verifies the dependency re-runs
that obligation's fixture at its own tip, appends those commands to the proof and sets `verified`.
The tracker accepts a proof block on a `review` item, so nothing is lost between the two landings.
Cycle 6 carried TUI-007 this way behind TUI-004.

Since cycle 7 an earlier gate runs the delta corpus only for the commands its own lane touched, and
the last gate in the order runs the keys and status scenario sets and the backpressure row once for
the whole cycle. Running the same rows at every gate cost hours per cycle and caught nothing a
single run at the end would miss.

Do not resume a killed cycle with `resumeFromRunId` when the runner schedules its lanes through a
concurrency pool. The cache replays the longest unchanged prefix of `agent()` calls in start order,
and a pool starts them in whichever order lanes happen to finish, so the prefix breaks and finished
lanes run again. Cycle 6 lost about eighteen hours of agent time this way. Instead read the run's
`journal.jsonl`, embed the finished workers and reviews in a fresh script so their lanes go straight
to their gates, and launch it as a new run; `compat/tui/run-6b.js` is the template.

A lane that removes or changes a zz-only screen string, prompt, label, format or option name greps
`compat/scenarios` for it and runs the rows it finds, and its reviewer runs the delta corpus for the
lane's touched commands. Cycle 6 twice landed a presentation change that a corpus row two directories
away still asserted, and both times the gate was the first place the two met, which is the most
expensive place to find it. Every batch
prompt already tells its agent to inspect its worktree and continue from the state it is in, so a
relaunch costs only the killed agent's own progress.

# Machine notes

- macOS: `/bin/bash` is 3.2; prefix `PATH=/opt/homebrew/bin:$PATH` on every `compat/` invocation.
  Commit with `git -c commit.gpgsign=false` in unattended sessions. The box has a real
  `~/.tmux.conf` and `~/.config/zz/mux.conf`; scrub both `HOME` and `XDG_CONFIG_HOME`. Cycle 17
  measured eleven environment-red corpus rows here; a red row is a lane's only if it is green at
  `origin/main`. Never run a bare `tmux` or `zz` command without a throwaway socket.
- The board: `--holder` is global, shell state does not persist between an agent's Bash calls,
  `release` and `withdraw` need `--reason`, `front` needs TRIAGE held. Board quirks are listed in
  the tmux handoff.
- Shared checkout: other sessions' uncommitted work lives in `~/dev/zz`; add worktrees from it,
  never stash, reset or clean it.
