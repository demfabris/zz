use zz_gpui::{App, GlassMaterial, WindowAppearance, px};
use zz_ui::{
    Theme, ThemeMode, UiZoom,
    chrome_palette::{ChromePresetId, inherited_chrome_colors},
};

const ADAPTIVE_CORNER_FRACTION: f32 = 0.45;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Knobs {
    pub theme: Option<ThemeMode>,
    pub radius: f32,
    pub shadow: f32,
    pub contrast: f32,
    pub zoom: f32,
    pub smoothing: f32,
    pub preset: Option<ChromePresetId>,
    pub pane_opacity: f32,
    pub pane_glow: f32,
    pub motion: bool,
    pub glass: Option<GlassMaterial>,
    pub backdrop: Backdrop,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Backdrop {
    #[default]
    Plain,
    Code,
    Color,
}

pub const GLASS_KNOBS: &[(&str, fn(&GlassMaterial) -> f32, fn(&mut GlassMaterial, f32))] = &[
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
    ("glass-specular", |m| m.specular, |m, v| m.specular = v),
    ("glass-fresnel", |m| m.fresnel, |m, v| m.fresnel = v),
    ("glass-edge", |m| m.edge_shadow, |m, v| m.edge_shadow = v),
    ("glass-noise", |m| m.noise, |m, v| m.noise = v),
];

impl Default for Knobs {
    fn default() -> Self {
        Self {
            theme: None,
            radius: 6.0,
            shadow: 1.0,
            contrast: 1.0,
            zoom: 1.0,
            smoothing: 4.0,
            preset: None,
            pane_opacity: 0.5,
            pane_glow: 1.0,
            motion: true,
            glass: None,
            backdrop: Backdrop::Plain,
        }
    }
}

impl Knobs {
    pub fn parse(query: &str) -> Result<Self, String> {
        let mut knobs = Self::default();
        let mut glass_overrides = Vec::new();
        for pair in query
            .trim_start_matches('?')
            .split('&')
            .filter(|pair| !pair.is_empty())
        {
            let (key, value) = pair
                .split_once('=')
                .ok_or_else(|| format!("expected key=value, not {pair}"))?;
            match key {
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
                "radius" => knobs.radius = number(key, value)?,
                "shadow" => knobs.shadow = number(key, value)?,
                "contrast" => knobs.contrast = number(key, value)?,
                "zoom" => knobs.zoom = number(key, value)?,
                "smoothing" => knobs.smoothing = number(key, value)?,
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
                "glass" => {
                    knobs.glass = match value {
                        "" | "off" => None,
                        name => Some(
                            GlassMaterial::preset(name)
                                .ok_or_else(|| format!("no glass preset named {name}"))?,
                        ),
                    }
                }
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
                    if let Some((_, _, set)) = GLASS_KNOBS.iter().find(|(name, _, _)| *name == key)
                    {
                        glass_overrides.push((*set, number(key, value)?));
                    }
                }
            }
        }
        if let Some(material) = knobs.glass.as_mut() {
            for (set, value) in glass_overrides {
                set(material, value);
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
        theme.colors = inherited_chrome_colors(self.preset, mode);
        theme.pane_background_opacity = self.pane_opacity;
        theme.pane_glow_strength = self.pane_glow;
        theme.radius = px(self.radius);
        theme.shadow = self.shadow > 0.0;
        theme.shadow_strength = self.shadow;
        theme.glass = self.glass;
        theme.set_contrast(self.contrast);
        cx.set_global(UiZoom(self.zoom));
        let knobs = *self;
        for window in cx.windows() {
            window
                .update(cx, |_, window, _| {
                    window.set_zoom(knobs.zoom);
                    window.set_default_corner_smoothing(knobs.smoothing);
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
        assert_eq!(knobs.radius, 12.0);
        assert_eq!(knobs.zoom, 1.25);
        assert_eq!(knobs.preset, ChromePresetId::parse("nord"));
        assert!(!knobs.motion);
        assert!(Knobs::parse("radius=wide").is_err());
        assert!(Knobs::parse("preset=sepia").is_err());
    }

    #[test]
    fn glass_sliders_override_the_preset_in_any_order() {
        let knobs = Knobs::parse("glass-blur=30&glass=frosted&glass-tint=0.5").unwrap();
        let glass = knobs.glass.unwrap();
        assert_eq!(glass.blur, px(30.0));
        assert_eq!(glass.tint.a, 0.5);
        assert_eq!(glass.bezel, GlassMaterial::frosted().bezel);
        assert_eq!(Knobs::parse("glass-blur=30").unwrap().glass, None);
        assert!(Knobs::parse("glass=lava").is_err());
    }
}
