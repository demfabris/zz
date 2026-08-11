use std::ops::Range;

use gpui::{App, Font, Pixels};
use ropey::Rope;

use super::fold_map::FoldMap;
use super::text_wrapper::{LineItem, TextWrapper, WrapDisplayPoint};
use super::{BufferPoint, WrapPoint};
use crate::code_editor::rope_ext::RopeExt;

/// Soft-wraps buffer lines into wrap rows, over the [`TextWrapper`] sum tree.
pub struct WrapMap {
    wrapper: TextWrapper,
}

impl WrapMap {
    pub fn new(font: Font, font_size: Pixels, wrap_width: Option<Pixels>) -> Self {
        Self {
            wrapper: TextWrapper::new(font, font_size, wrap_width),
        }
    }

    /// Visual row count after soft wrapping.
    #[inline]
    pub fn wrap_row_count(&self) -> usize {
        self.wrapper.len()
    }

    /// Logical line count, before soft wrapping.
    #[inline]
    pub fn buffer_line_count(&self) -> usize {
        self.wrapper.lines_count()
    }

    pub(super) fn buffer_pos_to_wrap_pos(&self, pos: BufferPoint) -> WrapPoint {
        let BufferPoint { line, col } = pos;

        let line = line.min(self.buffer_line_count().saturating_sub(1));
        let line_item = self.wrapper.line(line);

        let col = if let Some(line_item) = line_item {
            col.min(line_item.len())
        } else {
            0
        };

        let line_start_offset = self.wrapper.text().line_start_offset(line);
        let offset = line_start_offset + col;

        let display_point = self.wrapper.offset_to_display_point(offset);

        WrapPoint::new(display_point.row, display_point.column)
    }

    pub(super) fn wrap_pos_to_buffer_pos(&self, pos: WrapPoint) -> BufferPoint {
        let WrapPoint { row, col } = pos;

        let row = row.min(self.wrap_row_count().saturating_sub(1));

        let display_point = WrapDisplayPoint::new(row, 0, col);
        let offset = self.wrapper.display_point_to_offset(display_point);

        let point = self.wrapper.text().offset_to_point(offset);
        let line_start = self.wrapper.text().line_start_offset(point.row);
        let col = offset.saturating_sub(line_start);

        BufferPoint::new(point.row, col)
    }

    pub fn wrap_row_to_buffer_line(&self, wrap_row: usize) -> usize {
        self.wrapper.wrap_row_to_buffer_line(wrap_row)
    }

    pub fn buffer_line_to_first_wrap_row(&self, line: usize) -> usize {
        self.wrapper.buffer_line_to_first_wrap_row(line)
    }

    /// Wrap rows `[start, end)` that a buffer line occupies.
    pub fn buffer_line_to_wrap_row_range(&self, line: usize) -> Range<usize> {
        self.wrapper.buffer_line_to_wrap_row_range(line)
    }

    pub fn on_text_changed(
        &mut self,
        changed_text: &Rope,
        range: &Range<usize>,
        new_text: &Rope,
        cx: &mut App,
    ) {
        self.wrapper.update(changed_text, range, new_text, cx);
    }

    pub fn on_layout_changed(&mut self, wrap_width: Option<Pixels>, cx: &mut App) {
        self.wrapper.set_wrap_width(wrap_width, cx);
    }

    pub fn set_font(&mut self, font: Font, font_size: Pixels, cx: &mut App) {
        self.wrapper.set_font(font, font_size, cx);
    }

    pub fn ensure_text_prepared(&mut self, text: &Rope, cx: &mut App) -> bool {
        self.wrapper.prepare_if_need(text, cx)
    }

    pub fn set_text(&mut self, text: &Rope, cx: &mut App) {
        self.wrapper.set_default_text(text);
        self.wrapper.prepare_if_need(text, cx);
    }

    pub(crate) fn wrapper(&self) -> &TextWrapper {
        &self.wrapper
    }

    #[inline]
    pub(crate) fn line(&self, row: usize) -> Option<&LineItem> {
        self.wrapper.line(row)
    }

    pub fn text(&self) -> &Rope {
        self.wrapper.text()
    }

    pub fn visible_wrap_row_count_for_line(&self, line: usize, fold_map: &FoldMap) -> usize {
        let wrap_range = self.buffer_line_to_wrap_row_range(line);
        wrap_range
            .filter(|&wr| fold_map.wrap_row_to_display_row(wr).is_some())
            .count()
    }
}
