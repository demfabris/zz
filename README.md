# gpui

The GPU UI framework from [Zed](https://github.com/zed-industries/zed), as maintained for
[zz](https://github.com/demfabris/zz). It started as a patch branch on a Zed fork and drifted far
enough that rebasing stopped paying off, so it now lives here as its own repository.

It holds only the 22 crates zz builds against, plus the two font families `gpui` and `gpui_wgpu`
embed at build time. Crate names are unchanged, so `use gpui::...` keeps working.

## History

- The first commit is upstream `zed-industries/zed` at `933d8d9381`, limited to these crates.
- The next 89 commits are the zz patch set, replayed from `demfabris/zed` branch `zz-patches`
  (tip `5a00ac89a4`). Every crate tree here matches that tip.
- After that, this repository is the source of truth. Upstream fixes come in by hand.

## Using it

```toml
[dependencies]
gpui = { git = "https://github.com/demfabris/gpui", rev = "<sha>" }
gpui_platform = { git = "https://github.com/demfabris/gpui", rev = "<sha>" }
```

## Pulling a fix from upstream

```bash
git -C ~/src/zed format-patch -1 <sha> --stdout -- crates/gpui crates/gpui_wgpu | git am -3
```

Limit the pathspec to the crates the fix touches. Paths match upstream, so patches apply as-is.

## Liquid glass

Glass shows what was painted under it through a lens: the rim pulls the backdrop inward like the
thick edge of a drop, the face is frosted and tinted, a thin glint runs along the edge facing the
light, and shapes painted as one body melt into each other.

```rust
// Any element, in its own shape.
div().size(px(64.)).rounded_full().glass(GlassMaterial::regular())

// Glass that swells and glows where it is pressed, stretches like gel when a
// held press is dragged, and lenses in and out.
liquid_glass("play", GlassMaterial::regular())
    .drag_flex(px(10.))
    .light_follows_pointer(true)
    .glass_shadow(shadows)
    .size(px(44.))
    .rounded_full()
    .child("▶")

// A tab bar pill that slides to the selected tab like a drop, lifting as it goes.
liquid_glass("pill", GlassMaterial::regular())
    .morph(SpringConfig::new(380., 30., 1.))
    .lift_material(GlassMaterial::bubble())
    .lift_scale(1.25)
    .absolute()
    .left(selected.origin.x)

// Buttons that melt into their neighbors when a press swells them.
glass_group("toolbar", GlassMaterial::regular().merge(px(18.)))
    .flex()
    .gap(px(12.))
    .children(buttons)

// Or paint it yourself, one body made of up to eight shapes.
window.paint_glass_shapes(&shapes, &material);
```

`GlassMaterial` holds every knob: blur (frost), bezel and refraction (the lens), dispersion,
tint, saturation, brightness, contrast, specular and glint width, light angle, fresnel, edge shadow
and width, grain, touch glow, merge radius, and opacity. The presets `regular`, `clear`,
`frosted`, `bubble`, and `smoked` are starting points, and materials interpolate, so springs and
animations can carry one into another; `vanished()` is the identity glass appears from.
`LiquidRect` moves a shape on springs and stretches it along its velocity.

Metal and wgpu draw it from the same WGSL (`crates/gpui/src/glass.wgsl`; Metal translates it with
naga). Each batch of glass costs one render pass break, a copy of only the glass regions, a dual
Kawase blur over just those regions down to half resolution, and one analytic draw per body. At
5344x2964 on Apple silicon the first glass in a frame costs about 0.03 ms and a frosted sidebar
0.16 ms (`cargo test -p gpui_apple --release bench_glass -- --ignored --nocapture`). Renderers
that cannot read back their frame (DirectX, WebGL) paint a translucent fill instead.

Try it with `cargo run -p gpui --example liquid_glass`, or in a browser from the web gallery
(`/liquid-glass`).

## License

Apache-2.0, same as upstream. See `LICENSE-APACHE`. The bundled fonts keep their own licenses
under `assets/fonts`.
