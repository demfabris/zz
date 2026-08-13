use std::ops::Range;

use gpui::{App, Font, Pixels};
use ropey::Rope;

use super::fold_map::FoldMap;
use super::folding::FoldRange;
use super::text_wrapper::{LineItem, WrapDisplayPoint};
use super::wrap_map::WrapMap;
use super::{BufferPoint, DisplayPoint};
use crate::code_editor::Point as TreeSitterPoint;
use crate::code_editor::display_map::WrapPoint;
use crate::code_editor::rope_ext::RopeExt as _;

/// Maps buffer positions to display positions through the wrap and fold layers.
pub struct DisplayMap {
    wrap_map: WrapMap,
    fold_map: FoldMap,
}

impl DisplayMap {
    pub fn new(font: Font, font_size: Pixels, wrap_width: Option<Pixels>) -> Self {
        Self {
            wrap_map: WrapMap::new(font, font_size, wrap_width),
            fold_map: FoldMap::new(),
        }
    }

    pub fn buffer_pos_to_display_pos(&self, pos: BufferPoint) -> DisplayPoint {
        let wrap_pos = self.wrap_map.buffer_pos_to_wrap_pos(pos);

        if let Some(display_row) = self.fold_map.wrap_row_to_display_row(wrap_pos.row) {
            DisplayPoint::new(display_row, wrap_pos.col)
        } else {
            let display_row = self.fold_map.nearest_visible_display_row(wrap_pos.row);
            DisplayPoint::new(display_row, 0)
        }
    }

    pub fn display_pos_to_buffer_pos(&self, pos: DisplayPoint) -> BufferPoint {
        let wrap_row = self.fold_map.display_row_to_wrap_row(pos.row).unwrap_or(0);

        let wrap_pos = WrapPoint::new(wrap_row, pos.col);
        self.wrap_map.wrap_pos_to_buffer_pos(wrap_pos)
    }

    #[inline]
    pub fn display_row_count(&self) -> usize {
        self.fold_map.display_row_count()
    }

    pub fn display_row_to_buffer_line(&self, display_row: usize) -> usize {
        let wrap_row = self
            .fold_map
            .display_row_to_wrap_row(display_row)
            .unwrap_or(0);

        self.wrap_map.wrap_row_to_buffer_line(wrap_row)
    }

    /// Display rows `[start, end)` covering a buffer line, or None if it is fully folded.
    pub fn buffer_line_to_display_row_range(&self, line: usize) -> Option<Range<usize>> {
        let wrap_row_range = self.wrap_map.buffer_line_to_wrap_row_range(line);

        let mut first_display_row = None;
        let mut last_display_row = None;

        for wrap_row in wrap_row_range {
            if let Some(display_row) = self.fold_map.wrap_row_to_display_row(wrap_row) {
                if first_display_row.is_none() {
                    first_display_row = Some(display_row);
                }
                last_display_row = Some(display_row);
            }
        }

        if let (Some(start), Some(end)) = (first_display_row, last_display_row) {
            Some(start..end + 1)
        } else {
            None
        }
    }

    #[inline]
    pub fn is_buffer_line_hidden(&self, line: usize) -> bool {
        self.buffer_line_to_display_row_range(line).is_none()
    }

    /// First display row of a buffer line, or the nearest visible row if it is folded.
    pub fn buffer_line_to_display_row(&self, line: usize) -> usize {
        match self.buffer_line_to_display_row_range(line) {
            Some(range) => range.start,
            None => {
                let wrap_row = self.wrap_map.buffer_line_to_first_wrap_row(line);
                self.fold_map.nearest_visible_display_row(wrap_row)
            }
        }
    }

    pub fn set_fold_candidates(&mut self, candidates: Vec<FoldRange>) {
        self.fold_map.set_candidates(candidates);
        self.rebuild_fold_projection();
    }

    /// Folds or unfolds at `start_line`, which must be a candidate.
    pub fn set_folded(&mut self, start_line: usize, folded: bool) {
        self.fold_map.set_folded(start_line, folded);
        self.rebuild_fold_projection();
    }

    pub fn toggle_fold(&mut self, start_line: usize) {
        self.fold_map.toggle_fold(start_line);
        self.rebuild_fold_projection();
    }

    #[inline]
    pub fn is_folded_at(&self, start_line: usize) -> bool {
        self.fold_map.is_folded_at(start_line)
    }

    #[inline]
    pub fn is_fold_candidate(&self, start_line: usize) -> bool {
        self.fold_map.is_fold_candidate(start_line)
    }

    #[inline]
    pub fn folded_ranges(&self) -> &[FoldRange] {
        self.fold_map.folded_ranges()
    }

    pub fn clear_folds(&mut self) {
        self.fold_map.clear_folds();
        self.rebuild_fold_projection();
    }

    /// Shifts folds for an edit. Call with the pre-edit text, before the wrap map updates.
    pub fn adjust_folds_for_edit(&mut self, old_text: &Rope, range: &Range<usize>, new_text: &str) {
        if self.fold_map.folded_ranges().is_empty() && self.fold_map.fold_candidates().is_empty() {
            return;
        }

        let edit_start_line = old_text.offset_to_point(range.start).row;
        let edit_end_line = old_text.offset_to_point(range.end.min(old_text.len())).row;

        let old_lines_in_range = edit_end_line.saturating_sub(edit_start_line);
        let new_lines_in_range = new_text.chars().filter(|c| *c == '\n').count();
        let line_delta = new_lines_in_range as isize - old_lines_in_range as isize;

        self.fold_map
            .adjust_folds_for_edit(edit_start_line, edit_end_line, line_delta);
    }

    /// Re-extracts candidates inside the edited byte range and merges them with the rest.
    pub fn update_fold_candidates_for_edit(
        &mut self,
        tree: &super::folding::Tree,
        edit_byte_range: Range<usize>,
        new_text: &Rope,
    ) {
        let new_start_line = new_text.offset_to_point(edit_byte_range.start).row;
        let new_end_line = new_text
            .offset_to_point(edit_byte_range.end.min(new_text.len()))
            .row;

        let new_candidates = super::folding::extract_fold_ranges_in_range(tree, edit_byte_range);
        self.fold_map
            .merge_candidates_for_edit(new_start_line, new_end_line, new_candidates);
    }

    pub fn on_text_changed(
        &mut self,
        changed_text: &Rope,
        range: &Range<usize>,
        new_text: &Rope,
        cx: &mut App,
    ) {
        self.wrap_map
            .on_text_changed(changed_text, range, new_text, cx);
        self.rebuild_fold_projection();
    }

    pub fn on_layout_changed(&mut self, wrap_width: Option<Pixels>, cx: &mut App) {
        self.wrap_map.on_layout_changed(wrap_width, cx);
        self.rebuild_fold_projection();
    }

    pub fn set_font(&mut self, font: Font, font_size: Pixels, cx: &mut App) {
        self.wrap_map.set_font(font, font_size, cx);
        self.rebuild_fold_projection();
    }

    pub fn ensure_text_prepared(&mut self, text: &Rope, cx: &mut App) {
        let did_initialize = self.wrap_map.ensure_text_prepared(text, cx);
        if did_initialize {
            self.rebuild_fold_projection();
        }
    }

    pub fn set_text(&mut self, text: &Rope, cx: &mut App) {
        self.wrap_map.set_text(text, cx);
        self.rebuild_fold_projection();
    }

    fn rebuild_fold_projection(&mut self) {
        if !self.fold_map.folded_ranges().is_empty() {
            self.fold_map.rebuild(&self.wrap_map);
        } else {
            self.fold_map
                .mark_dirty_with_wrap_count(self.wrap_map.wrap_row_count());
        }
    }

    #[inline]
    pub(crate) fn offset_to_wrap_display_point(&self, offset: usize) -> WrapDisplayPoint {
        self.wrap_map.wrapper().offset_to_display_point(offset)
    }

    #[inline]
    pub(crate) fn wrap_display_point_to_offset(&self, point: WrapDisplayPoint) -> usize {
        self.wrap_map.wrapper().display_point_to_offset(point)
    }

    #[inline]
    pub(crate) fn wrap_display_point_to_point(&self, point: WrapDisplayPoint) -> TreeSitterPoint {
        self.wrap_map.wrapper().display_point_to_point(point)
    }

    /// Display row for a wrap row, or None if that row is folded away.
    #[inline]
    pub fn wrap_row_to_display_row(&self, wrap_row: usize) -> Option<usize> {
        self.fold_map.wrap_row_to_display_row(wrap_row)
    }

    #[inline]
    pub fn nearest_visible_display_row(&self, wrap_row: usize) -> usize {
        self.fold_map.nearest_visible_display_row(wrap_row)
    }

    #[inline]
    pub fn display_row_to_wrap_row(&self, display_row: usize) -> Option<usize> {
        self.fold_map.display_row_to_wrap_row(display_row)
    }

    #[inline]
    pub(crate) fn longest_row(&self) -> usize {
        self.wrap_map.wrapper().longest_row()
    }

    #[inline]
    pub(crate) fn line(&self, row: usize) -> Option<&LineItem> {
        self.wrap_map.line(row)
    }

    #[inline]
    pub fn text(&self) -> &Rope {
        self.wrap_map.text()
    }

    #[inline]
    pub fn visible_wrap_row_count_for_buffer_line(&self, line: usize) -> usize {
        self.wrap_map
            .visible_wrap_row_count_for_line(line, &self.fold_map)
    }

    /// Row count before folding.
    #[inline]
    pub fn wrap_row_count(&self) -> usize {
        self.wrap_map.wrap_row_count()
    }

    #[inline]
    pub fn buffer_line_count(&self) -> usize {
        self.wrap_map.buffer_line_count()
    }
}
