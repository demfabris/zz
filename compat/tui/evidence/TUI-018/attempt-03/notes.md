# TUI-018 attempt 03

Lane: alias. Delivery branch: campaign/tui-stream-alias-2.

Final worker status: partial; TUI-018 stays review with a filled proof block.
The requested stream fixes assert 57 comparisons, up from 43, with zero records
and the four existing cap decisions. All 29 self-check sabotages pass, including
16 new ones, and both equivalences pass. Shared-client self-check and attached-client
pass. The shared normal fixture and verify-claims fail on three CLI error statuses.
All 222 selected delta rows and 2,639 steps ran, with no unrun rows: 207 clean,
three matching known tuples, and 12 failures after retry. corpus-coverage.json,
corpus-summary-final.txt and corpus-final/ are the authoritative corpus outcome.
Daemon/mux/protocol package tests and CLI integration pass; two unchanged GUI tests
fail also alone. Four-crate clippy, cargo fmt, tracker checks and OKF validation
pass. compat/check.sh stops at the released-103 guard. No verified claim, independent
approval, board change or broad alias-parity claim is made.
Started on 2026-09-16. The lane fetched origin and rebased the four commits from
f65631c5 onto be5709df without conflicts. The rebased starting tip is abcaa5a6.

## Changes

The daemon keeps the caller stream in the request's CommandStreams record.
ExecutionContext::replay_client identifies that record during sourced-file replay.
Only an actual reader spends the bytes. ConfigReplay preserves redirected input
for a reader in a file and accepts binary bytes; the CLI skips that eager read
when stdin is a terminal. The response removes the request record. Daemon-start
configs have no such record and continue to refuse stdin.

A spent source reader now reports its read failure through the existing reported
failure path. It sets status 1 without pruning the alias queue. Missing source
paths retain the prior CommandExit path and abort later members. The independent
source-errors probe checks both paths and stdin parse-error continuation.

Alias output retains each child's writer claim. A raw buffer writer emits exact
bytes with no terminating newline. It closes stdout ownership to later print
output and makes a subsequent raw writer report EBADF, as the pin does.
Interactive and Control output retain their existing aggregation path.

The stream fixture adds source-, buffer-, and PaneInput-first aliases followed by
a spent source reader, trailing stdout, and a state write. It also adds the three
file-replay shapes, a daemon-start refusal control, binary unterminated alias
stdout, raw output followed by a print, and a missing-file abort control. Each new
case has a one-sided self-check sabotage. The spent-reader cases also sabotage
the state channel independently of stdout.

No wire fields or protocol version changed in this fix pass. Version remains
103. The inherited stdin_spent field remains serde-skipped.

## Run history

run.sh records commands, revisions, UTC timestamps, and exit status in commands.txt.
It selects this worktree's binary, tmux pin and corpus, scrubs HOME/config, and
retains CARGO_HOME/RUSTUP_HOME. Every Cargo invocation passes through /tmp/zz-cargo.sh.
The cargo-shim.sh copy in /tmp/zz-c11-alias-tools routes nested compat/check.sh
Cargo calls through that wrapper. Inside the wrapper's memory scope, it moves
appended --jobs before a test/clippy argument separator and removes it for fmt,
which does not accept --jobs. It does not bypass the memory scope or either lock.

The initial cached target/debug/zz predates the carried alias fixes. The
streams-baseline run used its daemon and failed earlier alias cases before the
new cases, then exited on cleanup of a buffer it had failed to create. Its log
is retained; it is not a measurement of the reviewed f65631c5 implementation.
The first compiled build matched the review's three independent probes. The
additional raw-then-print probe exposed another writer-ownership case; stdout-tail-1
retains it. The next fix preserves the pin's first-writer rule.

The literal batch command `compat/run.sh --delta --list` does not list with this
runner: --delta requires a range, so it consumed --list as the range and started
a run. I terminated that run with status 143. delta-selection.txt is the corrected
209-row selection for origin/main...HEAD plus source-file, load-buffer, save-buffer,
display-message and split-window. delta-all.txt is the full run of that selection.

The first build-2 wrapper waited on occupied slot 1 while slot 0 was free. After
checking that its flock process had no child, I terminated that waiter (143) and
retried through the same wrapper. The retry acquired slot 0 and built successfully.
No running compiler or other lane's process was stopped.

source-errors-1 also measures the alias containing `source-file -Z`: both sides
abort before effects with the same stderr, but zz exits 2 and tmux exits 1. This
fix pass has not changed CLI usage-error classification. That observation does
not establish complete alias parity outside the caller-stream cases.

The combined file-replay/raw-output probe found that ConfigLoadReport returned
its byte transcript as a lossy string and classified an alias by its name. The
fix keeps RawText and recognizes the member's raw writer during file replay.
The LC_ALL=C probe also found that a raw alias response passed through the text
sanitizer. Raw-owned responses now bypass it, as direct save-buffer already did.
The fixture adds both cases, with an extra newline on zz stdout as the self-check
sabotage. stdout-file-1.txt and stdout-locale-before.txt retain those failed probes.
I stopped the initial delta-all run after these new measurements; it is an
incomplete run, not final corpus coverage.

The fetched v0.10.0 tag ships protocol 103, contrary to the batch's unreleased
assumption. wire-version.py rejects any message.rs code change since that tag,
including the inherited serde-skipped marker and its test. Both wire.txt and
compat-check-1.txt retain the failure. I obeyed the explicit instruction to keep
103 and did not relax the guard. This remains a gate issue even when the wire
serialization tests pass.

At that checkpoint validation was still in progress. No verified claim or board change was made.

build-3 acquired the second slot but waited on this worktree's build-directory
lock while test-packages compiled. I verified that its Cargo process had no
children and terminated that waiting process (143), freeing the slot for other
lanes. Its retry will run after the package command releases the directory lock.

streams-fix2 printed `all 54 asserted comparisons identical, 0 recorded not
asserted, 4 decided`, but exited 127 after Bash read an invalid fragment at EOF.
I had edited the fixture while that process ran. That summary is not a successful
fixture run. The final fixture includes two further assertions, and I will keep the script unchanged during its closing
runs. streams-fix2.txt retains the error.

The package command stopped in the zz library with 660 passed, 2 failed and
1 ignored. The two failures are in unchanged GUI files (palette completion and
lazy sidebar settings). The command did not run the remaining packages after
that failure. Its binary linked after the last edits but used a daemon artifact
compiled earlier in the command. final-probes.txt exposed the stale binary: the
combined file/alias bytes and C-locale bytes still differed. I stopped final-streams
and final-delta-all; their names do not make them closing proof. The explicit
build-final command runs with the source frozen before any replacement proof.
The queued solo UI retries were also stopped before Cargo began to prioritize
that build; the two GUI failures still require isolated reruns.

streams-closing completed successfully with 56 asserted, 0 recorded and 4 decided.
While it ran, stdout-file-tail found that a later print-only alias in the same
config inherited the prior alias's raw classification and produced EBADF. The
fix records a sequence and the latest writer for each child execution, so the
request's accumulated claim is not confused with the current child's claim.
The alias group republishes its own final claim for its parent. The CLI regression
includes a later alias, and the fixture adds file-replay-binary-then-alias-print,
with a second raw write as the stderr sabotage. Final expected tally is 57.
The first corpus shards were stopped (143) and the proof pipeline parent was
stopped before self-check started. Their logs remain, but are not final coverage.

The no-fail-fast package run completed: daemon 937/937, mux 533/533 and protocol
228/228 library tests passed; their integration and doc tests passed as well.
The zz library had 660 passes, two GUI failures and one ignored test. Its other
binary and integration targets passed, including all 133 cli_binary tests.
The filtered final caller_stream_ integration run passed all three tests,
including the file replay followed by a print-only alias.

The closing proof binary (SHA256 158a9408818cb7fa48837b4a277cc8a0290ece374efce1573e62c2d53716a598)
was linked by the package run after the sequence-tracking fix. The integration
binary contains with-tail.conf, and stdout-file-tail-fixed measures the corrected
behavior with that build. The later source formatting does not alter its logic.
The complete corpus shards and proofs-closing.sh use a hard link of this binary,
with its sibling libcef.so, so later Cargo commands cannot replace the executable
under a running fixture. frozen-closing-artifacts.txt records both binary paths
and the final stream fixture hash.

The complete corpus shards initially warned that zz_cli was absent next to the
frozen zz executable. I hard-linked the just-built zz_cli and zz_helper companions
there before either shard reached a launcher row. Their hashes are appended to
frozen-closing-artifacts.txt. The startup warning is retained in both logs.

The invalid-source-flag exit-status observation in source-errors-1 is recorded
under owner TUI-011 (CLI argument errors): pin exit 1, zz exit 2, matching stderr
and no following effects. It was not added as an unattributed shared-fixture
case and is not counted as a TUI-018 assertion.

The local registry already closed aliases.command-bodies on 2026-08-30 and the
knowledge page names that closure F-ALIASES-MULTI-BODY. This fix addresses its
caller-stream residual, sharing the canonical_name=None alias-group root. The
existing eight-step aliases-multi-body scenario still passes. The closed record
and knowledge page now distinguish caller arguments (last child) from stdin
(first reader); the generated gaps.md was regenerated. No board was read or changed.

streams-self-check-57 exited 2 after detecting the source-, buffer-, and
PaneInput continuation sabotages. Both PaneInput sabotages create a pane;
drop_extra_panes enumerated indices 1 and 2, then deleting index 1 renumbered
index 2, so its second deletion missed the remaining pane. The fixture now
captures stable pane IDs before deletion. This changes cleanup only and keeps
the normal assertion count at 57. streams-self-check-stable-panes is the rerun;
the failed self-check log is retained. The later verify-claims run will execute
the normal fixture with the same corrected helper.

Both unchanged GUI failures reproduced in isolated filtered runs, each exit 101.
No GUI source edits or baseline pass are claimed. The standalone build-closing
passed, as did the filtered three-test caller stream run.

client-commands-57 exited 1: 3 of 84 asserted comparisons differ, with 25 recorded
cases. The three failures are refresh-missing-argument, client-tree-unknown-flag
and client-tree-usage: only exit status differs (tmux 1, zz 2). These are TUI-011
CLI error-status residuals, matching the separately recorded source-file -Z
observation. All five stream and stream-restoration comparisons passed. The owner
tally is TUI-014=6 TUI-015=4 TUI-017=6 decided:TUI-016=1
gap:clients.interactive-refresh=8 unattributed=0; no recorded TUI-018 case remains.
This lane did not change the flag-error classifier, and does not claim a baseline
binary measurement or a passing shared fixture.

clippy-closing passed all targets and all features for zz-daemon, zz-mux,
zz-protocol and zz with -D warnings, through the capped wrapper.

streams-self-check-stable-panes passed: every sabotage was caught in its own
channel and both equivalences passed, including separate state sabotages for
all three spent-source shapes and exact added-newline sabotages for binary
alias stdout. client-self-check-57 also passed with the same final summary.
fmt-closing passed through /tmp/zz-cargo.sh; no Rust files changed afterward.
The stable-ID fixture cleanup is committed at 357330a8, above implementation
36f57d72 and documentation correction 4615d685.

The first corpus pass of lane2-store has two OUT differences, at steps 35 and
193: zz's default lock-command is "lock -np" while the Linux pin reports vlock.
Owner: TUI-015 (lock-command default). The runner schedules its usual end-of-run
retry. No alias or source stream difference appears in that row's measured tuple.

The helper commit also removes two inherited added shell-comment lines, restoring
the original comment so the complete branch adds no Rust or shell code comments.
fixture-final.sha256 captures that comment-only change after the passing self-check.

attached-client-57 passed the full attached-client fixture. verify-claims-57
started at the pre-amendment cleanup commit 543184d6; the only subsequent fixture
change removed the inherited comment text, producing 357330a8. The helper and
assertions are identical. Both fixture hashes are retained for that distinction.

The corpus also records CLI error-status residuals under TUI-011:
smoke/alias-group-forgery differs only on the direct CLI's unknown-command
status (zz 2, pin 1); its config and Control channels match.
smoke/args-parse-choosers reports direct-buffer-arity and direct-tree-arity.
These rows are queued for the harness retry. The valid multi-member alias row
passes, but that does not justify claiming complete F-ALIASES-MULTI-BODY parity
across CLI error paths or changing its board status.

verify-claims-57 ran both mapped fixtures. It reproduced `all 57 asserted
comparisons identical, 0 recorded not asserted, 4 decided (0 for a sibling lane)`
for the final stream fixture, then exited 1 because the shared fixture reported
3 of 84 asserted comparisons differing and 25 recorded cases. The standalone
shared fixture identifies those failures above. The verifier's failure is not a
passing gate, even though the caller-stream cases themselves assert.

cli-error-status.txt records nine direct probes on both sides. set-buffer -G,
run-shell -C -G, if-shell -F -G, lock-server -G, wait-for -G, refresh-client -r,
choose-client -Q, choose-client with excess arguments, and the reserved alias
wrapper name all have identical empty stdout and identical stderr, but pin exits
1 and zz exits 2. Owner: TUI-011. The daemon-invalid-flags corpus helper requires
status 1 before setting its success marker, explaining that row's missing marker.
show-options-hooks also records seven instances of the same lock-command default
mismatch as lane2-store, under TUI-015.

Further first-pass CLI error-status rows are smoke/cli-chain-parse-abort
(CLI_PARSE_ABORT=broken versus clean:6, while CLI_RUNTIME_ORDER=clean on both)
and smoke/command-flag-errors (the success marker is absent on zz). Their helpers
also require status 1 for direct parser/flag errors. Owner: TUI-011. Final retry
outcomes remain in the corpus logs and coverage JSON.

Tracker validation passes with TUI-018 at review and a filled proof block.
compat-check-closing exits 1 at the released-103 guard; its preceding static
checks pass. post-wire-test-coverage.txt confirms that all 12 compatibility tests
listed after that guard passed in the completed package run, and census-closing
runs the separate non-gating census report. OKF validation passes with only the
existing aged-research warning after correcting the new cross-link's spelling.

Before close-out I compared the original review's delta command with this lane's
five-command selection. The review also named command-alias and send-keys. The
expanded closing command names those plus show-buffer, send-text and agent-send,
as well as the five original commands. delta-selection-full.txt contains 222
rows and 2,639 steps. It is a superset of the initial 209 rows. The 13 disjoint
additional rows in delta-extra-selection.txt run through corpus-extra.py against
the same frozen binary, without interrupting either original shard. The coverage
JSON now accounts for all three shards and lists every unfinished row until the
runs complete. The earlier 209-row inventory remains evidence for that initial
subset, not a claim that it alone covers the final selection.

The additional 13-row shard passed in full, including command-alias and all
selected copy-mode/send-keys rows. The first-pass resurrect restore smoke differed
because the tmux side failed its own title-restoration assertion: the prompt
replaced ALPHA/BETA with demfabris@alienware path titles. zz reported the expected
restored titles. This oracle/fixture residual belongs to the compat harness; its
retry result is recorded separately. Positional-maximums and stderr-parity also
expose TUI-011 error statuses; stderr-parity measures zz 2 versus pin 1 for
list-sessions -t and rename-window without an argument, with matching output.

All 222 selected rows and 2,639 steps completed their first pass. No row was
omitted. The second shard's remaining first-pass failures are positional-minimums
(TUI-011's direct error status) and status-background-jobs, whose zz side missed
a sampled DATE update. The latter is owned by gap:status.background-jobs and is
retried by the harness. The resurrect restore oracle/fixture issue is associated
with gap:plugins.runtime-paths. Neither gap's registry status was changed.

The first shard finished with seven failures after retry, including the tmux-only
resurrect title assertion. The harness footer says "Nothing failed on the first
pass" when no retry recovered; that footer contradicts its own warning lines and
exit 1. Coverage and results here use the scenario tuples, failure warnings and
exit statuses, not that footer. Detailed final logs for those failed rows are
preserved under corpus-final/; available first-pass logs are under corpus-first-pass/.


The final status-background-jobs retry also fails its zz DATE sample. Final corpus
failures are: lane2-store, smoke/alias-group-forgery, smoke/args-parse-choosers,
smoke/daemon-invalid-flags, smoke/plugin-runtime-resurrect-restore,
smoke/positional-maximums, stderr-parity, show-options-hooks,
smoke/cli-chain-parse-abort, smoke/command-flag-errors, smoke/positional-minimums,
and smoke/status-background-jobs. Both original shards exit 1; the 13-row addition
exits 0. The complete final failed-row logs are retained under corpus-final/.
