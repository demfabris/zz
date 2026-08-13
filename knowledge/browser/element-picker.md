---
type: Concept
title: In-page element picker
description: A token-guarded, single-use overlay that lets the user pick a DOM element in the page and returns a bounded, sanitized source-context string plus an optional screenshot of the picked area.
resource: crates/zz-browser/src/element_picker.rs
tags: [browser, element-picker, devtools, message-router]
timestamp: 2026-07-27T00:00:00Z
---

# Overview

The element picker lets a user select a DOM element inside a rendered page and
hands the app a short, bounded description of that element (an inspector-style
source-context string) and, when the page reported usable geometry, a PNG of the
area around it. It has two halves: an injected renderer-side script
(`assets/element-picker.js`) that draws the selection overlay and reports results,
and a Rust protocol guard (`element_picker.rs`, `ElementPickState`) that
authenticates and validates each result before it becomes a
[`BrowserEvent`](/browser/lifecycle.md). Communication rides CEF's JavaScript
**message router** wired up in `cef_runtime.rs`. Untrusted page content cannot
deliver arbitrary payloads: every result must match a fresh random token and a
strict shape. The overlay's appearance is a separate, app-authored snapshot of
the resolved zz-ui theme, JSON-serialized into the start call rather than kept
as a second hardcoded palette inside the page.

# Activation and pane focus

The browser toolbar's inspector button and the browser-scoped
`browser-element-selector-hotkey` setting both toggle the picker. The shortcut defaults to
`cmd-shift-c` on macOS and `ctrl-shift-c` elsewhere. It uses GPUI keystroke syntax and must contain
a non-Shift modifier (Control, Alt, Command/Super, or Function), which prevents ordinary page
typing from being claimed. Editing the Browser page in Settings writes the same scalar to
`zz/config`; the normal 500 ms config poll installs the new binding without restarting the app.
Previously bound values remain harmless: their action carries the source spelling and propagates
unless it still equals the current setting.

Browser chrome and native blank-page interactions claim their mux pane just like page-content
mouse input. A left click on the toolbar, address field, blank/recent-pages background, or recent
row selects the pane before moving focus or navigating. This keeps the selector shortcut and all
other Browser-context actions attached to the pane the user just clicked.

Each activation snapshots the current semantic theme roots in `BrowserView`.
The highlighter derives its outline and wash from `foreground`, while the DOM
preview uses `background.raised(1).opaque()`, `foreground`, `border`, the widget
radius, the resolved mono family, and the theme's shadow policy. Browser page
zoom is carried with the snapshot so the overlay's border, label, and radius
remain app-chrome sized instead of growing with the inspected page. Both
surfaces use the same adaptive-radius curve and squircle shape as native zz
widgets. Changing page zoom during an active pick replaces that snapshot so the
screen-space metrics stay stable.

# How it works

1. **Script injection**: `RuntimeRenderProcessHandler::on_context_created` runs
   `element-picker.js` in the main frame (URL `zz://browser/element-picker.js`),
   exposing `globalThis.__zzElementPicker`.
2. **Start**: `BrowserSession::start_element_pick()` (only when the session is
   `Ready` and the picker registered) mints a token via `ElementPickState::begin()`
   and executes `globalThis.__zzElementPicker?.start(<token>, <appearance>)` with
   both arguments JSON-serialized. The overlay lets the user hover/click a DOM
   element.
3. **Report**: the script calls the router's JS query function
   `__zzElementPickerQuery` (cancel function `__zzElementPickerQueryCancel`) with a
   JSON message. `ElementPickerQueryHandler::on_query_str` rejects persistent
   queries and any non-main-frame origin, then calls `ElementPickState::consume`.
4. **Emit**: on success the handler emits
   `BrowserEvent::ElementPicked { session, text, screenshot }`,
   `ElementPickCancelled`, or `ElementPickFailed`. When the result carried a
   usable geometry the handler holds the event until Chromium answers the
   capture, so the PNG is announced *with* the pick rather than after it;
   `screenshot` is `None` whenever the capture could not be produced or was
   never submitted.

The active pick is auto-cancelled on navigation (`on_before_browse`), before close
(`on_before_close`), and on renderer termination
(`on_render_process_terminated`), each emitting `ElementPickCancelled`.
`cancel_element_pick()` runs `globalThis.__zzElementPicker?.cancel()`.

# Protocol & validation (`ElementPickState`)

`ElementPickState` holds a single `active_token: Option<Arc<str>>`. Tokens are
16 random bytes from `getrandom`, hex-encoded. The pick is **single-use** and
constant-token: consuming a valid result clears the token.

Wire message (JSON, deserialized into `WireMessage`):

| Field | Type | Rule |
| --- | --- | --- |
| `version` | `u8` | Must equal `PICKER_PROTOCOL_VERSION` (1), else `UnsupportedVersion`. |
| `kind` | `String` | `"picked"`, `"cancelled"`, or `"failed"`. |
| `token` | `String` | Non-empty, ≤ 64 bytes; must equal the active token. |
| `text` | `Option<String>` | Required for `"picked"`; the element context. |
| `geometry` | `Option<serde_json::Value>` | Optional for `"picked"`; staged as raw `Value`, then parsed into `PickGeometry` and dropped if that fails or the values are unusable. |

`consume(request)` enforces, in order: message ≤ `MAX_PICKER_MESSAGE_BYTES`
(`MAX_ELEMENT_CONTEXT_BYTES` + 1024, the slack covering the framing plus the
eight-field geometry object), valid JSON, matching version, valid token length;
for `"picked"` the `text` must be non-empty, ≤ `MAX_ELEMENT_CONTEXT_BYTES`
(32 KiB), contain **no control characters**, and be bracketed (`starts_with('[')`
and `ends_with(']')`). Only then is the token compared; a mismatch is `StaleToken`
and does **not** consume the active pick.

`geometry` is deliberately the weak field. It is deserialized through
`serde_json::Value` first, so a geometry an older or tampered bundle got wrong
cannot fail the whole message and cost the text context. `PickGeometry` carries
eight CSS-pixel `f64`s . `x`, `y`, `width`, `height`, `scroll_x`, `scroll_y`,
`viewport_width`, `viewport_height` . the rect being viewport-relative and the
scroll offsets turning it into page coordinates. `is_usable()` requires every
value finite and all four extents positive; anything else is dropped and the pick
still succeeds without a screenshot. `cef_runtime.rs`'s `element_screenshot_clip`
turns a usable geometry into a `ScreenshotClip` (margin-padded, clamped to the
viewport) and `start_element_screenshot` captures it.

`ElementPickMessageError` variants: `OversizedMessage`, `MalformedMessage`,
`UnsupportedVersion`, `InvalidToken`, `NoActivePick`, `StaleToken`,
`InvalidContext`. `ElementPickOutcome`: `Picked(Arc<str>, Option<PickGeometry>)`,
`Cancelled`, `Failed`.

# Examples

A validated `"picked"` context string looks like an inspector source locator:

```text
[<button>Save</button> in SaveButton (at src/save.tsx:3:2)]
```

```rust
let token = state.begin().unwrap();               // 32-hex-char single-use token
let msg   = /* {"version":1,"kind":"picked","token":token,"text":"[<div />]"} */;
assert_eq!(
    state.consume(&msg),
    Ok(ElementPickOutcome::Picked(Arc::from("[<div />]"), None)),  // no geometry, no screenshot
);
assert_eq!(state.consume(&msg), Err(ElementPickMessageError::NoActivePick)); // single-use
```

# Key files

| File | Role |
| --- | --- |
| `src/element_picker.rs` | `ElementPickState` token guard, wire-message parsing/validation, outcomes/errors, and the typed appearance/start-call serializer. |
| `picker/src/index.ts` | Source for the theme-aware highlighter, measured DOM preview, and result reporter. |
| `assets/element-picker.js` | Generated injected renderer bundle (~49 KB); rebuild it from `picker/`. |
| `src/cef_runtime.rs` | Message-router setup, script injection, `ElementPickerQueryHandler`, start/cancel + auto-cancel. |
| `../zz/src/browser/view.rs` | Resolves the active zz-ui theme into `ElementPickerAppearance` at activation time. |

# Related

- Results arrive as [`BrowserEvent`](/browser/lifecycle.md) values and are consumed
  by [`crates/zz`](/crates/zz.md).
- Runs over the CEF message router set up by the
  [CEF runtime](/browser/cef-runtime.md).
- Part of the [zz-browser crate](/crates/zz-browser.md).
