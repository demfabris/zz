# TUI parity campaign

The plain zz TUI must give `alias tmux=zz` the same observable results as the pinned tmux, and
zz's additions stay reachable through superset commands with no enable switch or compatibility
profile. The contract compares exact CLI stdout, stderr and exit status, and for attached clients
the decoded screen: cells, styles, cursor, geometry, interaction. Escape-sequence spelling is
outside it. The contract is `knowledge/designs/tui-parity.md`; how a cycle runs is
`knowledge/playbooks/tui-parity-campaign.md`.

**Resuming?** `compat/tui/HANDOFF.md` is the current handoff: where the campaign stopped, which
branches are waiting for a gate, and what a gate arriving cold has to know. Read it before the
table below.

This directory is the campaign's state, shaped like the tmux compat campaign one level up:

| File | Role | tmux campaign counterpart |
| --- | --- | --- |
| `campaign.json` | The ledger: twelve obligations with acceptance clauses, dependencies, status and proof | `compat/tmux-gaps.json` |
| `tracker.py` | Validates the ledger, generates the report, lists ready obligations | `compat/tmux-tracker.py` |
| `tracker_test.py` | The validator's tests; `compat/check.sh` runs them | `compat/board_test.py` |
| `verify_claims_test.py` | The re-measurement guard's tests, attribution included; `compat/check.sh` runs them | |
| `run-1.js` | The cycle-1 runner: one worker lane, one adversarial reviewer, one gate | `compat/orchestration/opus-compat-run-N.js` |
| `run-N.js` | Each cycle's runner; `lint-runner.py` must pass before one is launched | `compat/orchestration/opus-compat-run-N.js` |
| `HANDOFF.md` | Where the campaign stopped and how to pick it up on another box | `compat/orchestration/HANDOFF.md` |
| `agentwatch.py` | Tells a spinning agent from a slow one on a 15-minute timer | |
| `verify-claims.py` | Re-measures a verified obligation instead of trusting a lane | |
| `lint-runner.py` | One rule per lesson a cycle paid for; gates a runner's launch | |
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

A fixture that several obligations share attributes each of its recorded cases to the one
obligation that case keeps open, because `verified` means zero recorded cases of this obligation's
own, not zero in the file. `compat/tui-client-commands.sh` is the roster of that kind:
`case_owner` names, for every recorded case, either an obligation id or `gap:<id>` for a divergence
`compat/tmux-gaps.json` already accepted and no obligation will close, and the summary line ends in
the per-owner tally `owners TUI-014=6 TUI-015=4 TUI-017=6 TUI-018=4 decided:TUI-016=1
gap:clients.interactive-refresh=8 unattributed=0` beside the total. A reason that opens with
`DECIDED` is a settled registration rather than an open clause - the case a clause asks for by name,
the way TUI-016 clause 2 asks for `messages-log` - and lands under `decided:<owner>`, which that
owner's own entry does not include. `compat/tui/verify-claims.py --run <ID>` then charges the
obligation its own entry plus every `unattributed` case, and charges the whole recorded count when
a fixture prints no tally at all, when the tally does not add up to the total, or when an owner
token is neither a ledger obligation nor a registry gap: a case added without an owner fails closed
against every obligation mapped to that fixture rather than sliding past one.

## State

The count is **11/12 baseline verified** (TUI-001 to TUI-010 and TUI-012; only TUI-011 is open, at
review), added scope **1/6** with TUI-016 verified, at wire protocol 103 (unreleased). Main is green
on the ubuntu box: build, workspace clippy, rustfmt, `compat/attached-client.sh` and every fixture
the day's gates ran.

The 2026-09-15 day on the ubuntu box closed cycle 9 (the introspection landing, `627e717a`) and landed
cycle 10's stream lane (`7e7cb1ee`, TUI-018 at review), then the menus second half together with the
mouse-context lane (`10779511`), which verified **TUI-008** (`compat/tui-mouse.sh` 45 asserted, 0
recorded) and with it **TUI-012** (three fresh `compat/tui-superset.sh` runs). Along the way main got
its Linux compile back, the three ubuntu-box reds fixed (one a real zz bug in the command-output
view), a chooser repaint bug fixed, and `compat/tui/verify-claims.py` learned to charge a shared
fixture's recorded cases to one obligation. From 14:30 the workers were Codex CLI lanes on fabrico's
instruction; every branch they reviewed was rejected on measured findings, which is the point of the
review. At the wrap-up three fix passes were in flight (TUI-018's alias groups, TUI-015 and TUI-017's
capture and lock residues, TUI-014's per-pane mode lifecycle); `HANDOFF.md` has the branch table, the
three alias-group blockers, and the one decision only fabrico can make: customize-mode and
suspend-client, which two lanes measured and did not build, and without which TUI-014 and therefore
TUI-011 cannot verify.

Cycle 8 (2026-09-13 to 14, alienware) verified TUI-009 after three cycles blocked on two daemon files
no lane held, and set the rule the campaign has kept since: **a lane's zones are drawn around its
obligation, across whatever crates it needs.** Two lane claims were caught by review rather than by a
gate, which is why the campaign re-measures instead of trusting: `verify-claims.py` runs an
obligation's fixture and reads the tally the fixture prints about itself, `compat/check.sh` runs its
structural half, and `lint-runner.py` holds one rule per lesson an earlier cycle paid for.

Cycle 9 (`run-9.js`) and cycle 10 (`run-10.js`) are recorded in the runners and in `HANDOFF.md`; the
day's gate protocol change (intermediate gates re-run only what a rebase can change, the close-out
gate runs everything once plus the corpus restamp CI needs) is in `HANDOFF.md`'s gate section.

## Box facts a fixture must not depend on

Two permanent facts of the ubuntu box, both measured on 2026-09-15 while three fixtures were red
here and green on the alienware box. `/bin/sh` is dash, so an inner pane started as `/bin/sh` has
no line editing: a key that a surface consumes and then also delivers to the pane (the key that
dismisses a `display-message`, a key that leaks under a menu) leaves its literal byte in the input
line, where it prefixes the next marker's `printf` and the marker never prints; flush the line with
a bare Enter before the next marker rather than assuming a shell that swallows it. And
`localhost` resolves to 127.0.0.1 alone (`getent ahosts localhost`) because the stock `/etc/hosts`
gives `::1` the names `ip6-localhost` and `ip6-loopback`, not `localhost`, so anything that
resolves the name cannot reach a service bound only on `::1`.

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
