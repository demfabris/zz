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
- `compat/tui-stock-keys.sh`: stock prefix and root chords typed into the attached client's stdin,
  50 cases (TUI-003).
- `compat/tui-indicators.sh`: copy/view/prefix indicators and message/prompt restoration, whole
  screen plus the cursor tuple (TUI-004).
- `compat/tui-copy-mode.sh`: the stock emacs and vi copy tables typed through real input, six
  channels per case including the paste-buffer bytes (TUI-005).
- `compat/tui-caps.sh`: what an attached client asks of its outer terminal, decoded as pane state
  by the outer pinned tmux (TUI-009).
- `compat/tui-overlays.sh`: the command prompt, confirm-before, display-menu, display-popup and
  display-panes, whole screen plus the cursor tuple, including resizes, a message over an open
  surface and keys that must not reach a covered pane (TUI-007).

## State

The count is **8/12 baseline verified** (TUI-001 to TUI-005, TUI-007, TUI-009, TUI-010), added scope
0/6, at wire protocol 102.

Cycle 8 (2026-09-13 to 14, alienware, three lanes, three agents at a time, 14.1 hours, nine agents,
no agent errors) verified **TUI-009**, which had been open since cycle 4 and spent three cycles
blocked on two daemon files no lane held. Giving one lane `client.rs` and `terminal_features.rs`
closed it in a single 132-minute worker run: `compat/tui-caps.sh` went from 351 asserted with 15
recorded to **366 asserted with none**, and its reviewer approved outright.

That is the cycle's real lesson, and it is now the campaign's rule: **a lane's zones are drawn
around its obligation, across whatever crates it needs.** Three cycles in a row an obligation stayed
open only because its last fix sat outside the lane's zones, and each time it closed in one run once
a lane held those files.

- **keys** (`9f0a7de7`) landed the pin's border, status-wheel and mouse-context bindings from a
  pointer and closed nine gap items, and kept **TUI-008 active at ten recorded checks** where its own
  worker had reported six. Its next_action now names the two unlocks exactly: `send-keys -M` handing
  the invoking event to the pane, together with `#{mouse_any_flag}` answering the pane's real mode
  instead of the constant zero it is in `crates/zz-mux/src/formats.rs`, and then a copy-table mouse
  name reachable from a pointer. The gate also measured `smoke/status-background-jobs` nine times at
  `origin/main` (2 green, 7 red) and established it as a fifth environmental row on this box rather
  than paying for it again.
- **caps** (`adc1a9d0`) verified TUI-009.
- **commands** (`420264a2`) landed **`choose-client`**, the command zz never had, and flipped the
  chooser fixture's `client-tree-open`. **TUI-006 still does not verify**: three command-output cases
  remain inside its own clause 2. TUI-014 stays active on its info preview, and TUI-016 and TUI-017
  were never reached, because the lane spent its whole budget on `choose-client`.

Two claims were caught by review rather than by a gate, which is why the campaign now re-measures
instead of trusting: the commands lane reported TUI-014's clause 1 proved when no case ever pressed
`i` on both sides, and stated in the ledger that the chooser fixture held no recorded case for
TUI-006 when it held three. `compat/tui/verify-claims.py` runs an obligation's fixture and reads the
tally the fixture prints about itself; `compat/check.sh` runs its structural half, so every gate gets
it. `compat/tui/lint-runner.py` holds one rule per lesson an earlier cycle paid for and must pass
before a cycle launches.

Cycle 9 (`compat/tui/run-9.js`) runs three lanes and can reach 11/12:

- **mouse** takes TUI-008's ten remaining checks behind the two named unlocks. TUI-012's
  verification waits on it.
- **choosers** takes TUI-006's three command-output records first, because it closes a baseline id,
  then TUI-014's info preview and its three mode tools. Its prompt change reaches verified TUI-005,
  so `compat/tui-copy-mode.sh` staying at zero recorded is a gate blocker.
- **introspection** takes TUI-016 and TUI-017's three text residues, and its gate, being last,
  verifies TUI-012 once TUI-008 lands.

After cycle 9 only **TUI-011** should remain, waiting on TUI-015 (a lock surface, which needs a
product decision about ownership and cancellation before code) and TUI-018 (a bounded caller-stream
channel, which its own record places on the superset roadmap rather than in parity polish).

Earlier cycles:

- Cycle 1 (`38c22b9e`, plus the same-day deferral records `ccab35ce`): the fixture baseline.
  TUI-001 and TUI-002 verified on banked, thrice-reproduced proof; the macOS half of TUI-001's
  clause 2 lives in `TUI-013` (fabrico's 2026-09-09 deferral; `compat/tui/run-2.js` is its ready
  macbook runner and it blocks nothing).
- Cycle 3 (`ce74bab7`): TUI-003 verified -- stock split/chooser/rename/selection/zoom/detach
  bindings produce the pinned result through real stdin (`compat/tui-stock-keys.sh`, 50 cases),
  the launcher compared on empty and live servers, and chrome no longer consumes root or
  application keys outside its owning context. TUI-010 verified -- a drop-and-redraw path
  replaces parking past the 4 MiB writer budget, detach under backlog leaks no queued paint,
  simultaneous/read-only/reattach clients asserted; `tui.client-output-queue-budget` closed.
  The sidebar decision landed: width never invokes the sidebar, and the 109/120-column cases of
  `tui-screen-diff.sh` and `tui-pane-geometry.sh` now assert instead of record. Cursor
  attributes match the pin (no DECSCUSR or OSC 12 by default) and that channel asserts too.
- `TUI-004` is `review`, REOPENED at the cycle-3 close-out: the cycle-3 gate promoted it to verified
  while its own evidence note says clause 3 is open (copy/view/prefix indicators and
  message/prompt restoration undriven; the three theme status rows still recorded). Clauses 1
  and 2 keep their whole-screen evidence at `4cd23eb3`. Cycle 4's canvas-close lane finishes
  clause 3, including the theme landing the gate measured: a 21-name roster (not the 11 the
  cycle-18 handoff recorded), `TMUX_OPTION_CONSUMERS` 118 to 139, the consumer half closing 21
  items of the accepted `options.theme-palette` gap (so the lane owns it), the wire half a
  `TmuxColour` field on `StatusLine` at protocol 99 to 100 -- one landing, never half.
- Cycle 4 (gated at `2b406cc1`, 2026-09-10, alienware: three lanes, the copy lane rejected once in
  review and fixed, all three merged): no obligation verified, so the count stays 4/12. TUI-004,
  TUI-005 and TUI-009 are `review` with every cited fixture green at the tip, and each record's
  evidence note ends with the gate's reason. TUI-004 is one step away: clause 3's copy-mode and
  view-surface indicator cells appear only in recorded cases, against the accepted
  `options.native-mode-styles`, and recorded cases are never counted. The theme landing is whole:
  `options.theme-palette` and `tui.status-row` closed, and `status-row.sh` asserts 14 of 14.
- PROTOCOL_VERSION is 100 since cycle 4: `StatusLine` gains `theme: ThemeColours`, ten `TmuxColour`
  slots in `colour_theme_table` order, a pure append. Cycle 3's upstream note for fabrico still
  stands: that gate repaired two reds that arrived with the thirteen mid-cycle main commits (the
  `tools [--skill]` list-commands assertion, and `commands.native-superset` item order) -- revert
  either if the intent differed.
- Box facts added: four corpus rows are environmental on alienware (micro-flags %b locale,
  show-options-hooks and lane2-store through lock-command defaulting to vlock,
  smoke/plugin-runtime-resurrect-restore), each proved red at origin/main;
  `compat/tui-stock-keys.sh` root-binding-detaches can flake on a wall-clock second boundary
  because it compares a row carrying a timestamp -- worth pinning.

Triage at the cycle-4 close-out (orchestrator, 2026-09-10): when an accepted gap keeps a native
presentation (`options.native-mode-styles`, `options.native-overlay-styles`,
`choosers.native-presentation`, `presentation.native-status`), the contract still decides the TUI
portion. The raw TUI renders the pin's cells, the GUI keeps its native surface, and a landing
closes exactly the items the raw TUI starts honouring (the `options.theme-palette` precedent).
Cycle 4's gate held TUI-004 over this question, so this decision is what unblocks it
(`knowledge/designs/tui-parity.md`, Proof and ownership).

Cycle 6 (`compat/tui/run-6.js`, lock front `F-TUI-CYCLE-6-LANES`) reruns the same five lanes as
punch lists of exactly what cycle 5's reviews and gate left open, all built on
`campaign/tui-cycle5-gated`. The modes lane first makes `attached-client.sh` green again. The
gate is now one agent per branch in the order modes, copy, caps, overlays, choosers, each pushing
main when green, so work lands incrementally. PROTOCOL_VERSION stays 101: 100 and 101 are
unreleased (zz 0.7.0 shipped 99), so cycle-6 appends fold into 101.

Cycle 5 (`compat/tui/run-5.js`) ran five lanes under one lock front, `F-TUI-CYCLE-5-LANES`:
- modes (TUI-004): the copy-mode position indicator and selection style inside the pane, the
  view-mode surface for command output, and message and prompt styles on `StatusLine`.
- copy (TUI-005): half-page and page placement, the vi rectangle newline, prefix precedence over
  copy tables, and three copy formats.
- caps (TUI-009): colour class through the frame and wire, the default foreground, `-2`/`-u`/`-T`,
  and extended keys.
- overlays (TUI-007) and choosers (TUI-006): started in the same cycle because TUI-004 verifies in
  the same gate.

A case that a sibling lane's landing fixes is recorded as `SIBLING:<lane>`, and the gate flips it
after that lane merges. Every wire append folds into one PROTOCOL_VERSION 101. After cycle 5,
TUI-008 and TUI-011 unlock, then TUI-012.

## Before launching any cycle

```sh
python3 compat/tui/lint-runner.py compat/tui/run-N.js
```

One rule per lesson an earlier cycle paid for, each carrying the cycle that earned it. It exits
non-zero until the runner carries them all. When a cycle teaches a new lesson, add the rule in the
same close-out commit that records the cycle; prose alone has been forgotten every time.

## Launching the deferred macOS run (macbook)

Whenever fabrico is next on the macbook: this closes `TUI-013` with one attested run of the
geometry fixture. `compat/tui/run-2.js` defaults to the macbook; every box fact is an `args`
override (see `M` at its top).

1. Preflight: `gh auth status` answers; `~/.claude/settings.json` carries the `Bash(rm:*)` allow
   and the `guard-rm-home.py` hook (see the tmux handoff's "Resuming on another machine");
   `caffeinate -is -w <claude pid>`; `compat/fetch-tmux.sh` has built the pin; readiness is the
   `formats` scenario running clean:
   `PATH=/opt/homebrew/bin:$PATH ZZ_COMPAT_TMUX=$PWD/compat/.cache/tmux-src/tmux ZZ_COMPAT_CORPUS=$PWD/compat/.cache/plugins compat/run.sh --strict-geometry formats`.
2. Mint the lock front under TRIAGE, then hold it for the run:
   ```sh
   export ZZ_BOARD_HOLDER=macbook/orchestrator
   python3 compat/board.py claim TRIAGE --lease 1h
   python3 compat/board.py front F-TUI-MACOS-TIMEOUT --kind lock --priority 1 \
     --contract "TUI-013 in compat/tui/campaign.json" \
     --zones raw-tui --path compat/tui/ --path compat/tui-pane-geometry.sh \
     --notes "TUI parity deferred macOS run: the attested geometry run TUI-013 needs; runtime changes only under the declared zz-tui excursion"
   python3 compat/board.py release TRIAGE --reason "minted F-TUI-MACOS-TIMEOUT"
   python3 compat/board.py claim F-TUI-MACOS-TIMEOUT --lease 6h
   ```
   `--holder` is a global flag and shell state does not persist between an agent's Bash calls;
   prefix `ZZ_BOARD_HOLDER=...` on every call when driving this from a session.
3. From a Claude Code session in the checkout:
   `Workflow({ scriptPath: '/Users/demfabris/dev/zz/compat/tui/run-2.js', args: { date: '<today>', protected: '<what runs on the default sockets right now>' } })`.
   Keep the session alive for the run (worker under two hours including the cold build, review
   under one, gate about one).
4. When the gate reports: verify `origin/main`, `python3 compat/tui/tracker.py check` and `ready`,
   `python3 compat/board.py status` (the lock INTEGRATED, MAIN and TRIAGE free). Write the
   close-out into this section and `knowledge/log.md`. This run gates nothing else: the main
   campaign continues on its own cycles regardless of when it lands.

A single unattended session can run the same loop by hand: claim the front, follow the runner's
worker prompt in a worktree, get an independent review, run the gate stages, ledger, release.
