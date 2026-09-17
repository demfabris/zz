# Repair the parked split observation and prove the exit-code dependency

This follow-up starts at 4c4c142b on e40a13e1. The first fetch left main at
e40a13e1. A later fetch brought in the exit-code lane and its follow-ups through
d1694e65. The rebase completed without conflicts; 0bab4e9c contains the fixture
repair on that base. environment.json records both stages and binary hashes.
TUI-015 and TUI-017 remain at review. The gate decides verification.

## Source cause of the split-window observation

The first divergence in the original scenario is step 1, at
`parked returned=[] panes=1`; the pin prints two panes. The scenario never calls
`wait-pane`. Its `split-window -W` waits for process exit, a different operation.
Main's wait-pane tail change does not participate in this test.

Both split-window and list-panes target `=splitwait:`. split_window_with_options
calls resolve_pane, which passes TargetSlot::Classified into resolve_window_core.
The session component is `=splitwait`, not the empty exact component `=`.
The new TargetSlot::Session compound dispatch therefore does not run. Both
versions select the named session's active window and its active pane.
resolver-path.txt retains the source paths and the main-to-candidate resolver
diff. Runtime all-window listings show the split in splitwait:0; the other
session's pane stays unchanged.

The fixture starts a one-second child, waits 0.4 seconds from launching the
CLI, then starts a separate list-panes CLI. Neither delay bounds CLI startup
or query latency. A query can reach the daemon before the split starts or
after its child exits. The missing pane is an observation race in the fixture.
No production resolver or wait-pane change is needed for this target.

The evidence separates the original scenario from diagnostic probes:

- The unchanged original scenario passes twice on each binary: once directly
  and once with MemoryMax=2G/MemorySwapMax=1G. Artifacts 02 and 04 retain both
  summaries. The first driver's per-step files were overwritten by the second
  successful pair; the retained scoped transcripts belong to artifact 04.
- With CPUQuota=25%, the unchanged original scenario fails once on each binary
  at the same parked-pane line. Artifact 06 and throttled-transcript-* retain
  the full step transcripts. The collector exits 0; both scenario children
  exit 1. This is an actual clean-main reproduction of the full row.
- The ordinary lifetime probe runs three normal and three remain-on-exit
  observations per binary. All 18 query results contain two panes.
- The throttled lifetime probe also catches queries before child startup.
  Its remain-on-exit listings retain the child in the correct window.
- The started-child probe waits for an actual child-written timestamp before
  the 0.4-second delay. Under throttling, all three candidate and all three
  clean-main queries return one pane after the child exits. Their CLI queries
  take 3.401–4.514 and 3.901–4.890 seconds respectively. With remain-on-exit,
  all three observations on each binary show two panes, including the dead
  child in splitwait:0. The pin's six queries return two panes. Raw timestamps,
  outputs and all-window listings are in lifetime-started.json; the derived
  counts are in lifetime-summary.json.

These measurements supersede attempt-04's conservative caused-here label with
a fixture race shared by clean main. They do not reconstruct the scheduler
state of the earlier three candidate-only failures. Those failures remain in
attempt-04. CPU throttling makes the faulty timing assumption reproducible
without generating a memory or CPU load on the box.

## Fixture repair and sabotage

The parked child now writes a readiness marker and waits for a release marker.
The parent waits for readiness, counts the panes, checks whether the waiting
client returned, and then releases the child. Both waits have a 300-iteration
bound with 0.1-second sleeps. The output contract and post-exit pane check stay
the same. No added code comments accompany the change.

The fixed scenario passes three pairs on the pre-rebase candidate and clean
e40a13e1: one ordinary pair and two CPUQuota=25% pairs, six successful scenario
executions. Each still compares against the pin. Artifact 11 catches both
one-sided sabotages: removing -W prints `parked returned=[0] panes=2`; directing
the split to another session prints `parked returned=[] panes=1`. Both corrupted
scenario runs exit 1. The sabotage driver exits 0 only after catching both.

## Exit-code dependency

The detached scratch worktree starts at 4c4c142b. It applies the nine crate-file
changes from sibling commit 888be5af, including the supporting catalog, daemon
and CLI changes. exitcode-source.patch and exitcode-apply.txt preserve the
exact patch and clean application. The capped scratch build passes.

Artifact 05 runs the complete unmodified capture-12 shared fixture against that
scratch binary. All eight named failures go green: refresh-missing-argument,
lock-server-arity, lock-server-unknown-flag, lock-session-arity, lock-client-arity,
lock-client-missing-argument, client-tree-unknown-flag and client-tree-usage.
The summary is 186 asserted comparisons identical, 32 recorded, unattributed=0,
with exit 0. exitcode-dependency.json records every exact passing line.
No assertion was weakened. Main subsequently landed that implementation as
81c971f1; the final rebase incorporates it rather than carrying a duplicate.

Protocol 104 now comes from main's exit-code change. This follow-up adds no
serialized field or variant and performs no further version bump.

## Measurement limits and retained failures

Artifact 14 began before the final rebuild and continued after its CLI path
changed from protocol 103 to 104. Its existing daemon still spoke 103. It exits
1 with protocol-mismatch diagnostics and is invalid as a parity measurement.
Fresh runs use an immutable executable copy. The first copied-binary attempts
(19–22 and 24) fail at startup because its new directory lacks libcef.so;
they exercise no assertions. Linking that directory's libcef.so to the existing
worktree build dependency fixes the setup. The failed logs remain here.

TUI-017 still carries FOUR ordinary owned records: capture-low-indexed-colour,
capture-tab-trailing, capture-tab-internal and capture-tab-wide. The sibling
must clear all four before verification. Tab provenance shares the existing
TAB-cell storage limitation and has no DEC-charset waiver. TUI-015 retains
its three pane-base-index records. The ordinary 100-column erased-background
probe differences and styled frozen-mode capture proof limit also remain.

The previous full 163-row corpus, attached-client retry, package tests and
failure classifications remain in attempt-04 at e40a13e1. Fresh final-base
results below distinguish themselves from those historical measurements.

The current-main baseline uses a separate git archive and build directory.
Its harness receives the same repaired split fixture, with no Rust-source
changes. Its compat/.cache is a real copy. Separate result directories keep
candidate and main transcripts from overwriting each other.

## Final-base proof

All three expanded shared-fixture runs (25–27) exit 0 with the same tally:

```
all 192 asserted comparisons identical, 32 recorded not asserted (0 for a sibling lane, owners TUI-014=6 TUI-015=3 TUI-017=4 decided:TUI-015=4 decided:TUI-016=1 decided:TUI-017=6 gap:clients.interactive-refresh=8 unattributed=0)
```

Main supplies the six assertions added since the 186-case scratch dependency
measurement. This lane does not claim those as its own flips. lock-client
remains the sole recorded-to-asserted flip carried by the full branch; this
follow-up adds no further flip. records.json names all 32 remaining records.

The final-base self-check (28) passes 98 checks and exits 0. The fresh attached
fixture (29) prints `attached-client compatibility: PASS` and exits 0. The
final-base split scenario (32) also passes with CPUQuota=25%. The capture probe
(23) matches all 44 enumerated batch comparisons, with the same 38 residual
observations retained. This enumeration includes wide-wrap join and trim;
its explicit list is in capture-summary.json. Lock probes (30) complete with
the documented OS-locking decisions; exit 0 is collection completion, not a
claim that those decisions vanished.

The full daemon library run (31) passes 949 tests and fails two:
positive_delay_shell_job_retains_destroyed_target_and_keeps_missing_target_sessionless
(`retained job did not finish`) and
loopback_forwards_http_and_tcp_in_both_families_with_original_port (`AddrInUse`).
Each passes alone in 36 and 37. Cargo stops the multi-package command after
the daemon failure, so 38 separately runs the remaining libraries: mux 535,
protocol 229, terminal 280 passed with one existing ignored throughput test.
Clippy for all four crates, all targets and all features passes with -D warnings
(35). Formatting (43) and the wire guard (42) pass. Version 104 is inherited
from main and remains unreleased.

Both executable claim checks (40 and 41) exit 0 against the final immutable
binary. Each runs the shared fixture again, gets all 192 asserted comparisons
identical, and explicitly accepts the review status with three TUI-015 records
and four TUI-017 records. Neither obligation has been promoted to verified.

The final-base compat/check.sh run (45) exits 0, including the full mux library
and required daemon manifest checks. Its elapsed time includes shared-slot
queueing behind another lane. Every Cargo command runs through /tmp/zz-cargo.sh;
the check script uses an exported wrapper function and the existing argument
adapter for Cargo's appended --jobs placement.

## Focused corpus and retry details

The final-base selection lists 193 rows. Artifact 34 executes 15 explicit rows:
the thirteen prior failures plus capture-pane and targets. Nine pass; six
remain different after the built-in retry. Capture-pane passes 34 steps,
targets passes all 20 steps, and split-window-wait passes both steps. This is
a focused rerun, not a fresh run of all 193 selected rows. The historical full
163-row run is retained in attempt-04.

The clean d1694e65 archive runs nine relevant rows in artifact 44. The four
stable failures reproduce with exact diff lines: show-options-hooks,
cli-chain-parse-abort, plugin-runtime-resurrect-restore and
source-replay-diagnostics. Its jobs-command-environment and status-background-jobs
first passes match the candidate first passes, then both pass on main's retry.
The candidate's jobs retry narrows to positive-destroyed; main's first pass
contains that failure plus positive-missing. This establishes the shared
failure but does not establish exact whole-row equality for the final retry.
The candidate's status retry instead fails DATE refresh, so that checkpoint
gets its own direct repeats rather than an inherited label based on a different
checkpoint. Both complete runner logs retain the initial failed excerpts.

The candidate runner's footer says `Nothing failed on the first pass` despite
six failures and exit 1. Its retried list only counts failures that later pass;
none of the candidate failures did. The per-row tables, summaries and command
exit statuses carry the actual result. Artifacts 46 and 47 retain failed
collection attempts: their equality checks rejected the narrowed jobs failure
and changed status checkpoint. They are collection failures, not additional
product test runs.

## Interrupted continuation

The usage limit stopped this continuation before the status-job repeat series
and corpus collector finished; the box subsequently rebooted. Only the first
status repeat transcript on each side was retained. No three-repeat result or
successful final corpus collection is claimed. The completed fixture, test and
compatibility checks above remain evidence at 0bab4e9c/d1694e65. Attempt-06
supersedes the delivery measurement after the reboot and next rebase, and adds
the requested child-start latency distributions. The handshake evidence alone
does not rule out a candidate startup-latency regression.
