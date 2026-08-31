---
type: Rust Crate
title: zz-gtk crate . GTK4/libadwaita GNOME client
description: A functionally equivalent GNOME port of the desktop client built entirely on ClientCore - engine/ui split, per-host reconnect supervisor, sidebar, settings, palette, fleet hosts over ssh - workspace-excluded because it needs system GTK.
resource: crates/zz-gtk/src/lib.rs
tags: [gtk, gnome, client, libadwaita, fleet, crate]
timestamp: 2026-08-15T00:00:00Z
---

# Overview

`crates/zz-gtk` is the native GNOME client for zz: GTK 4.22 + libadwaita 1.9
over the sans-IO [`crates/zz-client`](/crates/zz-protocol.md) `ClientCore`,
with the daemon owning every piece of mux and terminal state. It is
**workspace-excluded** (its own `[workspace]` root, own `Cargo.lock`,
replicated libghostty `[patch]`) because gtk4-sys needs system libraries the
mac CI lacks; build and run it with `just gtk` or `cargo` from the crate
directory. It landed on the `gtk-client` branch as a multi-agent build; the
distilled lessons live in the `new-client` skill
(`.agents/skills/new-client/`, see `references/gtk.md`).

# Architecture

- `src/engine/` has **zero GTK imports** and is testable from a plain
  `#[test]`: `Engine` (accessors answer about the *active* host) over a
  host-keyed `Fleet` of `Link`s (connection-outliving state: core, frame
  inbox, geometry cache, history rings), each pumped by its own
  `reader.rs` supervisor with a reconnect ladder (local: 100ms→2s within a
  30s window; fleet hosts: the desktop's 1/2/4/8/16/30s), frozen frames
  while retrying, `adopt_hello`-only re-ingestion, remembered-session
  re-attach with `MissingTarget` fallback, and geometry replay.
- `src/ui/` is the libadwaita shell: a custom `TerminalView` widget painting
  resolved cells with per-row cached render nodes and style-run Pango
  layouts, with `EventControllerLegacy` preserving hardware-keycode
  press/repeat/release pairing while IM commits are handled through manual
  `filter_keypress`; a window capture controller that claims the shared `ui`
  `ChromeKeymap` before focus-specific surfaces, while yielding to daemon-owned
  modal overlays; the custom terminal also implements `GtkAccessibleText` from
  a lazily cached visible-grid snapshot, exposing Unicode-character content,
  caret, and selection ranges without taxing the frame path until assistive
  technology queries it; a fixed-width
  `AdwOverlaySplitView` sidebar (session tree ported from the desktop's
  `MuxTreeModel`) that a scale-aware `AdwBreakpoint` collapses to an overlay
  under 640sp;
  the focused zz window's `PaneGrid` alone as the workspace, with no tab strip
  — windows are switched from the tree; an adaptive `AdwDialog` preferences
  shell using `AdwSidebar` and `AdwNavigationSplitView`, collapsing to
  single-page navigation at narrow widths, while its `AdwPreferencesPage`
  content shares the desktop's `zz/config` file through a comment-preserving
  writer and a 500ms poller as the single apply path; a native
  `AdwShortcutsDialog` generated from the live `ChromeKeymap`, rebuilt from
  the ordered `chrome-keybind` and `chrome-unbind` entries on every config poll;
  daemon-driven overlays (choosers, display-panes, command palette with the
  desktop's completion ranker); prefix-claim capture interceptor
  (`EventControllerLegacy`, hardware-keycode pairing); search strip, output
  pager, backfill-only scrollback ring, split-divider drag; ksni tray.
- Fleet hosts are `host-<name> = <destination>` config lines (ssh or
  `unix://`); ssh prompts ride `SSH_ASKPASS` pointing back at the zz-gtk
  binary itself, exactly like the desktop.

# Testing

`tests/engine.rs` runs everything against real in-process daemons: attach/
echo, reconnect through a cuttable unix-socket `Relay` (an in-process daemon
never drops clients on `kill-server`), multi-pane frame routing, resize
dedup, a cross-client convergence oracle against a plain `ClientCore`
reference, frame-flood coalescing, and two-daemon fleet scenarios.
`tests/palette.rs` proves the daemon prompt round-trip through the real key
path. Browser (CEF) and agent (ACP) panes are deliberately not ported — they
are local runtimes, not protocol views; those panes render placeholders and
GUI requests are answered with polite errors.
