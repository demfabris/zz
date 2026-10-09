# zz-app

The thin-client app the web and iOS clients share: one shell, sidebar, status bar,
settings, command palette, overlays, terminal pane, agent pane, pane picker,
protocol reducer, and image cache. `zz-ui` owns their widgets and painting;
`zz-client` owns protocol state.

`connection.rs` uses WebSocket on WASM and `transport.rs` (the daemon client) on
iOS. `input.rs` translates native keystrokes; `ios_browser.rs` is the WKWebView
browser pane. Native keyboard, paste, and menus call `zpui_ios`, the UIKit backend
in `zpui/crates/zpui_ios`. Each client (`clients/web`, `clients/ios`) owns its
entry point, fonts, window lifecycle, and platform backend.

Run `cargo test -p zz-app` for shared logic, `just web build` for WASM, and
`just ios build iPad` for iOS.
