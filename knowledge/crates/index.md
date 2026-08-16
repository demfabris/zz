<!-- okf:listing:start (managed by okf.py index — edit prose outside this fence) -->
# Concepts

* [zz-browser crate](zz-browser.md) - Browser-neutral abstraction over CEF Alloy off-screen rendering. Owns CEF init, named private request contexts, page zoom, input, lifecycle, and frame mailboxes.
* [zz-chrome-import crate](zz-chrome-import.md) - Store-agnostic Google Chrome data import - profile discovery, cookie snapshot/decryption, and read-only history extraction - isolating the app's only sqlite/crypto/keychain dependencies.
* [zz-client-ffi crate](zz-client-ffi.md) - Unix C ABI over zz-client for native shells, with pollable events, mux snapshots, raw terminal input, and caller-owned styled terminal viewports.
* [zz-client crate](zz-client.md) - The renderer-free client core that reduces protocol messages into shared state and typed effects, plus the client-local chrome keymap used by desktop and TUI skins.
* [zz-daemon crate](zz-daemon.md) - The persistent local daemon. Sole authority for mux state, owner of PTY-backed terminal sessions and Agent-pane ACP adapter children, and the fan-out engine that streams coalesced terminal frames and agent transcripts to attached and short-lived clients over a socket or named pipe.
* [zz-mux crate . renderer-free mux state machine](zz-mux.md) - The pure, UI-agnostic multiplexer core that owns sessions/windows/panes/splits, resolves tmux-style targets, executes tmux-compatible commands, holds key tables, and parses .tmux.conf.
* [zz-protocol crate](zz-protocol.md) - The stable, versioned wire vocabulary (IDs, framing, control messages, packed terminal lanes, and mux snapshots) shared by every zz client and the daemon.
* [zz-terminal crate](zz-terminal.md) - The per-pane terminal engine that owns a PTY child and every libghostty-vt object on a worker thread and publishes immutable renderer-neutral frames.
* [zz-xtask crate](zz-xtask.md) - Workspace build task that assembles and validates desktop CEF bundles across Linux, macOS, and Windows.
* [zz crate (the GPUI client)](zz.md) - The long-lived GPUI desktop client. Reconciles recursive pane layouts and hosts stable terminal, Chromium browser, and native Agent pane entities.
<!-- okf:listing:end -->

Workspace members without a crate page here: `zz-ui` (maintained gpui-component fork), `zz-tui`
(`zz attach`). The native Swift iPhone client lives under `clients/ios`, outside the Cargo workspace.
The map in
[architecture/overview](/architecture/overview.md) uses the live `zz-*` names.
