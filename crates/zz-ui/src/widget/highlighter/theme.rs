//! The data types a syntax palette is made of.

use std::{ops::Deref, sync::Arc};

use gpui::{HighlightStyle, Hsla};

use crate::ThemeMode;

use super::palette;

/// The font style a palette entry can request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontStyle {
    Normal,
    Italic,
    Underline,
}

impl From<FontStyle> for gpui::FontStyle {
    fn from(style: FontStyle) -> Self {
        match style {
            FontStyle::Normal => gpui::FontStyle::Normal,
            FontStyle::Underline => gpui::FontStyle::Normal,
            FontStyle::Italic => gpui::FontStyle::Italic,
        }
    }
}

/// The font weight a palette entry can request. Discriminants are CSS weights.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum FontWeightContent {
    Thin = 100,
    ExtraLight = 200,
    Light = 300,
    Normal = 400,
    Medium = 500,
    Semibold = 600,
    Bold = 700,
    ExtraBold = 800,
    Black = 900,
}

impl From<FontWeightContent> for gpui::FontWeight {
    fn from(value: FontWeightContent) -> Self {
        match value {
            FontWeightContent::Thin => gpui::FontWeight::THIN,
            FontWeightContent::ExtraLight => gpui::FontWeight::EXTRA_LIGHT,
            FontWeightContent::Light => gpui::FontWeight::LIGHT,
            FontWeightContent::Normal => gpui::FontWeight::NORMAL,
            FontWeightContent::Medium => gpui::FontWeight::MEDIUM,
            FontWeightContent::Semibold => gpui::FontWeight::SEMIBOLD,
            FontWeightContent::Bold => gpui::FontWeight::BOLD,
            FontWeightContent::ExtraBold => gpui::FontWeight::EXTRA_BOLD,
            FontWeightContent::Black => gpui::FontWeight::BLACK,
        }
    }
}

/// How one capture is painted. An unset field leaves the surrounding style alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThemeStyle {
    pub color: Option<Hsla>,
    pub font_style: Option<FontStyle>,
    pub font_weight: Option<FontWeightContent>,
}

impl From<ThemeStyle> for HighlightStyle {
    fn from(style: ThemeStyle) -> Self {
        HighlightStyle {
            color: style.color,
            font_weight: style.font_weight.map(Into::into),
            font_style: style.font_style.map(Into::into),
            ..Default::default()
        }
    }
}

/// One [`ThemeStyle`] per tree-sitter capture, dotted names spelled with `_`.
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub struct SyntaxColors {
    pub attribute: Option<ThemeStyle>,
    pub boolean: Option<ThemeStyle>,
    pub comment: Option<ThemeStyle>,
    pub comment_doc: Option<ThemeStyle>,
    pub constant: Option<ThemeStyle>,
    pub constructor: Option<ThemeStyle>,
    pub embedded: Option<ThemeStyle>,
    pub emphasis: Option<ThemeStyle>,
    pub emphasis_strong: Option<ThemeStyle>,
    pub enum_: Option<ThemeStyle>,
    pub function: Option<ThemeStyle>,
    pub hint: Option<ThemeStyle>,
    pub keyword: Option<ThemeStyle>,
    pub label: Option<ThemeStyle>,
    pub link_text: Option<ThemeStyle>,
    pub link_uri: Option<ThemeStyle>,
    pub number: Option<ThemeStyle>,
    pub operator: Option<ThemeStyle>,
    pub predictive: Option<ThemeStyle>,
    pub preproc: Option<ThemeStyle>,
    pub primary: Option<ThemeStyle>,
    pub property: Option<ThemeStyle>,
    pub punctuation: Option<ThemeStyle>,
    pub punctuation_bracket: Option<ThemeStyle>,
    pub punctuation_delimiter: Option<ThemeStyle>,
    pub punctuation_list_marker: Option<ThemeStyle>,
    pub punctuation_special: Option<ThemeStyle>,
    pub string: Option<ThemeStyle>,
    pub string_escape: Option<ThemeStyle>,
    pub string_regex: Option<ThemeStyle>,
    pub string_special: Option<ThemeStyle>,
    pub string_special_symbol: Option<ThemeStyle>,
    pub tag: Option<ThemeStyle>,
    pub tag_doctype: Option<ThemeStyle>,
    pub text_code_span: Option<ThemeStyle>,
    pub text_literal: Option<ThemeStyle>,
    pub title: Option<ThemeStyle>,
    pub type_: Option<ThemeStyle>,
    pub variable: Option<ThemeStyle>,
    pub variable_special: Option<ThemeStyle>,
    pub variant: Option<ThemeStyle>,
}

impl SyntaxColors {
    /// Looks up a capture. A dotted name with no entry falls back to its prefix,
    /// so `keyword.modifier` paints like `keyword`.
    pub fn style(&self, name: &str) -> Option<HighlightStyle> {
        if name.is_empty() {
            return None;
        }

        let style = match name {
            "attribute" => self.attribute,
            "boolean" => self.boolean,
            "comment" => self.comment,
            "comment.doc" => self.comment_doc,
            "constant" => self.constant,
            "constructor" => self.constructor,
            "embedded" => self.embedded,
            "emphasis" => self.emphasis,
            "emphasis.strong" => self.emphasis_strong,
            "enum" => self.enum_,
            "function" => self.function,
            "hint" => self.hint,
            "keyword" => self.keyword,
            "label" => self.label,
            "link_text" => self.link_text,
            "link_uri" => self.link_uri,
            "number" => self.number,
            "operator" => self.operator,
            "predictive" => self.predictive,
            "preproc" => self.preproc,
            "primary" => self.primary,
            "property" => self.property,
            "punctuation" => self.punctuation,
            "punctuation.bracket" => self.punctuation_bracket,
            "punctuation.delimiter" => self.punctuation_delimiter,
            "punctuation.list_marker" => self.punctuation_list_marker,
            "punctuation.special" => self.punctuation_special,
            "string" => self.string,
            "string.escape" => self.string_escape,
            "string.regex" => self.string_regex,
            "string.special" => self.string_special,
            "string.special.symbol" => self.string_special_symbol,
            "tag" => self.tag,
            "tag.doctype" => self.tag_doctype,
            "text.code.span" => self.text_code_span,
            "text.literal" => self.text_literal,
            "title" => self.title,
            "type" => self.type_,
            "variable" => self.variable,
            "variable.special" => self.variable_special,
            "variant" => self.variant,
            _ => None,
        }
        .map(Into::into);

        if style.is_some() {
            style
        } else if name.contains('.') {
            name.split('.').next().and_then(|prefix| self.style(prefix))
        } else {
            None
        }
    }
}

/// Chrome colors around highlighted code, plus the palette. Both built-in
/// themes leave every `editor_*` field unset, so readers fall back.
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub struct HighlightThemeStyle {
    pub editor_background: Option<Hsla>,
    pub editor_foreground: Option<Hsla>,
    pub editor_active_line: Option<Hsla>,
    pub editor_line_number: Option<Hsla>,
    pub editor_active_line_number: Option<Hsla>,
    pub editor_invisible: Option<Hsla>,
    /// Gutter background, falling back to [`Self::editor_background`] when unset.
    pub editor_gutter_background: Option<Hsla>,
    pub syntax: SyntaxColors,
}

/// A named syntax theme: the palette plus the mode it was written for.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HighlightTheme {
    pub name: String,
    pub appearance: ThemeMode,
    pub style: HighlightThemeStyle,
}

impl Deref for HighlightTheme {
    type Target = SyntaxColors;

    fn deref(&self) -> &Self::Target {
        &self.style.syntax
    }
}

impl HighlightTheme {
    pub fn default_dark() -> Arc<Self> {
        palette::dark()
    }

    pub fn default_light() -> Arc<Self> {
        palette::light()
    }
}
