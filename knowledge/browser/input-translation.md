---
type: Subsystem
title: Input translation (GPUI → CEF)
description: Browser-neutral pointer/wheel/keyboard/text/IME input, ordered GPUI-to-CEF dispatch, pane shortcuts, address resolution, and history-backed omnibox interaction.
resource: crates/zz-browser/src/input.rs
tags: [browser, input, ime, keyboard, url, history, omnibox]
timestamp: 2026-08-13T02:00:00Z
---

# Overview

Input translation converts GPUI's platform events into CEF host calls without
leaking GPUI or CEF types across the crate boundary. `input.rs` defines the
browser-neutral input value types (`Viewport`, `Modifiers`, pointer/wheel/key
records); `BrowserSession` and its main-thread `BrowserCommandSink` in
`cef_runtime.rs` perform the actual CEF `SendMouse*`/`SendKeyEvent`/`ImeX` calls.
`url_input.rs` handles the address field:
deciding between navigating and searching, normalizing typed text into an allowed
URL, and redacting URLs before diagnostics.
[`crates/zz`](/crates/zz.md) builds these neutral records from GPUI events and
calls the session methods on the foreground thread.

# Browser-neutral input types (`input.rs`)

| Type | Shape / notes |
| --- | --- |
| `Viewport` | `{ width, height, scale_factor, window_zoom, screen_x, screen_y, visible }`; `sanitized()` clamps `scale_factor` to `[0.5, 8.0]` and `window_zoom` to `[0.25, 4.0]` (non-finite or non-positive → 1.0). `window_zoom` is what `GetScreenPoint` folds into view-relative coordinates so Chromium-owned popups land correctly while the app UI is zoomed. |
| `Modifiers` | `#[repr(transparent)]` packed `u8`: SHIFT, CONTROL, ALT, PLATFORM, LEFT/MIDDLE/RIGHT_MOUSE, IS_REPEAT. Const builder + accessors. |
| `PointerButton` | `Left \| Middle \| Right`. |
| `PointerPhase` | `Move \| Leave \| Down \| Up`. |
| `PointerEvent` | `{ x, y, phase, button: Option<PointerButton>, click_count, modifiers }` (16 bytes). |
| `WheelEvent` | `{ x, y, delta_x, delta_y, precise, modifiers }` (20 bytes). |
| `KeyAction` | `Press \| Release`. |
| `BrowserKey` | `Character(char)`, named keys (Backspace…Delete), `Function(u8)`, `Unidentified`; `windows_key_code()` → Chromium cross-platform virtual key. |
| `KeyInput` | `{ action, key, modifiers }` (12 bytes). |

The packed layout is asserted by tests (`Modifiers` = 1 byte, `KeyInput` = 12,
`PointerEvent` = 16, `WheelEvent` = 20), matching the wire-size reductions the
README records for Chromium input records.

# Translation to CEF (`BrowserSession` / `BrowserCommandSink`)

| Neutral input | CEF host call |
| --- | --- |
| `send_pointer(PointerEvent)` | `MouseEvent` → `send_mouse_move_event` (Move/Leave, `1` = mouse-leave) or `send_mouse_click_event` (Down/Up + button + `click_count`). |
| `send_wheel(WheelEvent)` | `send_mouse_wheel_event`; `precise` sets `EVENTFLAG_PRECISION_SCROLLING_DELTA` and scales deltas into physical OSR space. |
| `send_key(KeyInput)` | `KeyEvent` (`RAWKEYDOWN`/`KEYUP`), `windows_key_code`, UTF-16 `character`, native key identity where required, `is_system_key` = Alt held. |
| `send_text(&str)` | One `KeyEventType::CHAR` event per UTF-16 code unit (`text_key_event`) . the committed-text path. |
| `commit_composition(&str)` | `ime_commit_text`. |
| `set_composition(text, selection_utf16)` | `ime_set_composition` with a `CompositionUnderline` over the whole text and a selection `Range`. |
| `finish_composition()` | `ime_finish_composing_text`. |
| `cancel_composition()` | `ime_cancel_composition`. |
| `set_focus(bool)` | `host.set_focus`. |

`Modifiers` map to CEF via `event_flags` (shift/control/alt/platform → command,
per-button flags, repeat). Pointer and precise-wheel coordinates are scaled with
`scaled_osr_coordinate(value, osr_raster_scale(viewport))` so that on the Wayland
physical-OSR path they land in device pixels, the same scaling described under
[OSR rendering](/browser/osr-rendering.md).

On macOS, CEF reconstructs Chromium keyboard events from a synthetic `NSEvent`.
Named keys therefore carry their Cocoa character and hardware key code instead
of zeroes; a press also emits the matching `CHAR` companion before `KEYUP`, as
CEF's OSR client does. This makes Tab/Shift+Tab focus traversal, Return form
submission, arrows, deletion, and function keys behave like Chromium rather
than being misclassified as modifier-flag changes. Common ASCII keys map to
their macOS ANSI positions so Command-based page shortcuts reach Chromium with
the correct key identity. Ordinary committed text still uses the separate
`send_text` path and is not duplicated.

The nested `Browser` key context binds Tab and Shift+Tab to GPUI's `NoAction`.
That masks the outer `Root` focus-navigation actions without consuming the raw
key events, keeping focus inside Chromium so its DOM traversal handles them.

Keyboard, committed-text, IME, focused-frame edit, and ordered navigation calls
from GPUI do not enter CEF inside an entity update. `BrowserController` captures
the active session's `BrowserCommandSink`, appends the owned operation to one
FIFO, and schedules one foreground drain for the burst. The drain takes the
batch while the controller is borrowed, returns from that update, and only then
calls CEF. CEF can synchronously re-enter the platform event loop, so moving the
call beyond the GPUI app borrow is required for correctness. The sink is
main-thread-only and keeps the browser identity captured at submission time.

# Keyboard ownership

Browser page keys and committed text use `InputMessage::BrowserSurfaceKey` and
`BrowserSurfaceText`. The daemon validates that their source is a Browser pane, skips the root
key table, and sends them through the existing synchronized Terminal/Browser sink resolver. A
`bind -n` or root `Any` binding therefore cannot steal page input.

The configured tmux prefix still wins. The workspace's window-root
[prefix claim](/crates/zz.md) captures that chord and every key while the sequence is armed, then
sends the existing key-table-routed `InputMessage::Key`. Prefix bindings, repeat bindings,
discarded unbound prefix keys, and paired releases keep their normal daemon behavior. The prefix
is not a literal page key; `<prefix> <prefix>` sends one.

Chromium's standard edit accelerators are page-owned before raw surface input. They call CEF's
focused-frame Undo, Redo, Cut, Copy, Paste, Paste-and-match-style, and Select-all commands directly
instead of relying on synthetic platform key events. The nested `Browser` binding outranks the
window-level `ZzRoot` copy handler, while a focused address field's deeper `ZzInput` context keeps
the same complete edit family and edits the URL normally.

# Pane shortcuts

The browser pane root installs a `Browser` key context before its raw key handler,
so browser-convention chords become GPUI actions and are not forwarded into the
web page or the daemon's synchronized-input path:

| Action | macOS | Linux / Windows |
| --- | --- | --- |
| Undo | `Cmd+Z` | `Ctrl+Z` |
| Redo | `Cmd+Shift+Z` | `Ctrl+Y` or `Ctrl+Shift+Z` |
| Cut | `Cmd+X` | `Ctrl+X` |
| Copy | `Cmd+C` | `Ctrl+C` |
| Paste | `Cmd+V` | `Ctrl+V` |
| Paste and match style | `Cmd+Shift+V` | `Ctrl+Shift+V` |
| Select all | `Cmd+A` | `Ctrl+A` |
| Zoom in | `Cmd+=` or `Cmd++` | `Ctrl+=` or `Ctrl++` |
| Zoom out | `Cmd+-` | `Ctrl+-` |
| Reset zoom | `Cmd+0` | `Ctrl+0` |
| New tab | `Cmd+T` | `Ctrl+T` |
| Close tab | `Cmd+W` (last tab closes the pane) | `Ctrl+W` |
| Next tab | `Ctrl+Tab`, `Cmd+Opt+→`, `Cmd+Shift+]` | `Ctrl+Tab`, `Ctrl+PgDn` |
| Previous tab | `Ctrl+Shift+Tab`, `Cmd+Opt+←`, `Cmd+Shift+[` | `Ctrl+Shift+Tab`, `Ctrl+PgUp` |
| Tab 1–8 | `Cmd+1`..`Cmd+8` | `Ctrl+1`..`Ctrl+8` |
| Last tab | `Cmd+9` | `Ctrl+9` |
| Back / Forward | `Cmd+[` / `Cmd+]` | `Alt+←` / `Alt+→` |
| Reload | `Cmd+R` | `Ctrl+R` or `F5` |
| Focus address bar | `Cmd+L` (selects the URL) | `Ctrl+L` (selects the URL) |
| Devtools | `Cmd+Opt+I` | `Ctrl+Shift+I` |

Edit, tab, navigation, and address actions dispatch to the same handlers as the context menu,
toolbar, and tab-strip controls. `Cmd+W` is not a `Browser`-context binding: a
contextless binding matches at the full context-stack depth and outranks every
context-scoped one, so the app-level `cmd-w → ClosePane` binding always wins.
The browser view instead handles `ClosePane` itself: closing the active tab,
and on the last tab propagating so the workspace handler kills the pane,
Chrome-style. Linux and Windows bind `Ctrl+W` to that same action, including the last-tab pane
close. `Cmd+Shift+W` closes the window.

`BrowserSession::{zoom_in, zoom_out, reset_zoom}` step through Chromium's
25–500% percentage ladder and call `CefBrowserHost::SetZoomLevel`. The page factor
is multiplied by the Wayland physical-OSR raster factor before conversion to CEF's
logarithmic zoom level, so changing page zoom never disables DPI compensation.
The same actions are available from the browser menu, which displays the current
percentage.

# Address / URL input (`url_input.rs`)

`normalize_url(input)` produces an allowed browser URL:

- trims; empty → `UrlInputError::Empty`;
- `about:blank` (case-insensitive) passes through;
- text without a scheme defaults to `https://`, except `localhost`,
  `*.localhost`, loopback IPs, and unspecified local bind IPs, which default to
  `http://` for development servers;
- a numeric `host:port` suffix is treated as an authority rather than a custom
  URL scheme, so `localhost:3000` resolves to `http://localhost:3000/`;
- an explicit `http://` or `https://` is always preserved;
- only `http`/`https` are accepted (else `UnsupportedScheme`); a missing host or
  parse failure → `Invalid`.

`resolve_address(input, provider)` is what the address bar actually submits, and
is the omnibox rule on top of `normalize_url`:

- an explicit scheme goes straight to `normalize_url` . `file:///tmp/a` names a
  URL the user meant, so it stays `UnsupportedScheme` rather than becoming a
  query, and `about:blank` still passes through;
- otherwise the entry navigates only if it *looks like a host*: no whitespace,
  and an authority carrying a numeric port, a bracketed IPv6 literal, an IPv4
  literal, `localhost`/`*.localhost`, or a dotted name whose last label is two or
  more alphabetic characters. `nas:5000`, `example.com/path`, `user@example.com`
  navigate; `rust lifetimes`, `weather`, `3.14`, `example.c0m` search;
- a search builds `<endpoint>?q=<form-encoded>` from `SearchProvider`
  (`Google` → `www.google.com/search`, `DuckDuckGo` → `duckduckgo.com/`,
  `Brave` → `search.brave.com/search`). All three read the query from `q`.

The provider comes from `browser-search-provider`
([app config](/configuration/app-config.md)), read per submit so a config edit
applies to open panes immediately. `normalize_url` stays strict and is what the
CLI's `open-uri` route and new-tab URLs use . a link that is not a URL must not
silently become a search.

`diagnostic_url(input)` redacts URLs before logging: it strips username/password
(except for `about:` URLs) and always drops query and fragment, returning
`"<invalid URL>"` for unparseable input. `UrlInputError` variants: `Empty`,
`UnsupportedScheme`, `Invalid`.

# History-backed omnibox

The address field's GPUI input layer searches the current logical browser
profile after each non-empty edit. Results match every whitespace-delimited term
across URL and title, then rank address fit, learned selection, typed use, visit
frequency, and recency. At most eight native title-and-URL rows appear below the
toolbar. Empty focus does not open a generic history popup; blank tabs retain
their separate recent-page surface.

Up and Down wrap through the rows and temporarily preview the selected URL.
Enter accepts that row or resolves the original text through `resolve_address`.
Escape first restores an original query from a preview, then closes the popup,
then restores the current page URL and returns focus to the page. Shift+Delete
removes the selected URL and its learned mappings from the current profile.

The top eligible URL-prefix result can complete a one-character append inline.
The appended suffix stays selected so another keystroke replaces it. Root URLs
need one successful typed use, while deeper URLs need two; deletes and
multi-character replacements do not inline-complete. Typed and learned-selection
credit is committed only after the submitted navigation starts and finishes
successfully; bulk direct replacements remain ordinary visits.
See [history and omnibox autocomplete](/browser/history-autocomplete.md) for the
store, significance filter, scorer, Chrome import, and privacy boundary.

# Examples

```rust
assert_eq!(normalize_url(" example.com/path ").unwrap(), "https://example.com/path");
assert_eq!(normalize_url("localhost:3000").unwrap(), "http://localhost:3000/");
assert_eq!(normalize_url("file:///tmp/a"), Err(UrlInputError::UnsupportedScheme));

// Omnibox: address vs. query, on the configured engine.
assert_eq!(
    resolve_address("example.com", SearchProvider::Google).unwrap(),
    "https://example.com/",
);
assert_eq!(
    resolve_address("rust lifetimes", SearchProvider::DuckDuckGo).unwrap(),
    "https://duckduckgo.com/?q=rust+lifetimes",
);

// Redaction: credentials + query + fragment removed.
assert_eq!(
    diagnostic_url("https://user:secret@example.com/path?q=token#s"),
    "https://example.com/path",
);
```

# Key files

| File | Role |
| --- | --- |
| `src/input.rs` | Browser-neutral `Viewport`/`Modifiers`/pointer/wheel/key types + `windows_key_code`. |
| `src/page_zoom.rs` | Chromium percentage ladder and factor-to-CEF-level conversion. |
| `src/url_input.rs` | `resolve_address`, `SearchProvider`, `normalize_url`, `diagnostic_url`, `UrlInputError`. |
| `src/cef_runtime.rs` | `BrowserSession`, main-thread `BrowserCommandSink`, CEF input/edit helpers, `event_flags`, coordinate scaling. |
| `crates/zz/src/browser/controller.rs` | Ordered foreground CEF command queue and the GPUI re-entrancy boundary. |
| `crates/zz/src/browser/view.rs` | Platform keybindings, nested omnibox key context, input events, selection and Escape stages, page edit actions, zoom actions, and menu controls. |
| `crates/zz-client/src/chrome.rs` | Browser chrome action names, platform profile defaults, extended chord grammar, and table resolution. |
| `crates/zz/src/keymap.rs` | GPUI bridge for browser chrome defaults and `chrome-keybind`/`chrome-unbind` overrides. |
| `crates/zz/src/browser/recent_pages.rs` | Profile-scoped history queries, scoring, inline eligibility, learned selections, and deletion. |

# Related

- Coordinate scaling is shared with the
  [off-screen rendering](/browser/osr-rendering.md) DPI path.
- Input records are built from GPUI events in [`crates/zz`](/crates/zz.md);
  focus/cursor state feeds the [lifecycle events](/browser/lifecycle.md).
- Address-field results and successful-use learning are detailed in
  [history and omnibox autocomplete](/browser/history-autocomplete.md).
- Part of the [zz-browser crate](/crates/zz-browser.md).
