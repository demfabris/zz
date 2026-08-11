---
type: Subsystem
title: Terminal interaction (input, selection, paste, words)
description: The renderer-neutral pointer, keyboard, word-boundary, and paste layer that turns client gestures into libghostty encoding, native selection, and copy-mode actions, plus the client-side local scroll overlay.
resource: crates/zz-terminal/src/interaction.rs
tags: [interaction, input, mouse, selection, paste, copy-mode, keyboard]
timestamp: 2026-08-10T00:00:00Z
---

# Overview

The interaction subsystem is the renderer-neutral vocabulary of everything a user does to a terminal
pane: pointer gestures, keyboard input, word/line selection, and paste. It spans four
[`zz-terminal`](/crates/zz-terminal.md) modules: `interaction.rs` (action + pointer types),
`input.rs` (packed key records), `word.rs` (boundary classifier), and `paste.rs` (tmux paste-buffer
transform). None of these types are GPUI-specific; the client sends them and the worker thread applies
them by driving [`libghostty-vt`](/terminal/libghostty-vt.md) encoders, selection, and copy-mode state.
The routing logic itself (`route_mouse_input`, `wheel_route`, `hover_link_at`, `plain_uri_at`) lives in
`session.rs`.

# Action model (`interaction.rs`)

`TerminalViewAction` is the single enum a client applies via `TerminalSession::view_action`:

| Group | Variants |
| --- | --- |
| Viewport | `ScrollLines`, `ScrollPages`, `ScrollTop`, `ScrollBottom`, `ScrollToFraction`, `ScrollToOffset`, `ScrollWheel` |
| Selection | `SelectionPress`, `SelectionDrag`, `SelectionAutoscroll`, `SelectionRelease`, `SelectAll`, `ClearSelection` |
| Mouse / links | `Mouse(TerminalMouseInput)`, `ClearLinkHover` |
| History / focus | `ClearHistory`, `Focus(bool)`, `Paste(String)` |
| Copy mode | `EnterCopyMode`, `CopyMode(CopyModeAction)`, `CopySelection { request_id, target }` |
| Search | `SearchBegin`, `SearchUpdate`, `SearchNext`, `SearchPrevious`, `SearchClose` |

`TerminalMouseInput` packs phase (`Press`/`Release`/`Motion`), button (incl. `ScrollUp`/`ScrollDown`),
modifiers, and a `force_selection` flag into a single `u16 routing` field alongside pixel coordinates and
a `PointerCellEvent` (column, row, `click_count`, `rectangle`). It is 32 bytes with a Serialize/Deserialize
wire form. `CopyModeAction` carries zz's native tmux copy-mode set (movement, selection, rectangle,
search, marks, `Jump`, `CopySelection`, and `CopyEndOfLine`), with cold copy payloads boxed so
navigation actions stay small. Pinned tmux's position-label and live-refresh toggles are deliberately
outside the model; the former is native chrome and the latter conflicts with a frozen revision.
`CopyModeCopy` carries independent clipboard / paste-buffer / pipe targets;
`SearchQuery` carries `mode` (Literal/Regex), `case` (Smart/Sensitive/Insensitive), and `direction`.

# Keyboard input (`input.rs`)

`KeyInput` is the renderer-independent record fed to libghostty's key encoder: `action`
(`Press`/`Repeat`/`Release`), `key` (`KeyCode`: characters, named keys, `Function(u8)`, `Unidentified`),
`modifiers`, optional committed `text`, and `unshifted_codepoint`. Raw keys and committed text are kept
separate so IME and non-US layouts stay correct. `Modifiers` packs shift/control/alt/platform into one
byte with reserved-bit validation on deserialize; `KeyInput` is 32 bytes on 64-bit. The worker calls
`encode_key` → `key::Encoder` (Kitty-keyboard aware) to produce PTY bytes.

The GPUI adapter derives both printable-text fallback and `unshifted_codepoint` from the mapped
`KeyCode::Character` rather than from GPUI's key-name string. Space is why: GPUI names the
physical key `space`, while the terminal mapping is `Character(' ')`. Press and release records must
therefore both carry `unshifted_codepoint: Some(' ')`. When it is absent under Kitty event reporting
(for example, Codex pushes `CSI > 7 u`), libghostty cannot identify the release and falls back to the
event's literal text, turning one Space press into two spaces.

# Word boundaries (`word.rs`)

`WordSeparators` is the precompiled classifier shared by live double-click selection and copy-mode word
motion. `DEFAULT_WORD_SEPARATORS` matches tmux (non-alphanumeric printable ASCII, **underscore excluded**).
Construction sorts and dedups codepoints, builds a two-word `u64` ASCII bitmask for O(1) `contains_separator`,
and precomputes a `boundary_codepoints` list (separators + a fixed Unicode-whitespace set incl. NUL/tab/space)
handed directly to libghostty selection. The session-wide separator set is swapped via
`TerminalSession::set_word_separators`, which reinstalls an active word selection without touching the PTY.

# Paste (`paste.rs`)

`prepare_paste_buffer(data, separator, literal)` implements tmux `paste-buffer` rules for daemon-owned
buffers: newlines become `separator`; unless `literal`, valid UTF-8 is preserved and unsafe/invalid bytes
are visibly encoded per tmux's `VIS_SAFE | VIS_NOSLASH` policy. Output length is computed before allocation
and bounded (`SeparatorTooLarge`, `OutputTooLarge`). Prepared bytes reach the terminal via
`TerminalSession::paste_prepared_bytes`, which lets the worker emit bracketed-paste markers only when the
application has enabled bracketed paste; bytes are not forced through UTF-8 again.

# Routing (in `session.rs`)

Pointer and wheel events are routed by precedence: native mux chrome/overlays, then active copy/view mode,
then a forced local-selection modifier, then terminal application mouse reporting, then live local
selection/scrollback. `wheel_route` chooses `ApplicationMouse`, `AlternateScroll` (translate to arrow keys
when alternate scroll mode is on), or `Viewport`. **No pointer gesture enters copy mode.** A `Viewport` wheel
only moves the live viewport offset, in either direction, and a drag paints the live selection overlay for
the whole gesture; the scrollbar, not a mode pill, says the view is scrolled back. Typing snaps the viewport
back to the bottom (`prepare_live_input`). Copy mode is entered only by the `copy-mode` command, which the
daemon dispatches together with the client's key-table switch — see [copy mode](/tmux/copy-mode.md).
The client filters ahead of all of it: `TerminalView` forwards a button-bearing pointer event only for a
button whose press it forwarded itself, so a motion or release belonging to a press taken elsewhere — a
chrome strip moving the window, whose queued moves land in panes while the window lags the cursor — never
reaches the daemon and cannot extend a selection.
Links: `hover_link_at`
resolves OSC 8 metadata (authoritative) and `plain_uri_at` recognizes bounded plain-text
`http`/`https`/`mailto`/`file`/`ssh` URIs on the current logical row; activation emits the `OpenUri` event and
never injects text into the PTY. The
[`zz` client](/crates/zz.md) normalizes browser-supported web URLs and queues navigation for the
topologically nearest browser pane in the source pane's mux window. It preserves the platform URL
opener for unsupported schemes or when that window has no browser pane.

# Local scroll (client side)

Scrollbar drags and desktop page navigation do not always have to cost a round trip. When the pane is in
`Live` mode, mouse tracking is off, the scrollbar has room to move, and the pane's client-side `HistoryRing`
holds rows,
`TerminalView` (`crates/zz/src/terminal/view.rs`) records a `LocalScroll { target_offset, started }`
and the next frame paints from the
ring: rows above the live viewport come out of the ring, rows still inside it come from the server
frame, rows the ring cannot cover yet paint as a dim shimmer, and the scrollbar thumb is drawn from
the local target. Cursor, selection, and overlay spans are projected onto whatever slice of the live
viewport remains visible. A target older than the ring's coverage or newer than the server's offset
skips the overlay and takes the round trip instead, and a target near the cold edge of the ring
triggers a `HistoryRequest` prefetch.

An upward wheel still bypasses this overlay and asks the daemon, which owns rows the ring may not cover.
If a local scrollbar position is still pending, the client sends its `ScrollToOffset` first and cancels the
debounced duplicate; the ordered wheel action then lands on exactly the viewport the user was looking at.

The daemon hears about it once. `ScrollToOffset(target)` goes out after a 120 ms
`LOCAL_SCROLL_DEBOUNCE`, so one local navigation gesture sends one message rather than dozens. The overlay
retires when the server's offset reaches the target, when the ring is invalidated, or after
`LOCAL_SCROLL_TIMEOUT` of 2 s.

Input that moves the server's view outranks a pending sync. A keystroke, committed IME text, a paste,
a search edit, and search next/previous all call `cancel_local_scroll`, which bumps the generation
counter the debounced task captured, so the queued `ScrollToOffset` never fires.

# Keyboard routing (client side)

Terminal keys and committed text go to the daemon, where the
[key tables](/tmux/key-tables.md) resolve first (the configured prefix arms the prefix table for
exactly one following key, tmux-style), and anything unbound passes through to the PTY. The client
does no key resolution of its own beyond the window-root
[prefix claim](/crates/zz.md), which recognizes only the configured prefix chord.

There is no client-side predictive echo. `Predictor` and the `predict` mux option existed to hide
WAN latency over QUIC and were deleted with that transport on 2026-08-01; a keystroke is drawn when
the daemon's next frame says so, exactly as it always was locally. Over ssh the RTT is the ssh RTT,
and the honest fix if it ever hurts is an application-level ping feeding a re-introduced overlay, not
a resurrection of the old one. See
[scene-streaming remote attach](/designs/scene-streaming-remote.md) for the retired design.

# Related

- Encoded and applied by [`zz-terminal`](/crates/zz-terminal.md) on the worker thread via [`libghostty-vt`](/terminal/libghostty-vt.md).
- Overlays produced here (selection, hover, copy cursor) are painted per [rendering-parity](/terminal/rendering-parity.md).
- Copy/view-mode semantics build on [copy-mode](/tmux/copy-mode.md); key routing layers over [key-tables](/tmux/key-tables.md).
- Actions travel the [terminal lanes](/protocol/terminal-lanes.md) of the [wire protocol](/protocol/wire-protocol.md).
- The client halves of local scroll (`HistoryRing`, `LocalScroll`) live in the [zz app](/crates/zz.md).
