# Cycle-5 handoff for the tmux-compat campaign

Paused 2026-09-01 ~18:40Z so orchestration can move to another machine. Everything a new machine
needs is in this repository and in issue #7; nothing lives only on the old one.

## State at pause

| Fact | Value |
| --- | --- |
| `origin/main` | `8dd47505` (cycle-4 ledger; daemon lane `21ef482c`, copy-mode `ad539f4c`, formats `747acb39`) |
| Agreed-scope meter | 54.9% (167/304 items), 25/65 groups done; `python3 compat/progress.py` |
| Live registry | 41 unresolved groups / 141 items (35 open, 6 blocked); ledger 80.8% (172/213) |
| Corpus | 145 scenarios / 2,094 steps, attached-client PASS, summary digest `fc988682…` |
| `PROTOCOL_VERSION` | 91 (next bump 91 -> 92: hex hello frame 0x5B -> 0x5C, test `..._ninety_one` -> `..._ninety_two`) |
| Board (issue #7) | MAIN and TRIAGE free. Cycle-5 lock fronts minted and READY: `F-TERMINAL-CLIENT-OPTIONS`, `F-PANE-MODEL-BASKET`, `F-DAEMON-INTERACTION-SMALLS` (contracts in their FRONT comments). Mooted fronts already withdrawn: `F-FORMAT-WINDOW-CELL-METRICS`, `F-CONTROL-DIAGNOSTICS-V2` |

Biggest remaining baskets: `options.terminal-behavior` (18), `keys.copy-mode-binding-fidelity`
(16, mostly blocked by the accepted `command-prompt -P` decision), `pane.selection-state` (11),
`options.pane-chrome` (11), `prompt.command-fidelity` (11), `choosers.command-flags` (8).

## The cycle

`opus-compat-run-5.js` beside this file is the ready-to-run Claude Code Workflow script: three Opus
implementor lanes in parallel worktrees, one Fable adversarial reviewer pipelined behind each lane,
one serialized Fable integration gate that merges to main, ledgers the board, recomputes
`TMUX_COMPAT_TRACKER.md`, and runs TRIAGE. Cycles 2-4 ran this exact shape (2-4h wall, roughly 2M
tokens each). Launch with the Workflow tool: `Workflow({scriptPath: "<path>/opus-compat-run-5.js"})`.
If the process dies mid-run, `Workflow({scriptPath, resumeFromRunId})` replays finished agents from
the journal cache and re-runs only what was in flight; the journal lives under the session's
`subagents/workflows/<runId>/journal.jsonl` (result key is `result`). If the gate died after pushing
some lanes, finishing the rest by hand is often cheaper than a resume: the gate's fix commits sit in
its `zz-gate-*` worktrees.

Orchestrator loop per cycle: claim the three lock fronts under your holder identity, launch, verify
`origin/main`, the board records, and `compat/progress.py` when the gate finishes, then write the
next script from a fresh registry census (protocol version, lock names, group lists, the mooted
fronts for TRIAGE), and repeat. `CAMPAIGN-LOG.md` beside this file is the per-cycle log with the
gotchas each one produced.

## Before launching on a new machine

Edit the machine-specific strings in the script:

- `/Users/demfabris/dev/zz` paths (shared checkout and worktree parent) and the 16-core etiquette
  numbers (`--jobs 8` / `--test-threads=4` for workers, `--jobs 16` / `--test-threads=8` for the
  lone gate).
- The HTTPS-only git rule existed because the old machine's yubikey was unplugged; keep it if SSH is
  unreliable, otherwise plain `origin` works.
- The rerere poison note is specific to the old checkout (a wrong auto-resolution recorded for
  `knowledge/tmux/gaps.md`). Harmless to keep: regenerating with
  `python3 compat/tmux-tracker.py write-report` is always the right resolution for that file.
- `PID 2250` was the old machine's real tmux server; replace with whatever must not be killed, or
  drop the line. The stale `zz-confirm-*` / `zz-dm-*` / `zz-flag-map2-*` pin servers were old
  probe leftovers on that machine and will not exist on a fresh one.
- Populate the caches once: the pinned tmux (`d77c9dc6`, `next-3.8`) builds through
  `compat/fetch-tmux.sh` into `compat/.cache/tmux-src/tmux`, and the plugin corpus through
  `compat/fetch-corpus.sh` into `compat/.cache/plugins`. Running any scenario through
  `compat/run.sh` triggers both. Preset `ZZ_COMPAT_TMUX` and `ZZ_COMPAT_CORPUS` in prompts so eight
  sharded runs never race the clone.
- `gh auth login` with repo scope for `compat/board.py`; pick a holder identity like
  `<host>/orchestrator` and claim the three lock fronts under it before launching (the gate prompt
  names `macbook/orchestrator`, change it).

## Board tool quirks

- `--holder` is a global flag (before the subcommand); `ZZ_BOARD_HOLDER` does the same job.
- `release` and `withdraw` require `--reason`; `note` takes `--note`; `candidate` takes `--commit
  --branch --base` plus repeatable `--proof`; `integrated` takes `--merge` and optional `--gate`;
  `front` takes `--contract --zones` plus `--priority --kind {work,lock} --deps --path --notes`.
- `renew <FRONT> --lease 2h`: a bare number is silently ignored, always give a unit.
- One zone, one claim, even for the same holder: claim one lock front per lane and let the gate
  ledger the fronts it moots.
- `withdraw` and `front` need TRIAGE held; `integrated`, `repair`, and `rejected` need MAIN held.
  A records-only push (ledger, docs) is ledgered as `integrated MAIN --merge <sha>`.
- A withdrawn front that other fronts depend on reads as `deps-broken` for them: remint them as
  V(n+1) with the merged dependency dropped under the same TRIAGE hold, then withdraw the old ones.
- Unknown zone names only warn; `python3 compat/board.py zones` lists the real ones.
- `python3 compat/board_test.py` is part of every gate.

## Moving the Claude Code session itself

The repository and the board are the durable state. If you also want the old session's transcript
and memory, Claude Code keeps them under `~/.claude/projects/<checkout path with slashes as
dashes>/`: the session's `.jsonl`, a same-named directory with subagent transcripts and workflow
journals, and `memory/` with the project's auto-memory. Copy them into the matching project
directory on the new machine (clone at the same absolute path or rename the directory to match),
then `claude --resume <session-id>` from that checkout. Workflow journal replay depends on the old
scratchpad paths, so launch the cycle-5 script fresh rather than resuming an old run.

## Lore the prompts already encode

Flaky-under-load list (all pass exact-solo): `client_focus_closes_display_panes_and_preserves_chooser_modes`,
`attached_client_extents_clamp_retained_and_default_dimensions`,
`history_request_is_guarded_clamped_and_returns_self_contained_rows`, copy-mode reconcile tests,
`wait_exit_holds_the_control_process_until_a_second_blank_line` (hangs under load; timeout-guard
cli_binary runs), `concurrent_default_interactive_attaches_share_session_zero` (headless "not a
terminal", may be misattributed), and `smoke/source-replay-diagnostics` (pin-side crash under
concurrent scenario load; run it solo after sharded gates).

Registry grammar: closing = removing the slug from the group's items (an emptied group moves to
`closed[]`); native decisions = relocate the slug into an accepted-native group with the measured
stance (precedent `1f24a1f1`); flag promotions move `catalog.rs` counters and
`compat_manifest_tests.rs` partition counts together (precedent `c6ce82c4`). `cargo test -p zz-mux`
is mandatory after any registry edit.

Reviewer catches worth remembering: proofs gathered before the final commit are worthless (cycle
4's daemon tip had four orphaned tests), durable registry resolutions must carry every divergence
the worker discloses, doc comments must stay attached to their fn, and a menu width rule that
ignored the title seed (cycle 3) was the kind of pin detail only an oracle probe finds.
