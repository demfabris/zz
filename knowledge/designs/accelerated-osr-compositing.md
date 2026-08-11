---
type: Design Plan
title: Accelerated OSR compositing & external begin frames
description: The plan that replaced the CPU BGRA readback pipeline with CEF shared GPU textures and phase-locked Chromium to zz's frame clock via external BeginFrames; shipped on Linux, macOS, and Windows, with external BeginFrames still unsupported on Windows.
status: In Progress
tags:
- browser
- osr
- gpu
- shared-texture
- dmabuf
- begin-frame
- wgpu
- design-plan
timestamp: 2026-07-16T23:00:00Z
---

# Overview

> **Status: shipped through M6.** The shared-texture path is the default
> tier (the equivalent of `ZZ_BROWSER_GPU=1 ZZ_BROWSER_SHARED_TEXTURE=1`), with explicit `=0`
> opt-outs down to GPU-backed and then software readback. On Linux, CEF dmabuf frames are imported
> on GPUI's device via `osr_texture_import::SharedTextureHandle`, GPU-blitted into zz-owned textures
> (`wgpu::util::TextureBlitter`; the import lacks `COPY_SRC`), and painted through the carried
> `external_texture` element with zero CPU pixel copies. On macOS, `metal_osr.rs` blits each
> `AcceleratedPaintInfo` IOSurface into a five-slot destination pool through Metal, sized so a
> published surface is never re-blitted while GPUI's CAMetalLayer may still sample it. On Windows,
> `d3d11_osr.rs` opens the shared NT handle on GPUI's own `ID3D11Device` (`OpenSharedResource1`;
> the legacy entry point rejects NT handles) and `CopyResource`s it into a five-slot pool inside the
> paint callback, since CEF returns the slot to its pool the moment that callback returns.
>
> External BeginFrames drive the cadence: `external_begin_frame_enabled = 1` at browser creation,
> default-on for macOS with `ZZ_BROWSER_EXTERNAL_BEGIN_FRAME=0` as the kill switch, exact opt-in
> (`=1`) on Linux/FreeBSD, unsupported elsewhere. Sends ride the CEF message-pump turn rather than
> GPUI's `on_next_frame`, which pipelined production one frame behind consumption and halved the
> effective rate. Two milestone items did **not** land as written: `ZZ_BROWSER_FPS` and the
> frame-rate clamp still exist and still govern BeginFrame cadence (focus-aware, wheel-boosted, with
> a visible-idle keepalive), and the copy-on-receive blit is kept on both platforms rather than
> audited away. External BeginFrames remain unsupported on Windows, and M0's cross-driver matrix
> remains design intent; the Windows tier is written but has not yet run on real hardware.
>
> Shipped behavior belongs in [OSR rendering](/browser/osr-rendering.md) and
> [CEF runtime](/browser/cef-runtime.md); this plan keeps only the reasoning and what is still open.

The readback pipeline this plan displaced took a CPU round trip per frame: Chromium rendered on the
GPU, read the result back, and handed zz a BGRA byte buffer through `RenderHandler::on_paint`
([OSR rendering](/browser/osr-rendering.md)). zz copied those bytes once, moved them into a
`RenderImage`, and re-uploaded them to the GPU for compositing . roughly 2 GB/s of memory traffic at
4K/60 fps to move pixels that never needed to leave the GPU. It survives as the permanent fallback
tier.

The replacement is CEF's **accelerated OSR** (shared GPU textures), with Chromium's frame production
driven from zz's presentation clock by **external BeginFrames**:

```text
readback:    viz renders on GPU -> readback -> shmem -> on_paint(BGRA) -> Vec copy
             -> RenderImage -> GPUI texture upload -> composite
accelerated: viz renders on GPU -> GPU-GPU copy into shared image pool
             -> on_accelerated_paint(dmabuf fds / IOSurface / D3D handle)
             -> wgpu or Metal import (zero CPU bytes moved) -> composite
```

Two independent wins, one dependency chain:

1. **Shared textures make >60 fps affordable**: per-frame cost collapses from a multi-MB CPU
   round trip to handle passing plus one GPU-side copy inside CEF.
2. **External BeginFrames make every rate correct**: CEF's internal paint timer is free-running, not
   phase-locked to the display; even at a nominal 60 fps it beats against vsync (some frames shown
   twice, some never). Driving `send_external_begin_frame` on zz's own cadence makes Chromium raster
   on demand at a rate zz chooses per pane.

# Verified starting position

Every fact below was checked against the pinned dependencies on 2026-07-16.

| Fact | Where |
|------|-------|
| Readback path: `on_paint` -> one `Vec<u8>` copy -> one-slot `FrameMailbox` -> `Vec` moves into `RenderImage` (no further CPU copy) | [`zz-browser`](/crates/zz-browser.md) `frame.rs`, `cef_runtime.rs`; [OSR rendering](/browser/osr-rendering.md) |
| Frame-rate ceiling `DEFAULT_BROWSER_FRAME_RATE = 60`, `MAX_BROWSER_FRAME_RATE = 240`, `ZZ_BROWSER_FPS` override; dynamic per-focus throttle `UNFOCUSED_FRAME_RATE_CAP = 30`, hidden panes stop via `was_hidden` | `cef_runtime.rs:36`, `browser/controller.rs:23` in [`app`](/crates/zz.md) |
| **Chromium's GPU process and shared-texture OSR are enabled by default**; `ZZ_BROWSER_SHARED_TEXTURE=0` forces GPU-backed readback and `ZZ_BROWSER_GPU=0` forces software readback | `cef_runtime.rs` `browser_gpu_enabled()` / `default_enabled_env_flag()`; [CEF runtime](/browser/cef-runtime.md) |
| zz already drives CEF from GPUI's foreground thread (`external_message_pump: 1`, `on_schedule_message_pump_work`) | [CEF runtime](/browser/cef-runtime.md) |
| Pinned `cef` crate `150.0.0+150.0.10` ships an `accelerated_osr` cargo feature (deps: `ash`, `wgpu`, `objc2-io-surface`, `objc2-metal`, `windows`) and an `osr_texture_import` module with `dmabuf.rs`, `iosurface.rs`, `d3d11.rs` . `SharedTextureHandle::new(&AcceleratedPaintInfo).import_texture(&wgpu::Device)` returns a `wgpu::Texture` | crates.io `cef-150.0.0+150.0.10/src/osr_texture_import/`; [CEF artifact lock](/references/cef-artifacts.md) |
| sys bindings expose `cef_window_info_t::shared_texture_enabled`, `cef_window_info_t::external_begin_frame_enabled`, `cef_browser_host_t::send_external_begin_frame`, `cef_render_handler_t::on_accelerated_paint`, and `cef_accelerated_paint_info_t` with native-pixmap planes (fd/stride/offset/modifier) on Linux | `cef-dll-sys` bindings |
| `on_paint` is only invoked when `shared_texture_enabled` is 0 . the two delivery paths are mutually exclusive per browser | `cef-dll-sys` doc comment on `on_paint` |
| CEF 150 no longer documents a 60 fps maximum for `windowless_frame_rate` (min 1, default 30) | `cef-dll-sys` doc comment |
| The [GPUI pin](/references/gpui-revision.md)'s Linux renderer is `gpui_wgpu::WgpuRenderer` (Wayland and X11) on **wgpu 29.0.4** . the same wgpu major the `cef` crate's import helper targets, and `Cargo.lock` resolves a single `wgpu 29.0.4` | Zed checkout `crates/gpui_linux/src/linux/wayland/window.rs`, workspace `Cargo.toml`, zz `Cargo.lock` |

The wgpu alignment is the load-bearing discovery: the import glue is **type-compatible with
GPUI's renderer out of the box**. No hand-rolled Vulkan external-memory code, no blade fork:
the carried GPUI patch shrinks to "expose the `wgpu::Device`/`Queue` and let a custom element
paint an externally imported `wgpu::Texture`".

# Target architecture

## Shared-texture frame path

- Browsers use `shared_texture_enabled = 1` by default at creation; `on_accelerated_paint` replaces
  `on_paint` for that browser (CEF enforces the exclusivity).
- The one-slot mailbox keeps its latest-wins semantics but the slot's payload changes from an
  owned BGRA `Vec<u8>` to a **shared-texture handle + pool-slot identity** (CEF rotates a small
  pool of shared images; slots are tracked by handle identity so a stale slot is never sampled
  after its next paint).
- **v1 sync policy: copy-on-receive.** On arrival, import the handle via the `cef` crate helper
  and immediately GPU-blit into a zz-owned `wgpu::Texture`, then release the pool slot. Still
  zero CPU bytes moved; the blit is microseconds of GPU time and sidesteps the API's lack of an
  explicit fence (see risks). A later milestone attempts direct sampling under implicit sync.
- The BGRA `on_paint` path is **retained as the fallback tier**, selected at browser creation
  and re-selectable at runtime: Chromium silently reverts to software compositing on drivers/VMs
  where GPU compositing is unavailable, and zz must render correctly there.

## External BeginFrames

- `external_begin_frame_enabled = 1` at browser creation, and `send_external_begin_frame` per pane
  from the [external message pump](/browser/cef-runtime.md) turn. Riding GPUI's `on_next_frame`
  instead was tried and rejected: it pipelined production one GPUI frame behind consumption and
  halved the effective browser rate, whereas the pump already runs at the frame interval while
  frames flow and the queued sends are picked up by the `do_message_loop_work` that follows.
- Chromium's internal OSR scheduler steps aside: page `requestAnimationFrame` and CSS animations
  tick at the fed cadence.
- The focus throttle is expressed as **BeginFrame cadence** rather than only
  `set_windowless_frame_rate`: a hot pane follows its focus-aware (and wheel-boosted) rate, a
  visible-idle pane gets a ~30 Hz keepalive so JS/CSS animation work can be discovered, and a hidden
  or non-ready pane gets none. `ZZ_BROWSER_FPS` and the `MAX_BROWSER_FRAME_RATE` clamp survive as the
  ceiling that cadence is computed against; an opt-in adaptive divisor
  (`ZZ_BROWSER_BF_ADAPTIVE=1`) backs off when delivery falls behind, still opt-in because
  demand-driven production makes a scroll pause look like a delivery shortfall.

## GPUI carried patch

Two small patches carried on the existing [GPUI pin](/references/gpui-revision.md):

1. Expose the renderer's `wgpu::Device`/`Queue` to the embedding app.
2. A paint primitive (or extension of the existing image element) that samples an
   externally provided `wgpu::Texture` with normal clipping; the browser pane element swaps
   its `RenderImage` for that texture.

# Milestones

Ordered so each ships something usable and de-risks the next.

| # | Milestone | Scope | Exit criterion |
|---|-----------|-------|----------------|
| M0 (open) | GPU-process-on tier is trustworthy | Validate `ZZ_BROWSER_GPU=1` (readback + GPU compositing) on Wayland and X11, Mesa (Intel/AMD) and NVIDIA . shared textures require viz GPU compositing | Browser fixture parity with the software path; WebGL/canvas/video accelerated; failure modes catalogued. NVIDIA catalogued 2026-08-06: the CEF 151 GL capture path produced zero paint callbacks and a silent blank pane. Shared textures remain the default, and a visible Linux pane that produces no first frame within two seconds falls back by itself. Skia-Vulkan delivered fixture frames but failed real-app resize stress under the unsupported Ozone/Wayland combination. The [NVIDIA accelerated OSR investigation](/research/2026-08-07-nvidia-cef-accelerated-osr.md) traces the failure to CEF's mappable shared-image preference. Revisit when CEF's OSR consumer requests Chromium's native-handle preference. |
| M1 (shipped) | Accelerated-paint spike | Enable the `cef` crate's `accelerated_osr` feature and `shared_texture_enabled` in `zz_browser_fixture`; log `AcceleratedPaintInfo` contents and pool behavior on real drivers | dmabuf frames observed on Linux; pool size and slot-reuse cadence documented |
| M2 (shipped) | GPUI external-texture element | The two carried patches above, proven in isolation | An app-provided `wgpu::Texture` painted by a GPUI element under clipping, HiDPI-correct |
| M3 (shipped) | End-to-end shared-texture pane | Handle-carrying mailbox variant, import via `osr_texture_import`, copy-on-receive blit, runtime fallback to `on_paint` | Browser pane renders with zero CPU pixel copies; readback tier still selectable and correct |
| M4 (shipped, scope trimmed) | External BeginFrames | `external_begin_frame_enabled`, pump-driven sends, focus-aware cadence with a visible-idle keepalive. `ZZ_BROWSER_FPS` and the clamp were **kept** as the ceiling cadence is computed against | Measured on a signed macOS bundle: 30.07 presents/s under the unfocused cap, 46% lower steady-state CPU |
| M5 (resolved: blit kept) | Sync tightening | `AcceleratedPaintInfo` carries no fence, so every platform keeps copy-on-receive: a wgpu blit on Linux, a Metal blit into a five-slot IOSurface pool on macOS, a `CopyResource` + `Flush` into a five-slot D3D11 pool on Windows | No tearing under scroll/animation stress |
| M6 (shipped, unproven on Windows hardware) | macOS & Windows tiers | `metal_osr.rs` imports the IOSurface and blits through Metal, bypassing wgpu as gpui_macos is Metal-native; `d3d11_osr.rs` does the same for D3D11 shared handles, bypassing wgpu as gpui_windows is DirectX-native | Fixture parity per platform |

M0 stays deliberately unglamorous: the default-on path still wants its cross-driver matrix, while the
permanent atomic readback fallback limits a failure to one pane recreation instead of lost content.

# Hard parts & risks

| Risk | Detail | Mitigation |
|------|--------|------------|
| Silent driver fallback | On GPU-compositing-unavailable hosts (VMs, software GL), CEF reverts to `on_paint` | Both handlers implemented; runtime tier detection; the existing readback path remains the permanent degraded tier |
| No explicit fence | `AcceleratedPaintInfo` carries no sync fd; sampling a pool texture while viz writes it tears | v1 copy-on-receive blit; rely on dmabuf implicit sync where honored; M5 revisits |
| Pool-slot reuse | Holding an imported texture past the slot's next paint shows stale/torn content | Track slots by handle identity; release on blit; never sample after release |
| Modifier negotiation | dmabufs arrive with vendor tiling (Intel Y-tile, AMD DCC); NVIDIA proprietary support varies by driver generation | wgpu/Vulkan `VK_EXT_image_drm_format_modifier` via the crate helper; fall back to readback tier on import failure |
| wgpu version lockstep | The `cef` crate and the GPUI pin must resolve to the same wgpu major or the import type-compatibility breaks (Windows sidesteps this: neither GPUI nor the D3D11 tier uses wgpu there, but the same rule applies to the `windows` crate version, which is why the tier codes against `gpui::windows`) | Both at wgpu 30 today; add a CI assertion; coordinate bumps with [the CEF update playbook](/playbooks/updating-cef.md) and the [GPUI pin](/references/gpui-revision.md) |
| Carried GPUI patches | Two patches ride every GPUI pin bump | Keep them minimal and documented in [GPUI revision pin](/references/gpui-revision.md); consider upstreaming the external-texture element to Zed |
| Popup widget textures | Dropdowns/selects arrive as a second `PaintElementType`; today non-VIEW paints are dropped | Same policy initially; a popup-surface slot is follow-up work either way |
| Resize races | Shared-texture dims lag pane dims for a frame or two during resize | Same latest-wins + dimension-validation discipline the mailbox already applies |
| HiDPI | Device-scale handling that keeps Wayland sharp today must carry over to imported textures | The [OSR rendering](/browser/osr-rendering.md) DPI rules apply unchanged . frames stay in device pixels |
| Power | Feeding 144 Hz begin frames to every pane costs battery | Begin-frame division by focus/visibility is the same lever, applied at the source |

# Non-goals

- **True zero-copy**: eliminating CEF's internal GPU copy into the shared-image pool requires
  patching `libcef` (a from-source CEF/Chromium build). The copy costs microseconds; not worth
  the build-farm tax at this stage.
- **Delegated compositing**: having Chromium hand over individual layers/quads for placement in
  GPUI's scene graph. Same fork requirement, strictly harder.
- **Video frame-rate magic**: a 30 fps video stays 30 fps; only compositing cadence improves.
- **Removing the explicit readback opt-outs**: both GPU-backed and software readback remain
  available for driver troubleshooting.

# Open questions

- The Windows D3D11 tier took the macOS platform-split shape (native import, no wgpu, five-slot
  pool). It has only been type-checked cross-compiled . does it hold up on real drivers, and does
  the per-callback `OpenSharedResource1` need the cefclient-style cache of opened slots?
- Should external BeginFrames be extended to Windows, where they are unsupported today?
- Can the adaptive BeginFrame divisor tell "renderer missed BeginFrames" from "page had nothing to
  draw"? Until it can, `ZZ_BROWSER_BF_ADAPTIVE` stays opt-in.
- Should external BeginFrames become the default on Linux/FreeBSD, where they are exact opt-in today?
- Do the carried GPUI patches get upstreamed to Zed (an external-texture element is broadly
  useful for video/embedding), and does that remove the lockstep risk entirely?
- Where does per-pane opt-out live if a specific site misbehaves under accelerated paint:
  [app config](/configuration/app-config.md) or a runtime toggle?

# Related

- [Off-screen rendering & the frame mailbox](/browser/osr-rendering.md) . the pipeline this plan
  replaces (and keeps as fallback)
- [CEF runtime & subprocess dispatch](/browser/cef-runtime.md) . the external message pump the
  begin-frame forwarding rides on; the GPU-process switch this plan mainlines
- [`zz-browser`](/crates/zz-browser.md) and [`app`](/crates/zz.md) . the crates the mailbox
  and element changes land in
- [End-to-end data flow](/architecture/data-flow.md) . the frame paths this plan re-routes
- [GPUI revision pin](/references/gpui-revision.md) and [CEF artifact lock](/references/cef-artifacts.md)
  . the two pins whose lockstep (wgpu 29) this plan depends on
- [Updating the CEF pin](/playbooks/updating-cef.md) . bump procedure that must now also check
  wgpu alignment
- [Scene-streaming remote attach](/designs/scene-streaming-remote.md) . sibling design plan; its
  local-renderer model composes with this one (remote panes render through the same accelerated
  local path)
