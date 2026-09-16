# Reconcile capture fixes with current main

This correction rebases campaign/tui-capture-12 from 98c5f77d onto e40a13e1.
The resolver conflict is reconciled through one session-only dispatch: bare
pane/window IDs and compound targets reuse resolve_window/resolve_pane; the
name-only helper does not retain main's duplicate ID lookup blocks. Compound
parsing remains behind TargetSlot::Session. Main's
session_targets_accept_pane_and_window_ids test and all four new target pins
are unchanged. The generated gaps report was regenerated during the rebase.

TUI-017 now carries FOUR ordinary owned records: capture-low-indexed-colour,
capture-tab-trailing, capture-tab-internal and capture-tab-wide. All four must
close before TUI-017 can verify. The sibling handoff must include the three tab
cases as well as indexed colour 1; its previous batch named only the latter.
The tab cases share the campaign TAB-cell storage limitation and are separate
from the DEC charset decision.

This attempt supersedes attempt-03's measured base and failure-attribution limits
only where fresh results below establish the new facts. Previous failed runs
and observations remain historical evidence.

## Tests at the reconciled source

The filtered session_targets run passes all four tests, including main's
session_targets_accept_pane_and_window_ids. The separate
window_targets_accept_pane_forms_like_tmux run passes. main-pins.json records
byte equality for main's test body and the complete twenty-step targets scenario.

Full package runs pass for zz-mux (534 library tests plus integrations),
zz-protocol (227 library tests plus integrations), and zz-terminal (280 passed,
one existing throughput test ignored). The full zz-daemon library run passes
947 tests and fails three under concurrent load: control_background_callbacks_cancel_after_disconnect,
default_shell_rejects_invalid_values_and_unset_controls_the_child_shell, and
loopback_forwards_http_and_tcp_in_both_families_with_original_port. Each passes
when rerun alone through the Cargo wrapper. The last failure was AddrInUse.
The full failed run remains artifact 05; individual reruns are artifacts 07.

Clippy passes for all four touched crates with all targets, all features and
-D warnings. The current all-payload wire guard passes at version 103.
Every Cargo invocation uses /tmp/zz-cargo.sh. Long command durations include
time queued behind other lanes' shared slots.

The clean main comparison uses an independently compiled git archive of e40a13e1
inside this worktree, with a separate target directory. main-environment.json
records its binary hash and source equality checks. Child measurement environments
omit credential variables before execution, so environment-dump cases can be
retained without exporting credentials.


## Baseline attribution

The eight shared-fixture assertion failures reproduce exactly on the clean
main binary: stdout and stderr match the pin, while main and the candidate
both exit 2 and the pin exits 1. Artifact 26 records every command and all three
channels for all three binaries. The sibling exit-code fix is still needed;
none of these assertions is weakened or converted to a record.

Twelve of the previous thirteen corpus failures reproduce on clean e40a13e1
with the same difference lines as attempt-03. prior-failure-attribution.json
retains their log hashes and exact differences. The full corrected corpus and
any extra failures receive their own final classification below.

The unchanged split-window-wait corpus passes on clean main and fails on the
candidate in the first comparison and all three paired repeats. Its parked
observation is two panes on main and the pin, versus one on the candidate.
Classify this as caused-here at the corpus boundary, not an inherited pass
waiver. The smaller diagnostic in artifact 29 produces the one-pane result on
both binaries and shows variable CLI query delays; it establishes timing
sensitivity but does not clear the candidate-only corpus regression. No source
cause or fix is claimed. All six paired corpus logs and the original delta
failure remain available.

The extra control-hard-loss observation is timing-sensitive on both binaries.
The initial candidate run differs at replacement extra-lines (zz 0, pin 1);
the clean-main run also prints zz 0 but its pin prints 0, and that main run
instead differs at the earlier hard-source marker. Both subsequent paired
runs pass on both binaries. Keep the initial failures and distinguish the
shared timing limitation from an exact reproduction of the whole diff.

All fresh Cargo commands used the shared wrapper. Daemon integration targets,
workspace formatting, compat/check.sh and the current wire guard pass. The
full daemon library result remains 947 passed and three failed, with all three
passing filtered reruns; it is not represented as an entirely green full run.


## Final delta corpus

All 163 selected rows completed. Each of the three disjoint batch commands
exited 1. Thirteen final rows diverge; capture-pane (34 steps) and targets
(20 steps, including main's four added pins) pass. There are no missing rows
or geometry differences. Two first-pass failures, control-hard-loss and
plugin-runtime-continuum, pass the runner's own retry and both later paired
repeats. Those initial observations remain in corpus-observations.jsonl.

Artifact 20 was an early summarization while a separate paired run was
rewriting one result log; it reported 162/163. Artifact 25 supersedes it with
163/163, using the preserved completed delta logs for rows also run in paired
diagnostics. The delta execution itself did not omit that row. Summarizer and
paired-driver exit 0 means successful collection, not a green parity result.

The table classifies every final failure. For each inherited row,
failure-comparison.json names an exact matching clean-main observation hash.
In particular, status-background-jobs reaches different failing checkpoints
across main retries, but one retained clean-main run has the candidate's exact
first-drawn-expansion failure. It is not inferred from the latest tally alone.

| Scenario | Attribution |
| --- | --- |
| show-options-hooks | inherited |
| smoke/alias-group-forgery | inherited |
| smoke/args-parse-choosers | inherited |
| smoke/cli-chain-parse-abort | inherited |
| smoke/command-flag-errors | inherited |
| smoke/daemon-invalid-flags | inherited |
| smoke/jobs-command-environment | inherited |
| smoke/plugin-runtime-resurrect-restore | inherited |
| smoke/positional-maximums | inherited |
| smoke/positional-minimums | inherited |
| smoke/source-replay-diagnostics | inherited |
| smoke/split-window-wait | caused-here |
| smoke/status-background-jobs | inherited |


The caused-here label is deliberately conservative at the unchanged corpus
boundary. Split-window-wait is unresolved and prevents a clean worker delivery.
A timing-sensitive reduced example is not a substitute for passing that row.
No assertion or scenario was changed to turn these measurements green.


## Shared fixture and sabotage proof

All three fresh fixture runs end with exactly:

```
8 of 186 asserted comparisons differ, 32 recorded (0 for a sibling lane, owners TUI-014=6 TUI-015=3 TUI-017=4 decided:TUI-015=4 decided:TUI-016=1 decided:TUI-017=6 gap:clients.interactive-refresh=8 unattributed=0)
```

Each exits 1 for the eight clean-main exit-policy failures. Every one of the
18 added regressions passes, and lock-client remains the sole recorded-to-asserted
flip. The four other lock CLI-only decision registrations remain unchanged.

The self-check exits 0 and prints 80 successful checks, including positive
controls, equivalences and deliberate failures. It catches the lock-client
status-row sabotage, all four erased-background scene sabotages, all three
wide-wrap scene sabotages, exact-empty rejection for each of lock-session,
has-session and list-windows, and one-sided bare and compound absolute-pane
targets. Its final line is:

```
self-check complete: every sabotage was caught in its own channel and both equivalences passed
```


## Capture, lock and transport probes

The fresh two-client 80x24/100x30 capture probe exits 0 and compares all 42
batch-scope results identically, including the settled resize and compound
target controls. probe-summary.json retains the exact channel values. The
probe is observational: it also prints 38 differences, so its exit 0 alone
is not a parity claim. Thirty are the three named tab scenes across modes and
sizes. Eight are ordinary -e/-N erased-background captures at 100 columns
(EL, ED, clear and scroll-region), retained under TUI-017; they are not new
fixture assertions or recorded-to-asserted flips. Styled frozen-mode capture
is still outside this live-grid proof.

The lock probe and rich-transports probe each complete with exit 0. Their raw
pin/zz measurements remain in artifacts 16 and 17, including the documented
locking decision and the refused rich transports. Completion does not mean
those deliberate differences disappeared.

The first attached-client run exits 1 at its final reattach observation:
`zz screen did not show ATTACHED_REATTACH_MARKER within 10 seconds`.
The zz screen contains `waiting for frame`. Artifact 14 retains the full
screens and daemon diagnostics; no old-base attached-client pass substitutes
for this failure. A fresh retry is recorded separately in artifact 38.


The fresh attached-client retry (artifact 38) prints
`attached-client compatibility: PASS` and exits 0. The initial reattach timeout
is still an observed failure, not removed or retroactively called a pass.
The independent clean-main attached-client comparison is artifact 40.

The TUI-015 executable claim check exits 1 for the same eight inherited
shared-fixture assertions. Its truncated summary is the validator's own
output; the complete owner tally is retained in each of the three direct
fixture runs. Collection drivers can exit 0 after retaining child failures;
only individual command results are parity evidence.

The clean-main attached-client comparison (artifact 40) also passes with exit 0.
This supports retaining the candidate retry as a fresh pass, but does not prove
the initial candidate reattach timeout was inherited. That failure remains an
explicit measurement limit.


The TUI-017 executable claim check also exits 1 for the same eight inherited
assertions. Both executable claim checks were actually run at the new base;
structural validation alone was not substituted for them. Both obligations
remain review, and this delivery is partial because the split-window corpus
regression remains open. The initial attached-client timeout and the three
full daemon-library failures are retained beside their passing reruns.

The production sources and shared fixture are unchanged from measured source
204c7975. The delivery commit updates only records, reports and fresh evidence.
The rebase onto e40a13e1 is complete, including main's later daemon changes and
its pane/window-ID session target pins. The generated gaps report was written
with the tracker, not hand-edited. No code comments or commit attribution
trailers were added.

Final tracker, tmux-tracker, structural claims, wire and source-whitespace checks pass.
OKF validation passes with its existing warning for five older research pages.
Artifact 41 checks the original environment's two credential-like values locally
and finds no matches in the fresh evidence; it also rechecks binary, fixture
and pin hashes and byte equality of main's target test and scenario.
The complete three-dot file list is footprint.txt, based on e40a13e1.
