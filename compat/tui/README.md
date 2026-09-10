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

Cycle 1 integrated at `38c22b9e` (2026-09-09, the alienware Linux box, workflow `wf_c9486b5d-863`:
one Opus lane, one reject, one fix pass, one approve, one gate). The fixture baseline is built and
its proof banked; **nothing is verified yet** because `TUI-001`'s clause 2 is macOS-bound:

- `TUI-001` is `active`. What holds at `38c22b9e`: six geometry runs exit 0 in 4-5s against the
  10s bound, `smoke/tui-client-input-backpressure` green, and the geometry fixture now retains
  timeout diagnostics (outer capture, daemon ring log, client stderr, list-clients, which wait
  fired). The recorded 2026-09-09 macOS geometry-report timeout stays UNEXPLAINED: the lane's
  stale-pre-fix-binary story was refuted by `tui.client-input-backpressure`'s own resolution
  (the fixture exited 0 at pre-fix `012b4dcc`, and the outer pinned tmux drains continuously, so
  the stalled backpressure can never build in this fixture on any platform). Closing clause 2
  needs one attested run of `compat/tui-pane-geometry.sh` on macOS; that run is cycle 2 and the
  campaign's entire critical path.
- `TUI-002` is `review`, finished as work: `compat/tui-screen-diff.sh` landed with 65 checkpoints
  (80x24, 100x24, 80x10 asserted; 109x24, 120x24 recorded; attach, status off/on, split, zoom,
  resize round-trip, status 2, status-position top), and `--self-check` proves each channel fails
  one-sided while the named-vs-indexed colour equivalence collapses. Worker, reviewer and gate
  each built their own zz and reproduced all 195 capture files byte for byte. It flips to
  `verified` the moment `TUI-001` does (the tracker's verified-dependencies rule).
- The 109/120 recordings `TUI-004` opens on: at 120x24 the pin's pane is 120 columns, zz's is 91
  (the 28-column sidebar plus its 1-column border; `AUTO_HIDE_COLUMNS = 80+28+1 = 109` in
  `crates/zz-tui/src/sidebar.rs`); resizing 120x24 down to 100x24 makes both screens identical
  cell for cell, so the sidebar is the entire >=109 difference. Rows match at every size.
- Divergences measured with no registry owner (all on the cycle-1 TRIAGE residual): cursor
  shape/blink/colour differ at all 65 checkpoints (zz writes DECSCUSR and OSC 12 from
  `crates/zz-tui/src/render.rs` where the pin writes neither; the screen-diff prints that channel
  but does not gate it, since a sabotage would be indistinguishable from the standing
  divergence); the pane body promotes named/indexed colours to RGB where the status row no
  longer does; an explicit default foreground under a non-default status-style; and
  `compat/status-row.sh` exits 1 under `LC_TIME=pt_BR.UTF-8` because the pin expands `%b`
  through libc strftime while zz's `crates/zz-mux/src/formats.rs` uses locale-independent chrono
  (the `LC_ALL=C LC_TIME=C` control exits 0).
- Machine facts (alienware: CachyOS, 16 cores, 15 GB + 15 GB zram, btrfs): first campaign box
  with no prior corpus stamp; `formats` clean at baseline; cold debug build 5m34s; a debug build
  is NOT bit-reproducible here (four distinct sha256 for identical source across worker, reviewer,
  gate and the pre-cycle environment record), so a recorded hash identifies an artifact but
  cannot attest it -- reproduction is the proof. `~/dev/zz-gate-target` (24G) and
  `~/dev/zz-tui-lane` stay warm for the next cycle on that box.

After cycle 2 verifies TUI-001 and TUI-002, the ready set is `TUI-003`, `TUI-004` and `TUI-010`;
cycle 3 runs them as parallel lanes (the auto-roll policy of 2026-09-09: a clean gate rolls the
next cycle without waiting).

## Launching cycle 2 (macbook)

Cycle 2 is one attested macOS run of the geometry fixture plus the records that flip `TUI-001`
and `TUI-002` to verified. `compat/tui/run-2.js` defaults to the macbook; every box fact is an
`args` override (see `M` at its top).

1. Preflight: `gh auth status` answers; `~/.claude/settings.json` carries the `Bash(rm:*)` allow
   and the `guard-rm-home.py` hook (see the tmux handoff's "Resuming on another machine");
   `caffeinate -is -w <claude pid>`; `compat/fetch-tmux.sh` has built the pin; readiness is the
   `formats` scenario running clean:
   `PATH=/opt/homebrew/bin:$PATH ZZ_COMPAT_TMUX=$PWD/compat/.cache/tmux-src/tmux ZZ_COMPAT_CORPUS=$PWD/compat/.cache/plugins compat/run.sh --strict-geometry formats`.
2. Mint the lock front under TRIAGE, then hold it for the run:
   ```sh
   export ZZ_BOARD_HOLDER=macbook/orchestrator
   python3 compat/board.py claim TRIAGE --lease 1h
   python3 compat/board.py front F-TUI-BASELINE-VERIFY --kind lock --priority 1 \
     --contract "TUI-001 clause 2 and the TUI-001/TUI-002 verification records in compat/tui/campaign.json" \
     --zones raw-tui --path compat/tui/ --path compat/tui-pane-geometry.sh \
     --notes "TUI parity cycle 2: the macOS geometry run TUI-001 clause 2 needs; runtime changes only under the declared zz-tui excursion"
   python3 compat/board.py release TRIAGE --reason "minted F-TUI-BASELINE-VERIFY"
   python3 compat/board.py claim F-TUI-BASELINE-VERIFY --lease 6h
   ```
   `--holder` is a global flag and shell state does not persist between an agent's Bash calls;
   prefix `ZZ_BOARD_HOLDER=...` on every call when driving this from a session.
3. From a Claude Code session in the checkout:
   `Workflow({ scriptPath: '/Users/demfabris/dev/zz/compat/tui/run-2.js', args: { date: '<today>', protected: '<what runs on the default sockets right now>' } })`.
   Keep the session alive for the run (worker under two hours including the cold build, review
   under one, gate about one).
4. When the gate reports: verify `origin/main`, `python3 compat/tui/tracker.py check` and `ready`,
   `python3 compat/board.py status` (the lock INTEGRATED, MAIN and TRIAGE free). Write the
   close-out into this section and `knowledge/log.md`. The next runner comes from a fresh `ready`
   listing, the way `compat/orchestration/HANDOFF.md` does for the tmux campaign; the alienware
   session watches `origin/main` and rolls cycle 3 itself if it is still alive.

A single unattended session can run the same loop by hand: claim the front, follow the runner's
worker prompt in a worktree, get an independent review, run the gate stages, ledger, release.
