---
type: Design Plan
title: Userpanes . JS+HTML panes on the CEF stack
description: Proposed plan for user-defined HTML panes rendered by the existing CEF browser machinery - a zz:// scheme serving local files, an origin-gated window.zz bridge (run/on/identity), a zz html verb plus a ~/.config/zz/panes registry, and zz pane-send for pushing data into a live pane.
status: Proposed
tags:
- browser
- cef
- panes
- bridge
- scheme-handler
- agent-canvas
- design-plan
timestamp: 2026-07-28T12:00:00Z
---

# Overview

Userpanes let anyone build a zz pane out of HTML+JS instead of Rust: drop a file, open it as a
pane, and script the workspace from inside it. zz already ships a browser engine, a CLI that
forwards any verb to the daemon socket, and a working renderer↔app query bridge (the element
picker); userpanes are the thin layer that composes them.

The flagship use case is the **agent canvas**: an agent pane already has `ZZ_PANE`/`ZZ_SOCKET` in
its environment, so an agent can write an HTML file and run `zz html /tmp/chart.html` to *render*
things . charts, diagrams, interactive reports . instead of printing text. Personal dashboards
(bench charts, sysmon widgets, scratchpads) fall out of the same mechanism.

## Goals

* Open any local HTML file as a pane (`zz html <path>`), and update it in place.
* A `window.zz` bridge so pane JS can run mux/workspace verbs and receive pushed data.
* A `~/.config/zz/panes/` registry surfaced in the pane picker.
* Minimal new surface: no new pane kind, one additive protocol change.

## Non-goals (v1)

* No plugin/extension story: no manifest, no permissions model, no versioned API, no sharing.
* No shell execution from pane JS . the command catalog is the boundary.
* No file watching or live-reload (rewrite + `reload` covers it).
* No migration of zz's own chrome to HTML. Userpanes are an addition, not a UI strategy.

# Decision: browser panes + a zz:// scheme, not a new PaneKind

A userpane is `PaneKind::Browser` with `url = "zz://pane/bench"` . not a sixth pane kind. Every
scoped capability (serving, bridge, registry, push events) is identical under both shapes; a
dedicated `PaneKind::Custom` would buy a different sidebar icon at the cost of the full ~15-file
pane-kind template, a protocol enum variant, and a view that is 90% `BrowserView`. Reusing
`BrowserDescriptor { url }` makes persistence, snapshot/restore, `split-browser`,
`set-browser-url`, and title plumbing work untouched. If userpanes earn their own identity later,
promotion to a real pane kind is mechanical (the Editor template remains the reference).

Accepted cost: userpanes show the browser icon in the sidebar and picker.

# The zz:// scheme

Chromium must be told about the scheme in every process: register `zz` as a standard, secure,
fetch-enabled scheme in the CEF `App`'s `on_register_custom_schemes` (the single-binary re-exec
model means the same registration code runs in browser, renderer, and GPU processes). A scheme
handler factory is registered on each profile's `RequestContext` when it is created in
`cef_runtime.rs` (per-profile lookup at `crates/zz-browser/src/cef_runtime.rs:742-747`); the
`cef` crate's unused `wrapper::resource_manager` / `register_scheme_handler_factory` surface covers
this without new bindings.

Two URL namespaces, both serving straight from disk:

| URL | Serves |
|---|---|
| `zz://pane/<name>/…` | `~/.config/zz/panes/<name>/…` (a directory with `index.html`), or `~/.config/zz/panes/<name>.html` for single-file panes |
| `zz://file/<abs-path>` | that path directly; relative asset URLs resolve against the file's parent directory |

Rules:

* **Canonicalize and prefix-check every resolved path.** `zz://pane/../../etc/passwd` dies in the
  handler. `zz://file/` is unrestricted by construction (it takes an absolute path), which is
  acceptable because only the local user . already able to read those files . can open one.
* MIME type from file extension; unknown → `application/octet-stream`.
* Missing file / unknown pane name → a minimal inline 404 page.
* Same-origin fetch works . a userpane can `fetch("data.json")` next to itself (the agent-canvas
  data path). Cross-scheme access stays closed: http(s) content cannot fetch, XHR, or iframe
  `zz://` URLs because the scheme is not CORS-enabled and the handler serves no CORS headers.
* Paths inside `zz://file/…` URLs are percent-encoded; the handler decodes before resolving.
* `crates/zz-browser/src/url_input.rs:20-46` adds `zz` to the http/https/about:blank allowlist
  so `zz://` URLs can be typed or passed to `set-browser-url` / `new-browser` directly.

The registry directory resolves with the same XDG-then-fallback logic as `zz/config`
(`crates/zz/src/config/mod.rs`, `parse_config`); the scheme handler runs in the app (browser) process, so the
lookup is ordinary filesystem code.

# The window.zz bridge

Two one-way channels, each a copy of a pattern the element picker already proves out:

**Page → app: a second message router.** Alongside `element_picker_router_config()`
(`cef_runtime.rs:68-74`), add a `MessageRouterConfig { js_query_function: "__zzPaneQuery", … }`
with its own per-session `BrowserSideRouter` and a `PaneQueryHandler` modeled on
`ElementPickerQueryHandler` (`cef_runtime.rs:3046-3117`): reject non-main-frame senders, parse a
small JSON envelope, answer via the callback.

**App → page: script execution.** The app delivers pushes by executing
`globalThis.__zzPaneDeliver(<json>)` on the main frame . the same `execute_java_script` pattern the
picker's start/cancel uses (`cef_runtime.rs:1320-1341`). No persistent queries.

**Origin gating, twice.** The renderer-side injection point (`on_context_created`,
`cef_runtime.rs:3199-3217`) injects the shim only when the frame URL scheme is `zz`; ordinary
browser panes never see `window.zz`. The app-side `PaneQueryHandler` independently re-checks the
browser's main-frame URL before honoring any query, so a compromised or navigated-away page cannot
keep driving the mux.

**The shim** is TypeScript bundled by the existing esbuild pipeline
(`crates/zz-browser/picker/`, second entry point, same `build.mjs --check` staleness gate in CI),
`include_str!`'d like the picker script. Its whole API:

```ts
zz.run(cmd: string): Promise<string>  // any mux/workspace verb; rejects with the daemon's error
zz.on("message", cb): () => void      // pushed JSON lands here; returns an unsubscribe fn
zz.pane: string                       // own pane id  ("info" query at startup)
zz.session: string                    // own session id
```

`zz.run` routes through the app's existing daemon connection as a normal `CommandInvocation` .
the same path the CLI takes . so the full catalog works with no per-verb glue. v1 imposes no
allowlist; the catalog itself (no `run-shell`, no exec verbs) is the security boundary, and a
future allowlist knob slots into the handler without API changes.

# Opening flows

* **`zz html <path> [-t <pane>]`** . a daemon-level workspace verb (the `capture-browser` family,
  `crates/zz-daemon/src/daemon.rs:1279-1290`). Resolves the path to absolute, then opens a new
  browser pane at `zz://file/<path>`, or navigates the target pane there when `-t` is given.
  Rewriting the file and issuing the existing `BrowserCommand::Reload` refreshes in place.
* **Registry + picker.** `~/.config/zz/panes/` is scanned on demand (no watcher). The pane picker
  (`crates/zz/src/pane/picker.rs:13-90`) gains a **Userpane** choice, shown only when the
  registry is non-empty, which opens a chooser of pane names via the existing chooser machinery;
  picking one materializes `new-browser "zz://pane/<name>"`. Command completion learns registry
  names for the URL-taking verbs.
* Everything else . `split-browser`, snapshot/restore, profiles . is untouched browser-pane
  plumbing.

# Push events

**`zz pane-send -t <pane> '<json>'`** mirrors `agent-send`: the daemon forwards over the existing
daemon→GUI channel (`EventPayload::BrowserCommand`, `crates/zz-protocol/src/message.rs:937-948`) with
one new variant, `BrowserCommand::PostMessage { data: String }` (`message.rs:610-628`). This is the
design's only protocol change . additive, `PROTOCOL_VERSION` 33 → 34 (`message.rs:31`). The GUI
side (`crates/zz/src/mux/client.rs`, `MuxClient::handle_message`) delivers via `__zzPaneDeliver`, landing in the page's
`zz.on("message", …)` listeners. Payloads are delivered verbatim; validation is the page's job.

A shell script, another pane, or an agent can therefore stream data into a live userpane without
the page polling.

# UI polish

* `crates/zz/src/browser/view.rs` hides the URL-bar toolbar when the current URL is `zz://` .
  userpanes should read as panes, not browser tabs.
* Pane title comes from `document.title` through the existing `TitleChanged` event; a `<title>`
  tag names the pane for free.

# Error handling

| Failure | Behavior |
|---|---|
| `zz.run` command fails | promise rejects with the daemon's error string |
| unknown registry name / missing file | inline 404 page |
| malformed `pane-send` payload | delivered verbatim; page decides |
| `zz://` typed while feature absent (old build) | `UrlInputError::UnsupportedScheme`, as today |
| renderer crash | existing `on_render_process_terminated` router plumbing; pane shows CEF error page |

# Testing

* Unit: URL→path resolution, exhaustively over traversal cases (`..`, symlinks, encoded slashes);
  `url_input` allowlist additions.
* Shim: esbuild `--check` staleness gate in CI, same as the picker bundle.
* Fixture: a `zz_browser_fixture` case loads a `zz://` page and round-trips one `zz.run`.
* Smoke: headless daemon recipe covers `zz html` and `pane-send` end to end.

# Deferred

File watching / live-reload; a command allowlist knob; userpane promotion to a real `PaneKind`;
any packaging or sharing story; `zz://` pages for zz-internal UI. All are additive on top of this
design; none require rework of it.
