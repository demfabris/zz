---
type: Research Report
title: Codebase Audit for Code Smells, Rust Antipatterns, and Performance Issues
description: Source-checked audit at 9ba4d0f. Fifteen findings that hold; nineteen first-pass claims removed after caller, timeout, cfg, and guard checks.
tags:
  - research
  - audit
  - rust
  - concurrency
  - performance
  - ffi
timestamp: 2026-08-17T14:45:00-03:00
git_commit: 9ba4d0f0074e9081dad46dfccebda2ffe8843265
---

# Overview

A first pass listed 34 findings across the workspace. A second pass at
`9ba4d0f0074e9081dad46dfccebda2ffe8843265` kept 15. Dropped claims usually
named the right file and the wrong failure: a guard, timeout, `cfg`, or a
caller that never takes that path.

One hang is real. The rest are input bugs, FFI footguns, and performance
smells.

# Hang

## PTY writer waits forever on the terminal thread

`crates/zz-terminal/src/session.rs` `PtyWriter::write`

The master PTY is made nonblocking with `ioctl_fionbio` on a `dup`. Writer,
drain, and master share that open-file flag. When the kernel write buffer is
full (`EAGAIN`), `PtyWriter::write` calls `rustix::event::poll(..., None)` on
the `zz-terminal` thread and retries.

That thread is the one that handles `Command::Resize` and `Command::Shutdown`.
A child that stops reading (job-control stop, slow stdin consumer) can leave
the pane frozen until the child reads again.

There is no separate writer thread. `wait_for_wake` is a different wait with a
bounded timeout.

# Input

## Prefix interceptor runs before dialog keymap

`crates/zz/src/workspace/view.rs` `AppView::intercept_keystroke`

The interceptor is registered with `App::intercept_keystrokes` and runs before
keymap dispatch. It skips Settings and platform/function keys. It does not
check `Root::has_active_dialog` (that is the real name; `has_active_modal`
does not exist).

Unarmed, only the prefix chord is claimed (stock `C-b`). Armed, every
non-platform key is forwarded to the mux active pane and
`cx.stop_propagation()` runs, so dialog Enter/Escape never fire. SSH secret
and add-host dialogs use `InputState`. Host-key and agent confirms are
buttons only.

With no attached session there is no active pane and the interceptor returns
without stopping propagation.

## `Alt-Ctrl-x` stores a different map key than live Ctrl+Alt

`crates/zz-protocol/src/key.rs` `canonical_key`, `input_key_name`

`canonical_key` peels prefixes in encounter order. `canonical_key("Alt-Ctrl-x")`
stores `M-C-x`. Live input always emits Control then Alt (`C-M-x`), tested as
`C-M-Left` / `C-M-F255` / `C-M-λ`. `KeyTables::get` looks up
`canonical_key` of the live name, so a bind written `Alt-Ctrl-x` misses.

`Ctrl-Alt-x` and `C-M-x` store `C-M-x` and match. Stock tables do not ship
`Alt-Ctrl-` chords.

# Performance

## Agent git summary stages the whole worktree on every refresh

`crates/zz-daemon/src/agent/host.rs` `start_git_refresh`
`crates/zz-daemon/src/agent/git_summary.rs` `capture_git_summary`, `write_tree`

`SessionReady`, `SessionSwitched`, and `PromptFinished` each spawn a
`zz-agent-git-*` thread with no debounce and no in-flight skip. Overlapping
runs can exist; `apply_git_summary` drops stale generations.

A successful capture with HEAD runs seven sequential git processes. `write_tree`
always does `git add -A --ignore-errors .` into a scratch `GIT_INDEX_FILE`, then
`write-tree`, then two `diff-tree`s. Each command starts two reader threads and
joins them before the next command, so concurrent OS threads during one refresh
are about three, not twelve to fourteen.

## Copy-mode `9999j` clones thousands of heap commands

`crates/zz-protocol/src/key.rs` `KeyEngine::decide`

Digits accumulate a repeat count clamped at 9,999. The next copy-mode `-X`
motion clones `CommandInvocation` that many times. Stock `j` is
`send-keys -X cursor-down`: each clone copies two heap `String`s plus a `Vec`.
The daemon then executes the list one command at a time.

## Frame diff scores every row shift

`crates/zz-terminal/src/model.rs` `best_row_shift_from_fingerprints`

When previous and current cell `Arc`s differ, the shift search scores every
offset in `-(rows-1)..rows` except zero. Matches are `u64` fingerprint
equality, not full-row memcmp. At 250 rows that is about 62,500 `u64`
compares plus an O(rows × columns) fingerprint build. The daemon pane watcher
runs this on coalesced `ViewportReady` events.

## FFI snapshot getters walk the layout tree per property

`crates/zz-client-ffi/src/ffi.rs` `pane_at`

`pane_id`, `pane_title`, `pane_kind`, `pane_is_active`, and `pane_has_bell`
each call `pane_at`, which allocates a `Vec` and walks `LayoutNode::panes`.
iOS `refreshSnapshot` does all five getters for every pane index: 5N walks
for N panes in the active window. `pane_count` uses `window.panes.len()` and
does not walk.

# FFI

## Uncaught panic in an `extern "C"` export aborts the process

`crates/zz-client-ffi/src/ffi.rs` (44 `extern "C"` exports)

None of the exports wrap `catch_unwind`. There is no wrapper in `lib.rs`.
Rust 1.81+ aborts an uncaught panic leaving `extern "C"`; MSRV is 1.97. This
is process abort, not silent undefined behavior. Lock poison is recovered with
`PoisonError::into_inner`. Consumers are the C smoke client and the iOS app.

## Connect queues hello without writing the wake fd

`crates/zz-client-ffi/src/ffi.rs` `zz_client_connect`

Connect feeds `ServerHello` into `ClientCore`, drains `poll_event` into the
queue (`ZZ_EVENT_HELLO`), and starts the reader. Wake bytes are written only
from the reader after a later `recv()`, or on disconnect.

`zz_client_next_event` pops the queue even if the fd is dry. In-tree iOS and
the C smoke client drain that way, so they see hello. A client that blocks on
`zz_client_event_fd()` with the documented poll-then-drain contract, before
any later daemon message, waits on a fd that is not readable.

## `zz_bytes` is pointer plus length, not a C string

`crates/zz-client-ffi/include/zz-client.h` `zz_bytes`
`crates/zz-client-ffi/src/ffi.rs` `ZzBytes::new`

`ZzBytes::new` points at a Rust `String` with `as_ptr()` and `len`. No trailing
NUL is guaranteed. The header does not mention `%s`. In-tree C compares with
`len` and `memcmp`. Swift decodes `UnsafeBufferPointer` of `len` bytes.
`zz_viewport_row_text` does write a NUL into the caller buffer; snapshot
name and title do not.

# Smaller

## Mux reducer reads process cwd for editor panes

`crates/zz-mux/src/command.rs` `select_pane_kind`

The editor arm of `select_pane_kind` calls `std::env::current_dir()` so
`EditorDescriptor.cwd` can pass validation. `ExecutionContext` has no cwd
field. After `PaneMaterialized`, the daemon overwrites editor cwd from the
donor terminal when that PTY has a working directory, and falls back to
`current_dir()` again if it does not. Agent kind uses `-c` and does not take
this path in the reducer.

## Copy-pipe stages stdin through a tempfile

`crates/zz-daemon/src/daemon.rs` `run_copy_pipe`

Selection text (capped at 32 MiB) is written to `tempfile::tempfile()`, seeked
to start, and handed to the child as stdin. Exit is polled every 20 ms for up
to 30 s. stdout and stderr are discarded. Same path on every platform.

## Windows DPAPI buffer is not zeroed before `LocalFree`

`crates/zz-chrome-import/src/cookie.rs` `dpapi_unprotect`

`CryptUnprotectData` fills a `LocalAlloc` buffer. The code copies those bytes
into `Zeroizing<Vec<u8>>` and `LocalFree`s `pbData` without `SecureZeroMemory`
on the allocator buffer. Windows-only. Same-user Chrome profile import from
`%LOCALAPPDATA%\Google\Chrome\User Data`.

## AppKit `HANDLING_SEND_EVENT` restore is not RAII

`crates/zz-browser/src/mac_app_protocol.rs` `send_event`

The swizzled `sendEvent:` swaps the flag to true, calls the previous IMP
(`extern "C-unwind"`), then stores the outer value. There is no `Drop` guard
and no `catch_unwind`. A panic or Objective-C exception from the original IMP
skips the restore. Later `setHandlingSendEvent:` can still write false.
macOS-only.

## macOS wake pipe drains 64 bytes once

`crates/zz-terminal/src/session.rs` `wait_for_wake` (macOS/BSD)

Each `ActorWake::notify` writes one byte. The wait path reads at most 64 bytes
in a single `read`. Leftover bytes keep `POLLIN` set, so the next poll returns
without a new write and the loop takes `Wake::Deadline` until the pipe is
empty. Commands still sit on their channels. Linux uses `select_biased!` and
has no wake pipe.

# Next

1. Fold PTY writes into the actor wait loop, or give `poll` a timeout, so
   resize and shutdown can run while the child is not reading.
2. Return from `intercept_keystroke` when `Root::has_active_dialog` is true.
3. Debounce git refresh onto one worker, and replace `git add -A` with a
   non-staging status query.
4. Write a wake byte from `zz_client_connect` when the hello event is queued.
