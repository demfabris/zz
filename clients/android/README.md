# zz for Android

The Android client is the iPhone client on Android: the same `clients/app` (`zz-app`) shell, phone
shell, settings, overview, key row, terminal, agent and browser panes, connection screen and
saved hosts. This crate (`zz-android`) is only the entry point, the Java glue and the APK files.
The backend is `zz_gpui_platform::android` in `crates/zz-gpui-platform/src/android`.

Phones (smallest width under 600 dp) get the phone shell; tablets get the sidebar layout, as on
iPad. The choice follows the display as it changes, so a foldable switches layouts when it opens
or closes. Everything the iOS README describes for the phone shell, sheets, settings, pinch, scrollback,
selection, the element picker and reconnects holds here; this page covers what is Android's own.

## Run

From the repository root, `just android` lists the actions.

```sh
just android setup
just android emulator
just android rig
just android run
```

`setup` installs the Android command-line tools into `$ANDROID_HOME` (default `~/Android/Sdk`),
then platform-tools, build-tools 36.1.0, the API 36 platform, NDK 29.0.14206865, the emulator and
the API 36 Google APIs x86_64 image, adds the `x86_64-linux-android` and `aarch64-linux-android`
Rust targets, and creates the `zz-phone` AVD (Pixel 8 profile). It needs a JDK 17 or newer on
`PATH`, or one unpacked at `~/.local/share/jdk/21`.

`emulator` boots `zz-phone` when no emulator is attached. It uses the host GPU, which needs a
display for EGL; without `DISPLAY` or `WAYLAND_DISPLAY` it falls back to SwiftShader, and
`ZZ_ANDROID_HEADLESS=1` hides the window. Boot also sets `show_ime_with_hard_keyboard` so Gboard
shows next to the emulator's virtual hardware keyboard.

`rig` is `just ios rig`: a throwaway daemon at `/tmp/zzios-<checkout folder>.sock` with a session
named `phone`. `run` builds a debug x86_64 APK, installs it, forwards the rig socket into the
emulator with `adb reverse localabstract:zz-rig localfilesystem:<socket>`, and launches with
`ZZ_GPUI_ENDPOINT=@zz-rig`. On Android, a local endpoint that starts with `@` is an abstract unix
socket.

```sh
just android device
just android logs
```

`device` builds a release arm64 APK, installs it on the attached phone (`adb devices`, USB or
wireless debugging) and launches it; pick one of several with `ZZ_ANDROID_DEVICE`. `logs` follows
the app's logcat; Rust logging, stdout, stderr and panics all go there under the `zz` tag
(`RUST_LOG` sets the level).

The launch forwards `ZZ_GPUI_ENDPOINT`, `ZZ_GPUI_SESSION`, `ZZ_GPUI_FRAME_LOG`, `ZZ_GPUI_BENCH`,
`ZZ_GPUI_GLASS`, `GPUI_FRAME_STATS` and `RUST_LOG` as intent extras; the activity turns every
`ZZ_*` and `GPUI_*` string extra into an environment variable before the app starts. Without an
endpoint the app opens the connection screen, as on iOS.

Dev builds (`ZZ_DEV_BUILD=1`, the default) install as `dev.zz.android.dev`, named **zz Dev**, with
the `zz-dev://` scheme and the dev icon. `ZZ_DEV_BUILD=0` builds `dev.zz.android` (**zz**,
`zz://`). Both are signed with `~/.android/debug.keystore` unless `ZZ_ANDROID_KEYSTORE` points at
another key. The APK is built without Gradle: `javac` and `d8` compile the Java glue, `aapt2`
links the manifest and resources, and `zipalign -P 16` keeps the native library on 16 KiB pages.
Build products land in `target/android/<abi>/zz.apk`.

## How it fits together

`ZzActivity` loads `libzz_android.so`, whose `JNI_OnLoad` registers the natives. The activity
hosts `ZzView`, a `SurfaceView` that forwards its surface, touches, mouse and keys, and answers
the input method through `ZzInputConnection`. GPUI's main thread is the activity's UI thread, as
it is UIKit's main thread on iOS: main-thread work runs from an eventfd on the UI thread's
`ALooper`, frames are paced by the `Choreographer` (stopped three idle frames after the last
request), and the app starts once the first surface exists, through
`Application::run_embedded`.

Rendering is zz-gpui's wgpu renderer on Vulkan, with GLES as the fallback. When the app goes to
the background the surface goes away; the renderer keeps its device and atlas and picks up the
new surface on return. Debug labels stay off because the emulator's Vulkan driver crashes naming
objects. Text uses the bundled Inter and Lilex plus every font in `/system/fonts`, memory-mapped,
so CJK, Arabic, Hebrew and color emoji render.

Native views laid over the GPUI surface (browser tabs, the tablet key row) move by layout, never
by translation: SurfaceFlinger crops the window to its opaque region, which Android only
recomputes on a layout pass, so a translated view disappears.

The window draws edge to edge. Safe area insets come from the system bars and display cutout, and
the keyboard's inset comes frame by frame from `WindowInsetsAnimation`, with the resting height
as `target_visual_viewport_bounds`, the way the iOS backend follows the keyboard's spring.

## Android differences

- **Back.** The system back button and gesture close what is open, in order: a bottom sheet, a
  dialog, menu, prompt or daemon overlay (by sending Escape to it), a Settings page, Settings,
  then the overview. With nothing open, back leaves the app. The left-edge swipe back of iPhone
  belongs to the system on Android.
- **Text menu.** A long-press selection in a terminal ends in Android's floating text toolbar with
  Copy, Paste (when the clipboard has something) and Select all.
- **Keyboard.** The input method gets a multi-line, no-suggestions editor for terminals (so Gboard
  types characters straight through) and the field's own type, capitalization, correction and
  action for everything else; number fields get the number pad with a Done action. Android
  decides whether the soft keyboard shows while a hardware keyboard is attached, following the
  user's "Use on-screen keyboard" setting.
- **Key row on tablets.** The phone shell draws its own key row. On tablets a native row rides on
  top of the soft keyboard while a terminal or another Return field has it, as on iPad: Esc, Tab,
  Ctrl and Alt (each latches onto the next key), the arrows, and a key that hides the keyboard. It
  follows the system's dark mode like the keyboard does, and its height counts as part of the
  keyboard's inset, so GPUI lays out above both.
- **ssh key.** The app's Ed25519 key lives in its private files directory
  (`files/zz/ssh/id_ed25519`, comment `zz-android`), next to `known_hosts`. Settings › Hosts copies
  the public half.
- **Browser.** Each tab is an `android.webkit.WebView` laid over the GPUI surface, clipped to the
  pane's lower corners with a hole where the glass pill floats. As on iOS, an ssh host's pages go
  through the host (a SOCKS override from androidx.webkit's `ProxyController`), `localhost`
  addresses reach the host's loopback servers through port forwards, and each host and browser
  profile keeps its own cookies and storage (androidx.webkit's `ProfileStore`). The proxy is the
  whole app's, which matches having one host connection at a time. WebView builds without these
  features load pages over the phone's network and share one store. Plain http is allowed, so
  local dev servers open. The build fetches androidx.webkit 1.14.0 once, checked against a
  pinned SHA-256, into `target/android/deps`.
- **Files.** Preferences and saved hosts are in `files/zz-gpui/`. Images copied from the app go
  through `ZzFileProvider` so other apps can paste them.
- **Links.** `zz-dev://attach/<session>` (`zz://` for release builds) switches the running app to
  that session or attaches to it on the next connect.
- **Accessibility.** TalkBack and other services read GPUI's AccessKit tree through
  `accesskit_android`, which attaches an accessibility delegate to the GPUI view.
- **Screen.** Settings › System › Display can keep the screen on while connected. The window asks
  for the display's top refresh rate. Font size, dark mode, removed animations and high contrast
  text from the system settings apply live.

## Verification

Checked on the `zz-phone` emulator (Android 16, API 36, host GPU through gfxstream) on
2026-10-22:

- Connection screen, saved hosts, the rig through `adb reverse`, automatic reconnect after the
  tunnel dropped, and resume after leaving the app (the surface comes back and the session
  repaints).
- ssh through russh to a user-level `sshd` on the host (`ssh://user@10.0.2.2:2222/<socket>`):
  key generation and copy, the host key prompt, `known_hosts`, key authentication and the
  `zz-dev proxy` session.
- Phone shell: pages, the pill pager, the overview with live cards, new panes, Settings with a
  live theme change that persisted across a relaunch, and back through every level.
- Terminal: Gboard typing through the input connection, injected hardware keys, the key row,
  scrollback drags and flings with rows under the header, pinch from 100% to 300% and back with
  one grid report after release, long-press selection, Copy and Paste.
- Browser: a pane over ssh loading `http://localhost:8765` from the host, links, back, a
  `target=_blank` link opening a tab, the hold-and-drag element picker copying an element, and
  `https://example.com` arriving at the host's `sshd` as a `direct-tcpip` channel.
- Agent pane: a prompt sent to a small ACP agent (`agent-command`), its streamed reply with
  markdown and a highlighted code block.
- Accessibility: `uiautomator dump` lists the overview's cards and labels with their bounds,
  TalkBack's focus lands on GPUI elements (a pane header's title), and the app keeps running
  after TalkBack turns off.
- `zz-dev://attach/<session>` while running.
- Landscape (the phone shell reflows and keeps clear of the camera cutout) and a live font scale
  change from the system settings.
- Tablet layout at 720 dp (`adb shell wm density 240`): the sidebar, Settings with Panes, a number
  field getting the decimal number pad whose Done saves the value and hides the keyboard, back
  closing Settings, and the switch between phone shell and sidebar while running. The key row:
  a latched Ctrl with Gboard's `u` clearing the shell line, Alt-B, Tab completion, Up recalling
  history, hide, dark and light. A web page's input keeps the keyboard to itself without the row,
  and picking a terminal in the sidebar afterwards hands focus back to GPUI.
- A mouse (`adb shell input mouse`): wheel scrolling through scrollback, three lines a notch, and
  a drag selecting terminal text.
- The release arm64 APK (fat LTO, 39 MB) running under the emulator's arm64 translation, over
  ssh.

Not yet checked: a physical phone or tablet, an arm64 build on arm64 hardware, TalkBack's spoken
output and gestures beyond focus, IME composition for CJK input methods, trackpad gestures, and
the backdrop tint under the pill over pages.

```sh
cargo test -p zz-gpui-platform --all-features --lib android
cargo clippy -p zz-gpui-platform -p zz-app -p zz-android -p zz-daemon-client --target x86_64-linux-android --lib -- -D warnings
just android build
```
