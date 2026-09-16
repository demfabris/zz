# Alias-stream correction, cycle 11

The lane starts at 5ae3cef7 and rebases onto origin/main d1694e65. It keeps
TUI-018 at review. The batch authorizes the branch campaign/tui-stream-alias-3,
commits, and its push; this lane does not update the board or open a PR.

The daemon requests stdin when a stream reader executes. The CLI advertises
availability and answers ReadStdin through the existing client-file exchange.
Open and oversized unused stdin stays unread. Earlier group members finish
before a pending reader. The client bounds consumed input at 1 MiB and handles
SIGTERM during the pending read with exit 0. Disconnect leaves the incomplete
payload and following members unapplied.

Control source-read failures emit an empty success frame followed by the
unframed Bad file descriptor diagnostic. The group continues. Missing source
paths still abort, and daemon-start configuration still has no stdin source.

Protocol 104 stays fixed. CommandInvocation appends stdin_available with a serde
default; ClientFileOperation appends ReadStdin. stdin_spent stays serde-skipped.
The existing !raw output guard remains unchanged. Read failures use runtime
InvalidCommand diagnostics with exit 1; there are no new native usage errors.

## Evidence provenance and unsuccessful runs

- baseline-fixture.txt used the stale preexisting executable. A build replaced
  that pathname during the run, so its later protocol errors do not measure a
  coherent revision. The lane stopped the run; it is not proof.
- filter-unsafe-rejected.txt records the workspace lint rejecting the initial
  unsafe signal implementation. The final implementation uses async-signal.
- filter-sinks.txt started fetching dependencies after HOME was scrubbed. The
  lane stopped it and kept CARGO_HOME and RUSTUP_HOME pointing at the existing
  caches for later scrubbed-home commands.
- fixture-first-build.txt could not load libcef.so after the executable was
  copied to /tmp. Later immutable copies stay beside the build's shared libraries.
- attached-client.txt is the invocation that omitted TMUX_BIN and printed usage.
- self-check-01.txt caught 38 sabotages and both equivalences, then exited 127
  because the lane edited the script while Bash was reading it. It is not a pass.
- fixture-01.txt found the single-reader file-replay regression. The CLI test
  found the same refusal. The replay gate now recognizes deferred availability
  as well as supplied bytes. repetition-intervention.txt and delta-intervention.txt
  record stopping repetitions against the superseded executable.
- tests-daemon.txt ran the full 953 tests: 952 passed and one retained-job test
  timed out under load. daemon-flake-solo.txt reran it successfully in 1.07 seconds.
- tests-zz.txt includes the two GUI failures already recorded by the preceding
  lane and the single-reader regression above. Its wait-exit control test hung;
  tests-zz-intervention.txt records terminating that child after three minutes.
  No full-package pass is claimed for that run.

The task-local cargo adapter handles compat/check.sh's Cargo calls and relocates
the shared wrapper's appended --jobs before Cargo's -- separator. For fmt it
removes that unsupported option after the command has entered the shared scope
and slot. Every Cargo invocation passes through /tmp/zz-cargo.sh. The adapter
runs Cargo only; it is not a generic command recorder.

## Final measurements

Code revision: 3d6f16a50e3857929752f31e1e00228b41ca7d84. The immutable
executable target/debug/zz-alias-final has SHA256
`aaf86edc28c00a2ee603103da0fccdb61d6502c07a29a72183c3a78f950077c0`.
The source identity artifact ties this executable to the committed implementation.

- final-streams-1.txt through final-streams-3.txt: each passes 66 asserted,
  zero recorded, four existing cap decisions; exit 0.
- final-self-check-1.txt through final-self-check-3.txt: each catches all
  38 sabotages and passes both equivalences; exit 0.
- final-execution-check.txt: all nine new cases pass; exit 0.
  final-execution-probes.txt preserves hex output and state measurements.
- final-client-commands.txt: 90 asserted, 25 recorded; exit 0. Owners are
  TUI-014=6, TUI-015=4, TUI-017=6, decided:TUI-016=1,
  gap:clients.interactive-refresh=8, unattributed=0. None belongs to TUI-018.
- final-attached-client.txt: attached-client compatibility PASS; exit 0.
- tests-protocol.txt: 231 unit, seven integration and 16 hunt-claims tests pass.
  final-compat-check.txt includes all 535 mux tests and required daemon tests;
  the entire repository compatibility gate exits 0.
- final-tests.txt: all 953 daemon unit tests and daemon integration suites pass;
  all 136 CLI binary tests and other zz integration suites pass. The zz GUI
  library has 662 passes, two failures and one ignored test. The combined command
  exits 101 solely for that library target. The failures are
  command::palette::tests::tab_accepts_completion_without_leaving_the_palette
  and workspace::sidebar::tests::settings_view_is_lazy_and_retained.
  gui-source-blobs.txt proves both source files match origin/main. The preceding
  lane recorded matching failures in attempt-03/test-palette-solo.txt and
  attempt-03/test-settings-solo.txt. No clean zz package pass is claimed.
- final-clippy.txt: zz, zz-daemon, zz-mux and zz-protocol pass all-target,
  all-feature clippy with warnings denied; exit 0.
- final-fmt.txt: workspace formatting passes; exit 0.

The complete delta command ran 222 selected rows and 2,639 command steps,
with no missing or extra rows. Final result: 213 zero-divergence rows, three
exact registered geometry-gap tuples, and six unexpected rows still failing
after the harness retried them alone. The command exits 1. The standalone
attached-client fixture above passed; this delta invocation did not request
its own attached-client rerun.

Persistent delta residuals:

- lane2-store: two output differences for lock-command, zz "lock -np" versus
  the pin's vlock.
- show-options-hooks: seven output differences for that same default.
- smoke/cli-chain-parse-abort: CLI_PARSE_ABORT=broken versus clean:6. Its
  CLI_RUNTIME_ORDER check passes.
- smoke/format-modifier-client-loop: the pin-side fixture expects lexical
  /dev/pts name order from sort, while the pin returns another order. The zz
  side reports clean:21. This row was clean in attempt-03; no prior-failure
  claim is made for it.
- smoke/plugin-runtime-resurrect-restore: the pin's restored shell pane titles
  replace the saved ALPHA/BETA titles; zz reports two-panes-restored.
- smoke/status-background-jobs: zz's drawn DATE status-job assertion fails.

All except the client-loop row were also failures in attempt-03. Seven prior
failures now pass on the rebased branch: alias-group-forgery, args-parse-choosers,
command-flag-errors, daemon-invalid-flags, positional-maximums,
positional-minimums and stderr-parity. This comparison does not attribute
those improvements to this pass; main supplied the exit-code split.

smoke/refresh-status failed its first pass's cached-output assertion and passed
its isolated retry. Both outputs are retained. delta-first-pass/ preserves
all seven initial failure logs before the harness overwrote its result files;
delta-final/ preserves their retry logs, including the refresh pass.
corpus-coverage.json checks every selected row against its final raw SUMMARY,
and corpus-baseline-comparison.json compares the preceding full attempt.

The three assigned stream corrections are implemented and measured. Broad
validation remains red on the six delta rows and two GUI tests above. Keep
TUI-018 at review; do not claim an independent gate approval. This lane did
not repair those other subsystems or run macOS/Windows fixtures.

The control comparisons replace only the timestamp and command-number fields
in %begin/%end/%error markers with TIME and ID. They retain frame order, flags,
error markers, diagnostic bytes, command output, process status and resulting
options. Removing either the continuation output or its state change triggers
a separate sabotage. FIFO cases hold the writer open while querying @before;
the SIGTERM cases separately sabotage the status, payload state and tail state.

## Assertion changes

The previous stream fixture had no recorded rows to flip. This pass adds nine
assertions for the rejected review findings, raising 57 to 66 without removing
an existing assertion. Each new assertion has its own failing self-check:

- file-unused-open-stdin
- file-unused-oversized-stdin
- alias-preceding-state-before-eof
- file-preceding-state-before-eof
- pending-source-sigterm-status
- pending-source-sigterm-payload-unapplied
- pending-source-sigterm-tail-unapplied
- control-source-read-error-continues-output
- control-source-read-error-continues-state

The four preexisting cap decisions remain load-buffer-over-the-cap,
load-buffer-over-the-cap-listed, source-file-over-the-cap and
source-file-over-the-cap-value. They are decisions, not asserted parity or
unowned recorded rows.
