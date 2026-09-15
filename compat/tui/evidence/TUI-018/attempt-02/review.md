# Adversarial review of the alias stream lane

Verdict: **reject**. Reviewed implementation: `ff58de8a6f8fc194db0480deb6893ba85e6121e0`.
The evidence-only commit `57bf163f8dc5dbe3547ed9a039485e08f5c46eff` preserves the worker's
34 pending output, reproduction, and notes files. No implementation fix or integration rebase
forms part of this review. TUI-018 remains at `review`, with no proof block.

## Blockers

### A spent source reader drops the rest of the alias

With `revieworder=source-file - ; source-file - ; display-message -p after` and stdin
`set -g @order yes\n`, both implementations apply the option once and return exit 1 with
`Bad file descriptor: -\n` on stderr. The pin also prints `after\n`; zz prints nothing.
The same missing continuation occurs with a buffer reader or a PaneInput reader before the
spent source reader. See `review-order-probes.txt`.

The new stream assignment reaches the existing source-specific break in
`crates/zz-daemon/src/daemon.rs:12369-12372`. The spent source effect produces `CommandExit`
at `:10262-10265`, so that break discards the remaining members. Pinned
`cmd-source-file.c:116-131` reports the asynchronous read error and resumes the queue;
`cmd-queue.c:768-798` does not prune the following members on that resumed path.

The fixture asserts continuation after a spent **buffer** reader but ends its source-reader
cases at the failing member (`compat/tui-command-streams.sh:539-560`). Extend that proof to
assert trailing stdout and state after a spent source reader, then distinguish asynchronous
read failure from the source failures that should abort a group. Removing all source-error
aborts without measuring their other callers would be too broad.

The extended repeat also appends `set -g @after yes`: the pin sets that option and zz leaves
it absent (`review-order-probes-state.txt`). The failure discards state changes as well as output.

### Config-file aliases lose an available caller stream

Put `reviewcfg` in a file and define `reviewcfg=source-file - ; source-file -`. Invoke
`source-file <that-file>` with `set -ag @reviewcfg once\n` on piped stdin. The pin applies
the payload, reports one EBADF, and exits 1. zz applies nothing, reports
`source-file from standard input is not supported\n`, and exits 1. A single-reader alias
and a nonreader followed by a source reader also fail: the pin applies the payload and exits
0; zz refuses it and exits 1. Fresh throwaway servers reproduced each shape twice in
`review-config-probes.txt`, in addition to the initial independent probe.

The CLI checks the outer command at `crates/zz/src/lib.rs:1077-1106`; the resolver at
`crates/zz-daemon/src/daemon.rs:38393-38398` recognizes only a literal `-` path. File replay
creates fresh invocations at `:25513-25579` and executes them without a stream at
`:26154-26182`. Pinned `cmd-source-file.c:119`, `cfg.c:173-195`, and
`cmd-queue.c:317-338` preserve the invoking client through replay. Its `file_read` rejects
stdin for absent, attached, or Control clients, not an ordinary command client sourcing a
file (`file.c:377-387`).

The design's assertion that a loaded config has no caller and the pin reads nothing there
(`knowledge/designs/command-stream-channel.md:114-115`) is false for this command-client
path. This is an uncovered residual, not a claim that the branch introduced a regression.
TUI-018's whole-obligation verification cannot rely on that assertion. Add the measured
case, correct the design, and carry the bounded stream through client-owned config replay
before claiming parity for it.

### A raw buffer writer inside the alias gains a newline

Define `revieworder=load-buffer -b review - ; save-buffer -b review -` and pipe the four
bytes `61 ff 00 7a` into it. The pin emits those same four bytes. zz emits five bytes,
`61 ff 00 7a 0a`. Both exit 0 with empty stderr, and a subsequent direct `save-buffer -`
proves that both stored buffers still contain the original four bytes. Plain `no-newline`
has the same added byte; a payload already ending in newline matches. See
`review-stdout-probes.txt`.

The daemon preserves RawText while collecting the member's output
(`crates/zz-daemon/src/daemon.rs:15793-15797`, `:12360-12361`, `:37748-37756`), but the
group result carries no child stdout claim. The response falls back to the alias group's
name at `:5924-5925`, and `default_stdout_claim` classifies that name as Print at
`:37691-37697`. `CommandOutputWriter` then appends a newline at
`crates/zz/src/lib.rs:1907-1909`. Carry the writer's raw-output ownership through the group
and add a binary, unterminated payload assertion. The current fixture only reads buffers
back through a separate direct command, so it misses this output change.

## Clause assessment

1. The one-reader, 1 MiB cap, RawText carrier, three-sink resolver, stdout transport, and
   in-process spent marker exist. The four over-cap cases retain the prior product decision.
   The config-replay lifetime assumption above remains wrong; no product decision in the
   lane excludes this measured workload.
2. The existing fixture compares exact CLI bytes, exit status, and resulting state for its
   named cases. It does not assert the three failing shapes above. Its zero-recorded tally is
   insufficient to establish this clause in full.

TUI-018 has no dependencies (`depends_on: []`). It must not become verified from this tip.
At the reviewed base, TUI-011 still waits for TUI-018 as well as TUI-014, TUI-015, and TUI-017.

The remaining accepted `protocol.binary-streams` items cover non-UTF-8 command arguments
and show-buffer presentation. They do not register these three residuals. The closed
`buffers.standard-streams` proof explicitly excluded binary stdin through multi-command
aliases; this lane now makes that path reachable and must preserve its output. The closed
`config.same-line-error-group` record also describes asynchronous source-read continuation.

## Considered and dismissed

- Wire change: `CommandInvocation.stdin_spent` has `#[serde(skip)]` at
  `crates/zz-protocol/src/message.rs:1738`. Its serialization test compares the spent and
  absent encodings. The existing 103 assertion and two `0x67` bytes in `hunt_claims.rs`
  remain unchanged. `review-wire.txt` passes.
- Binary alias payload corruption: the first buffer reader preserves `a ff 00 z 0a`.
  Direct buffer input preserves `d fe 00 e 0a`. Both raw readback and `od -c` agree.
- First-member-only routing: an option setter before `source-file -` leaves the bytes
  available. Direct and single-alias controls agree with the pin.
- Target handling: the measured source `-t` group and split `-I -c /tmp -t` group agree;
  an empty pane receives `display-message -I` inside an alias group.
- Direct text routing: piped `send-text --no-enter` preserves a leading `--` as literal
  text and matches tmux `send-keys -l` in the pane. Direct `agent-send` reaches the expected
  missing-agent diagnostic (`review-text-probes.txt`).
- A spent PaneInput reader need not report the file-reader EBADF. The pin resumes that
  path silently, matching the source-then-split probe. It is not a finding.
- Fixture cannot fail: the existing self-checks target each new/flipped assertion, and
  the review adds a separate one-sided stream-loss sabotage.
- Scope: the protocol in-process marker, CLI tests, daemon export, and channel design are
  declared in the worker's notes. No dependency, GUI, renderer, or sibling-ledger change
  appears in the lane. No added Rust comments or attribution trailers appear.

## Validation and delivery

Exact commands and exit codes are in `review-commands.txt`; environment and artifact hashes
are in `review-environment.txt`. The review uses the worktree's own build, the provided pinned tmux,
isolated homes and sockets, and the requested shared cargo lock and memory cap.

| Check | Exit and result |
| --- | --- |
| Build through the wrapper | 0 |
| Stream fixture, runs 1, 2, and 3-repeat | 0 each; `all 43 asserted comparisons identical, 0 recorded not asserted, 4 decided (0 for a sibling lane)` |
| Stream self-check | 0; the new/flipped cases' eight sabotages and both equivalences pass |
| Reviewer's stream-loss sabotage | Expected exit 1; exactly the source alias state assertion and option readback fail, `2 of 43 asserted comparisons differ` |
| Shared client fixture | 0; 84 asserted, 25 recorded, TUI-018=0 and unattributed=0 |
| Shared client self-check | 0; all channel sabotages and both equivalences pass |
| Mux, daemon, protocol library tests | 0; 527, 919, and 224 passed respectively |
| Full zz package | 0; 642 library tests passed, one existing ignored, 127 CLI tests passed, remaining integration targets passed |
| Clippy on all four affected crates, all targets/features, warnings denied | 0 |
| Formatting check | 0 |
| Both trackers and generated reports | 0; regeneration leaves no diff |
| TUI tracker and verify-claims Python tests | 0 each |
| Wire-version check | 0, version 103 |
| OKF validation | 0; one existing aged-research warning |

The original third stream run printed the complete 43/0/4 summary, but I changed its
logging wrapper while it was running and lost reliable child-exit capture. The wrapper
exited 2. I retained that output and repeated the run with a stable wrapper; `3-repeat`
supplies the third confirmed exit 0. The command log records that review tooling error.
Clippy waited for the shared cargo slot, then completed; no package-test retry or memory-cap
kill occurred.

The requested delta selection contains 221 rows and 2,637 steps. All 37 review chunks
returned 0, with no retries. The longest took 517 seconds. The only nonzero comparison
entries were the three registered known-difference rows: `known-main-preset-two-panes`,
`known-pane-scrollbar-columns`, and `known-spread-mixed`. The known baseline-sensitive
`smoke/status-background-jobs` row passed on its first attempt here. Exact rows, summaries,
and durations are in `review-delta-list.txt`, `review-corpus-*.txt`, and
`review-corpus-summary.txt`.

The worker's earlier selection contained 161 rows and its final run covered 24 named rows.
That 24-row result is real, but was not the complete requested delta selection. The full
review sweep now passes; it does not contain the three failing independent probe shapes.

The final process sweep found no matching leftovers, no own-worktree executable or scratch
HOME/config candidates, and no remaining independent-probe scratch directories. No process
needed killing and no binary was copied into `/tmp`. Temporary reviewer wrappers are removed
after their exact copies have been retained in this directory. See `review-cleanup.txt`.

## Disposition

Push the evidence and this rejection report to `campaign/tui-stream-alias-gated`, without
rebasing or changing production code. No main integration, MAIN lease, or verified proof
record is permitted by this verdict. The gated branch's final commit is evidence-only;
the tested implementation remains byte-identical to `ff58de8a`.

Gate-only post-rebase checks, `verify-claims --run`, and `compat/check.sh` were not rerun by
this reviewer because the review rejected the tip before integration. The worker's earlier
outputs for those commands remain distinguishable from the reviewer outputs in this directory.
