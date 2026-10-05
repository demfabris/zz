---
name: fork-rebase
description: Maintain zz's own GPUI repo (demfabris/gpui, the 22 gpui crates split out of Zed, carrying RenderImage::into_frames, WgpuDeviceContext, the external-texture element, the window corner mask, superellipse corner smoothing, refresh_rate exposure, and more) and the native Ghostty fork pinned in third_party/rust/libghostty-vt-sys/build.rs. Use when changing gpui, moving its pin, pulling an upstream Zed fix, when the user says "bump gpui", "update zed", or "rebase forks", and before debugging weird gpui build errors after a dependency change.
---

# GPUI: `demfabris/gpui`

Since 2026-10-05 zz builds against [`demfabris/gpui`](https://github.com/demfabris/gpui), our
own repo holding the 22 GPUI crates split out of Zed. It is not a patch branch: nothing gets
rebased, and upstream Zed fixes come in by hand. Its first commit is upstream
`zed-industries/zed` `933d8d9381` limited to those crates; the next 89 are the old
`demfabris/zed` `zz-patches` commits (tip `5a00ac89a4`), replayed under new IDs. The old fork
stays up as an archive; knowledge pages cite its commit IDs, and the same commits exist in
`demfabris/gpui` with the same subjects. Crate paths match Zed's (`crates/gpui`,
`crates/gpui_wgpu`, `tooling/perf`, ...), so upstream patches apply as-is.

Local checkout: `~/dev/gpui` on the macbook. Clone it anywhere else; it is about 7 MB.

## Landing a GPUI change

1. Commit and push in the `demfabris/gpui` checkout. Run `cargo check --workspace
   --all-targets` there first, plus the GPU tests for whatever the change touches.
2. Move the `rev` in root `Cargo.toml` `[workspace.dependencies]` (`gpui`, `gpui_platform`,
   `gpui_wgpu`) and in `clients/web/Cargo.toml` (every `demfabris/gpui` line). All must match:
   a second `rev` is a second source identity and builds a second copy of every GPUI crate.
3. Re-resolve both lockfiles and check each diff touches only the 22 GPUI `source =` lines:

   ```bash
   cargo metadata --format-version 1 >/dev/null
   cargo metadata --manifest-path clients/web/Cargo.toml --format-version 1 >/dev/null
   rg -c 'demfabris/gpui' Cargo.lock clients/web/Cargo.lock
   ```

4. Run the workspace gates and `just web build`, then an isolated app run for anything visual.

The gpui revision in diagnostics needs no manual bump: `crates/zz/build.rs` stamps
`ZZ_GPUI_SOURCE` from `Cargo.lock` at build time.

## Pulling a fix from upstream Zed

```bash
git -C <zed-checkout> format-patch -1 <sha> --stdout -- crates/gpui crates/gpui_wgpu | git am -3
```

Limit the pathspec to the crates the fix touches. Never bulk-merge upstream: take what we need,
read it, and run the GPU tests for the renderers it touches.

## What zz changed in GPUI

`git log` in `demfabris/gpui` is the authority. The core five:

  1. `RenderImage::into_frames()` — retired browser frames return their pixel
     buffers to the OSR paint pool.
  2. `WgpuDeviceContext` — `Window::wgpu_device_context()` exposes the Linux
     renderer's `wgpu::Device`/`Queue` (plus a `gpui::wgpu` re-export) so
     embedders create GPU resources on GPUI's exact device.
  3. External-texture element — `gpui::external_texture(wgpu::Texture)` paints
     an app-provided texture with normal clipping/HiDPI via the repurposed
     Linux surface pipeline (was a dead YCbCr stub upstream).
  4. Window corner mask — `Window::set_window_corner_mask()` clips everything
     the window draws except drop shadows to a rounded rect (content masks are
     rectangular, so scrollbars/surfaces would escape a CSD frame's rounded
     corners); the wgpu renderer applies it via per-frame globals in every
     fragment shader.
  5. Corner shape — `Window::set_default_corner_smoothing()` swaps every quad's
     circular arc for a superellipse (`2.0` circular, `4.0` squircle) and
     `set_adaptive_corner_fraction()` resolves an ordinary radius against the
     element so one global setting cannot turn a component into a pill.
     Shadows and the corner mask (4) trace the same exponent — a mask left
     circular around a squircle frame cuts the frame's own border off over the
     arc. Native per-element `CornerRadiusMode::Fixed` bypasses adaptive compression,
     while `Styled::corner_smoothing` overrides only the element's own fill, border,
     and both shadow passes. Descendants retain the window policy. Fully rounded quads stay true circles. GPU-facing structs must stay a
     multiple of 8 bytes (const-asserted in `scene.rs`) or every draw fails
     wgpu's binding-size check while Metal silently reads at a skew.

  Later additions: terminal glyph render effects, CoreGraphics stroke API
  variants, device-pixel synthetic bold, BGRA CoreVideo surfaces (macOS
  font/terminal rendering), `PlatformDisplay::refresh_rate()` (Wayland
  `wl_output` mode rate, for the browser frame-rate ceiling),
  `TestWindow` raw handles returning `HandleError::Unavailable` instead of
  panicking (app-level tests exercising compositor-hint paths), two pane-drag
  fixes (pointer-transparent previews; keeping the drag alive when `can_drop`
  rejects), `TextSystem::underline_thickness` (terminal box geometry sizes
  strokes from the face, like Ghostty's `box_thickness`), optional traffic
  light scale in `TitlebarOptions`, CSS-correct drop shadows (spread
  dilates the shadow's corner radii along with its bounds, so a spread shadow
  traces a rounded corner instead of bulging past it), per-window content
  zoom (`Window::set_zoom()` — effective scale factor × zoom, logical viewport
  ÷ zoom, input/IME coordinates converted at the core platform seam; powers
  zz's browser-style whole-UI zoom, gpui-core-only), and keeping the GPU
  visible under WSL (Mesa's dzn translates Vulkan to D3D12 and reports no
  conformance version, which wgpu hides, so gpui fell back to llvmpipe and
  drew every frame on the CPU; `ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER` is set
  only when `/dev/dxg` exists, a node native Linux cannot have), and
  `KeystrokeEvent::is_held` (the OS autorepeat flag carried through to
  keystroke observers/interceptors — zz's prefix layer swallows repeats by
  this flag instead of held-set inference, which a lost macOS keyUp desyncs). Newest: `gpui_wgpu` subpixel offset kept in
  device pixels (the swash scaler already runs at `pixel_size * scale_factor`, so the
  extra `/ scale_factor` upstream still carries shrank the quarter-pixel phase on HiDPI —
  a one-line upstream-able fix).

Gotchas that still apply when changing renderers or pulling upstream work:

  - zz ships wgpu 30; upstream Zed still pinned 29 at the split. Upstream wgpu
    code written against 29 needs porting: `color_space: SurfaceColorSpace::Auto`
    on both `SurfaceConfiguration`s and `Queue::present(frame)` instead of
    `frame.present()`. The `apply_limit_buckets` change only compiles against 30.
  - Upstream split wgpu instance loading into `shaders_storage.wgsl` /
    `shaders_webgl.wgsl` with per-record `load_*` functions. The storage
    variant reads the WGSL structs directly and follows them, but the WebGL
    loaders hard-code word strides and read sequences — every change
    that widens a scene struct (Quad, Shadow, PolychromeSprite, SurfaceParams)
    must also touch those loaders; nothing catches a
    miss at compile time. `Quad` must stay a multiple of 16 bytes (44 words)
    because the WebGL quad decoder reads whole texels.
  - Upstream unified perf tracking under gpui's `profiler` feature. Since
    `66cad0ed` (2026-09-22), zz leaves it disabled in production to avoid its
    collection overhead. Enable it only for diagnostic builds or GPUI validation,
    on the desktop crate's `gpui` dependency, not the shared workspace dependency.
    The profiler uses native timing and crashes on WASM action dispatch if
    inherited by `zz-ui`. `set_frame_trace_enabled` became
    the shared `set_trace_enabled`, and collectors now yield
    `FrameEvent::{Draw,Present}` instead of bare `FrameTiming`.

Linux and Windows can be type-checked from macOS: `cargo check --target
x86_64-unknown-linux-musl` with a `zig cc -target x86_64-linux-musl` wrapper named
`x86_64-linux-musl-gcc` (drop cc-rs's `--target=` flag; a `zig c++` twin as `-g++`),
`RUST_FONTCONFIG_DLOPEN=1`, and
`FREETYPE2_NO_PKG_CONFIG=1`; and `cargo +1.97.0 check -p gpui_windows --target
x86_64-pc-windows-msvc` with `RC_x86_64_pc_windows_msvc` set to Homebrew's `llvm-rc`.

## Native Ghostty fork

`libghostty-vt-sys/build.rs` fetches `demfabris/ghostty`. The published pin is
`e482b03688ccc9eebd6304176aa85bd5d81f0bfa` on `zz-2026-10-04` (the render state clip
`0ab7941c` and trimmed row copies), on `189df4a1f6403f5bdc349fe44d1d2809741a4c1d` from
`zz-2026-10-02` (the one-call row cell copy for frame build), on
`67351380b6dc30124938d809809ac0aa42813283` from `zz-2026-09-30`, based on copy snapshots
`7823f65dd55fc9ff420d5eb5cae761cbd1995994` and trim fix `c39414175ca2aad564b74b3f52196355f2671774`, upstream base
`6301810a48aaa3426887a4316668f18833a40138`. It adds owned active-screen C ABI snapshots,
shared resident and compressed history backing, snapshot regression tests,
active-page copies sized to their used rows, one-call row copies, clipped render
state updates and trimmed row copies to the three existing signal-stack,
spare-page and trim commits. The fast-forwards keep the earlier pins in their branch
history. The safe wrapper's copy and clip APIs live in published
`demfabris/libghostty-rs` commit `0db98a206681fd60c2b1a1719daf14049eda8c30` on new branch
`zz-2026-10-04`, on `f5f826018e290e776c8bc4e5969c562efe530846` from `zz-2026-10-02` (row
copies and the iteration lifetime fix on `8e40135f` from `zz-2026-09-30`), based on
`359ef751c189540eafb9110b2de89ad95ce48fc3`.
zz vendors only the sys snapshot, without native
source rewriting or a safe-wrapper path patch.

Published branches retain these pins:

| Fork | Branch | Pin |
|---|---|---|
| `demfabris/ghostty` | `zz-2026-10-04` | `e482b03688ccc9eebd6304176aa85bd5d81f0bfa`, render state clip `0ab7941cb98263423377627b8b17d8d090c504bd` and trimmed row copies |
| `demfabris/libghostty-rs` | `zz-2026-10-04` | `0db98a206681fd60c2b1a1719daf14049eda8c30`, clip and trimmed copy API |
| `demfabris/ghostty` | `zz-2026-10-02` | `189df4a1f6403f5bdc349fe44d1d2809741a4c1d`, row cell copy |
| `demfabris/libghostty-rs` | `zz-2026-10-02` | `f5f826018e290e776c8bc4e5969c562efe530846`, row copies and iteration lifetimes |
| `demfabris/ghostty` | `zz-2026-09-30` | `67351380b6dc30124938d809809ac0aa42813283`; copy snapshots `7823f65dd55fc9ff420d5eb5cae761cbd1995994` and trim fix `c39414175ca2aad564b74b3f52196355f2671774` remain in its history |
| `demfabris/ghostty` | `zz-2026-09-29` | `713374afee3d4890f14733877fb51d831ffc82ee`, spare-page reuse |
| `demfabris/ghostty` | `zz-2026-09-25` | `6fce227c`, C ABI signal-stack option |
| `demfabris/ghostty` | `codex/cabi-signal-stack` | `fa7986a9`, previous pin |
| `demfabris/libghostty-rs` | `zz-2026-09-30` | `8e40135fb20e9ed91c37c374fe1d14570c386d06`, owned copy API |
| `demfabris/libghostty-rs` | `zz-2026-09-25` | `359ef751c189540eafb9110b2de89ad95ce48fc3`, parent wrapper |

zz pins the native commit in `build.rs` and the wrapper commit in root `Cargo.toml`;
Cargo regenerates `Cargo.lock` against the published wrapper source.
`third_party/rust/libghostty-vt-sys/UPSTREAM.md` owns the full commit IDs, rationale,
validation, and removal conditions.

For a native update, inspect both upstream and fork histories, preserve the
published pin through a retained branch or tag, and push the new pin as a
new dated branch (`zz-YYYY-MM-DD`); never force-push an existing one. When the
upstream base is new to the fork, create the branch at the upstream commit
with `gh api repos/demfabris/ghostty/git/refs` (the fork network already holds
the objects) and push only the carried commit on top. When the new commits sit
on the current pin, push a local branch started at the pin under the new name;
`gh api repos/demfabris/ghostty/branches` lists every branch tip, which shows
the branch holding a pin. Recheck that C ABI code does not create Zig-owned workers or
install a Zig signal stack. Publish the tested commit, update `GHOSTTY_REPO`
and `GHOSTTY_COMMIT` in `build.rs`, and update the sys README, UPSTREAM record,
and knowledge pages. No Cargo lock update is needed for a native-only change.
A base move or carried commit that changes the C headers also needs a safe
wrapper that speaks the new API and bindings regenerated with the snapshot's
`gen-bindings` tool, then `rustfmt --edition 2024` on `src/bindings.rs` (the
tool's formatter output differs from the workspace's), copied into the wrapper
fork's sys crate; the 2026-09-25 bump records how in UPSTREAM.md. A wrapper
change is published the same way, as a new dated branch on
`demfabris/libghostty-rs`, and the root `Cargo.toml` rev and `Cargo.lock` move
with it.

Validate the actual archive and daemon: exported C symbols, terminal tests,
signal-handler/alternate-stack behavior, and a normal macOS bundle build with
no `GHOSTTY_SOURCE_DIR` or pkg-config bypass. Zig's default test runner uses its
own `std_options`, so its pass alone does not exercise this C ABI option.
When using local source overrides, edits at the same path do not trigger Cargo's
native rebuild; use distinct paths or explicitly rebuild the sys package.
