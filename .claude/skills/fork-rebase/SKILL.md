---
name: fork-rebase
description: Maintain the carried-patch forks in scripts/forks.conf (currently demfabris/zed carrying gpui patches - RenderImage::into_frames, WgpuDeviceContext, the external-texture element, the window corner mask, superellipse corner smoothing, refresh_rate exposure, and more; see git log on zz-patches). Use whenever bumping gpui/zed or any dep resolved through a [patch] to a demfabris fork, when the user says "bump gpui", "update zed", "rebase forks", or "fork status", and before debugging weird gpui build errors after a dependency change.
---

# Carried-patch forks

Some dependencies resolve through `[patch."<upstream-url>"]` sections in
`Cargo.toml` to a fork under `demfabris/` whose patch branch carries a few
commits on top of a pinned upstream rev. The manifest of all such forks is
`scripts/forks.conf`; the tooling is `scripts/fork-sync.sh`.

Current forks and why:

- **zed** (`demfabris/zed`, branch `zz-patches`): carried commits (authoritative
  list: `git log` on the branch; thirty-five as of 2026-08-05), each upstream-able
  as a small Zed PR; if Zed merges equivalents, drop them and eventually the fork
  branch. The core five:
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
     arc. Fully rounded quads stay true circles. GPU-facing structs must stay a
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
  this flag instead of held-set inference, which a lost macOS keyUp desyncs).

  Adding a carried patch is not a rebase: when `just forks` reports LOCK
  "in sync", commit on the branch tip in the local checkout, push, then repin
  with an explicit rev — plain `cargo update -p gpui` can silently keep the
  old branch tip without refetching ("Locking 0 packages" in its output), so
  use `cargo update -p gpui --precise <new-rev>` (and the same for
  `gpui_platform`), then verify the new rev actually appears in `Cargo.lock`.
  Do not run `fork-rebase` for this — it would move the upstream base as a
  side effect.

  The `[patch."https://github.com/zed-industries/zed"]` entries pin `rev =`
  directly, not the `zz-patches` branch, so any new fork tip must be written
  into `Cargo.toml` by hand as well.

  The gpui revision in diagnostics needs no manual bump: `crates/zz/build.rs`
  stamps `ZZ_GPUI_SOURCE` from `Cargo.lock` at build time.

## Check status

```
just forks
```

Shows, per fork: carried commit count, how far upstream has moved since our
base, and whether `Cargo.lock` matches the fork branch tip. Run this whenever
touching gpui/zed versions, and mention drift to the user if BEHIND is large.

## Rebase onto newer upstream

```
just fork-rebase zed          # onto upstream main tip
just fork-rebase zed <rev>    # onto a specific upstream rev
```

The script keeps a cached blobless clone in `~/.cache/zz-forks/<name>`,
rebases the patch branch, force-pushes (`--force-with-lease`), and runs
`cargo update -p <packages>` to refresh `Cargo.lock`. Rebasing rewrites the
branch, so afterwards set both `rev =` values in the `[patch."…/zed"]` section
of `Cargo.toml` to the new `zz-patches` tip (`git rev-parse` it in the cache
clone) and confirm `Cargo.lock` agrees. Then ALWAYS run the gates before
committing:

```
cargo check --workspace && cargo clippy --workspace --all-targets && cargo test --workspace
```

Gotchas:

- If upstream bumped a patched crate's version, the `version = "=X.Y.Z"` pin
  in the `[patch]` section of `Cargo.toml` must be updated by hand (the pin
  exists because the zed repo contains more than one crate named `gpui`).
- Rebase conflicts are resolved inside the cache clone; the script prints the
  exact commands.
- Never rebase as a side effect of unrelated work — bumping the base rev pulls
  in all upstream changes since the last pin and needs real verification (run
  the app, not just the gates).

## Adding a new carried-patch fork

1. `gh repo fork <upstream> --clone=false`
2. Branch at the exact rev Cargo.lock pins:
   `gh api repos/demfabris/<repo>/git/refs -f ref=refs/heads/zz-patches -f sha=<locked-rev>`
3. Commit the patch (GitHub contents API for small single-file changes — no
   clone needed).
4. Add a `[patch."<upstream-url>"]` section in `Cargo.toml` with a comment
   saying what the patch carries and why.
5. Add a line to `scripts/forks.conf`.
6. Prefer upstreaming: open a PR against upstream so the fork can eventually die.
