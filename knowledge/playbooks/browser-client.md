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
timestamp: 2026-09-07T22:26:00Z
---

# Overview

`clients/web` compiles the shared GPUI components from `zz-ui` and the state reducer from
`zz-client` to WebAssembly. `zz-web` serves the page and forwards the existing binary protocol
between each browser WebSocket and one daemon connection. The daemon owns sessions, PTYs,
key bindings, copy mode, and Agent processes.

# Run

Start a zz daemon with the installed app or an existing zz command, then run from the repo root:

```sh
just showcase-setup
just web-build
just web-serve
```

Open `http://127.0.0.1:8080`. `just web` builds the assets, starts the gateway, and watches
the client and shared crates for changes. Refresh the page after a rebuild.
`just web-build-release` produces optimized assets under `clients/web/dist`.

The gateway connects to the existing default daemon socket. `ZZ_SOCKET` or an explicit
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

Session and window selection, terminal splits and divider dragging, pane zoom, daemon command
prompts, choosers, menus, confirmations, popups, command output, and Agent prompt/permission
controls use live daemon state. Agent panes use the shared history picker with project scope,
keyboard selection, pagination, and deletion when the provider supports it. Model, mode, and effort
menus use the provider's current options. Composer actions, context usage, and Git counts share
the desktop presentation. Reclaimed drafts and PNG, JPEG, and WebP attachments are supported.
The working-directory picker lists known Agent directories and accepts an absolute path on the
daemon host; recursive filesystem discovery remains desktop-only. Appearance preferences for the
browser live in local storage: system/light/dark mode, all eleven desktop chrome presets, six
editable palette roots, zoom, and corner radius. Numeric fields, color pickers, theme previews, and
pane-picker rows use the desktop components. Panes settings use the shared Layout, Focus, and
Frame groups for gaps, inactive opacity, margins, pane radius, and border width. These settings are
saved independently from widget radius. Sidebar rows share desktop labels, markers, layout and
rename menus, add/split/close actions, and keyboard navigation. The window strip shares tabs,
overflow, rename/close menus, session selection, agent count, and clock presentation. The bundled
fonts include CJK and color emoji fallback.

The command palette shares its input, result rows, and keyboard hints with desktop. Both clients
use `zz-client` completion for commands, options, recent commands, and live session/window/pane
targets. Menus and confirmations share floating frames and rows, while pane indicators and
terminal search use the desktop overlays. Popup terminals also use the desktop cell geometry,
style colors, border rules, title, and content insets. Dead, waiting, and synchronized panes share
the desktop badges. Agent permission, error, and empty-state cards share
their presentation; provider switching, retry, image replay, and jump-to-bottom are available.
Agent slash-command suggestions share desktop rows and `zz-client` matching and replacement
rules, including argument hints. The browser retains the provider command catalog independently
of transcript limits so older replay entries can be discarded without losing completion.

Unsupported controls stay visible and disabled. Native window behavior, OS fonts and credentials,
native file dialogs, desktop notifications, local file editing, Chromium execution inside a pane, and
direct SSH host setup require capabilities outside the browser client. Browser panes retain their
toolbar and offer an external tab for their URL. The WASM editor has no tree-sitter highlighting.

# Checks

The browser has its own Cargo workspace and lockfile, like the UI showcase. Keep both GPUI
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
- `clients/web/src/connection.rs`: protocol reduction, bounded receive queue, reconnect, and replay.
- `clients/web/src/terminal.rs`: browser terminal input, selection, search, and shared-renderer adapter.
- `clients/web/src/terminal_images.rs`: bounded image transfer assembly and GPU texture retirement.
- `crates/zz-ui/src/terminal.rs`: shared terminal painting, metrics, scrollbars, cursor, and image placement.
- `clients/web/src/app.rs`: workspace, navigation, and daemon overlays.
- `clients/web/src/command_palette.rs`: command prompt input and completion interaction over the shared palette.
- `clients/web/src/floating.rs`: daemon menu and confirmation interaction over shared floating surfaces.
- `crates/zz-ui/src/command/`: shared palette, menu, confirmation, and frame presentation.
- `crates/zz-client/src/completion.rs`: shared command, option, history, and live-target completion.
- `clients/web/src/agent_pane.rs`: Agent timeline, composer, and conversation history.
- `clients/web/src/settings.rs`: local preferences and shared appearance controls.
- `clients/web/src/sidebar.rs`: session tree, keyboard focus, disclosure controls, and row actions.
- `clients/web/src/status_bar.rs`: live status model and command callbacks for the shared strip.
- `crates/zz-ui/src/navigation/status.rs`: shared window tabs, overflow, session chip, and status items.
- `crates/zz-ui/src/navigation/sidebar.rs`: shared tree markers, menus, action strips, and keyboard navigation.
- `crates/zz-client/src/navigation.rs`: shared tree labels, layout order, and rename command construction.
- `crates/zz-client/src/agent_completion.rs`: shared provider command matching, query replacement, and hints.
- `crates/zz-ui/src/agent/slash.rs`: shared Agent command suggestion rows and list.
- `clients/web/src/picker.rs`: pane-picker interaction over shared desktop rows.
- `crates/zz-ui/src/chrome_palette.rs`: shared presets and palette resolution.
- `crates/zz-ui/src/picker.rs`: shared history and path picker layout and rows.
- `crates/zz-ui/src/agent/controls.rs`: shared composer actions, option menus, Git counts, and context usage.
- `crates/zz-ui/src/agent/presentation.rs`: shared permission, error, and empty-state cards.
- `clients/web/src/attachments.rs`: browser image selection and protocol limits.
- `crates/zz-web/src/lib.rs`: `Gateway`, asset serving, and the binary WebSocket bridge.
- `scripts/build-web-wasm.sh`: build and static distribution assembly.

See [client core and contract](/designs/client-core-and-contract.md) and
[running zz](/playbooks/running-zz.md) for the shared client rules and daemon lifecycle.
