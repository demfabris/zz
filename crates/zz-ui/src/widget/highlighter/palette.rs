//! Light and dark syntax palettes, transcribed from gpui-component's `default-theme.json`.

use std::sync::{Arc, LazyLock};

use gpui::rgb;

use crate::ThemeMode;

use super::theme::{
    FontStyle, FontWeightContent, HighlightTheme, HighlightThemeStyle, SyntaxColors, ThemeStyle,
};

static LIGHT: LazyLock<Arc<HighlightTheme>> = LazyLock::new(|| Arc::new(light_theme()));
static DARK: LazyLock<Arc<HighlightTheme>> = LazyLock::new(|| Arc::new(dark_theme()));

pub(super) fn light() -> Arc<HighlightTheme> {
    LIGHT.clone()
}

pub(super) fn dark() -> Arc<HighlightTheme> {
    DARK.clone()
}

fn fg(hex: u32) -> Option<ThemeStyle> {
    Some(ThemeStyle {
        color: Some(rgb(hex).into()),
        font_style: None,
        font_weight: None,
    })
}

fn fg_style(hex: u32, font_style: FontStyle) -> Option<ThemeStyle> {
    Some(ThemeStyle {
        color: Some(rgb(hex).into()),
        font_style: Some(font_style),
        font_weight: None,
    })
}

fn fg_weight(hex: u32, font_weight: FontWeightContent) -> Option<ThemeStyle> {
    Some(ThemeStyle {
        color: Some(rgb(hex).into()),
        font_style: None,
        font_weight: Some(font_weight),
    })
}

fn face_style(font_style: FontStyle) -> Option<ThemeStyle> {
    Some(ThemeStyle {
        color: None,
        font_style: Some(font_style),
        font_weight: None,
    })
}

fn face_weight(font_weight: FontWeightContent) -> Option<ThemeStyle> {
    Some(ThemeStyle {
        color: None,
        font_style: None,
        font_weight: Some(font_weight),
    })
}

fn light_theme() -> HighlightTheme {
    HighlightTheme {
        name: "Default Light".to_string(),
        appearance: ThemeMode::Light,
        style: HighlightThemeStyle {
            syntax: SyntaxColors {
                attribute: fg(0x957931),
                boolean: fg(0xC5060B),
                comment: fg(0x007fff),
                constant: fg(0xC5060B),
                constructor: fg(0x0433ff),
                embedded: fg(0x333333),
                emphasis: face_style(FontStyle::Italic),
                emphasis_strong: face_weight(FontWeightContent::Bold),
                function: fg(0x0000A2),
                keyword: fg(0x0433ff),
                link_text: fg_style(0x0000A2, FontStyle::Normal),
                link_uri: fg_style(0x6A7293, FontStyle::Italic),
                number: fg(0x0433ff),
                property: fg(0x333333),
                string: fg(0x036A07),
                string_escape: fg(0x036A07),
                string_regex: fg(0x036A07),
                string_special: fg(0xd21f07),
                string_special_symbol: fg(0xd21f07),
                tag: fg(0x0433ff),
                text_code_span: fg(0x6F42C1),
                text_literal: fg(0x6F42C1),
                title: fg(0x0433FF),
                type_: fg(0x6f42c1),
                variable: fg(0x333333),
                variable_special: fg(0xC5060B),
                ..Default::default()
            },
            ..Default::default()
        },
    }
}

fn dark_theme() -> HighlightTheme {
    HighlightTheme {
        name: "Default Dark".to_string(),
        appearance: ThemeMode::Dark,
        style: HighlightThemeStyle {
            syntax: SyntaxColors {
                attribute: fg(0xe7cb8f),
                boolean: fg(0xE1D797),
                comment: fg(0x9E9E9E),
                constant: fg(0xE1D797),
                constructor: fg(0xb5af9a),
                embedded: fg(0xCACCCA),
                emphasis: face_style(FontStyle::Italic),
                emphasis_strong: face_weight(FontWeightContent::Bold),
                function: fg(0xfdd888),
                keyword: fg(0xc28b12),
                link_text: fg_style(0x307BF6, FontStyle::Normal),
                link_uri: fg_style(0x7faef9, FontStyle::Italic),
                number: fg(0xE1D797),
                property: fg(0xCACCCA),
                string: fg(0x62BA46),
                string_escape: fg(0x62BA46),
                string_regex: fg(0x62BA46),
                string_special: fg(0xE1D797),
                string_special_symbol: fg(0xE1D797),
                tag: fg(0xb5af9a),
                text_code_span: fg(0xE1D797),
                text_literal: fg(0xE1D797),
                title: fg_weight(0xfdd888, FontWeightContent::Semibold),
                type_: fg(0xc75828),
                variable_special: fg(0xE19773),
                ..Default::default()
            },
            ..Default::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_and_dark_differ() {
        assert_ne!(light(), dark());
        assert_eq!(light().appearance, ThemeMode::Light);
        assert_eq!(dark().appearance, ThemeMode::Dark);
    }

    #[test]
    fn dotted_capture_falls_back_to_its_prefix() {
        let light = light();

        assert_eq!(light.style("keyword.modifier"), light.style("keyword"));
        assert_ne!(light.style("string.special"), light.style("string"));
        assert!(light.style("nonsense.capture").is_none());
        assert!(light.style("").is_none());
    }
}
