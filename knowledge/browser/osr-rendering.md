---
type: Concept
title: Off-screen rendering & the frame mailbox
description: How CEF frames cross the one-slot mailbox through the universal readback tier, Linux wgpu tier, macOS Metal-IOSurface tier, or Windows D3D11 tier, and how zz paces visible sessions.
resource: crates/zz-browser/src/frame.rs
tags: [browser, osr, frame, gpu, iosurface, hidpi, wayland, pacing]
timestamp: 2026-07-27T00:00:00Z
---

# Overview

zz embeds CEF as an off-screen-rendered browser rather than a native child
window. Every complete view paint enters the same latest-frame mailbox, but the
payload depends on the active rendering tier:

| Tier | Platforms | CEF callback | Mailbox payload | GPUI paint path |
| --- | --- | --- | --- | --- |
| Readback | Universal fallback | `on_paint` | `OsrFrame::OwnedBgra` with owned premultiplied BGRA bytes | `RenderImage` through `BrowserElement::paint_image` |
| Linux wgpu | Linux/FreeBSD where GPUI exposes its wgpu context | `on_accelerated_paint` | `OsrFrame::Gpu` with a zz-owned destination texture | GPUI `external_texture` |
| macOS Metal-IOSurface | macOS | `on_accelerated_paint` | `OsrFrame::MacGpu` with a retained zz-owned `IOSurface` | `CVPixelBuffer` through GPUI `paint_surface` |
| Windows D3D11 | Windows where GPUI exposes its DirectX context | `on_accelerated_paint` | `OsrFrame::WinGpu` with a zz-owned `ID3D11Texture2D` | GPUI `external_texture` |

All tiers are bounded by a one-slot `FrameMailbox`: a slow consumer cannot build
an unbounded queue, and a new frame replaces an unread stale frame. The readback
tier remains the fallback everywhere. GPU import, format, or wrapping failures
atomically recreate only the affected session with shared textures disabled
while the view retains its last valid surface.

# Tier pipelines

## Universal readback

```text
CEF on_paint (callback-scoped BGRA buffer)
  -> validate width × height × 4 and the 512 MiB safety limit
  -> copy once into an owned Vec<u8>
  -> FrameMailbox::publish -> OsrFrame::OwnedBgra
  -> controller moves the Vec into one RenderImage
  -> BrowserElement paints the shared Arc<RenderImage>
```

The callback buffer must be copied before `on_paint` returns. After that copy,
the allocation moves through the mailbox and image construction without another
CPU pixel copy. Replaced or retired readback images return their buffers to a
bounded three-entry recycle pool when ownership is unique.

## Linux wgpu

The Linux/FreeBSD tier imports CEF's external texture on GPUI's wgpu device,
blits it immediately into a small zz-owned texture pool, and publishes the
destination texture. `pool_generation` distinguishes resize generations and
`sequence` tracks accelerated callbacks. CEF may recycle its source after the
callback, so only the zz-owned copy crosses the mailbox.

The platform-specific DMA-BUF import, Wayland physical-OSR scale path, and GPUI
external-texture element are unchanged by the macOS tier.

## macOS Metal-IOSurface

`metal_osr.rs` owns a system `MTLDevice`, command queue, and five BGRA
IOSurface-backed destination textures. For each accelerated paint it:

1. validates BGRA metadata and the coded size;
2. retains CEF's callback-scoped `IOSurface` and wraps it as a source Metal
   texture;
3. blits into the next zz-owned pool surface;
4. attaches a command-buffer completion handler, commits the blit, and returns
   to CEF without waiting;
5. on the Metal queue thread, publishes the retained destination `IOSurface` as
   `OsrFrame::MacGpu` only if its sequence is newer than the last published
   completion.

The retained source remains alive until the asynchronous completion even though
macOS viz normally supplies fresh handles. At most two blits may be in flight;
new paints at the cap are skipped instead of queued. Five destinations reserve
three surfaces that GPUI's CAMetalLayer may still be sampling plus two producer
writes, and each slot carries an in-flight marker as a final guard against
out-of-order reuse. Command-buffer failures enter the normal
`record_gpu_import_failure` → readback-recreation path from the completion
thread.

The app caches a `CVPixelBuffer` wrapper per destination surface and pool
generation. GPUI imports that buffer on its own Metal device and samples it
through the fork's single-plane BGRA shader with premultiplied alpha and the
pane's corner mask. `IOSurface` is the cross-device sharing boundary, not
`MTLTexture`.

Unlike the Linux tier, the blit tier tracks **no CEF pool identity**: macOS viz
hands out fresh IOSurface handles continually, so slot bookkeeping churned a new
destination pool every few frames and grew retired-slot state without bound
(gradual frame-rate decay, `CVMetalTextureCache` accumulation on the GPUI side).
The only invalidation is dimension agreement with the viewport;
`pool_generation` increments only when the destination pool is (re)created for
a new size, keeping the app's `CVPixelBuffer` cache and GPUI's texture cache
pinned to five stable surfaces.

## Windows D3D11

`d3d11_osr.rs` mirrors the macOS split . native import, no wgpu, because
`gpui_windows` renders through DirectX . with one structural simplification:
producer and consumer share GPUI's single immediate context, so the copy is
synchronous and needs no completion handler, no in-flight cap, and no per-slot
marker. For each accelerated paint it:

1. validates the coded size against the viewport (device pixels: Windows uses
   the same default OSR path as macOS, logical `view_rect` plus a scaled
   `screen_info`, so a scaled display delivers scaled textures);
2. checks `GetDeviceRemovedReason` and opens CEF's shared NT handle on GPUI's
   `ID3D11Device1` with **`OpenSharedResource1`** . the legacy
   `OpenSharedResource` returns `E_INVALIDARG` for NT handles;
3. validates the imported texture's own dimensions and DXGI format
   (`B8G8R8A8_UNORM` or `R8G8B8A8_UNORM`; typeless and sRGB variants are
   rejected because GPUI builds its shader resource view with a null
   description);
4. `CopyResource`s into the next of five zz-owned pool textures and `Flush`es,
   then releases the imported interface . all before returning to CEF, which
   releases the shared texture back to its pool the moment the callback returns;
5. publishes the pooled texture as `OsrFrame::WinGpu`.

zz never closes the shared handle: CEF owns it and recycles the slot. The pool
is keyed by size and format, and `pool_generation` increments only when it is
rebuilt, exactly as on macOS. Every D3D11 type crossing the GPUI boundary comes
from `gpui::windows`, GPUI's own re-export of the `windows` crate . zz's
workspace pin is a different, type-incompatible version.

External BeginFrames are **not** wired on Windows; the tier runs on CEF's
internal cadence.

Accelerated-paint observation keeps raw handle identities and counters in
fixed-capacity state on the frame callback. It formats identity strings and
materializes the public diagnostic shape only when the trace target is enabled
or a diagnostics snapshot is requested; the 64-handle bound remains unchanged.

# Mailbox and frame schema

Every variant carries:

| Field | Meaning |
| --- | --- |
| `session` | Owning CEF session. |
| `generation` | Monotonic latest-frame mailbox generation. |
| `delivery_generation` | Generation of the active tier; changes when delivery transitions between readback and GPU. |

The GPU variants also carry logical and device dimensions, `pool_generation`,
and callback `sequence`. `GpuFrame` carries a wgpu texture; `MacGpuFrame`
carries a retained `MacIoSurface`; `WinGpuFrame` carries an `ID3D11Texture2D`
(free-threaded COM, so it needs no thread-safety wrapper of its own).
`OwnedBgraFrame` carries device dimensions and the owned pixel vector.

`FrameDeliveryState` records the active tier, transitions, pending fallback, and
GPU import failures. Mailbox diagnostics expose per-tier publish/take counts,
pending bytes/generation, pool metadata, and the wake-coalescing state.

The core mailbox operations are:

| Method | Behavior |
| --- | --- |
| `publish` | Validates and publishes an owned BGRA frame. |
| `publish_gpu` | Publishes a Linux/FreeBSD zz-owned wgpu texture after validating destination dimensions. |
| `publish_mac_gpu` | Publishes a macOS retained destination `IOSurface` after validating dimensions. |
| `publish_win_gpu` | Publishes a Windows zz-owned `ID3D11Texture2D` after validating dimensions. |
| `take` | Removes the newest frame and clears the pending wake. |
| `record_gpu_import_failure` | Keeps the active tier visible while marking atomic fallback pending. |
| `clear` | Drops the pending frame on crash or close. |

A publish emits `BrowserEvent::FrameReady` only when no consumer wake is already
pending. Coalesced frames still replace the slot, so the eventual consumer sees
the newest frame.

# Frame pacing

The paint ceiling is computed once when `BrowserController` starts. An explicit
`ZZ_BROWSER_FPS=1..240` wins; otherwise macOS uses the maximum
`NSScreen.maximumFramesPerSecond` across attached displays and other platforms
retain the 60 FPS default. Focused sessions use the ceiling. Visible unfocused
sessions normally use `min(ceiling, 30)`, but wheel input temporarily raises
them to the ceiling and restarts a one-second decay timer. Hidden panes call
`was_hidden` and do not paint.

Delivery is additionally pump-bound: accelerated frames only arrive during
`do_message_loop_work` turns, so the controller pumps at the ceiling's frame
interval while a visible session delivered a frame within the last 500 ms
(`PUMP_HOT_WINDOW`), decaying to the 30 Hz visible watchdog once frames stop.
See [cef-runtime](/browser/cef-runtime.md).

On macOS, external BeginFrames are the default
(`ZZ_BROWSER_EXTERNAL_BEGIN_FRAME=0` reverts to CEF's internal timer, which caps
shared-texture OSR at 60 FPS). Linux/FreeBSD retain their internal timer by
default and enable the same pump-driven scheduler only for exact
`ZZ_BROWSER_EXTERNAL_BEGIN_FRAME=1`:

- a frame or browser input makes a visible session hot for about 500 ms;
- BeginFrames are sent from the message-pump turn itself (immediately before
  `do_message_loop_work` processes them), decoupled from GPUI's render loop:
  riding `on_next_frame` pipelined production one GPUI frame behind consumption
  and halved the effective browser frame rate;
- each pane owns an anchored next deadline: hot and visibility edges send
  immediately, on-time turns advance by one interval, and late turns send once
  before skipping every missed interval without moving the anchor;
- pump scheduling takes the minimum of the ordinary fallback and the earliest
  pane deadline, so incidental or immediate turns may run early but cannot make
  the compositor clock jitter;
- hot sessions run at the frame-rate ceiling; with the `ZZ_BROWSER_BF_ADAPTIVE=1`
  opt-in they also compare roughly one second of BeginFrames with delivered
  `FrameReady` events. Delivery below 85% selects the next 2× interval tier,
  bounded at 30 FPS, while at least 95% delivery for about two seconds probes
  one tier faster; zero-delivery windows are ignored and going cold resets the
  tier to the ceiling;
- when cold, the visible pump watchdog's turns supply about 30 BeginFrames per
  second so CSS/JavaScript animations can discover work; this keepalive does not
  participate in adaptive sampling;
- hidden sessions receive no external BeginFrames.

The external BeginFrame scheduler controls Chromium compositor cadence. It is
separate from CEF's external message pump. Linux/FreeBSD still use a 60 FPS
display ceiling unless `ZZ_BROWSER_FPS` overrides it; no display refresh query
exists there yet. Adaptive tiers stay opt-in because delivery is demand-driven:
a scroll pause inside the hot window reads as a delivery shortfall and
downshifts a tier the renderer could sustain, which compounded into a one-way
ratchet to the 30 FPS floor when adaptation defaulted on.

Why a stable divisor beats a larger instantaneous frame count on a fixed refresh
grid is argued in [the CEF runtime's external BeginFrame
section](/browser/cef-runtime.md).

The optional `show-fps` badge counts fresh frames consumed by a
`BrowserView`; it does not report the configured ceiling. Static pages can
therefore read zero, and coalesced frames are not counted as displayed.

# Device scale factor / DPI handling

The default macOS/Windows/X11 path reports logical view bounds with the real
display scale in `screen_info`; CEF sizes the backing surface. Wayland instead
uses the existing physical-OSR path:

- `view_rect` returns `ceil(logical × scale_factor)`;
- `screen_info.device_scale_factor` is `1.0` to avoid double scaling;
- Chromium zoom includes the display scale;
- pointer, precise-wheel, and screen coordinates use the same physical scale.

`BrowserSession::apply_viewport` preserves the required ordering:
`notify_screen_info_changed`, then `was_resized`, then `invalidate(VIEW)`.
Creation forces this synchronization once so a late fractional scale cannot be
suppressed by equality checks.

# Key files

| File | Role |
| --- | --- |
| `crates/zz-browser/src/frame.rs` | Frame variants, delivery state, mailbox, recycling, and diagnostics. |
| `crates/zz-browser/src/cef_runtime.rs` | CEF readback/accelerated callbacks, pool generation, fallback, scale, and external BeginFrame host calls. |
| `crates/zz-browser/src/metal_osr.rs` | macOS Metal source import, synchronized blit, and destination `IOSurface` pool. |
| `crates/zz-browser/src/d3d11_osr.rs` | Windows shared-NT-handle import, in-callback `CopyResource`, and destination D3D11 texture pool. |
| `crates/zz/src/browser/controller.rs` | Tier decoding, frame-rate policy, fallback recreation, and external BeginFrame activity state. |
| `crates/zz/src/browser/view.rs` | Latest-frame consumption, retained surfaces, and hot/idle BeginFrame scheduling. |
| `crates/zz/src/browser/macos_surface.rs` | Cached `IOSurface` to `CVPixelBuffer` wrapping. |
| `crates/zz/src/browser/element.rs` | GPUI image, wgpu external-texture, and macOS surface painting. |

# Related

- [CEF runtime](/browser/cef-runtime.md)
- [NVIDIA Linux CEF accelerated OSR failure](/research/2026-08-07-nvidia-cef-accelerated-osr.md)
- [Browser lifecycle](/browser/lifecycle.md)
- [Input translation](/browser/input-translation.md)
- [GPUI revision](/references/gpui-revision.md)
- [Browser-core crate](/crates/zz-browser.md)
