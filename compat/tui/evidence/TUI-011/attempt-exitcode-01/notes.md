# Exit-code contract split, cycle 11

Lane: `exitcode`. Branch: `campaign/tui-exitcode`.
Base: `be5709dfcd9936d44519683e5da8dc6ab7748d96` (zz 0.10.0).
Pin: tmux next-3.8, `d77c9dc6aa021e4bc61f0da128c591af695e6466`.

The three red client-command assertions are green. Tmux-compatible usage errors now exit 1;
zz-native usage errors exit 2. This attempt does not mark TUI-011 verified: inherited records,
unrelated runtime failures, and an unresolved documentation scope conflict remain.

## Implementation

The catalog marks native options and classifies each error. `list-panes -Z`,
`list-panes --json -Z`, and `list-panes --json -F` exit 1. The extension conflict
`list-panes --json -F x` and malformed extension `--json=true` exit 2. Pure native verbs retain
their native status through existing catalog metadata. Global tmux usage and unknown commands
exit 1; native help and native global/attach extensions keep 2.

`NativeCommandParse` carries native parse failures. `NativeUnsupportedCommand` preserves 2 for
native option refusals such as `set-option -a history-trickle`. `NativeInvalidCommand` preserves
the runtime phase when bind-key or untyped confirm-before promotes an invalid native callback.
That last case previously returned 1 despite originating in a native usage error; it now returns 2.
Typed callbacks remain parse failures. Diagnostic rendering, config replay, and Control guard
classification preserve their previous behavior. No command diagnostic or screen text was edited.

The pin returns 1 for an unknown command and for `clock-mode -t %999999`, but 0 for `clock-mode`
with a valid pane. zz still refuses the unimplemented command; its refusal now exits 1. Its
existing stderr and unsupported behavior remain a TUI-014 divergence.

Protocol 103 shipped in 0.10.0. This branch opens 104 and appends ServerError tags 14, 15, and 16
without moving existing tags. Both hello byte pins use 0x68. Sibling 104 changes must share one
version-history entry at integration; the observed sibling refs did not occupy these error tags.

## Proof

The immutable final functional binary is `target/debug/zz-exitcode-proof-v2`; its SHA-256 and
build environment are in `environment.txt`. Earlier proof binaries and their failed/provisional
runs remain evidence, but do not substitute for the v2 results.

- Three v2 client-command runs pass, each with 90 assertions and 25 records:
  `owners TUI-014=6 TUI-015=4 TUI-017=6 decided:TUI-016=1 gap:clients.interactive-refresh=8 unattributed=0`.
- The client self-check catches every sabotage. The status sabotages reject tmux exit 2 and
  native exit 1, including the native callback case.
- The full v2 attached-client fixture passes. The superset self-check passes.
- The v2 superset runtime exits 2 on sidebar withdrawal; the freshly rebuilt baseline reproduces
  that timeout. This is a TUI-012 residual.
- All 43 v2 probes meet expected statuses and preserve exact stdout/stderr against the retained
  baseline dataset. The freshly rebuilt base binary independently passes all 43 original dataset
  expectations, strengthening that output-preservation measurement.
- A callback-chain probe preserves stdout/stderr and before/after effects while native binding
  usage changes from baseline 1 to candidate 2. The daemon test checks typed/untyped confirm phases.

No recorded case was flipped. The restored cases `refresh-missing-argument`,
`client-tree-unknown-flag`, and `client-tree-usage` were already asserted. Six new assertions are
`native-verb-usage`, `native-bound-usage`, `native-json-format-usage`, `native-option-usage`,
`tmux-unknown-flag`, and `tmux-unknown-command`; each has a matching sabotage.

The inherited 25 records keep their owners. The four lock cases assert CLI channels and currently
match screen/state too, but this lane adds no lock-surface promotion or sabotage. The messages-log
record is a clause-mandated `decided:TUI-016` registration, not an open charge to that owner.
`recorded-cases.json` in the raw archive preserves every case name, owner, and reason.

## Package checks

The full four-package command ran with scrubbed HOME/XDG and one test thread, under the capped
cargo wrapper. It exits 101:

- zz-protocol and zz-mux unit, integration, and doc tests pass.
- zz library: 660 pass, two fail, one ignored. Palette completion and lazy settings-view failures
  reproduce independently against freshly rebuilt base source.
- CLI integration: 130 pass, one induced wait-exit failure. All exit-code expectations pass.
- daemon: 937 pass, one failure in `tools_skill_matches_workspace_skill` (scope conflict below).

The wait-exit test hung after emitting `%exit`. Its owned child was terminated after a diagnostic
timeout so later tests could run; this is not a clean assertion result. A baseline/candidate probe
at the test's 500 ms acknowledgement delay passes baseline and hangs candidate; five more baseline
runs at that delay pass. At 10 ms, all five fresh-baseline runs hang too. The unchanged exit-drain
loop can discard the acknowledgement and EOF before the wait-exit handler starts. This establishes
a baseline timing flaw, but does not reproduce the exact original-timing baseline failure. Neither
the control-flow implementation nor its test was changed or skipped.

The requested workspace skill table is also generated `zz tools` output. Updating the skill while
freezing all printed text makes the synchronization test fail. The user was asked to resolve that
conflict; no exception was assumed. `pending-tools-help.patch.txt` in the raw archive is the concrete
help-only patch, not applied. The skill/documentation edits are present; daemon help text is unchanged.

Final gates pass: `compat/check.sh`, clippy on all four touched crates with all targets/features
and `-D warnings`, `cargo fmt --all`, the wire-version guard, the campaign tracker, OKF validation,
and skill metadata validation. Cargo still runs only through the capped wrapper. The protocol
version test name was corrected to 104 after the full run; its focused integration test and a
second formatting pass pass. The implementation commit is `888be5af`.

The final corpus ran all 254 selected rows, totaling 3,083 steps. 249 rows match the harness
criteria, including four registered known scenarios. Five rows remain red:
`lane2-store`, `show-options-hooks`, `smoke/cli-chain-parse-abort`,
`smoke/plugin-runtime-resurrect-restore`, and `smoke/status-background-jobs`.
Batch exit statuses are 0, 1, 1, 1. No scenario was skipped. Six first-pass failures recover on
retry: menu-cell-layout, menu-shortcut-grammar, jobs-command-environment, plugin-runtime-oh-my-tmux,
source-file-diagnostics, and source-replay-diagnostics. `corpus-results.json` contains every row's
final counters and expected known tuple. Every one of the 254 final archived logs was checked
byte-for-byte against the completed result file before packaging.

## Corpus and residual controls

The initial default delta selected 144 smoke rows; against the implementation commit it selects 145 rows. Catalog command/alias terms expand it to every one of
254 scenario rows. The final run partitions those rows into four disjoint batches, each capped at
768 MiB RAM and 512 MiB swap, using the fixed v2 binary and a real plugin-cache directory. No
compatibility invocation builds zz. Retries run sequentially within each batch, without exclusive
use of the host. The scenario search found no CLI usage expectation encoding the old exit 2.

Residual controls and provisional failures:

- `lane2-store` and `show-options-hooks`: zz prints `lock -np`, the Linux pin prints `vlock`;
  statuses are 0. The fresh baseline reproduces the value. Owner: TUI-015.
- `smoke/cli-chain-parse-abort`: all six live checks pass; eleven cold candidate and pin statuses
  are 1 and no daemon is created. Nine cold diagnostics retain zz's parse error where the pin
  prints a missing-socket error. Baseline printed the same parse errors with status 2. The status
  is fixed; diagnostic precedence is outside this lane. Owner: TUI-011.
- `if-shell-background-order`: the earlier run and fresh baseline differed in scheduling/output
  order; the final v2 row passes. This is historical evidence, not a final residual.
- `smoke/source-replay-diagnostics`: step 60's internal confirmation request times out in zz (124)
  and returns 1 with callback events in the pin; both outer queries exit 0. A separate fresh-base
  run reproduces the difference exactly. The final v2 retry passes, so this is retained flaky
  evidence rather than a final residual. Follow-up owner: TUI-018.
- Job-environment and menu-cell-layout failures pass on the final retry. Source-file diagnostics
  and oh-my-tmux also pass on retry, as do menu-shortcut-grammar and source-replay-diagnostics.
  Their failed first runs remain archived.
- Resurrect-restore fails on the pin side: zz restores ALPHA/BETA titles, while the pin's restored
  titles become shell-prompt strings. This is a fixture/environment residual, not a failed zz restore.
  Follow-up owner: TUI-003.
- Status-background-jobs fails on the candidate's first drawn expansion. The fresh baseline fails
  later at DATE redraw timing, so this is not an identical baseline reproduction. Follow-up owner:
  TUI-004. The exact candidate assertion remains unisolated.

## Test expectation changes

Tmux-path CLI expectations change 2 to 1 in `unknown_tmux_flag_uses_tmux_usage_shape`,
`usage_errors_keep_their_surface_status_without_a_daemon`,
`non_start_server_commands_do_not_spawn_a_daemon`,
`native_attach_accepts_targets_and_stops_options_at_positional_sessions`,
`prepared_cli_chain_rejects_later_alias_parse_errors_before_effects`,
`prepared_cli_chain_rejects_later_unaliased_argument_errors_before_effects`,
`cold_cli_parse_errors_do_not_start_or_mutate_a_daemon`,
`arbitrary_startup_alias_cannot_trigger_cold_autostart`,
`failed_preflight_handshake_still_runs_the_cold_static_gate`,
`invalid_startup_alias_shadows_abort_before_cold_effects`,
`native_attach_flag_errors_match_tmux_for_both_spellings`, and
`target_and_unknown_command_errors_match_pinned_tmux_stderr`.
Native help/JSON conflicts still expect 2. Existing text assertions are unchanged.

Native expected variants change in mux reload/import tests, split-picker hunt coverage, catalog
absent-flag coverage, the native binding callback test, daemon wait/run validation, and browser
capture validation. New protocol/catalog/mux/daemon/CLI tests cover error source, wire tags,
round trips, callback phase, live daemon behavior, and cold CLI behavior.

## Evidence and execution limits

Every cargo call goes through `/tmp/zz-cargo.sh`. The retained cargo adapters move its appended
`--jobs` before a `--` separator or remove it for fmt; compat/check's internal cargo calls enter
the same wrapper. The shared cap changed from 5 GiB/3 jobs to 4 GiB/2 jobs during the run.
CARGO_HOME and RUSTUP_HOME preserve installed caches while daemon tests scrub HOME and XDG.
Two compatibility-check attempts and one clippy attempt were cancelled while queued before cargo
started: the wrapper repeatedly chose the occupied slot while the other slot was idle. A local
adapter then retried only queued, childless scopes with no cargo-start marker after 15 seconds;
every attempt still uses the unchanged shared wrapper, locks, and memory caps. Once cargo starts,
its result is preserved. Both adapter versions and queue-cancellation logs are retained.

Runner logs from failed and provisional attempts are retained. These include initial test setup mistakes, a fixture
edited during a provisional run, cancelled queued jobs, stale baseline build-cache compilation,
a plugin-cache setup lock collision, an incorrectly traced stderr comparison, an initial missing
attached-client argument, an incorrect callback-probe expectation, and the induced hang failures.
Baseline controls were later rebuilt in a separate cache from a clean base archive. No failed run
is silently counted as passing. The harness overwrites per-row files on retry; available v2 first-failure
files and final corpus snapshots are archived, but not every overwritten early per-step file survives.

The fresh baseline's 43-probe validation uses the original dataset as expected data and asserts
exact equality. Its runner metadata overwrites a transient result-array filename; the original
dataset, executed helper, and complete assertion log remain retained. Earlier full client runs
have capture archives; v2 client runs retain fixture output rather than separate per-case captures.

At packaging, raw files are consolidated only after the measurements finish. Archive contents are
verified byte-for-byte before redundant loose files are removed. `measurements.json` records every
runner command/status. `fixtures.txt`, `tests.txt`, and `corpus.txt` are readable aggregates with
normalized line endings and trailing whitespace; exact original output remains in the raw archives. The
raw/corpus/capture archives retain detailed probes, controls, helpers, and failures.

The literal `compat/run.sh --delta --list` form consumes `--list` as its range and starts a run.
That extra invocation was stopped after four completed rows and one in-flight row; the exact
process tree and runner output are retained. An explicit `--delta origin/main..HEAD --list`
correctly lists the selection. All five affected result files were then re-measured in a capped
run using the final binary, with five clean rows and exit 0, before final archive creation.
