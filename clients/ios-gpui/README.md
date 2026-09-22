# GPUI iOS client

The app opens the session sidebar beside the attached window's panes. It uses
`zz-ui` for the shell, navigation, pane frames, headers, terminal painting, and
agent interface. Web and iOS compile the same app shell, sidebar, status bar, settings,
command palette, overlays, terminal, agent, connection reducer, and image caches
from `clients/gpui-shared/src`.
The sidebar is 256 points wide and respects the iOS safe area. On iPhone it
opens over the workspace and closes when you select a session, window, or pane.

## Run

Use an Apple Silicon Mac with Xcode, Rust's `aarch64-apple-ios-sim` target, and
Zig 0.16.0. From the repository root:

```sh
just ios-gpui
just ios-gpui iPhone
just ios-gpui iPad build
```

To run on a paired iPad or iPhone, unlock it and use device mode:

```sh
just ios-gpui iPad device
```

Device mode builds a release arm64 binary, signs it with your first Apple Development identity
and a provisioning profile that covers `dev.zz.gpui-poc` (a team wildcard works), installs it with
`devicectl`, and launches it with `ZZ_GPUI_ENDPOINT=ssh://$USER@<this Mac>.local`. The app saves
that endpoint, so later launches from the home screen reconnect. The Mac needs Remote Login
enabled and `zz-dev` on its PATH (desktop dev runs link it into `~/.local/bin`). Sign in with your
password, or copy the app's key from Settings › Hosts into `~/.ssh/authorized_keys`. Override the
choices with `ZZ_GPUI_DEVICE`, `ZZ_GPUI_SIGN_IDENTITY`, `ZZ_GPUI_PROFILE`, or `ZZ_GPUI_ENDPOINT`.

Simulator and device builds use the zz Dev icon (`assets/zz-dev.icon`, compiled with `actool`).

## TestFlight

```sh
just ios-gpui iPad testflight
```

TestFlight mode builds a release binary with the production identity (`zz` on the host), packages
it as `dev.zz.ios` ("zz", the bundle the native client shipped under) with the zz icon, workspace
marketing version, and a UTC timestamp build number, and wraps it in an `.xcarchive` under
`target/ios-testflight`. `xcodebuild -exportArchive` then signs it for App Store distribution and
uploads it for internal TestFlight testing. Signing uses the Apple account signed into Xcode; set
`APPLE_API_ISSUER_ID` to use the App Store Connect key in `../.zz-signing` instead.
`ZZ_IOS_UPLOAD=0` exports the signed `.ipa` without uploading, and `ZZ_IOS_BUILD_NUMBER` overrides
the build number.

The launcher probes zz Dev socket candidates and skips stale sockets. It does not start a daemon. To
choose a socket and session:

```sh
ZZ_GPUI_ENDPOINT=/tmp/my-zz.sock ZZ_GPUI_SESSION=work just ios-gpui
```

`ZZ_DEV_SOCKET` also selects a local socket. Set `ZZ_GPUI_SIMULATOR` to a simulator
UDID to choose an exact device. The bundle ID is `dev.zz.gpui-poc`, its display name
is **zz GPUI**, and build products live in `target/ios-gpui/ZZ GPUI.app`.

The tree shows the connected host, its sessions, windows, and panes. Tap a marker to expand or
collapse it, or a row to select it. Arrow keys navigate the tree; Enter selects
the highlighted row. The plus buttons create sessions and windows. Long-press a
row or window pill to open its rename and close menu. Swipe to scroll. The top sidebar button hides the sidebar and remains available to reopen
it. The gear opens Settings; Command-comma also toggles Settings. The search button
opens the command palette without a hardware keyboard.

Without a daemon, the workspace shows connection status and a reconnect button.
SSH endpoints show host-key confirmation and authentication dialogs. Endpoint selection
currently uses `ZZ_GPUI_ENDPOINT`; a saved host manager remains a later step. The terminal example shares
the native transport, key translation, and UIKit backend.

## Panes

- Terminal and Agent panes use the web client's working pane entities. Each pane
  keeps its own input focus, viewport, and content state.
- Split right or bottom from the header, then choose Terminal or Agent. Close
  removes the pane through the daemon. Drag a divider to resize the split.
- A zoomed pane has an Exit zoom control. Drag a header grip to split or swap panes;
  the preview animates to the drop target. Mouse and touch divider drags update the daemon.
- Hardware keys, search, copy/paste, terminal images, mouse tracking, and
  scrollback use the shared terminal implementation. Swipe to scroll; long-press
  and drag to select text.
- Agent panes include the transcript, composer, permissions, provider/model
  controls, a combined project/conversation picker, and queued prompt feedback.
  Plans, permissions, and structured tool output use the shared transcript reducer.
  Code blocks use the native syntax highlighter. Command-V attaches a PNG or JPEG
  from the pasteboard; there is no image picker button.
- Existing Browser panes show a deferred message. Existing Editor panes explain
  that file contents are not yet shared by the daemon. Neither appears in the
  new-pane picker.

Command-P, Command-K, or the search button opens the palette that desktop and web also use. Use `:` for commands,
`@` for windows, `%` for panes, and `~` for the connected host. Opening it closes
any daemon prompt or chooser, as on desktop. Daemon choosers, command prompts, menus,
confirmations, and popup terminals use the shared interface. Command output replaces
its pane's content, and display-panes labels keep their tmux styles and alignment.
Notices preserve severity, duration, and explicit clearing; a failed command reports
as `command: error`.

The sidebar button reopens navigation on iPhone.

## Keyboard

Without a hardware keyboard, tapping a terminal, the agent composer, or any text
field raises the on-screen keyboard. A row above it adds Escape, Tab, Control, Option,
the arrows, and a hide button. Control and Option latch for the next key, so
Control then C sends Ctrl-C. The workspace shrinks to the space above the docked
keyboard; a floating keyboard leaves the layout alone. Attaching a hardware
keyboard hides the on-screen one, and detaching it brings it back for the focused
field. Autocorrect, smart punctuation, and capitalization stay off unless a field
asks for them.

The view implements `UITextInput`, so IME composition (Japanese, Chinese, Korean),
dead keys, dictation, and Pencil handwriting reach the focused field. While text is
being composed, hardware keys go to the input method first.

## Windows, menus, and links

- Each iPad window (Stage Manager, split view, or an external display) runs its own
  workspace with its own connection. New windows come from the system's window controls.
- The iPadOS menu bar and the Command-hold shortcut list show the zz, File, and View
  commands with their Command shortcuts: Settings, New Session, New Window, Split Right,
  Split Down, Close Pane, Kill Window, Command Palette, Choose Window, Toggle Sidebar,
  Zoom Pane, and UI zoom.
- `zz://attach/<session>` (`zz-dev://` for development builds) switches the frontmost
  window to that session, or attaches to it after the next connect. Shortcuts can open it
  with the Open URL action. The deprecated native dev app also claims `zz-dev://`; remove
  it if links open the wrong app.
- Long-press text in a terminal to select it; the system edit menu offers Copy, Paste,
  and Select All.
- Drop images onto the workspace to upload them to the focused pane, as with a pasted
  image; dropped text or links paste as text.

## Lifecycle

The connection reconnects two seconds after it drops, and immediately when the app
returns to the foreground. Leaving the app keeps the connection open for the short
background time iPadOS allows. Settings › Advanced › Display can keep the screen
awake while connected. Under thermal pressure or Low Power Mode, frames are capped
at 60 per second. iPadOS text size scales the interface (turn it off in Advanced ›
Display); Reduce Motion and Increase Contrast apply live. The text system loads the
iOS system fonts, so Chinese, Japanese, Korean, Arabic, Hebrew, and other scripts
render. Emoji still show as blank: GPUI's text system only treats Noto Color Emoji
as a color font.

## Settings

Settings uses the shared `zz-ui` form rows, previews, palettes, color pickers,
number fields, and switches. iPad keeps the section navigation beside the page;
iPhone uses a full-width page with a section menu and a back button.

- **Appearance:** System/Light/Dark appearance, light and dark palettes,
  background/foreground/accent colors, UI zoom, contrast, animations, widget
  corners, font family, and shadow strength. Fresh installs use System appearance;
  saved choices survive upgrades. System appearance follows live iOS changes;
  pinned modes also update the native status bar.
- **Panes:** live preview, gaps, background opacity, inactive opacity, selected
  glow, margin, corner radius, border width, and whether new Agent panes are offered.
  Frame controls apply with gaps.
- **Terminal:** a live preview, local font family, and text scale. Host colors,
  cursor, and padding remain visible as host-owned settings.
- **Status bar:** session menu, window badges, and agent activity controls.
- **Hosts:** the configured endpoint, connection status, and reconnect.
- **Advanced:** command palette layout (tree or flat), host prefix, and command shortcuts;
  Display: drawing under the home indicator, matching the system text size, and keeping
  the screen awake while connected.
- **About:** app version and source link.

The page omits desktop-only options and controls for unsupported pane kinds.
Interface and pane preferences are saved atomically in the app container
at `Library/Application Support/zz-gpui/preferences.json` and restored on launch.
Touch controls work without a keyboard; numeric values and hex colors open the
on-screen keyboard.

## Terminal experiment

The terminal uses `InteractiveClient`, `ClientCore`, and the shared `zz-ui`
painter and Kitty image cache. The daemon owns the PTY and Ghostty parser/encoder.

```sh
ZZ_GPUI_DEMO=terminal just ios-gpui
```

To attach the terminal example to a specific socket and session:

```sh
ZZ_GPUI_DEMO=terminal ZZ_GPUI_ENDPOINT=/tmp/my-zz.sock ZZ_GPUI_SESSION=work just ios-gpui
```

Without a saved endpoint, the terminal example opens a connection field. **Host** opens that
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
- A trackpad or mouse drives the app like a desktop pointer: hover states, two-finger and
  wheel scrolling, clicks and drags as mouse events, secondary click for context menus, and an
  I-beam pointer over text.
- Finger flicks and two-finger trackpad scrolls coast with `UIScrollView`'s deceleration
  (0.998 per ms); wheel ticks do not coast. Scroll views, lists, and the terminal's scrollback
  rubber-band past their edges and spring back, and a fling that reaches an edge bounces off it.
  Mouse-tracking applications, copy mode, and screens without scrollback keep plain scrolling.
  The bounce lives in the gpui fork (`Overscroll::Bounce` in the backend's gesture tuning).

The standalone example displays one active terminal pane. The main app supports
multiple terminal and agent panes; full mux overlays remain a later step.

## Verification

Pane checks on iPad and iPhone Simulator covered terminal input, creating and
closing splits, dragging dividers, zoom, focus after creating a pane, scrollback,
long-press selection, and copy/paste. An isolated ACP fixture exercised the agent
composer, transcript, and permission response. Pane settings updated the preview
and workspace and survived relaunch. The shared web tests and WASM build pass.

Settings checks on iPad and iPhone Simulator covered section navigation,
light/dark/system appearance (including a live OS change), palettes and color
overrides, zoom, contrast, animation and corner controls, scrolling, reconnect,
and persistence across relaunch. Preference tests cover file round trips,
missing fields, invalid data, and numeric limits.

Sidebar checks on iPad and iPhone Simulator covered touch expansion, session and
pane selection, session/window creation, hiding and reopening the sidebar, and
scrolling a tree with 14 windows. Hardware End/Enter selected a pane in another
session and activated its containing window. The shared tree navigation tests,
native keyboard tests, iOS Clippy, and arm64 device build pass.

Simulator checks exercised raw PTY bytes for normal/application arrows,
modifiers, Tab, function/navigation keys, repeats, Kitty releases, and bracketed
paste. Vim opened, edited, and saved a file through hardware key events. A live
shell also accepted hardware input on iPhone. An ANSI fixture and a Kitty image
rendered on iPad. Automated checks cover native key
translation, terminal bindings, and the shared image cache; the web client also
builds after adopting that cache.

iPad Simulator checks on 2026-09-22 covered the on-screen keyboard, key row, Control
latch, arrows, hide and system dismiss, tap to reopen, hiding on hardware keyboard
attach, layout above the keyboard, Option-E dead keys, CJK/Arabic/Hebrew rendering,
automatic reconnect after a daemon restart, live text size changes, the Command-D menu
command, a second window scene, `zz://attach/<session>`, and long-press Copy/Paste.
Drag and drop, dictation, Pencil handwriting, CJK input methods, and the menu bar
itself were not exercised.

Copy/paste of text copied within the app was verified. Text injected through
`simctl pbcopy` was not readable from UIKit in this simulator session; cross-app
paste still needs a device check. Physical keyboard layouts, dead keys, SSH
authentication, rotation, suspension/reconnect, and GPU recovery have not been
verified on hardware. The arm64 device binary builds, but the launcher currently
packages simulator apps only.

```sh
cargo test --locked -p zz-gpui-ios --test preferences --test keyboard
cargo test --locked -p zz-ui terminal_images::tests
cargo test --locked --manifest-path clients/web/Cargo.toml
cargo fmt -p zz-gpui-ios -p zz-ui --check
IPHONEOS_DEPLOYMENT_TARGET=26.0 cargo clippy --locked -p zz-gpui-ios --all-targets --target aarch64-apple-ios-sim -- -D warnings
IPHONEOS_DEPLOYMENT_TARGET=26.0 cargo build --locked -p zz-gpui-ios --bin zz-gpui-ios --target aarch64-apple-ios
just web-build
```

## Recovered history

- `015b3a39` (2026-08-07): original UIKit backend and GPUI card demo.
- `22c6bb5d` (2026-08-08): separate GPUI iOS client.
- `4c23f8d6` (2026-08-15): replacement with the Swift client. Its parent contains
  the last full GPUI iOS implementation.

`crates/zz-gpui-ios` restores the small window/display adapter and scene startup
from that work. It uses the pinned GPUI fork's wgpu renderer and CosmicText font
system, with the web client's Inter and Lilex fonts plus the memory-mapped iOS
system fonts. It explicitly selects Metal because
GPUI's native wgpu convenience constructor defaults to Vulkan and GL.
