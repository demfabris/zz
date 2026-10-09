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
    SpringState, hsla, point, px, size,
};

/// The WGSL source every renderer draws glass with.
pub const GLASS_SHADER: &str = include_str!("glass.wgsl");

/// The most shapes one glass primitive melts together.
pub const GLASS_MAX_SHAPES: usize = 4;

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
    /// Capped at a quarter of the shape's shorter side.
    pub bezel: Pixels,
    /// How far inward the rim pulls the backdrop at the very edge. Negative
    /// values push outward and pull in what lies past the edge. Capped at
    /// half the shape's shorter side.
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
}

impl Default for GlassMaterial {
    fn default() -> Self {
        Self::regular()
    }
}

impl GlassMaterial {
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

    /// How far past its shape this material reads the backdrop: the blur's
    /// reach, plus the pull when it points outward.
    pub fn backdrop_reach(&self) -> Pixels {
        let blur = self.blur.as_f32().max(0.) * 2.5;
        let outward = (-self.refraction.as_f32()).max(0.);
        px(blur + outward + 2.)
    }

    /// The fill painted in place of the glass where the renderer cannot read
    /// back the frame.
    pub fn fallback_fill(&self) -> Hsla {
        let tint = self.tint;
        let strength = (0.55 + tint.a).min(0.9);
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
    /// Viewport size, padding.
    pub viewport: [f32; 4],
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

    /// The uniform block for drawing this glass over a backdrop that holds
    /// `region` at level 0 and, when `blurred_level` is nonzero, its blur at
    /// that level.
    pub fn uniform(
        &self,
        region: Bounds<i32>,
        blurred_level: u32,
        viewport: Size<i32>,
    ) -> GlassUniform {
        let material = &self.material;
        let mut shapes = [[[0.; 4]; 2]; GLASS_MAX_SHAPES];
        let mut half_minor = f32::MAX;
        for (slot, (bounds, radii)) in shapes
            .iter_mut()
            .zip(self.shapes.iter())
            .take(self.shape_count as usize)
        {
            half_minor = half_minor.min(bounds.size.width.0.min(bounds.size.height.0) / 2.);
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
        let half_minor = half_minor.max(0.);
        let bezel = material
            .bezel
            .as_f32()
            .clamp(0.5, (half_minor * 0.5).max(0.5));
        let refraction = material.refraction.as_f32().clamp(-half_minor, half_minor);
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
            viewport: [viewport.width as f32, viewport.height as f32, 0., 0.],
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_blocks_match_the_shader_layout() {
        assert_eq!(std::mem::size_of::<GlassUniform>(), 288);
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
