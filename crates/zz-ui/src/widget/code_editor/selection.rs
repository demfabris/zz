use std::ops::Range;

use gpui::{Context, Window};
use sum_tree::Bias;
use unicode_segmentation::UnicodeSegmentation as _;

use super::{CodeEditorState, RopeExt as _};

impl CodeEditorState {
    pub(super) fn select_word(&mut self, offset: usize, _: &mut Window, cx: &mut Context<Self>) {
        let range = word_range(&self.text.to_string(), offset);
        self.selected_range = range.clone().into();
        self.selected_word_range = Some(range.into());
        self.selection_reversed = false;
        cx.notify();
    }

    pub(super) fn select_line(&mut self, offset: usize, _: &mut Window, cx: &mut Context<Self>) {
        let offset = self.text.clip_offset(offset, Bias::Left);
        let row = self.text.offset_to_point(offset).row;
        let range = self.text.line_start_offset(row)..self.text.line_end_offset(row);
        self.selected_range = range.into();
        self.selected_word_range = None;
        self.selection_reversed = false;
        cx.notify();
    }
}

fn word_range(text: &str, offset: usize) -> Range<usize> {
    if text.is_empty() {
        return 0..0;
    }
    let offset = floor_char_boundary(text, offset.min(text.len()));

    for (start, word) in text.split_word_bound_indices() {
        let end = start + word.len();
        if offset >= start && offset < end {
            return start..end;
        }
    }

    let end = text[offset..]
        .chars()
        .next()
        .map_or(offset, |character| offset + character.len_utf8());
    offset..end
}

fn floor_char_boundary(text: &str, mut offset: usize) -> usize {
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_selection_keeps_unicode_graphemes_intact() {
        let text = "alpha rök 🎉";
        assert_eq!(&text[word_range(text, 7)], "rök");
        assert_eq!(&text[word_range(text, 11)], "🎉");
    }
}
