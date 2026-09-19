# TUI-018 seventh pass

Base: `campaign/tui-stream-alias-6` at `834002cf`, merged with `origin/main` `394ef850` (zz 0.11.1).
The merge is a merge commit rather than a rebase: the branch already carries `47f6ab30`, the
0.11.0 merge, and thirty-eight commits whose review provenance is what the sixth review checked by
reverting them. Nothing on main since that merge touches this obligation's files.

Wire stays at **105**, unreleased (`python3 compat/wire-version.py`: `105 is unreleased (v0.11.1
shipped 104); appends are free`). Both appends of this pass go into the same v105 entry.

## The two findings

### BLOCKER 1, the target pane dying mid-stream

`compat/.cache/tmux-src/window.c:2130-2152`. `window_pane_input_callback` looks up the pane first
and only then at `closed` or `error`: with the pane gone it sets `c->retval = 1`, sets `CLIENT_EXIT`
and `file_cancel()`s the read, so the invocation exits 1, prints nothing more and the rest of the
command sequence never runs.

zz now does the same. `stream_caller_stdin_to_created_pane` checks the mux state for the target
before it looks at end of file; when the pane is gone it raises the request's status to 1, publishes
`EventPayload::CommandClientExit` on the Command lane and stops reading. Both queues that could
carry the tail stop on it: the daemon's inserted-command loop
(`execute_inserted_commands_with_control_target_and_mux_source`) breaks the same way it breaks on a
cancelled queue, and the CLI's `\;` chain, which is split client-side and sent as one request per
member, stops in `execute_command_chain`. Four cells assert it, both readers crossed with what the
writer does after the kill.

### MUST-FIX 2, `split-window -I -P` did not stream

Two costs were in front of the caller's bytes and one is not this lane's.

1. The feed started after the whole `split-window` execution returned. It now starts inside
   `execute_with_mux_source_inner`, next to the existing `MuxEffect::PaneStreamInput` feed, as soon
   as the created pane exists.
2. The `-P` line was withheld until the command ended. `cmdq_print` on a command client writes
   through to its stdout while the item runs, so the daemon now releases it as
   `EventPayload::CommandStdout` before the stream opens and takes those bytes out of the request's
   own output, and the CLI prints them through the same `CommandOutputWriter` the chain uses.
3. `-P` on an empty pane waited two seconds in `wait_for_terminal_identity` for a pid and a tty that
   an empty pane cannot have. A pane a caller stream will fill is empty by construction, so that
   wait is skipped for this path only. The general case is a registered gap, not a fix here; see
   below.

## The inherited latency divergence, registered not fixed

`compat/tmux-gaps.json` gains `clients.command-round-trip-latency` (open, adopt, daemon, priority
next). Measured on this box, three runs each, on the pre-fix binary and unchanged by this pass
(probes/split-latency-tip.txt, probes/split-print-empty-pane-cost.txt):

| command | zz | pin |
| --- | --- | --- |
| `split-window -d -t ... -P -F '#{pane_id}' ''` | 2183-2207 ms | 2-3 ms |
| `split-window -d -t ... ''` (no `-P`) | 214 ms | - |
| `split-window -d -t ... -P -F '#{pane_id}' '<shell>'` | 269-364 ms | 2-3 ms |
| `display-message -p` | 88-117 ms | 2 ms |
| `list-panes` | 142 ms | - |

So the "2.25 s split-window" the sixth review measured is `-P` on an *empty* pane and nothing else:
`wait_for_terminal_identity` polls for two seconds for facts that never arrive. The rest is a
per-invocation round trip about two orders of magnitude over the pin. Neither is named by the
observable-results contract, so no TUI clause closes on them; they are a gap for the orchestrator to
schedule.

## Nits

- `compat/tui-command-streams.sh` now reports an environment failure rather than a parity
  difference when the zz side produced no stdout at all where the pin produced bytes, the way a wait
  that never reached its execution point already did. It still fails the run - starvation is not
  proof - but it is labelled `env` and counted separately from the tally the campaign reads.
- `smoke/source-replay-diagnostics`: see the corpus section below.

## Proof

All counted runs use `target/alias7-bins/zz_cli-cand2`, sha256
`66d541d556202e4b1da430b9e9a95b9f9e1912b892bdcc54cc2b8b6ffdd8e544`, built from `1bba089c`. `52100308`
then reflowed three of those hunks with `cargo fmt --all` and nothing else;
`target/alias7-bins/zz_cli-final`, sha256
`44382b63da3503c656860ec592d05fa1c0128335accb4c9da629059615721496`, is that tip, and it takes the
seven new cells clean (new-cases-final-binary.txt) and a fourth whole run of the fixture
(streams-final.txt). The pre-fix comparison binary is
`target/alias7-bins/zz_cli-tip7`, sha256
`cb6bc171142f9619be1ae48ea2ce80f43b5de8f9ec645b7b1ea79dd180340d2b`, built from the merge of
`834002cf` with `origin/main`. Every fixture, probe and corpus command runs with the orchestrator's
credential variables unset; every Cargo command enters `/tmp/zz-cargo.sh`. No environment dump was
taken.

- `compat/tui-command-streams.sh`, three runs at the final fixture (streams-1/2/3.txt): **208
  asserted comparisons identical, 0 recorded, 5 decided**, exit 0 each. The six new cells are
  `stream-split-print-partial`, `stream-split-print-term`, `stream-display-kill-close`,
  `stream-display-kill-write`, `stream-split-kill-close`, `stream-split-kill-write`, plus the
  `pane-death-restored` scene check. streams-pre-restore-1/2/3.txt are three equally clean runs
  taken before `dfe71ab7`, which only adds the scene restore the `--self-check` path needs.
- `--self-check` (self-check.txt): every sabotage caught in its own channel, both equivalences
  passed, exit 0, with six new expectations. The four pane-death cells are sabotaged by not killing
  the pane on the zz side, which has to show up on exit, stdout and state at once; the two `-P`
  cells reuse the delivery sabotage, which has to show up in the state that now carries the caller's
  stdout while stdin is open. self-check-failed-equivalence.txt is the retained failed run that
  found the missing scene restore: every sabotage was caught there too, only the closing equivalence
  reported state.
- Fix reverted: the same six cells against `zz_cli-tip7` (new-cases-baseline-tip7.txt) fail all six,
  and `pane-death-restored` with them. The four pane-death cells fail on all three channels at once
  (zz exits 0, prints `tail-out`, sets `@zzcs-after`); `stream-split-print-partial` fails on the
  caller's stdout alone and `stream-split-print-term` on stdout and state. The candidate run of the
  same six is new-cases-candidate.txt.
- `compat/tui-client-commands.sh` (client-commands.txt): 198 asserted identical, 32 recorded, owners
  `TUI-014=6 TUI-015=3 TUI-017=4 decided:TUI-015=4 decided:TUI-016=1 decided:TUI-017=6
  gap:clients.interactive-refresh=8 unattributed=0`, no TUI-018 record, exit 0.
- `compat/attached-client.sh` (attached-client.txt): PASS.
- Probes (probes/): the split-window latency table above, on the pre-fix binary, the candidate and
  the pin; and the localization of the two seconds to `-P` on an empty pane.
- Tests (scrubbed home, `--test-threads=4`): zz-protocol 232 lib plus its integration targets pass;
  zz-client passes; zz-daemon lib 984 pass with `agent_stream_soak_slow_client` failing 1506 against
  1500, the same inherited flake attempt-10 and attempt-11 recorded on fresh main; zz-cli lib 107
  pass, `cli_binary` 135 pass and 1 fail. That one is
  `control_sourced_run_shell_closes_before_raw_output_and_same_line_continues`, which attempt-11
  already measured as a `%window-renamed @0 cat` race that its own final binary failed 0 of 5 times
  alone before any of this pass existed; it fails 3 of 3 alone here
  (test-sourced-run-shell-retry.txt) and nothing in this pass touches control mode.
- `clippy -p zz-protocol -p zz-client -p zz-daemon -p zz-cli --all-targets --all-features -D
  warnings`, `cargo fmt --all -- --check`, `compat/check.sh` and `python3
  compat/evidence-secrets.py` all exit 0.

## The delta corpus

`compat/run.sh --delta origin/main...HEAD --commands
source-file,load-buffer,display-message,split-window,run-shell,send-text,agent-send --list` selects
**220 rows** (delta-selection.txt), run in three shards against
`target/alias7-final/zz_cli` (the candidate) and re-run row by row against
`target/alias7-base/zz_cli` (the pre-fix tip). No row was skipped and none was left unrun.

- **208 rows zero-divergence.**
- **Three registered `known/` rows**: known-main-preset-two-panes, known-pane-scrollbar-columns,
  known-spread-mixed, all carrying the counts `compat/tmux-gaps.json` already lists under
  `known_differentials`.
- **Eight rows red twice on the candidate and red with byte-identical diff lines on the pre-fix
  binary** (delta-failed-rows.txt against delta-failed-rows-base-tip7.txt), so inherited, not caused
  here: `lane2-store` (2 OUT), `show-options-hooks` (7 OUT), `micro-flags` (1 OUT, the
  `17-Sep-26` against `17-set-26` locale difference), `smoke/cli-chain-parse-abort` (1 OUT),
  `smoke/default-client-command` (1 OUT, 2 WARN), `smoke/plugin-runtime-resurrect-restore` (1 OUT, 1
  WARN), `smoke/status-background-jobs` (1 OUT, 1 WARN, the timing-sensitive `drawn DATE` checkpoint
  attempt-11 measured over eight alternating trials) and **`smoke/source-replay-diagnostics`**.
- `smoke/source-replay-diagnostics` is the sixth review's first nit and it is **inherited, not
  load-sensitive**. Step 60 diverges with the same line on both binaries: zz answers
  `request-rc:124` with one `0:end:_` event where the pin answers `request-rc:1` with eight events
  ending `CONFIRM_OUTER_AFTER`. attempt-11's `delta-shard-1.txt` recorded the row clean, so it was
  green there and red twice here and twice on the pre-fix binary; whatever moved it is not in this
  pass's diff. It belongs on the inherited list.
- **One row moved**: `smoke/plugin-runtime-continuum` was clean in attempt-11 and is red here on
  both binaries. Over the corpus shard plus seven alternating trials the candidate is red in 2
  sessions of 8 and the pre-fix binary in 2 of 8, each red repeating on its own retry once and
  clearing on the other (continuum-trials.txt). The divergence is one field of one pane:
  `#{pane_current_command}` answers `sh` on zz where the pin answers `sleep`. That is the
  intermittent the cycle-9 chooser gate measured and wrote into `compat/tui/campaign.json`:
  `format_cb_current_command` re-reads `/proc/<pgid>/cmdline` on every expansion while zz caches the
  process name per foreground pid, and `execve` does not change a pid. Nothing in this pass touches
  pane runtime facts outside `split-window -I`, and the scenario uses no `-I`. It belongs on the
  inherited list as a second intermittent of this box, beside
  `smoke/status-background-jobs`.

## At the final tip

`cargo fmt --all -- --check`, `clippy -p zz-protocol -p zz-client -p zz-daemon -p zz-cli
--all-targets --all-features -D warnings` and `compat/check.sh` were re-run after the reflow, at the
tip this branch pushes, and all three exit 0 (fmt-check-final-tip.txt, clippy-final-tip.txt,
compat-check-final-tip.txt).
