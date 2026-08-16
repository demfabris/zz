---
type: Rust Crate
title: zz-client-ffi crate
description: Unix C ABI over zz-client for native shells, with pollable events, mux snapshots, raw terminal input, and caller-owned styled terminal viewports.
resource: crates/zz-client-ffi/include/zz-client.h
tags: [client, ffi, c-abi, unix, ios, crate]
timestamp: 2026-08-15T00:00:00Z
---

# Overview

`zz-client-ffi` exports a Unix C ABI over `ClientCore` and `InteractiveClient`. It builds as a Rust
library, static library, and dynamic library. The native iPhone app links the static library and
imports the hand-maintained `include/zz-client.h` contract through a Swift bridging header.

One `zz_client` owns the daemon connection, blocking reader thread, reduced core, event queue, and
one end of a nonblocking Unix socket pair. Native main loops poll `zz_client_event_fd()`, drain
`zz_client_next_event()`, then acquire the immutable state they need. `zz_client_free()` shuts down
the transport, joins the reader, and closes the wake descriptor.

# Current ABI

The header exposes:

- connect/free, the wake descriptor, typed events, appearance changes, disconnects, pane IDs, and
  viewport damage rows;
- attach, literal text, raw key press/repeat/release, tmux-style command execution, terminal resize,
  terminal focus, and line scrolling;
- caller-owned mux snapshots with generation, session identity/name/attachment, active window, and
  ordered active-window pane metadata;
- caller-owned terminal viewports with dimensions, generation counters, default colors, raw cells,
  style records, grapheme offsets/bytes, and cursor state;
- decoded UTF-8 row text for simple consumers that do not need graphical fidelity.

Mux snapshots own an `Arc<MuxSnapshot>`. Pane order comes from the active window's layout tree, so a
client can present a stable visual order without rebuilding target resolution. Viewport snapshots
share the core's immutable planes and remain valid until explicit release.

`zz_viewport_row_text()` resolves scalar and interned-grapheme glyphs, emits one visible glyph for a
wide cell, preserves blank cells as spaces, and truncates only between complete UTF-8 sequences.
Graphical clients should consume the cell/style/grapheme planes directly.

# Scope boundary

The ABI is renderer-neutral and sufficient for the native iPhone terminal slice. It still does not
export the command catalog, live chrome key tables/actions, layout rectangles, history chunk access,
Kitty image extraction, managed SSH connection setup, or richer Agent/Browser/Editor viewport data.
Those remain shared-core work rather than Swift responsibilities.

# Testing

`tests/smoke.c` connects to a real in-process daemon, attaches, validates mux/session/pane metadata,
creates and attaches another session, creates a second pane, reads styled terminal planes, sends raw
Enter, kills that attached session, observes the detached snapshot, explicitly reattaches the
surviving session, recovers its terminal content, then frees and reconnects in the same C process.
Rust tests cover interned graphemes, wide-cell spacers, UTF-8-safe truncation, and the real-daemon
link boundary. The iPhone build cross-compiles the crate for
`aarch64-apple-ios-sim` on every Xcode build.

# Key files

| File | Role |
| --- | --- |
| `crates/zz-client-ffi/include/zz-client.h` | Hand-maintained public C contract. |
| `crates/zz-client-ffi/src/ffi.rs` | Lifecycle, transport reader, wake fd, reduction, snapshots, and exports. |
| `crates/zz-client-ffi/tests/smoke.c` | From-scratch C consumer with live daemon assertions. |
| `crates/zz-client-ffi/tests/smoke.rs` | Harness that compiles, links, and runs the C client. |
| `clients/ios/Support/ZZ-Bridging-Header.h` | Swift import point for the ABI. |

# Related

- [zz-client](/crates/zz-client.md) supplies the reduced state and event model.
- [Native iPhone client](/designs/ios-client.md) is the first graphical consumer.
- [Packed terminal lanes](/protocol/terminal-lanes.md) describe the viewport representation.
