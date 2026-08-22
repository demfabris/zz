//! zz's theme overrides, layered over zz-ui's base palette.

use std::{ops::Range, sync::Arc};

use gpui::{
    App, FontStyle, FontWeight, Global, HighlightStyle, Hsla, Rgba, SharedString,
    StrikethroughStyle, StyledText, UnderlineStyle, Window, px,
};
use zz_mux::{
    TmuxAttributeState, TmuxColour, TmuxStyle, indexed_colour_rgb, parse_styled_segments,
    parse_tmux_colour,
};
use zz_terminal::TerminalAppearance;
use zz_ui::{ActiveTheme as _, Colorize as _, Theme, ThemeColor, ThemeMode};

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
    pub(crate) text: SharedString,
    pub(crate) highlights: Vec<(Range<usize>, HighlightStyle)>,
    background_from_reverse: Vec<bool>,
}

impl TmuxStyledText {
    pub(crate) fn into_styled_text(self) -> StyledText {
        StyledText::new(self.text).with_highlights(self.highlights)
    }

    pub(crate) fn take_explicit_background_color(&mut self) -> Option<Hsla> {
        let background = self
            .highlights
            .iter()
            .zip(&self.background_from_reverse)
            .find_map(|((_, highlight), from_reverse)| {
                (!from_reverse)
                    .then_some(highlight.background_color)
                    .flatten()
            });
        for ((_, highlight), from_reverse) in self
            .highlights
            .iter_mut()
            .zip(&self.background_from_reverse)
        {
            if !from_reverse {
                highlight.background_color = None;
            }
        }
        background
    }
}

pub(crate) fn tmux_styled_text(
    value: &str,
    base_foreground: Hsla,
    base_background: Hsla,
    cx: &App,
) -> TmuxStyledText {
    tmux_styled_segments_text(
        &parse_styled_segments(value),
        base_foreground,
        base_background,
        cx,
    )
}

pub(crate) fn tmux_styled_segments_text(
    segments: &[zz_mux::StyledSegment],
    base_foreground: Hsla,
    base_background: Hsla,
    cx: &App,
) -> TmuxStyledText {
    let mut text = String::new();
    let mut highlights = Vec::new();
    let mut background_from_reverse = Vec::new();
    for segment in segments {
        let start = text.len();
        text.push_str(&segment.text);
        let highlight = tmux_highlight_style(&segment.style, base_foreground, base_background, cx);
        if highlight != HighlightStyle::default() {
            highlights.push((start..text.len(), highlight));
            background_from_reverse
                .push(segment.style.attributes.reverse == TmuxAttributeState::On);
        }
    }
    TmuxStyledText {
        text: text.into(),
        highlights,
        background_from_reverse,
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

pub(crate) fn resolve_tmux_colour(colour: TmuxColour, cx: &App) -> Option<Hsla> {
    match colour {
        TmuxColour::Basic(index) | TmuxColour::Indexed(index) => {
            Some(packed_tmux_colour(indexed_colour_rgb(index)))
        }
        TmuxColour::Rgb(colour) => Some(packed_tmux_colour(colour)),
        TmuxColour::Default | TmuxColour::Terminal => None,
        TmuxColour::Theme(index) => Some(match index {
            0 => cx.theme().background,
            1 | 7..=9 => cx.theme().foreground,
            2 => cx.theme().border,
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

/// A palette root a user can recolor from `zz/config`.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ChromeColor {
    Background,
    Foreground,
    Border,
    Success,
    Warning,
    Danger,
}

impl ChromeColor {
    pub const ALL: [Self; 6] = [
        Self::Background,
        Self::Foreground,
        Self::Border,
        Self::Success,
        Self::Warning,
        Self::Danger,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Background => "chrome-background",
            Self::Foreground => "chrome-foreground",
            Self::Border => "chrome-border",
            Self::Success => "chrome-success",
            Self::Warning => "chrome-warning",
            Self::Danger => "chrome-danger",
        }
    }

    pub(crate) fn from_str(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|color| color.as_str() == key)
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Background => "Background",
            Self::Foreground => "Foreground",
            Self::Border => "Border",
            Self::Success => "Success",
            Self::Warning => "Warning",
            Self::Danger => "Danger",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Background => {
                "The window's base plane. Every panel, popover and hover state is this color, \
                 raised."
            }
            Self::Foreground => {
                "Default text, and the source of muted text, focus rings, links and selection."
            }
            Self::Border => {
                "Every edge: panel borders, dividers, input outlines, the window frame."
            }
            Self::Success => "Something completed or is healthy.",
            Self::Warning => "Something needs attention but still works.",
            Self::Danger => "Something failed or is destructive.",
        }
    }

    pub const fn read(self, colors: &ThemeColor) -> Hsla {
        match self {
            Self::Background => colors.background,
            Self::Foreground => colors.foreground,
            Self::Border => colors.border,
            Self::Success => colors.success,
            Self::Warning => colors.warning,
            Self::Danger => colors.danger,
        }
    }

    const fn write(self, colors: &mut ThemeColor, value: Hsla) {
        match self {
            Self::Background => colors.background = value,
            Self::Foreground => colors.foreground = value,
            Self::Border => colors.border = value,
            Self::Success => colors.success = value,
            Self::Warning => colors.warning = value,
            Self::Danger => colors.danger = value,
        }
    }
}

/// What `theme-mode` selects.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ThemeModeSetting {
    /// Follow the OS appearance.
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeModeSetting {
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|mode| mode.as_str() == value.trim())
    }

    const fn pinned(self) -> Option<ThemeMode> {
        match self {
            Self::System => None,
            Self::Light => Some(ThemeMode::Light),
            Self::Dark => Some(ThemeMode::Dark),
        }
    }
}

/// A stable configuration spelling for a paired chrome preset.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChromePresetId {
    TokyoNight,
    Catppuccin,
    Gruvbox,
    Nord,
    Breeze,
    Adwaita,
    Ubuntu,
    RosePine,
    Ayu,
    Solarized,
    MacosClassic,
}

impl ChromePresetId {
    pub(crate) const ALL: [Self; 11] = [
        Self::TokyoNight,
        Self::Catppuccin,
        Self::Gruvbox,
        Self::Nord,
        Self::Breeze,
        Self::Adwaita,
        Self::Ubuntu,
        Self::RosePine,
        Self::Ayu,
        Self::Solarized,
        Self::MacosClassic,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TokyoNight => "tokyo-night",
            Self::Catppuccin => "catppuccin",
            Self::Gruvbox => "gruvbox",
            Self::Nord => "nord",
            Self::Breeze => "breeze",
            Self::Adwaita => "adwaita",
            Self::Ubuntu => "ubuntu",
            Self::RosePine => "rose-pine",
            Self::Ayu => "ayu",
            Self::Solarized => "solarized",
            Self::MacosClassic => "macos-classic",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|preset| preset.as_str() == value.trim())
    }

    pub(crate) fn preset(self) -> &'static ChromePreset {
        CHROME_PRESETS
            .iter()
            .find(|preset| preset.id == self)
            .expect("every ChromePresetId has a built-in preset")
    }
}

/// A named pair of light and dark chrome colors.
pub struct ChromePreset {
    pub id: ChromePresetId,
    pub name: &'static str,
    pub(crate) light: [&'static str; ChromeColor::ALL.len()],
    pub(crate) dark: [&'static str; ChromeColor::ALL.len()],
}

impl ChromePreset {
    pub fn colors(&self, mode: ThemeMode) -> &[&'static str; ChromeColor::ALL.len()] {
        if mode.is_dark() {
            &self.dark
        } else {
            &self.light
        }
    }
}

/// The built-in presets, each carrying its family's own upstream light and dark
/// palette: KDE `Breeze*.colors`, libadwaita CSS variables, Yaru's GTK
/// `@define-color` block, `ayu-theme/ayu-colors` `ui.*`, Schoonover's Solarized
/// base/accent table, `~/.files/colors`.
pub const CHROME_PRESETS: [ChromePreset; 11] = [
    ChromePreset {
        id: ChromePresetId::TokyoNight,
        name: "Tokyo Night",
        light: [
            "#e1e2e7", "#3760bf", "#b4b5b9", "#587539", "#8c6c3e", "#f52a65",
        ],
        dark: [
            "#1a1b26", "#c0caf5", "#292e42", "#9ece6a", "#e0af68", "#f7768e",
        ],
    },
    ChromePreset {
        id: ChromePresetId::Catppuccin,
        name: "Catppuccin",
        light: [
            "#eff1f5", "#4c4f69", "#ccd0da", "#40a02b", "#df8e1d", "#d20f39",
        ],
        dark: [
            "#1e1e2e", "#cdd6f4", "#313244", "#a6e3a1", "#f9e2af", "#f38ba8",
        ],
    },
    ChromePreset {
        id: ChromePresetId::Gruvbox,
        name: "Gruvbox",
        light: [
            "#fbf1c7", "#3c3836", "#e0d0aa", "#79740e", "#b57614", "#9d0006",
        ],
        dark: [
            "#282828", "#ebdbb2", "#3c3836", "#b8bb26", "#fabd2f", "#fb4934",
        ],
    },
    ChromePreset {
        id: ChromePresetId::Nord,
        name: "Nord",
        light: [
            "#eceff4", "#2e3440", "#d0d6e1", "#a3be8c", "#ebcb8b", "#bf616a",
        ],
        dark: [
            "#2e3440", "#eceff4", "#434c5e", "#a3be8c", "#ebcb8b", "#bf616a",
        ],
    },
    ChromePreset {
        id: ChromePresetId::Breeze,
        name: "Breeze",
        light: [
            "#eff0f1", "#232629", "#d0d2d3", "#27ae60", "#f67400", "#da4453",
        ],
        dark: [
            "#202326", "#fcfcfc", "#31363b", "#27ae60", "#f67400", "#da4453",
        ],
    },
    ChromePreset {
        id: ChromePresetId::Adwaita,
        name: "Adwaita",
        light: [
            "#fafafb", "#323237", "#dcdcdd", "#007c3d", "#905400", "#c30000",
        ],
        dark: [
            "#222226", "#ffffff", "#36363a", "#78e9ab", "#ffc252", "#ff938c",
        ],
    },
    ChromePreset {
        id: ChromePresetId::Ubuntu,
        name: "Ubuntu",
        light: [
            "#fafafa", "#3d3d3d", "#cccccc", "#109b26", "#f99b11", "#c7162b",
        ],
        dark: [
            "#2c2c2c", "#f7f7f7", "#4d4d4d", "#50c856", "#f99b11", "#ff5c5d",
        ],
    },
    ChromePreset {
        id: ChromePresetId::RosePine,
        name: "Rosé Pine",
        light: [
            "#faf4ed", "#464261", "#dfdad9", "#286983", "#ea9d34", "#b4637a",
        ],
        dark: [
            "#191724", "#e0def4", "#2e2c3c", "#31748f", "#f6c177", "#eb6f92",
        ],
    },
    ChromePreset {
        id: ChromePresetId::Ayu,
        name: "Ayu",
        light: [
            "#f8f9fa", "#5c6166", "#dce0e5", "#6cbf43", "#f29718", "#e65050",
        ],
        dark: [
            "#0d1017", "#bfbdb6", "#1e242f", "#70bf56", "#e6b450", "#d95757",
        ],
    },
    ChromePreset {
        id: ChromePresetId::Solarized,
        name: "Solarized",
        light: [
            "#fdf6e3", "#586e75", "#dfdccb", "#859900", "#b58900", "#dc322f",
        ],
        dark: [
            "#002b36", "#93a1a1", "#16404b", "#859900", "#b58900", "#dc322f",
        ],
    },
    ChromePreset {
        id: ChromePresetId::MacosClassic,
        name: "macOS Classic",
        light: [
            "#ffffff", "#1a1a1a", "#e0e0e0", "#036a07", "#9e7008", "#c5060b",
        ],
        dark: [
            "#131313", "#caccca", "#272727", "#62ba46", "#b0a878", "#d2602d",
        ],
    },
];

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

const BLURRED_CHROME_ALPHA: f32 = 0.9;

pub(crate) fn chrome_blur(cx: &App) -> bool {
    config::resolved_config(cx).window_background_blur.value
        && crate::window::background::compositor_supports_blur(cx)
}

/// The chrome planes' fill: translucent while the blur is on, the base plane otherwise.
pub fn chrome_background(cx: &App) -> Hsla {
    let background = Theme::global(cx).background;
    if chrome_blur(cx) {
        background.opacity(BLURRED_CHROME_ALPHA)
    } else {
        background
    }
}

pub fn app_pane_background(cx: &App) -> Hsla {
    Theme::global(cx).background.opaque()
}

pub(crate) fn refresh_current_theme(cx: &mut App) {
    if !cx.has_global::<Theme>() {
        return;
    }
    let mode = config::theme_mode(cx).pinned().unwrap_or_else(|| {
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
    if let Some(mode) = config::theme_mode(cx).pinned() {
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
    let widget_corner_radius = config::widget_corner_radius(cx);
    let chrome_preset = config::chrome_preset(cx);
    let chrome = config::chrome_colors(cx);
    let theme = Theme::global_mut(cx);

    theme.colors = resolved_chrome_colors(chrome_preset, theme.mode, chrome);
    if let Some(font_family) = terminal_mono_font_family {
        theme.mono_font_family = font_family;
    }

    theme.radius = widget_corner_radius;
}

/// The palette a root inherits before explicit `chrome-*` edits.
pub fn inherited_chrome_colors(preset: Option<ChromePresetId>, mode: ThemeMode) -> ThemeColor {
    resolved_chrome_colors(preset, mode, [None; ChromeColor::ALL.len()])
}

fn resolved_chrome_colors(
    preset: Option<ChromePresetId>,
    mode: ThemeMode,
    overrides: [Option<Hsla>; ChromeColor::ALL.len()],
) -> ThemeColor {
    let mut colors = *ThemeColor::for_mode(mode);
    if let Some(preset) = preset {
        for (color, hex) in ChromeColor::ALL
            .into_iter()
            .zip(preset.preset().colors(mode))
        {
            color.write(
                &mut colors,
                zz_ui::parse_hex(hex).expect("built-in chrome preset colors are valid"),
            );
        }
    }
    for (color, value) in ChromeColor::ALL.into_iter().zip(overrides) {
        if let Some(value) = value {
            color.write(&mut colors, value);
        }
    }
    colors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn tmux_runs_map_colours_and_text_attributes(cx: &mut gpui::TestAppContext) {
        cx.update(zz_ui::init);
        cx.update(|cx| {
            let base_foreground = cx.theme().foreground.muted();
            let base_background = cx.theme().background;
            let styled = tmux_styled_text(
                "#[fg=#123456,bg=blue,us=green,bold,italics,underscore,strikethrough,dim]styled",
                base_foreground,
                base_background,
                cx,
            );

            assert_eq!(styled.text.as_ref(), "styled");
            assert_eq!(styled.highlights.len(), 1);
            assert_eq!(styled.highlights[0].0, 0..6);
            let highlight = styled.highlights[0].1;
            assert_eq!(highlight.color, Some(packed_tmux_colour(0x12_34_56)));
            assert_eq!(
                highlight.background_color,
                Some(packed_tmux_colour(indexed_colour_rgb(4)))
            );
            assert_eq!(highlight.font_weight, Some(FontWeight::BOLD));
            assert_eq!(highlight.font_style, Some(FontStyle::Italic));
            assert_eq!(highlight.fade_out, Some(0.5));
            assert_eq!(
                highlight.underline.expect("underline").color,
                Some(packed_tmux_colour(indexed_colour_rgb(2)))
            );
            assert!(highlight.strikethrough.is_some());
        });
    }

    #[gpui::test]
    fn tmux_runs_reverse_explicit_and_inherited_colours(cx: &mut gpui::TestAppContext) {
        cx.update(zz_ui::init);
        cx.update(|cx| {
            let base_foreground = cx.theme().foreground.muted();
            let base_background = cx.theme().background;
            let explicit = tmux_styled_text(
                "#[fg=red,bg=blue,reverse]x",
                base_foreground,
                base_background,
                cx,
            );
            let explicit = explicit.highlights[0].1;
            assert_eq!(
                explicit.color,
                Some(packed_tmux_colour(indexed_colour_rgb(4)))
            );
            assert_eq!(
                explicit.background_color,
                Some(packed_tmux_colour(indexed_colour_rgb(1)))
            );

            let inherited = tmux_styled_text("#[reverse]x", base_foreground, base_background, cx);
            let inherited = inherited.highlights[0].1;
            assert_eq!(inherited.color, Some(base_background));
            assert_eq!(inherited.background_color, Some(base_foreground));
        });
    }

    #[gpui::test]
    fn tmux_reverse_backgrounds_stay_on_runs_instead_of_becoming_pill_tints(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(zz_ui::init);
        cx.update(|cx| {
            let base_foreground = cx.theme().foreground.muted();
            let base_background = cx.theme().background.washed(1);
            for value in ["#[reverse]bell", "#[default reverse]#[push-default]bell"] {
                let mut styled = tmux_styled_text(value, base_foreground, base_background, cx);

                assert_eq!(styled.take_explicit_background_color(), None);
                assert_eq!(styled.highlights.len(), 1);
                assert_eq!(styled.highlights[0].1.color, Some(base_background));
                assert_eq!(
                    styled.highlights[0].1.background_color,
                    Some(base_foreground)
                );
            }

            let mut explicit =
                tmux_styled_text("#[bg=blue]styled", base_foreground, base_background, cx);
            assert_eq!(
                explicit.take_explicit_background_color(),
                Some(packed_tmux_colour(indexed_colour_rgb(4)))
            );
            assert_eq!(explicit.highlights[0].1.background_color, None);
        });
    }

    #[gpui::test]
    fn tmux_runs_use_utf8_byte_ranges(cx: &mut gpui::TestAppContext) {
        cx.update(zz_ui::init);
        cx.update(|cx| {
            let styled = tmux_styled_text(
                "#[fg=red]你",
                cx.theme().foreground.muted(),
                cx.theme().background,
                cx,
            );

            assert_eq!(styled.text.as_ref(), "你");
            assert_eq!(styled.highlights.len(), 1);
            assert_eq!(styled.highlights[0].0, 0..3);
            assert_eq!(
                styled.highlights[0].1.color,
                Some(packed_tmux_colour(indexed_colour_rgb(1)))
            );
        });
    }

    #[gpui::test]
    fn tmux_runs_reset_to_default_and_map_theme_colours(cx: &mut gpui::TestAppContext) {
        cx.update(zz_ui::init);
        cx.update(|cx| {
            let base_foreground = cx.theme().foreground.muted();
            let base_background = cx.theme().background;
            let styled = tmux_styled_text(
                "plain#[fg=themegreen,bg=themeyellow,bold]styled#[default] reset",
                base_foreground,
                base_background,
                cx,
            );

            assert_eq!(styled.text.as_ref(), "plainstyled reset");
            assert_eq!(styled.highlights.len(), 1);
            assert_eq!(styled.highlights[0].0, 5..11);
            assert_eq!(styled.highlights[0].1.color, Some(cx.theme().success));
            assert_eq!(
                styled.highlights[0].1.background_color,
                Some(cx.theme().warning)
            );
            assert_eq!(styled.highlights[0].1.font_weight, Some(FontWeight::BOLD));
        });
    }

    #[gpui::test]
    fn tmux_runs_default_returns_to_the_pushed_style(cx: &mut gpui::TestAppContext) {
        cx.update(zz_ui::init);
        cx.update(|cx| {
            let styled = tmux_styled_text(
                "#[fg=themegreen,bold]#[push-default]base#[fg=red]override#[default]base",
                cx.theme().foreground.muted(),
                cx.theme().background,
                cx,
            );

            assert_eq!(styled.text.as_ref(), "baseoverridebase");
            assert_eq!(styled.highlights.len(), 3);
            assert_eq!(styled.highlights[0].0, 0..4);
            assert_eq!(styled.highlights[0].1.color, Some(cx.theme().success));
            assert_eq!(styled.highlights[1].0, 4..12);
            assert_eq!(
                styled.highlights[1].1.color,
                Some(packed_tmux_colour(indexed_colour_rgb(1)))
            );
            assert_eq!(styled.highlights[2].0, 12..16);
            assert_eq!(styled.highlights[2].1.color, Some(cx.theme().success));
            assert!(
                styled
                    .highlights
                    .iter()
                    .all(|(_, highlight)| highlight.font_weight == Some(FontWeight::BOLD))
            );
        });
    }

    #[gpui::test]
    fn tmux_runs_leave_unstyled_text_and_spaces_untouched(cx: &mut gpui::TestAppContext) {
        cx.update(zz_ui::init);
        cx.update(|cx| {
            let styled = tmux_styled_text(
                "  unchanged  ",
                cx.theme().foreground.muted(),
                cx.theme().background,
                cx,
            );
            assert_eq!(styled.text.as_ref(), "  unchanged  ");
            assert!(styled.highlights.is_empty());

            let inherited = tmux_styled_text(
                "#[fg=default,bg=terminal]inherited",
                cx.theme().foreground.muted(),
                cx.theme().background,
                cx,
            );
            assert_eq!(inherited.text.as_ref(), "inherited");
            assert!(inherited.highlights.is_empty());
        });
    }

    #[test]
    fn each_root_reads_back_exactly_what_it_wrote() {
        let base = *ThemeColor::dark();
        for color in ChromeColor::ALL {
            let marker = zz_ui::parse_hex("#808080").expect("test marker parses");
            let mut colors = base;
            color.write(&mut colors, marker);

            assert_eq!(color.read(&colors), marker, "{color:?} did not round-trip");
            for other in ChromeColor::ALL.into_iter().filter(|it| *it != color) {
                assert_eq!(
                    other.read(&colors),
                    other.read(&base),
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
            let expected = preset.colors(mode);
            let colors = inherited_chrome_colors(Some(preset.id), mode);
            assert_eq!(zz_ui::to_hex(colors.background), expected[0]);
            assert_eq!(zz_ui::to_hex(colors.foreground), expected[1]);
            assert_eq!(zz_ui::to_hex(colors.border), expected[2]);
            assert_eq!(zz_ui::to_hex(colors.danger), expected[5]);
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
                let colors = preset.colors(mode);
                let plane = zz_ui::parse_hex(colors[0]).expect("preset background parses");
                let hairline = zz_ui::parse_hex(colors[2]).expect("preset border parses");
                let delta =
                    (zz_ui::oklab_lightness(hairline) - zz_ui::oklab_lightness(plane)).abs();

                assert!(
                    (floor..=SEPARATOR_DELTA_CEILING).contains(&delta),
                    "{} {mode:?}: border {} is {:.1}% from background {}, outside {:.1}%..={:.1}%",
                    preset.name,
                    colors[2],
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
        assert_eq!(ThemeModeSetting::System.pinned(), None);
        assert_eq!(ThemeModeSetting::Light.pinned(), Some(ThemeMode::Light));
        assert_eq!(ThemeModeSetting::Dark.pinned(), Some(ThemeMode::Dark));
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
                    background_opacity: 0.5,
                    ..TerminalAppearance::default()
                }),
                cx,
            );
        });

        cx.update(|cx| {
            assert!(!chrome_blur(cx));
            assert_alpha(Theme::global(cx).background, 1.0);
            assert_alpha(chrome_background(cx), 1.0);
            assert_alpha(app_pane_background(cx), 1.0);
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
            assert_alpha(chrome_background(cx), BLURRED_CHROME_ALPHA);
            assert_alpha(app_pane_background(cx), 1.0);
        });
    }

    #[gpui::test]
    fn app_panes_ignore_translucent_chrome_overrides(cx: &mut gpui::TestAppContext) {
        cx.update(zz_ui::init);
        cx.update(|cx| {
            Theme::global_mut(cx).colors.background =
                zz_ui::parse_hex("#10203066").expect("test background parses");

            assert_alpha(Theme::global(cx).background, 0.4);
            assert_alpha(app_pane_background(cx), 1.0);
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
