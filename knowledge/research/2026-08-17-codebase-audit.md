---
type: Research Report
title: Codebase Audit for Code Smells, Rust Antipatterns, and Performance Issues
description: Revalidation at 758dac0 found nine confirmed issues, four qualified or latent findings, one intentional ABI contract, and one overstated impact claim.
tags:
  - research
  - audit
  - rust
  - concurrency
  - performance
  - ffi
timestamp: 2026-08-17T14:45:00-03:00
git_commit: 758dac01cd491b7927d56a79685de3e3f05d802f
---

# Overview

The first audit listed 34 findings. Source checks at
`9ba4d0f0074e9081dad46dfccebda2ffe8843265` removed 19 claims whose callers,
timeouts, platform guards, or existing containment changed the verdict.

A second source check after the large merge, at
`758dac01cd491b7927d56a79685de3e3f05d802f`, found the remaining implementation
shapes intact. Nine findings describe confirmed issues. Four need qualified or
latent wording, one records an intentional ABI contract, and one overstated its
runtime impact.

| # | Finding | Verdict |
|---:|---|---|
| 1 | PTY writer blocks the terminal actor | Confirmed, Unix-only |
| 2 | Prefix interception preempts dialog input | Confirmed, conditional |
| 3 | Alt-Ctrl bindings use a different canonical order | Confirmed |
| 4 | Git refreshes can overlap | Confirmed performance risk, no production profile |
| 5 | Large copy-mode counts clone command values | Confirmed bounded cost |
| 6 | Frame diff scores every row shift | Confirmed algorithm, no production profile |
| 7 | FFI pane getters repeat layout traversal | Confirmed algorithm, no production profile |
| 8 | FFI exports lack panic containment | Latent containment gap |
| 9 | Connect queues HELLO without waking the event fd | Confirmed contract bug |
| 10 | `zz_bytes` uses pointer plus length | Intentional ABI contract |
| 11 | Editor reducer starts with process cwd | Qualified edge case |
| 12 | Copy-pipe stages input and kills one process | Qualified implementation fact |
| 13 | DPAPI output allocation is freed without a wipe | Confirmed, Windows-only |
| 14 | AppKit send-event state lacks unwind restoration | Latent hardening gap |
| 15 | BSD wake pipe drains one chunk | Impact claim overstated |

# Confirmed correctness issues

## 1. PTY writer can block the terminal actor

`crates/zz-terminal/src/session.rs` `PtyWriter::write`

The master PTY and its duplicate share the nonblocking open-file flag. Once the
kernel write buffer returns `EAGAIN`, the writer polls with no timeout on the
`zz-terminal` thread. An open slave whose process does not read can leave that
wait with no bound.

The same actor handles resize, shutdown, and terminal producers. The blocked
write can delay all three. `wait_for_wake` uses a separate bounded wait and does
not protect this path.

## 2. Prefix interception can preempt dialog input

`crates/zz/src/workspace/view.rs` `AppView::intercept_keystroke`

The global interceptor runs before keymap dispatch. The failure needs an active
mux pane and an armed prefix. Under those conditions, the interceptor forwards
the next non-platform key to the mux or daemon and stops propagation before the
dialog handles Enter or Escape. The prefix table consumes the key in the common
case, so the audit cannot claim that the PTY receives it.

With no active pane, or with no armed prefix beyond the prefix chord itself, the
interceptor returns without swallowing arbitrary dialog keys.

## 3. Alt-Ctrl bindings use a different canonical order

`crates/zz-protocol/src/key.rs` `canonical_key`, `input_key_name`

`canonical_key("Alt-Ctrl-x")` preserves encounter order and stores `M-C-x`.
Live input emits Control before Alt and looks up `C-M-x`. A binding written with
the inverse modifier order misses even though both strings describe the same
key chord.

## 9. Connect queues HELLO without waking the event fd

`crates/zz-client-ffi/src/ffi.rs` `zz_client_connect`

Connect feeds `ServerHello` into `ClientCore` and queues `ZZ_EVENT_HELLO`. It
writes no byte to the event fd until the reader receives a later daemon message
or disconnects. The header documents a poll-then-drain contract, so a poll-first
client can wait on a quiet daemon while HELLO already sits in the queue.

The Swift client drains after connect, and the C smoke client did the same at
the audited commit. Those consumers mask the contract bug.

## 13. DPAPI output allocation is freed without a wipe

`crates/zz-chrome-import/src/cookie.rs` `dpapi_unprotect`

`CryptUnprotectData` returns plaintext in a `LocalAlloc` buffer. The importer
copies it into `Zeroizing<Vec<u8>>`, then calls `LocalFree` without clearing the
original allocation. Microsoft asks callers to clear sensitive output before
freeing it. This Windows path runs during user-triggered Chrome cookie import.

# Confirmed performance shapes

## 4. Agent Git refreshes can overlap

`crates/zz-daemon/src/agent/host.rs` `start_git_refresh`
`crates/zz-daemon/src/agent/git_summary.rs` `capture_git_summary`, `write_tree`

`SessionReady`, `SessionSwitched`, and `PromptFinished` each start a refresh.
The audited gate drops stale completed results but does not prevent captures
from overlapping.

A repository with HEAD runs seven Git children per capture. An unborn
repository runs eight. Each command uses two reader threads, so one active
capture has the Rust refresh thread, two reader threads, and one Git child
during command execution. The scratch `GIT_INDEX_FILE` keeps the real index
untouched. No production measurement establishes user-visible cost.

The scratch-tree approach includes tracked and untracked content. A status
query has not shown equivalent additions, deletions, and rename behavior.

## 5. Large copy-mode counts clone command values

`crates/zz-protocol/src/key.rs` `KeyEngine::decide`

Copy-mode counts stop at 9,999. Stock `j` expands to `send-keys -X cursor-down`.
Each repeated invocation clones the command name, two argument strings, and a
`Vec`, then the daemon executes the invocations in sequence. The allocation is
eager and bounded.

## 6. Frame diff scores every row shift

`crates/zz-terminal/src/model.rs` `best_row_shift_from_fingerprints`

The shift search scores zero on its own, then visits every nonzero offset. At
250 rows it performs 62,500 fingerprint equalities plus 124,500 nonzero-loop
predicate visits, along with fingerprint construction. The comparison uses
`u64` fingerprints rather than full rows. No profile links this work to a
production frame-time problem.

## 7. FFI pane getters repeat layout traversal

`crates/zz-client-ffi/src/ffi.rs` `pane_at`

`pane_id`, `pane_title`, `pane_kind`, `pane_is_active`, and `pane_has_bell`
each call `pane_at`. That helper allocates a `Vec` and walks
`LayoutNode::panes`. The Swift snapshot loop calls all five getters for each
pane, causing 5N complete traversals and quadratic layout-node work for an
N-pane active window. No profile measures the cost in a real client refresh.

# Qualified and latent findings

## 8. FFI exports lack panic containment

`crates/zz-client-ffi/src/ffi.rs` (44 Unix `extern "C"` exports)

The 44 exports use no `catch_unwind` boundary. A Rust panic that escapes an
`extern "C"` function aborts the process. Lock poison already recovers through
`PoisonError::into_inner`, and the audit found no valid normal-call path that
panics. Treat this as one latent containment gap across the ABI, not 44 proven
defects.

## 11. Editor reducer starts with process cwd

`crates/zz-mux/src/command.rs` `select_pane_kind`
`crates/zz-daemon/src/daemon.rs` editor materialization

The reducer installs `std::env::current_dir()` as a placeholder. Before the
daemon publishes the pane, it replaces the placeholder with the donor
terminal's live cwd under the same lock. The live-cwd integration test passes.
Only direct reducer use and `current_dir()` failure remain as edge cases.

## 12. Copy-pipe staging and timeout have separate risk profiles

`crates/zz-daemon/src/daemon.rs` `spawn_copy_pipe`, `run_copy_pipe`

`spawn_copy_pipe` enforces the 32 MiB selection cap before worker creation.
`run_copy_pipe` stages the selection in a tempfile, rewinds it, and gives that
file to the shell as stdin. The 30-second timer starts after staging and spawn.
The audit found no failure caused by tempfile staging.

At the audited commit, timeout kills the immediate shell child. A pipeline or
background descendant can survive. This is a process-tree containment gap,
separate from the staging choice.

## 14. AppKit send-event state lacks unwind restoration

`crates/zz-browser/src/mac_app_protocol.rs` `send_event`

The swizzled `sendEvent:` sets `HANDLING_SEND_EVENT`, calls the previous IMP
through `extern "C-unwind"`, and restores the outer value after a normal return.
A native or Objective-C unwind can skip the store. The wrapper itself has no
reachable Rust panic path in the audited source. Treat this as hardening for a
foreign unwind, not a demonstrated crash.

# Contracts and corrected impact

## 10. `zz_bytes` is an intentional pointer-plus-length ABI

`crates/zz-client-ffi/include/zz-client.h` `zz_bytes`
`crates/zz-client-ffi/src/ffi.rs` `ZzBytes::new`

The header declares `uint8_t *` plus `len`, not `char *`. Rust returns
`as_ptr()` and `len`; C uses `memcmp` with that length, and Swift constructs an
`UnsafeBufferPointer` with it. Consumers must copy data before the next API
call that can invalidate the backing snapshot. The missing NUL terminator is
part of the contract, not a defect.

## 15. A partial wake drain causes short no-op actor cycles

`crates/zz-terminal/src/session.rs` `wait_for_wake` (macOS and BSD)

One wait drains at most 64 wake bytes. More bytes leave the pipe readable and
can trigger extra actor cycles with no work. The actor checks channels before
polling and checks them again after the drain, so queued commands do not remain
untouched as the original audit claimed.

# Remediation in the working tree

The changes following this revalidation address the confirmed correctness
issues and the containment fixes that fit the existing architecture:

| Finding | Change |
|---:|---|
| 1 | Admit whole PTY-input commands through a 256-command, 64 MiB budget; buffer complete nonblocking writes and retry them while control work and PTY reads continue. |
| 2 | Send a tokenized prefix-cancel barrier whenever a dialog opens, and keep ordinary workspace keys gated until the matching daemon acknowledgement or a connection reset. |
| 3 | Canonicalize Control before Alt for stored and live key names. |
| 4 | Coalesce Git captures through a single-flight, latest-wins gate. |
| 9 | Wake the event fd when connect queues HELLO. |
| 12 | Put the copy-pipe shell in a process group and terminate its descendants on timeout. |
| 13 | Zero the DPAPI-owned plaintext before `LocalFree`. |
| 14 | Restore the AppKit flag from a `Drop` guard during unwind. |
| 15 | Make both BSD wake-pipe ends nonblocking, retry interrupted wake writes, and drain until `EAGAIN`. |

Findings 5 through 7 need profiles before a more complex implementation earns
its maintenance cost. Finding 8 needs a reachable panic case or one shared ABI
boundary design. Findings 10 and 11 do not need a production fix.
