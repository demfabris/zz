# zz native macOS client and components

`ZZUI` is the SwiftUI/AppKit presentation library for a native zz client.
`ZZComponentGallery` opens a desktop workspace preview with the same sidebar,
pane layout, browser chrome, and agent composer as zz. The appearance menu opens
the 13-page component catalog; Option-Command-K opens it directly.
`ZZNative` connects to a running daemon through `zz-client-ffi`. It supports
styled terminal output, typing and paste, selection and copy, resizing, splits,
window and session navigation, and reconnect. Browser and agent panes still
need their native adapters.

The preview starts with the supplied screenshot's macOS Classic palette,
24-point corners, and white foreground. Its appearance menu also offers the
stock preset and light mode. All interactions use local fixtures.

From the repository root:

```sh
just macos-native
just macos-native --socket /tmp/zz.sock --session work
just macos-native-build release
just macos-native-test
just macos-gallery
just macos-gallery-build release
just macos-gallery-test
```

Requires macOS, Xcode, and Swift 6. Deployment target: macOS 14.
Open `Package.swift` in Xcode, or run the app from
`clients/macos/dist/zz Native.app` or
`clients/macos/dist/zz Native Gallery.app` after building.
The gallery build needs no Rust build. Tests build the Rust interface and run
a private daemon fixture. The native app uses the existing default daemon
socket, including `ZZ_SOCKET`; it does not start or replace an installed daemon.

See the [native client guide](../../knowledge/playbooks/native-macos-client.md)
for the connection lifecycle, renderer, current scope, and test coverage.

See the [build guide and component inventory](../../knowledge/playbooks/native-macos-gallery.md)
for source mappings, native substitutions, and the boundary with Rust.
