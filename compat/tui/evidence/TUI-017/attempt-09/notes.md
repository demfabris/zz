# Edited tab provenance and write cost

The lane ran `git rebase origin/main` before making this revision. It was already
based on `38c50df4`, including the empty-exact-window resolver review and the
credential-scrubbing harness changes. Final production source is `686506a1`.
The resolver is unchanged by this revision. Both obligations remain at review.

The tab writer now uses the cursor's existing cell pointer and row. It finds the
next stop locally and advances the cursor once. Provenance validation and writes
use contiguous cells, with a maximum span of 32 cells. There are no page lookups
per column: extra page-resolution work is constant, and tagging has a fixed
32-cell bound independent of terminal width. The pin has the same bound:
`input.c:1329` rejects tab provenance wider than `gc.data.data`, and
`tmux.h:711` defines that buffer as 32 bytes. This replaces the previous
`1 + 2*width` slow page lookups. No new row or terminal allocation is introduced.

A tab head and its padding now carry independent cell facts. Native cell moves
therefore preserve their meaning through ICH/DCH and vertical edits; capture no
longer demands an intact positional span. Overwriting a fragment clears the
appropriate neighboring tab cells. Cleared neighbors use the default background,
matching the pin even when the overwrite uses another background colour. The
ICH vacated-range rule also matches the pin when the insertion exceeds the moved
range. These changes affect the local engine representation, not wire payloads.

There are 23 new asserted pin comparisons, covering ICH/DCH placement, truncation,
overwrite and removal; ECH; IL/DL; scroll-region movement and removal; and coloured
overwrite. Each has a scene sabotage. DCH inside padding can have the same capture
bytes when omitted, so that case instead replaces HT with spaces. The unit test
runs 22 edit sequences both as whole input and byte by byte; the styled capture
test covers the additional background case.

## Performance

The native drivers are the reviewer's exact tab-heavy and space-control sources.
Both feed 8,388,600 bytes through one native write and measure process CPU time,
excluding initialization and teardown. The main archive and final candidate use
the same native pin, ReleaseSafe configuration and baseline CPU target. The main
binary is checked against 536 source and manifest files from current main; the
native source comparison checks the pristine engine files. Hashes and raw samples
are retained.

The final measurement uses three blocks of five alternating candidate/main pairs
for each workload. CPU 12 was selected before measurement from a one-second core
load sample; only benchmark children had affinity and ASLR settings changed.
There were 100 warm-up pairs. Other work remained on the host; no exclusive-host
claim is made. All earlier optimization and noisy measurement batches remain.

Pooled native medians: tabs are 0.107047136 seconds fixed versus 0.104710933 main
(+2.23%); spaces are 0.098959377 versus 0.094913279 (+4.26%). Tab block differences
are +5.10%, +2.11% and +2.55%; space block differences are +4.15%, +3.92% and
-2.21%. The no-tab control is not claimed to be exactly unchanged. The retained
samples show the shared-host variation and let review judge the remaining cost.

The attached cat probe completed five runs per binary with the expected marker
and 23-by-80 grid. Median wall time was 1.214 seconds fixed versus 1.510 main.
Those samples were grouped by binary and ran with other fixtures, so they do not
replace the alternating native CPU comparison. `bench/run.sh` was attempted but
could not run: its fixtures and release desktop bundle are absent. The headless
binary is not substituted for that desktop benchmark.

## Evidence boundaries

The seven-case counterfactual archive disables only the configured-index callback
and capture consumers. Its native producer is unchanged. The source audit records
exactly two changed files. It tests the seven original residuals; scene sabotages
separately test the edited-tab comparisons.

No environment dump is collected. The corpus logger omits only the stdout of its
unasserted `show-environment -g` command, while retaining command execution and
exit/hook comparisons. Earlier failed and interrupted runs are retained in
`intermediate-audit.json` and their transcripts. Editing a collector during an
initial run caused EOF failures; those runs are not counted as final passes.
Production source and scripts were frozen before the final suite.

Earlier limits in attempt-07 remain outside this edit matrix: narrow-screen
reflow, styled frozen-mode capture, compressed-history round trips and the wider
100-column erased-background differences. The new cases do not claim universal
engine parity. The final fixture and corpus summaries below are generated from
completed runs, with every remaining owner and unrun row named.

## Final fixture checks

Three executions of the shared fixture each pass 228 assertions with 25 records.
The first invocation is direct; the second and third are the TUI-015 and TUI-017
claim verifiers, each of which executes the same fixture. Both report zero
ordinary records owned by the obligation. The direct log retains the full tally:
TUI-014=6, decided:TUI-015=4, decided:TUI-016=1, decided:TUI-017=6,
gap:clients.interactive-refresh=8, unattributed=0. The six empty-exact-window cases
and all 23 new edited-tab cases pass. The shared self-check passes 170 expectations.

Copy-mode passes all 147 comparisons with zero records and 27 self-check
expectations. Screen-diff passes all 147 assertions with six existing decisions
and 18 self-check expectations. Full attached-client passes, including its
command-output interaction and detach/reattach checks. The 25-step configured
index probe is clean in all five channels.

The full mux and terminal packages pass 928 tests, with one ignored throughput
measurement. Five-crate clippy with warnings denied, formatting, the wire guard,
OKF validation and compat/check.sh pass. The production rebuild compares exactly
to the immutable candidate. The final source does not alter the protocol.

## Delta corpus

All 163 selected rows ran, partitioned into disjoint 55/54/54-row shards. The
selection includes capture-pane, lock-session, has-session and list-windows plus
the delta selector's related commands. First pass: 157 clean, six differences.
After the harness retries: 158 clean, five persistent differences. There are no
unrun rows. Shard exits are 1, 0 and 1; this is not a claim of a clean corpus.

The five persistent rows also fail against the immutable plain-main control,
with matching divergence counts and diff lines: show-options-hooks,
smoke/cli-chain-parse-abort, smoke/default-client-command,
smoke/source-replay-diagnostics and smoke/plugin-runtime-resurrect-restore.
The options row differs in this pin build's vlock default; the restore fixture
reports its own pin-side assertion failure. The raw comparisons are retained.

Status-background-jobs differs on the first pass and on the first main-control
probe, then passes the harness retry without a source change. The first main
probe overlapped the automatic retry's shared transcript path: its own result
line is retained, and the retry transcript contains both runs. A further main
probe is isolated after the corpus finishes. No timing cause or runtime fix is
claimed from this variation.

The subsequent isolated status-job probes both fail (fixed and main), with
matching diff lines. Their separate transcripts preserve that result after the
unchanged candidate automatic retry passed. This remains an intermittent
baseline residual, not a source regression attributed to the tab change.
