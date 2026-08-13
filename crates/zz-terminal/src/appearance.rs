use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    ffi::OsStr,
    fmt, fs,
    hash::{Hash, Hasher},
    io::Read as _,
    marker::PhantomData,
    ops::{Index, IndexMut},
    path::{Path, PathBuf},
    sync::OnceLock,
};

use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{Error as _, SeqAccess, Visitor},
    ser::SerializeTuple,
};

use crate::{Color, CursorStyle};

const MAX_CONFIG_DEPTH: usize = 16;
const MAX_CONFIG_BYTES: usize = 1024 * 1024;
const MAX_CONFIG_DIAGNOSTICS: usize = 1024;
const MAX_FONT_FAMILIES: usize = 32;
const MAX_FONT_FAMILY_BYTES: usize = 256;
const MAX_FONT_FEATURES: usize = 64;
const MAX_FONT_SIZE_POINTS: f32 = 256.0;
const MAX_PADDING: f32 = 1024.0;
const MAX_ADJUSTMENT: f32 = 1024.0;
const MAX_BLINK_INTERVAL_MS: u32 = 60_000;

/// Appearance keys accepted by the daemon-owned `zz/config` override layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum AppearanceConfigKey {
    Theme,
    Background,
    Foreground,
    CursorColor,
    SelectionForeground,
    SelectionBackground,
    Palette,
    FontFamily,
    FontFamilyBold,
    FontFamilyItalic,
    FontFamilyBoldItalic,
    FontSize,
    FontFeature,
    FontSyntheticStyle,
    FontThicken,
    FontThickenStrength,
    AdjustCellHeight,
    WindowPaddingX,
    WindowPaddingY,
    MinimumContrast,
    BackgroundOpacity,
    CursorStyle,
    CursorStyleBlink,
    ZzFontWeight,
    ZzCursorBlinkIntervalMs,
    ZzSearchMatchColor,
    ZzSearchCurrentColor,
    ZzLinkColor,
    ZzCopyCursorColor,
    ZzRoundedSelection,
}

impl AppearanceConfigKey {
    pub const ALL: [Self; 30] = [
        Self::Theme,
        Self::Background,
        Self::Foreground,
        Self::CursorColor,
        Self::SelectionForeground,
        Self::SelectionBackground,
        Self::Palette,
        Self::FontFamily,
        Self::FontFamilyBold,
        Self::FontFamilyItalic,
        Self::FontFamilyBoldItalic,
        Self::FontSize,
        Self::FontFeature,
        Self::FontSyntheticStyle,
        Self::FontThicken,
        Self::FontThickenStrength,
        Self::AdjustCellHeight,
        Self::WindowPaddingX,
        Self::WindowPaddingY,
        Self::MinimumContrast,
        Self::BackgroundOpacity,
        Self::CursorStyle,
        Self::CursorStyleBlink,
        Self::ZzFontWeight,
        Self::ZzCursorBlinkIntervalMs,
        Self::ZzSearchMatchColor,
        Self::ZzSearchCurrentColor,
        Self::ZzLinkColor,
        Self::ZzCopyCursorColor,
        Self::ZzRoundedSelection,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Theme => "theme",
            Self::Background => "background",
            Self::Foreground => "foreground",
            Self::CursorColor => "cursor-color",
            Self::SelectionForeground => "selection-foreground",
            Self::SelectionBackground => "selection-background",
            Self::Palette => "palette",
            Self::FontFamily => "font-family",
            Self::FontFamilyBold => "font-family-bold",
            Self::FontFamilyItalic => "font-family-italic",
            Self::FontFamilyBoldItalic => "font-family-bold-italic",
            Self::FontSize => "font-size",
            Self::FontFeature => "font-feature",
            Self::FontSyntheticStyle => "font-synthetic-style",
            Self::FontThicken => "font-thicken",
            Self::FontThickenStrength => "font-thicken-strength",
            Self::AdjustCellHeight => "adjust-cell-height",
            Self::WindowPaddingX => "window-padding-x",
            Self::WindowPaddingY => "window-padding-y",
            Self::MinimumContrast => "minimum-contrast",
            Self::BackgroundOpacity => "background-opacity",
            Self::CursorStyle => "cursor-style",
            Self::CursorStyleBlink => "cursor-style-blink",
            Self::ZzFontWeight => "zz-font-weight",
            Self::ZzCursorBlinkIntervalMs => "zz-cursor-blink-interval-ms",
            Self::ZzSearchMatchColor => "zz-search-match-color",
            Self::ZzSearchCurrentColor => "zz-search-current-color",
            Self::ZzLinkColor => "zz-link-color",
            Self::ZzCopyCursorColor => "zz-copy-cursor-color",
            Self::ZzRoundedSelection => "zz-rounded-selection",
        }
    }

    #[must_use]
    pub fn from_config_key(key: &str) -> Option<Self> {
        match key {
            "theme" => Some(Self::Theme),
            "background" => Some(Self::Background),
            "foreground" => Some(Self::Foreground),
            "cursor-color" => Some(Self::CursorColor),
            "selection-foreground" => Some(Self::SelectionForeground),
            "selection-background" => Some(Self::SelectionBackground),
            "palette" => Some(Self::Palette),
            "font-family" => Some(Self::FontFamily),
            "font-family-bold" => Some(Self::FontFamilyBold),
            "font-family-italic" => Some(Self::FontFamilyItalic),
            "font-family-bold-italic" => Some(Self::FontFamilyBoldItalic),
            "font-size" => Some(Self::FontSize),
            "font-feature" => Some(Self::FontFeature),
            "font-synthetic-style" => Some(Self::FontSyntheticStyle),
            "font-thicken" => Some(Self::FontThicken),
            "font-thicken-strength" => Some(Self::FontThickenStrength),
            "adjust-cell-height" => Some(Self::AdjustCellHeight),
            "window-padding-x" => Some(Self::WindowPaddingX),
            "window-padding-y" => Some(Self::WindowPaddingY),
            "minimum-contrast" => Some(Self::MinimumContrast),
            "background-opacity" => Some(Self::BackgroundOpacity),
            "cursor-style" => Some(Self::CursorStyle),
            "cursor-style-blink" => Some(Self::CursorStyleBlink),
            "zz-font-weight" => Some(Self::ZzFontWeight),
            "zz-cursor-blink-interval-ms" => Some(Self::ZzCursorBlinkIntervalMs),
            "zz-search-match-color" => Some(Self::ZzSearchMatchColor),
            "zz-search-current-color" => Some(Self::ZzSearchCurrentColor),
            "zz-link-color" => Some(Self::ZzLinkColor),
            "zz-copy-cursor-color" => Some(Self::ZzCopyCursorColor),
            "zz-rounded-selection" => Some(Self::ZzRoundedSelection),
            _ => None,
        }
    }
}

/// The layer that supplied one effective appearance setting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppearanceSource {
    #[default]
    Default,
    ThemeFile,
    Ghostty,
    Override,
}

/// Complete per-key provenance for a resolved terminal appearance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AppearanceProvenance(BTreeMap<AppearanceConfigKey, AppearanceSource>);

impl Default for AppearanceProvenance {
    fn default() -> Self {
        Self(
            AppearanceConfigKey::ALL
                .into_iter()
                .map(|key| (key, AppearanceSource::Default))
                .collect(),
        )
    }
}

impl AppearanceProvenance {
    #[must_use]
    pub fn source(&self, key: AppearanceConfigKey) -> AppearanceSource {
        self.0.get(&key).copied().unwrap_or_default()
    }

    pub fn set_source(&mut self, key: AppearanceConfigKey, source: AppearanceSource) {
        self.0.insert(key, source);
    }

    /// Ensure a wire payload contains exactly one source for every supported key.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.0.len() != AppearanceConfigKey::ALL.len()
            || AppearanceConfigKey::ALL
                .iter()
                .any(|key| !self.0.contains_key(key))
        {
            return Err("appearance provenance must contain every supported key exactly once");
        }
        Ok(())
    }
}

/// A compact RGBA color used by renderer-owned translucent presentation.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AppearanceColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl AppearanceColor {
    #[must_use]
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    #[must_use]
    pub const fn opaque(color: Color) -> Self {
        Self::rgba(color.r, color.g, color.b, u8::MAX)
    }

    #[must_use]
    pub const fn rgb(self) -> Color {
        Color::rgb(self.r, self.g, self.b)
    }
}

/// One validated OpenType feature assignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FontFeature {
    pub tag: [u8; 4],
    pub value: u32,
}

impl FontFeature {
    #[must_use]
    pub const fn new(tag: [u8; 4], value: u32) -> Self {
        Self { tag, value }
    }

    #[must_use]
    pub fn tag_string(self) -> String {
        String::from_utf8_lossy(&self.tag).into_owned()
    }
}

/// Synthetic font styles Ghostty may create when a configured family lacks a native face.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FontSyntheticStyle {
    pub bold: bool,
    pub italic: bool,
    pub bold_italic: bool,
}

impl Default for FontSyntheticStyle {
    fn default() -> Self {
        Self {
            bold: true,
            italic: true,
            bold_italic: true,
        }
    }
}

impl FontSyntheticStyle {
    #[must_use]
    pub const fn allows(self, bold: bool, italic: bool) -> bool {
        match (bold, italic) {
            (true, true) => self.bold_italic,
            (true, false) => self.bold,
            (false, true) => self.italic,
            (false, false) => false,
        }
    }
}

/// How an `adjust-cell-height` value modifies the font-derived line height.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum CellHeightAdjustment {
    #[default]
    None,
    Pixels(f32),
    Percent(f32),
}

impl CellHeightAdjustment {
    #[must_use]
    pub fn apply(self, base: f32) -> f32 {
        match self {
            Self::None => base,
            Self::Pixels(delta) => base + delta,
            Self::Percent(percent) => base * (1.0 + percent / 100.0),
        }
    }
}

/// GPUI-local cursor blink policy resolved by the daemon.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CursorBlinkPolicy {
    Off,
    On,
    #[default]
    Terminal,
}

/// The system color scheme used to resolve adaptive Ghostty themes.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TerminalColorScheme {
    Light,
    #[default]
    Dark,
}

impl TerminalColorScheme {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

/// A fixed-size terminal palette with an exact-length wire representation.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TerminalPalette([Color; 256]);

impl TerminalPalette {
    #[must_use]
    pub const fn new(colors: [Color; 256]) -> Self {
        Self(colors)
    }

    #[must_use]
    pub const fn as_array(&self) -> &[Color; 256] {
        &self.0
    }

    #[must_use]
    pub const fn into_array(self) -> [Color; 256] {
        self.0
    }
}

impl fmt::Debug for TerminalPalette {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TerminalPalette")
            .field("hash", &hash_palette(&self.0))
            .finish()
    }
}

impl Index<usize> for TerminalPalette {
    type Output = Color;

    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index]
    }
}

impl IndexMut<usize> for TerminalPalette {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.0[index]
    }
}

impl Serialize for TerminalPalette {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut tuple = serializer.serialize_tuple(256)?;
        for color in &self.0 {
            tuple.serialize_element(color)?;
        }
        tuple.end()
    }
}

impl<'de> Deserialize<'de> for TerminalPalette {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct PaletteVisitor;

        impl<'de> Visitor<'de> for PaletteVisitor {
            type Value = TerminalPalette;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("exactly 256 terminal palette colors")
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut colors = Vec::with_capacity(256);
                for index in 0..256 {
                    colors.push(
                        sequence.next_element()?.ok_or_else(|| {
                            A::Error::invalid_length(index, &"exactly 256 colors")
                        })?,
                    );
                }
                if sequence.next_element::<Color>()?.is_some() {
                    return Err(A::Error::invalid_length(257, &"exactly 256 colors"));
                }
                let colors: [Color; 256] = colors.try_into().map_err(|colors: Vec<Color>| {
                    A::Error::invalid_length(colors.len(), &"exactly 256 colors")
                })?;
                Ok(TerminalPalette(colors))
            }
        }

        deserializer.deserialize_tuple(256, PaletteVisitor)
    }
}

/// Renderer-neutral appearance shared by terminal actors and GPUI clients.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct TerminalAppearance {
    pub color_scheme: TerminalColorScheme,
    #[serde(deserialize_with = "deserialize_font_families")]
    pub font_families: Vec<String>,
    #[serde(deserialize_with = "deserialize_font_families")]
    pub font_families_bold: Vec<String>,
    #[serde(deserialize_with = "deserialize_font_families")]
    pub font_families_italic: Vec<String>,
    #[serde(deserialize_with = "deserialize_font_families")]
    pub font_families_bold_italic: Vec<String>,
    pub font_size_points: f32,
    pub font_weight: u16,
    #[serde(deserialize_with = "deserialize_font_features")]
    pub font_features: Vec<FontFeature>,
    pub font_synthetic_style: FontSyntheticStyle,
    pub font_thicken: bool,
    pub font_thicken_strength: u8,
    pub cell_height_adjustment: CellHeightAdjustment,
    pub padding_left: f32,
    pub padding_right: f32,
    pub padding_top: f32,
    pub padding_bottom: f32,
    pub foreground: Color,
    pub background: Color,
    pub cursor_color: Color,
    pub cursor_style: CursorStyle,
    pub selection_foreground: Color,
    pub selection_background: AppearanceColor,
    pub search_match_color: AppearanceColor,
    pub search_current_color: AppearanceColor,
    pub link_color: Color,
    pub copy_cursor_color: AppearanceColor,
    pub palette: TerminalPalette,
    pub minimum_contrast: f32,
    pub cursor_blink_policy: CursorBlinkPolicy,
    pub cursor_blink_interval_ms: u32,
    pub rounded_selection: bool,
    pub background_opacity: f32,
}

struct BoundedFontFamily(String);

impl<'de> Deserialize<'de> for BoundedFontFamily {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct FontFamilyVisitor;

        impl<'de> Visitor<'de> for FontFamilyVisitor {
            type Value = BoundedFontFamily;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(
                    formatter,
                    "a terminal font family no longer than {MAX_FONT_FAMILY_BYTES} bytes"
                )
            }

            fn visit_borrowed_str<E>(self, value: &'de str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_str(value)
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if value.len() > MAX_FONT_FAMILY_BYTES {
                    return Err(E::invalid_length(value.len(), &self));
                }
                Ok(BoundedFontFamily(value.to_owned()))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if value.len() > MAX_FONT_FAMILY_BYTES {
                    return Err(E::invalid_length(value.len(), &self));
                }
                Ok(BoundedFontFamily(value))
            }
        }

        deserializer.deserialize_str(FontFamilyVisitor)
    }
}

fn deserialize_font_families<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<D, BoundedFontFamily>(
        deserializer,
        MAX_FONT_FAMILIES,
        "at most 32 terminal font families",
    )
    .map(|families| families.into_iter().map(|family| family.0).collect())
}

fn deserialize_font_features<'de, D>(deserializer: D) -> Result<Vec<FontFeature>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec(
        deserializer,
        MAX_FONT_FEATURES,
        "at most 64 terminal font features",
    )
}

fn deserialize_bounded_vec<'de, D, T>(
    deserializer: D,
    maximum: usize,
    expected: &'static str,
) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct BoundedVecVisitor<T> {
        maximum: usize,
        expected: &'static str,
        marker: PhantomData<fn() -> T>,
    }

    impl<'de, T> Visitor<'de> for BoundedVecVisitor<T>
    where
        T: Deserialize<'de>,
    {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(self.expected)
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let hint = sequence.size_hint();
            if hint.is_some_and(|length| length > self.maximum) {
                return Err(A::Error::invalid_length(
                    hint.unwrap_or(self.maximum.saturating_add(1)),
                    &self,
                ));
            }
            let mut values = Vec::with_capacity(hint.unwrap_or(0).min(self.maximum));
            while values.len() < self.maximum {
                let Some(value) = sequence.next_element()? else {
                    return Ok(values);
                };
                values.push(value);
            }
            if sequence.next_element::<serde::de::IgnoredAny>()?.is_some() {
                return Err(A::Error::invalid_length(
                    self.maximum.saturating_add(1),
                    &self,
                ));
            }
            Ok(values)
        }
    }

    deserializer.deserialize_seq(BoundedVecVisitor {
        maximum,
        expected,
        marker: PhantomData,
    })
}

impl fmt::Debug for TerminalAppearance {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TerminalAppearance")
            .field("color_scheme", &self.color_scheme)
            .field("font_families", &self.font_families)
            .field("font_families_bold", &self.font_families_bold)
            .field("font_families_italic", &self.font_families_italic)
            .field("font_families_bold_italic", &self.font_families_bold_italic)
            .field("font_size_points", &self.font_size_points)
            .field("font_weight", &self.font_weight)
            .field("font_features", &self.font_features)
            .field("font_synthetic_style", &self.font_synthetic_style)
            .field("font_thicken", &self.font_thicken)
            .field("font_thicken_strength", &self.font_thicken_strength)
            .field("cell_height_adjustment", &self.cell_height_adjustment)
            .field("padding_left", &self.padding_left)
            .field("padding_right", &self.padding_right)
            .field("padding_top", &self.padding_top)
            .field("padding_bottom", &self.padding_bottom)
            .field("foreground", &self.foreground)
            .field("background", &self.background)
            .field("cursor_color", &self.cursor_color)
            .field("cursor_style", &self.cursor_style)
            .field("selection_foreground", &self.selection_foreground)
            .field("selection_background", &self.selection_background)
            .field("search_match_color", &self.search_match_color)
            .field("search_current_color", &self.search_current_color)
            .field("link_color", &self.link_color)
            .field("copy_cursor_color", &self.copy_cursor_color)
            .field("palette_hash", &hash_palette(self.palette.as_array()))
            .field("minimum_contrast", &self.minimum_contrast)
            .field("cursor_blink_policy", &self.cursor_blink_policy)
            .field("cursor_blink_interval_ms", &self.cursor_blink_interval_ms)
            .field("rounded_selection", &self.rounded_selection)
            .field("background_opacity", &self.background_opacity)
            .finish()
    }
}

impl Default for TerminalAppearance {
    fn default() -> Self {
        let foreground = Color::rgb(0xd8, 0xde, 0xe9);
        let background = Color::rgb(0x10, 0x13, 0x18);
        Self {
            color_scheme: TerminalColorScheme::Dark,
            font_families: vec![default_font_family().to_owned()],
            font_families_bold: Vec::new(),
            font_families_italic: Vec::new(),
            font_families_bold_italic: Vec::new(),
            // Ghostty's default font size, in points.
            font_size_points: 13.0,
            font_weight: 400,
            font_features: Vec::new(),
            font_synthetic_style: FontSyntheticStyle::default(),
            font_thicken: false,
            font_thicken_strength: u8::MAX,
            cell_height_adjustment: CellHeightAdjustment::None,
            padding_left: 10.0,
            padding_right: 10.0,
            padding_top: 10.0,
            padding_bottom: 10.0,
            foreground,
            background,
            cursor_color: Color::rgb(0xe5, 0xc0, 0x7b),
            cursor_style: CursorStyle::Block,
            selection_foreground: foreground,
            selection_background: AppearanceColor::rgba(0x50, 0x7d, 0xb8, 0x94),
            search_match_color: AppearanceColor::rgba(0xe0, 0xab, 0x38, 0x6b),
            search_current_color: AppearanceColor::rgba(0xfa, 0xc2, 0x47, 0xad),
            link_color: Color::rgb(0x4d, 0xa3, 0xeb),
            copy_cursor_color: AppearanceColor::rgba(0xb8, 0xc7, 0xe0, 0x6b),
            palette: TerminalPalette::new(default_palette()),
            minimum_contrast: 1.0,
            cursor_blink_policy: CursorBlinkPolicy::Terminal,
            cursor_blink_interval_ms: 500,
            rounded_selection: true,
            background_opacity: 1.0,
        }
    }
}

impl TerminalAppearance {
    /// Validate all dimensions and bounded variable-length fields received over IPC.
    pub fn validate(&self) -> Result<(), AppearanceValidationError> {
        if self.font_families.is_empty() || self.font_families.len() > MAX_FONT_FAMILIES {
            return Err(AppearanceValidationError::FontFamilies);
        }
        if [
            self.font_families.as_slice(),
            self.font_families_bold.as_slice(),
            self.font_families_italic.as_slice(),
            self.font_families_bold_italic.as_slice(),
        ]
        .into_iter()
        .any(|families| {
            families.len() > MAX_FONT_FAMILIES
                || families.iter().any(|family| {
                    family.is_empty()
                        || family.len() > MAX_FONT_FAMILY_BYTES
                        || family.chars().any(char::is_control)
                })
        }) {
            return Err(AppearanceValidationError::FontFamily);
        }
        if self.font_features.len() > MAX_FONT_FEATURES
            || self.font_features.iter().any(|feature| {
                !feature
                    .tag
                    .iter()
                    .all(|byte| byte.is_ascii_alphanumeric() || *byte == b' ')
            })
        {
            return Err(AppearanceValidationError::FontFeatures);
        }
        finite_range(self.font_size_points, 1.0, MAX_FONT_SIZE_POINTS)
            .then_some(())
            .ok_or(AppearanceValidationError::FontSize)?;
        if !(1..=1000).contains(&self.font_weight) {
            return Err(AppearanceValidationError::FontWeight);
        }
        validate_adjustment(self.cell_height_adjustment)?;
        if !finite_range(self.padding_left, 0.0, MAX_PADDING)
            || !finite_range(self.padding_right, 0.0, MAX_PADDING)
            || !finite_range(self.padding_top, 0.0, MAX_PADDING)
            || !finite_range(self.padding_bottom, 0.0, MAX_PADDING)
        {
            return Err(AppearanceValidationError::Padding);
        }
        if !finite_range(self.minimum_contrast, 1.0, 21.0) {
            return Err(AppearanceValidationError::MinimumContrast);
        }
        if !(50..=MAX_BLINK_INTERVAL_MS).contains(&self.cursor_blink_interval_ms) {
            return Err(AppearanceValidationError::BlinkInterval);
        }
        if !finite_range(self.background_opacity, 0.0, 1.0) {
            return Err(AppearanceValidationError::BackgroundOpacity);
        }
        Ok(())
    }

    /// Produce a deterministic appearance signature for renderer caches and diagnostics.
    #[must_use]
    pub fn stable_hash(&self) -> u64 {
        let mut hasher = StableHasher::default();
        self.color_scheme.hash(&mut hasher);
        self.font_families.hash(&mut hasher);
        self.font_families_bold.hash(&mut hasher);
        self.font_families_italic.hash(&mut hasher);
        self.font_families_bold_italic.hash(&mut hasher);
        self.font_size_points.to_bits().hash(&mut hasher);
        self.font_weight.hash(&mut hasher);
        self.font_features.hash(&mut hasher);
        self.font_synthetic_style.hash(&mut hasher);
        self.font_thicken.hash(&mut hasher);
        self.font_thicken_strength.hash(&mut hasher);
        hash_adjustment(self.cell_height_adjustment, &mut hasher);
        self.padding_left.to_bits().hash(&mut hasher);
        self.padding_right.to_bits().hash(&mut hasher);
        self.padding_top.to_bits().hash(&mut hasher);
        self.padding_bottom.to_bits().hash(&mut hasher);
        self.foreground.hash(&mut hasher);
        self.background.hash(&mut hasher);
        self.cursor_color.hash(&mut hasher);
        self.cursor_style.hash(&mut hasher);
        self.selection_foreground.hash(&mut hasher);
        self.selection_background.hash(&mut hasher);
        self.search_match_color.hash(&mut hasher);
        self.search_current_color.hash(&mut hasher);
        self.link_color.hash(&mut hasher);
        self.copy_cursor_color.hash(&mut hasher);
        self.palette.hash(&mut hasher);
        self.minimum_contrast.to_bits().hash(&mut hasher);
        self.cursor_blink_policy.hash(&mut hasher);
        self.cursor_blink_interval_ms.hash(&mut hasher);
        self.rounded_selection.hash(&mut hasher);
        self.background_opacity.to_bits().hash(&mut hasher);
        hasher.finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AppearanceValidationError {
    #[error("terminal appearance has an invalid font-family count")]
    FontFamilies,
    #[error("terminal appearance has an invalid font family")]
    FontFamily,
    #[error("terminal appearance has invalid font features")]
    FontFeatures,
    #[error("terminal appearance has an invalid font size")]
    FontSize,
    #[error("terminal appearance has an invalid font weight")]
    FontWeight,
    #[error("terminal appearance has an invalid cell-height adjustment")]
    CellHeight,
    #[error("terminal appearance has invalid padding")]
    Padding,
    #[error("terminal appearance has an invalid minimum contrast")]
    MinimumContrast,
    #[error("terminal appearance has an invalid cursor blink interval")]
    BlinkInterval,
    #[error("terminal appearance has an invalid background opacity")]
    BackgroundOpacity,
}

/// Outcome category for one Ghostty configuration entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppearanceConfigDisposition {
    Applied,
    Included,
    Unsupported,
    Invalid,
    NoOp,
}

/// Bounded source diagnostic produced while reading Ghostty configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppearanceConfigDiagnostic {
    pub path: PathBuf,
    pub line: u32,
    pub key: String,
    pub disposition: AppearanceConfigDisposition,
    pub message: String,
}

/// Fully resolved appearance plus configuration provenance and parse counters.
#[derive(Clone, Debug)]
pub struct AppearanceLoad {
    pub appearance: TerminalAppearance,
    pub provenance: AppearanceProvenance,
    pub root: Option<PathBuf>,
    pub diagnostics: Vec<AppearanceConfigDiagnostic>,
    pub diagnostics_dropped: u32,
    pub supported: u32,
    pub unsupported: u32,
    pub invalid: u32,
    pub bytes_read: usize,
    /// A requested theme could not be resolved or parsed safely.
    pub fatal: bool,
    /// The concrete theme file selected for this load, when any.
    pub theme_path: Option<PathBuf>,
}

impl AppearanceLoad {
    /// Build a load result containing only built-in defaults for `color_scheme`.
    #[must_use]
    pub fn defaults_for(color_scheme: TerminalColorScheme) -> Self {
        let appearance = TerminalAppearance {
            color_scheme,
            ..TerminalAppearance::default()
        };
        Self {
            appearance,
            provenance: AppearanceProvenance::default(),
            root: None,
            diagnostics: Vec::new(),
            diagnostics_dropped: 0,
            supported: 0,
            unsupported: 0,
            invalid: 0,
            bytes_read: 0,
            fatal: false,
            theme_path: None,
        }
    }
}

/// Discover and load the first standard Ghostty configuration file.
#[must_use]
pub fn load_ghostty_appearance() -> AppearanceLoad {
    load_ghostty_appearance_for(TerminalColorScheme::Dark)
}

/// Discover and load Ghostty appearance for a specific system color scheme.
#[must_use]
pub fn load_ghostty_appearance_for(color_scheme: TerminalColorScheme) -> AppearanceLoad {
    let Some(path) = discover_ghostty_config() else {
        return AppearanceLoad::defaults_for(color_scheme);
    };
    load_ghostty_appearance_from_for(&path, color_scheme)
}

/// Load a specific Ghostty configuration root, following bounded relative includes.
#[must_use]
pub fn load_ghostty_appearance_from(path: &Path) -> AppearanceLoad {
    load_ghostty_appearance_from_for(path, TerminalColorScheme::Dark)
}

/// Load a Ghostty configuration root for a specific system color scheme.
#[must_use]
pub fn load_ghostty_appearance_from_for(
    path: &Path,
    color_scheme: TerminalColorScheme,
) -> AppearanceLoad {
    let theme = discover_theme_directive(path);
    let mut load = AppearanceLoad::defaults_for(color_scheme);
    load.root = Some(path.to_path_buf());
    let defaults = TerminalAppearance::default();

    if let Some(theme) = theme {
        let invalid_before = load.invalid;
        {
            let mut loader = ConfigLoader::new(
                &mut load,
                &defaults,
                false,
                IncludePolicy::Follow,
                AppearanceSource::ThemeFile,
            );
            match select_theme_name(&theme.value, color_scheme) {
                Ok(name) => match resolve_theme_path(&name, path) {
                    Some(theme_path) => {
                        loader.load.theme_path = Some(theme_path.clone());
                        loader.load_tree(&theme_path);
                    }
                    None => loader.invalid(
                        &theme.path,
                        theme.line,
                        "theme",
                        format!("cannot resolve Ghostty theme `{name}`"),
                    ),
                },
                Err(message) => loader.invalid(&theme.path, theme.line, "theme", message),
            }
        }
        load.fatal = load.invalid > invalid_before;
    }

    {
        let mut loader = ConfigLoader::new(
            &mut load,
            &defaults,
            true,
            IncludePolicy::Follow,
            AppearanceSource::Ghostty,
        );
        loader.load_tree(path);
        if let Err(error) = loader.load.appearance.validate() {
            loader.invalid(path, 0, "appearance", error.to_string());
            loader.load.appearance = defaults.clone();
            loader.load.appearance.color_scheme = color_scheme;
            loader.load.provenance = AppearanceProvenance::default();
        }
    }
    load
}

/// Load the discovered Ghostty appearance and apply ordered `zz/config` entries last.
#[must_use]
pub fn load_ghostty_appearance_for_with_overrides(
    color_scheme: TerminalColorScheme,
    entries: &[(String, String)],
) -> AppearanceLoad {
    apply_appearance_overrides(load_ghostty_appearance_for(color_scheme), entries)
}

/// Load a specific Ghostty root and apply ordered `zz/config` entries last.
#[must_use]
pub fn load_ghostty_appearance_from_for_with_overrides(
    path: &Path,
    color_scheme: TerminalColorScheme,
    entries: &[(String, String)],
) -> AppearanceLoad {
    apply_appearance_overrides(
        load_ghostty_appearance_from_for(path, color_scheme),
        entries,
    )
}

/// Applies the daemon-owned appearance subset as a final ordered pass. A closing
/// `theme` entry resolves first and becomes the base for every other entry, which
/// then applies in file order.
#[must_use]
pub fn apply_appearance_overrides(
    mut load: AppearanceLoad,
    entries: &[(String, String)],
) -> AppearanceLoad {
    let defaults = TerminalAppearance::default();
    let override_path = PathBuf::from("<zz/config overrides>");
    let theme_root = load.root.clone().unwrap_or_else(|| override_path.clone());
    if entries
        .iter()
        .any(|(key, _)| key == AppearanceConfigKey::Theme.as_str())
    {
        load.fatal = false;
    }

    let theme = {
        let mut loader = ConfigLoader::new(
            &mut load,
            &defaults,
            true,
            IncludePolicy::Reject,
            AppearanceSource::Override,
        );
        for (index, (key, value)) in entries.iter().enumerate() {
            if key == AppearanceConfigKey::Theme.as_str() {
                loader.apply_raw_entry(&override_path, override_line(index), key, value);
            }
        }
        loader.theme.take()
    };

    if let Some(theme) = theme {
        let invalid_before = load.invalid;
        let color_scheme = load.appearance.color_scheme;
        {
            let mut loader = ConfigLoader::new(
                &mut load,
                &defaults,
                false,
                IncludePolicy::Follow,
                AppearanceSource::ThemeFile,
            );
            match select_theme_name(&theme.value, color_scheme) {
                Ok(name) => match resolve_theme_path(&name, &theme_root) {
                    Some(theme_path) => {
                        loader.load.theme_path = Some(theme_path.clone());
                        loader.load_tree(&theme_path);
                    }
                    None => loader.invalid(
                        &theme.path,
                        theme.line,
                        AppearanceConfigKey::Theme.as_str(),
                        format!("cannot resolve Ghostty theme `{name}`"),
                    ),
                },
                Err(message) => loader.invalid(
                    &theme.path,
                    theme.line,
                    AppearanceConfigKey::Theme.as_str(),
                    message,
                ),
            }
        }
        load.fatal |= load.invalid > invalid_before;
    }

    {
        let mut loader = ConfigLoader::new(
            &mut load,
            &defaults,
            true,
            IncludePolicy::Reject,
            AppearanceSource::Override,
        );
        for (index, (key, value)) in entries.iter().enumerate() {
            if key != AppearanceConfigKey::Theme.as_str() {
                loader.apply_raw_entry(&override_path, override_line(index), key, value);
            }
        }
        if let Err(error) = loader.load.appearance.validate() {
            loader.invalid(&override_path, 0, "appearance", error.to_string());
            let color_scheme = loader.load.appearance.color_scheme;
            loader.load.appearance = TerminalAppearance {
                color_scheme,
                ..defaults.clone()
            };
            loader.load.provenance = AppearanceProvenance::default();
        }
    }

    load
}

fn override_line(index: usize) -> u32 {
    u32::try_from(index.saturating_add(1)).unwrap_or(u32::MAX)
}

#[derive(Clone, Debug)]
struct ThemeDirective {
    value: String,
    path: PathBuf,
    line: u32,
}

fn discover_theme_directive(path: &Path) -> Option<ThemeDirective> {
    let defaults = TerminalAppearance::default();
    let mut scan = AppearanceLoad::defaults_for(TerminalColorScheme::Dark);
    let mut loader = ConfigLoader::new(
        &mut scan,
        &defaults,
        true,
        IncludePolicy::Follow,
        AppearanceSource::Ghostty,
    );
    loader.load_tree(path);
    loader.theme
}

fn select_theme_name(value: &str, color_scheme: TerminalColorScheme) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("theme name cannot be empty".to_owned());
    }
    let parts = value.split(',').map(str::trim).collect::<Vec<_>>();
    let adaptive = parts.len() > 1 || value.starts_with("light:") || value.starts_with("dark:");
    if !adaptive {
        return Ok(value.to_owned());
    }

    let mut light = None;
    let mut dark = None;
    for part in parts {
        let Some((variant, name)) = part.split_once(':') else {
            return Err("adaptive theme expects `light:name,dark:name`".to_owned());
        };
        let name = name.trim();
        if name.is_empty() {
            return Err(format!("adaptive theme `{variant}` name cannot be empty"));
        }
        let slot = match variant.trim() {
            "light" => &mut light,
            "dark" => &mut dark,
            _ => return Err(format!("unknown adaptive theme variant `{variant}`")),
        };
        if slot.replace(name.to_owned()).is_some() {
            return Err(format!("duplicate adaptive theme variant `{variant}`"));
        }
    }
    match color_scheme {
        TerminalColorScheme::Light => light,
        TerminalColorScheme::Dark => dark,
    }
    .ok_or_else(|| "adaptive theme must define both `light` and `dark`".to_owned())
}

fn resolve_theme_path(name: &str, config_root: &Path) -> Option<PathBuf> {
    let direct = PathBuf::from(name);
    if direct.is_absolute() {
        return direct.is_file().then_some(direct);
    }
    if let Some(rest) = name.strip_prefix("~/") {
        let path = PathBuf::from(std::env::var_os("HOME")?).join(rest);
        return path.is_file().then_some(path);
    }

    let mut candidates = Vec::new();
    if name.contains('/') || name.contains('\\') {
        candidates.push(
            config_root
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(name),
        );
    }
    candidates.extend(
        ghostty_theme_directories(Some(config_root))
            .into_iter()
            .map(|directory| directory.join(name)),
    );

    candidates.into_iter().find(|candidate| candidate.is_file())
}

fn ghostty_theme_directories(config_root: Option<&Path>) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    if let Some(config_root) = config_root {
        push_unique_path(
            &mut directories,
            config_root
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("themes"),
        );
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        push_unique_path(&mut directories, PathBuf::from(xdg).join("ghostty/themes"));
    }
    if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
        let home = PathBuf::from(home);
        push_unique_path(&mut directories, home.join(".config/ghostty/themes"));
        push_unique_path(
            &mut directories,
            home.join("Library/Application Support/com.mitchellh.ghostty/themes"),
        );
    }
    let data_dirs = std::env::var_os("XDG_DATA_DIRS")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| OsStr::new("/usr/local/share:/usr/share").to_os_string());
    for directory in std::env::split_paths(&data_dirs) {
        push_unique_path(&mut directories, directory.join("ghostty/themes"));
    }
    push_unique_path(
        &mut directories,
        PathBuf::from("/Applications/Ghostty.app/Contents/Resources/ghostty/themes"),
    );
    push_unique_path(
        &mut directories,
        PathBuf::from("C:/Program Files/Ghostty/share/ghostty/themes"),
    );
    directories
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.contains(&path) {
        paths.push(path);
    }
}

/// One discoverable Ghostty theme and its renderer-neutral swatch source.
#[derive(Clone, Debug)]
pub struct GhosttyTheme {
    pub name: String,
    pub path: PathBuf,
    pub appearance: TerminalAppearance,
}

/// Enumerate valid Ghostty theme files in the same precedence order used by theme resolution.
#[must_use]
pub fn enumerate_ghostty_themes_for(color_scheme: TerminalColorScheme) -> Vec<GhosttyTheme> {
    let config_root = discover_ghostty_config();
    let mut themes = BTreeMap::new();
    for directory in ghostty_theme_directories(config_root.as_deref()) {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        let mut paths = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .collect::<Vec<_>>();
        paths.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
        for path in paths {
            let Some(name) = path.file_name().and_then(OsStr::to_str).map(str::to_owned) else {
                continue;
            };
            if themes.contains_key(&name) {
                continue;
            }
            let load = load_ghostty_theme_file_for(&path, color_scheme);
            if load.fatal {
                continue;
            }
            themes.insert(
                name.clone(),
                GhosttyTheme {
                    name,
                    path,
                    appearance: load.appearance,
                },
            );
        }
    }
    themes.into_values().collect()
}

fn load_ghostty_theme_file_for(path: &Path, color_scheme: TerminalColorScheme) -> AppearanceLoad {
    let defaults = TerminalAppearance::default();
    let mut load = AppearanceLoad::defaults_for(color_scheme);
    load.root = Some(path.to_path_buf());
    load.theme_path = Some(path.to_path_buf());
    let invalid_before = load.invalid;
    {
        let mut loader = ConfigLoader::new(
            &mut load,
            &defaults,
            false,
            IncludePolicy::Follow,
            AppearanceSource::ThemeFile,
        );
        loader.load_tree(path);
        if let Err(error) = loader.load.appearance.validate() {
            loader.invalid(path, 0, "appearance", error.to_string());
        }
    }
    load.fatal = load.invalid > invalid_before;
    load
}

/// Return the first existing Ghostty configuration path in the supported order.
#[must_use]
pub fn discover_ghostty_config() -> Option<PathBuf> {
    let xdg = std::env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty());
    let home = std::env::var_os("HOME").filter(|value| !value.is_empty());
    ghostty_config_candidates(xdg.as_deref(), home.as_deref())
        .into_iter()
        .find(|path| path.is_file())
}

fn ghostty_config_candidates(xdg: Option<&OsStr>, home: Option<&OsStr>) -> Vec<PathBuf> {
    let mut candidates = Vec::with_capacity(4);
    if let Some(config) = xdg {
        let root = PathBuf::from(config).join("ghostty");
        candidates.push(root.join("config.ghostty"));
        candidates.push(root.join("config"));
    }
    if let Some(home) = home {
        let root = PathBuf::from(home).join(".config/ghostty");
        candidates.push(root.join("config.ghostty"));
        candidates.push(root.join("config"));
    }
    candidates
}

struct ConfigLoader<'a> {
    load: &'a mut AppearanceLoad,
    defaults: &'a TerminalAppearance,
    seen_paths: HashSet<PathBuf>,
    pending_files: VecDeque<PendingConfigFile>,
    font_family_seen: [bool; 4],
    font_feature_seen: bool,
    allow_theme: bool,
    include_policy: IncludePolicy,
    source: AppearanceSource,
    theme: Option<ThemeDirective>,
}

#[derive(Clone, Copy)]
enum FontFamilyStyle {
    Regular = 0,
    Bold = 1,
    Italic = 2,
    BoldItalic = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IncludePolicy {
    Follow,
    Reject,
}

struct PendingConfigFile {
    path: PathBuf,
    source_path: PathBuf,
    source_line: u32,
    depth: usize,
    optional: bool,
    ancestors: Vec<PathBuf>,
}

impl ConfigLoader<'_> {
    fn new<'a>(
        load: &'a mut AppearanceLoad,
        defaults: &'a TerminalAppearance,
        allow_theme: bool,
        include_policy: IncludePolicy,
        source: AppearanceSource,
    ) -> ConfigLoader<'a> {
        ConfigLoader {
            load,
            defaults,
            seen_paths: HashSet::new(),
            pending_files: VecDeque::new(),
            font_family_seen: [false; 4],
            font_feature_seen: false,
            allow_theme,
            include_policy,
            source,
            theme: None,
        }
    }

    fn load_tree(&mut self, path: &Path) {
        self.load_file(path, 0, None, &[]);
        while let Some(pending) = self.pending_files.pop_front() {
            self.load_file(
                &pending.path,
                pending.depth,
                Some((&pending.source_path, pending.source_line, pending.optional)),
                &pending.ancestors,
            );
        }
    }

    fn apply_raw_entry(&mut self, path: &Path, line: u32, key: &str, value: &str) {
        let (value, quoted) = decode_config_value(value.trim());
        self.apply(path, line, key.trim(), value, quoted, 0, &[]);
    }

    fn load_file(
        &mut self,
        path: &Path,
        depth: usize,
        source: Option<(&Path, u32, bool)>,
        ancestors: &[PathBuf],
    ) {
        if depth > MAX_CONFIG_DEPTH {
            let (source_path, line, _) = source.unwrap_or((path, 0, false));
            self.invalid(source_path, line, "config-file", "include depth exceeded");
            return;
        }
        let canonical = match fs::canonicalize(path) {
            Ok(path) => path,
            Err(error) => {
                let (source_path, line, optional) = source.unwrap_or((path, 0, false));
                if optional && error.kind() == std::io::ErrorKind::NotFound {
                    return;
                }
                self.invalid(
                    source_path,
                    line,
                    "config-file",
                    format!("cannot resolve {}: {error}", path.display()),
                );
                return;
            }
        };
        if !self.seen_paths.insert(canonical.clone()) {
            if ancestors.contains(&canonical) {
                let (source_path, line, _) = source.unwrap_or((path, 0, false));
                self.invalid(source_path, line, "config-file", "include cycle detected");
            }
            return;
        }
        let mut descendants = Vec::with_capacity(ancestors.len().saturating_add(1));
        descendants.extend_from_slice(ancestors);
        descendants.push(canonical.clone());
        let metadata = match fs::metadata(&canonical) {
            Ok(metadata) => metadata,
            Err(error) => {
                let (source_path, line, _) = source.unwrap_or((&canonical, 0, false));
                self.invalid(
                    source_path,
                    line,
                    "config-file",
                    format!("cannot inspect {}: {error}", canonical.display()),
                );
                return;
            }
        };
        if !metadata.file_type().is_file() {
            let (source_path, line, _) = source.unwrap_or((&canonical, 0, false));
            self.invalid(
                source_path,
                line,
                "config-file",
                format!("{} is not a regular file", canonical.display()),
            );
            return;
        }
        let remaining = MAX_CONFIG_BYTES.saturating_sub(self.load.bytes_read);
        if usize::try_from(metadata.len()).unwrap_or(usize::MAX) > remaining {
            let (source_path, line, _) = source.unwrap_or((&canonical, 0, false));
            self.invalid(
                source_path,
                line,
                "config-file",
                "configuration byte limit exceeded",
            );
            return;
        }
        let file = match fs::File::open(&canonical) {
            Ok(file) => file,
            Err(error) => {
                let (source_path, line, _) = source.unwrap_or((path, 0, false));
                self.invalid(
                    source_path,
                    line,
                    "config-file",
                    format!("cannot read {}: {error}", canonical.display()),
                );
                return;
            }
        };
        let read_limit = remaining.saturating_add(1);
        let mut bytes = Vec::with_capacity(read_limit.min(64 * 1024));
        if let Err(error) = file
            .take(u64::try_from(read_limit).unwrap_or(u64::MAX))
            .read_to_end(&mut bytes)
        {
            let (source_path, line, _) = source.unwrap_or((&canonical, 0, false));
            self.invalid(
                source_path,
                line,
                "config-file",
                format!("cannot read {}: {error}", canonical.display()),
            );
            return;
        }
        if bytes.len() > remaining {
            let (source_path, line, _) = source.unwrap_or((&canonical, 0, false));
            self.invalid(
                source_path,
                line,
                "config-file",
                "configuration byte limit exceeded",
            );
            return;
        }
        self.load.bytes_read += bytes.len();
        let content = match std::str::from_utf8(&bytes) {
            Ok(content) => content,
            Err(error) => {
                self.invalid(
                    &canonical,
                    0,
                    "config-file",
                    format!("configuration is not UTF-8: {error}"),
                );
                return;
            }
        };
        for (index, raw_line) in content.lines().enumerate() {
            let line = u32::try_from(index.saturating_add(1)).unwrap_or(u32::MAX);
            let trimmed = raw_line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                self.invalid(&canonical, line, "", "expected `key = value`");
                continue;
            };
            let (value, quoted) = decode_config_value(value.trim());
            self.apply(
                &canonical,
                line,
                key.trim(),
                value,
                quoted,
                depth,
                &descendants,
            );
        }
    }

    fn apply(
        &mut self,
        path: &Path,
        line: u32,
        key: &str,
        value: &str,
        value_was_quoted: bool,
        depth: usize,
        ancestors: &[PathBuf],
    ) {
        let result = match key {
            "theme" => {
                if !self.allow_theme {
                    Err("theme files cannot select another theme".to_owned())
                } else if value.is_empty() {
                    self.theme = None;
                    Ok(())
                } else {
                    self.theme = Some(ThemeDirective {
                        value: value.to_owned(),
                        path: path.to_path_buf(),
                        line,
                    });
                    Ok(())
                }
            }
            "font-family" => self.apply_font_family(FontFamilyStyle::Regular, value),
            "font-family-bold" => self.apply_font_family(FontFamilyStyle::Bold, value),
            "font-family-italic" => self.apply_font_family(FontFamilyStyle::Italic, value),
            "font-family-bold-italic" => self.apply_font_family(FontFamilyStyle::BoldItalic, value),
            "font-size" => reset_or_parse(value, self.defaults.font_size_points, parse_font_size)
                .map(|value| self.load.appearance.font_size_points = value),
            "font-feature" => self.apply_font_feature(value),
            "font-synthetic-style" => reset_or_parse(
                value,
                self.defaults.font_synthetic_style,
                parse_font_synthetic_style,
            )
            .map(|value| self.load.appearance.font_synthetic_style = value),
            "font-thicken" => reset_or_parse(value, self.defaults.font_thicken, parse_bool)
                .map(|value| self.load.appearance.font_thicken = value),
            "font-thicken-strength" => reset_or_parse(
                value,
                self.defaults.font_thicken_strength,
                parse_font_thicken_strength,
            )
            .map(|value| self.load.appearance.font_thicken_strength = value),
            "adjust-cell-height" => reset_or_parse(
                value,
                self.defaults.cell_height_adjustment,
                parse_adjustment,
            )
            .map(|value| self.load.appearance.cell_height_adjustment = value),
            "window-padding-x" => reset_or_parse(
                value,
                (self.defaults.padding_left, self.defaults.padding_right),
                parse_padding_pair,
            )
            .map(|(left, right)| {
                self.load.appearance.padding_left = left;
                self.load.appearance.padding_right = right;
            }),
            "window-padding-y" => reset_or_parse(
                value,
                (self.defaults.padding_top, self.defaults.padding_bottom),
                parse_padding_pair,
            )
            .map(|(top, bottom)| {
                self.load.appearance.padding_top = top;
                self.load.appearance.padding_bottom = bottom;
            }),
            "foreground" => reset_or_parse(value, self.defaults.foreground, parse_rgb)
                .map(|value| self.load.appearance.foreground = value),
            "background" => reset_or_parse(value, self.defaults.background, parse_rgb)
                .map(|value| self.load.appearance.background = value),
            "cursor-color" => reset_or_parse(value, self.defaults.cursor_color, parse_rgb)
                .map(|value| self.load.appearance.cursor_color = value),
            "cursor-style" => reset_or_parse(value, self.defaults.cursor_style, parse_cursor_style)
                .map(|value| self.load.appearance.cursor_style = value),
            "selection-foreground" => {
                reset_or_parse(value, self.defaults.selection_foreground, parse_rgb)
                    .map(|value| self.load.appearance.selection_foreground = value)
            }
            "selection-background" => {
                reset_or_parse(value, self.defaults.selection_background, parse_rgba)
                    .map(|value| self.load.appearance.selection_background = value)
            }
            "palette" => self.apply_palette(value),
            "minimum-contrast" => reset_or_parse(
                value,
                self.defaults.minimum_contrast,
                parse_minimum_contrast,
            )
            .map(|value| self.load.appearance.minimum_contrast = value),
            "cursor-style-blink" => {
                reset_or_parse(value, self.defaults.cursor_blink_policy, parse_blink_policy)
                    .map(|value| self.load.appearance.cursor_blink_policy = value)
            }
            "background-opacity" => {
                reset_or_parse(value, self.defaults.background_opacity, parse_opacity)
                    .map(|value| self.load.appearance.background_opacity = value)
            }
            "zz-font-weight" => reset_or_parse(value, self.defaults.font_weight, parse_font_weight)
                .map(|value| self.load.appearance.font_weight = value),
            "zz-cursor-blink-interval-ms" => reset_or_parse(
                value,
                self.defaults.cursor_blink_interval_ms,
                parse_blink_interval,
            )
            .map(|value| self.load.appearance.cursor_blink_interval_ms = value),
            "zz-search-match-color" => {
                reset_or_parse(value, self.defaults.search_match_color, parse_rgba)
                    .map(|value| self.load.appearance.search_match_color = value)
            }
            "zz-search-current-color" => {
                reset_or_parse(value, self.defaults.search_current_color, parse_rgba)
                    .map(|value| self.load.appearance.search_current_color = value)
            }
            "zz-link-color" => reset_or_parse(value, self.defaults.link_color, parse_rgb)
                .map(|value| self.load.appearance.link_color = value),
            "zz-copy-cursor-color" => {
                reset_or_parse(value, self.defaults.copy_cursor_color, parse_rgba)
                    .map(|value| self.load.appearance.copy_cursor_color = value)
            }
            "zz-rounded-selection" => {
                reset_or_parse(value, self.defaults.rounded_selection, parse_bool)
                    .map(|value| self.load.appearance.rounded_selection = value)
            }
            "config-file" => {
                if self.include_policy == IncludePolicy::Reject {
                    self.load.unsupported = self.load.unsupported.saturating_add(1);
                    self.record(
                        path,
                        line,
                        key,
                        AppearanceConfigDisposition::Unsupported,
                        "config-file is not supported in appearance overrides",
                    );
                    return;
                }
                if value.is_empty() {
                    Ok(())
                } else {
                    let (optional, value) = if value_was_quoted {
                        (false, value)
                    } else {
                        value
                            .strip_prefix('?')
                            .map_or((false, value), |value| (true, value))
                    };
                    if value.is_empty() {
                        return;
                    }
                    let include = Path::new(value);
                    let include = if include.is_absolute() {
                        include.to_path_buf()
                    } else {
                        path.parent()
                            .unwrap_or_else(|| Path::new("."))
                            .join(include)
                    };
                    self.record(
                        path,
                        line,
                        key,
                        AppearanceConfigDisposition::Included,
                        include.display().to_string(),
                    );
                    self.load.supported = self.load.supported.saturating_add(1);
                    self.pending_files.push_back(PendingConfigFile {
                        path: include,
                        source_path: path.to_path_buf(),
                        source_line: line,
                        depth: depth.saturating_add(1),
                        optional,
                        ancestors: ancestors.to_vec(),
                    });
                    return;
                }
            }
            _ => {
                self.load.unsupported = self.load.unsupported.saturating_add(1);
                self.record(
                    path,
                    line,
                    key,
                    AppearanceConfigDisposition::Unsupported,
                    "unsupported appearance key",
                );
                return;
            }
        };

        match result {
            Ok(()) => {
                if let Some(key) = AppearanceConfigKey::from_config_key(key) {
                    self.load.provenance.set_source(key, self.source);
                }
                self.load.supported = self.load.supported.saturating_add(1);
                self.record(
                    path,
                    line,
                    key,
                    AppearanceConfigDisposition::Applied,
                    "applied",
                );
            }
            Err(message) => self.invalid(path, line, key, message),
        }
    }

    fn apply_font_family(&mut self, style: FontFamilyStyle, value: &str) -> Result<(), String> {
        let style_index = style as usize;
        if value.is_empty() {
            let defaults = match style {
                FontFamilyStyle::Regular => &self.defaults.font_families,
                FontFamilyStyle::Bold => &self.defaults.font_families_bold,
                FontFamilyStyle::Italic => &self.defaults.font_families_italic,
                FontFamilyStyle::BoldItalic => &self.defaults.font_families_bold_italic,
            }
            .clone();
            *font_families_mut(&mut self.load.appearance, style) = defaults;
            self.font_family_seen[style_index] = false;
            return Ok(());
        }
        if value.len() > MAX_FONT_FAMILY_BYTES || value.chars().any(char::is_control) {
            return Err("invalid font family".to_owned());
        }
        if !self.font_family_seen[style_index] {
            font_families_mut(&mut self.load.appearance, style).clear();
            self.font_family_seen[style_index] = true;
        }
        let families = font_families_mut(&mut self.load.appearance, style);
        if families.len() >= MAX_FONT_FAMILIES {
            return Err("font-family count exceeds limit".to_owned());
        }
        families.push(value.to_owned());
        Ok(())
    }

    fn apply_font_feature(&mut self, value: &str) -> Result<(), String> {
        if value.is_empty() {
            self.load
                .appearance
                .font_features
                .clone_from(&self.defaults.font_features);
            self.font_feature_seen = false;
            return Ok(());
        }
        let mut features = if self.font_feature_seen {
            self.load.appearance.font_features.clone()
        } else {
            Vec::new()
        };
        for setting in split_quoted_commas(value)? {
            let feature = parse_font_feature(setting)?;
            if let Some(existing) = features
                .iter_mut()
                .find(|candidate| candidate.tag == feature.tag)
            {
                *existing = feature;
                continue;
            }
            if features.len() >= MAX_FONT_FEATURES {
                return Err("font-feature count exceeds limit".to_owned());
            }
            features.push(feature);
        }
        self.load.appearance.font_features = features;
        self.font_feature_seen = true;
        Ok(())
    }

    fn apply_palette(&mut self, value: &str) -> Result<(), String> {
        if value.is_empty() {
            self.load.appearance.palette = self.defaults.palette;
            return Ok(());
        }
        let Some((index, color)) = value.split_once('=') else {
            return Err("palette expects `index=color`".to_owned());
        };
        let index = parse_palette_index(index.trim())?;
        if index > 255 {
            return Err("palette index must be between 0 and 255".to_owned());
        }
        self.load.appearance.palette[usize::from(index)] = parse_rgb(color.trim())?;
        Ok(())
    }

    fn invalid(&mut self, path: &Path, line: u32, key: &str, message: impl Into<String>) {
        self.load.invalid = self.load.invalid.saturating_add(1);
        self.record(
            path,
            line,
            key,
            AppearanceConfigDisposition::Invalid,
            message,
        );
    }

    fn record(
        &mut self,
        path: &Path,
        line: u32,
        key: &str,
        disposition: AppearanceConfigDisposition,
        message: impl Into<String>,
    ) {
        if self.load.diagnostics.len() >= MAX_CONFIG_DIAGNOSTICS {
            self.load.diagnostics_dropped = self.load.diagnostics_dropped.saturating_add(1);
            return;
        }
        self.load.diagnostics.push(AppearanceConfigDiagnostic {
            path: path.to_path_buf(),
            line,
            key: key.chars().take(128).collect(),
            disposition,
            message: message.into().chars().take(512).collect(),
        });
    }
}

fn font_families_mut(
    appearance: &mut TerminalAppearance,
    style: FontFamilyStyle,
) -> &mut Vec<String> {
    match style {
        FontFamilyStyle::Regular => &mut appearance.font_families,
        FontFamilyStyle::Bold => &mut appearance.font_families_bold,
        FontFamilyStyle::Italic => &mut appearance.font_families_italic,
        FontFamilyStyle::BoldItalic => &mut appearance.font_families_bold_italic,
    }
}

fn reset_or_parse<T: Copy>(
    value: &str,
    default: T,
    parser: impl FnOnce(&str) -> Result<T, String>,
) -> Result<T, String> {
    if value.is_empty() {
        Ok(default)
    } else {
        parser(value)
    }
}

fn decode_config_value(value: &str) -> (&str, bool) {
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        (&value[1..value.len() - 1], true)
    } else {
        (value, false)
    }
}

fn parse_font_size(value: &str) -> Result<f32, String> {
    parse_finite_range(value, 1.0, MAX_FONT_SIZE_POINTS, "font size")
}

fn parse_padding_pair(value: &str) -> Result<(f32, f32), String> {
    let mut values = value.split(',').map(str::trim);
    let first = values
        .next()
        .ok_or_else(|| "padding expects one or two values".to_owned())?;
    let first = parse_finite_range(first, 0.0, MAX_PADDING, "padding")?;
    let second = values.next().map_or(Ok(first), |value| {
        parse_finite_range(value, 0.0, MAX_PADDING, "padding")
    })?;
    if values.next().is_some() {
        return Err("padding expects one or two values".to_owned());
    }
    Ok((first, second))
}

fn parse_font_weight(value: &str) -> Result<u16, String> {
    let weight = value
        .parse::<u16>()
        .map_err(|_| "font weight must be an integer".to_owned())?;
    (1..=1000)
        .contains(&weight)
        .then_some(weight)
        .ok_or_else(|| "font weight must be between 1 and 1000".to_owned())
}

fn parse_font_thicken_strength(value: &str) -> Result<u8, String> {
    value
        .parse::<u8>()
        .map_err(|_| "font-thicken-strength must be an integer between 0 and 255".to_owned())
}

fn parse_font_synthetic_style(value: &str) -> Result<FontSyntheticStyle, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => return Ok(FontSyntheticStyle::default()),
        "false" | "no" | "off" | "0" => {
            return Ok(FontSyntheticStyle {
                bold: false,
                italic: false,
                bold_italic: false,
            });
        }
        _ => {}
    }

    let mut styles = FontSyntheticStyle::default();
    for setting in value.split(',').map(str::trim) {
        if setting.is_empty() {
            return Err("font-synthetic-style contains an empty setting".to_owned());
        }
        let (enabled, style) = setting
            .strip_prefix("no-")
            .map_or((true, setting), |style| (false, style));
        match style.to_ascii_lowercase().as_str() {
            "bold" => styles.bold = enabled,
            "italic" => styles.italic = enabled,
            "bold-italic" => styles.bold_italic = enabled,
            _ => {
                return Err("font-synthetic-style expects bold, italic, or bold-italic".to_owned());
            }
        }
    }
    Ok(styles)
}

fn parse_font_feature(value: &str) -> Result<FontFeature, String> {
    let value = value.trim();
    let (prefix_value, value) = value.strip_prefix('-').map_or_else(
        || {
            value
                .strip_prefix('+')
                .map_or((None, value), |value| (Some(1), value))
        },
        |value| (Some(0), value),
    );
    let value = value.trim_start();
    let (tag, remainder) = if matches!(value.as_bytes().first(), Some(b'"' | b'\'')) {
        let quote = value.as_bytes()[0];
        let quoted = &value[1..];
        let end = quoted
            .as_bytes()
            .iter()
            .position(|byte| *byte == quote)
            .ok_or_else(|| "unterminated quoted font-feature tag".to_owned())?;
        (&quoted[..end], quoted[end + 1..].trim())
    } else {
        let end = value
            .find(|character: char| character.is_ascii_whitespace() || character == '=')
            .unwrap_or(value.len());
        (&value[..end], value[end..].trim())
    };
    let remainder = remainder.strip_prefix('=').map_or(remainder, str::trim);
    let feature_value = if let Some(value) = prefix_value {
        if !remainder.is_empty() {
            return Err("prefixed font-feature does not accept another value".to_owned());
        }
        value
    } else {
        match remainder.to_ascii_lowercase().as_str() {
            "" | "on" => 1,
            "off" => 0,
            _ => remainder
                .parse::<u32>()
                .map_err(|_| "invalid font-feature value".to_owned())?,
        }
    };
    let tag = tag.as_bytes();
    let tag: [u8; 4] = tag
        .try_into()
        .map_err(|_| "font-feature tag must contain exactly four bytes".to_owned())?;
    if !tag.iter().all(u8::is_ascii_alphanumeric) {
        return Err("font-feature tag must be ASCII alphanumeric".to_owned());
    }
    Ok(FontFeature::new(tag, feature_value))
}

fn split_quoted_commas(value: &str) -> Result<Vec<&str>, String> {
    let mut output = Vec::new();
    let mut quote = None;
    let mut escaped = false;
    let mut start = 0;
    for (index, character) in value.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match character {
            '\\' if quote.is_some() => escaped = true,
            '"' | '\'' if quote == Some(character) => quote = None,
            '"' | '\'' if quote.is_none() => quote = Some(character),
            ',' if quote.is_none() => {
                let setting = value[start..index].trim();
                if setting.is_empty() {
                    return Err("empty font-feature setting".to_owned());
                }
                output.push(setting);
                if output.len() > MAX_FONT_FEATURES {
                    return Err("font-feature count exceeds limit".to_owned());
                }
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    if quote.is_some() || escaped {
        return Err("unterminated quoted font-feature setting".to_owned());
    }
    let setting = value[start..].trim();
    if setting.is_empty() {
        return Err("empty font-feature setting".to_owned());
    }
    output.push(setting);
    if output.len() > MAX_FONT_FEATURES {
        return Err("font-feature count exceeds limit".to_owned());
    }
    Ok(output)
}

fn parse_palette_index(value: &str) -> Result<u16, String> {
    let (digits, radix) = if let Some(digits) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        (digits, 16)
    } else if let Some(digits) = value
        .strip_prefix("0o")
        .or_else(|| value.strip_prefix("0O"))
    {
        (digits, 8)
    } else if let Some(digits) = value
        .strip_prefix("0b")
        .or_else(|| value.strip_prefix("0B"))
    {
        (digits, 2)
    } else {
        (value, 10)
    };
    if digits.is_empty() {
        return Err("invalid palette index".to_owned());
    }
    u16::from_str_radix(digits, radix).map_err(|_| "invalid palette index".to_owned())
}

fn parse_adjustment(value: &str) -> Result<CellHeightAdjustment, String> {
    if let Some(percent) = value.strip_suffix('%') {
        return parse_finite_range(
            percent.trim(),
            -MAX_ADJUSTMENT,
            MAX_ADJUSTMENT,
            "cell-height percentage",
        )
        .map(CellHeightAdjustment::Percent);
    }
    parse_finite_range(
        value,
        -MAX_ADJUSTMENT,
        MAX_ADJUSTMENT,
        "cell-height adjustment",
    )
    .map(CellHeightAdjustment::Pixels)
}

fn parse_minimum_contrast(value: &str) -> Result<f32, String> {
    parse_finite_range(value, 1.0, 21.0, "minimum contrast")
}

fn parse_opacity(value: &str) -> Result<f32, String> {
    parse_finite_range(value, 0.0, 1.0, "background opacity")
}

fn parse_blink_interval(value: &str) -> Result<u32, String> {
    let interval = value
        .parse::<u32>()
        .map_err(|_| "blink interval must be an integer".to_owned())?;
    (50..=MAX_BLINK_INTERVAL_MS)
        .contains(&interval)
        .then_some(interval)
        .ok_or_else(|| "blink interval must be between 50 and 60000 ms".to_owned())
}

fn parse_blink_policy(value: &str) -> Result<CursorBlinkPolicy, String> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Ok(CursorBlinkPolicy::On),
        "false" | "no" | "off" | "0" => Ok(CursorBlinkPolicy::Off),
        "terminal" => Ok(CursorBlinkPolicy::Terminal),
        _ => Err("cursor-style-blink expects true, false, or terminal".to_owned()),
    }
}

fn parse_cursor_style(value: &str) -> Result<CursorStyle, String> {
    match value.to_ascii_lowercase().as_str() {
        "bar" => Ok(CursorStyle::Bar),
        "block" => Ok(CursorStyle::Block),
        "underline" => Ok(CursorStyle::Underline),
        "block_hollow" => Ok(CursorStyle::BlockHollow),
        _ => Err("cursor-style expects block, bar, underline, or block_hollow".to_owned()),
    }
}

fn parse_bool(value: &str) -> Result<bool, String> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Ok(true),
        "false" | "no" | "off" | "0" => Ok(false),
        _ => Err("expected a boolean".to_owned()),
    }
}

fn parse_rgb(value: &str) -> Result<Color, String> {
    if let Some(color) = x11_colors().get(&value.trim().to_ascii_lowercase()) {
        return Ok(*color);
    }
    let color = csscolorparser::parse(value).map_err(|error| error.to_string())?;
    let [r, g, b, alpha] = color.to_rgba8();
    if alpha != u8::MAX {
        return Err("this color does not accept alpha".to_owned());
    }
    Ok(Color::rgb(r, g, b))
}

fn parse_rgba(value: &str) -> Result<AppearanceColor, String> {
    if let Some(color) = x11_colors().get(&value.trim().to_ascii_lowercase()) {
        return Ok(AppearanceColor::opaque(*color));
    }
    let color = csscolorparser::parse(value).map_err(|error| error.to_string())?;
    let [r, g, b, a] = color.to_rgba8();
    Ok(AppearanceColor::rgba(r, g, b, a))
}

fn x11_colors() -> &'static HashMap<String, Color> {
    static COLORS: OnceLock<HashMap<String, Color>> = OnceLock::new();
    COLORS.get_or_init(|| {
        // Sourced from Ghostty's MIT/X11-licensed terminal/res/rgb.txt.
        include_str!("x11-rgb.txt")
            .lines()
            .filter_map(|line| {
                let mut fields = line.split_whitespace();
                let red = fields.next()?.parse::<u8>().ok()?;
                let green = fields.next()?.parse::<u8>().ok()?;
                let blue = fields.next()?.parse::<u8>().ok()?;
                let name = fields.collect::<Vec<_>>().join(" ").to_ascii_lowercase();
                (!name.is_empty()).then_some((name, Color::rgb(red, green, blue)))
            })
            .collect()
    })
}

fn parse_finite_range(value: &str, min: f32, max: f32, label: &str) -> Result<f32, String> {
    let parsed = value
        .parse::<f32>()
        .map_err(|_| format!("invalid {label}"))?;
    finite_range(parsed, min, max)
        .then_some(parsed)
        .ok_or_else(|| format!("{label} must be finite and between {min} and {max}"))
}

fn finite_range(value: f32, min: f32, max: f32) -> bool {
    value.is_finite() && value >= min && value <= max
}

fn validate_adjustment(value: CellHeightAdjustment) -> Result<(), AppearanceValidationError> {
    let valid = match value {
        CellHeightAdjustment::None => true,
        CellHeightAdjustment::Pixels(value) | CellHeightAdjustment::Percent(value) => {
            finite_range(value, -MAX_ADJUSTMENT, MAX_ADJUSTMENT)
        }
    };
    valid
        .then_some(())
        .ok_or(AppearanceValidationError::CellHeight)
}

fn default_font_family() -> &'static str {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    return "Menlo";
    #[cfg(target_os = "windows")]
    return "Cascadia Mono";
    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
    return "Noto Sans Mono";
}

fn default_palette() -> [Color; 256] {
    let mut palette = [Color::rgb(0, 0, 0); 256];
    let ansi = [
        0x00_00_00, 0xcd_00_00, 0x00_cd_00, 0xcd_cd_00, 0x00_00_ee, 0xcd_00_cd, 0x00_cd_cd,
        0xe5_e5_e5, 0x7f_7f_7f, 0xff_00_00, 0x00_ff_00, 0xff_ff_00, 0x5c_5c_ff, 0xff_00_ff,
        0x00_ff_ff, 0xff_ff_ff,
    ];
    for (index, packed) in ansi.into_iter().enumerate() {
        palette[index] = Color::from_packed(packed);
    }
    let levels = [0, 95, 135, 175, 215, 255];
    let mut index = 16;
    for red in levels {
        for green in levels {
            for blue in levels {
                palette[index] = Color::rgb(red, green, blue);
                index += 1;
            }
        }
    }
    for gray in 0_u8..24 {
        let level = 8_u8.saturating_add(gray.saturating_mul(10));
        palette[232 + usize::from(gray)] = Color::rgb(level, level, level);
    }
    palette
}

#[derive(Default)]
struct StableHasher(u64);

impl Hasher for StableHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        let mut hash = if self.0 == 0 {
            0xcbf2_9ce4_8422_2325
        } else {
            self.0
        };
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        self.0 = hash;
    }
}

fn hash_adjustment(adjustment: CellHeightAdjustment, hasher: &mut StableHasher) {
    match adjustment {
        CellHeightAdjustment::None => 0_u8.hash(hasher),
        CellHeightAdjustment::Pixels(value) => {
            1_u8.hash(hasher);
            value.to_bits().hash(hasher);
        }
        CellHeightAdjustment::Percent(value) => {
            2_u8.hash(hasher);
            value.to_bits().hash(hasher);
        }
    }
}

fn hash_palette(palette: &[Color; 256]) -> u64 {
    let mut hasher = StableHasher::default();
    palette.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_wire_safe_and_hash_deterministically() {
        let appearance = TerminalAppearance::default();
        appearance.validate().expect("valid defaults");
        assert!((appearance.font_size_points - 13.0).abs() < f32::EPSILON);
        assert_eq!(
            appearance.cell_height_adjustment,
            CellHeightAdjustment::None
        );
        assert_eq!(
            appearance.font_synthetic_style,
            FontSyntheticStyle::default()
        );
        assert!(!appearance.font_thicken);
        assert_eq!(appearance.font_thicken_strength, u8::MAX);
        assert_eq!(appearance.cursor_style, CursorStyle::Block);
        assert_eq!(appearance.stable_hash(), appearance.clone().stable_hash());
        assert_ne!(appearance.palette[16], appearance.palette[17]);
        assert_eq!(appearance.palette[255], Color::rgb(238, 238, 238));
    }

    #[test]
    fn parses_every_ghostty_cursor_style_and_rejects_invalid_values() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("config");
        for (value, expected) in [
            ("bar", CursorStyle::Bar),
            ("block", CursorStyle::Block),
            ("underline", CursorStyle::Underline),
            ("block_hollow", CursorStyle::BlockHollow),
        ] {
            fs::write(&path, format!("cursor-style = {value}\n")).expect("write config");
            let load = load_ghostty_appearance_from(&path);
            assert_eq!(load.invalid, 0, "{:#?}", load.diagnostics);
            assert_eq!(load.appearance.cursor_style, expected);
            assert_eq!(
                load.provenance.source(AppearanceConfigKey::CursorStyle),
                AppearanceSource::Ghostty
            );
        }

        fs::write(&path, "cursor-style = beam\n").expect("write invalid config");
        let load = load_ghostty_appearance_from(&path);
        assert_eq!(load.invalid, 1, "{:#?}", load.diagnostics);
        assert_eq!(load.appearance.cursor_style, CursorStyle::Block);
    }

    #[test]
    fn discovery_candidates_follow_ghostty_precedence() {
        assert_eq!(
            ghostty_config_candidates(Some(OsStr::new("/xdg")), Some(OsStr::new("/home/me"))),
            [
                PathBuf::from("/xdg/ghostty/config.ghostty"),
                PathBuf::from("/xdg/ghostty/config"),
                PathBuf::from("/home/me/.config/ghostty/config.ghostty"),
                PathBuf::from("/home/me/.config/ghostty/config"),
            ]
        );
    }

    #[test]
    fn parses_supported_values_including_named_and_alpha_colors() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("config");
        fs::write(
            &path,
            "\
font-family = Example Mono\n\
font-family = Emoji Font\n\
font-family-bold = Example Mono Bold\n\
font-family-bold = Bold Symbols\n\
font-family-italic = Example Mono Italic\n\
font-family-bold-italic = Example Mono Bold Italic\n\
font-size = 12.5\n\
font-feature = -liga\n\
font-feature = ss01=2\n\
font-synthetic-style = no-bold,italic,no-bold-italic\n\
font-thicken = true\n\
font-thicken-strength = 173\n\
adjust-cell-height = 12%\n\
window-padding-x = 7\n\
foreground = rebeccapurple\n\
selection-background = #11223380\n\
palette = 42=#abcdef\n\
cursor-style-blink = terminal\n\
zz-font-weight = 550\n\
zz-rounded-selection = false\n",
        )
        .expect("write config");

        let load = load_ghostty_appearance_from(&path);
        assert_eq!(load.invalid, 0, "{:#?}", load.diagnostics);
        assert_eq!(
            load.appearance.font_families,
            ["Example Mono", "Emoji Font"]
        );
        assert_eq!(
            load.appearance.font_families_bold,
            ["Example Mono Bold", "Bold Symbols"]
        );
        assert_eq!(
            load.appearance.font_families_italic,
            ["Example Mono Italic"]
        );
        assert_eq!(
            load.appearance.font_families_bold_italic,
            ["Example Mono Bold Italic"]
        );
        assert_eq!(load.appearance.font_features.len(), 2);
        assert_eq!(
            load.appearance.font_features[0],
            FontFeature::new(*b"liga", 0)
        );
        assert_eq!(load.appearance.foreground, Color::rgb(102, 51, 153));
        assert_eq!(
            load.appearance.selection_background,
            AppearanceColor::rgba(0x11, 0x22, 0x33, 0x80)
        );
        assert_eq!(load.appearance.palette[42], Color::rgb(0xab, 0xcd, 0xef));
        assert_eq!(load.appearance.font_weight, 550);
        assert_eq!(
            load.appearance.font_synthetic_style,
            FontSyntheticStyle {
                bold: false,
                italic: true,
                bold_italic: false,
            }
        );
        assert!(load.appearance.font_thicken);
        assert_eq!(load.appearance.font_thicken_strength, 173);
        assert!(!load.appearance.rounded_selection);
    }

    #[test]
    fn named_theme_is_loaded_before_explicit_user_overrides() {
        let directory = tempfile::tempdir().expect("tempdir");
        let root = directory.path().join("config");
        let themes = directory.path().join("themes");
        fs::create_dir(&themes).expect("create themes");
        let theme = themes.join("My Theme");
        fs::write(
            &theme,
            "foreground = #112233\nbackground = #445566\nfont-family = Theme Mono\npalette = 4=#abcdef\n",
        )
        .expect("write theme");
        fs::write(
            &root,
            "foreground = #fedcba\ntheme = My Theme\nfont-family = User Mono\n",
        )
        .expect("write config");

        let load = load_ghostty_appearance_from_for(&root, TerminalColorScheme::Dark);
        assert!(!load.fatal, "{:#?}", load.diagnostics);
        assert_eq!(load.theme_path, Some(theme));
        assert_eq!(load.appearance.foreground, Color::rgb(0xfe, 0xdc, 0xba));
        assert_eq!(load.appearance.background, Color::rgb(0x44, 0x55, 0x66));
        assert_eq!(load.appearance.font_families, ["User Mono"]);
        assert_eq!(load.appearance.palette[4], Color::rgb(0xab, 0xcd, 0xef));
        assert!(load.diagnostics.iter().any(|diagnostic| {
            diagnostic.key == "theme"
                && diagnostic.disposition == AppearanceConfigDisposition::Applied
        }));
    }

    #[test]
    fn appearance_override_beats_the_ghostty_value() {
        let directory = tempfile::tempdir().expect("tempdir");
        let root = directory.path().join("config");
        fs::write(&root, "background = #112233\n").expect("write config");

        let load = load_ghostty_appearance_from_for_with_overrides(
            &root,
            TerminalColorScheme::Dark,
            &[("background".to_owned(), "#abcdef".to_owned())],
        );

        assert_eq!(load.appearance.background, Color::rgb(0xab, 0xcd, 0xef));
        assert_eq!(
            load.provenance.source(AppearanceConfigKey::Background),
            AppearanceSource::Override
        );
    }

    #[test]
    fn override_theme_loads_before_other_override_keys_regardless_of_file_order() {
        let directory = tempfile::tempdir().expect("tempdir");
        let root = directory.path().join("config");
        let themes = directory.path().join("themes");
        fs::create_dir(&themes).expect("create themes");
        fs::write(&root, "background = #010203\n").expect("write config");
        fs::write(
            themes.join("Overlay Theme"),
            "background = #112233\nforeground = #445566\npalette = 4=#778899\n",
        )
        .expect("write theme");

        let entries = vec![
            ("background".to_owned(), "#abcdef".to_owned()),
            ("theme".to_owned(), "Overlay Theme".to_owned()),
        ];
        let load = load_ghostty_appearance_from_for_with_overrides(
            &root,
            TerminalColorScheme::Dark,
            &entries,
        );

        assert!(!load.fatal, "{:#?}", load.diagnostics);
        assert_eq!(load.appearance.background, Color::rgb(0xab, 0xcd, 0xef));
        assert_eq!(load.appearance.foreground, Color::rgb(0x44, 0x55, 0x66));
        assert_eq!(load.appearance.palette[4], Color::rgb(0x77, 0x88, 0x99));
    }

    #[test]
    fn empty_override_set_restores_the_unmodified_base_load() {
        let directory = tempfile::tempdir().expect("tempdir");
        let root = directory.path().join("config");
        fs::write(&root, "foreground = #123456\n").expect("write config");
        let base = load_ghostty_appearance_from_for(&root, TerminalColorScheme::Dark);
        let overridden = apply_appearance_overrides(
            base.clone(),
            &[("foreground".to_owned(), "#abcdef".to_owned())],
        );
        let restored = apply_appearance_overrides(base.clone(), &[]);

        assert_ne!(overridden.appearance, base.appearance);
        assert_eq!(restored.appearance, base.appearance);
        assert_eq!(restored.provenance, base.provenance);
    }

    #[test]
    fn provenance_distinguishes_default_theme_ghostty_and_override_tiers() {
        let directory = tempfile::tempdir().expect("tempdir");
        let root = directory.path().join("config");
        let themes = directory.path().join("themes");
        fs::create_dir(&themes).expect("create themes");
        fs::write(themes.join("Tiered"), "palette = 4=#778899\n").expect("write theme");
        fs::write(&root, "theme = Tiered\nforeground = #123456\n").expect("write config");

        let load = load_ghostty_appearance_from_for_with_overrides(
            &root,
            TerminalColorScheme::Dark,
            &[("background".to_owned(), "#abcdef".to_owned())],
        );

        assert_eq!(
            load.provenance.source(AppearanceConfigKey::FontSize),
            AppearanceSource::Default
        );
        assert_eq!(
            load.provenance.source(AppearanceConfigKey::Palette),
            AppearanceSource::ThemeFile
        );
        assert_eq!(
            load.provenance.source(AppearanceConfigKey::Foreground),
            AppearanceSource::Ghostty
        );
        assert_eq!(
            load.provenance.source(AppearanceConfigKey::Background),
            AppearanceSource::Override
        );
        assert_eq!(
            load.provenance.source(AppearanceConfigKey::Theme),
            AppearanceSource::Ghostty
        );
    }

    #[test]
    fn adaptive_theme_uses_the_requested_system_scheme() {
        let directory = tempfile::tempdir().expect("tempdir");
        let root = directory.path().join("config");
        let themes = directory.path().join("themes");
        fs::create_dir(&themes).expect("create themes");
        fs::write(themes.join("day"), "background = #f0f1f2\n").expect("write light theme");
        fs::write(themes.join("night"), "background = #101112\n").expect("write dark theme");
        fs::write(&root, "theme = dark:night,light:day\n").expect("write config");

        let light = load_ghostty_appearance_from_for(&root, TerminalColorScheme::Light);
        let dark = load_ghostty_appearance_from_for(&root, TerminalColorScheme::Dark);
        assert_eq!(light.appearance.color_scheme, TerminalColorScheme::Light);
        assert_eq!(dark.appearance.color_scheme, TerminalColorScheme::Dark);
        assert_eq!(light.appearance.background, Color::rgb(0xf0, 0xf1, 0xf2));
        assert_eq!(dark.appearance.background, Color::rgb(0x10, 0x11, 0x12));
    }

    #[test]
    fn missing_or_invalid_theme_is_fatal_but_bounded() {
        let directory = tempfile::tempdir().expect("tempdir");
        let root = directory.path().join("config");
        fs::write(&root, "theme = missing-theme\nforeground = #abcdef\n").expect("write config");

        let load = load_ghostty_appearance_from(&root);
        assert!(load.fatal);
        assert_eq!(load.invalid, 1);
        assert_eq!(load.appearance.foreground, Color::rgb(0xab, 0xcd, 0xef));
        assert!(load.diagnostics[0].message.contains("cannot resolve"));
    }

    #[test]
    fn follows_relative_includes_once_and_preserves_valid_values_after_errors() {
        let directory = tempfile::tempdir().expect("tempdir");
        let root = directory.path().join("root");
        let child = directory.path().join("child");
        fs::write(
            &root,
            "font-size = 11\nconfig-file = child\nconfig-file = child\nfont-size = nope\n",
        )
        .expect("write root");
        fs::write(&child, "window-padding-y = 9\nconfig-file = root\n").expect("write child");

        let load = load_ghostty_appearance_from(&root);
        assert!((load.appearance.font_size_points - 11.0).abs() < f32::EPSILON);
        assert!((load.appearance.padding_top - 9.0).abs() < f32::EPSILON);
        assert!((load.appearance.padding_bottom - 9.0).abs() < f32::EPSILON);
        assert_eq!(load.invalid, 2);
        assert!(load.diagnostics.iter().any(|diagnostic| {
            diagnostic.key == "config-file" && diagnostic.message.contains("cycle")
        }));
        assert_eq!(
            load.bytes_read,
            fs::read(&root).unwrap().len() + fs::read(&child).unwrap().len()
        );
    }

    #[test]
    fn ghostty_background_blur_is_ignored_but_background_opacity_is_applied() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("config");
        fs::write(&path, "background-opacity = 0.72\nbackground-blur = 24\n")
            .expect("write config");

        let load = load_ghostty_appearance_from(&path);
        assert_eq!(load.invalid, 0, "{:#?}", load.diagnostics);
        assert_eq!(load.unsupported, 1);
        assert!((load.appearance.background_opacity - 0.72).abs() < f32::EPSILON);
        assert!(load.diagnostics.iter().any(|diagnostic| {
            diagnostic.key == "background-blur"
                && diagnostic.disposition == AppearanceConfigDisposition::Unsupported
        }));
    }

    #[test]
    fn parses_every_renderer_color_and_behavior_setting() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("config");
        fs::write(
            &path,
            "\
background = #010203\n\
cursor-color = #040506\n\
selection-foreground = #070809\n\
zz-search-match-color = #10111280\n\
zz-search-current-color = #13141590\n\
zz-link-color = #161718\n\
zz-copy-cursor-color = #191a1ba0\n\
minimum-contrast = 4.5\n\
background-opacity = 0.75\n\
cursor-style-blink = off\n\
zz-cursor-blink-interval-ms = 725\n\
font-thicken = true\n\
font-thicken-strength = 191\n\
unsupported-key = ignored\n",
        )
        .expect("write config");

        let load = load_ghostty_appearance_from(&path);
        assert_eq!(load.invalid, 0, "{:#?}", load.diagnostics);
        assert_eq!(load.unsupported, 1);
        assert_eq!(load.appearance.background, Color::rgb(1, 2, 3));
        assert_eq!(load.appearance.cursor_color, Color::rgb(4, 5, 6));
        assert_eq!(load.appearance.selection_foreground, Color::rgb(7, 8, 9));
        assert_eq!(
            load.appearance.search_match_color,
            AppearanceColor::rgba(0x10, 0x11, 0x12, 0x80)
        );
        assert_eq!(load.appearance.link_color, Color::rgb(0x16, 0x17, 0x18));
        assert!((load.appearance.minimum_contrast - 4.5).abs() < f32::EPSILON);
        assert!((load.appearance.background_opacity - 0.75).abs() < f32::EPSILON);
        assert_eq!(load.appearance.cursor_blink_policy, CursorBlinkPolicy::Off);
        assert_eq!(load.appearance.cursor_blink_interval_ms, 725);
        assert!(load.appearance.font_thicken);
        assert_eq!(load.appearance.font_thicken_strength, 191);
        assert!(load.diagnostics.iter().any(|diagnostic| {
            diagnostic.key == "font-thicken"
                && diagnostic.disposition == AppearanceConfigDisposition::Applied
        }));
    }

    #[test]
    fn invalid_synthetic_style_and_thickening_strength_preserve_defaults() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("config");
        fs::write(
            &path,
            "font-synthetic-style = no-bold,unknown\nfont-thicken-strength = 256\n",
        )
        .expect("write config");

        let load = load_ghostty_appearance_from(&path);
        let defaults = TerminalAppearance::default();
        assert_eq!(load.invalid, 2, "{:#?}", load.diagnostics);
        assert_eq!(
            load.appearance.font_synthetic_style,
            defaults.font_synthetic_style
        );
        assert_eq!(
            load.appearance.font_thicken_strength,
            defaults.font_thicken_strength
        );
    }

    #[test]
    fn matches_ghostty_quotes_features_x11_colors_and_asymmetric_padding() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("config");
        fs::write(
            &path,
            "\
font-family = \"JetBrains Mono\"\n\
font-feature = -calt, -liga, 'kern' off, cv01 2\n\
window-padding-x = 2,4\n\
window-padding-y = 3,5\n\
foreground = gray42\n\
background = medium spring green\n\
palette = 0x0f=#102030\n\
palette = 0o20=#405060\n\
palette = 0b10001=#708090\n",
        )
        .expect("write config");

        let load = load_ghostty_appearance_from(&path);
        assert_eq!(load.invalid, 0, "{:#?}", load.diagnostics);
        assert_eq!(load.appearance.font_families, ["JetBrains Mono"]);
        assert_eq!(
            load.appearance.font_features,
            [
                FontFeature::new(*b"calt", 0),
                FontFeature::new(*b"liga", 0),
                FontFeature::new(*b"kern", 0),
                FontFeature::new(*b"cv01", 2),
            ]
        );
        assert!((load.appearance.padding_left - 2.0).abs() < f32::EPSILON);
        assert!((load.appearance.padding_right - 4.0).abs() < f32::EPSILON);
        assert!((load.appearance.padding_top - 3.0).abs() < f32::EPSILON);
        assert!((load.appearance.padding_bottom - 5.0).abs() < f32::EPSILON);
        assert_eq!(load.appearance.foreground, Color::rgb(107, 107, 107));
        assert_eq!(load.appearance.background, Color::rgb(0, 250, 154));
        assert_eq!(load.appearance.palette[15], Color::rgb(0x10, 0x20, 0x30));
        assert_eq!(load.appearance.palette[16], Color::rgb(0x40, 0x50, 0x60));
        assert_eq!(load.appearance.palette[17], Color::rgb(0x70, 0x80, 0x90));
    }

    #[test]
    fn invalid_font_feature_lines_do_not_partially_mutate_prior_state() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("config");
        fs::write(
            &path,
            "font-feature = liga=0\nfont-feature = liga=1, invalid\n",
        )
        .expect("write config");

        let load = load_ghostty_appearance_from(&path);
        assert_eq!(load.invalid, 1, "{:#?}", load.diagnostics);
        assert_eq!(
            load.appearance.font_features,
            [FontFeature::new(*b"liga", 0)]
        );
    }

    #[test]
    fn includes_apply_after_the_containing_file_and_optional_missing_files_are_quiet() {
        let directory = tempfile::tempdir().expect("tempdir");
        let root = directory.path().join("root");
        let child = directory.path().join("child");
        fs::write(
            &root,
            "config-file = child\nconfig-file = ?missing\nfont-size = 12\n",
        )
        .expect("write root");
        fs::write(&child, "font-size = 20\n").expect("write child");

        let load = load_ghostty_appearance_from(&root);
        assert_eq!(load.invalid, 0, "{:#?}", load.diagnostics);
        assert!((load.appearance.font_size_points - 20.0).abs() < f32::EPSILON);
    }

    #[test]
    fn quoted_empty_repeated_values_restore_defaults() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("config");
        fs::write(
            &path,
            "font-family = Custom\nfont-family = \"\"\nfont-family-bold = Custom Bold\nfont-family-bold = \"\"\nfont-feature = liga\nfont-feature = \"\"\n",
        )
        .expect("write config");

        let load = load_ghostty_appearance_from(&path);
        let defaults = TerminalAppearance::default();
        assert_eq!(load.invalid, 0, "{:#?}", load.diagnostics);
        assert_eq!(load.appearance.font_families, defaults.font_families);
        assert_eq!(
            load.appearance.font_families_bold,
            defaults.font_families_bold
        );
        assert_eq!(load.appearance.font_features, defaults.font_features);
    }

    #[test]
    fn configuration_diagnostics_are_counted_but_memory_bounded() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("config");
        fs::write(&path, "invalid\n".repeat(MAX_CONFIG_DIAGNOSTICS + 137)).expect("write config");

        let load = load_ghostty_appearance_from(&path);
        assert_eq!(load.invalid as usize, MAX_CONFIG_DIAGNOSTICS + 137);
        assert_eq!(load.diagnostics.len(), MAX_CONFIG_DIAGNOSTICS);
        assert_eq!(load.diagnostics_dropped, 137);
    }

    #[test]
    fn non_regular_configuration_roots_are_rejected_without_opening_them() {
        let directory = tempfile::tempdir().expect("tempdir");
        let load = load_ghostty_appearance_from(directory.path());
        assert_eq!(load.invalid, 1);
        assert!(load.diagnostics[0].message.contains("not a regular file"));
    }

    #[test]
    fn empty_repeated_values_restore_defaults_before_later_overrides() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("config");
        fs::write(
            &path,
            "\
font-family = First\n\
font-family =\n\
font-family = Final\n\
font-feature = liga=1\n\
font-feature =\n\
font-feature = calt=0\n\
palette = 7=#abcdef\n\
palette =\n\
font-size = 20\n\
font-size =\n",
        )
        .expect("write config");

        let load = load_ghostty_appearance_from(&path);
        let defaults = TerminalAppearance::default();
        assert_eq!(load.invalid, 0, "{:#?}", load.diagnostics);
        assert_eq!(load.appearance.font_families, ["Final"]);
        assert_eq!(
            load.appearance.font_features,
            [FontFeature::new(*b"calt", 0)]
        );
        assert_eq!(load.appearance.palette[7], defaults.palette[7]);
        assert!(
            (load.appearance.font_size_points - defaults.font_size_points).abs() < f32::EPSILON
        );
    }

    #[test]
    fn oversized_configuration_stops_before_unbounded_parsing() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("config");
        let file = fs::File::create(&path).expect("create oversized config");
        file.set_len(u64::try_from(MAX_CONFIG_BYTES).expect("limit fits u64") * 128)
            .expect("extend sparse config");

        let load = load_ghostty_appearance_from(&path);
        assert_eq!(load.invalid, 1);
        assert_eq!(load.bytes_read, 0);
        assert_eq!(load.appearance, TerminalAppearance::default());
        assert!(load.diagnostics[0].message.contains("byte limit"));
    }

    #[test]
    fn invalid_wire_values_are_rejected() {
        let appearance = TerminalAppearance {
            font_size_points: f32::NAN,
            ..TerminalAppearance::default()
        };
        assert_eq!(
            appearance.validate(),
            Err(AppearanceValidationError::FontSize)
        );
        let appearance = TerminalAppearance {
            font_families: vec!["x".repeat(MAX_FONT_FAMILY_BYTES + 1)],
            ..TerminalAppearance::default()
        };
        assert_eq!(
            appearance.validate(),
            Err(AppearanceValidationError::FontFamily)
        );
        let appearance = TerminalAppearance {
            cursor_blink_interval_ms: 0,
            ..TerminalAppearance::default()
        };
        assert_eq!(
            appearance.validate(),
            Err(AppearanceValidationError::BlinkInterval)
        );
    }
}
