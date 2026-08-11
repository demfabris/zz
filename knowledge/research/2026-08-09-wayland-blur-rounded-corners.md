---
type: Research Report
title: Wayland background blur and rounded client-side corners
description: Why GPUI cannot match antialiased client-side window corners with ext-background-effect-v1, how the zoom and KWin coordinate bugs were corrected, and how zz removed the pane-edge backdrop seam.
tags:
- gpui
- wayland
- kwin
- blur
- rounded-corners
- linux
timestamp: 2026-08-09T15:31:25Z
---

# Overview

On Linux Wayland, zz asks the compositor to blur the desktop behind its
transparent client-side window frame. GPUI renders that frame with a floating-point,
antialiased superellipse. `ext-background-effect-v1` gives KWin a separate integer
`wl_region` for the blur. The protocol cannot carry the frame's curve or fractional
pixel coverage.

That information loss creates a fixed tradeoff at the outer edge. An expanded blur
region reaches transparent shadow pixels and exposes blurred corner tips. A region
confined to the visible frame leaves the client-side shadow rails over the raw
desktop. No rectangle rasterizer can make those two independently rendered edges
share the same antialiasing.

The screenshots from 2026-08-09 also exposed a GPUI zoom regression that creates a
larger, fixable error. `RoundedWindowFrame::update_corner_mask` in
`crates/zz/src/window/frame.rs` builds its mask size from the platform window bounds,
while GPUI content zoom lays out the scene in a viewport whose size is the platform
size divided by zoom. The mask therefore mixes two coordinate spaces. An uncommitted
outer-circle experiment in the GPUI fork amplified that error into the large raw
wedges shown at high zoom.

This report separates the fixable coordinate bugs from the protocol limit. A live
KWin geometry capture confirmed the client-side-decoration mapping issue. The
tested GPUI patch compensates for it, keeps the effect inside the rendered mask, and
hides the outer shadow that cannot share that mask.

# Environment

| Component | Value on 2026-08-09 |
| --- | --- |
| Session | KDE Plasma Wayland |
| KWin | 6.7.4, tag commit `8438567a` |
| Active output | HDMI-A-1 at scale 1 |
| GPUI fork branch | `demfabris/zed`, `zz-patches` |
| Pinned corrected revision | `71bcbb21d5c1a14cf126b49ae1c8408da317b4ed` |
| Blur protocol | `ext_background_effect_manager_v1`; KDE legacy blur unavailable |
| Client-side shadow inset | 12 logical pixels |
| Shadow blur radius | 6 logical pixels |
| Window corner smoothing | p-norm exponent 4 |

The [GPUI revision reference](/references/gpui-revision.md) owns the fork pin and
carried-patch history. Read the live manifests and branch before relying on the
revision in this dated report.

# Findings

| Question | Finding | Confidence |
| --- | --- | --- |
| Does the effect create another Wayland surface? | No. GPUI attaches an effect object to the existing `wl_surface`. KWin renders it as separate compositor state and a separate blur pass. | High |
| Can `ext-background-effect-v1` encode GPUI's antialiased superellipse? | No. It accepts an integer rectangle region without radius, path, alpha, or fractional coverage. | High |
| Why does application zoom move the corners? | zz authors the mask with platform bounds while GPUI lays it out in zoomed viewport units. | High |
| Why did the outer-circle experiment expose a large top-left wedge? | Its solved radius grows with the zoomed inset and only matches the first integer scanline of the inner mask. | High |
| Does output fractional scaling explain the supplied screenshot? | No. The active output used scale 1. Fractional output scale can add another one-pixel phase error in other setups. | High |
| Does KWin 6.7.4 shift a surface-local CSD region? | Yes. Runtime frame and buffer geometry differed by the same inset as the blur displacement. An inverse translation aligned the effect with the visible frame, but the separate CSD shadow margin still exposed raw desktop. | High |
| Can a GPUI-only patch match macOS at the last antialiased pixel? | The current protocol cannot carry enough shape information. The tested patch removes the large zoom error and presents blurred Wayland windows without an outer shadow. | High |
| Why remove the outer shadow while blur is active? | The compositor applies blur without consulting the shadow alpha. Blurring the shadow margin exposes corner wedges; excluding it leaves raw rails. A transparent margin with no painted shadow shows neither artifact. | High |
| Why did pane corners pick up the desktop hue? | The translucent corner-notch plane stopped at the pane's antialiased outer contour, while a second half-pixel ring extended outside it. Independent fractional coverage exposed the backdrop between the chrome and border. | High |

# Protocol and renderer boundary

`ext_background_effect_surface_v1.set_blur_region` copies a `wl_region` in
surface-local coordinates. The effect state applies on the next `wl_surface.commit`,
and the compositor chooses the blur algorithm.[1] Core Wayland constructs a
`wl_region` by adding and subtracting integer axis-aligned rectangles.[2]

GPUI uses richer geometry:

1. `WindowCornerMask` in `crates/gpui/src/scene.rs` carries scaled floating-point
   bounds, four radii, and a corner-smoothing exponent.
2. The WGPU renderer uploads those values without reducing them to surface cells.
3. `window_mask_alpha` in `crates/gpui_wgpu/src/shaders.wgsl` evaluates a p-norm
   signed-distance curve and returns fractional coverage across one device pixel.
4. Blurred shadows use a Gaussian path with circular source corners. They skip the
   scene-wide window mask by design.

GPUI's Wayland backend must flatten that geometry into integer scanline rectangles
before it calls the protocol. At HiDPI, one surface-local region cell can cover more
than one device pixel. Fractional scale adds buffer rounding and a `wp_viewport`
mapping between device pixels and surface coordinates.[3][4]

KWin cannot use the client buffer's transparent pixels as an implicit clip. A fully
transparent terminal can request blur over its whole surface, so transparency does
not tell the compositor where the client wants blur.[5] KDE tracked the resulting
blocky rounded regions and tied its antialiased solution to decoration-provided
border radii.[6]

# GPUI zoom-space regression

GPUI's `zoomed_platform_metrics` in `crates/gpui/src/window.rs` defines these
quantities:

```text
platform surface size = W
platform output scale = S
content zoom           = Z
logical viewport size  = W / Z
effective scene scale  = S * Z
```

`RoundedWindowFrame::update_corner_mask` uses `window.window_bounds()` for the mask
width and height. On Wayland, that API returns the platform surface bounds `W`, not
the logical viewport `W / Z`. The app then subtracts logical frame insets from that
platform-sized value. GPUI scales the resulting mask by `S * Z` when it finishes the
scene.

For horizontal insets `aL` and `aR`, the authored and desired device widths are:

```text
authored = (W - aL - aR) * S * Z
desired  = (W / Z - aL - aR) * S * Z
```

After the Wayland backend maps device coordinates back to surface coordinates, the
far edge differs by `(Z - 1) * W`. The error grows with window width and application
zoom. The blur rasterizer then clips the oversized region at the surface boundary,
which destroys right/bottom symmetry and changes the inferred corner insets.

GPUI also snaps frame quads to device pixels but leaves the scene-wide window mask
unsnapped. That phase difference can move an edge by half a device pixel before the
Wayland backend converts it to integer surface cells.

# Patch history and failed bridge

The branch history demonstrates the geometry tradeoff:

| Revision | Effect geometry | Visual result |
| --- | --- | --- |
| `1819822b47` | Exact scene mask | Rounded tips stay clean; the 12-pixel CSD shadow rails remain unblurred. |
| `20169fe468` | Full surface with each exposed p=4 radius expanded by its inset | Rails blur; the compositor also blurs transparent corner and shadow wedges. |
| `c1b13a1908` | Exact scene mask restored | Tips disappear; raw rails return. |

The uncommitted 2026-08-09 experiment unions the exact inner p=4 mask with a
full-surface p=2 circle. It solves a radius that meets the first rasterized row and
column of the inner mask:

```text
R - sqrt(2 R d - d^2) = x
R = x + d + sqrt(2 x d)
```

That single-row equality makes the outer radius grow too fast:

```text
Z=1.0  R about 60,  first top blur near x=52
Z=1.5  R about 92,  first top blur near x=82
Z=2.0  R about 125, first top blur near x=113
```

The experiment causes the apparent down-right blur offset in the high-zoom
screenshot. Integer rasterization also lets the union gain or lose complete surface
cells as zoom changes. It was discarded.

The next A/B run restored the exact scene mask. That result kept the corner wedges
clean, but the app's six-pixel Gaussian shadow still occupied the 12-pixel CSD
margin over unmodified desktop. The user identified those top and left bands in a
second screenshot. A circular outset blurred the bands and brought back the corner
wedge. Extending only the four straight edges moved the discontinuity to each rail
endpoint. The tested patch keeps the exact mask and clips the outer drop shadow to
that mask while ext blur is active.

# KWin 6.7.4 coordinate evidence

KWin stores the ext protocol region in surface-local coordinates.[7] Its blur plugin
later reads the region, translates it by `contentsRect().topLeft()`, and intersects it
with `contentsRect()`.[8] KWin maps a client-side-decorated surface buffer relative to
the xdg window geometry through a separate negative window-geometry origin.[9]

Those paths omit the surface-buffer-to-frame offset when the blur plugin interprets
the region. The live 300% zoom run first reported a buffer at `(452, 191)` and frame
at `(464, 203)`: exactly 12 pixels down and right. KWin therefore drew a correct
surface-local region 12 pixels too far into the client. The inverse translation
removed that displacement. It could not change the raw shadow margin because the
exact effect region excludes that margin by design.

The same run exposed a second GPUI lifecycle bug. Content zoom scaled the rendered
CSD inset but did not immediately update xdg window geometry. GPUI now resubmits the
stored inset when zoom changes, and the Wayland setter applies new window geometry
before committing the surface. At 300%, KWin then reported buffer `(452, 191)` and
frame `(488, 227)`, the expected 36-pixel inset.

KWin still treats the effect region as content-local, so GPUI detects a KDE session
and subtracts the exposed top/left CSD inset before submitting the ext region. The
KWin translation adds it back, aligning the effect with the rendered frame. This is
a compositor-specific compatibility path: it should be removed when KWin adopts the
protocol's surface-local coordinates.

KWin's antialiased rounded blur shader cannot repair an unknown client curve. KWin
uses that path only when `Window::borderRadius()` has a value, and it gets that value
from a server-side KDecoration3 decoration.[10][11] GPUI owns zz's decorations and
uses a p=4 curve, so KWin sees neither the radii nor the smoothing exponent.

A minimal client should submit a full-surface ext region, set xdg window geometry
inside a known transparent margin, and color diagnostic strips in the buffer. That
test can distinguish a KWin coordinate error from GPUI's mask math without carrying
GPUI rendering or application zoom.

# macOS comparison

AppKit gives GPUI an `NSVisualEffectView` that lives in the native window view
hierarchy. GPUI's macOS backend inserts it below the renderer, gives it the content
view bounds, and enables width and height autoresizing. AppKit also exposes
`behindWindow` blending and an alpha-bearing `maskImage` for the material.[12][13]

The app, effect view, mask, transforms, and window clip therefore share one native
composition hierarchy. Wayland asks the client and compositor to reconstruct the
same boundary from two representations. Apple does not document the private filter
pipeline, so this comparison covers the public geometry and masking contracts.

# Pane-corner glare

The external background-effect region describes the outer window. It carries no
pane geometry. GPUI renders a pane's rounded edge as fractional premultiplied-alpha
pixels over the window's blurred and translucent background. Those edge pixels mix
the pane color with the blurred desktop, which can produce a desktop-colored ring.

The global zoom regression can worsen clipping near the outer window boundary, but
it does not explain an interior pane ring. Stable 100% and 300% captures showed the
ring remaining after the outer effect aligned. The app assembled each pane edge from
three pieces:

1. `pane_corner_notches` painted the translucent chrome wedge outside the pane and
   stopped at the pane's outer contour.
2. The pane painted its opaque layout border on that same contour.
3. A half-logical-pixel inset-shadow ring was expanded outside the pane box. At 300%
   UI zoom its footprint was 1.5 device pixels wide.

The two shapes' antialiasing coverage was not complementary under source-over
composition. The expanded translucent ring amplified the low-coverage pixels, so a
saturated desktop appeared as a blue arc. An opaque pane backstop hid the blur and
stacked another plane under translucent pane content, so it was not a valid fix.

The app correction removes the external shadow ring and extends the corner-notch
border inward by exactly the pane border width. The chrome plane now continues under
the opaque border and stops at its inner edge, hiding the antialias overlap without
painting beneath the content. The border remains part of layout; a paint-only border
experiment was rejected because it let terminal glyphs and scrollbars expand into
the chrome.

# Implemented correction and validation

GPUI fork commit `71bcbb21d5c` now:

1. Keeps the exact scene-mask implementation; no blur is extended through the soft
   CSD shadow.
2. Normalizes the persistent window mask against GPUI's logical viewport at the
   window-finalization seam, preserving the caller's logical insets and radii.
3. Snaps the resulting mask with the same device-pixel policy as the frame quads.
4. Maps device coordinates to Wayland surface coordinates using the renderer's actual
   buffer dimensions per axis instead of assuming an exact reciprocal platform scale.
   Drawable rounding can produce different axis ratios, so the corner radius uses
   the smaller ratio and stays inside the rendered curve.
5. Resubmits and immediately applies xdg window geometry when content zoom changes
   the CSD inset.
6. Applies KWin's inverse content-origin translation only in a KDE session.
7. Clips non-inset shadows to the scene-wide window mask while an ext background
   effect and a window mask are active. The 12-pixel CSD allocation remains available
   for resize input, but it paints no shadow over raw desktop.

The app-side pane assembly now:

1. Removes the half-pixel surface ring that extended beyond each gapped pane.
2. Carries the translucent corner-notch plane through the pane border footprint.
3. Keeps the border in layout so pane content and scrollbars stay inside the chrome.

The full Wayland library suite passes 29 tests. The focused geometry suite covers
the KWin offset, actual buffer-to-surface ratios, partial tiling, square and rounded
masks, and zoom/output-scale symmetry. The GPUI test matrix covers zoom values `0.8`,
`1.0`, `1.5`, and `2.0` at output scales `1`, `1.25`, and `2`.

Live screenshots at 100%, 110%, 150%, 200%, and 300% application zoom were inspected
at pixel scale. The tested state has no visible top/left shadow band, right/bottom
effect protrusion, or rounded-corner blur tip. Wayland blurred windows lose their
outer drop shadow; their rounded frame border remains. Separate pane captures at 100%
and 300% show the terminal viewport and scrollbar inside the border with no saturated
corner arc. The GPUI correction changes only the fork; the pane correction changes
`zz-ui`'s shared pane assembly and its single constructor call, without a manifest
change.

A pixel-identical outer edge needs compositor-visible continuous geometry. Viable
architectures include a future background-effect protocol with a rounded or alpha
mask, a KWin extension that accepts client-side radii and smoothing, or server-side
decorations that let KWin own the shared clip.

# Citations

1. [Wayland Protocols, ext-background-effect-v1](https://gitlab.freedesktop.org/wayland/wayland-protocols/-/blob/dac6393216d2755c0302d77e739eb1bd96156852/staging/ext-background-effect/ext-background-effect-v1.xml#L83-126)
2. [Wayland core protocol, wl_region](https://gitlab.freedesktop.org/wayland/wayland/-/blob/main/protocol/wayland.xml#L3142-3175)
3. [Wayland Protocols, fractional-scale-v1](https://gitlab.freedesktop.org/wayland/wayland-protocols/-/blob/main/staging/fractional-scale/fractional-scale-v1.xml#L26-45)
4. [Wayland Protocols, viewporter](https://gitlab.freedesktop.org/wayland/wayland-protocols/-/blob/main/stable/viewporter/viewporter.xml#L64-105)
5. [KDE bug 395725, comment 9](https://bugs.kde.org/show_bug.cgi?id=395725#c9)
6. [KDE bug 453229, blocky Wayland blur corners](https://bugs.kde.org/show_bug.cgi?id=453229)
7. [KWin 6.7.4, backgroundeffect_v1.cpp](https://invent.kde.org/plasma/kwin/-/blob/8438567a741826da8b7536a8b10eb3af8fc8820d/src/wayland/backgroundeffect_v1.cpp#L103-117)
8. [KWin 6.7.4, blur region mapping](https://invent.kde.org/plasma/kwin/-/blob/8438567a741826da8b7536a8b10eb3af8fc8820d/src/plugins/blur/blur.cpp#L453-476)
9. [KWin 6.7.4, xdg surface-buffer mapping](https://invent.kde.org/plasma/kwin/-/blob/8438567a741826da8b7536a8b10eb3af8fc8820d/src/xdgshellwindow.cpp#L291-296)
10. [KWin 6.7.4, rounded blur shader path](https://invent.kde.org/plasma/kwin/-/blob/8438567a741826da8b7536a8b10eb3af8fc8820d/src/plugins/blur/blur.cpp#L800-839)
11. [KWin 6.7.4, KDecoration border-radius source](https://invent.kde.org/plasma/kwin/-/blob/8438567a741826da8b7536a8b10eb3af8fc8820d/src/window.cpp#L2614-2647)
12. [Apple, NSVisualEffectView](https://developer.apple.com/documentation/appkit/nsvisualeffectview)
13. [Apple, NSVisualEffectView maskImage](https://developer.apple.com/documentation/appkit/nsvisualeffectview/maskimage)
