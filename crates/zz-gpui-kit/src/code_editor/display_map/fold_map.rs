use super::folding::FoldRange;
use super::wrap_map::WrapMap;

/// Projects wrap rows onto display rows by hiding folded regions.
pub struct FoldMap {
    /// display_row → wrap_row, ascending.
    visible_wrap_rows: Vec<usize>,

    /// wrap_row → display_row, None where folding hides the row.
    wrap_row_to_display_row: Vec<Option<usize>>,

    candidates: Vec<FoldRange>,
    folded: Vec<FoldRange>,
    needs_rebuild: bool,
    cached_wrap_row_count: usize,
}

impl FoldMap {
    pub fn new() -> Self {
        Self {
            visible_wrap_rows: Vec::new(),
            wrap_row_to_display_row: Vec::new(),
            candidates: Vec::new(),
            folded: Vec::new(),
            needs_rebuild: true,
            cached_wrap_row_count: 0,
        }
    }

    pub(super) fn mark_dirty_with_wrap_count(&mut self, wrap_row_count: usize) {
        self.needs_rebuild = true;
        self.cached_wrap_row_count = wrap_row_count;
    }

    pub fn display_row_count(&self) -> usize {
        if self.folded.is_empty() {
            return self.cached_wrap_row_count;
        }
        self.visible_wrap_rows.len()
    }

    /// Display row for a wrap row, or None if folding hides it.
    pub fn wrap_row_to_display_row(&self, wrap_row: usize) -> Option<usize> {
        if self.folded.is_empty() {
            return if wrap_row < self.cached_wrap_row_count {
                Some(wrap_row)
            } else {
                None
            };
        }
        self.wrap_row_to_display_row
            .get(wrap_row)
            .copied()
            .flatten()
    }

    pub fn display_row_to_wrap_row(&self, display_row: usize) -> Option<usize> {
        if self.folded.is_empty() {
            return if display_row < self.cached_wrap_row_count {
                Some(display_row)
            } else {
                None
            };
        }
        self.visible_wrap_rows.get(display_row).copied()
    }

    pub fn nearest_visible_display_row(&self, wrap_row: usize) -> usize {
        if self.folded.is_empty() {
            return wrap_row.min(self.cached_wrap_row_count.saturating_sub(1));
        }

        if let Some(dr) = self.wrap_row_to_display_row(wrap_row) {
            return dr;
        }

        match self.visible_wrap_rows.binary_search(&wrap_row) {
            Ok(idx) => idx,
            Err(insert_pos) => insert_pos.saturating_sub(1),
        }
    }

    /// Replaces every candidate and drops folds that are no longer candidates.
    pub fn set_candidates(&mut self, mut candidates: Vec<FoldRange>) {
        candidates.sort_by_key(|r| r.start_line);
        candidates.dedup_by_key(|r| r.start_line);
        self.candidates = candidates;

        self.folded.retain(|fold| {
            self.candidates
                .iter()
                .any(|c| c.start_line == fold.start_line)
        });
    }

    /// Replaces candidates inside `[edit_start_line, edit_end_line]` and keeps the rest.
    pub fn merge_candidates_for_edit(
        &mut self,
        edit_start_line: usize,
        edit_end_line: usize,
        new_candidates: Vec<FoldRange>,
    ) {
        self.candidates
            .retain(|c| c.start_line < edit_start_line || c.start_line > edit_end_line);

        self.candidates.extend(new_candidates);
        self.candidates.sort_by_key(|r| r.start_line);
        self.candidates.dedup_by_key(|r| r.start_line);
    }

    /// Folds or unfolds at `start_line`, which must be a candidate.
    pub fn set_folded(&mut self, start_line: usize, folded: bool) {
        if folded {
            if let Some(candidate) = self.candidates.iter().find(|c| c.start_line == start_line) {
                if !self.folded.iter().any(|f| f.start_line == start_line) {
                    self.folded.push(*candidate);
                    self.folded.sort_by_key(|r| r.start_line);
                    self.needs_rebuild = true;
                }
            }
        } else {
            self.folded.retain(|f| f.start_line != start_line);
            self.needs_rebuild = true;
        }
    }

    pub fn toggle_fold(&mut self, start_line: usize) {
        let is_folded = self.is_folded_at(start_line);
        self.set_folded(start_line, !is_folded);
    }

    pub fn is_folded_at(&self, start_line: usize) -> bool {
        self.folded.iter().any(|f| f.start_line == start_line)
    }

    pub fn is_fold_candidate(&self, start_line: usize) -> bool {
        self.candidates.iter().any(|c| c.start_line == start_line)
    }

    #[inline]
    pub fn fold_candidates(&self) -> &[FoldRange] {
        &self.candidates
    }

    #[inline]
    pub fn folded_ranges(&self) -> &[FoldRange] {
        &self.folded
    }

    #[inline]
    pub fn clear_folds(&mut self) {
        self.folded.clear();
    }

    /// Drops folds and candidates touching the edited lines, shifts later ones by `line_delta`.
    pub fn adjust_folds_for_edit(
        &mut self,
        edit_start_line: usize,
        edit_end_line: usize,
        line_delta: isize,
    ) {
        if !self.folded.is_empty() {
            self.folded.retain(|fold| {
                !(fold.start_line <= edit_end_line && fold.end_line >= edit_start_line)
            });

            if line_delta != 0 {
                for fold in &mut self.folded {
                    if fold.start_line > edit_end_line {
                        fold.start_line = (fold.start_line as isize + line_delta).max(0) as usize;
                        fold.end_line = (fold.end_line as isize + line_delta).max(0) as usize;
                    }
                }
            }
        }

        if !self.candidates.is_empty() {
            self.candidates
                .retain(|c| !(c.start_line <= edit_end_line && c.end_line >= edit_start_line));

            if line_delta != 0 {
                for c in &mut self.candidates {
                    if c.start_line > edit_end_line {
                        c.start_line = (c.start_line as isize + line_delta).max(0) as usize;
                        c.end_line = (c.end_line as isize + line_delta).max(0) as usize;
                    }
                }
            }
        }

        self.needs_rebuild = true;
    }

    /// Recomputes the wrap-row to display-row projection.
    pub fn rebuild(&mut self, wrap_map: &WrapMap) {
        let wrap_row_count = wrap_map.wrap_row_count();

        if !self.needs_rebuild && wrap_row_count == self.cached_wrap_row_count {
            return;
        }

        self.cached_wrap_row_count = wrap_row_count;

        self.visible_wrap_rows.clear();
        self.wrap_row_to_display_row = vec![None; wrap_row_count];

        if self.folded.is_empty() {
            self.visible_wrap_rows = (0..wrap_row_count).collect();
            for (display_row, &wrap_row) in self.visible_wrap_rows.iter().enumerate() {
                self.wrap_row_to_display_row[wrap_row] = Some(display_row);
            }
            self.needs_rebuild = false;
            return;
        }

        let mut hidden_ranges = Vec::new();
        for fold in &self.folded {
            let hide_start_line = fold.start_line + 1;
            let hide_end_line = fold.end_line.saturating_sub(1);

            if hide_start_line > hide_end_line {
                continue;
            }

            let start_wrap_row = wrap_map.buffer_line_to_first_wrap_row(hide_start_line);
            let end_wrap_row = if hide_end_line + 1 < wrap_map.buffer_line_count() {
                wrap_map.buffer_line_to_first_wrap_row(hide_end_line + 1)
            } else {
                wrap_row_count
            };

            if start_wrap_row < end_wrap_row {
                hidden_ranges.push(start_wrap_row..end_wrap_row);
            }
        }

        hidden_ranges.sort_by_key(|r| r.start);
        let mut merged_hidden = Vec::new();
        for range in hidden_ranges {
            if let Some(last) = merged_hidden.last_mut() {
                if range.start <= *last {
                    *last = (*last).max(range.end);
                } else {
                    merged_hidden.push(range.start);
                    merged_hidden.push(range.end);
                }
            } else {
                merged_hidden.push(range.start);
                merged_hidden.push(range.end);
            }
        }

        let mut display_row = 0;
        let mut hidden_iter = merged_hidden.chunks_exact(2);
        let mut current_hidden = hidden_iter.next();

        for wrap_row in 0..wrap_row_count {
            let is_hidden = if let Some(&[start, end]) = current_hidden {
                if wrap_row >= end {
                    current_hidden = hidden_iter.next();
                    if let Some(&[new_start, new_end]) = current_hidden {
                        wrap_row >= new_start && wrap_row < new_end
                    } else {
                        false
                    }
                } else {
                    wrap_row >= start && wrap_row < end
                }
            } else {
                false
            };

            if !is_hidden {
                self.visible_wrap_rows.push(wrap_row);
                self.wrap_row_to_display_row[wrap_row] = Some(display_row);
                display_row += 1;
            }
        }

        self.needs_rebuild = false;
    }
}
