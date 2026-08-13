//! Style knobs a call site can set on a `TextView`.
#![allow(clippy::pedantic, clippy::style, clippy::complexity)]

use std::sync::Arc;

use gpui::{Pixels, Rems, StyleRefinement, px, rems};

use crate::highlighter::HighlightTheme;

#[derive(Clone)]
pub struct TextViewStyle {
    /// Gap between paragraphs, default is 1 rem.
    pub paragraph_gap: Rems,
    /// Base font size for headings, default is 14px.
    pub heading_base_font_size: Pixels,
    /// Computes a heading's size from its level (1-6) and the base font size.
    pub heading_font_size: Option<Arc<dyn Fn(u8, Pixels) -> Pixels + Send + Sync + 'static>>,
    /// Highlight theme for code blocks. Default: [`HighlightTheme::default_light()`]
    pub highlight_theme: Arc<HighlightTheme>,
    pub code_block: StyleRefinement,
    /// Style for the bordered table container. Set `overflow_x: scroll` here to
    /// scroll wide tables instead of wrapping cell content.
    pub table: StyleRefinement,
    pub table_cell: StyleRefinement,
    pub is_dark: bool,
}

impl PartialEq for TextViewStyle {
    fn eq(&self, other: &Self) -> bool {
        self.paragraph_gap == other.paragraph_gap
            && self.heading_base_font_size == other.heading_base_font_size
            && self.highlight_theme == other.highlight_theme
    }
}

impl Default for TextViewStyle {
    fn default() -> Self {
        Self {
            paragraph_gap: rems(1.),
            heading_base_font_size: px(14.),
            heading_font_size: None,
            highlight_theme: HighlightTheme::default_light().clone(),
            code_block: StyleRefinement::default(),
            table: StyleRefinement::default(),
            table_cell: StyleRefinement::default(),
            is_dark: false,
        }
    }
}

impl TextViewStyle {
    /// Set paragraph gap, default is 1 rem.
    pub fn paragraph_gap(mut self, gap: Rems) -> Self {
        self.paragraph_gap = gap;
        self
    }

    pub fn heading_font_size<F>(mut self, f: F) -> Self
    where
        F: Fn(u8, Pixels) -> Pixels + Send + Sync + 'static,
    {
        self.heading_font_size = Some(Arc::new(f));
        self
    }

    pub fn code_block(mut self, style: StyleRefinement) -> Self {
        self.code_block = style;
        self
    }

    /// Set `overflow_x: scroll` on the refinement to scroll wide tables instead
    /// of shrinking them to fit.
    pub fn table(mut self, style: StyleRefinement) -> Self {
        self.table = style;
        self
    }

    pub fn table_cell(mut self, style: StyleRefinement) -> Self {
        self.table_cell = style;
        self
    }
}
