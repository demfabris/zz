# Capture fix pass, cycle 12

This pass starts from 82746ea3 and rebases its fourteen commits onto origin/main
be5709df (zz 0.10.0). The rebase combines the capture flag count with main's twenty
usage overrides and regenerates both conflicted reports. The branch is
campaign/tui-capture-12. The gate owns verification; this is worker evidence.

## Changes and limits

Styled live capture now tracks text extent separately from retained background
cells. `-J` and `-T` use text extent; ordinary styled capture still reads retained
backgrounds. A SpacerHead contributes default-coloured padding when the capture
keeps allocated cells, and joined capture skips it. SpacerTail remains skipped.
Tests exercise both 80 and 100 columns. The attached probe uses two clients at
80x24 and 100x30, plus a resize, EL, ED, clear, and scroll-region workloads.

The fixture adds four erased-background assertions and three wide-wrap assertions.
Each new scene runs an equivalence and a one-sided text sabotage in `--self-check`.
The existing `lock-client` record changes to a five-channel assertion with a
one-sided status-row sabotage. Other OS-lock decisions remain registered.

Tabs remain an ordinary TUI-017 residual, now named `capture-tab-trailing`,
`capture-tab-internal`, and `capture-tab-wide`. The pin retains a TAB cell and
prints a literal tab; Ghostty stores cursor motion and subsequent cell contents.
This is the same engine-storage root as the campaign's TAB-cell residual. The
DEC charset decision does not waive it. Indexed colour 1 stays recorded for the
sibling lane. This pass does not claim styled frozen-mode capture parity.

## Measurement qualifications

The initial build overlapped the first local edits. Its target resolver already
included the fix, but its capture code still exhibited the rejected behavior.
Do not treat 02-rebased-baseline.txt as a pristine build of f0b70b7c. The initial
binary hash and this qualification are in environment-initial.txt.

The detached probes 04, 06 and 08 did not establish actual 100-column terminal
storage: the model reported a resized window while the terminal could still
retain an 80-column grid. They remain as failed setup attempts. Probe 11 uses
two attached clients and reproduces the wide-wrap defect at 100 columns; later
runs of probe.py use that same attached setup. The probe prints observations and
DIFF lines; exit 0 means the probe completed, not that all comparisons matched.
Its extra default `-e`/`-N` erase measurements at 100 columns remain observations,
not a claim that retained allocation/erase history matches the pin.

Main's command-parse exit policy returns 2 where the pin returns 1 in eight
pre-existing assertions: refresh-missing-argument, lock-server-arity,
lock-server-unknown-flag, lock-session-arity, lock-client-arity,
lock-client-missing-argument, client-tree-unknown-flag, client-tree-usage.
The policy and its tests are unchanged from origin/main (d66501cc,
crates/zz-protocol/src/message.rs ServerError::exit_code and
crates/zz/src/lib.rs cli_exit_contract_preserves_explicit_status_and_classifies_errors).
These assertions remain assertions. No record or decision hides these failures.

The first scrubbed-HOME Cargo attempt (07) started downloading a separate toolchain
and dependency cache and was interrupted (130). Later commands preserve
CARGO_HOME and RUSTUP_HOME while scrubbing application HOME/XDG_CONFIG_HOME.
The wrapper appends --jobs after all arguments, so a temporary cargo adapter moves
that pair before `--`, or removes it for cargo fmt. All Cargo invocations, including
those within compat/check.sh, enter /tmp/zz-cargo.sh first and keep its shared lock
and memory cap. cargo-adapter.py retains the adapter source. runs.jsonl records
command exits; logs retain failed runs.

The first full daemon run passed 935 tests and failed two under parallel load.
Both passed alone using the same freshly built test executable: positive_delay_shell_job_retains_destroyed_target_and_keeps_missing_target_sessionless
and switch_client_key_table_and_formats_are_client_local. Direct integration-test
runs complete the targets that Cargo did not reach after the library failure.

The inherited catalog.rs change adds CLI flags and changes test inventory counts;
it does not change any serialized payload. PROTOCOL_VERSION stays 103.
The checked-out wire guard passes. Contrary to the batch's description, this base's
guard still inspects message.rs alone; the complete source diff was also inspected.

## Attached probe at the fixed candidate

26-attached-probe.txt completes with 42 batch-scope comparisons identical:
18 target-command comparisons, 18 erased-background join/trim comparisons
(including the resize), and six wide-wrap escape/padding/join comparisons.
The full observation matrix also contains 38 differences: thirty tab combinations
and eight ordinary `-e`/`-N` erased-background captures at 100 columns (EL, ED,
clear, scroll-region). TUI-017 owns those eight additional probe observations;
this pass does not claim to fix them. They are not fixture assertions or counted
as flips. The exact stdout, stderr and exit statuses remain in the probe artifact.

Source confirmation for the tab root: Ghostty 20c3eae04dee606349eb21e2dd0293b203d47179,
src/terminal/Terminal.zig `horizontalTab`, advances cursorRight until a tabstop;
it does not retain a tab glyph or origin marker. The pin's grid.c
`grid_string_cells` emits a literal tab for GRID_FLAG_TAB.

## Delta corpus execution

The original 163-row delta run completes its first six rows, then the remaining
157 rows run in three disjoint batches with MemoryMax=2G and MemorySwapMax=1G.
The batch driver pauses only its own original runner while that runner's current
scenario finishes, then terminates the runner without restarting any completed row.
The original runner's termination exit is not a passing corpus result. The manifest
in delta-batches.json and each scenario SUMMARY line establish coverage across the
prefix and batches. Each batch invokes compat/run.sh with the prebuilt binary;
none invokes Cargo. The canonical corpus summary remains unchanged.

The first driver launch exited 1 because --list emits relative scenario paths;
it selected zero paths and asserted before starting any batch. The corrected
launch uses paths relative to compat/scenarios. The current scenario continued
while its original parent waited. No scenario result was discarded.

## Roster text moved out of the shell header

The following twenty-six lines came from the rejected branch's added shell header.
They describe that historical roster; the measurements above supersede its claims.

```text
                                                                                            TUI-015
lock-server/-session/     too many arguments, unknown       same                           PROVED
  -client arity and -t     flag, -t without an argument
lock-session -t with a    validates session, window and    same at the default pane       PROVED, with configured
  window or pane suffix     pane components                   base index                     pane-base-index recorded
                                                                                             under TUI-015
after-lock-session        neither is a hook name: the pin   same                           PROVED
after-lock-client           answers `invalid option`
lock-after-time           arms a per-client server timer    store-only at both scopes,     DECLARED, TUI-015
                                                              and an invalid value is
                                                              refused the pin's way
lock-command              spawned on the client tty         store-only at both scopes;     DECLARED, the pin's
                                                              the pin's own default is a     default is whatever
                                                              build-time choice              configure found
capture-pane -C           backslashes doubled, and with     same for retained cell facts   PROVED; DEC charset and
                            -e escaped style controls                                       low indexed colour
                                                                                             provenance recorded
capture-pane -L           each line numbered from the       same                           PROVED
                            history size, negative in
                            history, and with -J the
                            number of every joined row
                            inside the joined line
capture-pane -F -H -P -R  line flags, the line OSC 8        loudly unsupported             DECLARED, TUI-017
                            URIs, the pending input                                         capture.rich-transports,
                            buffer and the whole internal                                   each with the workload
                            grid                                                            its refusal names
```

## Required checks completed

Clippy passed for zz-mux, zz-terminal, zz-daemon and zz-protocol with all targets,
all features and -D warnings. cargo fmt --all passed through the capped wrapper.
compat/check.sh passed; tracker and structural claim checks passed after the
review records were written. OKF validation reported zero errors and one existing
warning. The package tests retain the existing ignored terminal throughput and
agent streaming soak tests. Both obligation re-measurements returned 1 for the
same eight inherited exact-exit failures; neither obligation is verified.

The corrected attached-client invocation passed. Its first invocation omitted
TMUX_BIN and exited 2 with usage; artifact 27 retains that setup failure.

## Repeated fixture result

All three runs (22, 23, 24) return exit 1 with the same eight inherited failures:

```text
8 of 186 asserted comparisons differ, 32 recorded (0 for a sibling lane, owners TUI-014=6 TUI-015=3 TUI-017=4 decided:TUI-015=4 decided:TUI-016=1 decided:TUI-017=6 gap:clients.interactive-refresh=8 unattributed=0)
```

All eighteen added regressions pass on all three runs. The one recorded-to-asserted
flip is lock-client: its attached channels now assert too. Self-check passes
(25, exit 0), and attached-client passes (29, exit 0). Both verify-claims runs
(32 and 33) return exit 1 because the shared fixture is nonzero.

## Clean main comparison

An isolated git archive of origin/main be5709df was built through the capped
Cargo wrapper into a separate target directory. Source equality and the binary
hash are recorded in main-environment.txt. The main comparison passes
chooser-tree-vocabulary and display-menu-mouse. alias-group-forgery fails on main
both initially and on retry: its CLI unknown-command exit is 2 instead of 1.
The candidate's first-run chooser/menu failures cannot be called inherited on
this evidence; the candidate retry results determine their final disposition.
The raw main scenario logs are retained in main-corpus-observations.json.

The two slow candidate menu scenarios were inspected while running. Their cgroups
had no memory.max or OOM events, with peak usage around 1.1 GB under the 2 GB cap.
Artifact 43 retains the cgroup counters and fixture acknowledgements.

The census-hooks environment dump inherited two credential variables. Their
values are replaced with named redaction markers in the exported observation
artifact. evidence-redactions.json lists names and occurrence counts only. The
scenario SHA-256 still identifies the original local log; the exported text is
redacted. Comparison results and summaries are unchanged.

The pane-base-index fixture reason retains the previous pass's historical
implementation-scope explanation. In this batch those three records remain open
because the user explicitly assigns them to a sibling lane, not because a crate
or function boundary prevents taking a necessary fix.

The three batch runners can overlap each other's retry phase. Their standard
`retrying ... alone` message means one scenario in that runner; it does not claim
that no other capped batch or campaign lane is active on the box.

The shared /tmp/zz-cargo.sh defaults changed during the campaign. The initial
environment snapshot records 5G/3G/three jobs; the final helper reads
4G/2G/two jobs. Explicit build overrides remain in the recorded commands.
All invocations used that shared wrapper and its slots. environment-final.json
records the final helper hash and confirms that the measured candidate binary,
fixture and pin hashes still match after the separate main build.

## Final delta corpus result

All 163 selected rows completed, with zero missing rows and zero geometry
divergences. Twenty-one rows failed initially; eight passed on retry, leaving
thirteen final divergent rows. All three capped batch commands exited 1. The
combined summarizer also exited 1. The retained observations include 172 distinct
complete scenario logs; identical retry logs need no duplicate copy.

Final divergent rows:

- show-options-hooks.txt
- smoke/alias-group-forgery.txt
- smoke/args-parse-choosers.txt
- smoke/cli-chain-parse-abort.txt
- smoke/command-flag-errors.txt
- smoke/daemon-invalid-flags.txt
- smoke/jobs-command-environment.txt
- smoke/plugin-runtime-resurrect-restore.txt
- smoke/positional-maximums.txt
- smoke/positional-minimums.txt
- smoke/source-replay-diagnostics.txt
- smoke/split-window-wait.txt
- smoke/status-background-jobs.txt

Passed on retry:

- smoke/chooser-tree-vocabulary.txt
- smoke/display-menu-mouse.txt
- smoke/copy-mode-refresh.txt
- smoke/display-menu-shortcut-grammar.txt
- smoke/display-menu-cell-layout.txt
- smoke/own-conf.txt
- smoke/plugin-runtime-continuum.txt
- smoke/plugin-runtime-oh-my-tmux.txt

The capture-pane row (34 steps) and targets row (16 steps) are clean.
show-options-hooks includes the already measured build-time lock-command default
(vlock on the pin versus lock -np). alias-group-forgery reproduces on clean main
with exit 2 versus 1. The chooser and menu-mouse rows pass both on clean main
and on the candidate retry. Other final corpus failures were not reproduced
on main in this pass and must not be described as proven baseline failures.
The resurrect restore failure reports the tmux side; shell-job and source-replay
failures remain unresolved observations. The absent completion markers in flag
and positional fixtures remain failures, not waived records.

This is a partial worker delivery. The requested target and capture regressions
pass, tabs have named ordinary records, and the header comment nit is removed.
The shared fixture, both --run claim checks, and the delta corpus are not green.
Both obligations remain at review pending the gate and unresolved failure triage.
