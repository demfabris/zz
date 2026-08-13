---
type: Rust Crate
title: zz-browser crate
description: Browser-neutral abstraction over CEF Alloy off-screen rendering. Owns CEF init, named private request contexts, page zoom, input, lifecycle, and frame mailboxes.
resource: crates/zz-browser/src/lib.rs
tags: [browser, cef, crate, osr]
timestamp: 2026-08-01T00:00:00Z
---

# Overview

`crates/zz-browser` (package name `zz-browser`) is the single crate that touches
CEF. It wraps CEF Alloy off-screen rendering (OSR) behind a small, CEF-free public
API so the rest of zz never sees a raw Chromium handle. It owns global CEF
initialization, single-binary multi-process subprocess dispatch, named private
persistent request contexts ("zz profiles"), GPUI-to-CEF input translation,
runtime/session lifecycle state machines, and one **latest-frame mailbox** per
session. The GPUI client in [`crates/zz`](/crates/zz.md) consumes only the
browser-neutral types re-exported from `lib.rs`, never CEF binding types.

Frame transport is a replaceable boundary, and it has already been replaced twice:
`OsrFrame` is an enum over `OwnedBgra` (the universal readback tier), `Gpu` (a
zz-owned wgpu texture on Linux/FreeBSD), and `MacGpu` (a retained `IOSurface` on
macOS). Adding or losing a tier changes nothing about navigation, profile,
lifecycle, or input. See [OSR rendering](/browser/osr-rendering.md) for the three
pipelines and how a session falls back to readback.

# Architecture

The crate splits into a CEF-linked half and a CEF-free half, gated by the
`cef-runtime` Cargo feature (on by default):

- **CEF-free modules** (`cookies`, `event`, `frame`, `input`, `lifecycle`,
  `page_zoom`, `profile`, `url_input`, `element_picker`) contain only browser-neutral types, validation,
  and state machines. They compile and test without downloading Chromium
  (`cargo test -p zz-browser --no-default-features`).
- **`cef_runtime`** is the only module that links `cef`. It hosts
  `BrowserRuntime`, `BrowserSession`, `bootstrap`, `run_subprocess`, and every
  CEF handler (`cef::wrap_*!`) callback. Its module attribute locally allows
  `unsafe_code` solely to validate and copy CEF's callback-scoped BGRA paint
  buffer into owned memory; no borrowed CEF pointer survives that callback.

The one-slot mailbox model, whatever the tier: CEF hands over a callback-scoped
resource, `cef_runtime` copies it exactly once into something zz owns (a `Vec<u8>`
from `on_paint`, or a zz-owned destination texture/`IOSurface` from
`on_accelerated_paint`), and publishes that into a
[`FrameMailbox`](/browser/osr-rendering.md) holding at most one pending frame. A
new frame replaces a stale unread one, `take()` hands the consumer the owned
frame, and the app moves it onward without another pixel copy. A slow GPUI
consumer therefore never builds an unbounded queue; it always paints the newest
frame.

## No proxy plumbing

The crate sets no proxy preferences on any request context. It used to: a pane on
a remote session ran on a composite `<profile>@egress-<hash8>` profile whose
context carried a `fixed_servers` pref at a loopback tunnel. That whole path .
`egress_profile_name`, `ensure_egress_profile`, `set_profile_proxy`, and
`BrowserError::ProxyPreference` . was deleted on 2026-08-01 with the QUIC
transport it tunnelled over. A pane's traffic leaves from the client's own
network, and one profile name means one context and one jar. The protocol's
user-facing profile-name cap binds the daemon-owned
prefix alone.

# Public API surface

`lib.rs` re-exports the browser-neutral types (always available) and the CEF
runtime types (only under `cef-runtime`):

| Re-export | Module | Role |
| --- | --- | --- |
| `SessionId`, `BrowserEvent`, `BrowserCursor`, `ContextMenuRequest`, `EditFlags` | `event` | Owned browser-domain events, cursor shapes, and context-menu state |
| `OsrFrame`, `FrameMailbox`, `FrameError`, `FrameMailboxDiagnostics`, `FrameTier`, `OwnedBgraFrame`, `GpuFrame`, `BrowserGpuContext` (+ `MacGpuFrame`/`MacIoSurface` on macOS) | `frame` | Latest-frame mailbox and the three frame payloads |
| `Viewport`, `Modifiers`, `PointerEvent`, `WheelEvent`, `KeyInput`, `BrowserKey`, `KeyAction`, `EditCommand`, `PointerButton`, `PointerPhase` | `input` | Browser-neutral input + viewport types |
| `RuntimePhase`, `SessionPhase` | `lifecycle` | Runtime + session state machines |
| `BrowserProfilePaths`, `BrowserProfileError`, `resolve_profile_paths`, profile-name constants/normalization | `profile` + `zz-protocol` | Safe named-profile validation, path resolution, creation, and legacy aliasing |
| `resolve_address`, `SearchProvider`, `normalize_url`, `diagnostic_url`, `UrlInputError` | `url_input` | Address-field navigate-or-search, URL normalization + redaction |
| `parse_cookie_import`, `CookieImportBatch`, `CookieImportResult`, `SiteDataClearResult` | `cookies` | Bounded Cookie-Editor/Netscape parsing plus browser-data operation results |
| `BrowserBootstrap`, `BrowserRuntime`, `BrowserSession`, `BrowserError`, `RuntimeSignal`, `bootstrap`, `run_subprocess` (+ `bootstrap_windows`) | `cef_runtime` | CEF init, subprocess dispatch, session control |

# Key files

| File | Role |
| --- | --- |
| `src/lib.rs` | Crate root; module declarations and the public re-export surface. Gates `cef_runtime`/`element_picker` on the `cef-runtime` feature. |
| `src/cef_runtime.rs` | The only CEF-linked module: `BrowserRuntime`, `BrowserSession`, `bootstrap`/`run_subprocess`, all `cef::wrap_*!` handlers, `on_paint`, input dispatch. |
| `src/frame.rs` | The `OsrFrame` tier enum + one-slot `FrameMailbox` (latest-wins publish, wake coalescing, dimension/byte-length validation, GPU-import failure tracking). |
| `src/event.rs` | `SessionId`, `BrowserEvent` variants, `BrowserCursor`. |
| `src/input.rs` | Browser-neutral `Viewport`, packed `Modifiers`, pointer/wheel/key input types + `windows_key_code` mapping. |
| `src/lifecycle.rs` | `RuntimePhase` and `SessionPhase` strict transition tables. |
| `src/page_zoom.rs` | Chrome percentage ladder and logarithmic CEF zoom-level conversion. |
| `src/profile.rs` | `BrowserProfilePaths` + `resolve_profile_paths`; encoded named paths, per-platform data dir, user-only permissions. |
| `src/cookies.rs` | Cookie-Editor JSON/Netscape parsing, normalization, limits, and secret-free result types. |
| `src/url_input.rs` | `resolve_address` (address field: navigate or search), `normalize_url`, and `diagnostic_url` (credential/query redaction). |
| `src/element_picker.rs` | Token-guarded in-page element-picker protocol state (`ElementPickState`). |
| `assets/element-picker.js` | Injected renderer-side script backing the element picker (~45 KB). |
| `Cargo.toml` | Takes the workspace `cef` dependency (pinned in the root `Cargo.toml`, `accelerated_osr` feature) as an optional dep behind `cef-runtime`. |

# Module map (concept links)

- [CEF runtime & subprocess dispatch](/browser/cef-runtime.md) . `cef_runtime.rs` + `lib.rs`
- [Off-screen rendering pipeline](/browser/osr-rendering.md) . `frame.rs` + `on_paint`/DPI handling
- [Runtime & session lifecycle](/browser/lifecycle.md) . `lifecycle.rs` + `event.rs`
- [Named private profiles & request contexts](/browser/profile.md) . `profile.rs`
- [Input translation](/browser/input-translation.md) . `input.rs` + `url_input.rs`
- [Element picker](/browser/element-picker.md) . `element_picker.rs`

# Related

- Consumed by [`crates/zz`](/crates/zz.md), the GPUI mux client, which keys one
  CEF `BrowserSession` per browser pane ID and paints its frames via a custom GPUI
  element.
- CEF is pinned through `Cargo.lock`; the resolved versions and per-target archive
  hashes are recorded in `third_party/cef/ARTIFACTS.md`, described by
  [CEF artifacts](/references/cef-artifacts.md).
- Fits into the overall [process model](/architecture/process-model.md) and
  [data flow](/architecture/data-flow.md).
- Session restore behavior is described under
  [session persistence](/concepts/session-persistence.md).
