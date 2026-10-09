---
type: Reference
title: GPUI source
description: Where zz's GPUI crates live (gpui/), how to change them or pull an upstream Zed fix, and what zz's GPUI changes do. gpui-component is not a dependency.
resource: gpui/Cargo.toml
tags: [gpui, reference, vendored]
timestamp: 2026-10-09T00:00:00Z
---

# Overview

zz's GPUI layer lives in `gpui/`: the 22 GPUI crates split out of Zed on 2026-10-05. They
spent four days as their own repository, [`demfabris/gpui`](https://github.com/demfabris/gpui),
and moved into this repo on 2026-10-09 with their history (`git subtree add` of
`99114ccc`), so `git log -- gpui` reaches back to the split. The first commit there is upstream
`zed-industries/zed` at `933d8d9381`, limited to those crates; the next 89 are the commits that
lived on the old `demfabris/zed` `zz-patches` branch (tip `5a00ac89a4`), replayed under new
commit IDs. Upstream fixes come in by hand, when we want them.

Commit IDs cited below come from the old `demfabris/zed` fork, which stays up as an archive. The
same commits exist under `gpui/` with the same subjects.

On Linux, `zpui_platform` is built with `font-kit`, Wayland, and X11 enabled; the same crate
selects the native macOS and Windows backends automatically.

`gpui/` is its own Cargo workspace (its root `Cargo.toml` holds the shared dependency versions
and Zed's lints), excluded from the zz workspace. Both zz workspaces depend on it by path:

| Place | Role |
| --- | --- |
| `Cargo.toml`, `[workspace.dependencies]` | `gpui`, `zpui_platform`, and `zpui_wgpu` (the iOS crate's direct renderer dependency) as `path = "gpui/crates/…"`. |
| `clients/web/Cargo.toml` | The browser client's own workspace, `path = "../../gpui/crates/…"`. |

Because the crates are not workspace members, `cargo clippy --workspace` and
`cargo test --workspace` skip them and `[profile.dev.package."*"]` still optimizes them in dev
builds. `cargo fmt --all` does format them, since it follows path dependencies. Run gpui's own
checks from inside `gpui/`.

**`gpui-component` is not a dependency.** It was forked into `crates/zz-ui` (`zz-ui`) and both
`gpui-component` and `gpui-component-assets` are gone from the workspace and its lockfiles; nothing
outside `gpui` itself is left. The fork's source revision and per-module port notes live in
`crates/zz-ui/UPSTREAM.md`, not here.

# Pane renderer changes

Fork commit `37d0b352ed` carries the pane renderer changes. Metal, WGPU, and DirectX blurred inset shadows
use position-seeded stochastic alpha rounding in 1/128 steps to reduce banding. WGPU applies
this before any required premultiplication. Dithering changes only alpha.
The GPU test `faint_inset_shadows_dither_dark_composites` checks variation, noise size,
brightness, and opaque composition. Both workspaces build these changes
from `gpui/`.

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

Wide blurred inset shadows take a cheaper path in Metal, WGPU, and DirectX (`1b395e5`). A
pixel more than three blur radii (plus the largest corner) inside the hole is discarded, and
when every corner radius is at most a quarter of the blur the shadow is blurred as a sharp
rectangle with two erf pairs instead of the four-sample corner loop. zz's pane focus glow is
such a shadow: zz's GPU fragment time on a 2672x1482pt window fell from 237 to 144 ms/s. The
ignored test `bench_pane_glow` times the glow offscreen; see
[the surface cost research](/research/2026-10-06-surface-cost.md).

Five fixes from building the iPhone client (`67e9cbc` to `01d7c9a`):

- `Window::transact` rolls back accessibility nodes, focus, and active descendant along with the
  rest of prepaint. A list honoring an autoscroll prepaints its rows twice, and iOS always builds
  the tree, so debug builds panicked with "Duplicate a11y node id". zz's settings column no longer
  takes the autoscroll request from its rows to dodge it.
- `ListState::bounds_for_item` counts the list's top padding, as `scroll_to_reveal_item` does.
- `zpui_wgpu` uploads instance data through a `wgpu::util::StagingBelt` with one write for all six
  primitive kinds, and binds the whole instance buffer once, indexing kinds by `first_instance`.
  Before, every primitive kind made its own `queue.write_buffer`, and wgpu made a new staging buffer for each
  one. The ignored `bench_swipe_frames` (a 1179x2556 page swipe, about 650 KB of instances, Metal
  on an M4 Max) went from 102-112 to 37-40 us of CPU per frame.
- `PlatformWindow::keeps_presenting_after_input` (default true) lets a platform skip re-presenting
  the last frame for a second after fast input, which Zed does to keep ProMotion Macs from
  lowering their refresh rate. The iOS window returns false: its display link asks for its rate
  itself, and each present encoded the whole scene again.
- `Window::end_touch_momentum` ends the touch fling a scroll listener swallowed. The next tick
  closes the stream with a zero-delta `Ended`. The pager calls it after a release it handled, and
  `coast_guard` calls it for sheets and the swipe back, so the display link can pause instead of
  ticking through a 2 to 3 s fling.

Two APIs for the phone keyboard (`99114cc`): `TextInputConfiguration::input_mode` (`TextInputMode`,
the HTML `inputmode` set; the web backend writes it as the `inputmode` attribute, the iOS backend
maps it onto `UIKeyboardType`), and `Window::target_visual_viewport_bounds`, where the visible
viewport comes to rest once a keyboard transition ends (default: the current visual viewport).

# zz changes to GPUI

Fork commit `7bdd43258b` gives the native macOS Window menu first chance to handle
`performKeyEquivalent:` in `zpui_macos/src/window.rs` `handle_key_equivalent`.
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
33. Desktop interface font on Linux (`19aae7aa64`, `25655fcd99`). `zpui_linux` reads GNOME's
    `org.gnome.desktop.interface` `font-name` or KDE's `org.kde.kdeglobals.General` `font` through
    the settings portal and maps `.SystemUIFont` to the first installed family it names, following
    later changes. The first read happens while the platform is built (capped at 200 ms) so no
    frame is shaped in the fallback family first. `PlatformTextSystem::font_generation` tells
    `TextSystem` to drop resolved ids, and open windows force a redraw.
34. Shader layers (`Window::paint_shader_layer`, `CustomShader`; `88d396491d`, `e01edb6b1a`). What the closure paints goes into
    a nested `Scene` carried by one `ShaderLayer` primitive; the scene records start and end
    operations, so layers nest and survive cached-view replay. Metal and wgpu draw the nested scene
    into a window-sized texture, copy the layer's pixels into a layer-sized one, and run the
    caller's WGSL fragment shader over the layer with premultiplied blending (Metal translates it to
    MSL through naga). DirectX, and any backend whose compile fails, draws the content unshaded. The
    wgpu headless renderer now uses Metal on macOS so the layer's pixel tests run there.
35. The GPUI lab series (27 commits from `337a10612d` to `cc9e4d1804`): per-frame speedups ported
    from longbridge/gpui-fast (scene bounds grid, cached element path hashes, glyph tile cache,
    recent native line shapes and others), text fixes from gpui-fast, renderer fixes from gpui-ce
    and upstream zed, gpui-ce's accessibility automation APIs, and the `GPUI_FRAME_STATS` frame
    recorder. Each commit body names its source repo and hash. Measured on Linux and on Metal in
    the [GPUI fork lab](/research/2026-09-30-gpui-fork-lab.md).
36. One CoreText font per size (`ff805a96b0`, from gpui-fast `9b2f43d`). `layout_line` reuses a
    cached `CTFont` per font and size instead of `clone_with_font_size` for every run, which keeps
    CoreText's shaping caches alive between lines. `5a00ac89a4` adds `accessibility_active` to
    the frame recorder.

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
and a new `frame_waker`, the same contract `zpui_web` uses for `requestAnimationFrame`.

`git log -- gpui` is the authority (it carries
more commits than this list numbers, because a few patches landed as follow-up fixes to an entry
above).

# Examples

```toml
# Cargo.toml [workspace.dependencies]
gpui = { path = "gpui/crates/gpui" }
zpui_platform = { path = "gpui/crates/zpui_platform", default-features = false, features = ["font-kit", "wayland", "x11"] }
zpui_wgpu = { path = "gpui/crates/zpui_wgpu" }
```

Landing a GPUI change: edit `gpui/` in the same commit as the zz code that needs it. Run
`cargo check --workspace` and `cargo test -p <crate>` inside `gpui/` for the crates you touched,
then the zz workspace gates and `just web build`.

Pulling a fix from upstream Zed: crate paths under `gpui/` match Zed's, so a patch limited to the
touched crates applies with a directory prefix.

```bash
git -C <zed-checkout> format-patch -1 <sha> --stdout -- crates/gpui crates/zpui_wgpu | git am -3 --directory=gpui
```

The two rebase sections below are history from the patch-branch era. Their layout sizes and
test counts still hold.

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
47 WGPU tests including WGSL validation. `zpui_linux` and `zpui_wgpu` were type-checked
for Linux (musl target, zig as the C compiler) and `zpui_windows` for
`x86_64-pc-windows-msvc`. Nothing ran on a Linux or Windows host.

# Related

- [Terminal rendering parity concept](/terminal/rendering-parity.md) . the work done against this revision
- [`app` crate](/crates/zz.md) . the GPUI client consuming these dependencies
- [UI design conventions](/configuration/ui-conventions.md) . the zz-ui fork that replaced `gpui-component`
- [Prerequisites](/playbooks/prerequisites.md) . toolchain needed to build this GPUI source
