---
type: Rust Crate
title: zz-client-ffi crate
description: The Unix C ABI proof surface over zz-client, with a pollable wake fd, typed events, daemon commands, and caller-owned terminal viewport snapshots.
resource: crates/zz-client-ffi/include/zz-client.h
tags: [client, ffi, c-abi, unix, crate]
timestamp: 2026-08-14T00:00:00Z
---

# Overview

`zz-client-ffi` exports a Unix-only C ABI over `ClientCore` and `InteractiveClient`. It builds as a
Rust library, static library, and dynamic library. The contract is hand-maintained in
`include/zz-client.h`; the smoke harness compiles a C program against that header and links the
result to catch symbol and layout drift.

One `zz_client` owns the daemon connection, a blocking reader thread, the reduced core, an event
queue, and one end of a nonblocking Unix socket pair. Toolkit main loops poll
`zz_client_event_fd()`, drain `zz_client_next_event()`, then read the new state. `zz_client_free()`
shuts down the transport to unblock `recv()`, joins the reader, and closes the wake fd before it
returns. A long-lived process can free and reconnect without leaving an interactive client attached
to the daemon.

# Current ABI

The header exposes:

- connect/free, event fd, and typed event drain;
- session attach, literal text, tmux-style command execution, and terminal resize;
- terminal-pane enumeration for the attached session;
- acquire/release viewport snapshots, dimensions, raw `zz_cell` storage, and decoded row text.

`zz_viewport_row_text()` resolves scalar and interned grapheme glyphs, emits one visible glyph for a
wide cell, preserves blank cells as spaces, and truncates only between complete UTF-8 sequences.
Viewport snapshots share the core's immutable planes and remain stable until release.

# Scope boundary

This is the narrow proof surface shipped for the first C consumer. It does not expose raw key
forwarding, style and grapheme tables, viewport generation counters, the command catalog, live key
tables, or chrome action events. A full GTK, Qt, or Swift renderer needs those additions before the
ABI can serve as its complete graphical contract. The broader target remains in the
[client-core decision record](/designs/client-core-and-contract.md).

# Testing

`tests/smoke.c` connects to a real in-process daemon, attaches, reads terminal rows, types through
the ABI, frees the connection, and reconnects in the same C process. Rust unit tests cover interned
graphemes, wide-cell spacers, and UTF-8-safe truncation.

# Key files

| File | Role |
| --- | --- |
| `crates/zz-client-ffi/include/zz-client.h` | Hand-maintained public C contract. |
| `crates/zz-client-ffi/src/ffi.rs` | Handle lifecycle, reader thread, wake fd, core reduction, exports, and row decoding. |
| `crates/zz-client-ffi/tests/smoke.c` | From-scratch C proof client, including reconnect after free. |
| `crates/zz-client-ffi/tests/smoke.rs` | Real-daemon harness that compiles, links, and runs the C client. |

# Related

- [zz-client](/crates/zz-client.md) supplies the reduced state and event model.
- [zz-daemon](/crates/zz-daemon.md) supplies `InteractiveClient` and transport shutdown.
- [Packed terminal lanes](/protocol/terminal-lanes.md) describe the viewport representation behind
  the snapshots.
