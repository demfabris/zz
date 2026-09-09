---
type: Reference
title: GPUI revision pin
description: Where the patched Zed revision zz builds against is defined, how to read it, and what the carried GPUI patches do. gpui-component is not a dependency.
resource: Cargo.toml
tags: [gpui, zed, pin, reference, git-dependency]
timestamp: 2026-09-09T00:00:00Z
---

# Overview

zz's GPUI application layer comes from the `demfabris/zed` `zz-patches` branch rather than a
published crate. Desktop, browser, and showcase pin the same published fork revision through
their manifests and lockfiles.
On Linux, `gpui_platform` is built with `font-kit`, Wayland, and X11 enabled; the same crate
selects the native macOS and Windows backends automatically.

**Do not read a revision out of this document.** The normal pin lives in three manifests and
three lockfiles, which must agree outside a local experiment:

| Place | Role |
| --- | --- |
| `Cargo.toml`, `[patch."https://github.com/zed-industries/zed"]` | The `rev = "…"` on `gpui` and `gpui_platform`. This is the authority . editing it is how the pin moves. |
| `examples/ui-showcase/Cargo.toml` | The gallery's independent workspace patch. Keep it on the desktop revision so stories use the same GPUI behavior. |
| `clients/web/Cargo.toml` | The browser client's independent workspace patch. Keep it on the desktop revision. |
| `Cargo.lock`, `examples/ui-showcase/Cargo.lock`, and `clients/web/Cargo.lock` | The resolved `source = "git+https://github.com/demfabris/zed?rev=…"` for normal Git pins. Regenerated, never hand-edited. |

The appearance diagnostics log line no longer holds a third copy to keep in sync:
`crates/zz/build.rs` reads the resolved source out of `Cargo.lock` and stamps it into
`ZZ_GPUI_SOURCE`, which `crates/zz/src/lib.rs` prints as `GPUI_SOURCE`. To read the pin rather than
trust this document:

```bash
rg 'demfabris/zed|zz-forks/zed' Cargo.toml Cargo.lock examples/ui-showcase/{Cargo.toml,Cargo.lock} clients/web/{Cargo.toml,Cargo.lock}
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
brightness, and opaque composition. All three workspaces resolve these changes
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
    to 4 in `theme::CORNER_SMOOTHING`. Sprites and the window corner mask stay circular.
20. Two corrections to that smoothing: a quad whose radius reaches half its shorter side is a
    circle or a pill, so it keeps true arcs (this is also what `rounded_full` clamps to);
    and `Shadow` carries the smoothing of the element it traces, honored on the unblurred
    path so a hairline spread ring stops detaching from a squircle edge at the corners.
21. `Window::set_adaptive_corner_fraction`: resolve an ordinary corner radius against the element
    it rounds, in `Style::paint` where requested radii meet laid-out bounds. Approaches a fraction
    of the shorter side along `cap * tanh(radius / cap)` instead of being clamped at half, so one
    global radius setting cannot make a component change shape category . or stop responding . at
    a value that differs per component. `FULL_CORNER_RADIUS` (what `rounded_full` sets) is exempt
    and still resolves to exactly half, making a pill something a widget declares.
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

WGPU window-frame antialiasing (`c8135f5b6b`) applies outer coverage once when a quad and the
window mask have identical bounds, radii, and corner smoothing. Other intersections retain their
mask multiplication, and material opacity remains independent. GPU pixel comparisons cover
opaque and translucent fills, borders, circular and smoothed corners, partial tiling, and fractional
positions. The user confirmed the correction in a native GNOME window on 2026-09-09.

The comment block above the `[patch]` section in `Cargo.toml` narrates the same list; treat the
branch's `git log` as the tiebreaker (it currently carries more commits than this list numbers,
because a few patches landed as follow-up fixes to an entry above).

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

Bumping upstream means rebasing `zz-patches`, then moving the `rev` in `Cargo.toml`,
`examples/ui-showcase/Cargo.toml`, and `clients/web/Cargo.toml` before regenerating their lockfiles.

# Related

- [Terminal rendering parity concept](/terminal/rendering-parity.md) . the work done against this revision
- [`app` crate](/crates/zz.md) . the GPUI client consuming these dependencies
- [UI design conventions](/configuration/ui-conventions.md) . the zz-ui fork that replaced `gpui-component`
- [Prerequisites](/playbooks/prerequisites.md) . toolchain needed to build against this GPUI pin
