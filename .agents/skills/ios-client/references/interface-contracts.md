# Interface contracts

Use this reference when changing the compact phone shell, regular iPad shell, terminal surfaces,
Agent panes, settings, reconnect presentation, or accessibility.

## Adaptive shell

- `ContentView.workspace` selects the shell by horizontal size class. One app can move between
  compact and regular width while running, so device-name branches are wrong.
- `ZZMobileApp` owns `ZZStore` and `ZZClientSettings`. Appearance changes native chrome; terminal
  colors continue to come from the daemon viewport.
- Keep the Settings sheet outside the size-class branch. Presenting it must not replace or resize the
  iPad detail hierarchy.

## Compact phone shell

- `PaneOverview` represents the attached session's active window as a two-column card grid.
  `ZZSession.panes` means active-window panes; `allPanes` spans the full session.
- A terminal card uses a passive retained preview. An Agent card uses retained typed Agent state.
  Browser, Editor, and picker cards remain honest placeholders until a native representation exists.
- Keep pane opening and pane closing as separate buttons. Closing requires destructive confirmation
  because `kill-pane` stops the process. Interactive targets remain at least 44 points.
- `SessionRail` has three independent pieces: create session, paged session selector, and actions.
  The selected page expresses desired state; `ZZStore.selectSession` requests the daemon attachment,
  and the next snapshot confirms it.
- Session creation stays single-flight until a snapshot contains a new session ID. A successful
  command write does not prove the session exists.
- `FullscreenPane` shows one interactive pane. Preserve the overview control, full-width pane pager
  or shortcut strip, and keyboard-strip toggle as separate controls.
- Compose uses a native multiline editor and sends the completed Unicode string. This preserves IME,
  paste, and dictation.
- The compact New Pane action creates a terminal. The regular iPad menu may offer Terminal and Agent.

## Regular iPad workspace

- `IPadWorkspace` uses a balanced `NavigationSplitView`. Preserve the sidebar hierarchy as session,
  every window in that session, then every pane in that window. Do not flatten it.
- A row in another session or inactive window routes through `ZZNavigationTarget`. The store issues
  attach/window/pane commands and waits for reduced snapshots; a local selection highlight cannot
  replace that convergence.
- The attached session's active pane is the visual fallback until the user makes an explicit pane
  selection.
- Sidebar rows remain full-width 44-point buttons. Window chevrons stay trailing, pane labels stay
  optically centered, and only the selected pane receives the selection capsule and trait.
- `IPadPaneWorkspace` mounts panes whose snapshot layout is present. Zoom-hidden siblings stay in the
  sidebar but do not mount without rectangles.
- `IPadPaneSplitLayout` multiplies the normalized rectangles supplied by `zz-client` by current detail
  bounds. Keep split-tree solving out of Swift.
- Every visible terminal tile in the regular split workspace may stay live. `ZZStore.terminalInput`
  still owns at most one pane, and tapping a tile transfers both UIKit first responder and daemon
  pane focus through the store.
- `IPadStatusBar` is snapshot-backed and bounded around the active window. Do not imitate custom
  daemon-expanded status text until the FFI exports that payload.

## Panorama

- Panorama expresses the mux hierarchy directly: one horizontally arranged column per session, the
  session name, its vertically scrolling windows, then each window's pane topology.
- Preserve daemon-provided normalized rectangles inside each window card. A Swift-only grid lies
  about the session layout.
- Entering Panorama releases terminal input through `ZZStore.showOverview()`.
- Terminal previews use `TerminalSurface(interactive: false, preview: true)`. They do not accept
  input, become first responder, intercept the card button, or report a PTY resize.
- `live` and `interactive` are separate. A live preview can receive frames while the app keeps one
  first responder and one input owner. Setting both `interactive: true` and `preview: true` makes
  miniature UIKit bounds eligible for resize reporting and can shrink the real PTY.
- While Panorama is open, the app retains a pane-local frame slot for every terminal across every
  window of the attached session. The v87 preview stream carries inactive-window frames separately
  from foreground geometry and input. Other sessions keep pane-kind placeholders until navigation
  attaches them. Do not create hidden interactive terminals to fill those cards.
- Live thumbnails for multiple sessions at once would require extending the daemon/client preview
  subscription beyond the attached session. Mounting more Swift views cannot obtain frames the
  daemon never sends.
- Panorama owns horizontal session paging and vertical window paging. Any future interactive
  thumbnail mode must define when terminal scroll, selection, and pinch gestures take control.
- The temporary full-detail transition snapshot is passive and disables hit testing.
- Wait for the first real window snapshot before starting the entrance transition.
- Entrance and exit transform one fixed-size passive capture of the selected window instead of
  resizing live pane views. Lock the destination card rectangle before movement starts. During exit,
  restore the detail navigation bar before the reverse transform and mount the live workspace only
  after it completes. Changing that order recreates the visible navigation-bar jump.
- Preserve Reduce Motion with target alignment and a short crossfade without scale or blur movement.

## Terminal rendering and input

- `TerminalFrame` owns the acquired FFI viewport. Its cell, style, grapheme, color, and cursor buffers
  are valid only while that handle lives.
- `TerminalGridView` draws the render-ready plane. Preserve grapheme lookup, wide-cell spacer
  suppression, row damage, decorations, faint and invisible text, ANSI blinking text, and daemon
  cursor shape and color.
- A preview scales an immutable viewport to fit. An interactive surface uses the selected logical
  font and reports geometry from its actual UIKit bounds.
- Direct text uses `UIKeyInput`; hardware keys use raw press, repeat, and release events. Do not fold
  either path into hardcoded Swift shortcuts.
- Shift, Control, and Alt are one-shot after one tap, lock after a double tap, and clear when the
  locked control is tapped again. Current reset points are scene transitions, session or pane
  navigation, overview entry, connection teardown, and Prefix dispatch. `releaseTerminalInput()` by
  itself does not reset modifiers.
- Touch pan sends semantic line scrolling. Long-press drag sends semantic selection coordinates.
  Copy waits for the typed clipboard event and writes that value to `UIPasteboard`.
- Pinch zoom is a per-pane integer offset from the persisted base size, clamped to 9 through 23
  points. Report every crossed step, resize from the new metrics, and preserve selection haptics.

## Focus and keyboard geometry

- `TerminalInputState` is the single input owner. Acquisition advances an activation token.
  `TerminalGridView.reconcileInput` waits one main-actor turn and checks identity, interactivity,
  mount state, and scene activity before becoming first responder.
- Do not call `becomeFirstResponder` from SwiftUI lifecycle callbacks. A stale mounted surface can
  otherwise reclaim the keyboard after navigation.
- Switching input owners unfocuses the old daemon pane and restores its stable geometry before it
  focuses the new pane.
- Client-window focus and terminal-pane focus are distinct. `ZZ_EVENT_ATTACHED` permits the client
  focus signal; scene transitions also update the current pane focus.
- Derive terminal rows and columns from UIKit bounds and cell metrics. Do not subtract keyboard
  heights in Swift.
- Treat a docked keyboard's smaller bounds as transient. Preserve the latest keyboard-hidden grid as
  the reconnect baseline, share keyboard visibility across surfaces, and restore stable geometry
  when input leaves. Floating keyboards stay overlays.
- Backgrounding keeps the FFI client, reduced core, and retained frames alive while it releases
  focus. Reconnecting uses the stable keyboard-hidden geometry.
- Cursor-blink preference can steady a cursor that requests blinking. It must not disable ANSI
  blinking text.

## Agent panes, settings, and reconnects

- Render Agent status, activity, permission, error, git summary, queue count, and composer from the
  retained typed state. Do not invent transcript bubbles or streaming deltas.
- Keep drafts independent per pane. Composer behavior follows daemon phase: send while ready; queue
  up to four prompts while running or awaiting permission; use an empty action to stop in either
  phase; and disable actions during startup or failure.
- Permission choices are typed in Rust. Render approvals and rejections from that metadata.
- Local Agent notifications come from live attention edges and route by stable session and pane IDs.
  They are not push delivery while the app is suspended or terminated.
- Agent creation requires the connected daemon to enable `experimental-agent-pane`.
- Preserve the known URL routes and App Shortcuts through the shared exact-pane navigation path.
  Reject unknown routes instead of interpreting arbitrary URLs or commands.
- Persist native appearance, terminal font family, 9 through 23 point base size, cursor blinking, and
  the iPad home-indicator extension. Per-pane zoom stays in memory, survives automatic reconnect,
  and is not persisted to `UserDefaults`.
- With no retained sessions, reconnect uses a full page. With retained sessions, keep the frozen
  workspace and show the banner. Both surfaces display the last transport error through the next
  automatic attempt.

## Accessibility and visual proof

- Keep semantic buttons around passive previews and explicit previous/next actions on paged rails.
- Preserve labels, values, selected traits, stable identifiers, and 44-point targets.
- The terminal grid does not yet expose readable cell text to VoiceOver. Do not claim that support.
- Unit tests cover policies such as endpoint normalization, geometry, backoff, input ownership,
  modifiers, Agent drafts, settings, and deep links. They do not cover size-class switching, sidebar
  hierarchy, normalized placement, Panorama transforms, first-responder timing, keyboard frames, or
  terminal drawing.
- Verify affected interface behavior on the relevant simulator. Use a physical iPhone or iPad for
  keyboard, safe-area, focus, networking, and interaction claims that depend on real hardware.
