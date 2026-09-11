---
type: Rust Crate
title: zz-gtk crate . GTK4/libadwaita GNOME client
description: Native GTK4/libadwaita client for terminal, daemon-owned Agent, and CEF Browser panes, with shared client state, fleet connections, and GNOME navigation.
resource: crates/zz-gtk/src/lib.rs
tags: [gtk, gnome, client, libadwaita, fleet, crate]
timestamp: 2026-09-07T00:00:00Z
---

# Overview

`crates/zz-gtk` is the native GNOME client for zz: GTK 4.22 + libadwaita 1.9
over the sans-IO [`crates/zz-client`](/crates/zz-client.md) `ClientCore`,
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
path. `tests/fixtures/agent.awk` supplies a deterministic ACP adapter for the
engine's permission, prompt, transcript, and replay test.

# Current refresh

The `gtk-client` branch rebased onto `04d28cb1` on 2026-09-07. The pre-refresh
GNOME changes remain in checkpoint `4cdf4fb5` and backup branch
`codex/gtk-checkpoint-20260907`.

- Window activation uses a separate `ClientFocus` lifecycle. The engine caches
  activation during attachment and replays it after reconnect or session attach.
- The compact header uses `StatusBarModel`; ordinary sidebar mode shows the
  session tree. The GUI no longer displays daemon `StatusLine` fragments.
- Command prompts track publication revisions and honor single-key, numeric,
  incremental, key, and backspace-exit modes. Menu, confirmation, and terminal
  popup dialogs use the daemon's published state and actions.
- Agent panes consume daemon-owned ACP state and transcript replay. The GTK
  surface provides Markdown, tool and thought expanders, prompt queueing,
  cancellation, permission choices with tool arguments, settings, file/image
  attachments, conversation history, and draft reclaim. Sent prompts appear
  immediately and reconcile with adapter replay. The shell
  retains Agent drafts when switching windows, sessions, or fleet hosts.
- Browser panes use the existing CEF runtime with GTK memory textures, native
  URL/navigation controls and browser tabs, shared browser key routing, IME,
  captures, and adopted popup sessions. A separate GTK cache root avoids locking
  the GPUI client's active Chromium profile.
- The pane picker offers Terminal, Browser, and Agent. Editor panes remain
  outside this refresh.

# Settings parity

The GTK settings refresh follows current GPUI settings semantics through native
libadwaita pages: Interface, Terminal, Status Bar, Browser, Agents, Multiplexer,
Hosts, and System. Interface offers the installed-font chooser and native
animation preference. Terminal retains typed controls, with current value
ranges, numeric font weights, cell-height adjustment, and minimum contrast.
Controls for appearance effects GTK does not render are hidden.

Status settings drive the shared `StatusBarModel`. Browser search changes apply
to subsequent address submissions; the egress preference applies to new browser
panes. Agent controls cover enablement, adapters, approval policy, and the
working directory for new local Agent panes.

Multiplexer uses `zz_mux::settings_bindings`, also used by GPUI, for split key
and pane-type edits. It displays discovered config paths, preserves drafts,
checks external changes before saving, and waits for daemon confirmation of
shortcut edits. Reload and split changes require a connected local session.
A copied-tmux notice can remove a matching legacy prefix from the draft; only
Save writes it. Ghostty import resolves included files and active-scheme theme
colors through the shared appearance loader; it never copies tmux files.

System includes a bounded editor for the shared `zz/config` file, with explicit
Save, external-edit checks, and confirmation before discarding a draft. Unlike
GPUI's appearance-only Terminal editor, this page exposes the full shared file.
Typed controls still apply through the config watcher, which refreshes the
status and other GTK consumers after a successful write.

# Build and validation

Run `just gtk [--socket /tmp/zz.sock] [session]`. The launcher uses
`crates/zz-gtk/target` by default (`ZZ_GTK_TARGET_DIR` overrides it), builds with
its locked dependencies, checks CEF resources, and sets the library path for
the client and its subprocesses. It also builds the matching `zz` executable
in the workspace target directory (`CARGO_TARGET_DIR` overrides it). If the
local socket is absent or refuses connections, GTK starts that executable as
a detached daemon. A launch without a session creates the default session in
the launch directory when needed and reuses it on later launches. Explicit
session names retain ordinary attach semantics. Direct binary launches look
for a protocol-compatible `zz` beside `zz-gtk` and then on `PATH`;
`ZZ_GTK_DAEMON_EXECUTABLE` selects an explicit executable. Keep `cef-dll-sys` aligned with the workspace
lockfile when updating the browser dependency.

```sh
cargo test --manifest-path crates/zz-gtk/Cargo.toml --all-targets
cargo clippy --manifest-path crates/zz-gtk/Cargo.toml --all-targets -- -D warnings
cargo fmt --manifest-path crates/zz-gtk/Cargo.toml --check
```

Validation on 2026-09-07: all 193 GTK tests pass, GTK and workspace clippy pass,
and changed Rust files pass formatting. Live isolated-display checks cover
terminal input, Agent prompts/permissions/draft restoration, adaptive layout,
popup terminals, CEF input/popup adoption, CEF shutdown, and cold local daemon
startup with session reuse across GTK closure and reopening. Settings checks
cover persisted animation changes, live split-key edits confirmed by the
daemon, draft preservation across page changes, refusal to overwrite external
edits, invalid-value rollback, copied-tmux cleanup, and the narrow layout.

The full workspace run still has the existing
`explicit_boot_configs_replace_default_discovery_and_load_in_order` failure
(`C-a` versus `C-x`), reproduced separately on clean `main` and again alone
during the settings refresh. The control-source output-order test passed in
the initial run, failed during a parallel repeat, and passed alone. An
additional daemon-suite continuation ended with SIGTERM before completion.
All workspace packages outside `zz` and `zz-daemon` pass their tests and
doctests in a separate completed run. Workspace-wide formatting reports
pre-existing drift in unchanged `zz` config/workspace and `zz-tui` files.

CEF's external message pump needs a GTK-specific GLib integration fix, qualified
against the locked `cef`/`cef-dll-sys` 151.2.0 build. CEF's
[`MessagePumpExternal`](https://github.com/chromiumembedded/cef/blob/master/libcef/browser/browser_message_loop.cc)
runs work directly instead of entering its inherited GLib pump. Chromium's
[`MessagePumpGlib`](https://chromium.googlesource.com/chromium/src/+/refs/heads/main/base/message_loop/message_pump_glib.cc)
still attaches an unused work source to the default GLib context; its null run
state requests zero-timeout polling. Unlike GPUI, GTK continuously drives that
context, causing a busy loop. `src/ui/browser/runtime/glib_pump.rs` brackets CEF
startup with source ID markers and detaches exactly one newly registered,
unnamed, recursive idle source whose prepare/check/dispatch/finalize callbacks
all belong to `libcef.so`, verified with `dladdr`. It leaves GTK sources and
CEF's other sources intact, and CEF retains its reference for normal shutdown.
Requalify this integration when upgrading CEF; a changed source signature
produces an explicit initialization error. The GPUI runtime defaults are
unchanged. Live checks must cover idle CPU, page input, popup adoption, and
clean shutdown, not only a successful build or page title update.

For isolated GUI checks, build `--example gtk-fixture`. Set a short `ZZ_SOCKET`
path and run `gtk-fixture --daemon`; other arguments execute daemon commands.
Run the GTK binary on a separate virtual display with an isolated
`XDG_CONFIG_HOME`. Copy the binary and staged CEF resources into a private
runtime directory before running builds concurrently; CEF build scripts can
replace mapped libraries in the target directory. Do not inject test input
into the user's live desktop.

# Remaining differences

The terminal renderer does not yet paint Kitty image payloads. Popup dialogs
use native dialog geometry rather than tmux's exact cell-positioned border and
pointer-drag behavior. Agent transcript image/audio blocks use descriptive
placeholders, and tool terminal output lacks the GPUI live terminal surface.
The editor and browser element selector are outside this refresh. SSH browser
egress uses the shared managed SOCKS route, but has not been exercised against a
live remote host in this refresh.
