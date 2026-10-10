//! Liquid glass: a material that refracts, frosts, tints, and lights whatever
//! was painted beneath it.
//!
//! Paint it with [`Window::paint_glass`], [`Window::paint_glass_shapes`], or
//! [`Styled::glass`](crate::Styled::glass). The knobs live on
//! [`GlassMaterial`]; [`LiquidRect`] moves a shape on springs and stretches it
//! along its velocity, which is most of what makes glass read as liquid.
//!
//! Renderers that can read back the frame draw it in one pass per glass
//! batch over a blur chain that only covers the glass regions (see
//! `glass.wgsl`). Others paint a translucent fill in its place.

use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    Bounds, Corners, Hsla, Interpolate, Pixels, Point, ScaledPixels, Size, SpringConfig,
    SpringState, WindowCornerMask, hsla, point, px, size,
};

/// The WGSL source every renderer draws glass with.
pub const GLASS_SHADER: &str = include_str!("glass.wgsl");

/// The most shapes one glass primitive melts together.
pub const GLASS_MAX_SHAPES: usize = 8;

/// The deepest level of the blur chain. Level `n` is the viewport scaled by
/// `2^-n`.
pub const GLASS_MAX_BLUR_LEVELS: u32 = 6;

/// How light passes through a glass surface. Every field is a knob; the
/// constructors are starting points, fitted by eye to iOS.
///
/// Lengths are logical pixels. Materials interpolate field by field, so a
/// spring or an animation can carry one material into another; glass
/// appears best by growing out of [`Self::vanished`] rather than fading in.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GlassMaterial {
    /// Standard deviation of the backdrop blur, the frost. Zero is clear.
    pub blur: Pixels,
    /// Width of the rim where the surface curves and lenses the backdrop.
    /// Each shape caps it at a quarter of its shorter side.
    pub bezel: Pixels,
    /// How far inward the rim pulls the backdrop at the very edge. Negative
    /// values push outward and pull in what lies past the edge. Each shape
    /// caps it at half its shorter side.
    pub refraction: Pixels,
    /// Splits the lensing by wavelength along the rim, as a fraction of the
    /// pull; 0.1 is a faint fringe, 0.5 a rainbow.
    pub dispersion: f32,
    /// Color mixed over the backdrop; its alpha is how much.
    pub tint: Hsla,
    /// Backdrop saturation: 1 keeps it, above 1 makes it more vivid.
    pub saturation: f32,
    /// Added to the backdrop color, from -1 to 1.
    pub brightness: f32,
    /// Backdrop contrast around mid gray: 1 keeps it.
    pub contrast: f32,
    /// Brightness of the glint, the thin line where the edge catches light.
    pub specular: f32,
    /// Width of the glint line.
    pub glint_width: Pixels,
    /// The axis light falls along, in radians clockwise from the right edge;
    /// `-3π/4` lights the top left and bottom right edges.
    pub light_angle: f32,
    /// Glow filling the rim, strongest at the edge.
    pub fresnel: f32,
    /// Darkness of the contour just outside the edge.
    pub edge_shadow: f32,
    /// Width of that contour.
    pub edge_width: Pixels,
    /// Frosted grain over the surface; 0.02 hides banding.
    pub noise: f32,
    /// Light from inside the glass, as a press would cause.
    pub glow: f32,
    /// How far the glow reaches.
    pub glow_radius: Pixels,
    /// Where the glow starts, as a fraction of the glass's bounds.
    pub glow_center: Point<f32>,
    /// Shapes closer than this melt into one body.
    pub merge: Pixels,
    /// Fades the whole surface, backdrop included.
    pub opacity: f32,
    /// What renderers that cannot read back the frame paint in its place.
    /// Unset, they paint a translucent fill derived from the tint.
    #[serde(default)]
    pub fallback: Option<Hsla>,
}

impl Default for GlassMaterial {
    fn default() -> Self {
        Self::regular()
    }
}

impl GlassMaterial {
    /// Every preset by name, as settings and URLs spell them.
    pub const PRESETS: &[(&str, fn() -> Self)] = &[
        ("regular", Self::regular),
        ("clear", Self::clear),
        ("frosted", Self::frosted),
        ("bubble", Self::bubble),
        ("smoked", Self::smoked),
    ];

    /// The preset called `name` in [`Self::PRESETS`].
    pub fn preset(name: &str) -> Option<Self> {
        Self::PRESETS
            .iter()
            .find(|(preset, _)| *preset == name)
            .map(|(_, material)| material())
    }

    /// The everyday material: strong lensing at the rim, barely frosted.
    pub fn regular() -> Self {
        Self {
            blur: px(2.),
            bezel: px(20.),
            refraction: px(60.),
            dispersion: 0.04,
            tint: hsla(0., 0., 1., 0.06),
            saturation: 1.5,
            brightness: 0.02,
            contrast: 1.0,
            specular: 0.6,
            glint_width: px(1.2),
            light_angle: -std::f32::consts::FRAC_PI_4 * 3.,
            fresnel: 0.08,
            edge_shadow: 0.25,
            edge_width: px(0.75),
            noise: 0.0,
            glow: 0.0,
            glow_radius: px(90.),
            glow_center: point(0.5, 0.5),
            merge: px(20.),
            opacity: 1.0,
            fallback: None,
        }
    }

    /// No frost: a lens over sharp content.
    pub fn clear() -> Self {
        Self {
            blur: px(0.35),
            tint: hsla(0., 0., 1., 0.0),
            saturation: 1.15,
            brightness: 0.0,
            ..Self::regular()
        }
    }

    /// Heavy frost for panels that carry text.
    pub fn frosted() -> Self {
        Self {
            blur: px(16.),
            bezel: px(16.),
            refraction: px(40.),
            tint: hsla(0., 0., 1., 0.16),
            saturation: 1.8,
            brightness: 0.04,
            noise: 0.02,
            specular: 0.45,
            ..Self::regular()
        }
    }

    /// A thick drop: deep rim, strong lensing, rainbow fringe.
    pub fn bubble() -> Self {
        Self {
            blur: px(0.),
            bezel: px(36.),
            refraction: px(90.),
            dispersion: 0.25,
            tint: hsla(0., 0., 1., 0.0),
            saturation: 1.25,
            specular: 0.9,
            glint_width: px(1.6),
            fresnel: 0.14,
            ..Self::regular()
        }
    }

    /// Dark smoked glass.
    pub fn smoked() -> Self {
        Self {
            blur: px(10.),
            tint: hsla(0., 0., 0.06, 0.42),
            saturation: 1.4,
            brightness: -0.02,
            specular: 0.5,
            edge_shadow: 0.4,
            ..Self::regular()
        }
    }

    /// Colored glass, as a prominent button: the backdrop still lenses
    /// through, washed in `color`.
    pub fn tinted(color: impl Into<Hsla>) -> Self {
        let color = color.into();
        Self {
            blur: px(4.),
            tint: Hsla { a: 0.55, ..color },
            saturation: 1.3,
            brightness: 0.03,
            specular: 0.7,
            fresnel: 0.12,
            ..Self::regular()
        }
    }

    /// This material with every effect off: it draws the backdrop unchanged.
    /// Interpolate from here to make glass appear by lensing in.
    pub fn vanished(&self) -> Self {
        Self {
            blur: px(0.),
            refraction: px(0.),
            dispersion: 0.,
            tint: Hsla { a: 0., ..self.tint },
            saturation: 1.,
            brightness: 0.,
            contrast: 1.,
            specular: 0.,
            fresnel: 0.,
            edge_shadow: 0.,
            noise: 0.,
            glow: 0.,
            ..*self
        }
    }

    /// Sets [`Self::blur`].
    pub fn blur(mut self, blur: impl Into<Pixels>) -> Self {
        self.blur = blur.into();
        self
    }

    /// Sets [`Self::bezel`].
    pub fn bezel(mut self, bezel: impl Into<Pixels>) -> Self {
        self.bezel = bezel.into();
        self
    }

    /// Sets [`Self::refraction`].
    pub fn refraction(mut self, refraction: impl Into<Pixels>) -> Self {
        self.refraction = refraction.into();
        self
    }

    /// Sets [`Self::dispersion`].
    pub fn dispersion(mut self, dispersion: f32) -> Self {
        self.dispersion = dispersion;
        self
    }

    /// Sets [`Self::tint`].
    pub fn tint(mut self, tint: impl Into<Hsla>) -> Self {
        self.tint = tint.into();
        self
    }

    /// Sets [`Self::saturation`].
    pub fn saturation(mut self, saturation: f32) -> Self {
        self.saturation = saturation;
        self
    }

    /// Sets [`Self::brightness`].
    pub fn brightness(mut self, brightness: f32) -> Self {
        self.brightness = brightness;
        self
    }

    /// Sets [`Self::contrast`].
    pub fn contrast(mut self, contrast: f32) -> Self {
        self.contrast = contrast;
        self
    }

    /// Sets [`Self::specular`].
    pub fn specular(mut self, specular: f32) -> Self {
        self.specular = specular;
        self
    }

    /// Sets [`Self::glint_width`].
    pub fn glint_width(mut self, glint_width: impl Into<Pixels>) -> Self {
        self.glint_width = glint_width.into();
        self
    }

    /// Sets [`Self::light_angle`].
    pub fn light_angle(mut self, light_angle: f32) -> Self {
        self.light_angle = light_angle;
        self
    }

    /// Sets [`Self::fresnel`].
    pub fn fresnel(mut self, fresnel: f32) -> Self {
        self.fresnel = fresnel;
        self
    }

    /// Sets [`Self::edge_shadow`].
    pub fn edge_shadow(mut self, edge_shadow: f32) -> Self {
        self.edge_shadow = edge_shadow;
        self
    }

    /// Sets [`Self::edge_width`].
    pub fn edge_width(mut self, edge_width: impl Into<Pixels>) -> Self {
        self.edge_width = edge_width.into();
        self
    }

    /// Sets [`Self::noise`].
    pub fn noise(mut self, noise: f32) -> Self {
        self.noise = noise;
        self
    }

    /// Sets [`Self::glow`].
    pub fn glow(mut self, glow: f32) -> Self {
        self.glow = glow;
        self
    }

    /// Sets [`Self::glow_radius`].
    pub fn glow_radius(mut self, glow_radius: impl Into<Pixels>) -> Self {
        self.glow_radius = glow_radius.into();
        self
    }

    /// Sets [`Self::glow_center`].
    pub fn glow_center(mut self, glow_center: Point<f32>) -> Self {
        self.glow_center = glow_center;
        self
    }

    /// Sets [`Self::merge`].
    pub fn merge(mut self, merge: impl Into<Pixels>) -> Self {
        self.merge = merge.into();
        self
    }

    /// Sets [`Self::opacity`].
    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity;
        self
    }

    /// Sets [`Self::fallback`].
    pub fn fallback(mut self, fill: impl Into<Hsla>) -> Self {
        self.fallback = Some(fill.into());
        self
    }

    /// How far past its shape this material reads the backdrop: the blur's
    /// reach, plus the pull when it points outward.
    pub fn backdrop_reach(&self) -> Pixels {
        let blur = self.blur.as_f32().max(0.) * 2.5;
        let outward = (-self.refraction.as_f32()).max(0.);
        px(blur + outward + 2.)
    }

    /// The fill painted in place of the glass where the renderer cannot read
    /// back the frame.
    /// It fades with the material, so [`Self::vanished`] paints nothing.
    pub fn fallback_fill(&self) -> Hsla {
        if let Some(fill) = self.fallback {
            return Hsla {
                a: fill.a * self.opacity,
                ..fill
            };
        }
        let tint = self.tint;
        let lensing = (self.refraction.as_f32().abs() / 20.
            + self.blur.as_f32() / 4.
            + self.specular
            + self.fresnel)
            .clamp(0., 1.);
        let strength = (0.55 * lensing + tint.a).min(0.9);
        let lightness = if tint.a > 0.0 { tint.l } else { 1.0 };
        hsla(tint.h, tint.s, lightness * 0.92, strength * self.opacity)
    }

    pub(crate) fn scale(&self, factor: f32) -> Self {
        Self {
            blur: self.blur * factor,
            bezel: self.bezel * factor,
            refraction: self.refraction * factor,
            glint_width: self.glint_width * factor,
            edge_width: self.edge_width * factor,
            glow_radius: self.glow_radius * factor,
            merge: self.merge * factor,
            ..*self
        }
    }
}

impl Interpolate for GlassMaterial {
    fn interpolate(from: Self, to: Self, phase: f32) -> Self {
        let lerp = |a: f32, b: f32| f32::interpolate(a, b, phase);
        let length = |a: Pixels, b: Pixels| Pixels::interpolate(a, b, phase);
        Self {
            blur: length(from.blur, to.blur),
            bezel: length(from.bezel, to.bezel),
            refraction: length(from.refraction, to.refraction),
            dispersion: lerp(from.dispersion, to.dispersion),
            tint: Hsla::interpolate(from.tint, to.tint, phase),
            saturation: lerp(from.saturation, to.saturation),
            brightness: lerp(from.brightness, to.brightness),
            contrast: lerp(from.contrast, to.contrast),
            specular: lerp(from.specular, to.specular),
            glint_width: length(from.glint_width, to.glint_width),
            light_angle: lerp(from.light_angle, to.light_angle),
            fresnel: lerp(from.fresnel, to.fresnel),
            edge_shadow: lerp(from.edge_shadow, to.edge_shadow),
            edge_width: length(from.edge_width, to.edge_width),
            noise: lerp(from.noise, to.noise),
            glow: lerp(from.glow, to.glow),
            glow_radius: length(from.glow_radius, to.glow_radius),
            glow_center: point(
                lerp(from.glow_center.x, to.glow_center.x),
                lerp(from.glow_center.y, to.glow_center.y),
            ),
            merge: length(from.merge, to.merge),
            opacity: lerp(from.opacity, to.opacity),
            fallback: match (from.fallback, to.fallback) {
                (Some(from), Some(to)) => Some(Hsla::interpolate(from, to, phase)),
                (from, to) => {
                    if phase < 0.5 {
                        from
                    } else {
                        to
                    }
                }
            },
        }
    }
}

/// One rounded rectangle of a glass body, in window coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GlassShape {
    /// Where the shape sits.
    pub bounds: Bounds<Pixels>,
    /// Its corner radii, clamped to half its shorter side when drawn.
    pub corner_radii: Corners<Pixels>,
}

impl GlassShape {
    /// A rounded rectangle with one radius on every corner.
    pub fn new(bounds: Bounds<Pixels>, radius: impl Into<Pixels>) -> Self {
        Self {
            bounds,
            corner_radii: Corners::all(radius.into()),
        }
    }

    /// A pill or circle filling `bounds`.
    pub fn capsule(bounds: Bounds<Pixels>) -> Self {
        let radius = bounds.size.width.min(bounds.size.height) / 2.;
        Self::new(bounds, radius)
    }
}

/// How a glass region's blur runs: how many times the chain halves the
/// backdrop, and how far apart its taps sit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlassBlurPlan {
    /// Halvings, 0 for no blur. The result always lands on level 1.
    pub levels: u32,
    /// Tap offset in source texels.
    pub offset: f32,
}

impl GlassBlurPlan {
    /// Plans a blur with standard deviation `sigma` device pixels.
    ///
    /// A dual Kawase chain `n` levels deep with offset `o` spreads by about
    /// `1.4 · o · 2^n`. It stays smooth for `o` up to about 2, so the
    /// shallowest chain that reaches `sigma` there is chosen and `o` fitted.
    pub fn new(sigma: f32, max_levels: u32) -> Self {
        if sigma.is_nan() || sigma < 0.5 || max_levels == 0 {
            return Self {
                levels: 0,
                offset: 0.,
            };
        }
        let levels = (sigma / 2.8)
            .log2()
            .ceil()
            .clamp(1., max_levels.min(GLASS_MAX_BLUR_LEVELS) as f32) as u32;
        let offset = (sigma / (1.4 * (1 << levels) as f32)).clamp(0.35, 2.5);
        Self { levels, offset }
    }
}

/// The texels a region covers at blur level `level`, as min and max corners,
/// rounded outward.
pub fn glass_level_rect(region: Bounds<i32>, level: u32) -> Bounds<i32> {
    let shift = |value: i32, up: bool| {
        let scale = 1 << level;
        if up {
            (value + scale - 1).div_euclid(scale)
        } else {
            value.div_euclid(scale)
        }
    };
    let left = shift(region.origin.x, false);
    let top = shift(region.origin.y, false);
    let right = shift(region.origin.x + region.size.width, true);
    let bottom = shift(region.origin.y + region.size.height, true);
    Bounds {
        origin: point(left, top),
        size: size(right - left, bottom - top),
    }
}

/// The uniform block `glass.wgsl` reads for one glass draw.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct GlassUniform {
    /// Up to [`GLASS_MAX_SHAPES`] shapes: `[x, y, width, height]` then
    /// `[top_left, top_right, bottom_right, bottom_left]` radii.
    pub shapes: [[[f32; 4]; 2]; GLASS_MAX_SHAPES],
    /// The rectangle drawn.
    pub quad: [f32; 4],
    /// Level 0 texels holding the backdrop: min and max corners.
    pub backdrop: [f32; 4],
    /// Straight RGBA tint.
    pub tint: [f32; 4],
    /// Bezel, refraction, dispersion, blurred level (0 = sharp).
    pub optics: [f32; 4],
    /// Light direction, glint strength, glint width.
    pub light: [f32; 4],
    /// Saturation, brightness, contrast, grain.
    pub tone: [f32; 4],
    /// Fresnel glow, edge shadow strength and width, opacity.
    pub rim: [f32; 4],
    /// Shape count, corner smoothing, merge radius, padding.
    pub shape: [f32; 4],
    /// Glow center, radius, strength.
    pub glow: [f32; 4],
    /// Viewport size, then where the chain's level 0 starts in the frame.
    pub viewport: [f32; 4],
    /// The window's rounded clip, as a shape: `[x, y, width, height]`.
    pub mask_rect: [f32; 4],
    /// Its corner radii.
    pub mask_radii: [f32; 4],
    /// Its corner smoothing, then 1 when there is a clip.
    pub mask: [f32; 4],
}

impl GlassUniform {
    /// The block's bytes, as the shader lays them out.
    pub fn as_bytes(&self) -> &[u8] {
        // SAFETY: `GlassUniform` is `repr(C)` and only holds `f32`s, so it
        // has no padding and every byte is initialized.
        unsafe {
            std::slice::from_raw_parts(
                (self as *const Self).cast::<u8>(),
                std::mem::size_of::<Self>(),
            )
        }
    }
}

/// The uniform block `glass.wgsl` reads for one blur pass over one region.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct GlassBlurUniform {
    /// Source texels holding valid data: min and max corners.
    pub source: [f32; 4],
    /// Tap offset in source texels, then padding.
    pub params: [f32; 4],
}

impl GlassBlurUniform {
    /// A pass reading `source` with taps `offset` texels apart.
    pub fn new(source: Bounds<i32>, offset: f32) -> Self {
        Self {
            source: [
                source.origin.x as f32,
                source.origin.y as f32,
                (source.origin.x + source.size.width) as f32,
                (source.origin.y + source.size.height) as f32,
            ],
            params: [offset, 0., 0., 0.],
        }
    }

    /// The block's bytes, as the shader lays them out.
    pub fn as_bytes(&self) -> &[u8] {
        // SAFETY: as for `GlassUniform`.
        unsafe {
            std::slice::from_raw_parts(
                (self as *const Self).cast::<u8>(),
                std::mem::size_of::<Self>(),
            )
        }
    }
}

/// Glass painted into a scene, sized in device pixels.
#[derive(Clone, Debug)]
#[expect(missing_docs)]
pub struct Glass {
    pub order: crate::DrawOrder,
    /// What the glass draws over: its shapes and the contour around them.
    pub bounds: Bounds<ScaledPixels>,
    /// What the glass reads: its bounds grown by the material's reach.
    pub backdrop_bounds: Bounds<ScaledPixels>,
    pub content_mask: crate::ContentMask<ScaledPixels>,
    pub shapes: [(Bounds<ScaledPixels>, Corners<ScaledPixels>); GLASS_MAX_SHAPES],
    pub shape_count: u32,
    pub corner_smoothing: f32,
    /// The material with its lengths in device pixels.
    pub material: GlassMaterial,
}

impl Glass {
    /// The blur this glass asks for, given how deep the chain may go.
    pub fn blur_plan(&self, max_levels: u32) -> GlassBlurPlan {
        GlassBlurPlan::new(self.material.blur.as_f32(), max_levels)
    }

    /// The level 0 texels this glass reads, clipped to the viewport.
    pub fn backdrop_region(&self, viewport: Size<i32>) -> Option<Bounds<i32>> {
        let bounds = self.backdrop_bounds;
        let left = (bounds.origin.x.0.floor() as i32).max(0);
        let top = (bounds.origin.y.0.floor() as i32).max(0);
        let right = (bounds.bottom_right().x.0.ceil() as i32).min(viewport.width);
        let bottom = (bounds.bottom_right().y.0.ceil() as i32).min(viewport.height);
        (right > left && bottom > top).then(|| Bounds {
            origin: point(left, top),
            size: size(right - left, bottom - top),
        })
    }

    /// The scissor the glass draws through: its bounds within its content
    /// mask, in whole pixels, clipped to the viewport.
    pub fn scissor(&self, viewport: Size<i32>) -> Option<Bounds<i32>> {
        let bounds = self.bounds.intersect(&self.content_mask.bounds);
        let left = (bounds.origin.x.0.floor() as i32).max(0);
        let top = (bounds.origin.y.0.floor() as i32).max(0);
        let right = (bounds.bottom_right().x.0.ceil() as i32).min(viewport.width);
        let bottom = (bounds.bottom_right().y.0.ceil() as i32).min(viewport.height);
        (right > left && bottom > top).then(|| Bounds {
            origin: point(left, top),
            size: size(right - left, bottom - top),
        })
    }

    /// The uniform block for drawing this glass over a blur chain whose
    /// level 0 starts at `origin` in the frame and holds `region` of it,
    /// blurred at `blurred_level` when that is nonzero.
    pub fn uniform(
        &self,
        region: Bounds<i32>,
        origin: Point<i32>,
        blurred_level: u32,
        viewport: Size<i32>,
        window_mask: Option<WindowCornerMask>,
    ) -> GlassUniform {
        let mask = window_mask.filter(|mask| !mask.bounds.is_empty());
        let region = Bounds {
            origin: region.origin - origin,
            size: region.size,
        };
        let material = &self.material;
        let mut shapes = [[[0.; 4]; 2]; GLASS_MAX_SHAPES];
        for (slot, (bounds, radii)) in shapes
            .iter_mut()
            .zip(self.shapes.iter())
            .take(self.shape_count as usize)
        {
            *slot = [
                [
                    bounds.origin.x.0,
                    bounds.origin.y.0,
                    bounds.size.width.0,
                    bounds.size.height.0,
                ],
                [
                    radii.top_left.0,
                    radii.top_right.0,
                    radii.bottom_right.0,
                    radii.bottom_left.0,
                ],
            ];
        }
        // Each shape caps its own bezel and lensing in the shader.
        let bezel = material.bezel.as_f32().max(0.5);
        let refraction = material.refraction.as_f32();
        let tint = material.tint.to_rgb();
        let (sin, cos) = material.light_angle.sin_cos();
        let shape_bounds = self.shape_bounds();
        let glow_center = point(
            shape_bounds.origin.x.0 + shape_bounds.size.width.0 * material.glow_center.x,
            shape_bounds.origin.y.0 + shape_bounds.size.height.0 * material.glow_center.y,
        );
        GlassUniform {
            shapes,
            quad: [
                self.bounds.origin.x.0,
                self.bounds.origin.y.0,
                self.bounds.size.width.0,
                self.bounds.size.height.0,
            ],
            backdrop: [
                region.origin.x as f32,
                region.origin.y as f32,
                (region.origin.x + region.size.width) as f32,
                (region.origin.y + region.size.height) as f32,
            ],
            tint: [tint.r, tint.g, tint.b, tint.a],
            optics: [
                bezel,
                refraction,
                material.dispersion.max(0.),
                blurred_level as f32,
            ],
            light: [
                cos,
                sin,
                material.specular.max(0.),
                material.glint_width.as_f32().max(0.),
            ],
            tone: [
                material.saturation.max(0.),
                material.brightness,
                material.contrast.max(0.),
                material.noise.max(0.),
            ],
            rim: [
                material.fresnel.max(0.),
                material.edge_shadow.clamp(0., 1.),
                material.edge_width.as_f32().max(0.),
                material.opacity.clamp(0., 1.),
            ],
            shape: [
                self.shape_count.max(1) as f32,
                self.corner_smoothing,
                material.merge.as_f32().max(0.),
                0.,
            ],
            glow: [
                glow_center.x,
                glow_center.y,
                material.glow_radius.as_f32().max(1.),
                material.glow.max(0.),
            ],
            viewport: [
                viewport.width as f32,
                viewport.height as f32,
                origin.x as f32,
                origin.y as f32,
            ],
            mask_rect: mask.map_or([0.; 4], |mask| {
                [
                    mask.bounds.origin.x.0,
                    mask.bounds.origin.y.0,
                    mask.bounds.size.width.0,
                    mask.bounds.size.height.0,
                ]
            }),
            mask_radii: mask.map_or([0.; 4], |mask| {
                [
                    mask.corner_radii.top_left.0,
                    mask.corner_radii.top_right.0,
                    mask.corner_radii.bottom_right.0,
                    mask.corner_radii.bottom_left.0,
                ]
            }),
            mask: mask.map_or([0.; 4], |mask| [mask.corner_smoothing, 1., 0., 0.]),
        }
    }

    /// The union of the shapes, without the contour's margin.
    pub fn shape_bounds(&self) -> Bounds<ScaledPixels> {
        self.shapes
            .iter()
            .take(self.shape_count.max(1) as usize)
            .map(|(bounds, _)| *bounds)
            .reduce(|union, bounds| union.union(&bounds))
            .unwrap_or_default()
    }
}

/// One run of a glass batch, worked out for a renderer to carry out in
/// order: copy each frame rectangle into level 0 of the blur chain, run the
/// blur passes, then draw each glass over the frame.
///
/// The chain is addressed in device pixels relative to [`Self::origin`],
/// scaled down by `2^level`, so every region keeps its place at every level.
#[derive(Clone, Debug, Default)]
pub struct GlassRun {
    /// Where level 0 of the chain starts in the frame.
    pub origin: Point<i32>,
    /// The level 0 texels the chain must span.
    pub extent: Size<i32>,
    /// How many levels below 0 the blur passes reach.
    pub depth: u32,
    /// Frame rectangles to copy, each with where it lands in level 0.
    pub copies: Vec<(Bounds<i32>, Point<i32>)>,
    /// The blur passes, in order.
    pub passes: Vec<GlassBlurPass>,
    /// One draw per glass: its uniform block and its scissor in the frame.
    pub draws: Vec<(GlassUniform, Bounds<i32>)>,
}

/// A render pass over one level of the blur chain.
#[derive(Clone, Debug)]
pub struct GlassBlurPass {
    /// The level read.
    pub source: u32,
    /// The level written.
    pub destination: u32,
    /// Down passes halve the resolution and write every texel later passes
    /// read, so the destination's old contents can be dropped. Up passes
    /// double it and must keep what they do not cover.
    pub down: bool,
    /// One draw per region: its uniform block and its scissor in the
    /// destination level.
    pub draws: Vec<(GlassBlurUniform, Bounds<i32>)>,
}

/// Plans a batch of glass drawn into a frame of `viewport` device pixels.
///
/// Glass whose backdrop overlaps an earlier one's goes in a later run, so
/// that it sees the earlier glass and the chain holds one region per texel.
pub fn plan_glass(
    glasses: &[Glass],
    viewport: Size<i32>,
    window_mask: Option<WindowCornerMask>,
) -> Vec<GlassRun> {
    struct Planned<'a> {
        glass: &'a Glass,
        region: Bounds<i32>,
        scissor: Bounds<i32>,
        blur: GlassBlurPlan,
    }
    let planned: Vec<Planned> = glasses
        .iter()
        .filter_map(|glass| {
            Some(Planned {
                glass,
                region: glass.backdrop_region(viewport)?,
                scissor: glass.scissor(viewport)?,
                blur: glass.blur_plan(GLASS_MAX_BLUR_LEVELS),
            })
        })
        .collect();

    // Two regions closer than a texel at the deepest level either blurs to
    // would share texels down the chain, so they count as overlapping.
    let overlap = |a: &Planned, b: &Planned| {
        let reach = 1 << a.blur.levels.max(b.blur.levels);
        let grown = Bounds {
            origin: a.region.origin - point(reach, reach),
            size: size(
                a.region.size.width + 2 * reach,
                a.region.size.height + 2 * reach,
            ),
        };
        intersects(grown, b.region)
    };
    let mut runs = Vec::new();
    let mut start = 0;
    for end in 1..=planned.len() {
        let overlaps = end < planned.len()
            && planned[start..end]
                .iter()
                .any(|other| overlap(other, &planned[end]));
        if end < planned.len() && !overlaps {
            continue;
        }
        let run = &planned[start..end];
        start = end;

        let align = 1 << GLASS_MAX_BLUR_LEVELS;
        let low = run
            .iter()
            .map(|p| p.region.origin)
            .reduce(|a, b| point(a.x.min(b.x), a.y.min(b.y)))
            .unwrap_or_default();
        let high = run
            .iter()
            .map(|p| p.region.origin + point(p.region.size.width, p.region.size.height))
            .reduce(|a, b| point(a.x.max(b.x), a.y.max(b.y)))
            .unwrap_or_default();
        let origin = point(
            low.x.div_euclid(align) * align,
            low.y.div_euclid(align) * align,
        );
        let local = |region: Bounds<i32>| Bounds {
            origin: region.origin - origin,
            size: region.size,
        };
        let depth = run.iter().map(|p| p.blur.levels).max().unwrap_or(0);

        let mut passes = Vec::new();
        for level in 0..depth {
            passes.push(GlassBlurPass {
                source: level,
                destination: level + 1,
                down: true,
                draws: run
                    .iter()
                    .filter(|p| p.blur.levels > level)
                    .map(|p| {
                        let region = local(p.region);
                        (
                            GlassBlurUniform::new(glass_level_rect(region, level), p.blur.offset),
                            glass_level_rect(region, level + 1),
                        )
                    })
                    .collect(),
            });
        }
        for level in (1..depth).rev() {
            passes.push(GlassBlurPass {
                source: level + 1,
                destination: level,
                down: false,
                draws: run
                    .iter()
                    .filter(|p| p.blur.levels > level)
                    .map(|p| {
                        let region = local(p.region);
                        (
                            GlassBlurUniform::new(
                                glass_level_rect(region, level + 1),
                                p.blur.offset,
                            ),
                            glass_level_rect(region, level),
                        )
                    })
                    .collect(),
            });
        }

        runs.push(GlassRun {
            origin,
            extent: size(high.x - origin.x, high.y - origin.y),
            depth,
            copies: run
                .iter()
                .map(|p| (p.region, p.region.origin - origin))
                .collect(),
            passes,
            draws: run
                .iter()
                .map(|p| {
                    let blurred_level = if p.blur.levels > 0 { 1 } else { 0 };
                    (
                        p.glass
                            .uniform(p.region, origin, blurred_level, viewport, window_mask),
                        p.scissor,
                    )
                })
                .collect(),
        });
    }
    runs
}

fn intersects(a: Bounds<i32>, b: Bounds<i32>) -> bool {
    a.origin.x < b.origin.x + b.size.width
        && b.origin.x < a.origin.x + a.size.width
        && a.origin.y < b.origin.y + b.size.height
        && b.origin.y < a.origin.y + a.size.height
}

/// The size of blur chain level `level` for a chain spanning `extent` at
/// level 0.
pub fn glass_level_size(extent: Size<i32>, level: u32) -> Size<i32> {
    let scale = 1 << level;
    size(
        (extent.width + scale - 1).div_euclid(scale).max(1),
        (extent.height + scale - 1).div_euclid(scale).max(1),
    )
}

/// A rectangle whose center and size each chase their target on a spring,
/// reporting the velocity it moves with.
///
/// Step it once per frame and paint [`Self::stretched`]: squashing a shape
/// along its motion and swelling it across is what makes moving glass look
/// like liquid rather than a sliding pane.
#[derive(Clone, Debug)]
pub struct LiquidRect {
    /// The spring every axis follows.
    pub config: SpringConfig,
    center: [SpringState; 2],
    size: [SpringState; 2],
    target: Bounds<Pixels>,
}

impl LiquidRect {
    /// A rectangle resting at `bounds`.
    pub fn new(bounds: Bounds<Pixels>, config: SpringConfig) -> Self {
        let center = bounds.center();
        Self {
            config,
            center: [
                SpringState {
                    position: center.x.as_f32(),
                    velocity: 0.,
                },
                SpringState {
                    position: center.y.as_f32(),
                    velocity: 0.,
                },
            ],
            size: [
                SpringState {
                    position: bounds.size.width.as_f32(),
                    velocity: 0.,
                },
                SpringState {
                    position: bounds.size.height.as_f32(),
                    velocity: 0.,
                },
            ],
            target: bounds,
        }
    }

    /// Where the rectangle is heading.
    pub fn target(&self) -> Bounds<Pixels> {
        self.target
    }

    /// Retargets without losing velocity.
    pub fn set_target(&mut self, target: Bounds<Pixels>) {
        self.target = target;
    }

    /// Jumps to `bounds` and stops.
    pub fn snap(&mut self, bounds: Bounds<Pixels>) {
        *self = Self::new(bounds, self.config);
    }

    /// Adds velocity, in logical pixels per second, as a flick would.
    pub fn push(&mut self, velocity: Point<Pixels>) {
        self.center[0].velocity += velocity.x.as_f32();
        self.center[1].velocity += velocity.y.as_f32();
    }

    /// Advances every axis by `delta`. Returns whether anything still moves.
    pub fn step(&mut self, delta: Duration) -> bool {
        let delta = delta.as_secs_f32().min(0.1);
        let center = self.target.center();
        let targets = [
            center.x.as_f32(),
            center.y.as_f32(),
            self.target.size.width.as_f32(),
            self.target.size.height.as_f32(),
        ];
        let config = self.config;
        let mut moving = false;
        for (state, target) in self
            .center
            .iter_mut()
            .chain(self.size.iter_mut())
            .zip(targets)
        {
            *state = config.step(*state, target, delta);
            if config.is_settled(*state, target, 0.05) {
                *state = SpringState {
                    position: target,
                    velocity: 0.,
                };
            } else {
                moving = true;
            }
        }
        moving
    }

    /// Whether every axis has come to rest on its target.
    pub fn is_settled(&self) -> bool {
        self.center
            .iter()
            .chain(self.size.iter())
            .all(|state| state.velocity == 0.)
            && self.bounds() == self.target
    }

    /// The center's velocity in logical pixels per second.
    pub fn velocity(&self) -> Point<Pixels> {
        point(px(self.center[0].velocity), px(self.center[1].velocity))
    }

    /// Where the rectangle is now.
    pub fn bounds(&self) -> Bounds<Pixels> {
        let size = size(
            px(self.size[0].position.max(0.)),
            px(self.size[1].position.max(0.)),
        );
        let center = point(px(self.center[0].position), px(self.center[1].position));
        Bounds::new(center - point(size.width / 2., size.height / 2.), size)
    }

    /// Where the rectangle is now, stretched along its motion and thinned
    /// across it with the area kept. `amount` is the stretch per 1000
    /// logical pixels per second, and `limit` caps it (0.3 is a 30% stretch).
    pub fn stretched(&self, amount: f32, limit: f32) -> Bounds<Pixels> {
        let bounds = self.bounds();
        let velocity = [self.center[0].velocity, self.center[1].velocity];
        let speed_x = velocity[0].abs() / 1000.;
        let speed_y = velocity[1].abs() / 1000.;
        let stretch = ((speed_x - speed_y) * amount).clamp(-limit, limit);
        let scale_x = 1. + stretch;
        let scale_y = 1. / scale_x;
        let size = size(bounds.size.width * scale_x, bounds.size.height * scale_y);
        let center = bounds.center();
        Bounds::new(center - point(size.width / 2., size.height / 2.), size)
    }
}

/// Melts a list of shapes into the glass primitive's fixed slots, keeping
/// the first [`GLASS_MAX_SHAPES`].
pub(crate) fn glass_shape_slots(
    shapes: &[GlassShape],
    scale_factor: f32,
) -> (
    [(Bounds<ScaledPixels>, Corners<ScaledPixels>); GLASS_MAX_SHAPES],
    u32,
    Bounds<ScaledPixels>,
) {
    let mut slots: [(Bounds<ScaledPixels>, Corners<ScaledPixels>); GLASS_MAX_SHAPES] =
        Default::default();
    let mut union: Option<Bounds<ScaledPixels>> = None;
    let mut count = 0;
    for (slot, shape) in slots.iter_mut().zip(shapes) {
        let bounds = shape.bounds.scale(scale_factor);
        let radii = shape
            .corner_radii
            .clamp_radii_for_quad_size(shape.bounds.size)
            .scale(scale_factor);
        *slot = (bounds, radii);
        union = Some(match union {
            Some(union) => union.union(&bounds),
            None => bounds,
        });
        count += 1;
    }
    (slots, count, union.unwrap_or_default())
}

/// A 64 by 64 device pixel scene for renderer tests: red left of x = 12,
/// blue right of it, and glass with `material` (lengths in device pixels)
/// over (8, 8) to (56, 56) when one is given.
#[cfg(any(test, feature = "test-support", feature = "bench-support"))]
pub fn glass_test_scene(material: Option<GlassMaterial>) -> crate::Scene {
    use crate::{ContentMask, Quad, Scene};
    let rect = |x: f32, y: f32, width: f32, height: f32| Bounds {
        origin: point(ScaledPixels(x), ScaledPixels(y)),
        size: size(ScaledPixels(width), ScaledPixels(height)),
    };
    let quad = |bounds: Bounds<ScaledPixels>, color: Hsla| Quad {
        bounds,
        content_mask: ContentMask { bounds },
        background: color.into(),
        corner_smoothing: 2.,
        ..Default::default()
    };
    let mut scene = Scene::default();
    scene.insert_primitive(quad(rect(0., 0., 12., 64.), hsla(0., 1., 0.5, 1.)));
    scene.insert_primitive(quad(rect(12., 0., 52., 64.), hsla(2. / 3., 1., 0.5, 1.)));
    if let Some(material) = material {
        let shape = rect(8., 8., 48., 48.);
        let mut shapes: [(Bounds<ScaledPixels>, Corners<ScaledPixels>); GLASS_MAX_SHAPES] =
            Default::default();
        shapes[0] = (shape, Corners::all(ScaledPixels(8.)));
        scene.insert_primitive(Glass {
            order: 0,
            bounds: shape.dilate(ScaledPixels(2.)),
            backdrop_bounds: shape.dilate(ScaledPixels(material.backdrop_reach().as_f32())),
            content_mask: ContentMask {
                bounds: rect(0., 0., 64., 64.),
            },
            shapes,
            shape_count: 1,
            corner_smoothing: 2.,
            material,
        });
    }
    scene.finish();
    scene
}

/// Times the glass cases every renderer is benchmarked on, at `width` by
/// `height` device pixels over 24 colored stripes, and prints one line per
/// case. `time` draws a scene repeatedly and returns its milliseconds per
/// frame; each case keeps its best of five.
#[cfg(any(test, feature = "test-support", feature = "bench-support"))]
pub fn bench_glass_rendering(
    width: f32,
    height: f32,
    mut time: impl FnMut(&crate::Scene) -> anyhow::Result<f64>,
) -> anyhow::Result<()> {
    use crate::{ContentMask, Quad, Scene};
    let rect = |x: f32, y: f32, w: f32, h: f32| Bounds {
        origin: point(ScaledPixels(x), ScaledPixels(y)),
        size: size(ScaledPixels(w), ScaledPixels(h)),
    };
    let viewport = rect(0., 0., width, height);
    let scene = |glasses: &[(Bounds<ScaledPixels>, f32, GlassMaterial)]| {
        let mut scene = Scene::default();
        let stripes = 24;
        let stripe = width / stripes as f32;
        for index in 0..stripes {
            let bounds = rect(index as f32 * stripe, 0., stripe, height);
            scene.insert_primitive(Quad {
                bounds,
                content_mask: ContentMask { bounds },
                background: hsla(index as f32 / stripes as f32, 0.8, 0.5, 1.).into(),
                ..Default::default()
            });
        }
        for (bounds, radius, material) in glasses {
            let mut shapes: [(Bounds<ScaledPixels>, Corners<ScaledPixels>); GLASS_MAX_SHAPES] =
                Default::default();
            shapes[0] = (*bounds, Corners::all(ScaledPixels(*radius)));
            scene.insert_primitive(Glass {
                order: 0,
                bounds: bounds.dilate(ScaledPixels(3.)),
                backdrop_bounds: bounds.dilate(ScaledPixels(material.backdrop_reach().as_f32())),
                content_mask: ContentMask { bounds: viewport },
                shapes,
                shape_count: 1,
                corner_smoothing: 2.,
                material: *material,
            });
        }
        scene.finish();
        scene
    };
    // Lengths are device pixels, as the scene holds them: a 2x display
    // doubles the logical values.
    let regular = GlassMaterial::regular().scale(2.);
    let frosted = GlassMaterial::frosted().scale(2.);
    let clear = GlassMaterial::clear().scale(2.);
    let buttons: Vec<_> = (0..10)
        .map(|index| {
            (
                rect(200. + index as f32 * 140., 200., 100., 100.),
                50.,
                regular,
            )
        })
        .collect();
    let mut toolbar = vec![(rect(150., 170., 1450., 160.), 80., regular)];
    toolbar.extend(buttons.iter().copied());
    let cases = [
        ("one 100x100 button", vec![buttons[0]]),
        ("ten buttons in a row", buttons.clone()),
        ("toolbar under its buttons", toolbar),
        (
            "frosted sidebar 640x2800",
            vec![(rect(60., 80., 640., 2800.), 56., frosted)],
        ),
        (
            "clear glass over the whole window",
            vec![(viewport, 0., clear)],
        ),
        (
            "frosted glass over the whole window",
            vec![(viewport, 0., frosted)],
        ),
    ];
    let mut best = |glasses: &[(Bounds<ScaledPixels>, f32, GlassMaterial)]| {
        let scene = scene(glasses);
        (0..5).try_fold(f64::MAX, |fastest, _| {
            Ok::<_, anyhow::Error>(fastest.min(time(&scene)?))
        })
    };
    let base = best(&[])?;
    println!("glass {width}x{height} background only: {base:.3} ms/frame");
    for (name, glasses) in &cases {
        let ms = best(glasses)?;
        println!(
            "glass {width}x{height} {name}: {ms:.3} ms/frame (+{:.3})",
            ms - base
        );
    }
    Ok(())
}

/// What a renderer must draw for [`glass_test_scene`]; `render` draws a
/// scene at 64 by 64 device pixels.
#[cfg(any(test, feature = "test-support", feature = "bench-support"))]
pub fn check_glass_rendering(
    mut render: impl FnMut(&crate::Scene) -> anyhow::Result<image::RgbaImage>,
) -> anyhow::Result<()> {
    let plain = render(&glass_test_scene(None))?;
    let vanished = render(&glass_test_scene(Some(GlassMaterial::regular().vanished())))?;
    for (x, y, expected) in plain.enumerate_pixels() {
        let actual = vanished.get_pixel(x, y);
        let close = expected
            .0
            .iter()
            .zip(actual.0)
            .all(|(expected, actual)| expected.abs_diff(actual) <= 1);
        anyhow::ensure!(
            close,
            "vanished glass changed ({x}, {y}): {:?} became {:?}",
            expected.0,
            actual.0
        );
    }

    let lens = GlassMaterial::regular()
        .vanished()
        .refraction(px(24.))
        .bezel(px(12.));
    let lensed = render(&glass_test_scene(Some(lens)))?;
    let rim = lensed.get_pixel(9, 32).0;
    anyhow::ensure!(
        plain.get_pixel(9, 32).0[0] > 200 && rim[2] > 200 && rim[0] < 60,
        "the rim should show the blue pulled in from further inside, got {rim:?}"
    );
    let middle = lensed.get_pixel(32, 32).0;
    anyhow::ensure!(
        middle == plain.get_pixel(32, 32).0,
        "the flat face should not bend, got {middle:?}"
    );

    let masked = |material: Option<GlassMaterial>| {
        let mut scene = glass_test_scene(material);
        scene.window_corner_mask = Some(WindowCornerMask {
            bounds: Bounds {
                origin: point(ScaledPixels(16.), ScaledPixels(0.)),
                size: size(ScaledPixels(48.), ScaledPixels(64.)),
            },
            corner_radii: Corners::all(ScaledPixels(8.)),
            corner_smoothing: 2.,
        });
        scene
    };
    let clipped = render(&masked(None))?.get_pixel(9, 32).0;
    let lensed_clipped = render(&masked(Some(lens)))?.get_pixel(9, 32).0;
    anyhow::ensure!(
        clipped == lensed_clipped,
        "glass should stay inside the window's rounded clip, got {lensed_clipped:?} for {clipped:?}"
    );

    // Whatever the scene paints after the glass draws in the same pass, so
    // the glass must leave that pass's scissor as it found it.
    let mut covered = glass_test_scene(Some(lens));
    let corner = Bounds {
        origin: point(ScaledPixels(2.), ScaledPixels(2.)),
        size: size(ScaledPixels(8.), ScaledPixels(8.)),
    };
    covered.insert_primitive(crate::Quad {
        bounds: corner,
        content_mask: crate::ContentMask { bounds: corner },
        background: hsla(1. / 3., 1., 0.5, 1.).into(),
        corner_smoothing: 2.,
        ..Default::default()
    });
    covered.finish();
    let after = render(&covered)?.get_pixel(3, 3).0;
    anyhow::ensure!(
        after[1] > 200 && after[0] < 60,
        "a quad painted after the glass should draw, got {after:?}"
    );

    // The contour darkens only past the edge, even under translucent glass.
    let faded = GlassMaterial::regular()
        .vanished()
        .edge_shadow(0.8)
        .opacity(0.5);
    let inside = render(&glass_test_scene(Some(faded)))?.get_pixel(32, 32).0;
    let expected = plain.get_pixel(32, 32).0;
    anyhow::ensure!(
        inside.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 1),
        "translucent glass should not darken inside, got {inside:?} for {expected:?}"
    );

    let frost = GlassMaterial::regular().vanished().blur(px(3.));
    let frosted = render(&glass_test_scene(Some(frost)))?;
    let seam = frosted.get_pixel(12, 32).0;
    anyhow::ensure!(
        seam[0] > 40 && seam[2] > 40,
        "frost should mix red and blue across the seam, got {seam:?}"
    );
    let far = frosted.get_pixel(40, 32).0;
    let expected = plain.get_pixel(40, 32).0;
    anyhow::ensure!(
        far.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 2),
        "frost should leave flat color alone, got {far:?} for {expected:?}"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_blocks_match_the_shader_layout() {
        assert_eq!(std::mem::size_of::<GlassUniform>(), 464);
        assert_eq!(std::mem::size_of::<GlassBlurUniform>(), 32);
    }

    #[test]
    fn blur_plans_reach_the_requested_spread() {
        assert_eq!(GlassBlurPlan::new(0., 6).levels, 0);
        for sigma in [1., 2., 4., 8., 16., 32., 64.] {
            let plan = GlassBlurPlan::new(sigma, 6);
            assert!(plan.levels >= 1, "{sigma}");
            let spread = 1.4 * plan.offset * (1 << plan.levels) as f32;
            assert!(
                (spread - sigma).abs() / sigma < 0.6,
                "{sigma}: {plan:?} spreads {spread}"
            );
        }
    }

    fn test_glass(x: f32, blur: f32) -> Glass {
        let bounds = Bounds::new(
            point(ScaledPixels(x), ScaledPixels(100.)),
            size(ScaledPixels(40.), ScaledPixels(40.)),
        );
        let mut shapes: [(Bounds<ScaledPixels>, Corners<ScaledPixels>); GLASS_MAX_SHAPES] =
            Default::default();
        shapes[0] = (bounds, Corners::default());
        let material = GlassMaterial::regular().blur(px(blur));
        Glass {
            order: 0,
            bounds,
            backdrop_bounds: bounds.dilate(ScaledPixels(material.backdrop_reach().as_f32())),
            content_mask: crate::ContentMask {
                bounds: Bounds::new(
                    point(ScaledPixels(0.), ScaledPixels(0.)),
                    size(ScaledPixels(1000.), ScaledPixels(1000.)),
                ),
            },
            shapes,
            shape_count: 1,
            corner_smoothing: 2.,
            material,
        }
    }

    #[test]
    fn glass_plans_split_where_backdrops_overlap() {
        let viewport = size(1000, 1000);
        let apart = plan_glass(
            &[test_glass(100., 4.), test_glass(400., 12.)],
            viewport,
            None,
        );
        assert_eq!(apart.len(), 1);
        let run = &apart[0];
        assert_eq!(run.draws.len(), 2);
        assert_eq!(run.origin.x % (1 << GLASS_MAX_BLUR_LEVELS), 0);
        assert!(run.extent.width < 500 && run.extent.height < 200);
        let deepest = GlassBlurPlan::new(12., GLASS_MAX_BLUR_LEVELS).levels;
        assert_eq!(run.depth, deepest);
        assert_eq!(run.passes.len() as u32, 2 * deepest - 1);
        assert!(run.passes.iter().all(|pass| !pass.draws.is_empty()));

        let stacked = plan_glass(
            &[test_glass(100., 4.), test_glass(120., 4.)],
            viewport,
            None,
        );
        assert_eq!(stacked.len(), 2);
    }

    #[test]
    fn level_rects_round_outward() {
        let region = Bounds {
            origin: point(3, 5),
            size: size(10, 6),
        };
        let level = glass_level_rect(region, 1);
        assert_eq!(level.origin, point(1, 2));
        assert_eq!(level.size, size(6, 4));
    }

    #[test]
    fn liquid_rects_settle_on_their_target() {
        let start = Bounds::new(point(px(0.), px(0.)), size(px(40.), px(40.)));
        let end = Bounds::new(point(px(200.), px(0.)), size(px(80.), px(40.)));
        let mut rect = LiquidRect::new(start, SpringConfig::new(300., 30., 1.));
        rect.set_target(end);
        assert!(rect.step(Duration::from_millis(16)));
        assert!(rect.velocity().x > px(0.));
        assert!(rect.stretched(0.2, 0.3).size.width > rect.bounds().size.width);
        for _ in 0..600 {
            rect.step(Duration::from_millis(16));
        }
        assert!(rect.is_settled());
        assert_eq!(rect.bounds(), end);
    }
}
