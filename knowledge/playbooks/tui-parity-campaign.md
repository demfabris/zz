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
