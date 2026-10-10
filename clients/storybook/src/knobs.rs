use zz_gpui::{App, GlassMaterial, WindowAppearance, px};
use zz_ui::{
    SelectionStyle, Theme, ThemeMode, UiZoom,
    chrome_palette::{ChromePresetId, inherited_chrome_colors},
    interface_style::{self, InterfaceStyle, Look},
};

const ADAPTIVE_CORNER_FRACTION: f32 = 0.45;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Knobs {
    pub theme: Option<ThemeMode>,
    pub style: InterfaceStyle,
    pub look: Look,
    pub contrast: f32,
    pub zoom: f32,
    pub preset: Option<ChromePresetId>,
    pub pane_opacity: f32,
    pub pane_glow: f32,
    pub motion: bool,
    pub backdrop: Backdrop,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Backdrop {
    #[default]
    Plain,
    Code,
    Color,
}

type Knob<T> = (&'static str, fn(&T) -> f32, fn(&mut T, f32));

/// The numeric parts of a [`Look`] other than its glass, by knob name.
pub const LOOK_KNOBS: &[Knob<Look>] = &[
    ("radius", |l| l.radius, |l, v| l.radius = v),
    (
        "smoothing",
        |l| l.corner_smoothing,
        |l, v| l.corner_smoothing = v,
    ),
    ("shadow", |l| l.shadow, |l, v| l.shadow = v),
    ("elevation", |l| l.elevation, |l, v| l.elevation = v),
    ("outline", |l| l.outline, |l, v| l.outline = v),
    (
        "outline-width",
        |l| l.outline_width,
        |l, v| l.outline_width = v,
    ),
    ("row-inset", |l| l.row_inset, |l, v| l.row_inset = v),
];

pub const GLASS_KNOBS: &[Knob<GlassMaterial>] = &[
    ("glass-blur", |m| m.blur.as_f32(), |m, v| m.blur = px(v)),
    ("glass-tint", |m| m.tint.a, |m, v| m.tint.a = v),
    (
        "glass-refraction",
        |m| m.refraction.as_f32(),
        |m, v| m.refraction = px(v),
    ),
    ("glass-bezel", |m| m.bezel.as_f32(), |m, v| m.bezel = px(v)),
    (
        "glass-dispersion",
        |m| m.dispersion,
        |m, v| m.dispersion = v,
    ),
    (
        "glass-saturation",
        |m| m.saturation,
        |m, v| m.saturation = v,
    ),
    (
        "glass-brightness",
        |m| m.brightness,
        |m, v| m.brightness = v,
    ),
    ("glass-contrast", |m| m.contrast, |m, v| m.contrast = v),
    ("glass-specular", |m| m.specular, |m, v| m.specular = v),
    (
        "glass-glint-width",
        |m| m.glint_width.as_f32(),
        |m, v| m.glint_width = px(v),
    ),
    (
        "glass-light",
        |m| m.light_angle.to_degrees(),
        |m, v| m.light_angle = v.to_radians(),
    ),
    ("glass-fresnel", |m| m.fresnel, |m, v| m.fresnel = v),
    ("glass-edge", |m| m.edge_shadow, |m, v| m.edge_shadow = v),
    (
        "glass-edge-width",
        |m| m.edge_width.as_f32(),
        |m, v| m.edge_width = px(v),
    ),
    ("glass-noise", |m| m.noise, |m, v| m.noise = v),
];

impl Default for Knobs {
    fn default() -> Self {
        Self {
            theme: None,
            style: InterfaceStyle::DEFAULT,
            look: Look::default(),
            contrast: 1.0,
            zoom: 1.0,
            preset: None,
            pane_opacity: 0.5,
            pane_glow: 1.0,
            motion: true,
            backdrop: Backdrop::Plain,
        }
    }
}

impl Knobs {
    /// Reads a knob query. The style comes first, wherever it sits in the
    /// query, and every look knob then overrides what the style set.
    pub fn parse(query: &str) -> Result<Self, String> {
        let pairs = query
            .trim_start_matches('?')
            .split('&')
            .filter(|pair| !pair.is_empty())
            .map(|pair| {
                pair.split_once('=')
                    .ok_or_else(|| format!("expected key=value, not {pair}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut knobs = Self::default();
        if let Some((_, value)) = pairs.iter().find(|(key, _)| *key == "style") {
            knobs.style = InterfaceStyle::parse(value)
                .ok_or_else(|| format!("style must be flat, modern or full, not {value}"))?;
        }
        knobs.look = interface_style::look(knobs.style);
        if let Some((_, value)) = pairs.iter().find(|(key, _)| *key == "glass") {
            knobs.look.glass = match *value {
                "" | "style" => knobs.look.glass,
                "off" => None,
                name => Some(
                    GlassMaterial::preset(name)
                        .ok_or_else(|| format!("no glass preset named {name}"))?,
                ),
            };
        }
        for (key, value) in pairs {
            match key {
                "style" | "glass" => {}
                "theme" => {
                    knobs.theme = match value {
                        "light" => Some(ThemeMode::Light),
                        "dark" => Some(ThemeMode::Dark),
                        "system" => None,
                        _ => {
                            return Err(format!(
                                "theme must be light, dark or system, not {value}"
                            ));
                        }
                    }
                }
                "selection" => {
                    knobs.look.selection = match value {
                        "accent" => SelectionStyle::Accent,
                        "wash" => SelectionStyle::Wash,
                        _ => return Err(format!("selection must be accent or wash, not {value}")),
                    }
                }
                "contrast" => knobs.contrast = number(key, value)?,
                "zoom" => knobs.zoom = number(key, value)?,
                "preset" => {
                    knobs.preset = match value {
                        "" | "default" => None,
                        id => Some(
                            ChromePresetId::parse(id)
                                .ok_or_else(|| format!("no chroma preset named {id}"))?,
                        ),
                    }
                }
                "pane-opacity" => knobs.pane_opacity = number(key, value)?,
                "pane-glow" => knobs.pane_glow = number(key, value)?,
                "motion" => knobs.motion = value != "0" && value != "off" && value != "false",
                "backdrop" => {
                    knobs.backdrop = match value {
                        "plain" => Backdrop::Plain,
                        "code" => Backdrop::Code,
                        "color" => Backdrop::Color,
                        _ => {
                            return Err(format!(
                                "backdrop must be plain, code or color, not {value}"
                            ));
                        }
                    }
                }
                _ => {
                    if let Some((_, _, set)) = LOOK_KNOBS.iter().find(|(name, ..)| *name == key) {
                        set(&mut knobs.look, number(key, value)?);
                    } else if let Some((_, _, set)) =
                        GLASS_KNOBS.iter().find(|(name, ..)| *name == key)
                    {
                        let value = number(key, value)?;
                        if let Some(material) = knobs.look.glass.as_mut() {
                            set(material, value);
                        }
                    }
                }
            }
        }
        Ok(knobs)
    }

    pub fn apply(&self, cx: &mut App) {
        let mode = match self.preset {
            Some(preset) if preset.preset().dark => ThemeMode::Dark,
            Some(_) => ThemeMode::Light,
            None => self.theme.unwrap_or_else(|| match cx.window_appearance() {
                WindowAppearance::Dark | WindowAppearance::VibrantDark => ThemeMode::Dark,
                WindowAppearance::Light | WindowAppearance::VibrantLight => ThemeMode::Light,
            }),
        };
        Theme::change(mode, None, cx);
        cx.set_reduce_motion(!self.motion);
        let theme = Theme::global_mut(cx);
        self.look.apply(theme);
        theme.colors = inherited_chrome_colors(self.preset, mode);
        theme.pane_background_opacity = self.pane_opacity;
        theme.pane_glow_strength = self.pane_glow;
        theme.set_contrast(self.contrast);
        cx.set_global(UiZoom(self.zoom));
        let knobs = *self;
        for window in cx.windows() {
            window
                .update(cx, |_, window, _| {
                    window.set_zoom(knobs.zoom);
                    window.set_default_corner_smoothing(knobs.look.corner_smoothing);
                    window.set_adaptive_corner_fraction(Some(ADAPTIVE_CORNER_FRACTION));
                    window.refresh();
                })
                .ok();
        }
    }
}

fn number(key: &str, value: &str) -> Result<f32, String> {
    value
        .parse::<f32>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or_else(|| format!("{key} must be a number, not {value}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_keys_and_ignores_the_rest() {
        let knobs =
            Knobs::parse("?theme=dark&radius=12&zoom=1.25&story=agent&preset=nord&motion=0")
                .unwrap();
        assert_eq!(knobs.theme, Some(ThemeMode::Dark));
        assert_eq!(knobs.look.radius, 12.0);
        assert_eq!(knobs.zoom, 1.25);
        assert_eq!(knobs.preset, ChromePresetId::parse("nord"));
        assert!(!knobs.motion);
        assert!(Knobs::parse("radius=wide").is_err());
        assert!(Knobs::parse("preset=sepia").is_err());
    }

    #[test]
    fn glass_sliders_override_the_preset_in_any_order() {
        let knobs = Knobs::parse("glass-blur=30&glass=frosted&glass-tint=0.5").unwrap();
        let glass = knobs.look.glass.unwrap();
        assert_eq!(glass.blur, px(30.0));
        assert_eq!(glass.tint.a, 0.5);
        assert_eq!(glass.bezel, GlassMaterial::frosted().bezel);
        assert_eq!(
            Knobs::parse("glass=off&glass-blur=30").unwrap().look.glass,
            None
        );
        assert!(Knobs::parse("glass=lava").is_err());
        let lit = Knobs::parse("glass-light=90&glass-glint-width=3").unwrap();
        let glass = lit.look.glass.unwrap();
        assert!((glass.light_angle - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        assert_eq!(glass.glint_width, px(3.0));
    }

    #[test]
    fn the_style_sets_the_look_until_a_knob_says_otherwise() {
        let flat = Knobs::parse("style=flat").unwrap();
        assert_eq!(flat.look, interface_style::look(InterfaceStyle::Flat));
        let full = Knobs::parse("radius=12&outline=0.3&selection=wash&style=full").unwrap();
        assert_eq!(full.look.radius, 12.0);
        assert_eq!(full.look.outline, 0.3);
        assert_eq!(full.look.selection, SelectionStyle::Wash);
        assert_eq!(
            full.look.glass,
            interface_style::look(InterfaceStyle::Full).glass
        );
        let blurred = Knobs::parse("glass-blur=30").unwrap().look.glass.unwrap();
        assert_eq!(blurred.blur, px(30.0));
        assert!(Knobs::parse("style=round").is_err());
        assert!(Knobs::parse("selection=loud").is_err());
    }
}
