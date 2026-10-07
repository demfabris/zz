# GPUI iOS client

The app opens the session sidebar beside the attached window's panes. It uses
`zz-ui` for the shell, navigation, pane frames, headers, terminal painting, and
agent interface. Web and iOS compile the same app shell, sidebar, status bar, settings,
command palette, overlays, terminal, agent, connection reducer, and image caches
from `clients/gpui-shared/src`.
The sidebar is 256 points wide and respects the iOS safe area.

On iPhone, and in any window narrower than 640 points, the attached session uses the
phone shell instead: one pane fills the screen and a sideways swipe pages through every
pane in window order. Landing on a pane zooms it (`resize-pane -Z`), so the daemon sizes
the window to that pane; terminal previews keep the neighbouring pane live while it
slides in. The bottom bar has a tree button (the sidebar tree as a bottom sheet), the
pane name with page dots grouped by window (tap it for the window chooser), and a
keyboard button. The keyboard opens only from that button or a tap on the terminal.
While it is up, a key row replaces the bar: hide, esc, tab, ctrl, alt, `|`, `~`, `/`,
prefix, and an arrow pad. Keys with a dot share one gesture: tap for the key's own job,
hold or slide up for a card, and release on an item to pick it. A tap latches ctrl or
alt for the next key; holding them offers common chords (^C, ^D, ^Z, ^R, M-b, M-f).
Prefix opens a short menu (new pane, new window, rename pane, last pane, kill pane) and
an All bindings sheet that sends any prefix binding through the daemon's key table. The
arrow pad repeats while held.

Both sheets follow the finger when dragged down by the grabber or the header, or by their
content once it is scrolled to the top. Dragged past about half their height, or flicked down,
they close; a shorter drag springs back, and dragging up resists. A tap on the dimmed area above
still closes them. Small controls take taps across a 44-point box around the glyph (pane close,
settings back, tree actions, reset and stepper buttons, switches, select menus), workspace tree
rows are 44 points tall, and each key in the key row answers across the whole row height and
up to the middle of the gap to its neighbours.

The phone sizes chrome like iOS: settings, the workspace tree, menus, dialogs, and daemon
overlays render their text at 17 points instead of the desktop's 13 (everything sized in rems
grows by 17/13), and settings get a 44-point navigation bar with a "‹ Settings" back button and a
centred title. Switches and color wells sit at the right of their row; wider controls stay below
the description. A rightward drag anywhere on a settings page goes back, as the iOS 26 content
back gesture does, except on a horizontal strip (the palette tiles) that can still scroll back.

Buttons, rows, menu items, and keys show presses the way Flutter's CupertinoButton and React
Native's Pressability do: a touch lights the control 100 ms after it lands unless it turns into a
scroll first, the highlight fades in over 120 ms and out over 180 ms, and a quick tap still shows
it for 130 ms. Keys in the key row light at once. Lifting a finger clears hover, so a tapped row
does not stay lit.

Pinch a terminal to change its text size, as in Blink Shell: the pinch starts once the
fingers' spread changes by 6% and by at least 8 points (gpui's touch slop), then follows the
fingers from that spread. The size moves in 5% steps between 50% and 300% (the Terminal
font scale setting), and a badge shows the percentage while pinching and for a second after.
The text resizes live; the pane reports its new grid to the daemon once, after the fingers
lift, and the size is saved then. A second finger cancels the first finger's scroll, page
swipe, or tap, so two fingers never page or scroll.

## Run

Use an Apple Silicon Mac with Xcode, Rust's `aarch64-apple-ios-sim` target, and
Zig 0.16.0. From the repository root, `just ios` lists the actions. Each takes `iPhone`
(the default) or `iPad`:

```sh
just ios rig
just ios run
just ios run iPad
just ios build
```

`just ios rig` builds `zz_cli` and starts a throwaway daemon for simulator runs, with its own
home under `target/ios-rig` and a socket at `/tmp/zzios-<checkout folder>.sock`, so every
worktree gets its own. It seeds a session named `phone` with two panes in the first window and
a second window, and `just ios run` attaches to it while it is up. `just ios rig stop` stops it.
`ZZ_IOS_RIG_SOCKET` picks another socket.

To run on a paired iPhone or iPad, unlock it and use device mode:

```sh
just ios device
just ios device iPad
```

Device mode builds a release arm64 binary, signs it with your first Apple Development identity
and a provisioning profile that covers `dev.zz.gpui-poc` (a team wildcard works), installs it with
`devicectl`, and launches it with `ZZ_GPUI_ENDPOINT=ssh://$USER@<this Mac>.local`. That launch
connects straight away and adds the host to the saved list; later launches from the home screen
open the connection screen with it at the top. The Mac needs Remote Login
enabled and `zz-dev` on its PATH (desktop dev runs link it into `~/.local/bin`). Sign in with your
password; after a password sign-in the app offers to add its key to `~/.ssh/authorized_keys` so
later connections skip the password. The key is also in Settings › Hosts to copy by hand. Override the
choices with `ZZ_GPUI_DEVICE`, `ZZ_GPUI_SIGN_IDENTITY`, `ZZ_GPUI_PROFILE`, or `ZZ_GPUI_ENDPOINT`.

Simulator and device builds use the zz Dev icon (`assets/zz-dev.icon`, compiled with `actool`).

## TestFlight

```sh
just ios testflight
```

TestFlight mode builds a release binary with the production identity (`zz` on the host), packages
it as `dev.zz.ios` ("zz", the bundle the native client shipped under) with the zz icon, workspace
marketing version, and a UTC timestamp build number, and wraps it in an `.xcarchive` under
`target/ios-testflight`. The `testflight` Cargo profile keeps line tables, so the archive carries a
dSYM for crash symbolication and the shipped binary is stripped afterwards. `xcodebuild -exportArchive` then signs it for App Store distribution and
uploads it for internal TestFlight testing. Signing uses the Apple account signed into Xcode; set
`APPLE_API_ISSUER_ID` to use the App Store Connect key in `../.zz-signing` instead.
`ZZ_IOS_UPLOAD=0` exports the signed `.ipa` without uploading, and `ZZ_IOS_BUILD_NUMBER` overrides
the build number.

Without a rig, the launcher probes zz Dev socket candidates and skips stale sockets. It does not
start a daemon. To choose a socket and session:

```sh
ZZ_GPUI_ENDPOINT=/tmp/my-zz.sock ZZ_GPUI_SESSION=work just ios run
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

The app does not connect on launch. Until it connects, the workspace shows the connection screen:
saved hosts (most recent first) with Connect and Remove, a destination field (`user@host` or
`user@host:port`), and the app's SSH key. Settings › Hosts shows the same list, plus Disconnect for
the current host. Hosts are saved in `Library/Application Support/zz-gpui/hosts.json`; a
successful connection moves its host to the top. `ZZ_GPUI_ENDPOINT` (or `ZZ_SOCKET`) connects
for that launch only, and opening a `zz://attach/<session>` link connects to the most recent host.
SSH endpoints show host-key confirmation and authentication dialogs. The terminal example shares
the native transport, key translation, and UIKit backend.

## Panes

- Terminal and Agent panes use the web client's working pane entities. Each pane
  keeps its own input focus, viewport, and content state.
- Split right or bottom from the header, then choose Terminal, Agent, or Browser. Close
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
- Browser panes use native WebKit with tabs, an address field, back/forward, and reload.
  SSH connections route page traffic through the host; `localhost` addresses reach
  the host's loopback servers through port forwarding. Cookies and site storage
  persist separately for each host and browser profile on the iPad. They do not
  share the desktop browser's login or live page state.
  The element picker copies context and a cropped screenshot for pasting into an
  Agent pane. Tap the picker, then tap a page element; Cancel exits without copying.
- Existing Editor panes explain that file contents are not yet shared by the daemon.
  Editor does not appear in the new-pane picker.

Command-P, Command-K, or the search button opens the palette that desktop and web also use. Use `:` for commands,
`@` for windows, `%` for panes, and `~` for the connected host. Opening it closes
any daemon prompt or chooser, as on desktop. Daemon choosers, command prompts, menus,
confirmations, and popup terminals use the shared interface. Command output replaces
its pane's content, and display-panes labels keep their tmux styles and alignment.
Notices preserve severity, duration, and explicit clearing; a failed command reports
as `command: error`.

The tree button opens navigation on iPhone.

## Keyboard

Without a hardware keyboard, tapping a terminal, the agent composer, or any text
field raises the on-screen keyboard. For a terminal, a row above it adds Escape, Tab,
Control, Option, the arrows, and a hide button; the backend shows it only for an input
whose Return inserts a line break, so settings fields, prompts, the palette, and host
forms get the plain keyboard, and the phone shell uses its own key row instead.
Control and Option latch for the next key, so Control then C sends Ctrl-C. The
workspace shrinks to the space above the docked keyboard, and a focused settings
field scrolls back into view above it; a focused row of a long settings page keeps
painting while it is off screen, so the keyboard stays up. A floating keyboard
leaves the layout alone. Attaching a hardware
keyboard hides the on-screen one, and detaching it brings it back for the focused
field. Autocorrect, smart punctuation, and capitalization stay off unless a field
asks for them.

The layout moves with the keyboard frame by frame, as Flutter's keyboard inset does: the
backend reads the keyboard's own spring from the keyboard layout guide and evaluates it on
the display link at each frame's display time, so the phone key row, the terminal's bottom
edge, popovers, and a focused settings field slide with the keys on open and close. In the
phone shell the key row takes the bar's place where the bar's top edge was and rides on the
keyboard once the keys pass it. Terminals keep their grid while the keyboard moves (the
prompt row stays above the key row) and report their new size once it stops.

Number fields in Settings open a decimal pad with a Done button above it; a comma from the
pad is typed as a point. Done, or Return in any settings field, commits the value and hides
the keyboard. Return in the Hosts destination field connects and hides it too.

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
background time iPadOS allows. Settings › System › Display can keep the screen
awake while connected. Under thermal pressure or Low Power Mode, frames are capped
at 60 per second. The system text size (Dynamic Type) scales the interface and applies
live. On iPad it multiplies UI zoom and can be turned off in System › Display; on iPhone it
is the only interface scale, capped at the largest non-accessibility size (135%) because it
scales controls and icons as well as text. Reduce Motion and Increase Contrast apply live.
The text system loads the iOS system fonts, so Chinese, Japanese, Korean, Arabic, Hebrew,
and other scripts render. Emoji still show as blank: GPUI's text system only treats Noto
Color Emoji as a color font.

## Frame rate

The display link asks for the screen's top rate with the range Flutter uses (half the maximum, but
at least 60, up to the maximum), so ProMotion iPhones and iPads animate at 120 Hz;
`CADisableMinimumFrameDurationOnPhone` in `Info.plist` lifts the iPhone's 60 Hz cap. The link
pauses three ticks after the last frame request and restarts when the window is invalidated, a
touch begins, or a momentum scroll, key repeat, or keyboard animation is running, so an idle
app takes no vsync callbacks and the display drops to its idle rate. The window opts out of GPUI
presenting the last frame again for a second after fast input, and the pager, sheets, and swipe
back end the touch fling they swallow, so a swipe stops ticking once the page settles.

To measure frames on a paired, unlocked device without touching it:

```sh
just ios bench
```

Bench mode builds and installs the device app like `device` mode, creates a session named
`iphone-bench` on the host's `zz-dev` (two panes with scrollback), launches the app three times
against it (pager swipes, terminal flings, then 25 idle seconds), kills the session, and prints one
`frames total` line per bench and the last idle link reports. `ZZ_BENCH_CYCLES` (default 16),
`ZZ_BENCH_SESSION`, `ZZ_BENCH_CLI`, and `ZZ_BENCH_OUT` (log directory) adjust it. To bench an app
built elsewhere, such as a baseline, run `scripts/ios-bench.sh <udid> <app> <endpoint>`. In the
output, `interval` is the time between new frames in milliseconds (8.33 at 120 Hz), `dropped`
counts vsyncs that passed without a new frame inside a burst, `cpu` is the main-thread time of
each display link tick, and an idle `link:` line with few ticks means the display link is paused.

`ZZ_GPUI_FRAME_LOG=1` prints, per burst of drawn frames, the interval between new frames (p50,
p95, max), missed vsyncs, and the CPU time of each display link tick, plus link ticks and draws
every five seconds. `ZZ_GPUI_BENCH=swipe`, `scroll`, or `drag` (optionally `:count`, default 16)
waits eight seconds after launch and then plays horizontal pager swipes, vertical terminal flings,
or slow 1.5 s terminal drags that hold still before letting go, each followed by a fling back, from
the display link. `GPUI_FRAME_STATS=frames.jsonl` writes gpui's per-frame JSON stats into the app's
`tmp` directory. Device and simulator launches forward these variables and `ZZ_GPUI_SESSION`;
`ZZ_GPUI_CARGO_PROFILE=testflight` builds the device app with line tables for Instruments.

## Settings

Settings uses the shared `zz-ui` form rows, previews, palettes, color pickers,
number fields, and switches. iPad keeps the section navigation beside the page;
iPhone opens on a list of sections; tapping one opens its page, and back returns
to the list. A drag from the left edge also goes back: the page follows the finger with
the section list sliding in underneath, and releasing past half the width or with a
rightward flick completes it. On the list, the same swipe returns to the workspace.

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

The page omits desktop-only options and controls for unsupported pane kinds. iPhone also
leaves out Panes and Status bar (the phone shell shows one pane without gaps or frames and
has its own bottom bar), UI zoom, and Match system text size; its Agent panes switch moves to
System.
Interface and pane preferences are saved atomically in the app container
at `Library/Application Support/zz-gpui/preferences.json` and restored on launch.
Touch controls work without a keyboard; numeric values open a decimal pad and hex colors
open the on-screen keyboard.

## Terminal experiment

The terminal uses `InteractiveClient`, `ClientCore`, and the shared `zz-ui`
painter and Kitty image cache. The daemon owns the PTY and Ghostty parser/encoder.

```sh
ZZ_GPUI_DEMO=terminal just ios run
```

To attach the terminal example to a specific socket and session:

```sh
ZZ_GPUI_DEMO=terminal ZZ_GPUI_ENDPOINT=/tmp/my-zz.sock ZZ_GPUI_SESSION=work just ios run
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
- Terminal scrollback moves with the finger by pixels, and a fling coasts the same way. The app
  keeps up to 2000 rows above the screen (the `history-trickle` option) and draws scrolled rows
  from them without waiting for the daemon. The daemon's view trails the gesture by up to a
  screen, more rows are fetched as the view nears the oldest kept row, and once the view stays on
  one row for 120 ms the daemon's view is moved there. New output does not move a scrolled-back
  view; scrolling back to the bottom follows output again. This is the desktop's trackpad
  scrolling (`LocalScrollState` and `HistoryPacer` in `zz-client`). `less`, `vim`, and other
  mouse-tracking or alternate-screen programs still get wheel events.

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

iPhone Simulator checks on 2026-10-06 covered pinching a terminal in the phone shell (one grid
report per pinch, after release; a two-finger sideways pan neither pages nor scrolls), live
system text size changes from XS to AX5, and the phone's Settings list. iPad Simulator kept
Panes, Status bar, UI zoom, and Match system text size. Screen recordings on iPhone 17 Pro
Simulator then followed the key row, the terminal bottom, an Interface number field, and the
color picker frame by frame while the keyboard opened and closed (one terminal resize per
keyboard move), and covered the decimal pad, Done, and Return in the Hosts field.

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
just web build
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
