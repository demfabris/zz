# Interface contracts

Use this reference when changing the compact phone shell, regular iPad shell, terminal surfaces,
Agent panes, settings, reconnect presentation, or accessibility.

## Adaptive shell

- `ContentView.workspace` selects the shell by horizontal size class. One app can move between
  compact and regular width while running, so device-name branches are wrong.
- `ZZMobileApp` owns `ZZStore` and `ZZClientSettings`. The device owns its presentation config.
  The Rust FFI applies the local terminal theme to acquired viewport cells before UIKit draws them;
  it does not change other clients' terminal palettes.
- Keep Settings sheet presentation outside the size-class branch. Both widths use one native form
  with Appearance (System, Light, Dark), Font, Text Size, and Color Theme. Opening Settings
  releases terminal input and keeps the pane hierarchy mounted underneath.
- Use system surfaces, typography, and tint for app chrome. Desktop chrome presets, overrides, and
  contrast do not style the mobile interface. Terminal themes still apply to terminal contents.

## Compact phone shell

- `PaneOverview` represents the attached session's active window as a two-column card grid.
  `ZZSession.panes` means active-window panes; `allPanes` spans the full session.
- A terminal card uses a passive retained preview. An Agent card uses retained typed Agent state.
  Browser panes render locally with WebKit through SSH, using SOCKS for remote addresses and
  same-port local forwards for localhost; Editor keeps its
  desktop-only placeholder. Picker previews stay passive; opening a
  picker shows `PaneKindPicker` with Terminal and Agent choices.
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
- The compact New Pane action creates a terminal. The regular iPad menu offers Terminal, Agent,
  and Choose Pane Type. Picker conversion targets the existing pane with `select-pane-kind -t`.
  Agent creation requires the host to enable `experimental-agent-pane`.

## Regular iPad workspace

- `IPadWorkspace` uses `NavigationSplitView` with both navigation bars hidden. Preserve the sidebar
  hierarchy as session, every window, then every pane. Keep the sidebar visible at regular widths;
  omit collapse controls and floating toolbars. Its footer provides Panorama, Add, Settings, and More.
  Add contains New Window; More contains New Session and Help.
- A row in another session or inactive window routes through `ZZNavigationTarget`. The store issues
  attach/window/pane commands and waits for reduced snapshots; a local selection highlight cannot
  replace that convergence.
- The attached session's active pane is the visual fallback until the user makes an explicit pane
  selection.
- Sidebar rows use native list spacing and `List(selection:)` highlighting. Window chevrons stay
  trailing and pane labels sit beside their icons.
- `IPadPaneWorkspace` mounts panes whose snapshot layout is present. Zoom-hidden siblings stay in the
  sidebar but do not mount without rectangles.
- `IPadPaneSplitLayout` multiplies the normalized rectangles supplied by `zz-client` by current detail
  bounds. Keep split-tree solving out of Swift.
- Every visible terminal tile in the regular split workspace may stay live. `ZZStore.terminalInput`
  still owns at most one pane, and tapping a tile transfers both UIKit first responder and daemon
  pane focus through the store.
- Connection status belongs below the sidebar list. Pane rows show bells and Agent attention without
  visibility preferences. Session and window navigation live in the sidebar.
- `PaneActionsMenu` sends daemon commands for picker splits, zoom, named layouts, and directional
  cell-based resizing. Closing requires confirmation; a successful write still needs snapshot
  convergence.
- Tiles use fixed 6-point gaps, 14-point corners, opaque surfaces, and 1-point outlines. The selected
  pane has a subtle accent outline and a fixed 5% elliptical accent glow along its edges. No inactive
  dimming. Extend the detail area behind the
  home indicator by ignoring only the bottom container safe area; preserve keyboard avoidance.
- The sidebar footer remains reachable during Panorama and stays within the safe area.
- Pane headers use flat, dimmed Split Down, Split Right, Pane Actions, and Close controls. Keep
  44-point targets and the menu fallback for narrow tiles. Close requires confirmation.
- Terminal headers and padding match the acquired frame's opaque background. Header controls use
  its foreground color. Other headers match their native pane surfaces. Observe frames locally; do
  not publish their colors through the workspace store. Paint the padding outside the grid only.

## Panorama

- Clip each preview pane and its inset outline to the same rounded shape. Leave the window backing
  transparent so square black corners cannot surround rounded panes.

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
  mount the live workspace only after it completes. Neither transition changes a navigation bar.
- Preserve Reduce Motion with target alignment and a short crossfade without scale or blur movement.


## Terminal rendering and input

- `TerminalFrame` owns the acquired FFI viewport. Its cell, style, grapheme, color, and cursor buffers
  are valid only while that handle lives.
- `TerminalGridView` draws the render-ready plane. Preserve grapheme lookup, wide-cell spacer
  suppression, row damage, decorations, faint and invisible text, ANSI blinking text, and daemon
  cursor shape and color. The local Rust appearance conversion may restyle defaults and indexed
  colors; Swift must not rebuild terminal colors or selection from glyphs.
- A preview scales an immutable viewport to fit. An interactive surface uses the selected logical
  font and reports geometry from its actual UIKit bounds.
- Direct text uses `UIKeyInput`; hardware keys use raw press, repeat, and release events. Do not fold
  either path into hardcoded Swift shortcuts.
- The Prefix control uses `switch-client -T prefix` to enter the daemon's key table. `send-prefix`
  writes a prefix character into the PTY and is for nested multiplexers.
- `TmuxCopyBar` renders daemon copy-mode state and sends semantic selection/search actions and
  `send-keys -X` movement. Preserve raw hardware input for daemon vi/emacs tables.
- `TmuxOverlay` renders daemon prompts, confirmations, tree and buffer choosers, pane indicators,
  menus, and command output through the JSON FFI. Send typed actions back to Rust and prevent
  underlying terminals from consuming overlay interaction.
- `TmuxBindingsView` lists the daemon-published tables, including custom bindings and repeat flags.
- Previous and next pane controls use daemon-backed pane selection, just like sidebar navigation.
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
- Preserve terminal-program cursor blink requests and independent ANSI blinking text.

## Agent panes, settings, and reconnects

- Render Agent status, permission, error, git summary, queue count, and composer from the retained
  typed state, and the conversation from the daemon's journal batches. Reduce those batches with the
  published cursor rules (per-pane cursor, idempotent overlap, gap and lane-overflow replay,
  restoring-reset jumps); never synthesize transcript the daemon did not send.
- Each pane's transcript lives in its own `ZZAgentThreadSlot`, mirroring `TerminalFrameSlot`. A
  streamed batch must not invalidate views for other panes, so keep it off `ZZStore`'s `@Published`
  surface.
- Agent prose renders as full-width markdown, not a chat bubble: only the user's own turns get one.
  Block structure comes from the pinned `swift-markdown` package; walk its tree into the app's block
  model rather than scanning lines, which cannot carry fence lengths, nesting, or GFM tables. Inline
  syntax stays with `AttributedString`, and inline source must be read from a detached copy of a
  node's inline children because `format()` inherits quote and list prefixes in place.
- Keep `Markdown` imported only where the block model is built. It exports names that collide with
  SwiftUI's, `Table` among them, so view code consumes the app's own types.
- Tool rows carry the ACP `kind` icon, `title`, and the first `locations` entry as `path:line`. ACP
  replaces `locations` and `content` when present and leaves them alone when absent; preserve that
  merge rule.
- The transcript follows new output only while the reader is already at the end. Submitting a prompt
  always returns to the end because the reader asked for it.
- The composer matches the desktop key contract in `crates/zz-ui/src/widget/input/state.rs`: Return
  submits, Shift-Return inserts a newline, Command-Return submits. `TextField(axis: .vertical)`
  cannot express it, so the field is UIKit-backed.
- Keep drafts independent per pane. Composer behavior follows daemon phase: send while ready; queue
  up to four prompts while running or awaiting permission; use an empty action to stop in either
  phase; and disable actions during startup or failure.
- Permission choices are typed in Rust. Render approvals and rejections from that metadata.
- Local Agent notifications come from live attention edges and route by stable session and pane IDs.
  They are not push delivery while the app is suspended or terminated.
- Agent creation requires the connected daemon to enable `experimental-agent-pane`.
- Preserve the known URL routes and App Shortcuts through the shared exact-pane navigation path.
  Reject unknown routes instead of interpreting arbitrary URLs or commands.
- Persist terminal font and 9 through 23 point base size; retain the device-local terminal theme.
  Per-pane zoom stays in memory and survives automatic reconnect. Persist the System/Light/Dark
  appearance choice, defaulting to System, and honor Reduce Motion. Use fixed 8-point terminal padding and opaque backgrounds.
- `ZZSharedSettings` owns the shared Rust settings model and bundled theme catalog. Before local
  appearance reaches FFI frames, migrate terminal config to only font-family, font-size, and theme.
  If migration fails, report the error and render daemon viewports without the old local overrides.
- Do not apply legacy local mux.conf on attachment or edits. The host owns multiplexer configuration;
  keep Keyboard Bindings as a read-only reference under More > Help and in terminal pane actions.
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
- `Tests/UI/IPadAcceptanceTests.swift` uses `ZZ_IOS_UI_TEST_SOCKET` to exercise the connected iPad
  settings, picker, resizing, copy/search controls, and key tables. It skips without the isolated
  socket or on iPhone. Read its assertions before treating a pass as coverage for another behavior.
- Verify affected interface behavior on the relevant simulator. Use a physical iPhone or iPad for
  keyboard, safe-area, focus, networking, and interaction claims that depend on real hardware.
