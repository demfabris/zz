---
type: Rust Crate
title: zz-client crate
description: The renderer-free client core that reduces protocol messages into shared state and typed effects, plus the client-local chrome keymap used by desktop and TUI skins.
resource: crates/zz-client/src/lib.rs
tags: [client, core, sans-io, keybindings, crate]
timestamp: 2026-08-26T00:00:00-03:00
---

# Overview

`zz-client` is the renderer-free client brain shared by the GPUI app, the raw-terminal client, and
the C ABI. It depends only on `zz-protocol` and the model-only half of `zz-terminal`. It owns no
socket, thread, clock, renderer, or toolkit type.

`ClientCore` accepts decoded `ProtocolMessage`s and retains the current handshake data, mux
snapshot, attachment, terminal viewports, daemon key-table snapshots, status, prefix state, and
daemon-owned overlays. A shell drains two queues after each message:

- `Outbound::RequestFull(pane)` asks the transport to repair a patch that could not apply.
- `CoreEvent` reports state changes and carries effects that the core does not retain, including
  clipboard writes, URI opens, browser/terminal commands, GUI work, history, and Kitty image data.

`InteractiveClient` consumes `ServerHello` during connection setup, so each shell seeds the core
with `client.server_hello().clone()` before starting its receive loop. A reconnect may call
`adopt_hello` alone to keep frozen frames, or handle the full hello to reset attachment and overlay
state.

Connection-time caller facts stay outside this crate. `zz-daemon::InteractiveClient` constructs a
local Control hello with the process cwd, an stdin-only `client-tty-v1:` identity when available,
and `client-nested-v1` for a nonempty `$TMUX`. `ClientCore` neither discovers nor retains those
facts. The Control path sends no `client-size-v1:` fact and no `ClientTerminalSize` update, so it
adds no implicit geometry or renderer state here.

`ClientCore` accepts and ignores the Control-only v77 `ControlCommandGuard` event, including its
frame flags and independent sticky-status bit. It does not own `-C`
frame rendering, stdin ordering, or process exit state; `crates/zz/src/control_mode.rs` owns those
front-end concerns. That front end now closes `control-mode.source-file-exit-status` by retaining the
pin's bounded retval. A Return captured during a preceding non-detach command precedes later queued
stdin, while a Return observed during self-detach is discarded when the caller receives its own
`Detached` event. The shared interactive reducer needs no new state or wire message. Generic config
Warning typing remains separate.

# Desktop hot-path boundary

The desktop `MuxClient` sends non-frame messages through `ClientCore` and drains the resulting
requests and events into GPUI state. It intercepts `TerminalViewport`, `TerminalPatch`, and
`CommandOutput` first because `RetainedTerminalViewport` also owns history, row revisions, and paint
diff scratch. Passing those frames through both stores would duplicate the highest-rate work. The
TUI and C ABI use the core's viewport retention directly.

# Chrome keymap

`ChromeKeymap` stores the client-owned `ui`, `sidebar`, `browser`, and `terminal` tables. It uses the
same `KeyTables` resolution rules as the daemon contract, with a chrome-only grammar that preserves
Command/Super (`D-`) and Shift (`S-`) chords. `ChromeProfile` supplies TUI, desktop, and Apple
defaults; `chrome-keybind` and `chrome-unbind` apply user overrides. Skins resolve a key to a
`ChromeAction` and apply the action instead of matching chords in view code.

# Testing

`tests/simulator.rs` boots a real daemon and drives seeded client cores. It checks snapshot and
viewport convergence, patch/full duality, and recovery after a requested full frame. Sequence gaps
are not errors because the daemon may supersede an unread terminal frame under backpressure.

# Key files

| File | Role |
| --- | --- |
| `crates/zz-client/src/lib.rs` | Crate facade and public contract. |
| `crates/zz-client/src/core.rs` | `ClientCore`, retained state, protocol reduction, `CoreEvent`, and `Outbound`. |
| `crates/zz-client/src/chrome.rs` | `ChromeKeymap`, profiles, action names, chord grammar, defaults, and override API. |
| `crates/zz-client/tests/simulator.rs` | Real-daemon deterministic convergence harness. |

# Related

- [Wire protocol](/protocol/wire-protocol.md) and [key tables](/tmux/key-tables.md) supply the
  renderer-neutral contract.
- [GPUI client](/crates/zz.md) uses the hybrid core plus retained-painter path.
- [C ABI](/crates/zz-client-ffi.md) wraps the core for non-Rust consumers.
- [Client core decision record](/designs/client-core-and-contract.md) records the extraction and its
  remaining ABI scope.
