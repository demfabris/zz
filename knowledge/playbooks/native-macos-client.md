---
type: Playbook
title: Native macOS client
description: Build and verify the Swift terminal, ACP agent, CEF browser, and shared settings client.
resource: clients/macos/Sources/ZZNative/NativeApp.swift
tags:
- swift
- macos
- client
- terminal
timestamp: 2026-09-08T02:00:00Z
---

# Build and connect

Run from the repository root on macOS with Xcode and the repository's Rust toolchain:

```sh
just macos-native
just macos-native --socket /tmp/zz.sock --session work
just macos-native --config /tmp/native-config --mux-config /tmp/native-mux
just macos-native-build release
just macos-native-test
```

The build script compiles `zz-client-ffi` with `native-browser`, links `ZZNative`,
and bundles CEF with `ZZNative Helper.app` and its renderer/GPU helpers. It signs
`clients/macos/dist/zz Native.app` with the separate `sh.zzmux.native` identifier.
`MACOSX_DEPLOYMENT_TARGET=14.0` applies to Rust C dependencies as well as Swift.

Release Chromium uses macOS Keychain for its Safe Storage key. A Keychain access
prompt can hold page loading until the user answers it, even while renderer
processes and the CEF event pump are running. Debug builds use the shared runtime's
development keychain setting. If release pages remain blank, check for that system
prompt before changing rendering or sandbox settings.

Without a socket argument, the app asks Rust for `default_socket_path()`, including
`ZZ_SOCKET`. It starts the bundled daemon helper when that default socket is absent.
An explicit `--socket` connects to an existing daemon. The connection sheet also
accepts `ssh://user@host`; AppKit dialogs display Rust's trust and authentication
prompts. The auto-restart setting controls incompatible local-daemon replacement.
Helper identity checks remain in Rust.

An empty session name uses the daemon's default attach behavior. `--config` and
`--mux-config` accept absolute paths for isolated runs. The latter applies both
to daemon startup and subsequent reloads. Saved hosts use the shared config parser.

`ZZ_MACOS_FFI_DIR` lets direct Swift builds locate `libzz_client_ffi.a`; the recipe
derives it from Cargo metadata. The component gallery remains a separate Swift
product. See [the gallery guide](/playbooks/native-macos-gallery.md).

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

# Agent panes

`NativeAgent.swift` presents the Rust `zz_agent_model` transcript and controls. The
shared reducer handles replay, ordered turn boundaries, optimistic message echo
suppression, queued-prompt recovery, and preference acknowledgments. Swift keeps
composer drafts and image attachments across reconnects.

The native pane shows messages, reasoning, plans, tool output and before/after
changes. It supports image prompts, slash completion, permissions, cancellation,
queued prompt recovery, model/mode selection, authentication, and the provider's
new/list/load/delete session capabilities. A successful user setting change enters
the shared preference store; a new agent session restores that selection.
Images can come from the file chooser or clipboard. Enter and Command-Enter queue
a populated draft during a running turn. Permission shortcuts leave an engaged
composer draft alone.

# Browser panes

`NativeBrowser.swift` owns one Rust CEF runtime on the main thread. Swift presents
the daemon's tab/profile descriptors and retains CEF frames until Metal completes
rendering. IOSurface frames require no pixel copy; software frames borrow the
retained Rust buffer. `BrowserSurface.swift` supplies AppKit input, IME, cursor,
context menu, focus, and viewport updates. The application event monitor routes
pointer events to the hit browser view and retains that view during a drag, so
AppKit text composition does not interrupt page clicks. Native sheets and chrome
controls retain their own event handling.

Tabs support navigation/search, history suggestions, back/forward/reload, page
zoom, popups, profile switching, Chrome import, site-data clearing, developer tools,
and the element picker. Rust owns URL resolution, history ranking and persistence,
Chrome database import, and SSH browser egress. Native profiles live under the
`root-native` CEF directory, separate from the GPUI browser cache. Browser profiles
continue through daemon reconnects; an egress change rebuilds CEF sessions with the
current SSH SOCKS route.
Reconnect also clears unacknowledged tab-save requests so subsequent tab changes
can reach the daemon.

Browser GUI commands and keyboard encoding share the desktop Rust paths. The shell
resolves chrome bindings through `ChromeKeymap`; daemon prefix keys take priority
while editing a page or agent prompt.

# Settings

`zz-config` contains the shared parser, defaults, provenance, validation, config
writers, Ghostty import, split-binding edits, agent preferences, and release checks.
The desktop and native app use these same Rust implementations. Swift receives a
settings snapshot and sends typed JSON actions through `zz_settings_model`.

The native settings pages cover interface colors and metrics, status, panes, editor
preferences, browser, hosts, system options, terminal configuration, multiplexer
bindings, and updates. Scalar controls write atomically and refresh the client.
Configuration editors retain dirty drafts during external changes and require Save.
Multiplexer saves reload the attached daemon and display command failures.

UI zoom lasts for the current process. Native appearance follows the system or the
selected light/dark override. The tray setting controls the menu item and behavior
after closing the workspace. Release checks offer a download only when the release
contains a native macOS asset for this architecture.

The editor-pane flag and Vim preference remain shared configuration for the GPUI
editor. The native configuration editor currently uses AppKit editing. Native
editor panes, terminal images/history search, terminal application mouse reporting,
and daemon chooser/command-prompt overlays remain separate work. Terminal drawing
uses AppKit and simplifies wavy, dotted, and dashed underlines.

# Verification

`just macos-native-test` builds the real daemon fixture in
`crates/zz-client-ffi/examples/native_fixture.rs` and runs the combined Swift tests.
The fixture uses a private `/tmp` socket, no user configuration, and deterministic terminal and ACP fixtures. The terminal test exercises styled Unicode, typed and pasted
input, resize, split geometry, pane/window selection, zoom, retained-frame lifetime,
manual reconnect, and automatic recovery after a daemon restart. Tests without
`ZZ_NATIVE_TEST_FIXTURE` skip the daemon integration case; use the recipe for the
complete check. The ACP fixture covers permission replies, queued prompts,
setting acknowledgments, new-session preferences, and reconnect replay. Settings
tests use private files to check persistence, dirty drafts, live appearance, and
multiplexer reload. `tests/native_connection.rs` verifies helper startup, custom
mux configuration, and bounded handshake failures.

The September 2026 native integration run covers 42 Swift tests. Manual checks on
a local HTTP fixture cover typing followed by clicks, page links, popup tabs,
history navigation, Chrome profile discovery, context menus, wheel scrolling,
developer tools, and copying element context. Agent checks cover completion,
keyboard permissions, saved model restoration, session loading, and prefix keys
while retaining the composer draft. Palette persistence and per-run UI zoom are
also checked in the running client.

The default-endpoint test checks buffer sizing, truncation, and NUL termination
through the C import. `cargo test -p zz-client-ffi` also checks the linked C client,
terminal paste, and command-output cancellation against real daemons.

For native QA, connect to a private fixture, type and paste a shell command,
split the terminal, switch windows, resize the app, and reconnect. Check that the
active pane receives input and that closing the app preserves the session.
