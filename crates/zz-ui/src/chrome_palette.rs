use crate::{ThemeColor, ThemeMode};
use gpui::Hsla;
pub use zz_client::chrome_palette::{
    CHROME_PRESETS, ChromeColor, ChromePreset, ChromePresetId, ThemeModeSetting, chrome_presets,
};

pub const fn read_chrome_color(color: ChromeColor, colors: &ThemeColor) -> Hsla {
    match color {
        ChromeColor::Background => colors.background,
        ChromeColor::Foreground => colors.foreground,
        ChromeColor::Accent => colors.accent,
    }
}

pub const fn write_chrome_color(color: ChromeColor, colors: &mut ThemeColor, value: Hsla) {
    match color {
        ChromeColor::Background => colors.background = value,
        ChromeColor::Foreground => colors.foreground = value,
        ChromeColor::Accent => colors.accent = value,
    }
}

pub const fn pinned_theme_mode(setting: ThemeModeSetting) -> Option<ThemeMode> {
    match setting.pinned() {
        Some(true) => Some(ThemeMode::Dark),
        Some(false) => Some(ThemeMode::Light),
        None => None,
    }
}

pub fn inherited_chrome_colors(preset: Option<ChromePresetId>, mode: ThemeMode) -> ThemeColor {
    resolved_chrome_colors(preset, mode, [None; ChromeColor::ALL.len()])
}

pub fn resolved_chrome_colors(
    preset: Option<ChromePresetId>,
    mode: ThemeMode,
    overrides: [Option<Hsla>; ChromeColor::ALL.len()],
) -> ThemeColor {
    let mut colors = *ThemeColor::for_mode(mode);
    if let Some(preset) = preset {
        let preset = preset.preset();
        let parse = |hex| crate::parse_hex(hex).expect("built-in chrome preset colors are valid");
        colors.background = parse(preset.background);
        colors.foreground = parse(preset.foreground);
        colors.accent = parse(preset.accent);
        colors.success = parse(preset.success);
        colors.warning = parse(preset.warning);
        colors.danger = parse(preset.danger);
    }
    for (color, value) in ChromeColor::ALL.into_iter().zip(overrides) {
        if let Some(value) = value {
            write_chrome_color(color, &mut colors, value);
        }
    }
    colors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Colorize as _;
    use std::collections::HashSet;

    fn luminance(color: Hsla) -> f32 {
        let rgba: gpui::Rgba = color.into();
        let linear = |channel: f32| {
            if channel <= 0.04045 {
                channel / 12.92
            } else {
                ((channel + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(rgba.r) + 0.7152 * linear(rgba.g) + 0.0722 * linear(rgba.b)
    }

    fn contrast(left: Hsla, right: Hsla) -> f32 {
        let left = luminance(left);
        let right = luminance(right);
        (left.max(right) + 0.05) / (left.min(right) + 0.05)
    }

    #[test]
    fn preset_catalog_has_valid_ids_colors_and_contrast() {
        let mut ids = HashSet::new();
        for preset in &CHROME_PRESETS {
            let id = preset.id.as_str();
            assert!(ids.insert(id), "duplicate preset id: {id}");
            assert_eq!(preset.id.dark(), preset.dark, "{id}");
            assert_eq!(ChromePresetId::parse(id), Some(preset.id), "{id}");
            assert_eq!(ChromePresetId::parse(&format!(" {id}\n")), Some(preset.id));
            assert_eq!(ChromePresetId::parse(&id.to_uppercase()), None);
            let parse =
                |hex| crate::parse_hex(hex).unwrap_or_else(|error| panic!("{id}: {hex}: {error}"));
            let background = parse(preset.background);
            let foreground_contrast = contrast(parse(preset.foreground), background);
            assert!(
                foreground_contrast >= 7.0,
                "{id}: foreground {} on background {} has contrast {foreground_contrast}, expected >= 7.0",
                preset.foreground,
                preset.background,
            );
            for (color, hex) in [
                ("accent", preset.accent),
                ("success", preset.success),
                ("warning", preset.warning),
                ("danger", preset.danger),
            ] {
                let value = parse(hex);
                for (surface, background) in [
                    ("background", background),
                    ("background.raised(2)", background.raised(2)),
                ] {
                    let ratio = contrast(value, background);
                    assert!(
                        ratio >= 3.0,
                        "{id}: {color} {hex} on {surface} has contrast {ratio}, expected >= 3.0",
                    );
                }
            }
        }
    }
}
