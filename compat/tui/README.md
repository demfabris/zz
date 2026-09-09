# TUI parity campaign

The plain zz TUI must give `alias tmux=zz` the same observable results as the pinned tmux, and
zz's additions stay reachable through superset commands with no enable switch or compatibility
profile. The contract compares exact CLI stdout, stderr and exit status, and for attached clients
the decoded screen: cells, styles, cursor, geometry, interaction. Escape-sequence spelling is
outside it. The contract is `knowledge/designs/tui-parity.md`; how a cycle runs is
`knowledge/playbooks/tui-parity-campaign.md`.

This directory is the campaign's state, shaped like the tmux compat campaign one level up:

| File | Role | tmux campaign counterpart |
| --- | --- | --- |
| `campaign.json` | The ledger: twelve obligations with acceptance clauses, dependencies, status and proof | `compat/tmux-gaps.json` |
| `tracker.py` | Validates the ledger, generates the report, lists ready obligations | `compat/tmux-tracker.py` |
| `tracker_test.py` | The validator's tests; `compat/check.sh` runs them | `compat/board_test.py` |
| `run-1.js` | The cycle-1 runner: one worker lane, one adversarial reviewer, one gate | `compat/orchestration/opus-compat-run-N.js` |
| `evidence/<ID>/<attempt>/` | Real measurements: environment, fixture output, captures, the review | scenarios and `compat/results/summary.md` |
| `knowledge/tmux/tui-parity.md` | The generated report; never edit it by hand | `knowledge/tmux/gaps.md` |

The board (GitHub issue 7, `compat/board.py`) is shared with the tmux campaign: TUI cycles claim
lock fronts there, and the gate ledgers integrations there.

```sh
python3 compat/tui/tracker.py check         # ledger valid, report current
python3 compat/tui/tracker.py write-report  # regenerate knowledge/tmux/tui-parity.md
python3 compat/tui/tracker.py ready         # dependency-ready obligations, by priority
compat/check.sh                             # the compat gate; runs both trackers and their tests
```

## Proof surfaces

Every proof drives real attached clients of both binaries inside an outer pinned tmux, which is
also the decoder: `capture-pane -e` re-emits attributes from its own grid, so attribute order,
batching and cursor-movement spelling collapse on both sides, while colour class (named, indexed,
RGB) stays part of the cell, as `tui.status-row` in `compat/tmux-gaps.json` established.

- `compat/tui-pane-geometry.sh`: columns and rows the pane receives at 80, 100 and 120 columns.
- `compat/status-row.sh`: the last row's bytes after each status option, at 79 columns.
- `compat/attached-client.sh`: the large attached fixture the corpus summary stamp depends on.
- `compat/tui-screen-diff.sh`: the whole-screen comparison at named checkpoints. TUI-002 builds it.

## State

Cycle 1 is written and not launched. Nothing is verified. `TUI-001` (a reliable attached-client
baseline) is the only ready obligation; `TUI-002` (the whole-screen fixture) depends on it, and
everything else depends on those two. The obligation records carry the measurements taken on
2026-09-09 that each cycle opens on. Runtime parity changes belong to `TUI-003` onward.

## Launching cycle 1

The runner defaults to this macbook; every box fact is an `args` override (see `M` at its top).

1. Preflight: `gh auth status` answers; `~/.claude/settings.json` carries the `Bash(rm:*)` allow
   and the `guard-rm-home.py` hook (see the tmux handoff's "Resuming on another machine");
   `caffeinate -is -w <claude pid>`; about 60 GB free; `compat/fetch-tmux.sh` has built the pin;
   readiness is the `formats` scenario running clean:
   `PATH=/opt/homebrew/bin:$PATH ZZ_COMPAT_TMUX=$PWD/compat/.cache/tmux-src/tmux ZZ_COMPAT_CORPUS=$PWD/compat/.cache/plugins compat/run.sh --strict-geometry formats`.
2. Mint the lock front under TRIAGE, then hold it for the run:
   ```sh
   export ZZ_BOARD_HOLDER=macbook/orchestrator
   python3 compat/board.py claim TRIAGE --lease 1h
   python3 compat/board.py front F-TUI-FIXTURE-BASELINE --kind lock --priority 1 \
     --contract "TUI-001 and TUI-002 in compat/tui/campaign.json" --zones raw-tui \
     --path compat/tui/ --path compat/tui-pane-geometry.sh --path compat/status-row.sh \
     --path compat/tui-screen-diff.sh \
     --notes "TUI parity cycle 1: fixture reliability and the whole-screen comparison; runtime changes only under the declared zz-tui excursion"
   python3 compat/board.py release TRIAGE --reason "minted F-TUI-FIXTURE-BASELINE"
   python3 compat/board.py claim F-TUI-FIXTURE-BASELINE --lease 14h
   ```
   `--holder` is a global flag and shell state does not persist between an agent's Bash calls;
   prefix `ZZ_BOARD_HOLDER=...` on every call when driving this from a session.
3. From a Claude Code session in the checkout:
   `Workflow({ scriptPath: '/Users/demfabris/dev/zz/compat/tui/run-1.js', args: { date: '<today>', protected: '<what runs on the default sockets right now>' } })`.
   Keep the session alive for the run (worker two to four hours including the cold build, review
   under one, gate one to two). Renew the lock and MAIN if it passes six hours.
4. When the gate reports: verify `origin/main`, `python3 compat/tui/tracker.py check` and `ready`,
   `python3 compat/board.py status` (the lock INTEGRATED, MAIN and TRIAGE free). Write the
   close-out into this section and `knowledge/log.md`, and the next cycle's runner from a fresh
   `ready` listing, the way `compat/orchestration/HANDOFF.md` does for the tmux campaign.

A single unattended session can run the same loop by hand: claim the front, follow the runner's
worker prompt in a worktree, get an independent review, run the gate stages, ledger, release.
