# GPUI iOS terminal

This experiment connects a GPUI terminal to a running zz daemon. The daemon owns
the PTY and Ghostty parser/encoder; the app uses `InteractiveClient`, `ClientCore`,
and the shared `zz-ui` terminal painter and Kitty image cache.

## Run

Use an Apple Silicon Mac with Xcode, Rust's `aarch64-apple-ios-sim` target, and
Zig 0.16.0. From the repository root:

```sh
just ios-gpui
just ios-gpui iPhone
just ios-gpui iPad build
```

The launcher finds an existing zz Dev socket. It does not start a daemon. To
attach to a specific socket and session:

```sh
ZZ_GPUI_ENDPOINT=/tmp/my-zz.sock ZZ_GPUI_SESSION=work just ios-gpui
```

`ZZ_DEV_SOCKET` also selects a local socket. Set `ZZ_GPUI_SIMULATOR` to a simulator
UDID to choose an exact device. The bundle ID is `dev.zz.gpui-poc`, its display name
is **zz GPUI**, and build products live in `target/ios-gpui/ZZ GPUI.app`.

Without a saved endpoint, the app opens a connection field. **Host** opens that
field again. Enter a socket path on Simulator or `ssh://user@host` for a remote
daemon. The transport exposes host-key trust and authentication prompts in the
app. The remote host needs a compatible `zz` executable. Only the endpoint is
saved by this example.

## Terminal behavior

- Hardware typing, Ctrl, Alt/Option, Shift, Command, Escape, Tab, arrows,
  Home/End, Insert/Delete, Page Up/Down, and F1 through F24.
- Held-key repeat and release events, including the Kitty keyboard protocol.
  Ghostty chooses escape sequences from the terminal's current modes.
- ANSI styles, true color, alternate screen applications, cursor state, terminal
  replies, and Kitty images use the existing daemon and shared renderer.
- Command-C/V/A copy, paste, and select all. The toolbar also exposes Copy and
  Paste. Pasted text uses the daemon's bracketed-paste handling.
- Drag to scroll; enable **Select** to drag a selection. **End** returns to the
  live screen. Shift-Page Up/Down and Shift-Home/End navigate scrollback.
- Touch events become terminal mouse events when the application requests mouse
  tracking. Ctrl/Command-click can activate links. Resizing updates the PTY grid.

This is one active terminal pane. Full mux overlays and the agent/browser UI are
not implemented. The software keyboard stays hidden; composition through
`UITextInput`, IME, and mobile editing controls still need work.

## Verification

Simulator checks exercised raw PTY bytes for normal/application arrows,
modifiers, Tab, function/navigation keys, repeats, Kitty releases, and bracketed
paste. Vim opened, edited, and saved a file through hardware key events. A live
shell also accepted hardware input on iPhone. An ANSI fixture and a Kitty image
rendered on iPad. Automated checks cover native key
translation, terminal bindings, and the shared image cache; the web client also
builds after adopting that cache.

Copy/paste of text copied within the app was verified. Text injected through
`simctl pbcopy` was not readable from UIKit in this simulator session; cross-app
paste still needs a device check. Physical keyboard layouts, dead keys, SSH
authentication, rotation, suspension/reconnect, and GPU recovery have not been
verified on hardware. The arm64 device binary builds, but the launcher currently
packages simulator apps only.

```sh
cargo test --locked -p zz-gpui-ios --test keyboard
cargo test --locked -p zz-ui terminal_images::tests
cargo fmt -p zz-gpui-ios -p zz-ui --check
IPHONEOS_DEPLOYMENT_TARGET=26.0 cargo clippy --locked -p zz-gpui-ios --all-targets --target aarch64-apple-ios-sim -- -D warnings
IPHONEOS_DEPLOYMENT_TARGET=26.0 cargo build --locked -p zz-gpui-ios --example terminal --target aarch64-apple-ios
just web-build
```

## Recovered history

- `015b3a39` (2026-08-07): original UIKit backend and GPUI card demo.
- `22c6bb5d` (2026-08-08): separate GPUI iOS client.
- `4c23f8d6` (2026-08-15): replacement with the Swift client. Its parent contains
  the last full GPUI iOS implementation.

`crates/zz-gpui-ios` restores the small window/display adapter and scene startup
from that work. It uses the pinned GPUI fork's wgpu renderer and CosmicText font
system, with the web client's Lilex fonts. It explicitly selects Metal because
GPUI's native wgpu convenience constructor defaults to Vulkan and GL.
