use gpui::{Context, Window};

use super::{
    CodeEditorState, RopeExt as _,
    state::{
        MoveDown, MoveEnd, MoveHome, MoveLeft, MovePageDown, MovePageUp, MoveRight, MoveToEnd,
        MoveToNextWord, MoveToPreviousWord, MoveToStart, MoveUp,
    },
    vim::VimKey,
};

impl CodeEditorState {
    pub(super) fn left(&mut self, _: &MoveLeft, window: &mut Window, cx: &mut Context<Self>) {
        if self.vim_key(VimKey::Left, window, cx) {
            return;
        }
        let target = if self.selected_range.is_empty() {
            self.previous_boundary(self.cursor())
        } else {
            self.selected_range.start
        };
        self.move_to(target, cx);
    }

    pub(super) fn right(&mut self, _: &MoveRight, window: &mut Window, cx: &mut Context<Self>) {
        if self.vim_key(VimKey::Right, window, cx) {
            return;
        }
        let target = if self.selected_range.is_empty() {
            self.next_boundary(self.cursor())
        } else {
            self.selected_range.end
        };
        self.move_to(target, cx);
    }

    pub(super) fn up(&mut self, _: &MoveUp, window: &mut Window, cx: &mut Context<Self>) {
        if self.vim_key(VimKey::Up, window, cx) {
            return;
        }
        self.move_vertical(-1, cx);
    }

    pub(super) fn down(&mut self, _: &MoveDown, window: &mut Window, cx: &mut Context<Self>) {
        if self.vim_key(VimKey::Down, window, cx) {
            return;
        }
        self.move_vertical(1, cx);
    }

    pub(super) fn page_up(&mut self, _: &MovePageUp, window: &mut Window, cx: &mut Context<Self>) {
        if self.vim_key(VimKey::PageUp, window, cx) {
            return;
        }
        self.move_vertical(-self.viewport_rows(), cx);
    }

    pub(super) fn page_down(
        &mut self,
        _: &MovePageDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.vim_key(VimKey::PageDown, window, cx) {
            return;
        }
        self.move_vertical(self.viewport_rows(), cx);
    }

    pub(super) fn home(&mut self, _: &MoveHome, window: &mut Window, cx: &mut Context<Self>) {
        if self.vim_key(VimKey::Home, window, cx) {
            return;
        }
        self.move_to(self.start_of_line(), cx);
    }

    pub(super) fn end(&mut self, _: &MoveEnd, window: &mut Window, cx: &mut Context<Self>) {
        if self.vim_key(VimKey::End, window, cx) {
            return;
        }
        self.move_to(self.end_of_line(), cx);
    }

    pub(super) fn move_to_start(
        &mut self,
        _: &MoveToStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_to(0, cx);
    }

    pub(super) fn move_to_end(&mut self, _: &MoveToEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.text.len(), cx);
    }

    pub(super) fn move_to_previous_word(
        &mut self,
        _: &MoveToPreviousWord,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_to(self.previous_word_start(self.cursor()), cx);
    }

    pub(super) fn move_to_next_word(
        &mut self,
        _: &MoveToNextWord,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_to(self.next_word_end(self.cursor()), cx);
    }

    fn move_vertical(&mut self, delta: isize, cx: &mut Context<Self>) {
        let point = self.text.offset_to_point(self.cursor());
        let column = self.preferred_column.unwrap_or(point.column);
        self.preferred_column = Some(column);
        let last_row = self.text.lines_len().saturating_sub(1);
        let row = point.row.saturating_add_signed(delta).min(last_row);
        let line = self.text.slice_line(row);
        let target = self.text.line_start_offset(row) + column.min(line.len());
        self.move_to_preserving_column(target, cx);
    }

    pub(super) fn viewport_rows(&self) -> isize {
        self.editor_layout
            .as_ref()
            .map(|layout| {
                (layout.viewport_bounds.size.height / layout.line_height)
                    .floor()
                    .max(1.0) as isize
            })
            .unwrap_or(10)
    }
}
