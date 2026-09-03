---
name: ios-client
description: Develop, build, test, run, and diagnose zz's existing native iPhone and iPad client under clients/ios, including SwiftUI/UIKit behavior, zz-client-ffi integration, simulators, signed physical-device installs, and SSH reconnect failures. Use for requests mentioning the zz iOS, iPhone, iPad, or iPadOS app or the just ios, ipad, or ios-device workflows. For shared client-core or wire-contract work with no Apple-client behavior, use new-client instead.
---

# Native iPhone and iPad client

`clients/ios` contains one universal app. Horizontal size class selects the compact phone shell or
the regular iPad shell. Both use the same `ZZStore`, FFI client, retained terminal frames, settings,
and daemon connection. The deleted `crates/zz-ios` and `crates/zz-gpui-ios` targets are history, not
an alternate implementation path.

## Load the right context

Read `knowledge/designs/ios-client.md` before changing behavior. It records the current product and
architecture decisions. Verify any fact that affects code against the cited source.

- Read [interface contracts](references/interface-contracts.md) for navigation, Panorama, terminal
  input, geometry, Agent panes, settings, accessibility, or visual QA.
- Read [device workflows](references/device-workflows.md) for XcodeGen, simulator recipes, signed
  device builds, Rust archive reuse, launch verification, or test selection.
- Read [connection diagnostics](references/connection-diagnostics.md) for unreachable hosts, SSH,
  trust or authentication, remote daemon startup, `zz proxy`, protocol mismatches, or reconnects.
- For changes to `zz-client`, `zz-client-ffi`, shared key handling, or wire behavior, also read
  `../new-client/SKILL.md` and its pitfalls reference.
- For SwiftUI state, layout, animation, accessibility, or Instruments work, also read
  `../swiftui-expert-skill/SKILL.md` and only the references needed for that task.

## Ownership boundaries

| Layer | Owner |
| --- | --- |
| App shell, navigation, native controls | SwiftUI in `clients/ios/Sources` |
| Terminal drawing, touch, keyboard, first responder | UIKit in `TerminalSurface.swift` |
| Published presentation and connection state | Main-actor `ZZStore` |
| Snapshots, viewport retention, split geometry, Agent reduction | `zz-client` through `zz-client-ffi` |
| Sessions, windows, panes, commands, PTYs, key tables | daemon and mux |
| Local and SSH transport | `zz-daemon::InteractiveClient` behind the FFI |

Swift must render reduced state and forward intent. Keep mux command parsing, split solving, terminal
patching, Agent reduction, and key-table semantics below the FFI boundary.

`ChromeProfile::DesktopApple` belongs to the macOS GPUI client. It does not name the native Apple
app, and ChromeKeymap actions do not reach iOS through the current FFI. ChromeKeymap-only work with
no Apple-client behavior belongs to `new-client` and needs no iOS simulator or device gate.

## Contracts that survive feature work

- Preserve the one-app adaptive design. Branch on size class, not device model.
- Keep zero or one terminal input owner even when an iPad window mounts several live terminal tiles.
- Treat pane focus and client-window focus as separate signals.
- Treat `zz_client_attach == true` as request submission. Wait for `ZZ_EVENT_ATTACHED` before sending
  client-window focus or claiming attachment succeeded.
- Release every acquired snapshot, viewport, and Agent state. `TerminalFrame` may retain its viewport
  handle until `deinit`; raw FFI buffers must not outlive that handle.
- Use a fresh FFI client and `ClientCore` after disconnect. Swift may retain immutable frames and
  desired navigation while it reconnects.
- Render an authoritative zero-session snapshot and recover from it. Session `0` exists at fresh
  daemon boot, not for the daemon's whole lifetime.
- Keep terminal colors, cursor appearance, cell semantics, selection, and clipboard extraction owned
  by the viewport/core. Do not add a Swift VT parser or reconstruct selection text from drawn cells.
- Keep Browser and Editor panes explicit placeholders until the native viewport contract exists.
  Agent panes render the daemon's journal transcript through `zz_client_agent_updates_next`; reduce
  it with the published cursor rules instead of inventing client-side history.
- Keep the current one-host mobile model unless the user asks for fleet aggregation. Saved host
  selection and desktop multi-host presentation are different product surfaces.

## Editing and validation

Edit `clients/ios/project.yml`, source files, or tracked support files. XcodeGen owns the ignored
`clients/ios/ZZMobile.xcodeproj`; do not edit or add it. Regenerate and retain a changed tracked
`Support/Info.plist` when project metadata changes.

Run the closest checks first:

```sh
env -u ZZ_IOS_REUSE_CLIENT_CORE just ios-test
env -u ZZ_IOS_REUSE_CLIENT_CORE just ipad-test
cargo test -p zz-client
cargo test -p zz-client-ffi
```

Choose the checks that cover the changed layer. SSH/probe changes also need the focused
`zz-daemon` tests. Layout, keyboard, safe-area, focus, terminal drawing, and Panorama changes need a
simulator or physical-device interaction pass because the Swift suite currently tests policy rather
than rendered UI behavior.

If source and `knowledge/designs/ios-client.md` disagree after the change, update the design and its
managed index in the same work. Client-only changes do not require a protocol-version bump.
