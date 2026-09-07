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
timestamp: 2026-09-07T19:55:35Z
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

Terminal panes receive full frames and patches through `ClientCore`. The painter caches shaped
rows and resolves the shared style and grapheme dictionaries. Keyboard input, text composition,
selection, scrolling, search, and paste reach the daemon through its existing input messages.
Reconnects attach to the remembered session and replay focus and Agent transcript state.
Agent replay includes submitted prompts when the provider emits `user_message_chunk` updates;
the daemon does not journal a separate copy of submitted prompt text. The terminal painter
currently renders text and selection overlays, without Kitty image graphics.

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
pane-picker rows use the desktop components. Sidebar rows expose expand/collapse, add, split,
and close controls. The bundled fonts include CJK and color emoji fallback.

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
- `clients/web/src/terminal.rs`: terminal painter, input, selection, and search.
- `clients/web/src/app.rs`: workspace, navigation, and daemon overlays.
- `clients/web/src/agent_pane.rs`: Agent timeline, composer, and conversation history.
- `clients/web/src/settings.rs`: local preferences and shared appearance controls.
- `clients/web/src/sidebar.rs`: session tree, disclosure controls, and row actions.
- `clients/web/src/picker.rs`: pane-picker interaction over shared desktop rows.
- `crates/zz-ui/src/chrome_palette.rs`: shared presets and palette resolution.
- `crates/zz-ui/src/picker.rs`: shared history and path picker layout and rows.
- `crates/zz-ui/src/agent/controls.rs`: shared composer actions, option menus, Git counts, and context usage.
- `clients/web/src/attachments.rs`: browser image selection and protocol limits.
- `crates/zz-web/src/lib.rs`: `Gateway`, asset serving, and the binary WebSocket bridge.
- `scripts/build-web-wasm.sh`: build and static distribution assembly.

See [client core and contract](/designs/client-core-and-contract.md) and
[running zz](/playbooks/running-zz.md) for the shared client rules and daemon lifecycle.
