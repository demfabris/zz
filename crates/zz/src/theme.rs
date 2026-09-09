//! zz's theme overrides, layered over zz-ui's base palette.

use std::{ops::Range, sync::Arc};

use gpui::{
    App, FontStyle, FontWeight, Global, HighlightStyle, Hsla, Rgba, SharedString,
    StrikethroughStyle, StyledText, UnderlineStyle, Window, px,
};
use zz_mux::{TmuxAttributeState, TmuxColour, TmuxStyle, indexed_colour_rgb, parse_tmux_colour};
use zz_terminal::TerminalAppearance;
use zz_ui::{ActiveTheme as _, Colorize as _, Theme, ThemeMode};

use crate::config;

pub(crate) fn tmux_style_colour(style: &str, key: &str, fallback: Hsla, cx: &App) -> Hsla {
    let Some(value) = style.split(',').find_map(|part| {
        let (name, value) = part.split_once('=')?;
        name.eq_ignore_ascii_case(key).then_some(value)
    }) else {
        return fallback;
    };
    let Some(colour) = parse_tmux_colour(value) else {
        return fallback;
    };
    resolve_tmux_colour(colour, cx).unwrap_or(fallback)
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TmuxStyledText {
    text: SharedString,
    highlights: Vec<(Range<usize>, HighlightStyle)>,
}

impl TmuxStyledText {
    pub(crate) fn into_styled_text(self) -> StyledText {
        StyledText::new(self.text).with_highlights(self.highlights)
    }
}

pub(crate) fn tmux_styled_segments_text(
    segments: &[zz_mux::StyledSegment],
    base_foreground: Hsla,
    base_background: Hsla,
    cx: &App,
) -> TmuxStyledText {
    let mut text = String::new();
    let mut highlights = Vec::new();
    for segment in segments {
        let start = text.len();
        text.push_str(&segment.text);
        let highlight = tmux_highlight_style(&segment.style, base_foreground, base_background, cx);
        if highlight != HighlightStyle::default() {
            highlights.push((start..text.len(), highlight));
        }
    }
    TmuxStyledText {
        text: text.into(),
        highlights,
    }
}

fn tmux_highlight_style(
    style: &TmuxStyle,
    base_foreground: Hsla,
    base_background: Hsla,
    cx: &App,
) -> HighlightStyle {
    let mut color = style.fg.and_then(|colour| resolve_tmux_colour(colour, cx));
    let mut background_color = style.bg.and_then(|colour| resolve_tmux_colour(colour, cx));
    if style.attributes.reverse == TmuxAttributeState::On {
        let resolved_foreground = color.unwrap_or(base_foreground);
        let resolved_background = background_color.unwrap_or(base_background);
        color = Some(resolved_background);
        background_color = Some(resolved_foreground);
    }

    let underline = [
        style.attributes.underscore,
        style.attributes.double_underscore,
        style.attributes.curly_underscore,
        style.attributes.dotted_underscore,
        style.attributes.dashed_underscore,
    ]
    .into_iter()
    .any(|state| state == TmuxAttributeState::On)
    .then(|| UnderlineStyle {
        thickness: px(1.0),
        color: style.us.and_then(|colour| resolve_tmux_colour(colour, cx)),
        wavy: false,
    });

    HighlightStyle {
        color,
        font_weight: match style.attributes.bold {
            TmuxAttributeState::On => Some(FontWeight::BOLD),
            TmuxAttributeState::Off => Some(FontWeight::NORMAL),
            TmuxAttributeState::Unset => None,
        },
        font_style: match style.attributes.italics {
            TmuxAttributeState::On => Some(FontStyle::Italic),
            TmuxAttributeState::Off => Some(FontStyle::Normal),
            TmuxAttributeState::Unset => None,
        },
        background_color,
        underline,
        strikethrough: (style.attributes.strikethrough == TmuxAttributeState::On).then(|| {
            StrikethroughStyle {
                thickness: px(1.0),
                color: None,
            }
        }),
        fade_out: style.dim_percentage.map_or_else(
            || (style.attributes.dim == TmuxAttributeState::On).then_some(0.5),
            |percentage| Some(f32::from(percentage) / 100.0),
        ),
    }
}

fn resolve_tmux_colour(colour: TmuxColour, cx: &App) -> Option<Hsla> {
    match colour {
        TmuxColour::Basic(index) | TmuxColour::Indexed(index) => {
            Some(packed_tmux_colour(indexed_colour_rgb(index)))
        }
        TmuxColour::Rgb(colour) => Some(packed_tmux_colour(colour)),
        TmuxColour::Default | TmuxColour::Terminal => None,
        TmuxColour::Theme(index) => Some(match index {
            0 => cx.theme().background,
            1 | 7..=9 => cx.theme().foreground,
            2 => cx.theme().border(),
            3 => cx.theme().background.raised(1).opaque(),
            4 => cx.theme().success,
            5 => cx.theme().warning,
            6 => cx.theme().danger,
            _ => return None,
        }),
    }
}

fn packed_tmux_colour(colour: u32) -> Hsla {
    let channel = |shift: u32| {
        f32::from(u8::try_from((colour >> shift) & 0xff_u32).unwrap_or_default()) / 255.0
    };
    Rgba {
        r: channel(16),
        g: channel(8),
        b: channel(0),
        a: 1.0,
    }
    .into()
}

pub use zz_ui::chrome_palette::{
    CHROME_PRESETS, ChromeColor, ChromePreset, ChromePresetId, ThemeModeSetting,
    inherited_chrome_colors, resolved_chrome_colors,
};

#[derive(Clone, Copy)]
struct SystemThemeMode(ThemeMode);

impl Global for SystemThemeMode {}

#[derive(Clone)]
struct LatestTerminalAppearance(Arc<TerminalAppearance>);

impl Global for LatestTerminalAppearance {}

pub(crate) fn set_terminal_appearance(appearance: Arc<TerminalAppearance>, cx: &mut App) {
    cx.set_global(LatestTerminalAppearance(appearance));
    config::apply_window_background_appearance(cx);
    refresh_current_theme(cx);
}

pub(crate) fn terminal_appearance(cx: &App) -> Option<Arc<TerminalAppearance>> {
    cx.try_global::<LatestTerminalAppearance>()
        .map(|appearance| Arc::clone(&appearance.0))
}

pub(crate) fn chrome_blur(cx: &App) -> bool {
    config::resolved_config(cx).window_background_blur.value
        && crate::window::background::compositor_supports_blur(cx)
}

/// The chrome planes' fill: translucent while the blur is on, the base plane otherwise.
pub fn chrome_background(cx: &App) -> Hsla {
    zz_ui::shell::chrome_background(Theme::global(cx).background, chrome_blur(cx))
}

pub fn app_pane_background(cx: &App) -> Hsla {
    let theme = Theme::global(cx);
    theme
        .background
        .opaque()
        .opacity(theme.pane_background_opacity)
}

pub(crate) fn refresh_current_theme(cx: &mut App) {
    if !cx.has_global::<Theme>() {
        return;
    }
    let mode =
        zz_ui::chrome_palette::pinned_theme_mode(config::theme_mode(cx)).unwrap_or_else(|| {
            cx.try_global::<SystemThemeMode>()
                .map_or_else(|| Theme::global(cx).mode, |mode| mode.0)
        });
    Theme::change(mode, None, cx);
    apply_zz_overrides(cx);
    for window in cx.windows() {
        window
            .update(cx, |_, window, _| {
                window.set_default_corner_smoothing(CORNER_SMOOTHING);
                window.set_adaptive_corner_fraction(Some(ADAPTIVE_CORNER_FRACTION));
            })
            .ok();
    }
    cx.refresh_windows();
}

const CORNER_SMOOTHING: f32 = 4.0;

const ADAPTIVE_CORNER_FRACTION: f32 = 0.45;

/// Sync the zz-ui theme with the OS appearance, then reapply zz's overrides.
pub fn sync_system_appearance(mut window: Option<&mut Window>, cx: &mut App) {
    let system_mode = window
        .as_ref()
        .map_or_else(|| cx.window_appearance(), |window| window.appearance())
        .into();
    cx.set_global(SystemThemeMode(system_mode));
    if let Some(window) = window.as_deref_mut() {
        window.set_default_corner_smoothing(CORNER_SMOOTHING);
        window.set_adaptive_corner_fraction(Some(ADAPTIVE_CORNER_FRACTION));
    }
    Theme::sync_system_appearance(window, cx);
    crate::app_icon::apply(cx);
    if let Some(mode) = zz_ui::chrome_palette::pinned_theme_mode(config::theme_mode(cx)) {
        Theme::change(mode, None, cx);
    }
    apply_zz_overrides(cx);
}

fn apply_zz_overrides(cx: &mut App) {
    let appearance = cx
        .try_global::<LatestTerminalAppearance>()
        .map(|appearance| Arc::clone(&appearance.0));
    let terminal_mono_font_family = appearance
        .as_deref()
        .map(crate::terminal::view::terminal_font)
        .map(|font| font.family);
    let ui_font_family = config::ui_font_family(cx)
        .value
        .map_or_else(|| gpui::Font::default().family, SharedString::from);
    let widget_corner_radius = config::widget_corner_radius(cx);
    let chrome_contrast = config::chrome_contrast(cx);
    let shadow_strength = config::shadow_strength(cx);
    let pane_background_opacity = config::pane_background_opacity(cx);
    let chrome_preset = config::chrome_preset(cx);
    let chrome = config::chrome_colors(cx);
    let theme = Theme::global_mut(cx);

    theme.colors = resolved_chrome_colors(chrome_preset, theme.mode, chrome);
    theme.font_family = ui_font_family;
    if let Some(font_family) = terminal_mono_font_family {
        theme.mono_font_family = font_family;
    }

    theme.radius = widget_corner_radius;
    theme.set_contrast(chrome_contrast);
    theme.shadow_strength = shadow_strength;
    theme.pane_background_opacity = pane_background_opacity;
}

#[cfg(test)]
mod tests {
    use super::*;
    use zz_ui::ThemeColor;

    #[test]
    fn each_root_reads_back_exactly_what_it_wrote() {
        let base = *ThemeColor::dark();
        for color in ChromeColor::ALL {
            let marker = zz_ui::parse_hex("#808080").expect("test marker parses");
            let mut colors = base;
            zz_ui::chrome_palette::write_chrome_color(color, &mut colors, marker);

            assert_eq!(
                zz_ui::chrome_palette::read_chrome_color(color, &colors),
                marker,
                "{color:?} did not round-trip"
            );
            for other in ChromeColor::ALL.into_iter().filter(|it| *it != color) {
                assert_eq!(
                    zz_ui::chrome_palette::read_chrome_color(other, &colors),
                    zz_ui::chrome_palette::read_chrome_color(other, &base),
                    "writing {color:?} also changed {other:?}"
                );
            }
            assert_eq!(
                colors.scrim, base.scrim,
                "writing {color:?} moved the scrim"
            );
        }
    }

    #[test]
    fn paired_presets_land_on_the_roots_in_order() {
        let preset = &CHROME_PRESETS[0];
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            let expected = preset.colors(mode.is_dark());
            let colors = inherited_chrome_colors(Some(preset.id), mode);
            assert_eq!(zz_ui::to_hex(colors.background), expected[0]);
            assert_eq!(zz_ui::to_hex(colors.foreground), expected[1]);
            assert_eq!(zz_ui::to_hex(colors.danger), expected[4]);
        }
    }

    const SEPARATOR_DELTA_FLOOR_LIGHT: f32 = 0.045;
    const SEPARATOR_DELTA_FLOOR_DARK: f32 = 0.060;
    const SEPARATOR_DELTA_CEILING: f32 = 0.160;

    #[test]
    fn separators_stay_legible_without_reading_as_rules() {
        for preset in &CHROME_PRESETS {
            for mode in [ThemeMode::Light, ThemeMode::Dark] {
                let floor = if mode.is_dark() {
                    SEPARATOR_DELTA_FLOOR_DARK
                } else {
                    SEPARATOR_DELTA_FLOOR_LIGHT
                };
                let colors = preset.colors(mode.is_dark());
                let plane = zz_ui::parse_hex(colors[0]).expect("preset background parses");
                let hairline = inherited_chrome_colors(Some(preset.id), mode).border();
                let delta =
                    (zz_ui::oklab_lightness(hairline) - zz_ui::oklab_lightness(plane)).abs();

                assert!(
                    (floor..=SEPARATOR_DELTA_CEILING).contains(&delta),
                    "{} {mode:?}: border {} is {:.1}% from background {}, outside {:.1}%..={:.1}%",
                    preset.name,
                    zz_ui::to_hex(hairline),
                    delta * 100.0,
                    colors[0],
                    floor * 100.0,
                    SEPARATOR_DELTA_CEILING * 100.0,
                );
            }
        }
    }

    #[test]
    fn explicit_chrome_color_wins_over_the_active_preset_variant() {
        let marker = zz_ui::parse_hex("#808080").expect("test marker parses");
        let mut overrides = [None; ChromeColor::ALL.len()];
        overrides[0] = Some(marker);
        let colors = resolved_chrome_colors(
            Some(ChromePresetId::TokyoNight),
            ThemeMode::Light,
            overrides,
        );

        assert_eq!(colors.background, marker);
        assert_eq!(
            zz_ui::to_hex(colors.foreground),
            ChromePresetId::TokyoNight.preset().light[1]
        );
    }

    #[test]
    fn only_an_explicit_mode_pins_the_palette() {
        assert_eq!(
            zz_ui::chrome_palette::pinned_theme_mode(ThemeModeSetting::System),
            None
        );
        assert_eq!(
            zz_ui::chrome_palette::pinned_theme_mode(ThemeModeSetting::Light),
            Some(ThemeMode::Light)
        );
        assert_eq!(
            zz_ui::chrome_palette::pinned_theme_mode(ThemeModeSetting::Dark),
            Some(ThemeMode::Dark)
        );
    }

    #[gpui::test]
    fn returning_to_system_restores_the_last_os_mode(cx: &mut gpui::TestAppContext) {
        cx.update(zz_ui::init);
        cx.update(|cx| {
            cx.set_global(SystemThemeMode(ThemeMode::Dark));
            let mut config = config::AppConfig::default();
            config.theme_mode.value = ThemeModeSetting::Light;
            cx.set_global(config);
            refresh_current_theme(cx);
            assert_eq!(Theme::global(cx).mode, ThemeMode::Light);

            config.theme_mode.value = ThemeModeSetting::System;
            cx.set_global(config);
            refresh_current_theme(cx);
            assert_eq!(Theme::global(cx).mode, ThemeMode::Dark);
        });
    }

    #[gpui::test]
    fn terminal_opacity_never_reaches_chrome_or_app_panes(cx: &mut gpui::TestAppContext) {
        cx.update(zz_ui::init);
        cx.update(|cx| {
            set_terminal_appearance(
                Arc::new(TerminalAppearance {
                    background_opacity: 0.25,
                    ..TerminalAppearance::default()
                }),
                cx,
            );
        });

        cx.update(|cx| {
            assert!(!chrome_blur(cx));
            assert_alpha(Theme::global(cx).background, 1.0);
            assert_alpha(chrome_background(cx), 1.0);
            assert_alpha(app_pane_background(cx), 0.5);
        });

        cx.update(|cx| {
            let mut config = config::AppConfig::default();
            config.window_background_blur.value = true;
            cx.set_global(config);
            refresh_current_theme(cx);
        });

        cx.update(|cx| {
            assert!(chrome_blur(cx));
            assert_alpha(Theme::global(cx).background, 1.0);
            assert_alpha(chrome_background(cx), 0.93);
            assert_alpha(app_pane_background(cx), 0.5);
        });
    }

    #[gpui::test]
    fn app_panes_ignore_translucent_chrome_overrides(cx: &mut gpui::TestAppContext) {
        cx.update(zz_ui::init);
        cx.update(|cx| {
            Theme::global_mut(cx).colors.background =
                zz_ui::parse_hex("#10203066").expect("test background parses");

            assert_alpha(Theme::global(cx).background, 0.4);
            assert_alpha(app_pane_background(cx), 0.5);
        });
    }

    #[gpui::test]
    fn unsupported_compositor_keeps_the_chrome_opaque(cx: &mut gpui::TestAppContext) {
        cx.update(zz_ui::init);
        cx.update(|cx| {
            let mut config = config::AppConfig::default();
            config.window_background_blur.value = true;
            cx.set_global(config);
            crate::window::background::set_compositor_support_for_tests(false, cx);
        });

        cx.update(|cx| {
            assert!(!chrome_blur(cx));
            assert_alpha(chrome_background(cx), 1.0);
        });
    }

    #[track_caller]
    fn assert_alpha(color: Hsla, expected: f32) {
        assert!(
            (color.a - expected).abs() < f32::EPSILON,
            "alpha {} is not {expected}",
            color.a
        );
    }
}
