---
type: Design Plan
title: iOS/iPadOS client v1 . the desktop client, compiled for iPad
description: iPad client v1. crates/zz gated for aarch64-apple-ios on zz-gpui-ios; simulator and just ios-device install exist. Remaining gaps are safe-area insets, UITextInput, and an interactive tunnel pass.
status: Complete (v1; device install exists)
tags:
- ios
- gpui
- fleet
- transport
- design-plan
timestamp: 2026-08-15T00:00:00Z
---

# Overview

> **Status: all four milestones shipped 2026-08-07, validated on the iPad
> Pro 11" simulator** — M0 feature-splits, M1 full-client render, M2
> keyboard/scroll/CADisplayLink/clipboard, M3 in-process russh tunnel
> (TOFU + generated ed25519 + `zz proxy` over an exec channel), M4
> lifecycle/appearance/key-strip and `cargo xtask ios-sim [--run]`.
> Device signing and install exist (`just ios-device`, `scripts/ios-device.sh`).
> Still owed: dynamic safe-area insets, the UITextInput stub for the spacebar
> cursor, and an interactive attach-through-tunnel pass. Keychain-backed key
> storage landed 2026-08-08 (see M3).

v1 goal: the zz experience on iPad — session-tree sidebar, live terminal panes,
hardware + soft keyboard, touch scrolling — attached to a real zz daemon. The
client is **`crates/zz` itself compiled for iOS**, running on the existing
`crates/zz-gpui-ios` platform backend (Metal renderer + UIKit window, already
rendering on the iPad Pro 11" simulator).

Non-goals for v1: browser panes (no CEF on iOS; WKWebView overlay is a separate
later project), Android, client-side VT, editor/agent panes, App Store
packaging. One attached host at a time, same as desktop.

# Settled decisions — do not relitigate

1. **Port `crates/zz`, don't fork views into a new app crate.** The terminal
   element (`crates/zz/src/terminal/element.rs`, ~3.9k lines) is pure gpui —
   no platform APIs in its paint path — and the sidebar tree renders from pure
   `zz-protocol` snapshot types. Copying them would create a permanent fork of
   the hottest-maintained rendering code. The blockers to compiling `zz` for
   iOS are dependency plumbing, not architecture.
2. **No gpui hard fork.** `zz-gpui-ios` stays an out-of-tree backend against
   the pinned demfabris/zed fork rev; upstream's platform split makes the
   Platform/PlatformWindow traits public API.
3. **Thin client, daemon-side VT.** The daemon streams parsed cells
   (`TerminalViewport` / `TerminalViewportPatch` on the packed lane,
   `HistoryChunk` on the control lane). The iOS client ships **no libghostty,
   no PTY** — this is the position remux had to fork Ghostty to reach.
4. **Simulator-first over the local unix socket.** The simulator shares the
   Mac's filesystem: `InteractiveClient::connect_endpoint(Local(path))` reaches
   the host daemon with zero new transport code. russh (in-process ssh → `zz
   proxy` on the remote, the Windows-port stdio-proxy shape) comes last, only
   needed for physical devices.
5. **Feature-split, not crate-split, for v1.** `zz-terminal` grows a `session`
   feature (default-on) gating the libghostty/PTY half; `zz-daemon` grows a
   `daemon` feature (default-on) gating the daemon half. The later `zz-client`
   extraction now supplies shared protocol reduction, while iOS still compiles
   the GPUI shell and its retained terminal path.
6. **React Native rejected** (byte-stream widget vs cell-patch protocol
   mismatch; see knowledge on the copy-mode single-writer redesign).

# Reuse map (verified 2026-08-07 against protocol v44; live wire is v55)

Clean on iOS as-is:

- `zz-protocol` — framing (`framing.rs`: u32 LE length, lane u8, flags u8,
  version u16, checked every frame), `ProtocolMessage` / `EventPayload` in
  `message.rs`, snapshot tree in `snapshot.rs`. Pure serde. iOS speaks whatever
  `PROTOCOL_VERSION` the linked `zz-protocol` crate defines.
- `zz-terminal` client half — `model.rs` (PackedCell/TerminalViewport/
  `apply_patch`), `input.rs` (KeyInput), `interaction.rs`
  (TerminalViewAction), `appearance.rs`, `paste.rs`, `word.rs`. Zero libghostty
  hits; `session.rs` + `shell_integration.rs` are the only consumers.
- `zz_daemon::InteractiveClient` (`crates/zz-daemon/src/client.rs`, ~420
  lines) — complete blocking protocol client, zero gpui. Handshake:
  ClientHello/ServerHello (version gate, not negotiation), `Attach{""}` →
  `Attached` + resync burst (`daemon.rs:4279-4366` is the authoritative
  post-attach sequence).
- `MuxClient` (`crates/zz/src/mux/client.rs`) — the GPUI shell around
  `zz_client::ClientCore`: reconnect ladder (1,1,2,4,8,16,30s; attached host
  retries forever), retained terminal hot path, history backfill, and
  `RequestFull` recovery. It is a gpui Entity, which fits this client.
- Key encoding — `terminal/view.rs:2232-2377` (`key_code`, `modifiers`,
  `key_input`) and `mux/prefix.rs` (zero crate:: imports) are free functions
  over `gpui::Keystroke` + zz-terminal types.
- All of `zz-ui` (no platform deps; tree-sitter gates already exclude only
  wasm), `workspace/tree.rs`, `mux/prefix.rs`, `ui_scale.rs`,
  `window/corners.rs`, `pane/mod.rs`.
- Sidebar core — `workspace/sidebar.rs` tree model + row renderer read
  `MuxSnapshot` via `MuxClient`; desktop-only bits (window drag handle,
  WindowCorners, add-host dialog shell) get iOS arms/gates.

Known hostile (the actual work):

- `libghostty-vt-sys` build.rs panics on iOS triples → never reach it
  (feature-split).
- `zz-browser` default features → `cef-dll-sys` build.rs `unimplemented!` on
  iOS → `default-features = false` on the iOS dep row (precedent:
  `zz-chrome-import/Cargo.toml:23`).
- `gpui_platform` has no iOS arm in `current_platform()` → not on iOS;
  `Application::with_platform(Rc::new(IosPlatform::new()))` instead.
- `diagnostics/mod.rs:29` imports `browser::controller::BrowserController` —
  the one module that drags CEF into everything (the terminal element calls
  `diagnostics::timer`). Decouple for all platforms, not just iOS.
- `terminal/view.rs:52-56` — `GPUI_UNITS_PER_FONT_POINT` takes the 96/72
  branch on iOS but zz-gpui-ios is CoreText → fonts 33% too large; needs
  `any(macos, ios)`. `TERMINAL_FONT` fallback must be `Menlo` on iOS.

# Milestones

## M0 — dependency plumbing (no crates/zz changes)

- `zz-terminal`: `session` feature (default-on) gating `session.rs`,
  `shell_integration.rs`, and the `libghostty-vt`/`portable-pty` (+ any
  session-only) deps as optionals. Client half compiles standalone.
- `zz-protocol`: depends on `zz-terminal` with `default-features = false`
  (it only uses model types).
- `zz-daemon`: `daemon` feature (default-on) gating `daemon.rs`, `status.rs`,
  and the `zz-mux` (+ any daemon-only) deps; `client.rs`/`transport.rs`/
  `endpoint.rs`/`askpass.rs` stay unconditional; the `zz-terminal/session`
  requirement moves under the `daemon` feature.
- Exit: `cargo check -p zz-daemon --no-default-features --target
  aarch64-apple-ios-sim` passes; host `cargo check --workspace` and
  `cargo test -p zz-daemon` stay green.

## M1 — crates/zz compiles for iOS; real client renders on the simulator

- `crates/zz/Cargo.toml`: move `gpui_platform`, `zz-browser` (default
  features), `zz-chrome-import`, `mimalloc` under
  `[target.'cfg(not(target_os = "ios"))']`; add iOS rows: `zz-browser`
  `default-features = false`, `zz-daemon` `default-features = false`,
  `zz-gpui-ios` dep.
- Gate desktop modules (browser subtree, app_icon AppKit half,
  window/background+drag+state, config iOS arms, bins) — follow the Windows
  port's patterns (module gates, per-field cfg, inert stubs like
  `agent/stub.rs`).
- Decouple `diagnostics` from `BrowserController`.
- iOS entry: `zz::run_ios()` (skip CLI/daemon-spawn/askpass bootstrap; build
  `Application::with_platform`) + `crates/zz/examples/ios.rs` cfg-gated main;
  app bundle recipe reuses the PoC shape (`target/ios-app/ZZ.app`,
  Info.plist without scene manifest, `simctl install/launch`, socket path via
  `SIMCTL_CHILD_ZZ_SOCKET` — keep it short, sun_path cap).
- Fix the two `terminal/view.rs` iOS constants.
- Exit: app on iPad sim shows the sidebar session tree and a live scrolling
  terminal attached to the host daemon. Touch taps focus panes (existing
  mouse synthesis).

## M2 — input, in zz-gpui-ios

- Hardware keyboard: `pressesBegan/Ended` on a first-responder view (no
  keyCommands), `UIApplicationSupportsIndirectInputEvents` in the plist; map
  UIKey → `gpui::Keystroke` → `PlatformInput::KeyDown/KeyUp`.
- Soft keyboard/IME: `UIKeyInput` conformance feeding the stored
  `PlatformInputHandler` (`replace_text_in_range`); stub `UITextInput` only
  for the spacebar floating cursor (remux pattern).
- Scroll: hidden UIScrollView ("Blink Shell pattern" — hitTest nil, 9×
  virtual content, contentOffset deltas → `ScrollWheelEvent` with UIKit's
  real deceleration). Plain pan-gesture synthesis is the fallback if this
  fights the PoC window model.
- Fix the CADisplayLink-never-fires bug (NSTimer 60Hz is the standing
  workaround); clipboard via UIPasteboard.
- Exit: type into a remote shell from the sim with a hardware keyboard;
  momentum scrollback.

## M3 — russh transport (physical device)

- In-process ssh: russh client → exec channel running `zz proxy --socket
  <remote.sock>` (`endpoint.rs:1577-1594` runs on any host), skip to the
  `zz-proxy-1\n` preamble marker, then the framed protocol over the channel —
  exactly the Windows stdio-proxy shape, minus the subprocess.
- `InteractiveClient` over a generic byte stream (today it's LocalStream-only).
- Auth: ed25519 key generated on-device, stored in Keychain **with
  `kSecAttrAccessible` set** (remux shipped without it — don't); password
  fallback; TOFU host-key pinning. Bootstrap hardening per remux: `exec
  /bin/sh -c` wrapper, single-line `;`-joined scripts (csh rule), `--` before
  hosts.
- Keychain storage landed 2026-08-08 in `zz-daemon/src/ios_keychain.rs`: a
  generic-password item (`dev.zz.ios.ssh` / `id_ed25519`) holding the OpenSSH
  text, `kSecAttrAccessible` =
  `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` — *AfterFirstUnlock*
  because the reconnect ladder can fire before the user unlocks, *ThisDeviceOnly*
  to keep the identity out of iCloud and backups. Raw `SecItem` FFI, because
  `security-framework`'s `PasswordOptions` keeps its query private and cannot
  set that attribute (it offers only `kSecAttrAccessControl`), and
  `security-framework-sys` exports the accessibility *values* but not the key —
  one extern static covers it. A v1 key file is imported and deleted on first
  run; `known_hosts` and `id_ed25519.pub` stay files, neither being secret.
- **The simulator does have a keychain — through a linker section, not a
  signature.** Measured 2026-08-08: `SecItem*` from the bundle `cargo xtask
  ios-sim` installs used to answer `errSecMissingEntitlement` (-34018,
  `securityd`: "Client has neither application-identifier nor
  keychain-access-groups entitlements"), and `codesign --entitlements` is not
  the fix — ad-hoc signing an app with those keys makes SpringBoard refuse the
  launch outright (`NSPOSIXErrorDomain 163`), with or without a team prefix.
  Simulator binaries are not code signed at all; the loader reads entitlements
  from a `__TEXT,__entitlements` section, which is what Xcode emits for
  simulator destinations. So `crates/zz/build.rs` links
  `crates/zz/ios/sim.entitlements` into that section for the one triple
  `cargo xtask ios-sim` builds (`aarch64-apple-ios-sim`; device builds are
  signed by Xcode and must not carry it), and the app gets access group
  `dev.zz.ios`. Verified on a booted iPad sim: add, update, read-back, and
  `kSecAttrAccessible` coming back as `cku` — the raw spelling of
  `AfterFirstUnlockThisDeviceOnly`. The same section makes a *test binary*
  keychain-capable under `simctl spawn`, which is what
  `ios_keychain::tests::stores_and_loads_the_identity` runs on (recipe in the
  test module; without the entitlements it fails `Unavailable` rather than
  passing vacuously).
- -34018, and only -34018, is still treated as "this build has no keychain" —
  an Intel simulator triple or a bare test binary — and falls back to the v1
  file with a warning; every other status (a locked device, say) is an error
  that fails the attach rather than silently minting a second identity.
  Unverified: a signed device/TestFlight build, where the access group gains
  the team prefix, and an end-to-end attach that actually calls
  `load_or_generate_key` (it needs a reachable ssh host).

## M4 — iPad lifecycle + polish

- Let the socket die in background; on foreground, probe and reconnect by
  full replacement — this is mostly `MuxClient`'s existing disconnect path +
  a UIApplication lifecycle hook.
- Safe-area insets, rotation/resize, appearance from
  `UITraitCollection.userInterfaceStyle`, accessory bar with sticky
  modifiers (Esc/Tab/Ctrl/arrows; expo-libghostty is the UX reference).
- Remove the `[zz-ios]` eprintln probes.

# Standing constraints (every handoff carries these)

- No `git stash`, no commits — parallel sessions share this working tree.
  The tree already carries the uncommitted zz-gpui-ios PoC; leave it alone.
- Touch only the crates the milestone names. Daemon behavior must not change:
  desktop builds keep identical features resolved (default-on features).
- `cargo fmt` scoped to touched crates only; afterwards `git diff --stat` and
  revert files you didn't intend to touch.
- Never edit the fork checkout (`~/.cache/zz-forks/zed`) or bump the pinned
  rev.
- The zz-daemon test suite has one known flaky test under full-workspace
  parallel load — `--no-fail-fast`, don't chase it.

# Risks / open questions

- **Glyph fast path**: the terminal element calls the fork's
  `paint_with_raster_data`/`compute_glyph_raster_data`; zz-gpui-ios has no
  `GlyphRasterData` plumbing and its renderer marks `SubpixelSprites` as
  `unreachable!`. Must verify at M1 that iOS degrades to the monochrome slow
  path instead of panicking; if the API surface demands platform support,
  stub it in zz-gpui-ios.
- **CADisplayLink never fires** on the sim (cause unknown; run loop verified
  healthy). NSTimer workaround holds until M2.
- `interprocess` crate on iOS (unix-socket path) is assumed to compile —
  verified transitively (iOS is `cfg(unix)`) but not yet built.
- Simulator ≠ device: MTLSimDevice already forced StorageModeShared
  everywhere; first device build will find more (signing, embedded bitcode,
  Keychain entitlements).
- Cargo feature unification: iOS builds go through per-target dep tables, so
  desktop and iOS resolve features independently — but any new unconditional
  dep on the session/daemon features breaks the iOS graph silently until the
  next iOS check. CI or a pre-push `cargo check --target aarch64-apple-ios-sim`
  guard is the eventual answer.

# v2 backlog (2026-08-08) — UIKit depth

v1 shipped a touch-and-hardware-keyboard terminal; the platform integrations
below are what separate "runs on iPad" from "is an iPad app". Ordered by
value-per-effort within each tier.

**Tier 1 — input correctness (small patches, all in zz-gpui-ios):**

1. `UITextInputTraits` on the input view: `autocorrectionType`,
   `smartQuotesType`, `smartDashesType`, `smartInsertDeleteType`,
   `spellCheckingType` all `.no` — today the soft keyboard can curly-quote
   shell input.
2. Soft-keyboard avoidance: nothing observes the keyboard frame; the
   on-screen keyboard covers the lower half including the key strip. Use
   `keyboardWillChangeFrame` → shrink the gpui window (resize callback), so
   panes and strip re-layout. Subsumes part of the dynamic safe-area item.
3. ~~Pointer phase 1~~ **landed 2026-08-08** (`zz-gpui-ios/src/window.rs`,
   "pointer" section): a `UIHoverGestureRecognizer` on the drawing view
   synthesises `MouseMove` while the pointer is inside (`Began`/`Changed`) and
   `MouseExited` when it leaves, carrying the modifier state the keyboard
   tracking already keeps — so every gpui hover style, tooltip and cursor
   region now works from the trackpad. `set_cursor_style` stores the style in
   a process-wide cell and sends `invalidate` to a `UIPointerInteraction`
   *only when it changed*, with the cell released first because UIKit is free
   to answer `invalidate` by calling the delegate straight back; the delegate
   (`pointerInteraction:styleForRegion:`, hand-rolled on the view like every
   other ObjC method here) answers a vertical `UIPointerShape` beam for IBeam
   and `nil` — the system pointer — for everything else, iOS having no arrow,
   crosshair or resize pointer to ask for. The load-bearing gpui fact:
   `reset_cursor_style` runs only when `Window::is_window_hovered()`, which
   off Windows/Linux resolves to `is_window_active()` — this backend's
   hard-coded `true` — so the style does reach the platform on iOS. No snap;
   item 6 stands.
4. ~~Secondary/middle click~~ **landed 2026-08-08**: `touch_click` reads
   `UIEvent.buttonMask` for `UITouchTypeIndirectPointer` touches at
   *`touchesBegan`* — a release reports only the buttons still held — and maps
   secondary → `MouseButton::Right`, `1 << 2` → `Middle`. UIKit names no
   constant past secondary, but `UIEvent.h` defines the mask as
   `1 << (buttonNumber - 1)`, which is all `UIEventButtonMaskForButtonNumber`
   computes, so button 3 is the middle one; whether iPadOS ever sets that bit
   is unverified (no hardware pass, and the simulator has no way to send it).
   The same read takes `UITouch.tapCount` as the click count for indirect
   pointers, so a trackpad double-click selects a word the way the desktop
   does; a finger's tap count is deliberately ignored (the long press owns
   "select this word"). Synthesised taps also carry the tracked modifiers now
   instead of `Modifiers::default()` — a Ctrl- or Shift-click has to arrive as
   one for terminal mouse reporting and range-select.
5. ~~Trackpad two-finger scroll~~ **stated 2026-08-08**: the transplanted pan
   recogniser sets `allowedScrollTypesMask` to discrete|continuous in
   `make_scroll_view`. It has to be stated rather than inherited — a
   `UIScrollView` configures its own recogniser, and this one has been moved
   onto a plain `UIView`. **Unverified**: the simulator delivers a host
   two-finger scroll as a touch pan, so only real trackpad hardware proves
   the continuous path.

**Tier 2 — pointer snap + accessibility:**

6. Pointer "snap" to elements needs per-element rects, which
   `Platform::set_cursor_style(style)` does not carry (gpui keeps the
   hitbox gpui-side, window.rs:3592, drops it at platform.rs:298). Routes:
   (a) style-only pointer from item 3 — no snap, no fork; (b) zz-side
   side-channel: ios_chrome registers pointer regions (key strip, sidebar
   rows, tabs) with the backend each frame → `UIPointerShape.roundedRect`
   regions, no fork but manual coverage; (c) small additive fork rider
   passing the hovered hitbox bounds through — full fidelity, adds rebase
   weight. Start (a), then (b); (c) only if (b) proves inadequate.
7. ~~VoiceOver~~ **wired 2026-08-08** (`zz-gpui-ios/src/window.rs`,
   "accessibility" section): the three `PlatformWindow` hooks now drive an
   `accesskit_ios::SubclassingAdapter` (0.1.2, which wants accesskit 0.24.1 —
   exactly what the pinned gpui resolves, so the crate takes its tree types
   from `gpui::accesskit` and holds no `accesskit` row of its own; that is the
   tripwire if a gpui bump ever moves off it). The adapter dynamically
   subclasses the *drawing* view, adding no ivars, so `zzWindowState` and every
   protocol the view conforms to survive the class swap; it is built with the
   window lock released because it catches up on the missed `didMoveToWindow`
   inline, and `QueuedEvents::raise` likewise, since UIKit may answer a
   notification by querying the view straight back.
   `a11y_update_window_bounds` stays a no-op — element frames are converted
   from the view's own space per query, and the pinned gpui never calls it.
   Coverage still depends on zz-ui's element annotations (separate pass); the
   terminal grid needs a custom text node eventually. Unverified with VoiceOver
   actually running.
8. ~~System text-size and traits~~ **landed 2026-08-08**
   (`zz-gpui-ios/src/system_traits.rs`): the text size is read as a *factor*,
   not a category — `UIFontMetrics.defaultMetrics scaledValueForValue:17` over
   a trait collection built from `UIApplication.preferredContentSizeCategory`,
   divided by 17 — which is two calls instead of a twelve-string switch and
   keeps working for categories Apple adds later. It is published as a relative
   delta on the pinch's channel shape (`take_content_size_scale`, consumed in
   `ios_chrome`), so raising the system size multiplies the UI zoom instead of
   stamping on whatever the user pinched; refreshed at window open and from the
   existing `traitCollectionDidChange:` (atomics only — that callback must not
   re-enter gpui). Accessibility text sizes reach ~3.1x and clip against
   `MAX_UI_ZOOM` (300%). `reduce_motion`/`reduce_transparency` are exported as
   live reads of the two `UIAccessibilityIs*Enabled` functions. Startup folds
   `reduce_motion` into the global interface-animation preference; a live flip still forces no
   redraw. `reduce_transparency` remains unconsumed.

**Tier 3 — platform citizenship:**

9. ~~Scene lifecycle~~ **landed 2026-08-08** (`zz-gpui-ios/src/platform.rs`,
   `ZZGPUISceneDelegate`): `UIApplicationSceneManifest` (single scene) is in
   both `crates/zz/ios/Info.plist` and the generated-plist properties in
   `project.yml`, and the launch closure now runs from
   `scene:willConnectToSession:` instead of `didFinishLaunchingWithOptions:` —
   the first moment there is a scene for `-[UIWindow initWithWindowScene:]` to
   attach to. The manifest's *presence* is the switch: `didFinishLaunching`
   asks `NSBundle` for the key and only launches the old way when it is
   missing, so both paths stay live and removing the key from `Info.plist` is
   the rollback. Everything else was kept whole — the window still builds the
   root view controller, drawing view, display link, scroll-view transplant,
   keyboard observation and a11y adapter in `IosWindow::open`, which now sizes
   from the *window* (the scene's coordinate space) rather than
   `UIScreen.bounds`, so a Split View or Stage Manager launch starts at the
   right size. Resizes need no new plumbing: UIKit owns a scene-attached
   window's frame, and both `windowScene:didUpdateCoordinateSpace:…` (iOS
   16–25) and `windowScene:didUpdateEffectiveGeometry:` (iOS 26, which
   deprecated the former) only `setNeedsLayout` the drawing view, leaving
   `layoutSubviews` — already the one place the drawable is resized and gpui's
   resize callback fires — to run on UIKit's own layout pass with no gpui
   update open. `applicationDidBecomeActive:` stops being called for a
   scene-based app, so the reconnect nudge moved to `sceneDidBecomeActive:`;
   both spellings call one shared handler and share the
   first-activation-is-launch flag. Verified on the iPad sim: boots windowed
   under iPadOS 26, chrome and key strip align to the window, a system
   text-size change still drives the zoom, and a background/foreground round
   trip keeps the process. Rotation verified 2026-08-08 on the iPad Pro 11"
   sim (Device ▸ Rotate Left): the screenshot goes 1668×2420 → 2420×1668 and
   the chrome re-lays out to the landscape frame, so the geometry callback
   does reach `layoutSubviews`. Not verified: an actual Stage Manager corner
   drag.
10. ~~Touch text selection~~ **landed 2026-08-08** (`zz-gpui-ios/src/window.rs`,
    "touch text selection" section): a 0.45s `UILongPressGestureRecognizer`
    synthesises a left `MouseDown` (click count 2, so a press with no drag
    already holds the word), `MouseMove`s while the finger travels, and a
    `MouseUp` on lift — the app's own mouse-selection path, untouched. The
    transplanted pan recogniser is disabled for the length of the drag, which
    is what keeps the grid still; a flick never trips the press, so momentum
    scrolling is unchanged. On release a `UIEditMenuInteraction` (no delegate,
    so UIKit builds the menu from `canPerformAction:withSender:`) offers Copy
    and Paste, each spelled as the `platform-c`/`platform-v` chord zz already
    binds — so copy stays on the pane's existing copy path and the pasteboard
    keeps one writer. Still owed: UIKit-drawn selection handles and the
    magnifier, which need real `selectionRectsForRange:` geometry the terminal
    cannot supply through `PlatformInputHandler`. Unverified on hardware or
    simulator.
11. ~~`UITextInput` stub~~ **landed 2026-08-08** (`zz-gpui-ios/src/window.rs`,
    "text input document" section): `ZZTextPosition`/`ZZTextRange` over UTF-16
    offsets, every query forwarded to `PlatformInputHandler` with
    gpui_macos's `NSTextInputClient` semantics, so marked-text IME and
    dictation now have a document to compose against. `UIKeyInput` stays the
    fast path for plain characters — untouched. Two knowingly-partial edges:
    the document ends at the composition/caret for elements that do not
    implement `text_length_utf16` (the terminal), and the floating cursor is
    accepted but does not move the terminal cursor (the shell owns that
    caret; it would mean synthesising arrow keys). Unverified on hardware
    or simulator — no CJK/dictation smoke has been run.
12. Drag and drop **(drop-in landed 2026-08-08)**: a `UIDropInteraction` on
    the drawing view takes anything that loads as `NSURL` (file → its path,
    link → its address) or `NSString`, joins several items with a space, and
    inserts the result through `replace_text_in_range` — the path
    `insertText:` already takes. The load is async and UIKit names no queue,
    so the completion parks the string in a cell and the next frame pump
    drains it, which is also what keeps it off a re-entrant update. Still
    open: images (they want the pasteboard + daemon upload path a drop never
    touches), shell quoting of dropped paths, aiming the text at the pane
    under the finger rather than at the focused one, and drag *out* of a
    selection.
13. Cmd-hold shortcut HUD **(landed 2026-08-08, three entries)**, pinch →
    `UiZoom` **(landed)**, home-indicator autohide **(landed)**; app icon
    still open.
    - HUD: `keyCommands` publishes Cmd-C/Cmd-V/Cmd-A spelled as the standard
      edit selectors the view already answers (`copy:`/`paste:`/`selectAll:`,
      each synthesising the `platform-<key>` chord). Exactly-once holds
      whichever way UIKit orders the two paths, because every entry carries
      Command: a Command chord has no `key_char`, so `presses_began` never
      calls super and the command is never reached; and if UIKit matched the
      command first, the press never arrives. That argument is why nothing
      modifierless — the key strip's Esc/Tab/arrows — is registered.
      **Unverified**: whether UIKit raises the HUD at all while
      `pressesBegan:` swallows the bare Command press without calling super.
      If it does not, the fix is to propagate *modifier-only* press sets to
      super (provably inert: a bare modifier matches no `UIKeyCommand`).
    - Pinch: a `UIPinchGestureRecognizer` accumulates per-callback deltas into
      a process-wide factor the app takes on the next frame
      (`take_pinch_scale`) and multiplies its zoom by, same channel shape as
      `keyboard_inset` and for the same re-entrancy reason. The transplanted
      pan is switched off for the length of the gesture, the way the long
      press already does it.

Carried from the v1 status block, still open: physical-iPad smoke
(`just ios-device` tooling landed 2026-08-08, build verified, install/
launch/attach unverified — which is also the first real exercise of the
Keychain key storage, see M3) and the IosChrome reentrant-update crash from
the key strip (being fixed in a parallel session).

The "wide letter-spacing" was never spacing: it was proportional Helvetica.
`select_family_by_name` cannot fail on CoreText — font-kit builds a collection
out of the descriptor it was handed, and `CTFontCreateWithFontDescriptor`
substitutes Helvetica for a family the device has not got — so a desktop
config's `font-family` (the sim inherits the host `XDG_CONFIG_HOME`) drew the
terminal grid with proportional advances and logged nothing. Fixed 2026-08-08
in `zz-gpui-ios/src/text_system.rs`: the family is *matched* first
(`CTFontDescriptorCreateMatchingFontDescriptors`, which does answer "no", and
answers it correctly for the hidden dot-prefixed system families), and an
unmatched name is aliased to Menlo before `.AppleSystemUIFont`, with one warn
naming both. Measured on both sims: 20×`i`, 20×`m` and 20×`W` end at the same
pixel column.

# v3 (2026-08-08) — app crate, composition, mobile product layer

The audit that motivated this: the transport (russh + Keychain + password
dialog) was device-viable end to end, but there was no mobile *product* on
top — a fresh iPad booted into a raw `io::Error`, the only add-host entry
point was a `…` dropdown on the dead local row, the generated public key was
unreachable, and the settings surface showed tray/browser/blur/quit-daemon
controls for subsystems that are compiled out. Landed (uncommitted), one
codex handoff + review pass:

- **`crates/zz-ios`**: thin app crate — cfg-gated `main` (empty on host so
  `--workspace` builds it), `zz = { default-features = false }` so plain
  feature resolution replaces the old `--example ios --no-default-features`
  incantation, bundle assets moved to `crates/zz-ios/ios/`, sim-entitlements
  embedding moved to its `build.rs` (`rustc-link-arg-bins`). The zz example
  target is deleted; xtask/scripts/project.yml build `-p zz-ios`.
- **`AppProfile`** (`crates/zz/src/profile.rs`): composition inversion. A
  struct of pub fields (settings sections in nav order; `has_tray`,
  `has_window_blur`, `has_daemon_lifecycle`, `has_config_import`;
  `local_host: LocalHostPolicy`; `fixed_window`) installed as a Global by
  each entry point — `AppProfile::desktop()` in `run()`/`RunWinMain`, a
  literal in `zz-ios/src/main.rs`. Views consult the profile, never
  `cfg(target_os)`. Test builds fall back to the desktop profile.
- **Host policy**: `LocalHostPolicy::IfEnvSocket` — the registry synthesizes
  `local` only when `$ZZ_SOCKET` is set (sim rig), and `HostId` assignment is
  stable either way (`LOCAL`'s slot 0 is reserved, configured hosts always
  start at 1). With no local host: a placeholder LOCAL connection keeps the
  attached-connection invariants, remotes auto-dial at launch (the old gate
  on local-connected is bypassed), `release_host` grew a no-local detach arm,
  and `run_ios` skips the 3-second connect-retry window entirely (the
  policy's `synthesize_local()` is the single shared answer).
- **First-run**: empty registry → the new-session panel renders a connect CTA
  ("Connect to a computer" + Add host… button → the existing add_host
  dialog) instead of keyboard-chord hints; `new_session` targets the
  registry's first host instead of hardcoding LOCAL. The sidebar controls
  cluster gained a `+` add-host button (both chrome modes, all platforms).
  Host-failure reasons now also fire a warning toast on tap (were
  hover-tooltip-only). `NSLocalNetworkUsageDescription` added to both
  plists (dialing `user@mac.local` trips the iOS 14+ local-network gate).
- **Honest surface**: iOS composes no Browser settings page; tray/blur/
  quit-daemon rows are profile-gated; the quit hook always detaches on iOS
  (honoring `quit-daemon-on-exit` there would `kill-server` the desktop
  daemon on every app eviction — it was live before); pane picker and
  palette stop offering browser (the stub's `is_available()` finally has
  call sites; `PaneKindAvailability` absorbed the experimental-panes
  struct); `open_url` implemented in zz-gpui-ios (About links work); import
  buttons/prompt gated off.
- **Apple family membership**: iOS joins the macOS cfg arm for ⌘ glyphs
  (kbd.rs), cmd-based input/code-editor bindings, zoom/settings chords, and
  Menlo mono defaults (was Noto Sans Mono/DejaVu — fonts no iPad ships).
  Windowing-flavored `not(macos)` arms deliberately stay (tray, drag,
  titlebar layout, process/env).

Still owed after v3 (the next campaign, in order of user pain): pubkey
surfacing + ssh-copy-id-over-password-session key bootstrap, TOFU host-key
confirmation wired to the existing (currently dead-on-iOS) `ssh_prompt`
HostKey dialog, keyboard-interactive auth; plus the carried v2 items
(VoiceOver runtime pass, physical-device smoke). Deliberately deferred:
Bonjour discovery of Macs (mDNS was deleted in the ssh-only consolidation;
`user@mac.local` in the add-host field is fine), a `zz-core` extraction
(two app crates over one library is enough until the desktop-only mass in
`zz` actually hurts).

# v4 (2026-08-08, same evening) — engine surface + iOS-owned shell

v3 gave the iOS app composition ownership; v4 gives it **shell ownership**. The
premise: profile flags scale to hiding rows, not to different interaction
paradigms — mobile-native UI needs files desktop never compiles. Four pieces,
each a codex handoff + review (build gates, desktop-identical audit, and an
interactive Simulator pass driving real taps):

- **Engine surface** (`crates/zz/src/engine.rs`): the deliberate public facade
  turning `zz` into a library with two consumers. Nested `pub use` mirroring
  internal paths (`zz::engine::config::init`); items go `pub` at definition,
  modules stay private, and the header documents the growth rule: an app
  needing a desktop-view internal means fork the view or push the helper to
  zz-ui — never widen casually. `run_ios` + the iPad chrome moved to
  `crates/zz-ios` (`src/app.rs`, `src/chrome.rs`); the sticky-modifier
  machinery stayed in zz as `ios_input.rs` because `TerminalView` consumes it
  (moving it would invert the dependency). `engine::connect_local` owns the
  connect-or-skip policy in one place.
- **Navigation brain shared, drawer iOS-owned**: the pure tree projection
  (`MuxTreeModel::from_hosts` etc.) and the activation machine
  (`NavActivation`, `activate_nav`, command builders) moved from sidebar.rs to
  `crates/zz/src/mux/nav.rs` — desktop sidebar re-imports, one brain for both
  shells. `zz-ios/src/drawer.rs` renders it touch-first: 44pt rows,
  always-visible affordances and wrapped failure text (no hover anywhere),
  scrim dismiss, daemon `prefix+s` opens it via `sidebar_focus_revision`.
  The shell (`chrome.rs`) grew a 44pt top bar (drawer toggle / attached-session
  title / settings gear), took over AppShell's key-up capture and OpenSettings
  handling, and fixed the historical key-strip reentrancy crash for good
  (`window.defer` around `dispatch_keystroke`; the old cherry-pick died when
  the file moved). `WorkspaceSidebar` stays alive as route/state holder with a
  new `release_focus` so a non-sidebar navigator can hand focus back.
- **Settings state shared, presentation iOS-owned**: the engine exports the
  full settings state API (resolved_config/ConfigValue/provenance, per-key
  writers, atomic preset write, theme vocabularies, ui_scale, the hoisted
  editor-IO helpers) and `WorkspaceSidebar::open_settings_route` flips the
  route WITHOUT constructing the desktop SettingsView. `zz-ios/src/settings.rs`
  is a full-screen drill-down (Root → section pages → editor pages) honoring
  the load-bearing invariants: Terminal edits the *view* and saves only via
  `save_appearance_editor` (verified on disk: app-side keys hoisted above the
  buffer), observed-snapshot reconcile against the 500ms poller, commit on
  enter/blur/page-leave, provenance-gated Reset affordances. Desktop settings
  untouched (nav gained group labels — the one visible desktop delta, via
  zz-ui's `SettingsNavigationGroup`).
- **Connect groundwork**: `engine::workspace::add_fleet_host` exported for a
  future app-owned connect flow; dialog layer verified live in the new shell
  (add-host dialog over the workspace, iOS copy). No onboarding UI built —
  deferred by explicit decision ("groundwork now, tweak UI later").

Verified interactively on-sim (isolated `XDG_CONFIG_HOME` so smoke taps never
touch the real config): drawer open/activate/dismiss, settings drill-down,
theme-mode write → poller → live re-theme with provenance Reset appearing,
editor dirty→Save→splice-correct file, add-host dialog, key strip taps —
zero panics across the whole session. Known minors for the UI pass:
page-leave commits an unchanged stepper as an explicit override (spurious
provenance flip); accent-popup quirk under synthetic sim typing (not
reproducible with real input paths).

# Validation

```sh
# M0 gate
cargo check -p zz-terminal --no-default-features --target aarch64-apple-ios-sim
cargo check -p zz-daemon  --no-default-features --target aarch64-apple-ios-sim
cargo check --workspace                    # desktop unchanged
cargo test  -p zz-daemon --no-fail-fast

# M1 gate (historical — the example target is gone since v3)
cargo build -p zz --example ios --no-default-features --target aarch64-apple-ios-sim
# then: bundle + simctl install/launch per the PoC recipe, screenshot, pixel-sample

# v3 gate (current)
cargo check -p zz-ios --target aarch64-apple-ios-sim   # no feature flags needed
cargo check --workspace
just ios                                               # sim smoke, real config
```
