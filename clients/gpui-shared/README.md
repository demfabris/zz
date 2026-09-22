# Shared GPUI client source

The web and iOS binaries include these Rust modules with `#[path]`. They keep
one terminal pane, agent pane, pane picker, protocol reducer, and image cache.
`zz-ui` owns their widgets and painting; `zz-client` owns protocol state.

`connection.rs` uses WebSocket on WASM and the iOS transport on UIKit. Native
keyboard translation and paste call the iOS backend; browser behavior stays in
the WASM build. Each application owns its entry point, fonts, window lifecycle,
and platform backend; the shell, navigation, settings, and preferences are shared.

Run `cargo test --manifest-path clients/web/Cargo.toml --lib` for shared logic
checks, `just web-build` for WASM, and `just ios-gpui iPad build` for iOS.
