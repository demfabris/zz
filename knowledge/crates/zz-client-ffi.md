---
type: Rust Crate
title: zz-client-ffi crate
description: Unix C ABI over zz-client for native shells, with interactive SSH, Agent supervision, pollable events, mux snapshots, semantic terminal actions, and caller-owned styled viewports.
resource: crates/zz-client-ffi/include/zz-client.h
tags: [client, ffi, c-abi, unix, ios, ipad, crate]
timestamp: 2026-08-30T00:00:00-03:00
---

# Overview

`zz-client-ffi` exports a Unix C ABI over `ClientCore` and `InteractiveClient`. It builds as a Rust
library, static library, and dynamic library. The native Apple app links the static library and
imports the hand-maintained `include/zz-client.h` contract through a Swift bridging header.

One `zz_client` owns the daemon connection, blocking reader thread, reduced core, event queue, and
one end of a nonblocking Unix socket pair. Native main loops poll `zz_client_event_fd()`, drain
`zz_client_next_event()`, then acquire the immutable state they need. `zz_client_free()` shuts down
the transport, joins the reader, and closes the wake descriptor.

# Current ABI

The header exposes:

- local socket connection plus parsed local or SSH endpoint connection, typed retryable,
  authentication, host-key, configuration, and incompatibility failures, an interactive prompt
  callback for trust, secrets, confirmations, and keyboard-interactive batches, the iOS app
  identity's OpenSSH public key, free, the wake descriptor, typed events, appearance changes,
  disconnects, pane IDs, and viewport damage rows;
- attach, literal text, raw key press/repeat/release, tmux-style command execution, terminal resize,
  client-window focus, terminal pane/application focus, line scrolling, semantic selection, and
  asynchronous copy requests with typed clipboard results;
- caller-owned mux snapshots with generation, session identity/name/attachment, compatibility
  accessors for the active window, the full window and pane hierarchy, zoom state, and normalized
  visible pane rectangles;
- caller-owned Agent summaries with phase, attention, title, error, queue count, permission request
  and options, and git summary, plus permission-response and cancellation actions;
- caller-owned terminal viewports with dimensions, generation counters, default colors, raw cells,
  style records, grapheme offsets/bytes, and cursor state;
- decoded UTF-8 row text for simple consumers that do not need graphical fidelity.

`zz_client_attach` returning true means the client wrote the request; `ZZ_EVENT_ATTACHED` confirms
the attachment. A shell sends `zz_client_set_focused` after that event. The function carries the
client-window signal. `zz_client_focus_terminal` remains the independent pane/application signal and
may accompany it when an owned terminal follows the outer scene transition.

Mux snapshots own an `Arc<MuxSnapshot>`. Pane order comes from each window's layout tree. The shared
client layout solver turns that tree into normalized rectangles, so native shells do not reproduce
split semantics. A zoomed pane occupies the full rectangle while its siblings remain available as
tree metadata. Viewport snapshots share the core's immutable planes and remain valid until explicit
release.

`zz_client_connect_endpoint_interactive()` accepts the same endpoint strings as
`zz_daemon::Endpoint`. On iOS, SSH stays in process and calls the native shell with the server's
exact prompt text and echo policy for unknown or changed host keys, keyboard-interactive prompts,
and password authentication. The callback returns trust-once, trust-and-save, a bounded answer, or
cancel. The older password-only endpoint function remains available. Both paths advertise the
portable terminal-surface facts, copy display text into a caller-owned bounded buffer, and classify
failures separately so shells retry only transport-class failures. `zz_client_ssh_public_key()` uses
the usual size-query pattern and returns the app's Keychain-backed Ed25519 public identity for host
setup.

Agent state remains daemon-owned and is retained by `ClientCore`; acquiring a state object is a
cheap immutable snapshot. `ZZ_EVENT_AGENT_STATE_CHANGED` carries lossless request, completion, and
first-failure edge flags in addition to the current state. Clipboard extraction is likewise typed:
the caller sends a request ID, receives `ZZ_EVENT_CLIPBOARD`, and drains caller-owned clipboard
objects rather than reconstructing text from viewport cells.

`zz_viewport_row_text()` resolves scalar and interned-grapheme glyphs, emits one visible glyph for a
wide cell, preserves blank cells as spaces, and truncates only between complete UTF-8 sequences.
Graphical clients should consume the cell/style/grapheme planes directly.

# Scope boundary

The ABI is renderer-neutral and sufficient for the native iPhone terminal, iPad split workspace,
and Agent-supervision slices. It still does not export the command catalog, live chrome key
tables/actions, history chunk access, Kitty image extraction, multi-host selection, the retained
daemon-expanded status payload, the heavy Agent transcript stream, or Browser and Editor viewport
data. `ZZ_EVENT_STATUS_CHANGED` can wake a shell, but no status snapshot accessors currently let it
read the formatted fields. Those remain shared-core work rather than Swift responsibilities.

# Testing

`tests/smoke.c` rejects an invalid interactive endpoint with a typed configuration failure, connects
a parsed local endpoint to a real in-process daemon, writes an attach request, waits for attached
mux/session/pane metadata, creates and attaches another session, creates a second pane, reads styled
terminal planes, sends raw Enter, exercises selection, copy, Agent, and clipboard symbols, reports
client focus and blur through `zz_client_set_focused`, kills that attached session, observes the
detached snapshot, explicitly reattaches the surviving session, recovers its terminal content, then
frees and reconnects in the same C process. `zz_client_focus_terminal` remains the separate
pane/application signal.

Rust tests cover endpoint failure classification, SSH prompt mapping, normalized split geometry,
interned graphemes, wide-cell spacers, UTF-8-safe truncation, and the real-daemon link boundary. The
Apple build cross-compiles the crate for `aarch64-apple-ios-sim` on every Xcode build.

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
- [Native Apple client](/designs/ios-client.md) is the first graphical consumer.
- [Packed terminal lanes](/protocol/terminal-lanes.md) describe the viewport representation.
