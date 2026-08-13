---
type: Research Report
title: NVIDIA Linux CEF accelerated OSR failure
description: Root-cause analysis of CEF 151 producing no accelerated OSR frames on NVIDIA Linux despite a complete EGL, GBM, DMA-BUF, and Vulkan stack.
tags:
- browser
- cef
- nvidia
- linux
- osr
- dmabuf
- gbm
timestamp: 2026-08-07T10:24:50Z
---

# Overview

On 2026-08-06, a CachyOS Wayland host with an NVIDIA RTX 3070 Laptop GPU
reproduced a blank CEF browser pane while zz used accelerated off-screen
rendering (OSR). Chromium loaded the page and reported its title, but CEF sent
neither `on_accelerated_paint` nor `on_paint`. The GPU process logged
`Unable to initialize SkSurface` for each compositor attempt.

The host has the libraries, kernel interfaces, and driver extensions required
for DMA-BUF rendering. The failure starts inside CEF's video-capture-backed OSR
producer. CEF asks Chromium for a CPU-mappable shared image, which makes
Chromium request a linear GBM allocation. NVIDIA's GBM path cannot use that
allocation as the Skia render target. Chromium added a native-handle preference
for this case in 2025, but CEF 151 and CEF master still request the older
mappable preference as of 2026-08-07.[1][2][3]

The NVIDIA GPU supports the intended GPU-only pipeline. CEF requests the wrong
buffer contract for it.

# Findings

| Question | Finding | Confidence |
| --- | --- | --- |
| Does the host lack an NVIDIA EGL, GBM, DRM, DMA-BUF, or Vulkan component? | No missing prerequisite appeared in the package, extension, device-node, or live-process audit. | High |
| Does zz default to XWayland on this NVIDIA system? | No. Chromium selected native Wayland and its child command lines carried `--ozone-platform=wayland`. | High |
| Can the NVIDIA GPU support exportable GPU images? | Yes. EGL exposes DMA-BUF import and modifiers; Vulkan exposes DMA-BUF external memory and DRM format modifiers. | High |
| Why does AMD render through the same CEF API? | Mesa's AMD GBM implementation can satisfy the linear, CPU-mappable request that fails on this NVIDIA path. | High |
| Do `use-angle` and `ozone-platform` solve the NVIDIA failure? | They configure the GL and platform backend. They cannot change CEF's shared-image preference, so they do not remove the linear allocation. | High |
| Does zz need the readback guard after a CEF fix? | Yes, until the patched path passes the cross-driver and resize matrix. Drivers and virtual machines can fail for other reasons. | Medium |

# Test environment

| Component | Observed value |
| --- | --- |
| Distribution and session | CachyOS, KDE Wayland |
| Kernel | `7.1.6-1-cachyos-deckify` |
| GPU | NVIDIA GeForce RTX 3070 Laptop GPU, PCI `10de:24dd` |
| NVIDIA kernel and userspace | `610.57.04` |
| CEF | `151.3.14`, commit `5d67476` |
| Chromium embedded by CEF | `151.0.7922.72` |
| Mesa / libgbm | `26.1.6` |
| EGL GBM platform | `egl-gbm 1.1.3` |
| EGL Wayland platform | `egl-wayland 1.1.21` |
| libdrm | `2.4.134` |

The CEF version comes from `CEF_VERSION` in the installed
`cef-dll-sys-151.2.0+151.3.14` bindings. The package versions and live process
inspection describe this host on 2026-08-06.

# Reproduction evidence

The shared-texture fixture and the full application showed the same producer
failure:

1. Chromium loaded the page. The full application received the page title, and
   the fixture server received the document and favicon requests.
2. Chromium's GPU process attempted to draw each compositor frame and logged
   `gpu/command_buffer/service/shared_image/shared_image_representation.cc:438`
   with `Unable to initialize SkSurface`.
3. The fixture reported zero accelerated callbacks and zero readback callbacks.
   It also reported zero zz GPU-import attempts because CEF supplied no native
   handle to import.
4. Setting `ZZ_BROWSER_SHARED_TEXTURE=0` restored visible, animated output.

The failure occurs before zz's Linux wgpu importer. Import fallback can react to
an `on_accelerated_paint` handle that wgpu rejects; it cannot react when CEF
produces no callback. The first-frame guard documented in
[CEF runtime](/browser/cef-runtime.md) covers that silent state.

Several controls did not change the result:

- The fixture failed with MangoHud removed from its environment.
- Forcing NVIDIA's GLVND vendor JSON, `GBM_BACKEND=nvidia-drm`, and the NVIDIA
  GLX vendor still produced zero callbacks.
- The CEF GPU process mapped NVIDIA EGL, GLX, allocator, EGL-GBM, EGL-Wayland,
  and GBM libraries and opened `/dev/dri/renderD128`.
- `modetest -M nvidia-drm` opened the driver and enumerated the active display.

These controls exclude MangoHud injection, GLVND vendor ambiguity, device
permissions, and a missing KMS setup as causes.

# Host capability audit

NVIDIA documents four Linux GBM prerequisites: DRM KMS, Mesa libgbm 21.2 or
newer, egl-wayland 1.1.8 or newer, and egl-wayland 1.1.9 or newer for Xwayland's
`wl_drm` path.[4] This host exceeds each version requirement and runs KMS.

NVIDIA EGL advertises the interfaces zz and Chromium need for native image
exchange:

- `EGL_EXT_image_dma_buf_import`
- `EGL_EXT_image_dma_buf_import_modifiers`
- `EGL_ANDROID_native_fence_sync`
- `EGL_KHR_fence_sync` and `EGL_KHR_wait_sync`
- `EGL_MESA_image_dma_buf_export`

The Wayland compositor advertises `zwp_linux_dmabuf_v1` version 5, explicit DRM
sync objects, linear modifiers, and NVIDIA block-linear modifiers for common
ARGB/XRGB formats. The NVIDIA Vulkan device advertises
`VK_EXT_external_memory_dma_buf`, `VK_EXT_image_drm_format_modifier`,
`VK_EXT_physical_device_drm`, and `VK_KHR_external_memory_fd`.

The audit therefore found a working native-handle transport. It did not find a
host package that would make a linear CPU-mappable allocation renderable.

# CEF and Chromium allocation path

CEF's `CefVideoConsumerOSR::SetActive` chooses
`kPreferMappableSharedImage` whenever a window enables shared textures.[1]
Chromium 151 converts that preference into this allocation chain:[3][5][6][7]

```text
CefVideoConsumerOSR::SetActive
  -> BufferFormatPreference::kPreferMappableSharedImage
  -> requires_cpu_access = true
  -> gfx::BufferUsage::SCANOUT_CPU_READ_WRITE
  -> GBM_BO_USE_LINEAR | GBM_BO_USE_SCANOUT | GBM_BO_USE_TEXTURING
  -> Skia BeginWriteAccess cannot create an SkSurface on NVIDIA
  -> CEF receives no captured frame
  -> zz receives no paint callback
```

Chromium's `ui/gfx/linux/gbm_util.cc` maps
`SCANOUT_CPU_READ_WRITE` to a linear allocation and omits
`GBM_BO_USE_RENDERING`. The same file maps `SCANOUT` to
`GBM_BO_USE_RENDERING | GBM_BO_USE_SCANOUT | GBM_BO_USE_TEXTURING`.[7]

Chromium commit `a531c83` added
`kPreferSharedImageWithNativeHandle` after Chromium developers traced CEF
shared-texture and video-capture failures on NVIDIA to
`SCANOUT_CPU_READ_WRITE`. The new preference sets `requires_cpu_access = false`
and selects `SCANOUT`, avoiding `GBM_BO_USE_LINEAR` while preserving an
exportable native handle.[3]

CEF has not adopted that preference. The installed CEF commit and the CEF
master snapshot checked on 2026-08-07 both pass
`kPreferMappableSharedImage`.[1][2] The public CEF OSR API exposes
`shared_texture_enabled`, but it does not let zz select Chromium's internal
`BufferFormatPreference`.

# GL and Ozone flags

CEF issue 3953 records two Linux OSR requirements.[8] Chromium needs
`--use-angle=gl-egl` on the documented GL route so its shared-image backing can
produce a GL Ozone representation. CEF's cefclient sample also selects
`--ozone-platform=x11` on Linux.[9] Libcef does not add those sample switches
for an embedder.

The flags address a different stage from the buffer allocation:

```text
ANGLE/Ozone setup
  -> can Chromium create the GL shared-image representation?

CEF buffer preference
  -> can Chromium allocate a renderable image for capture?
```

A missing GL Ozone representation produces a
`SharedImageManager::ProduceSkia` error about an incompatible backing. The local
native-Wayland run reached the later `shared_image_representation.cc` failure:
Chromium created the representation, then failed to initialize its SkSurface
during write access. That later signature agrees with the non-renderable linear
allocation. The log line alone cannot prove the allocation choice, but the
source trace, host audit, and controlled runs point to the same stage.

CEF's cefclient change says NVIDIA also needs an additional Chromium patch.[9]
Changing ANGLE or Ozone cannot select
`kPreferSharedImageWithNativeHandle`, so flags alone cannot repair this CEF 151
path.

One Vulkan experiment produced accelerated callbacks on native Wayland after
enabling Chromium's Vulkan feature. Chromium warned that Vulkan and Ozone
Wayland form an unsupported combination, and the full application failed resize
stress with that setup. The experiment proves that the GPU can produce and
export frames; it does not provide a production configuration.

# Why AMD works

CEF gives AMD and NVIDIA the same CPU-mappable request. Mesa's AMD allocator can
return a buffer that Chromium renders and exports under that request. NVIDIA's
GBM path cannot make this linear allocation serve as the Skia render target on
the tested driver.

The difference concerns one memory-layout contract, not shader, compositor, or
DMA-BUF capability. NVIDIA exposes tiled block-linear modifiers and native
external-memory extensions. Chromium's native-handle preference lets the
driver choose that GPU-oriented layout and removes the CPU-mapping constraint
that CEF does not use.

With that preference, NVIDIA follows the same class of pipeline as AMD:

```text
Chromium GPU render
  -> exportable DMA-BUF with a native modifier
  -> zz imports it on GPUI's wgpu device
  -> GPU blit into a zz-owned texture
  -> GPUI composition
```

No pixel buffer crosses through system memory in this path. AMD and NVIDIA may
show different timings, so the cross-driver fixture must measure throughput and
resize behavior before zz claims performance parity.

# Root fix and acceptance test

The root fix belongs in libcef:

1. Change `CefVideoConsumerOSR::SetActive` to request
   `kPreferSharedImageWithNativeHandle` for shared-texture OSR on Linux.
2. Apply the GL-EGL and supported Ozone setup required by the chosen CEF Linux
   route. Test X11 as the upstream baseline and native Wayland as a separate
   backend.
3. Build a CEF artifact and update it through the
   [CEF update playbook](/playbooks/updating-cef.md).
4. Run `zz_browser_fixture` and require accelerated callbacks, successful wgpu
   imports, animated frame changes, and no first-frame readback recreation.
5. Stress resize, video, WebGL, popups, multiple panes, hide/show transitions,
   and shutdown on NVIDIA and Mesa hardware.

The source diagnosis has one proof gap: the investigation has not run an A/B
build of libcef with the preference changed. That build provides the final
end-to-end confirmation. Until it passes the matrix, zz should keep the
per-pane guard as a recovery path.

# Current zz containment

Shared textures remain the default on every GPU. On Linux, zz gives each visible
shared-texture session two seconds to deliver its first frame. A session that
delivers no frame gets recreated with shared textures disabled while Chromium's
GPU process stays enabled.

The fallback does not mean that zz renders all browser content on the CPU.
Chromium may still use the GPU for page rendering, WebGL, canvas, and video, but
CEF reads the composed frame into CPU-visible BGRA memory. zz then uploads that
frame to GPUI. This tier moves far more memory than the DMA-BUF path described in
[OSR rendering](/browser/osr-rendering.md), so it serves as containment rather
than the NVIDIA performance solution.

# Related

- [CEF runtime and subprocess dispatch](/browser/cef-runtime.md)
- [Off-screen rendering and the frame mailbox](/browser/osr-rendering.md)
- [Accelerated OSR compositing design](/designs/accelerated-osr-compositing.md)
- [Running zz](/playbooks/running-zz.md)
- [CEF artifact lock](/references/cef-artifacts.md)

# Citations

1. [CEF 151 `video_consumer_osr.cc` at the installed commit](https://github.com/chromiumembedded/cef/blob/5d67476b12f718c8388918d1740aeec27f6b2b80/libcef/browser/osr/video_consumer_osr.cc), accessed 2026-08-06.
2. [CEF master `video_consumer_osr.cc` snapshot](https://github.com/chromiumembedded/cef/blob/57a32eaf2317f67982563a263c126c96bd034f88/libcef/browser/osr/video_consumer_osr.cc#L49-L55), accessed 2026-08-06.
3. [Chromium commit `a531c83`: prefer shared images with native handles](https://chromium.googlesource.com/chromium/src/+/a531c83a9bbb552fa13ceeaf73d0a19d1203cc12), 2025-08-06.
4. [NVIDIA 610.57.04 GBM documentation](https://download.nvidia.com/XFree86/Linux-x86_64/610.57.04/README/gbm.html), accessed 2026-08-06.
5. [Chromium 151 mappable shared-image video-frame pool](https://chromium.googlesource.com/chromium/src/+/refs/tags/151.0.7922.76/components/viz/service/frame_sinks/video_capture/mappable_shared_image_video_frame_pool.cc), accessed 2026-08-06.
6. [Chromium 151 renderable mappable shared-image video-frame pool](https://chromium.googlesource.com/chromium/src/+/refs/tags/151.0.7922.76/media/video/renderable_mappable_shared_image_video_frame_pool.cc), accessed 2026-08-06.
7. [Chromium 151 Linux GBM usage mapping](https://chromium.googlesource.com/chromium/src/+/refs/tags/151.0.7922.76/ui/gfx/linux/gbm_util.cc), accessed 2026-08-06.
8. [CEF issue 3953: shared texture support for Linux](https://github.com/chromiumembedded/cef/issues/3953), accessed 2026-08-06.
9. [CEF cefclient Linux shared-texture flags](https://github.com/chromiumembedded/cef/commit/cf5fddcb6dbff93d2d253deda8243aa37bbc39bb), 2025-07-10.
