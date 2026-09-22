---
type: Playbook
title: Running the browser client
description: Build the shared GPUI browser client and connect it to a zz daemon through the local WebSocket gateway.
resource: clients/web/src/lib.rs
tags:
- browser
- wasm
- client
- websocket
- gpui
timestamp: 2026-09-22T18:00:00Z
---

# Overview

`clients/web` compiles the shared GPUI components from `zz-ui` and the state reducer from
`zz-client` to WebAssembly. The app shell, navigation, settings, palette, overlays, pane entities, connection reducer,
and image caches in `clients/gpui-shared/src` also compile into the native iOS GPUI app. `zz-web` serves the page and forwards the existing binary protocol
between each browser WebSocket and one daemon connection. The daemon owns sessions, PTYs,
key bindings, copy mode, and Agent processes.

# Run

Start the development daemon with `just run mac` or `just run linux`, then run from the repo root:

```sh
just web-setup
just web-build
just web-serve
```

Open `http://127.0.0.1:8081`. `just web` builds the assets, starts the gateway, and watches
the client and shared crates for changes. Refresh the page after a rebuild.
`just web-build` produces dev assets under `clients/web/dist-dev`. Dev builds use a separate
local-storage key for preferences. `just web-build-release` produces optimized release assets
under `clients/web/dist` with the ordinary zz identity. Serve those with
`ZZ_DEV_BUILD=0 cargo run -p zz-web -- --assets clients/web/dist` (port 8080 by default).
Both builds enable code highlighting (tree-sitter compiled to WASM), which needs `llvm-ar`
(`brew install llvm` on macOS); `WEB_SYNTAX_HIGHLIGHTING=0` builds without it. The grammars add
roughly 0.7 MB gzipped, so `just live-demo` leaves them out.

The dev recipes compile the gateway with `ZZ_DEV_BUILD=1` and connect to the existing
`zz-dev` daemon socket. They clear inherited `ZZ_SOCKET` and pane context. An explicit
`--socket` selects another daemon:

```sh
just web-serve --bind 127.0.0.1:8081 --socket /tmp/zz-browser.sock
```

The browser and daemon must use the same protocol version. If an installed daemon reports a
version mismatch, rebuild the client against that version or restart the daemon with the matching
binary when its sessions can be stopped.

# Remote access

Run the gateway beside the daemon on the remote host, then forward its port:

```sh
ssh -L 8080:127.0.0.1:8080 user@host
```

Open the same loopback URL locally. Keep the forwarded local port equal to the gateway port.
The gateway accepts only loopback bindings and matching HTTP Host and WebSocket Origin headers.
It has no public listener, TLS termination, or account authentication.

# Browser behavior

The page uses the same `zz-ui` shell, navigation, pane borders, browser toolbar, Agent timeline,
composer, settings groups, and overlays as the desktop client. GPUI selects WebGPU when available
and falls back to WebGL2. The page fills the viewport and reacts to browser resizing and zoom.

Terminal panes receive full frames and patches through `ClientCore`. Both clients use the
`zz-ui` terminal painter, which caches shaped rows and resolves shared style and grapheme
dictionaries. Scrollbars, selection autoscroll, cursor blinking, inactive cursor outlines, and
Kitty image placement use the shared rendering rules. Keyboard input, text composition,
selection, scrolling, search, and paste reach the daemon through its existing input messages.
Reconnects attach to the remembered session and replay focus and Agent transcript state.
Agent replay includes submitted prompts when the provider emits `user_message_chunk` updates;
the daemon does not journal a separate copy of submitted prompt text. Browser image transfers
are assembled in a bounded cache and retired textures are released from GPUI.
Hovering a terminal `[Image #N]` marker fetches its preview from the daemon and shows it after
250 ms in the desktop-style popover. The browser limits pasted-image transfers and cached encoded
bytes to 64 MiB and 512 entries, drops them on reconnect or session attachment, and releases
retired image assets from GPUI. Missing previews do not trigger repeated fetches while cached.
Clipboard paste accepts PNG, JPEG, GIF, and WebP images up to 6 MiB through GPUI's browser paste
callback. The browser uploads their original bytes in ordered chunks; the daemon writes the image
on its host and pastes its path into the terminal. The browser does not resize or transcode clipboard
images. Popup terminals, command output, and search retain text paste behavior.
Page blur and local dialogs cancel the daemon prefix; ordinary keys wait for the cancellation
acknowledgement after the dialog closes or the page regains focus.

Terminal panes show the shared desktop header with the pane title, split controls, drag handle,
and close button. Inactive pane headers reveal their controls on hover. Drag the handle, or arm
the daemon prefix and drag any pane, to swap at another pane's center or join at an edge. The
browser draws the shared drop preview and predicts the resulting layout until the daemon sends
its next snapshot. Both clients use `zz-client` for drop targets and commands, and `zz-protocol`
for the layout transforms. Recursive split surfaces keep one divider between panes,
with the desktop active-pane edge highlight. Terminal dimming changes text and header
content without dimming the pane background. Pointer movement selects inactive panes when the daemon's
`focus-follows-mouse` option is on; dragging leaves focus unchanged.

When the fixed sidebar is hidden, a sidebar focus request opens it over the workspace with a
scrim. Escape, confirmation, or clicking the scrim dismisses it and returns focus to the workspace.
Opening this panel leaves the saved sidebar preference unchanged.

Session and window selection, terminal splits and divider dragging, pane zoom, daemon command
prompts, choosers, menus, confirmations, popups, command output, and Agent prompt/permission
controls use live daemon state. Agent panes use a combined project/history picker with project scope,
keyboard selection, pagination, and deletion when the provider supports it. Model, mode, and effort
menus use the provider's current options. Composer actions, context usage, and Git counts share
the desktop presentation. Reclaimed drafts and PNG, JPEG, and WebP attachments are supported.
The working-directory picker lists known Agent directories and accepts an absolute path on the
daemon host; recursive filesystem discovery remains desktop-only. Appearance preferences for the
browser live in local storage: system/light/dark mode, a desktop chrome preset per mode, the
three editable palette roots, zoom, corner radius, animations, and shadow strength.
The animation switch respects the platform’s reduced-motion preference. Terminal color schemes
follow the effective light/dark mode after attachment and when that mode changes. Numeric fields,
color pickers, theme previews, and pane-picker rows use the desktop components. Panes settings use the shared Layout, Focus, and
Frame groups for gaps, inactive opacity, margins, pane radius, and border width. These settings are
saved independently from widget radius. Sidebar rows share desktop labels, markers, layout and
rename menus, add/split/close actions, and keyboard navigation. Drag the sidebar’s right edge to
resize it; the browser saves the width and clamps it to 160–640 logical pixels and half the viewport,
with a 160-pixel minimum. The window strip shares tabs, overflow, rename/close menus, session
selection, and agent activity. Status bar settings control
the session menu, window bell/activity badges, and agent activity, with the shared live preview.
The browser omits host and update items. It bundles Inter for chrome and Lilex for terminals, code, and command output; daemon font stacks retain families available on the client and fall back to Lilex.
Terminal settings can override the local family and text scale, with a shared renderer preview.
UI font selection uses the client’s available fonts. It ships no
CJK or emoji fallback, so unsupported glyphs render as missing glyphs.

The command palette shares desktop input, result rows, pills, and keyboard hints.
Command-P/Command-K (Control-Shift-P/Control-Shift-K on non-Apple desktops) opens
Workspace search. Prefixes `:`, `@`, `%`, and `~` select commands, windows, panes,
and the connected host. The window chooser uses the same compact navigation tree. Both clients
use `zz-client` completion for commands, options, recent commands, and live session/window/pane
targets. Menus and confirmations share floating frames and rows, while pane indicators and
terminal search use the desktop overlays. Popup terminals also use the desktop cell geometry,
style colors, border rules, title, and content insets. Dead, waiting, and synchronized panes share
the desktop badges, and terminal panes stack them with search and mode tags. Command output
replaces its pane's content. Display-panes labels render tmux styles and alignment through
`zz_ui::tmux_style`, which desktop uses too. Agent permission, error, and empty-state cards share
their presentation; provider switching, retry, image replay, and jump-to-bottom are available.
Agent slash-command suggestions share desktop rows and `zz-client` matching and replacement
rules, including argument hints. The browser retains the provider command catalog independently
of transcript limits so older replay entries can be discarded without losing completion.

Settings and the new-pane picker omit unsupported controls. The Agent creation preference
also controls palette suggestions; existing Agent panes keep rendering. Native window behavior, OS fonts and credentials,
native file dialogs, desktop notifications, local file editing, Chromium execution inside a pane, and
direct SSH host setup require capabilities outside the browser client. Browser panes retain their
toolbar and offer an external tab for their URL. The browser client does not enable the shared
code editor. File paths in `save-buffer`, `load-buffer`, and `source-file` resolve on the daemon host;
the daemon only asks command-line clients for their files. Browser
screenshot commands return an error because the page has no Chromium pane runtime. Daemon
`OpenUri` requests open HTTP, HTTPS, and mailto links; other schemes show a client error toast.

# Checks

The browser has its own Cargo workspace and lockfile. Keep both GPUI
patch revisions aligned with the root workspace.

```sh
cargo test -p zz-web --locked
cargo clippy -p zz-web --all-targets --locked -- -D warnings
cargo test --manifest-path clients/web/Cargo.toml --lib --locked
rustup run nightly cargo check --manifest-path clients/web/Cargo.toml --target wasm32-unknown-unknown --all-targets --locked
just web-build
```

Gateway integration tests start a real daemon, attach over WebSocket, resize and type into a
terminal, execute commands, and check that disconnect closes the daemon attachment. Other tests
reject foreign origins, invalid frames, and asset paths outside the served directory.

# Source

- `clients/web/src/lib.rs`: GPUI initialization, portable fonts, and browser entrypoint.
- `clients/gpui-shared/src/connection.rs`: protocol reduction, bounded receive queue, reconnect, and replay.
- `clients/gpui-shared/src/terminal.rs`: browser terminal input, selection, search, and shared-renderer adapter.
- `clients/gpui-shared/src/terminal_images.rs`: bounded Kitty and pasted-image transfer assembly and image retirement.
- `crates/zz-ui/src/terminal.rs`: shared terminal painting, metrics, scrollbars, cursor, and image placement.
- `clients/gpui-shared/src/app.rs`: workspace, navigation, and daemon overlays.
- `clients/gpui-shared/src/command_palette.rs`: the web/iOS `PaletteBackend` for zz-ui's shared `CommandPaletteView`.
- `clients/gpui-shared/src/floating.rs`: daemon menu and confirmation interaction over shared floating surfaces.
- `crates/zz-ui/src/command/`: shared palette, menu, confirmation, and frame presentation.
- `crates/zz-client/src/completion.rs`: shared command, option, history, and live-target completion.
- `clients/gpui-shared/src/agent_pane.rs`: Agent timeline, composer, and conversation history.
- `clients/gpui-shared/src/settings.rs`: supported settings pages and shared previews.
- `clients/gpui-shared/src/preferences.rs`: local preference persistence and migration.
- `clients/gpui-shared/src/sidebar.rs`: session tree, keyboard focus, disclosure controls, and row actions.
- `clients/gpui-shared/src/status_bar.rs`: live status model and command callbacks for the shared strip.
- `crates/zz-ui/src/navigation/status.rs`: shared window tabs, overflow, session chip, and status items.
- `crates/zz-ui/src/navigation/sidebar.rs`: shared tree markers, menus, action strips, and keyboard navigation.
- `crates/zz-client/src/navigation.rs`: shared tree labels, layout order, and rename command construction.
- `crates/zz-client/src/agent_transcript.rs`: shared streaming transcript, plan, permission, and tool reduction.
- `crates/zz-client/src/pane_separator.rs`: active pane separator spans for desktop and thin clients.
- `crates/zz-client/src/agent_completion.rs`: shared provider command matching, query replacement, and hints.
- `crates/zz-ui/src/agent/slash.rs`: shared Agent command suggestion rows and list.
- `clients/gpui-shared/src/picker.rs`: pane-picker interaction over shared desktop rows.
- `crates/zz-ui/src/chrome_palette.rs`: shared presets and palette resolution.
- `crates/zz-ui/src/picker.rs`: shared history and path picker layout and rows.
- `crates/zz-ui/src/agent/controls.rs`: shared composer actions, option menus, Git counts, and context usage.
- `crates/zz-ui/src/agent/presentation.rs`: shared permission, error, and empty-state cards.
- `clients/gpui-shared/src/attachments.rs`: browser image selection and protocol limits.
- `crates/zz-web/src/lib.rs`: `Gateway`, asset serving, and the binary WebSocket bridge.
- `scripts/build-web-wasm.sh`: build and static distribution assembly.

See [client core and contract](/designs/client-core-and-contract.md) and
[running zz](/playbooks/running-zz.md) for the shared client rules and daemon lifecycle.
