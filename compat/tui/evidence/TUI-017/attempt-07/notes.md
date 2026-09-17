# Capture residuals, cycle 11

Seven ordinary records become assertions: TUI-015's three configured pane-index
targets and TUI-017's explicit low indexed colour and three tab captures. Both
obligations stay at review. The gate owns verification.

## Implementation

`MuxEngine::resolve_session` supplies its existing configured `pane_at_index`
lookup to `MuxState::resolve_session_with_pane_index`. Compound parsing remains
behind `TargetSlot::Session`; internal window and pane fallbacks still use the
name-only path. Engine and daemon session-target callers use the configured
entry point. Absolute pane IDs do not pass through the configured index.
The 21-step `TUI-015/attempt-04/pane-index-scopes.txt` probe also checks option
scope routing, override/unset inheritance and the independent window base index.

Native Ghostty cells already move with scrolling, copying, reflow and erasure.
Their spare bits can carry provenance without a Rust coordinate side table.
The local native patch stores a tab head width and continuation offsets in eight
spare cell bits, and the indexed background class in another bit. Two spare
style bits retain explicit indexed foreground and background classes. Cell and
native style allocation sizes do not increase. The C style uses existing tail
padding and two appended cell query tags expose the facts to the safe wrapper.
The safe wrapper is vendored at the same already-pinned release, with its source
comments omitted under the repository contribution rule.

HT tags only a uniform span of blank, narrow, unlinked cells up to 32 columns,
following the pin's compact tab-cell rule. Capture checks every continuation
before emitting HT; a partial overwrite invalidates the span. Ordinary narrow
cells remain visually blank. Tests cover trailing, internal and wide-text tabs,
pre-existing text, partial overwrite, erase, byte-at-a-time input and widening
from 80 to 100 columns. They do not establish every shrinking/reflow case.

Plain capture now scans the requested cells for HT provenance before choosing
the native formatter or the per-cell capture renderer. This adds a linear scan
and C API calls proportional to the requested cell count. The styled renderer
also reads one row of tab tags. There is no new persistent per-cell allocation
or duplicate grid, but this is not a throughput benchmark for large histories.

The first implementation published colour-class bits to clients. The attached
screen fixture exposed why that was wrong: the pin preserves `38;5;1` in capture
output but draws that low index using named red. Five screen checkpoints failed.
The final implementation retains provenance only in the terminal worker; the
frame model, wire codec, TUI renderer and mobile decoder are unchanged by this
lane. The existing colour scene now includes low indexed red as well as named,
high indexed and RGB colours. Its final 147 asserted checkpoints pass.

## Executable proof

Three initial shared-fixture runs and three final-source runs each report 199
asserted comparisons, 25 records and `unattributed=0`. The latter three comprise
one direct run and the two obligation verifier executions. The former counts
were 192 assertions and 32 records. TUI-015's own count falls by three to zero;
TUI-017's own count falls by four to zero. `records.json` preserves the other
25 records and their owners; `flipped-cases.json` names the seven changes.

The ordinary `--self-check` catches every sabotage. The bounded seven-case
`residual-self-check.sh` reuses the same fixture functions and comparison
channels. All seven checks pass against the fixed binary and exit 1 against a
separately built archive with the corresponding behavior reverted. Exact
reversion diffs, build output and individual positive/negative outputs remain
here. The low-colour sabotage changes only `38;5;1` to `31`; the tab sabotages
substitute spaces with the same visible geometry. The pane-index sabotage
changes only zz's configured index after establishing equivalent targets.

The unmodified historical capture probe reports 108 matching comparisons and
eight differences, down from 38 differences in attempt-06. All 30 former tab
differences match at 80x24 and 100x30. The eight remaining comparisons are
100x30 erased-background captures: clear, erase-display, erase-line and
scroll-region, each under escape and padding modes. They remain owned TUI-017
probe residuals outside the seven-case roster. Styled frozen-mode capture also
remains unproven. Zero ordinary roster records is not a claim that these wider
capture questions are solved.

## Retained failures and interrupted collectors

The first attached fixture missed a popup marker; the full retry passed. The
first screen collector observed a prompt/cursor mismatch and then failed with
shell EOF because this lane edited the fixture while it was running. That is a
collection error, not a clean compatibility result. The next complete screen
run exposed the five low-colour failures described above; the corrected run
passes. All outputs remain here.

The first daemon Cargo invocation used the isolated HOME without restoring the
toolchain/cache locations and was cancelled while downloading duplicate build
inputs. Later invocations explicitly set CARGO_HOME and RUSTUP_HOME. Full daemon
library runs had timing failures which passed individually. The full CLI
integration run had three failures, each passing alone. One wait-exit child was
terminated by this lane after remaining blocked for more than three minutes;
that full-run result is not described as a normal assertion timeout. The daemon
slow-client soak failed both in the package integration run and in isolation,
with 1501 update items where the test expects 1500. Its baseline comparison is
recorded separately. No all-tests-green claim is made.

The initial sequential delta run used the earlier immutable binary and completed
20 rows with zero differences. This lane stopped it after `formats-values` to
run all selected rows in balanced batches against the rebased binary. Its exit
143, completed transcripts and selection are retained; those rows are rerun in
the final batches rather than counted as final-source proof.

## Source and base

Initial final-source revision: `c8887b3bcd582f02badf57a7787886ab587256f0`.
The lane then rebased onto origin/main
`f9c52359e3da88cbde735076b38e22cdf3e003c6`, retaining capture-12's carried commits.
The generated gap-report conflict was resolved by regeneration. The inherited
native-command expectation update was already upstream and Git dropped it.
The rebased source revision and immutable binary hashes are separate artifacts.
Only main's sidebar control changes alter crate sources across that rebase.

TUI-015's new outputs were initially collected under its already-existing
attempt-02 directory with new filenames, then moved to attempt-04. No historical
attempt-02 artifact was overwritten. Its notes and review remain intact.

This branch introduces no wire payload or protocol-version change relative to
origin/main. Protocol 104 is inherited unchanged; 103 is the frozen v0.10.0
release. The carried capture changes update the command catalog only.

## Copy-mode readiness and later retries

The native-only final binary's full copy-mode run failed at vi-halfpage-up and
vi-selection-extends; the rebased full run failed only at vi-halfpage-up. At the
repeated checkpoint the captured viewport still began with line-39 while the pin
had exposed line-27, but the later logical-position and view queries agreed.
The checkpoint's `none` readiness mode returned immediately; two identical
screen samples could describe the screen before the key took effect.

`copy-readiness-probe.txt` runs the repository fixture with only that checkpoint
changed to wait for `line-27 filler-27`. All 147 cases pass. Commit bb693da5
applies the same one-line change. It preserves every comparison and still fails
if the key never exposes the new row or lands on the wrong viewport. The full
fixture and its existing channel sabotages are rerun after applying the change.
This is stronger readiness, not a waived comparison.

The later final-binary attached run failed at ATTACHED_REATTACH_MARKER, displaying
the TUI's waiting-for-frame placeholder. This differs from the earlier popup
timeout and has no established common cause. A complete rebased-binary retry is
retained separately. The rebased package run also blocked in the CLI wait-exit
test's unbounded wait_with_output; its own control child was terminated after
more than three minutes, as recorded in packages-rebased-interruption.txt.

## Independent baseline and corpus findings

The clean c550a396 archive reproduces the slow-client soak failure exactly:
1501 update items against the expected 1500. `baseline-source-audit.json` checks
430 Rust/manifest/lock/patch files against Git blobs and confirms that its native
Ghostty sources lack this lane's provenance fields. The first baseline attempt
incorrectly reused a patched build-script executable from the shared target and
failed because the clean archive has no provenance.patch. That is a build-cache
failure, not a baseline test result. The successful independent build uses an
isolated target seeded only with non-Ghostty dependencies; archive source mtimes
force recompilation. Childless Cargo-slot waits cancelled before any compiler or
test started remain separate artifacts.

The first corpus passes of lane2-store and show-options-hooks differ only in
lock-command's default: zz prints `lock -np`, while this pin prints `vlock`.
The pin's configure.ac probes for an executable vlock on Linux, and config.log
records finding /usr/bin/vlock on this box. The original TUI-015 attempt-01 notes
excluded this build-selected default from their assertions and set lock-command
explicitly on both sides. This lane does not normalize these corpus bytes or
invent a new decision: their two and seven OUT differences remain measured and
owned by TUI-015, outside the three supplied target records. Both first-pass
transcripts are saved before automatic retries can replace them.

The rebased shared fixture and both obligation verifiers each completed with
199 assertions, 25 owned records and unattributed=0. Its self-check catches every
sabotage. The committed copy readiness change passes all 147 comparisons and
its self-check. The rebased screen fixture passes all 147 asserted checkpoints.

The independent pre-lane baseline at 327eb1af passes the complete attached
fixture. This prevents classifying the candidate reattach failures as inherited.
The focused candidate lifecycle fixture passes; a further complete candidate
run is retained as attached-rebased-retry.txt. No attached comparison is waived.

The full rebased package run passes all five library suites (CLI 106, daemon
979, mux 536, terminal 286 with one ignored, TUI 208). CLI integration reports
129 passes and four failures; the slow-client integration soak also fails.
A serial control-family run gives 49 passes and three failures. The independent
pre-lane control-family run gives 50 passes and two failures: its return-status
matrix stalls at generic-nonzero-detach-queued-open, while the candidate serial
run stalls at generic-nonzero-eof. Its wait-exit child also blocks indefinitely
and was terminated after more than three minutes, with that intervention kept
in baseline-control-interruption.txt. This supports an existing control-family
problem, not identical timing at every failing checkpoint. The candidate
refresh_client_ filter then passes all three tests, including both notification
and menu-sizing cases that failed in the serial family run.

## Complete delta corpus

All 205 selected rows ran against the rebased immutable binary in three
disjoint batches: exits 1, 0 and 1. After the repository runner retries, 197
rows match and eight retain differences. No row is unrun. delta-summary.json
contains every row, its five-channel tuple and its retained transcript.
delta-residuals.json assigns every remaining difference an owner and reason.
Popup resize, chooser confirmation and prompt chaining fail first and pass
on retry; their first failures are retained separately. The targets corpus
passes all 20 steps. This lane does not describe the complete corpus as green.

compat/check.sh passes on the rebased source, as do formatting and clippy for
all five required crates with all targets/features and warnings denied.

The complete candidate attached retry repeats the waiting-for-frame reattach
failure. A bounded tail probe skips the earlier main-body calls from probe_side
through the call before probe_requested_client_flags, then executes the existing
requested-flags, sizing, context, hooks, copy-pipe and lifecycle probes unchanged.
That probe passes, as does --lifecycle. Its PASS line is not a full-fixture pass.
The exact sed command is in commands.json. Further complete baseline and
reverted-fix comparisons are retained separately.

The later clean-baseline comparisons reproduce both remaining unclassified
corpus failures: the status background job has an empty first drawn expansion,
and accepted confirmation source replay times out with status 124 instead of
the pin status/event stream. Six persistent smoke failures now have pre-lane
reproductions; the other two corpus failures are the build-selected lock default.
The baseline comparisons are measurements, not new decisions or waivers.

## Final attached comparison

Both clean pre-lane full attached runs pass. The full reverted-fix build passes.
The no-tab-scan and no-resolver variants each differ in exactly one source file
from bb693da5; isolation-builds.json audits 469 source/manifest/lock/patch files
for each. Their focused checks prove the intended single behavior is reverted.
Both variants pass the full attached fixture. The unchanged rebased candidate
also passes the complete fixture in the same later interval after the delta
workload ends (attached-rebased-controlled.txt). Earlier candidate full failures
remain retained. This establishes intermittent behavior but does not establish
a cause or a frame-delivery fix. No attached assertion or timeout was changed.

proof-summary.json records the final results; footprint.json declares every
file in the delivered three-dot diff, including the inherited capture-12 work.
Both obligations remain at review for the gate.

The normal target/debug/zz_cli build was restored after the diagnostic builds.
Its SHA-256 is identical to the immutable rebased proof binary (see
restored-binary-hashes.txt), so no diagnostic behavior remains in that output.
Final tracker checks, wire guard and OKF validation pass; OKF reports only the
existing stale-research warning. The final fetch still has origin/main f9c52359
and origin/campaign/tui-capture-12 c550a396. No newer capture-12 review changes
were available to incorporate.

GitHub push protection rejected the first proof commit because the census-hook
environment dumps inherited credentials. Both initial and rebased census-hook
transcripts now replace the GitHub access token and messaging token values with
[REDACTED_CREDENTIAL], after the comparisons ran. No expectation, comparison
result or divergence tuple changed. evidence-redactions.json lists the affected
files and the private, permission-restricted original copies. The unpublished
proof commit was replaced; push protection was not bypassed.
