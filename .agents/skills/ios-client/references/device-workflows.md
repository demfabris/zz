# Build, simulator, and device workflows

Use this reference to choose the correct recipe, avoid stale Rust archives, and distinguish build,
install, launch, connection, and visual verification.

## Generated project

XcodeGen reads `clients/ios/project.yml` and regenerates `clients/ios/ZZMobile.xcodeproj`. The project
directory is ignored and must remain untracked. `clients/ios/Support/Info.plist` is generated but
tracked, so retain its diff when project metadata changes.

The Xcode pre-build phase cross-compiles `zz-client-ffi` and links the static archive through
`ZZ-Bridging-Header.h`.

## Recipe matrix

| Goal | Command |
| --- | --- |
| Run an iPhone simulator | `just ios` |
| Build for an iPhone simulator | `just ios-build` |
| Test on an iPhone simulator | `just ios-test` |
| Run an iPad simulator | `just ipad` |
| Build for an iPad simulator | `just ipad-build` |
| Test on an iPad simulator | `just ipad-test` |
| Build, sign, install, and launch on hardware | `just ios-device <device-id>` |
| Use a Release device core | `ZZ_IOS_CONFIGURATION=Release just ios-device <device-id>` |

`scripts/ios-sim.sh`, `scripts/ios-device.sh`, and `scripts/build-ios-client-core.sh` implement these
recipes. Read them before changing recipe behavior.

Use the CoreDevice identifier from `xcrun devicectl list devices` with `just ios-device`. The current
Just recipe does not preserve whitespace in a device name; a single-word name also works.

## Rust archive reuse

Simulator and device archives differ:

```text
target/ios-client-core/<identity>/aarch64-apple-ios-sim/<profile>/libzz_client_ffi.a
target/ios-client-core/<identity>/aarch64-apple-ios/<profile>/libzz_client_ffi.a
```

Identity `1` is development; `0` is production. Debug and Release archives also differ. A simulator build cannot refresh the physical-device
archive.

`ZZ_IOS_REUSE_CLIENT_CORE=1` skips Cargo and copies the archive that already exists for the selected
identity, platform, and profile. Use it only after Swift-only edits and only after a fresh matching archive.

Use an authoritative fresh build after changes to Rust, FFI, transport, protocol, build scripts, or
the selected profile:

```sh
env -u ZZ_IOS_REUSE_CLIENT_CORE just ios-test
env -u ZZ_IOS_REUSE_CLIENT_CORE just ios-device <device-id>
```

The device command builds, signs, installs, and launches the replacement.
For a diagnose-only request, get explicit approval before running it. A request to build, run, or
install on the named device already places that action in scope.

After that device build, a Swift-only iteration may use:

```sh
ZZ_IOS_REUSE_CLIENT_CORE=1 just ios-device <device-id>
```

A stale device archive can authenticate over SSH and then fail during the remote probe or proxy
startup. Rebuilding a simulator does not rule it out.

## Simulator behavior

`scripts/ios-sim.sh` starts or selects a simulator, builds the same universal app, installs bundle
`dev.zz.ios.dev`, and launches it with `SIMCTL_CHILD_ZZ_SOCKET`. `ZZStore.start` sees `ZZ_SOCKET` and
bypasses host setup and SSH. The recipe selects the desktop dev socket, ignores inherited
`ZZ_SOCKET`, and accepts `ZZ_DEV_SOCKET` as an explicit override. Without an override it prefers the
environment-derived socket, then checks per-user dev defaults under `getconf DARWIN_USER_TEMP_DIR`
and `/tmp`. This handles desktop and terminal launches with different `TMPDIR` values. The selected
socket is printed before the build; a missing socket stops the run before building or replacing the app.

The recipe selects the newest installed iOS runtime with a device in the requested family,
preferring a booted device within that runtime. It waits for boot completion before installing.
Xcode 27 uses `Contents/Applications/DeviceHub.app` instead of Simulator; the script opens Device
Hub from the selected Xcode installation and uses Simulator on older Xcode versions.

Socket existence does not establish attachment. Verify a real session, window, pane, and terminal
frame after launch. Start the desktop dev daemon with `just run mac` when no dev socket exists.

## Physical-device behavior

A physical device cannot use the Mac's Unix socket. It stores a normalized `ssh://user@host`
endpoint, reaches SSH over the LAN, and uses its Keychain-backed identity. See
[connection diagnostics](connection-diagnostics.md) for the transport and evidence ladder.

Local recipes install the separate `zz Dev` app. Its SSH transport uses the host’s `zz-dev`
executable and socket namespace; desktop dev runs create the executable link in `~/.local/bin`.
`just ios-preview` retains the production identity.

When probing a macOS dev host without `XDG_RUNTIME_DIR`, SSH prefers an existing
`/tmp/zz-dev-$USER/default.sock` before the login shell's temporary-directory default. This keeps
desktop launches using `/private/tmp` and SSH shells using Darwin's user temp directory on the same
dev socket. Explicit socket paths in SSH URLs still win. Production and Linux keep their existing
temporary-directory selection.

Physical Debug builds are the default. The recipe requires a paired device with Developer Mode,
valid signing, and an unlocked screen for launch.

Treat these as separate checkpoints:

1. Xcode build succeeded.
2. The app installed.
3. The app launched.
4. SSH authenticated and the proxy stayed connected.
5. The UI rendered a real daemon session and live pane content.

Do not call step 2 or 3 a connected client.

If installation fails after `BUILD SUCCEEDED` with CoreDevice error 4000 and connection reset by
peer, check device details and lock state first. On 2026-09-14 the paired iPad's local-network
connection recovered and retrying `devicectl device install app` with the completed `.app` succeeded;
another Rust build was unnecessary. Launch afterward and verify app state separately. A fresh
`zz Dev` installation can open at host setup because its saved endpoint is separate from `zz`.

## CoreDevice checks

Use read-only commands first:

```sh
xcrun devicectl list devices
xcrun devicectl device info details --device <device-id>
xcrun devicectl device info lockState --device <device-id>
xcrun devicectl device info apps --device <device-id> --bundle-id dev.zz.ios.dev
xcrun devicectl device info processes --device <device-id> --search ZZ
```

Launch with console output when the app exits or stalls before showing useful UI. This terminates the
existing app first, so a diagnose-only request needs explicit approval:

```sh
xcrun devicectl device process launch \
  --device <device-id> \
  --terminate-existing \
  --console \
  dev.zz.ios.dev
```

The console command remains attached until the app exits. A forgotten attachment can interfere with
a later reinstall. Find the exact `devicectl` process and stop only that process.

Capture runtime proof. The command writes a local `/tmp` file; skip it under a no-write request unless
the user approves that artifact:

```sh
xcrun devicectl device capture screenshot \
  --device <device-id> \
  --destination /tmp/zz-device.png
```

`FBSOpenApplicationErrorDomain` error 7 means the device was locked. The install may still have
succeeded. Unlock it, launch again, and verify the UI before reporting runtime success.

## Validation by changed layer

| Changed layer | Closest checks |
| --- | --- |
| Swift models or policy | `env -u ZZ_IOS_REUSE_CLIENT_CORE just ios-test` |
| iPad-specific interface | `env -u ZZ_IOS_REUSE_CLIENT_CORE just ipad-test` plus visual interaction |
| `zz-client` state or geometry | `cargo test -p zz-client` plus the affected simulator suite |
| FFI surface or lifecycle | `cargo test -p zz-client-ffi` plus the affected simulator suite |
| SSH prompt, probe, daemon start, or proxy | focused `zz-daemon` tests, FFI tests, then a fresh physical build |
| Keyboard, focus, safe area, rendering, Panorama | simulator and physical-device visual interaction |

`ZZMobileTests` covers Swift policies. `ZZMobileUITests` also runs
`Tests/UI/IPadAcceptanceTests.swift` against an isolated daemon. The UI test opens the simplified settings form,
chooses a bundled font and theme, materializes a picker, checks pane resizing, opens copy/search
controls, and inspects published key tables. It captures screenshots and the accessibility tree.
It skips on iPhone and when `ZZ_IOS_UI_TEST_SOCKET` is absent.

Start a dedicated daemon with an initial terminal pane on a short `/tmp` socket. Boot the selected
iPad simulator, then put the socket variable in the simulator's launch environment so the test
runner can read it. The test forwards that value to the app as `ZZ_SOCKET`:

```sh
xcrun simctl spawn <iPad-UDID> launchctl setenv ZZ_IOS_UI_TEST_SOCKET /tmp/zz-ipadqa.sock
xcodegen generate --spec clients/ios/project.yml --project clients/ios
env -u ZZ_IOS_REUSE_CLIENT_CORE xcodebuild \
  -project clients/ios/ZZMobile.xcodeproj \
  -scheme ZZMobile \
  -destination 'platform=iOS Simulator,id=<iPad-UDID>' \
  -derivedDataPath target/ios-sim-dev \
  -only-testing:ZZMobileUITests \
  -resultBundlePath /tmp/zz-ipad-ui-result.xcresult \
  ZZ_DEV_BUILD=1 ZZ_APP_BUNDLE_ID=dev.zz.ios.dev \
  ZZ_APP_DISPLAY_NAME="zz Dev" ZZ_APP_URL_SCHEME=zz-dev \
  CODE_SIGNING_ALLOWED=NO test
xcrun simctl spawn <iPad-UDID> launchctl unsetenv ZZ_IOS_UI_TEST_SOCKET
```

Choose a new result-bundle path for each run. Clear the simulator variable after the run, including a
failed run, and stop only the dedicated test daemon. Archive reuse follows the fresh-archive rule
above. A green unit suite does not prove rendered geometry, keyboard behavior, or focus transfer;
the UI suite proves only its asserted flows and does not replace physical-device checks.

The zz daemon outlives desktop app replacement. When a protocol or daemon change appears missing,
identify the running daemon binary and version before restarting it. Ask before disrupting a live
daemon or its attached clients.
