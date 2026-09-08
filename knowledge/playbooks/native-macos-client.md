---
type: Playbook
title: Native macOS terminal client
description: Build the Swift terminal client, connect it to a zz daemon, and verify native input, layout, and reconnect.
resource: clients/macos/Sources/ZZNative/NativeApp.swift
tags:
- swift
- macos
- client
- terminal
timestamp: 2026-09-08T00:21:10Z
---

# Build and connect

Run from the repository root on macOS with Xcode and the repository's Rust toolchain:

```sh
just macos-native
just macos-native --socket /tmp/zz.sock --session work
just macos-native-build release
just macos-native-test
```

The build script compiles `zz-client-ffi`, statically links `ZZNative`, and signs
`clients/macos/dist/zz Native.app` with the separate `sh.zzmux.native` identifier.
The existing GPUI app and its daemon stay installed. The native app connects to
a running daemon; it does not start one. Without a socket argument it asks Rust
for `default_socket_path()`, including the `ZZ_SOCKET` override. The connection
sheet accepts a socket path and an optional session name. An empty session uses
the daemon's default attach behavior.

`ZZ_MACOS_FFI_DIR` lets direct Swift builds locate `libzz_client_ffi.a`; the build
recipe derives it from Cargo metadata. The component gallery remains a separate
product in the same Swift package. See [the gallery guide](/playbooks/native-macos-gallery.md).

# Client boundary

`Sources/ZZNativeCore/NativeClient.swift` opens the Rust connection away from the
main actor and drains its wake fd with a main-queue `DispatchSourceRead`. A stale
connection attempt releases its result. The connection owner cancels the source
before freeing the Rust handle, whose destructor shuts down and joins its reader.

Swift retains session, window, and pane presentation values from owned snapshot
handles. The daemon supplies pane rectangles, selection, active windows, and zoom.
The shell sends commands through `zz_client_execute_request` and displays errors
from its reply queue. Prefix hints come from the daemon's published table; Swift
forwards raw terminal keys to Rust for binding resolution.

`Sources/ZZNativeCore/TerminalFrame.swift` retains the viewport handle and borrows
its immutable cells, styles, and grapheme dictionaries. The event loop acquires
one latest frame per changed pane after draining events. Each terminal observes
its own frame slot so output does not invalidate the workspace tree.

`Sources/ZZNative/TerminalSurface.swift` paints the grid through AppKit. It handles
wide-cell spacers, Unicode graphemes, foreground/background colors, bold, italic,
faint and invisible text, basic decorations, and the cursor. It implements
`NSTextInputClient` for composition and candidate placement. Selection, copy,
scrolling, paste encoding, terminal key encoding, and PTY ownership remain in Rust.

The view reports its cell dimensions in backing pixels and its available grid
size. It reports again after reconnect, activation, and backing-scale changes.
The daemon derives the complete window size from the active pane. Split rounding
can make the returned grid one cell wider than the reported size; the view uses
the returned viewport. `zz-mux` owns this policy in `set_pane_geometry` and
`back_solved_extent_axis`.

A lost connection leaves the last frame visible while reconnect attempts back
off. A successful connection replaces the Rust handle and reattaches the
remembered session. Quitting the native app detaches its client and leaves the
daemon's sessions running.

# Current scope

As of 2026-09-08, the native app supports terminal output and input, paste,
selection/copy, scrolling, split creation, zoom, window creation/switching/renaming,
session navigation, and reconnect. The sidebar and status use the native `ZZUI`
components built for the gallery.

Browser, agent, and editor adapters remain to be connected. Full chrome-keymap
actions, daemon appearance/configuration, terminal images, terminal history/search
presentation, mouse reporting to terminal applications, and daemon chooser and
command-prompt overlays still need interface or presentation work. The first
renderer uses AppKit drawing; it is not the GPUI painter or a Metal renderer.
It currently redraws a changed pane's visible rows. Wavy, dotted, and dashed
underlines render as a simple underline. This is a functional terminal milestone,
not desktop feature parity.

# Verification

`just macos-native-test` builds the real daemon fixture in
`crates/zz-client-ffi/examples/native_fixture.rs` and runs the combined Swift tests.
The fixture uses a private `/tmp` socket, no user configuration, and a deterministic
`cat` session. The integration test exercises styled Unicode, typed and pasted
input, resize, split geometry, pane/window selection, zoom, retained-frame lifetime,
manual reconnect, and automatic recovery after a daemon restart. Tests without
`ZZ_NATIVE_TEST_FIXTURE` skip the daemon integration case; use the recipe for the
complete check.

The default-endpoint test checks buffer sizing, truncation, and NUL termination
through the C import. `cargo test -p zz-client-ffi` also checks the linked C client,
terminal paste, and command-output cancellation against real daemons.

For native QA, connect to a private fixture, type and paste a shell command,
split the terminal, switch windows, resize the app, and reconnect. Check that the
active pane receives input and that closing the app preserves the session.
