# Capture-12 resumed proof

This attempt follows the usage-limit interruption and reboot. Attempt-05 keeps
its completed measurements and interrupted collectors; it is not rewritten as
proof at this base. The 23 carried commits rebased without conflicts onto
origin/main eef2df941183af8ab7b16bf648dfadcc4486d22e. The source revision for this
attempt is 3209ffb704182b170471c3eac8707f6474d96de9. Both obligations stay at review.

## Source path behind the parked split

`Engine::split_window_with_options` resolves its target with `Engine::resolve_pane`,
which calls `MuxState::resolve_pane_with_index`. For `=splitwait:` there is no dot;
its colon selects `TargetSlot::Classified` for `resolve_window_core`. The session
component `=splitwait` reaches the name lookup with that same slot. It does not
enter the new `TargetSlot::Session` compound parser, allocate the normalization
string, or perform the additional pane/window lookup. The new exact-empty check
compares this component with `=` and returns the original component unchanged.
Main's old bare-ID checks also do not apply to this named target. Neither resolver
implementation changes the selected pane or current window on this path.

The capture changes run on capture requests. `capture_terminal` also serves
`used_visible_rows` for `send-keys -R`; that reset is not part of this split
scenario. Styled capture, text-extent trimming and spacer padding do not run to
resolve or spawn this child. The parked operation is `split-window -W`, which
waits for child exit; this scenario never invokes `wait-pane` or its tail counter.
These call paths offer no plausible added scan or blocking work on the measured
split path. The runtime measurement below checks that source assessment.

## Measurement method

Build the same debug-profile headless `zz-cli` on the candidate and an independent
archive of exact origin/main. Use immutable executable copies for all runtime
proof. The old monolithic executables are not the comparison baseline.

`startup-latency.py` starts isolated servers with both `other` and `splitwait`
sessions. It alternates candidate-first and main-first order, discards two warmup
pairs and retains 40 pairs. Each sample timestamps immediately before launching
`split-window -d -W -t =splitwait:`; the child's first command writes `date +%s%N`,
then blocks on a release FIFO. Thus the measurement includes CLI startup, IPC,
resolution, PTY creation, shell startup and the date command, but excludes the
parent's readiness polling and the parked duration. It also records observation
and pane-query times. Before releasing the child, it requires the caller still
parked, two live panes in splitwait:0 and one pane in the other session. Every raw
sample and warmup is retained. A shared machine cannot establish nanosecond-level
resolver equivalence; the relevant question is whether a candidate startup delay
explains the former fixed 0.4-second observation failure.

## Retained obligations

TUI-017 still owns FOUR ordinary records: capture-low-indexed-colour,
capture-tab-trailing, capture-tab-internal and capture-tab-wide. The three tab
cases share the TAB-cell storage limitation; the DEC charset decision does not
waive them. TUI-015 retains its three pane-base-index records. A later residuals
lane owns their repair. This attempt does not weaken or change these assertions,
records or their attribution.

## Child-start latency result

Forty measured pairs plus two warmup pairs completed successfully. Every one of
the 84 children was alive in splitwait:0 while its caller remained parked; the
other session always retained exactly one pane. All split clients exited zero
with empty stdout and stderr after release.

| Startup milliseconds | Candidate | Clean main |
| --- | ---: | ---: |
| Minimum | 268.765 | 303.506 |
| Median | 384.147 | 383.765 |
| P90 | 617.671 | 670.335 |
| P95 | 638.012 | 717.456 |
| Maximum | 987.666 | 784.641 |
| Mean | 456.418 | 480.470 |

The candidate was slower in 20 of 40 pairs. The median paired difference was
-1.376 ms; the mean paired difference was -24.053 ms. A 10,000-resample paired
bootstrap with seed 20260916 gives a percentile 95% interval of [-107.190,
57.478] ms for that mean difference. There is no measurable consistent candidate
slowdown in this run. The candidate does have the single larger maximum, which
is retained rather than hidden. This does not prove equivalence at sub-millisecond
resolution or under every workload.

The box was shared with other campaign work: one-minute load ranged from 16.41
to 22.48 on 16 logical CPUs. Our shared/attached fixtures were also active; the
samples alternate order to balance gradual load changes. Median pane-query time
was 964.688 ms on the candidate and 1024.474 ms on main; ranges were
727.543–1636.160 and 785.763–1525.207 ms respectively. A fixed 0.4-second delay
followed by such a query can observe the old one-second child after exit. The
readiness/release change removes that unsupported observation deadline while
still checking the live pane's location and the blocked caller. The previous
three paired failures remain evidence of different outcomes in that sampling
window, not evidence of a different resolver target. Attempt-05's deliberately
throttled direct rows and retained-child probes already reproduced the same
expired-pane outcome on both binaries.

No production latency change is justified by these measurements. Retain the
synchronized fixture and its early-return/wrong-window sabotages.

## Split, target and capture proof at this base

The direct strict-geometry split scenario passes three times against the
candidate and three times against independently built main, with the pin on the
other side of every run. All six runs report two steps and zero topology,
geometry, format, output or warning differences. The refreshed sabotages each
produce the expected differential failure: dropping -W exposes early return;
redirecting the split to another window exposes a missing parked pane. The
sabotage driver exits zero only after catching both failures.

The focused delta run passes capture-pane (34 steps), targets (20 executable
steps in the unchanged 21-line file), and smoke/split-window-wait (two steps).
The selection lists 193 rows; only those three are executed in this attempt.
The earlier full and focused corpus measurements remain in attempts 04 and 05,
including their failed collectors and interrupted status-row repeats. This is
not a current-base green claim for the entire corpus. The runner initially
warned that its installed-layout sibling zz_cli was absent next to the immutable
zz copy. No installed-layout row was selected; the identical zz_cli copy was
added afterward for any subsequent runner use.

The capture probe matches all 44 enumerated comparisons in this batch. It still
prints 38 differences: 30 tab comparisons and eight ordinary 100-column erased
background comparisons under the unjoined modes. Those are retained probe
residues, not new asserted passes. The clientless lock probe completes with its
registered decision differences intact. Styled frozen-mode capture is still
unproven.

Main's bare pane/window-ID test and targets.txt are byte-identical to main.
The bare-ID test, compound-session regression test and pane/window fallback
containment test all pass in the full mux run. The shared fixture's only source
change from the previous proof is main's removal of export ZZ_TRAY=0; its case
roster, comparisons and sabotage bodies are unchanged. The initial whole-file
identity check rejected that setup change before the narrower audit recorded it.

## Attached retries

The first complete candidate run exits 1: the retained POPUP_C popup opens but
does not display POPUP_C_DEAD_zz within ten seconds. A complete clean-main run
exits 1 earlier, because its alert expires before the 1.8-second freeze
checkpoint. That different failure does not establish inheritance of the blank
popup. A focused runner sources the unchanged attached fixture through setup
and executes only its full popup function; both clean main and candidate pass.
The complete candidate retry subsequently passes the entire attached fixture.
All four diagnostic outputs and the final full retry are retained. The initial
popup failure was not reproduced in the focused run or full retry; its exact
source cause is not established, and is not labelled an inherited main bug.

## Crate checks and inherited expectation repair

The first complete library run reports daemon 975 passed / 4 failed, mux 533
passed / 2 failed, protocol 229 passed, and terminal 285 passed / 1 ignored.
The four daemon failures are control_background_callbacks_cancel_after_disconnect,
default_shell_rejects_invalid_values_and_unset_controls_the_child_shell,
positive_delay_shell_job_retains_destroyed_target_and_keeps_missing_target_sessionless,
and switch_client_key_table_and_formats_are_client_local. Each passes its own
filtered rerun with the scrubbed home. This is not a full daemon-suite green
claim; the original failed run is retained.

Clean eef2df94 reproduces both mux failures: the new native new-agent-session
command lacks its registry declaration, and the list-commands test expects the
old agent-send usage. Updating that expectation exposes the same test's old
tools usage, which omits main's new section argument. Commit 7c807b92 adds the
native declaration, refreshes those two expected strings and regenerates gaps.md.
There is no runtime implementation change and no alteration of main's session-ID
pins. The first filtered repair run still fails at tools; the second passes all
four selected tests. The full candidate mux run then passes all 535 tests.

Four-crate all-target/all-feature clippy passes with -D warnings. Wrapped cargo
fmt --all passes. Some requests initially waited over half an hour on the same
randomly selected Cargo slot without spawning Cargo. Own childless flock waiters
were stopped and requeued through the unchanged /tmp/zz-cargo.sh; the 143 exits
are queue cancellations, not compiler/test failures. cargo-queue.json and
cargo-requeue.json preserve the exact own PIDs and commands. No compiler or
other lane process was stopped; the wrapper's caps and two shared locks were
unchanged. Successful requeued outputs are retained alongside the cancelled
attempts.

## Final gate and delivery

All three direct shared-fixture runs pass all 192 asserted comparisons with 32
records and unattributed=0. All eight exit-code assertions pass unchanged in
every run. The self-check passes all 98 checks, including the carried lock-client
flip and this branch's compound-target/capture sabotages. Both executable claim
checks exit zero and confirm review with three TUI-015 and four TUI-017 records.

The first compat/check.sh invocation passed its complete mux suite and two of
three daemon manifest tests, then waited on occupied slot 0 for the last test.
Its childless waiter was cancelled, retaining exit 143 and the partial output.
The complete gate was restarted with a task-local Bash initializer setting
RANDOM=1, which makes the unchanged wrapper choose shared slot 1. The initializer
unsets BASH_ENV before Cargo starts; this changes only slot selection, not test
or build environment. The same systemd memory/swap caps, appended job limit and
shared flock still enclose every Cargo invocation. The wrapper itself is
byte-identical to the environment snapshot. The complete restarted gate exits 0,
including all 535 mux tests and all three required daemon manifest checks.

No unmeasured corpus-wide or first-run-green claim is made. The full 193-row
selection was not rerun, the four full-suite daemon failures are retained beside
passing filtered reruns, and the first attached popup failure has no established
source cause despite the passing focused/full retries. The old pre-reboot
status-row classification series remains interrupted in attempt-05. The latency
experiment establishes no consistent current-headless candidate slowdown under
the measured shared load; it is not a quiet-machine performance equivalence
proof. The remaining ordinary capture probe differences and frozen-mode limit
are unchanged.

The ledger and generated report keep both obligations at review. The delivery
footprint includes every three-dot file, including older carried evidence and
all measurement helpers. No PR, main push or board edit is part of this delivery.

Raw screen and byte-diff transcripts intentionally retain trailing spaces and
tabs. A staged whitespace check reports those exact measurements; they are not
trimmed. The non-evidence changes pass the whitespace check.
