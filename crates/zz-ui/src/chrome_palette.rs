use crate::{ThemeColor, ThemeMode};
use gpui::Hsla;
pub use zz_client::chrome_palette::{
    CHROME_PRESETS, ChromeColor, ChromePreset, ChromePresetId, ThemeModeSetting,
};

pub const fn read_chrome_color(color: ChromeColor, colors: &ThemeColor) -> Hsla {
    match color {
        ChromeColor::Background => colors.background,
        ChromeColor::Foreground => colors.foreground,
        ChromeColor::Border => colors.border,
        ChromeColor::Success => colors.success,
        ChromeColor::Warning => colors.warning,
        ChromeColor::Danger => colors.danger,
    }
}

pub const fn write_chrome_color(color: ChromeColor, colors: &mut ThemeColor, value: Hsla) {
    match color {
        ChromeColor::Background => colors.background = value,
        ChromeColor::Foreground => colors.foreground = value,
        ChromeColor::Border => colors.border = value,
        ChromeColor::Success => colors.success = value,
        ChromeColor::Warning => colors.warning = value,
        ChromeColor::Danger => colors.danger = value,
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
        for (color, hex) in ChromeColor::ALL
            .into_iter()
            .zip(preset.preset().colors(mode.is_dark()))
        {
            write_chrome_color(
                color,
                &mut colors,
                crate::parse_hex(hex).expect("built-in chrome preset colors are valid"),
            );
        }
    }
    for (color, value) in ChromeColor::ALL.into_iter().zip(overrides) {
        if let Some(value) = value {
            write_chrome_color(color, &mut colors, value);
        }
    }
    colors
}
