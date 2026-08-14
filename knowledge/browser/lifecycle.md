---
type: Subsystem
title: Browser runtime & session lifecycle
description: Runtime/profile-context/session state machines and the browser-neutral events that CEF callbacks translate into.
resource: crates/zz-browser/src/lifecycle.rs
tags: [browser, lifecycle, events, state-machine]
timestamp: 2026-08-14T00:00:00Z
---

# Overview

The lifecycle subsystem is `lifecycle.rs` (two strict state machines) plus
`event.rs` (the browser-neutral event and cursor enums), driven by the runtime and
session logic in `cef_runtime.rs`. It governs how the single CEF runtime starts and
shuts down, how named persistent request contexts become ready, how each windowless
`BrowserSession` is created, becomes ready, may
crash, and closes, and how every CEF callback becomes an owned `BrowserEvent` that
[`crates/zz`](/crates/zz.md) drains on the foreground thread. CEF handles never
escape the crate; the app only sees these enums.

# State machines

`RuntimePhase` (`lifecycle.rs`), the global CEF runtime:

```text
Uninitialized -> Initializing -> Running -> Closing -> Closed
                             \-> Failed
```

`SessionPhase`, one browser session:

```text
Creating -> Ready -> Crashed
   |          |         |
   +----------+---------+-> Closing -> Closed
```

Both expose `may_transition_to(next) -> bool` enforcing exactly these edges. A
crashed session cannot go backward to `Creating`; recovery creates a **new** session
generation against the same [profile](/browser/profile.md). `Failed` is terminal
only when CEF never initialized; a failure after successful init still follows
`Closing -> Closed` so partial startup cannot bypass CEF cleanup.

# Runtime control (`BrowserRuntime`)

| Method | Role |
| --- | --- |
| `handle_context_initialized()` | On `ContextInitialized`, create the `default` persistent `RequestContext` (still `Initializing`). |
| `handle_request_context_initialized(profile)` | Mark the tagged context ready; the default callback advances the runtime to `Running`. |
| `ensure_profile_context(profile)` | Validate/create a named zz profile and lazily start its request context; returns whether it is ready. |
| `create_session(profile, initial_url, viewport, page_zoom_factor, ...)` | Create a windowless browser against a ready named context; requires `Running`. |
| `do_message_loop_work()` | Step the external CEF pump (see [CEF runtime](/browser/cef-runtime.md)). |
| `shutdown()` | Refuses while sessions/data operations remain, releases every profile context, then calls `cef::shutdown()` exactly once. |

`Drop for BrowserRuntime` logs an error if dropped while still initialized:
`cef::shutdown` ordering must be explicit. `active_sessions` is an
`Arc<AtomicU64>` incremented at `create_session` and decremented in
`mark_closed`.

A remote pane uses a local composite request context named
`<profile>@egress-<hash8>`. Once that context becomes ready, the controller points
its proxy preference at the managed `ssh -D` SOCKS port before creating the
session. Local panes keep the plain named context. A reconnect can change the
SOCKS port, so snapshot refresh re-applies the preference to the retained context.

# Session control (`BrowserSession`)

- **State:** `mark_ready` (`Creating -> Ready`), `mark_crashed`
  (`Ready -> Crashed`, clears the frame mailbox and cancels any element pick),
  `mark_closed` (`-> Closed`, decrements the live count once), `close(force)`
  (`-> Closing`, calls `host.close_browser`). `Drop` force-closes an un-closed
  browser.
- **Navigation:** `navigate(url)`, `go_back`/`go_forward` (guarded by
  `can_go_back`/`can_go_forward`), `reload`.
- **View:** `set_viewport` (see [DPI handling](/browser/osr-rendering.md)),
  `set_focus(bool)` → `host.set_focus`, and `zoom_in`/`zoom_out`/`reset_zoom`
  → the Chrome percentage ladder while retaining the raster-scale factor.
- **Input / IME:** `send_pointer`, `send_wheel`, `send_key`, `send_text`, and the
  composition calls, covered under [input translation](/browser/input-translation.md).

# Schema . `BrowserEvent`

Owned events (`event.rs`), each tagged with its `SessionId` (retrievable via
`BrowserEvent::session()`), delivered over an `async-channel` and produced by the
CEF handlers noted:

| Variant | Fields | CEF source |
| --- | --- | --- |
| `Created` | `session` | `LifeSpanHandler::on_after_created` |
| `AddressChanged` | `url: Arc<str>` | `DisplayHandler::on_address_change` (main frame) |
| `TitleChanged` | `title: Arc<str>` | `DisplayHandler::on_title_change` |
| `LoadingChanged` | `loading`, `can_go_back`, `can_go_forward` | `LoadHandler::on_loading_state_change` |
| `FrameReady` | `generation: u64` | `RenderHandler::on_paint` publish wake |
| `LoadFailed` | `code`, `description: Arc<str>`, `url: Arc<str>` | `LoadHandler::on_load_error` (ignores `ABORTED`, non-main frames) |
| `CursorChanged` | `cursor: BrowserCursor` | `DisplayHandler::on_cursor_change` |
| `ElementPicked` / `ElementPickCancelled` / `ElementPickFailed` | `text?` | [element picker](/browser/element-picker.md) query handler |
| `PopupRequested` | `url: Arc<str>`, `foreground: bool` | `LifeSpanHandler::on_before_popup`, `RequestHandler::on_open_urlfrom_tab` (native popup cancelled; `foreground` is false only for `NEW_BACKGROUND_TAB` dispositions) |
| `RenderProcessTerminated` | `status: Arc<str>`, `error_code` | `RequestHandler::on_render_process_terminated` |
| `Closed` | `session` | `LifeSpanHandler::on_before_close` |

`SessionId(pub u64)` identifies one immutable CEF browser generation.
`BrowserCursor` is a GPUI-representable cursor-shape enum (`Arrow`, `IBeam`,
`PointingHand`, `Crosshair`, `Wait`, `Help`, `Move`, resize variants, `Grab`,
`Grabbing`, `NotAllowed`, `None`) mapped from CEF `CursorType` by `browser_cursor`.

# Focus, resize, popups

- **Focus** is one-directional: `set_focus(true/false)` forwards to the CEF host;
  focus is never shared with the terminal surface.
- **Resize** flows through `set_viewport` → `apply_viewport`, which drives
  `was_hidden`, `notify_screen_info_changed`, and `was_resized` in CEF's reference
  order (see [OSR rendering](/browser/osr-rendering.md)).
- **Popups / new windows** never open a native surface: `on_before_popup` and
  `on_open_urlfrom_tab` cancel the popup and emit `BrowserEvent::PopupRequested`;
  the app opens the URL as a new tab in the same pane (a tab is one
  `BrowserSession`, keyed `(PaneId, TabId)` in the app's controller, with only
  the pane's active tab visible). Privileged operations are denied by the
  `Denied*` handlers.

# Key files

| File | Role |
| --- | --- |
| `src/lifecycle.rs` | `RuntimePhase`/`SessionPhase` + `may_transition_to` tables and tests. |
| `src/event.rs` | `SessionId`, `BrowserEvent`, `BrowserCursor`, `BrowserEvent::session()`. |
| `src/cef_runtime.rs` | `BrowserRuntime`/`BrowserSession` methods and the handlers that emit events. |

# Related

- Driven by the [CEF runtime](/browser/cef-runtime.md); events consumed by the GPUI
  client in [`crates/zz`](/crates/zz.md), which discards events from a stale
  session generation.
- Sessions restore against the persistent [profile](/browser/profile.md); see
  [session persistence](/concepts/session-persistence.md).
- Part of the [zz-browser crate](/crates/zz-browser.md).
