---
type: Reference
title: zpui web windows for agents
description: How zpui-web hosts several windows in one page (WindowOptions::mount), mirrors each window's AccessKit tree into ARIA elements, and exposes globalThis.zpui so agents can find, click, wait for and capture GPUI content.
resource: crates/zpui-web/src/automation.rs
tags: [zpui, web, wasm, accessibility, accesskit, agents, automation]
timestamp: 2026-10-09T00:00:00Z
---

# Overview

A GPUI page used to be one canvas covering `<body>`. `zpui-web` now draws each window into its own
canvas, and a page can host several windows among its own HTML. Every window can also put its
accessibility tree into the page as ordinary elements, which is what lets agents, Playwright and
screen readers see buttons and text that only exist as pixels.

The worked example is `crates/zpui-web/examples/web_agents`: three windows mounted in a static
page. `just web agents` builds it and serves it on `http://127.0.0.1:8095`.

# Mounting windows

`WindowOptions::mount` takes a CSS selector. The window's canvas is placed inside the first match
and fills it (`position: absolute; inset: 0`), so the page decides the window's size with CSS.
`None` keeps the old behavior of filling the page.

- `Window::resize` on a mounted window sets only the element's height. Its width follows the page.
  A window that sizes itself to its content measures the content and calls `resize`.
- Mounted windows start inactive and do not take keyboard focus until clicked, so a page with many
  windows keeps normal page scrolling and tabbing. Each window has its own hidden IME textarea.
- Only WebGPU can draw more than one window. WebGL2 binds its device to one canvas, so on the WebGL2
  fallback the second window fails with `WebWindowError::MultipleWindowsNeedWebGpu`.
- `set_title` on a mounted window does not touch `document.title`.

Sources: `crates/zpui-web/src/platform.rs` (`open_window`, `find_mount`) and
`crates/zpui-web/src/window.rs` (`mount_canvas`).

# The accessibility mirror

GPUI core builds an AccessKit tree every frame once accessibility is active (see
`crates/zpui/src/_accessibility.rs`). The web backend used to drop it. `crates/zpui-web/src/a11y.rs`
now keeps one transparent layer per window, next to the canvas, holding one `div` per node.

- Each node gets the matching ARIA role, `aria-label`, value, checked, selected, expanded,
  disabled, level and position attributes. Text leaves carry their text as content.
- Nodes are absolutely placed over the pixels they describe, using the AccessKit bounds divided by
  the device pixel ratio.
- `data-zpui-node` holds the AccessKit node id, `data-zpui-id` the author id set with
  `accessibility_id`, and `data-zpui-focused` marks GPUI's focused node.
- The layer never takes pointer input by default, so real clicks still land on the canvas.
  `element.click()` on a mirrored node becomes an AccessKit `Click`.

Interactive mode is for tools that only act on elements under the pointer. Playwright's agent
snapshot, for one, gives a ref only to elements whose `pointer-events` is not `none`. In this mode,
nodes that support `Click` or `Focus` take pointer input, and the layer re-dispatches their
pointer, wheel and context-menu events to the canvas. Hover and drag keep working, and a real click
runs once through GPUI's own input path instead of as an AccessKit action.

Accessibility is off until asked for, because the tree costs work every frame:

- `<html data-zpui-a11y>` or `globalThis.zpuiAccessibility = true` before boot turns it on.
- `<html data-zpui-a11y="interactive">` turns on interactive mode as well.
- `zpui.enableAccessibility()` turns it on later. Until then a visually hidden "Enable
  accessibility" button lets a screen reader do it.

Core change: `Window::new` in `crates/zpui/src/window.rs` used to skip `a11y_init` on wasm because
`async_channel::Sender::send_blocking` does not exist there. It now uses `try_send`, which never
fails on the unbounded channels involved.

# The zpui global

`crates/zpui-web/src/automation.rs` installs `globalThis.zpui` when the first window opens.

| Call | Result |
|---|---|
| `zpui.windows()` | `[{ id, mount, title, width, height, active, accessibility }]` |
| `zpui.enableAccessibility({ interactive })` | Turns the mirror on for every window; resolves when idle |
| `zpui.disableAccessibility()`, `zpui.setInteractive(bool)` | The reverse, and the mode switch |
| `zpui.tree(windowId?)` | The window's tree: `{ ref, role, name, value, text, id, bounds, focused, states, children }` |
| `zpui.find(query, windowId?)` | Matching nodes, in document order, without children |
| `zpui.click(ref)`, `zpui.focus(ref)` | An AccessKit action on the node; resolves when idle |
| `zpui.capture(windowId?)` | Renders a frame and resolves to a PNG data URL of the window |
| `zpui.idle()` | Resolves once no window has a frame pending |

- A `ref` is `"<window>:<node>"`. Node ids are 64-bit hashes, so they travel as strings.
- `bounds` are viewport CSS pixels, ready for CDP mouse events.
- `find` accepts `{ role, name, text, id }` or a bare string. `role` is the ARIA role, `name` an
  exact label, `text` a case-insensitive substring of the node's label, value or descendant text,
  and `id` an author id. A text query returns the innermost match, not every ancestor.
- Hidden pages get no `requestAnimationFrame`, so GPUI never draws and the tree goes stale.
  `zpui.idle()` polls on a timer and renders pending frames itself while `document.hidden` is
  true, so agents can drive background tabs.

A typical agent loop:

```js
await zpui.enableAccessibility();
const [send] = zpui.find({ role: "button", name: "Send" });
await zpui.click(send.ref);
const png = await zpui.capture(send.window);
```

# Related

- [zpui](/references/zpui.md): where the zpui crates live and how to change them.
- [Browser client](/playbooks/browser-client.md): the zz web client, which uses one page-filling
  window.
