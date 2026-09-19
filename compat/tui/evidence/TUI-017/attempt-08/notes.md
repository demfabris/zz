# Capture residuals after landing capture-12

The lane rebases onto `origin/main` at `38c50df4`. Main includes the reviewed
empty-exact-window resolver fix, its six asserted cases, and credential
scrubbing in both corpus harnesses. The seven lane commits replay without
duplicating the landed capture-12 work. The tested source is `57160865`.

The resolver conflict needed both changes: strip the empty exact window's
leading `=`, then recurse with the configured pane-index callback. The new
regression test exercises global/window indices, exact and current session
forms, absolute pane IDs, empty pane components and missing indices. The
25-step pin probe in TUI-015/attempt-05 passes all comparison channels,
including `cli:=.1` under `pane-base-index 1` for all three requested commands.
Main's six new assertions remain in the shared fixture.

The engine and vendored sources are unchanged from the previous delivery.
`rebase-audit.json` records that comparison. A fresh reverted archive disables
only the configured resolver entry point, low indexed colour spelling and the
plain-capture tab renderer. Its native provenance patch remains present.
`reverted-source-audit.json` compares 453 source, manifest, lock and patch files;
only the two intended files differ. All seven fixed checks pass and all seven
reverted checks exit 1. After that build, the production rebuild compares
byte-for-byte equal to the immutable fixed binary.

The full mux and terminal packages pass 927 tests, with one ignored terminal
test. Five-crate clippy with warnings denied, formatting, the compatibility
gate and wire guard pass. Protocol 104 remains inherited and unreleased; this
lane adds no payload change. `build-slot-waits.json` records cancelled childless
waits and capped requeues. No running compiler or test was cancelled.

No environment dump was collected. `execution-context.json` contains selected
build facts. Raw fixture comparisons use the repository harnesses; source and
result audits read only the named artifacts.

## Scope and retained failures

The first exact-window probe renamed the harness's `w` session. All command
results matched, but its topology and geometry queries then failed on both
sides. The corrected probe creates a separate `cli` session. Both runs remain
in TUI-015/attempt-05.

The first full attached run failed because zz's alert had expired before the
1.8-second freeze checkpoint. At that point other fixtures were running.
That observation does not establish a cause or a runtime fix. The failure and
daemon diagnostics remain in `attached-client.txt`.

The follow-up reruns the 20-step target corpus and 34-step capture corpus;
both pass. It does not repeat the earlier full 205-row delta. `delta-scope.json`
names the 203 rows not rerun here. Attempt-07 retains the full earlier run and
its eight persistent differences. This follow-up also does not repeat the
five-package integration run or the wider capture probe. Their unresolved
limits remain: the integration failures and interrupted wait-exit children,
eight 100-column erased-background differences, styled frozen-mode capture,
shrinking/reflow coverage and large-history capture throughput.

Both obligations stay at review. The lane does not grant verification.

## Completed comparisons

Three executions of the shared fixture each pass 205 assertions with 25
records. The first runs the fixture directly; the second and third run it
through the TUI-015 and TUI-017 claim verifiers. The verifier prints a truncated
summary, then its complete owned-record result; it checks attribution against
the full captured output. Both report zero ordinary records of their own.
The direct log retains the full tally: TUI-014=6, decided:TUI-015=4,
decided:TUI-016=1, decided:TUI-017=6, gap:clients.interactive-refresh=8,
unattributed=0. These are the same 25 records as attempt-07; the six assertions
added on main raise the asserted count from 199 to 205.

The shared self-check passes 124 expectations. Copy-mode passes all 147
comparisons with zero records and all 27 self-check expectations. Screen-diff
passes all 147 assertions with six recorded decisions and all 18 self-check
expectations. `fixture-summary.json` lists commands, artifacts, results and
exits; `records.json` retains the shared cases and their reasons.

The full attached retry passes, including detach/reattach. This lane had
finished its other fixtures before starting it. The binary and timing checks
are unchanged. This proves a passing run at the rebased source; it does not
establish why the earlier alert observation failed or claim a runtime fix.

`footprint.json` lists the complete three-dot delivery footprint against main,
including retained evidence. `footprint-check.txt` records the path audit.

The whitespace check passes for source and ledger metadata. Raw capture and
self-check transcripts retain trailing tabs, spaces and blank lines as measured;
the all-files whitespace check reports those bytes. They were not stripped.
