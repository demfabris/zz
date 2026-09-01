# Cycle-6 handoff for the tmux-compat campaign

Written 2026-09-01 ~23:40Z on the Ubuntu box after cycle 5 integrated. Everything a new machine
needs is in this repository and in issue #7; nothing lives only on one machine.

## State now

| Fact | Value |
| --- | --- |
| `origin/main` | `bbc09f5` (cycle-5 ledger; terminal lane `8c1da05`, panes `887a372`, daemon `9cab1fa`) |
| Agreed-scope meter | 65.8% (200/304 items), 30/65 groups done; `python3 compat/progress.py` |
| Live registry | 36 unresolved groups (27 open, 9 blocked); ledger 83.4% (181/217) |
| Corpus | 154 scenarios / 2,361 steps, attached-client PASS, summary digest `a13fe7ad…` |
| `PROTOCOL_VERSION` | 92 (next bump 92 -> 93: hex hello frame 0x5C -> 0x5D, test `..._ninety_two` -> `..._ninety_three`) |
| Board (issue #7) | MAIN and TRIAGE free. Cycle-6 lock fronts minted and READY: `F-MUX-KEYS-COPY-FORMATS`, `F-DAEMON-PROMPT-HOOKS` (p2, carries the alias-forgery defect), `F-CLIENT-CHOOSERS-OVERLAYS` (contracts in their FRONT comments 5501598102, 5501598283, 5501598474) |

Biggest remaining baskets: `keys.copy-mode-binding-fidelity` (16, 14 blocked by the accepted
`command-prompt -P` decision), `prompt.command-fidelity` (11), `choosers.command-flags` (8),
`options.pane-engine-knobs` (8, park), `options.pane-border-chrome` (7, park), `terminal.key-control`
(5), `display-menu.behavior-fidelity` (5), `display-popup.behavior-fidelity` (5). The one p2 item on
the board is a defect, not a gap: a user-authored `__zz-command-alias-group` block still executes
instead of being rejected as an unknown command (`F-ALIAS-GROUP-FORGERY`).

## The cycle

`opus-compat-run-6.js` beside this file is the ready-to-run Claude Code Workflow script: three Opus
implementor lanes in parallel worktrees, one Fable adversarial reviewer pipelined behind each lane,
one serialized Fable integration gate that merges to main, ledgers the board, recomputes
`TMUX_COMPAT_TRACKER.md`, and runs TRIAGE. Cycle 5 ran this exact shape on this box: 3h55m wall,
2.04M subagent tokens, seven agents, three of three lanes merged. Launch with the Workflow tool:
`Workflow({scriptPath: "<path>/opus-compat-run-6.js"})`. If the process dies mid-run,
`Workflow({scriptPath, resumeFromRunId})` replays finished agents from the journal cache and re-runs
only what was in flight; the journal lives under the session's
`subagents/workflows/<runId>/journal.jsonl` (result key is `result`). If the gate died after pushing
some lanes, finishing the rest by hand is often cheaper than a resume: the gate's fix commits sit in
its `zz-gate-*` worktrees.

Orchestrator loop per cycle: claim the three lock fronts under your holder identity (6h leases,
renew them and MAIN with `renew <FRONT> --lease 6h` while the gate runs; renew sets expiry to now
plus the lease), launch, verify `origin/main`, the board records, and `compat/progress.py` when the
gate finishes, then write the next script from a fresh registry census (protocol version, lock
names, group lists, the mooted fronts for TRIAGE), mint the next lock fronts under TRIAGE, commit
the orchestration records under MAIN, and repeat. `CAMPAIGN-LOG.md` beside this file is the
per-cycle log with the gotchas each one produced.

## This machine (`ubuntu`)

8 cores, 30 GB, Ubuntu 26.04. The scripts assume it: worker etiquette `--jobs 4` /
`--test-threads=2`, gate `--jobs 8` / `--test-threads=4`, four corpus shards. `origin` is SSH and
works non-interactively; HTTPS `https://github.com/demfabris/zz.git` is the hang fallback (a
credential store exists). `gh` is logged in. Holder identity `ubuntu/orchestrator`. Caches are
populated (`compat/.cache/tmux-src/tmux` at `d77c9dc6`, `compat/.cache/plugins`). No rerere cache.
The cycle-5 worker worktrees `zz-opus-panes`, `zz-opus-dint`, and `zz-opus-termopts` are clean with
warm `target/` directories (15 to 29 GB each); the cycle-6 lanes reuse them with
`checkout --detach origin/main` instead of cold builds. Remove them only when disk gets tight; their
branches are on `origin` as `campaign/batch-*`.

## Before launching on a new machine

Edit the machine-specific strings in the script:

- The `/home/demfabris/dev/zz` paths (shared checkout and worktree parent), the core-count etiquette
  numbers, and the WORKDIR warm-reuse lines (on a fresh machine every lane creates its worktree).
- The git transport lines: keep `origin` if SSH works non-interactively, otherwise switch the fetch
  and push commands to the HTTPS URL.
- The protected-server line: no tmux server was running here at launch, so the prompts only forbid
  killing servers a worker did not start; name any real server that must survive.
- Populate the caches once: the pinned tmux builds through `compat/fetch-tmux.sh`, the plugin corpus
  through `compat/fetch-corpus.sh`; running any scenario through `compat/run.sh` triggers both.
  Preset `ZZ_COMPAT_TMUX` and `ZZ_COMPAT_CORPUS` in prompts so sharded runs never race the clone.
- `gh auth login` with repo scope for `compat/board.py`; pick a holder identity like
  `<host>/orchestrator`, replace `ubuntu/orchestrator` in the gate prompt, and claim the three lock
  fronts under it before launching.

## Board tool quirks

- `--holder` is a global flag (before the subcommand); `ZZ_BOARD_HOLDER` does the same job.
- `release` and `withdraw` require `--reason`; `note` takes `--note`; `candidate` takes `--commit
  --branch --base` plus repeatable `--proof`; `integrated` takes `--merge` and optional `--gate`;
  `front` takes `--contract --zones` plus `--priority --kind {work,lock} --deps --path --notes`.
- `renew <FRONT> --lease 2h`: a bare number is silently ignored, always give a unit; the new expiry
  is the comment time plus the lease, so a short renew can shorten a long lease.
- One zone, one claim, even for the same holder: claim one lock front per lane and let the gate
  ledger the fronts it moots. READY fronts whose zones overlap a claimed lock read `zones-busy`
  until the release; that is expected.
- `withdraw` and `front` need TRIAGE held; `integrated`, `repair`, and `rejected` need MAIN held.
  A records-only push (ledger, docs) is ledgered as `integrated MAIN --merge <sha>`.
- A withdrawn front that other fronts depend on reads as `deps-broken` for them: remint them as
  V(n+1) with the merged dependency dropped under the same TRIAGE hold, then withdraw the old ones.
- `board.py` stores a front's contract as a free string: when a contract group closes but its slugs
  move to another group, a RESIDUAL redirecting the claim is enough, no remint needed.
- Unknown zone names only warn; `python3 compat/board.py zones` lists the real ones.
- `python3 compat/board_test.py` is part of every gate.

## Moving the Claude Code session itself

The repository and the board are the durable state. If you also want the old session's transcript
and memory, Claude Code keeps them under `~/.claude/projects/<checkout path with slashes as
dashes>/`: the session's `.jsonl`, a same-named directory with subagent transcripts and workflow
journals, and `memory/` with the project's auto-memory. Copy them into the matching project
directory on the new machine (clone at the same absolute path or rename the directory to match),
then `claude --resume <session-id>` from that checkout. Workflow journal replay depends on the old
scratchpad paths, so launch the next cycle's script fresh rather than resuming an old run.

## Lore the prompts already encode

Flaky-under-load list (all pass exact-solo): `client_focus_closes_display_panes_and_preserves_chooser_modes`,
`attached_client_extents_clamp_retained_and_default_dimensions`,
`history_request_is_guarded_clamped_and_returns_self_contained_rows`, copy-mode reconcile tests,
`daemon_native_split_resize_commits_exactly_and_rejects_stale_contexts`,
`nested_alias_queue_bubbles_shutdown_and_yield_to_its_parent`,
`control_sourced_run_shell_closes_before_raw_output_and_same_line_continues`,
`wait_exit_holds_the_control_process_until_a_second_blank_line` (hangs under load; timeout-guard
cli_binary runs), `concurrent_default_interactive_attaches_share_session_zero` (headless "not a
terminal", may be misattributed), and `smoke/source-replay-diagnostics` (pin-side crash under
concurrent scenario load; run it solo after sharded gates).

Registry grammar: closing = removing the slug from the group's items (an emptied group moves to
`closed[]`); native decisions = relocate the slug into an accepted-native group with the measured
stance (precedent `1f24a1f1`); park = relocate into a `park`/`blocked` group with the recipe; flag
promotions move `catalog.rs` counters and `compat_manifest_tests.rs` partition counts together
(precedent `c6ce82c4`); a wrong close is reverted and the reason records the refuting measurement
(precedent `0fec342` + `9cab1fa`). `cargo test -p zz-mux` is mandatory after any registry edit, and
`cargo test -p zz-daemon --lib` whenever mux target resolution or effect shapes change.

Reviewer catches worth remembering: proofs gathered before the final commit are worthless (cycle 4);
durable registry resolutions must carry every divergence the worker discloses; doc comments must
stay attached to their fn; a menu width rule that ignored the title seed (cycle 3); a mode reset
built on DECSTR, which the pinned libghostty ignores entirely (cycle 5); a fixture that configured
away the pin's gating (`focus-events on`) and so could not see that the pin emits two focus hooks
where zz emitted ten (cycle 5); a resolution claiming the mark survives cross-session `move-window`
when the pin never retargets `marked_pane.s` (cycle 5). Gate lore: `compat/run.sh --delta --list`
once returned a stale selection, so list twice and reconcile against `git diff --name-only`.
