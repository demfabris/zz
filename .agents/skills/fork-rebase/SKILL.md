---
name: fork-rebase
description: Maintain zz's own GPUI (gpui/, the 22 gpui crates split out of Zed, carrying RenderImage::into_frames, WgpuDeviceContext, the external-texture element, the window corner mask, superellipse corner smoothing, refresh_rate exposure, and more) and the vendored native Ghostty in third_party/ghostty with its libghostty-vt crates. Use when changing gpui or Ghostty, pulling an upstream Zed or Ghostty change, when the user says "bump gpui", "update zed", "bump ghostty", "sync ghostty", or "rebase forks", and before debugging weird gpui build errors after a dependency change.
---

# GPUI: `gpui/`

zz's GPUI lives in `gpui/`, the 22 GPUI crates split out of Zed on 2026-10-05. They were the
[`demfabris/gpui`](https://github.com/demfabris/gpui) repo until 2026-10-09, when they moved in
with their history (`git log -- gpui`). It is not a patch branch: nothing gets rebased, and
upstream Zed fixes come in by hand. The first commit is upstream `zed-industries/zed`
`933d8d9381` limited to those crates; the next 89 are the old `demfabris/zed` `zz-patches`
commits (tip `5a00ac89a4`), replayed under new IDs. The old fork stays up as an archive;
knowledge pages cite its commit IDs, and the same commits exist under `gpui/` with the same
subjects. Crate paths under `gpui/` match Zed's (`crates/gpui`, `crates/zpui_wgpu`,
`tooling/perf`, ...).

`gpui/` is its own Cargo workspace, excluded from the root one. The root `Cargo.toml` and
`clients/web/Cargo.toml` depend on it by path, so there is no revision to move and no lockfile
to re-resolve.

## Landing a GPUI change

1. Edit `gpui/` in the same commit as the zz code that needs it.
2. From inside `gpui/`, run `cargo check --workspace --all-targets` plus the GPU tests for
   whatever the change touches. The root clippy and test runs skip gpui; `cargo fmt --all`
   from the root formats it.
3. Run the workspace gates and `just web build`, then an isolated app run for anything visual.

## Pulling a fix from upstream Zed

```bash
git -C <zed-checkout> format-patch -1 <sha> --stdout -- crates/gpui crates/zpui_wgpu | git am -3 --directory=gpui
```

Limit the pathspec to the crates the fix touches. Never bulk-merge upstream: take what we need,
read it, and run the GPU tests for the renderers it touches.

## What zz changed in GPUI

`git log -- gpui` is the authority. The core five:

  1. `RenderImage::into_frames()` — retired browser frames return their pixel
     buffers to the OSR paint pool.
  2. `WgpuDeviceContext` — `Window::wgpu_device_context()` exposes the Linux
     renderer's `wgpu::Device`/`Queue` (plus a `zpui::wgpu` re-export) so
     embedders create GPU resources on GPUI's exact device.
  3. External-texture element — `zpui::external_texture(wgpu::Texture)` paints
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
  this flag instead of held-set inference, which a lost macOS keyUp desyncs). Newest: `zpui_wgpu` subpixel offset kept in
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
`FREETYPE2_NO_PKG_CONFIG=1`; and `cargo +1.97.0 check -p zpui_windows --target
x86_64-pc-windows-msvc` with `RC_x86_64_pc_windows_msvc` set to Homebrew's `llvm-rc`.

## Native Ghostty: `third_party/ghostty`

Ghostty lives in `third_party/ghostty`: a trimmed snapshot of upstream `ghostty-org/ghostty`
with zz's commits on top. `third_party/rust/libghostty-vt-sys/build.rs` builds it with Zig;
`GHOSTTY_SOURCE_DIR` still points the build at another checkout. Cargo reruns the native build
when `build.zig`, `build.zig.zon`, `include`, `pkg` or `src` change under the source dir. Zig
unpacks Ghostty's packages into `third_party/ghostty/zig-pkg` (ignored), once per checkout.

The trim keeps what the libghostty-vt build reads on every target: `build.zig`,
`build.zig.zon`, `LICENSE`, `include/`, `src/` without fonts and crash dumps, every
`pkg/*/build.zig` and `build.zig.zon`, and the pkg dirs it compiles (apple-sdk, highway,
simdutf, translate-c, wuffs). The list is `keep` in `scripts/vendor-ghostty.sh`.

History:

- `Import Ghostty <sha>` commits hold pristine trimmed upstream and end with a
  `Ghostty-Upstream: <full sha>` trailer. Each import's parent is the previous import.
- zz's changes are ordinary commits after it: `git log <last import>..HEAD -- third_party/ghostty`.
  Today that is upstream `6301810a` plus eight commits: the C ABI signal-stack option,
  spare-page reuse, the history-erase trim fix, owned copy snapshots, active-page copies at
  their used size, one-call row cell copies, render state clips and trimmed row copies.

The safe wrapper and the sys crate are vendored next to each other in `third_party/rust`
(`libghostty-vt`, `libghostty-vt-sys`); their `UPSTREAM.md` files own the commit IDs,
rationale, validation, and removal conditions.

## Syncing Ghostty upstream

1. `just vendor ghostty <full sha|branch|tag>`. It fetches into `~/.cache/zz/ghostty.git`,
   commits the trimmed snapshot on top of the last import in a throwaway worktree, and merges
   that commit. The merge base is the last import, so conflicts are exactly the places where
   upstream changed lines zz changed.
2. Resolve conflicts under `third_party/ghostty` and `git commit`.
3. If Zig fails on a missing file the trim dropped, add a pathspec to `keep`, commit the script,
   and rerun step 1 with the same rev: the new import adds the file and merges cleanly.
4. If the C headers changed, regenerate `src/bindings.rs` with the sys crate's tool, then
   `rustfmt --edition 2024` it (the tool's formatter output differs from the workspace's), and
   update the wrapper in `third_party/rust/libghostty-vt` to the new API:

   ```bash
   GHOSTTY_SOURCE_DIR=$PWD/third_party/ghostty cargo run --manifest-path third_party/rust/libghostty-vt-sys/Cargo.toml --features bindgen-tool --bin gen-bindings
   ```

5. Recheck that C ABI code does not create Zig-owned workers or install a Zig signal stack,
   drop zz commits upstream now covers, and update the sys README, the UPSTREAM records, and
   the knowledge pages.

Sending a change upstream: `git format-patch <last import>..HEAD --relative=third_party/ghostty
-- third_party/ghostty` gives patches that apply to a Ghostty checkout; open the PR from a
branch on `demfabris/ghostty`.

Validate the actual archive and daemon: exported C symbols, terminal tests,
signal-handler/alternate-stack behavior, and a normal macOS bundle build with
no `GHOSTTY_SOURCE_DIR` or pkg-config bypass. Zig's default test runner uses its
own `std_options`, so its pass alone does not exercise this C ABI option. Cross-check
`cargo check -p zz-terminal --all-features --target <t>` for `aarch64-apple-ios` and
`x86_64-unknown-linux-gnu` from a Mac; `x86_64-pc-windows-msvc` needs a Windows host or CI,
because Zig has no MSVC C headers on a Mac.

## Pre-vendoring branches

Before 2026-10-09 zz fetched Ghostty and the wrapper from these fork branches, and knowledge
pages still cite their commit IDs. Nothing builds from them now; keep them for history.

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
