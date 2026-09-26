---
type: Reference
title: GPUI revision pin
description: Where the patched Zed revision zz builds against is defined, how to read it, and what the carried GPUI patches do. gpui-component is not a dependency.
resource: Cargo.toml
tags: [gpui, zed, pin, reference, git-dependency]
timestamp: 2026-09-25T00:00:00Z
---

# Overview

zz's GPUI application layer comes from the `demfabris/zed` `zz-patches` branch rather than a
published crate. The desktop and browser clients pin the same published fork revision through
their manifests and lockfiles.
The iOS platform crate pins `gpui_wgpu` directly to the same fork revision.
On Linux, `gpui_platform` is built with `font-kit`, Wayland, and X11 enabled; the same crate
selects the native macOS and Windows backends automatically.

**Do not read a revision out of this document.** The normal pin lives in three manifests and
two lockfiles, which must agree outside a local experiment:

| Place | Role |
| --- | --- |
| `Cargo.toml`, `[patch."https://github.com/zed-industries/zed"]` | The `rev = "…"` on `gpui` and `gpui_platform`. This is the authority . editing it is how the pin moves. |
| `clients/web/Cargo.toml` | The browser client's independent workspace patch. Keep it on the desktop revision. |
| `crates/zz-gpui-ios/Cargo.toml` | The direct `gpui_wgpu` fork dependency. Keep its `rev` aligned; it bypasses the upstream patch table. |
| `Cargo.lock` and `clients/web/Cargo.lock` | The resolved `source = "git+https://github.com/demfabris/zed?rev=…"` for normal Git pins. Regenerated, never hand-edited. |

The appearance diagnostics log line no longer holds a third copy to keep in sync:
`crates/zz/build.rs` reads the resolved source out of `Cargo.lock` and stamps it into
`ZZ_GPUI_SOURCE`, which `crates/zz/src/lib.rs` prints as `GPUI_SOURCE`. To read the pin rather than
trust this document:

```bash
rg 'demfabris/zed|zz-forks/zed' Cargo.toml Cargo.lock clients/web/{Cargo.toml,Cargo.lock} crates/zz-gpui-ios/Cargo.toml
```

The fork itself is declared in `scripts/forks.conf` (`zed  zed-industries/zed  demfabris/zed
zz-patches  main  gpui,gpui_platform`), which is what `just forks` and `just fork-rebase zed` read.

**`gpui-component` is not a dependency.** It was forked into `crates/zz-ui` (`zz-ui`) and both
`gpui-component` and `gpui-component-assets` are gone from the workspace and its lockfiles; nothing
outside `gpui` itself is left. The fork's source revision and per-module port notes live in
`crates/zz-ui/UPSTREAM.md`, not here.

# Pane renderer changes

Fork commit `37d0b352ed` carries the pane renderer changes. Metal, WGPU, and DirectX blurred inset shadows
use position-seeded stochastic alpha rounding in 1/128 steps to reduce banding. WGPU applies
this before any required premultiplication. Dithering changes only alpha.
The GPU test `faint_inset_shadows_dither_dark_composites` checks variation, noise size,
brightness, and opaque composition. Both workspaces resolve these changes
through their shared Git revision pin.

Blurred shadows also follow the element's superellipse cross-section in Metal, WGPU, and
DirectX. The earlier carried smoothing fix covered only unblurred shadows, leaving the 2px
pane shadow tracing a circle around a squircle. The GPU test
`blurred_shadows_follow_smoothed_corners` checks all four corners and the circular and
fully rounded cases. The Gaussian blur is unchanged.

Pane background opacity also requires source-over destination alpha in Metal
and DirectX color/path pipelines and WGPU path composition. The Metal GPU test
`translucent_layers_preserve_source_over_alpha` checks the root, one and two half-opacity
layers, and an opaque foreground. This prevents translucent layers from adding up to
opaque window alpha.

# Carried patches

Fork commit `7bdd43258b` gives the native macOS Window menu first chance to handle
`performKeyEquivalent:` in `gpui_macos/src/window.rs` `handle_key_equivalent`.
Users can tile the app with their macOS keyboard shortcuts while a terminal has focus.
Unmatched keys continue through GPUI's existing input path. The menu call precedes
the window-state lock and GPUI event callback because menu validation can call back
into GPUI.

Each is upstream-able as a small Zed PR; if Zed merges an equivalent, drop it. In branch order:

1. `RenderImage::into_frames` for OSR frame reclamation.
2. Linux `WgpuDeviceContext` access.
3. Linux wgpu external-texture painting.
4. Scene-wide rounded window clipping (`Window::set_window_corner_mask`).
5. Terminal glyph render effects.
6. CoreGraphics stroke API compatibility.
7. Device-pixel-sized synthetic bold after the Retina transform.
8. macOS BGRA CoreVideo surface sampling with premultiplied alpha and element-level corner masking.
9. `PlatformDisplay::refresh_rate()` on Wayland, for the browser frame-rate ceiling.
10. `TestWindow` raw handles returning `HandleError::Unavailable` instead of panicking.
11. Pointer-transparent drag previews.
12. Keeping the active drag alive when `can_drop` rejects.
13. `TextSystem::underline_thickness(font_id, font_size)`, mirroring the existing `line_gap`
    accessor. `FontMetrics` already carried the metric but only behind the private `read_metrics`,
    so terminal box geometry had no way to match Ghostty's
    `box_thickness = max(1, ceil(underline_thickness))`. See
    [rendering parity](/terminal/rendering-parity.md).
14. `ShapedLine` glyph raster data caching.
15. Writing image and text pasteboard flavors together on macOS.
16. Reading image flavors riding alongside text on macOS.
17. Optional traffic light scale in `TitlebarOptions`.
18. CSS-correct drop shadows: spread dilates the shadow's corner radii along with its bounds.
19. Superellipse corner smoothing for quads: `PaintQuad::corner_smoothing` plus a window-wide
    default (`Window::set_default_corner_smoothing`; 2 = circular, 4 = squircle), which zz pins
    to 4 in `theme::CORNER_SMOOTHING`. Later carried patches apply the window's curve to
    sprite, surface, and window masks, and styled radii to external textures.
20. Two corrections to that smoothing: a quad whose radius reaches half its shorter side is a
    circle or a pill, so it keeps true arcs (this is also what `rounded_full` clamps to);
    and `Shadow` carries the smoothing of the element it traces, honored on the unblurred
    path so a hairline spread ring stops detaching from a squircle edge at the corners.
21. `Window::set_adaptive_corner_fraction`: resolve an ordinary corner radius against the element
    it rounds, in `Style::paint` where requested radii meet laid-out bounds. Approaches a fraction
    of the shorter side along `cap * tanh(radius / cap)` instead of being clamped at half, so one
    global radius setting keeps ordinary corners below that fraction. At high requested values,
    small radius changes become visually compressed. `FULL_CORNER_RADIUS` (what `rounded_full` sets) is exempt
    and still resolves to exactly half, making a pill something a widget declares.
    Per-element `CornerRadiusMode::Fixed` bypasses adaptive compression and uses the normal
    geometric clamp. `Styled::corner_smoothing` overrides the contour for an element's fill,
    border, and shadows without changing descendants. PopupMenu rows use these native
    controls for circular corners with a separate non-pill radius cap.
22. Building the wgpu renderer against wgpu 30: `Queue::present()` replaces
    `SurfaceTexture::present()`, and `SurfaceConfiguration` gains `color_space:
    SurfaceColorSpace::Auto` (wgpu 29's behaviour). Carried because the `cef` crate moved its
    `accelerated_osr` texture importer to wgpu 30 while Zed was still on 29, and that importer takes
    GPUI's own device . see [updating CEF](/playbooks/updating-cef.md). Drop it as soon as Zed bumps
    wgpu upstream.
23. Per-window content zoom at the platform-metrics seam (`Window::set_zoom`).
24. Retaining WSL's `dzn` adapter when `/dev/dxg` proves the non-conformant Vulkan report belongs to
    Microsoft's translation layer rather than an unknown native driver.
25. Windows `Blurred` surfaces through `DWMWA_SYSTEMBACKDROP_TYPE` acrylic, with the older accent
    path retained before Windows 11 build 22621.
26. Wayland `ext-background-effect-v1` blur (`1819822b47`), preferred when the compositor
    advertises its blur capability, with the KDE protocol retained as fallback. The initial effect
    region follows GPUI's scene-wide window mask, including its per-corner radii, tiling, scale, and
    superellipse exponent.
27. The platform OS-autorepeat flag on `KeystrokeEvent` (`28b3864bfa`), carried from the native
    `KeyDownEvent` so an interceptor does not have to infer held keys after a dropped key-up.
28. Extending the Wayland effect through the CSD shadow inset (`20169fe468`). GPUI's scene mask sits
    on the visible frame inside that inset; the effect now covers the full surface and grows each
    exposed radius by its adjacent inset. The titlebar and sidebar edges therefore blur without
    turning the transparent outer corner wedges into a rectangular blur halo.
29. Confining the Wayland effect to the scene mask (`c1b13a1908`). KWin applies the effect even to
    transparent surface pixels, so the preceding full-surface region exposed blur wedges beyond the
    top-right and both lower window corners. The corrected region uses the mask's bounds and radii;
    transparent CSD shadow and corner pixels retain the compositor's unmodified background.
30. Aligning the Wayland effect with the rendered rounded CSD window (`71bcbb21d5c`). GPUI preserves
    the mask's edge insets through content zoom, snaps it like frame quads, maps it with the actual
    buffer-to-surface ratios, resubmits xdg geometry after zoom changes, and compensates for KWin's
    content-local effect coordinates. While ext blur is active, GPUI clips the incompatible outer
    shadow to the scene mask so raw rails and blurred corner tips cannot appear.
31. Vulkan before OpenGL on Linux (`e166028f70`). The wgpu renderer first creates a Vulkan-only
    instance and keeps it when the adapter is a discrete or integrated GPU; software, virtual, or
    failed Vulkan falls back to the combined Vulkan+GL instance, as does a valid `ZED_DEVICE_ID`.
    A hardware GPU therefore never loads the EGL/GL driver stack. Window-sized path intermediate
    and MSAA textures are created by the first frame that rasterizes a path.
32. Memoized font resolution (`c8081ba076`). `TextSystem::resolve_font` walks the fallback stack
    once per requested font. Before, every text shape re-walked it, and each missing family
    formatted a new error; on Linux `.SystemUIFont` maps to IBM Plex Sans, so every UI text run
    paid several failed lookups when that font was not installed.
33. Desktop interface font on Linux (`19aae7aa64`, `25655fcd99`). `gpui_linux` reads GNOME's
    `org.gnome.desktop.interface` `font-name` or KDE's `org.kde.kdeglobals.General` `font` through
    the settings portal and maps `.SystemUIFont` to the first installed family it names, following
    later changes. The first read happens while the platform is built (capped at 200 ms) so no
    frame is shaped in the fallback family first. `PlatformTextSystem::font_generation` tells
    `TextSystem` to drop resolved ids, and open windows force a redraw.

WGPU window-frame antialiasing (`c8135f5b6b`) applies outer coverage once when a quad and the
window mask have identical bounds, radii, and corner smoothing. Other intersections retain their
mask multiplication, and material opacity remains independent. GPU pixel comparisons cover
opaque and translucent fills, borders, circular and smoothed corners, partial tiling, and fractional
positions. The user confirmed the correction in a native GNOME window on 2026-09-09.

The 2026-09-23/24 performance work added four commits, all on `zz-patches` since
2026-09-24: `a64e53ec` retires Metal atlas tiles only after the frames that read them
complete; `2d9f5676f5` sorts sprites by `(order, texture)` in `Scene::finish` instead of
`(order, tile_id)`, which removed most of a per-frame sort during terminal output;
`7f33860f41` adds `Window::paint_underlay_hole` and `Window::set_underlay_active`, which zz
uses to show macOS browser frames on a native layer under the window (see
[OSR rendering](/browser/osr-rendering.md)); and `9b46226e6f` pauses a macOS window's
display link after three vsyncs without frame demand, restarting it through `schedule_frame`
and a new `frame_waker`, the same contract `gpui_web` uses for `requestAnimationFrame`.

`Cargo.toml` no longer narrates the list; the branch's `git log` is the authority (it carries
more commits than this list numbers, because a few patches landed as follow-up fixes to an entry
above).

# Examples

```toml
# Cargo.toml . workspace.dependencies declare upstream…
gpui = { git = "https://github.com/zed-industries/zed" }
gpui_platform = { git = "https://github.com/zed-industries/zed", default-features = false, features = ["font-kit", "wayland", "x11"] }

# …and the patch section redirects both to the fork at one pinned rev.
# `version = "=0.2.2"` is required on gpui because the zed repo holds more than
# one crate by that name.
[patch."https://github.com/zed-industries/zed"]
gpui = { git = "https://github.com/demfabris/zed", rev = "<rev>", version = "=0.2.2" }
gpui_platform = { git = "https://github.com/demfabris/zed", rev = "<rev>" }
```

Adding a carried patch (no rebase; the lock is already at the branch tip):

```bash
just forks   # confirm LOCK is "in sync" before appending a commit
```

Bumping upstream means rebasing `zz-patches`, then moving the `rev` in `Cargo.toml`
`clients/web/Cargo.toml`, and `crates/zz-gpui-ios/Cargo.toml` before regenerating
the two lockfiles.

# Rebase checks from 2026-09-12

We replayed all 46 zz commits from `c8135f5b6b4c79d534b005aa150d17bd52e9c4de`
onto upstream `7960b2a7c9568e90fbe0727332149e5b2a5fd57a`, covering 416 upstream
commits since the previous base. Two follow-up commits adapt the merged code and
tests to upstream APIs. The scene structs and Metal, WGSL, WebGL, and HLSL
shaders retain the previous fork's contents.

The conflicts required preserving glyph effects in the new line-paint API,
zoom across touch predictions, direct gestures, viewport bounds and system
insets, and Wayland blur updates alongside upstream's new frame scheduling.
Upstream still uses wgpu 29, so zz retains the wgpu 30 patch and shared CEF device.

Include `gpui/profiler` in validation: upstream's new debug overlay constructs a
`Quad` and needs zz's `corner_smoothing` and padding fields. Also inspect new
platform-gated test initializers; a Windows color-emoji test still used the
removed `dilation` field. WGPU layout tests must expect 42 words for `Quad`,
26 for `PolychromeSprite`, and 14 for Linux `SurfaceParams`.

The fork passed 332 GPUI tests, six Apple tests including Metal pixel comparisons,
and 32 WGPU tests. The profiler suite passed 386 tests with one spring timing
failure that passed alone; all six debug-overlay tests passed. These checks ran
on macOS with Rust 1.97.1. Native Linux and Windows validation needs those hosts.

# Rebase checks from 2026-09-25

The 59 commits on `zz-patches` (`9b46226e6f`) were replayed onto upstream
`933d8d93819c749a607e561883855a9b95c79cea` (203 upstream commits) as the branch
`zz-patches-2026-09-25`, which zz pins while `zz-patches` still points at the old tip.
Two commits were dropped because upstream now covers them: the Windows manifest path
(upstream #62525) and the `TestWindow` raw handle error (upstream already returns an
error instead of panicking). One commit was added: `Quad` is padded to 44 words because
upstream's WebGL quad decoder reads whole 16-byte texels at a fixed stride.

Upstream moved the wgpu renderer's device, pipelines, and per-frame state into
`WgpuRendererCore` behind `RendererState`, so the corner mask, shadow clipping, external
textures, and lazily allocated path textures now live on the core. `KeystrokeEvent::is_held`
is threaded through the new `&Keystroke` dispatch, Metal atlas retirement sits in the
`AtlasState` backend, and the Linux desktop font generation also bumps upstream's
`TextSystem` font generation so cached line layouts are dropped. Keystroke interceptors
now also see standalone Shift, Control, Alt, Cmd, and Fn taps, which zz's two interceptors
ignore.

WGPU layout tests now expect 44 words for `Quad`. The fork passed 410 GPUI tests plus 17
profiler and bench tests (these share global state and pass with `--test-threads=1`),
14 Apple tests including the Metal atlas retirement and pixel tests, 12 macOS tests, and
47 WGPU tests including WGSL validation. `gpui_linux` and `gpui_wgpu` were type-checked
for Linux (musl target, zig as the C compiler) and `gpui_windows` for
`x86_64-pc-windows-msvc`. Nothing ran on a Linux or Windows host.

# Related

- [Terminal rendering parity concept](/terminal/rendering-parity.md) . the work done against this revision
- [`app` crate](/crates/zz.md) . the GPUI client consuming these dependencies
- [UI design conventions](/configuration/ui-conventions.md) . the zz-ui fork that replaced `gpui-component`
- [Prerequisites](/playbooks/prerequisites.md) . toolchain needed to build against this GPUI pin
