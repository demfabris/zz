# TUI-018 seventh pass, handoff for the reviewer and the gate

Branch `campaign/tui-stream-alias-7`, based on `campaign/tui-stream-alias-6` (`834002cf`) merged
with `origin/main` `394ef850` (zz 0.11.1). Wire stays at 105, unreleased; both appends of this pass
go into the existing v105 entry.

## What to re-measure first

1. `compat/tui-command-streams.sh` with `ZZ_BIN` pointing at a binary you built yourself. It must
   end `all 208 asserted comparisons identical, 0 recorded not asserted, 5 decided (0 for a sibling
   lane)`. The six cells this pass adds are `stream-split-print-partial`,
   `stream-split-print-term`, `stream-display-kill-close`, `stream-display-kill-write`,
   `stream-split-kill-close` and `stream-split-kill-write`; `ZZ_STREAM_MATRIX_FILTER` accepts
   `'split-print|kill-close|kill-write'` with `--matrix` if you only want those seven.
2. Revert `1bba089c` and run the same filter. All six must go red, four of them on exit, stdout and
   state at once. new-cases-baseline-tip7.txt is that run against the pre-fix binary.
3. `--self-check`. Both equivalences must pass; the pane-death sabotage is "do not kill the pane on
   the zz side", which is only one-sided if the scene is restored afterwards, which is what
   `dfe71ab7` adds.

## What this pass claims and what it does not

It claims the two review findings are fixed and asserted, and that `compat/tui-client-commands.sh`
still reports `unattributed=0` with no TUI-018 record.

It does not claim the latency divergence is fixed. `wait_for_terminal_identity` is skipped only for
the pane a caller stream fills; `split-window -d -P ''` still takes 2.2 seconds, `display-message
-p` still costs about 100 ms, and both are registered as
`clients.command-round-trip-latency` in `compat/tmux-gaps.json` with the measurements that localize
them. If the orchestrator would rather have the general guard here, it is a two-line change in the
`MuxEffect::PaneFormatOutput` arm, but it changes the gap's own numbers and was reserved for
another lane by this batch.

## Residuals a reviewer will see

- `cli_binary::daemon_autostart::control_mode::control_sourced_run_shell_closes_before_raw_output_and_same_line_continues`
  fails 3 of 3 alone. attempt-11 measured the same test failing 0 of 5 alone on its own final
  binary, before this pass; it is a `%window-renamed @0 cat` race in the control transcript and
  nothing here touches control mode.
- `agent_stream_soak_slow_client` fails 1506 against 1500. Same inherited flake attempt-10 and
  attempt-11 recorded on fresh main.
- `smoke/source-replay-diagnostics` and the rest of the delta corpus: see notes.md.

## The wire

Two `EventPayload` tail appends, `CommandStdout { output: RawText }` and `CommandClientExit`, both
after `ChooserPresentation`, pinned by
`released_command_stdout_and_client_exit_append_after_the_chooser_presentation` and described in the
v105 entry of `knowledge/protocol/wire-protocol.md`. `CommandResponse` is untouched, so the 236
construction and pattern sites of `CommandResponse::Success` did not move.
