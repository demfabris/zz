use std::ops::Range;

use gpui::{ClipboardItem, Context, Window};
use sum_tree::Bias;

use crate::code_editor::{CodeEditorState, RopeExt as _};

use super::super::state::{Enter, Redo, Undo};
use super::{
    Register, VimMode, VimState,
    motion::{self, Motion, MotionContext, OperatorSpan},
    parser::{
        self, Command, InsertAt, Key, Operator, OperatorTarget, ScrollAlign, Step, Verb, VisualKind,
    },
    text_object::{self, TextObject},
};

impl CodeEditorState {
    /// Turns the vim layer on or off. Enabling enters normal mode.
    pub fn set_vim_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if enabled == self.vim.is_some() {
            return;
        }
        self.vim = enabled.then(VimState::default);
        if enabled {
            let cursor = self.vim_clamp(self.cursor());
            self.move_to(cursor, cx);
        }
        cx.notify();
    }

    /// The current vim mode, or None when the layer is off.
    pub fn vim_mode(&self) -> Option<VimMode> {
        self.vim.as_ref().map(VimState::mode)
    }

    pub(crate) fn vim_block_cursor(&self) -> bool {
        self.vim_mode()
            .is_some_and(|mode| !matches!(mode, VimMode::Insert))
    }

    pub(crate) fn vim_on_pointer(&mut self) {
        if let Some(vim) = self.vim.as_mut() {
            if vim.mode.is_visual() {
                vim.mode = VimMode::Normal;
            }
        }
    }

    pub(crate) fn vim_highlight_range(&self) -> Option<Range<usize>> {
        let (start, end) = self.vim_visual_ends()?;
        match self.vim_mode()? {
            VimMode::Visual => Some(start..self.next_boundary(end)),
            VimMode::VisualLine => {
                Some(motion::line_span(&self.text, self.vim_row(start), self.vim_row(end)).content)
            }
            _ => None,
        }
    }

    pub(crate) fn vim_key(
        &mut self,
        key: Key,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(vim) = self.vim.as_mut() else {
            return false;
        };
        if vim.mode == VimMode::Insert {
            if key == Key::Escape {
                self.vim_leave_insert(cx);
                return true;
            }
            return false;
        }

        let mode = vim.mode;
        let had_pending = !vim.pending.is_empty();
        match parser::step(&mut vim.pending, mode, key) {
            Step::Pending => true,
            Step::Cancel => had_pending || key != Key::Escape,
            Step::PassThrough => false,
            Step::Command(command) => {
                self.vim_run(command, window, cx);
                true
            }
        }
    }

    pub(crate) fn vim_intercepts_text(&self) -> bool {
        self.vim_block_cursor()
    }
}

impl CodeEditorState {
    fn vim_run(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        let count = command.count;
        match command.verb {
            Verb::Motion(motion) => self.vim_motion(motion, count, cx),
            Verb::Operate { operator, target } => self.vim_operate(operator, target, count, cx),
            Verb::SelectObject(object) => self.vim_select_object(object, cx),
            Verb::Insert(at) => self.vim_insert(at, window, cx),
            Verb::DeleteChar { before } => {
                self.vim_delete_char(Operator::Delete, before, count, cx);
            }
            Verb::DeleteToLineEnd => self.vim_to_line_end(count, false, cx),
            Verb::ChangeToLineEnd => self.vim_to_line_end(count, true, cx),
            Verb::YankLine => {
                let row = self.vim_row(self.cursor());
                self.vim_linewise(Operator::Yank, row, self.vim_last_row(row, count), cx);
            }
            Verb::SubstituteChar => self.vim_delete_char(Operator::Change, false, count, cx),
            Verb::SubstituteLine => {
                let row = self.vim_row(self.cursor());
                self.vim_linewise(Operator::Change, row, self.vim_last_row(row, count), cx);
            }
            Verb::Join => self.vim_join(count, cx),
            Verb::Replace(character) => self.vim_replace(character, count, cx),
            Verb::ToggleCase => self.vim_toggle_case(count, cx),
            Verb::Indent { outdent } => self.vim_indent(outdent, count, cx),
            Verb::Paste { before } => self.vim_paste(before, count, cx),
            Verb::Undo => {
                self.undo(&Undo, window, cx);
                self.vim_set_mode_and_cursor(VimMode::Normal, self.cursor(), cx);
            }
            Verb::Redo => {
                self.redo(&Redo, window, cx);
                self.vim_set_mode_and_cursor(VimMode::Normal, self.cursor(), cx);
            }
            Verb::EnterVisual(kind) => self.vim_enter_visual(kind, cx),
            Verb::SwapVisualEnds => self.vim_swap_visual_ends(cx),
            Verb::EnterNormal => self.vim_set_mode_and_cursor(VimMode::Normal, self.cursor(), cx),
            Verb::Scroll(align) => self.vim_scroll_to(align, cx),
        }
    }

    fn vim_motion(&mut self, motion: Motion, count: Option<usize>, cx: &mut Context<Self>) {
        self.vim_remember_find(motion);
        let context = self.vim_motion_context();
        let cursor = self.cursor();
        let Some(target) = motion::resolve_motion(&self.text, cursor, motion, count, &context)
        else {
            return;
        };
        let before = self.vim_row(cursor);
        self.vim_move(target.offset, target.goal_column, cx);
        if matches!(motion, Motion::HalfPage { .. } | Motion::Page { .. }) {
            let rows = self.vim_row(self.cursor()) as isize - before as isize;
            self.vim_scroll_rows(rows);
        }
    }

    fn vim_operate(
        &mut self,
        operator: Operator,
        target: OperatorTarget,
        count: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        let cursor = self.cursor();
        let span = match target {
            OperatorTarget::Motion(motion) => {
                self.vim_remember_find(motion);
                let context = self.vim_motion_context();
                motion::operator_span(&self.text, cursor, operator, motion, count, &context)
            }
            OperatorTarget::Object(object) => {
                text_object::resolve_object(&self.text, cursor, object).map(OperatorSpan::Charwise)
            }
            OperatorTarget::Line => {
                let row = self.vim_row(cursor);
                Some(OperatorSpan::Linewise {
                    first_row: row,
                    last_row: self.vim_last_row(row, count),
                })
            }
            OperatorTarget::Selection => self.vim_selection_span(),
        };

        match span {
            Some(OperatorSpan::Charwise(range)) => self.vim_charwise(operator, range, cx),
            Some(OperatorSpan::Linewise {
                first_row,
                last_row,
            }) => self.vim_linewise(operator, first_row, last_row, cx),
            None => self.vim_set_mode_and_cursor(VimMode::Normal, cursor, cx),
        }
    }

    fn vim_charwise(&mut self, operator: Operator, range: Range<usize>, cx: &mut Context<Self>) {
        let range = self.vim_clamp_range(range);
        let text = self.text.slice(range.clone()).to_string();
        self.vim_store(text, false, cx);
        match operator {
            Operator::Yank => self.vim_set_mode_and_cursor(VimMode::Normal, range.start, cx),
            Operator::Delete => {
                self.replace_range(range.clone(), "", false, cx);
                self.vim_set_mode_and_cursor(VimMode::Normal, range.start, cx);
            }
            Operator::Change => {
                self.replace_range(range.clone(), "", false, cx);
                self.vim_set_mode_and_cursor(VimMode::Insert, range.start, cx);
            }
        }
    }

    fn vim_linewise(
        &mut self,
        operator: Operator,
        first_row: usize,
        last_row: usize,
        cx: &mut Context<Self>,
    ) {
        let span = motion::line_span(&self.text, first_row, last_row);
        let mut text = self.text.slice(span.content.clone()).to_string();
        if !text.ends_with('\n') {
            text.push('\n');
        }
        self.vim_store(text, true, cx);
        match operator {
            Operator::Yank => {
                self.vim_set_mode_and_cursor(VimMode::Normal, self.cursor(), cx);
            }
            Operator::Delete => {
                self.replace_range(span.delete.clone(), "", false, cx);
                let row = first_row.min(self.text.lines_len().saturating_sub(1));
                let cursor = motion::first_non_blank(&self.text, row);
                self.vim_set_mode_and_cursor(VimMode::Normal, cursor, cx);
            }
            Operator::Change => {
                let indent = self.vim_indent_text(first_row);
                let start = self.text.line_start_offset(first_row);
                let end = self.text.line_end_offset(last_row);
                let cursor = start + indent.len();
                self.replace_range(start..end, &indent, false, cx);
                self.vim_set_mode_and_cursor(VimMode::Insert, cursor, cx);
            }
        }
    }

    fn vim_select_object(&mut self, object: TextObject, cx: &mut Context<Self>) {
        let Some(range) = text_object::resolve_object(&self.text, self.cursor(), object) else {
            return;
        };
        if let Some(vim) = self.vim.as_mut() {
            vim.visual_anchor = range.start;
        }
        let cursor = self.previous_boundary(range.end).max(range.start);
        self.vim_move(cursor, None, cx);
    }

    fn vim_insert(&mut self, at: InsertAt, window: &mut Window, cx: &mut Context<Self>) {
        let cursor = self.cursor();
        let row = self.vim_row(cursor);
        self.vim_set_mode(VimMode::Insert);
        match at {
            InsertAt::Cursor => self.move_to(cursor, cx),
            InsertAt::After => {
                let target = self
                    .next_boundary(cursor)
                    .min(self.text.line_end_offset(row));
                self.move_to(target, cx);
            }
            InsertAt::LineStart => {
                let target = motion::first_non_blank(&self.text, row);
                self.move_to(target, cx);
            }
            InsertAt::LineEnd => {
                let target = self.text.line_end_offset(row);
                self.move_to(target, cx);
            }
            InsertAt::OpenBelow => {
                let target = self.text.line_end_offset(row);
                self.move_to(target, cx);
                self.enter(&Enter, window, cx);
            }
            InsertAt::OpenAbove => {
                let indent = self.vim_indent_text(row);
                let start = self.text.line_start_offset(row);
                let cursor = start + indent.len();
                self.replace_range(start..start, &format!("{indent}\n"), false, cx);
                self.move_to(cursor, cx);
            }
        }
    }

    fn vim_delete_char(
        &mut self,
        operator: Operator,
        before: bool,
        count: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        let cursor = self.cursor();
        let row = self.vim_row(cursor);
        let range = if before {
            let bound = self.text.line_start_offset(row);
            let mut start = cursor;
            for _ in 0..count.unwrap_or(1) {
                if start <= bound {
                    break;
                }
                start = self.previous_boundary(start);
            }
            start..cursor
        } else {
            let bound = self.text.line_end_offset(row);
            let mut end = cursor;
            for _ in 0..count.unwrap_or(1) {
                if end >= bound {
                    break;
                }
                end = self.next_boundary(end);
            }
            cursor..end
        };
        if range.is_empty() {
            return;
        }
        self.vim_charwise(operator, range, cx);
    }

    fn vim_to_line_end(&mut self, count: Option<usize>, change: bool, cx: &mut Context<Self>) {
        let cursor = self.cursor();
        let row = self.vim_last_row(self.vim_row(cursor), count);
        let range = cursor..self.text.line_end_offset(row);
        let operator = if change {
            Operator::Change
        } else {
            Operator::Delete
        };
        self.vim_charwise(operator, range, cx);
    }

    fn vim_join(&mut self, count: Option<usize>, cx: &mut Context<Self>) {
        let row = self.vim_row(self.cursor());
        let last_row = self.vim_last_row(row, Some(count.unwrap_or(2).max(2)));
        if last_row == row {
            return;
        }
        let start = self.text.line_start_offset(row);
        let end = self.text.line_end_offset(last_row);
        let source = self.text.slice(start..end).to_string();

        let mut joined = String::with_capacity(source.len());
        let mut cursor = start;
        for (index, line) in source.split('\n').enumerate() {
            if index == 0 {
                joined.push_str(line);
                continue;
            }
            let trimmed = line.trim_start_matches([' ', '\t']);
            if trimmed.is_empty() {
                continue;
            }
            cursor = start + joined.len();
            if !(joined.is_empty() || joined.ends_with([' ', '\t'])) {
                joined.push(' ');
            }
            joined.push_str(trimmed);
        }
        self.replace_range(start..end, &joined, false, cx);
        self.vim_set_mode_and_cursor(VimMode::Normal, cursor, cx);
    }

    fn vim_replace(&mut self, character: char, count: Option<usize>, cx: &mut Context<Self>) {
        let (range, visual) = match self.vim_selection_range() {
            Some(range) => (range, true),
            None => {
                let cursor = self.cursor();
                let bound = self.text.line_end_offset(self.vim_row(cursor));
                let mut end = cursor;
                for _ in 0..count.unwrap_or(1) {
                    if end >= bound {
                        return;
                    }
                    end = self.next_boundary(end);
                }
                (cursor..end, false)
            }
        };
        if range.is_empty() {
            return;
        }
        let replacement: String = self
            .text
            .slice(range.clone())
            .chars()
            .map(|existing| if existing == '\n' { '\n' } else { character })
            .collect();
        let cursor = if visual {
            range.start
        } else {
            range.start + replacement.len() - character.len_utf8()
        };
        self.replace_range(range, &replacement, false, cx);
        self.vim_set_mode_and_cursor(VimMode::Normal, cursor, cx);
    }

    fn vim_toggle_case(&mut self, count: Option<usize>, cx: &mut Context<Self>) {
        let (range, visual) = match self.vim_selection_range() {
            Some(range) => (range, true),
            None => {
                let cursor = self.cursor();
                let bound = self.text.line_end_offset(self.vim_row(cursor));
                let mut end = cursor;
                for _ in 0..count.unwrap_or(1) {
                    if end >= bound {
                        break;
                    }
                    end = self.next_boundary(end);
                }
                (cursor..end, false)
            }
        };
        if range.is_empty() {
            return;
        }
        let flipped: String = self
            .text
            .slice(range.clone())
            .chars()
            .flat_map(|character| {
                if character.is_lowercase() {
                    character.to_uppercase().collect::<Vec<_>>()
                } else if character.is_uppercase() {
                    character.to_lowercase().collect()
                } else {
                    vec![character]
                }
            })
            .collect();
        let cursor = if visual {
            range.start
        } else {
            range.start + flipped.len()
        };
        self.replace_range(range, &flipped, false, cx);
        self.vim_set_mode_and_cursor(VimMode::Normal, cursor, cx);
    }

    fn vim_indent(&mut self, outdent: bool, count: Option<usize>, cx: &mut Context<Self>) {
        let (first_row, last_row) = match self.vim_visual_ends() {
            Some((start, end)) => (self.vim_row(start), self.vim_row(end)),
            None => {
                let row = self.vim_row(self.cursor());
                (row, self.vim_last_row(row, count))
            }
        };
        let start = self.text.line_start_offset(first_row);
        let end = self.text.line_end_offset(last_row);
        self.selected_range = (start..end).into();
        self.selection_reversed = false;
        if outdent {
            self.outdent_selection(true, cx);
        } else {
            self.indent_selection(true, cx);
        }
        let cursor = motion::first_non_blank(&self.text, first_row);
        self.vim_set_mode_and_cursor(VimMode::Normal, cursor, cx);
    }

    fn vim_paste(&mut self, before: bool, count: Option<usize>, cx: &mut Context<Self>) {
        let Some((text, linewise)) = self.vim_register(cx) else {
            return;
        };
        if text.is_empty() {
            return;
        }
        let text = text.repeat(count.unwrap_or(1).max(1));

        if let Some(range) = self.vim_selection_range() {
            let cursor = range.start;
            self.replace_range(range, &text, false, cx);
            let end = cursor + text.len();
            let cursor = self.previous_boundary(end).max(cursor);
            self.vim_set_mode_and_cursor(VimMode::Normal, cursor, cx);
            return;
        }

        let row = self.vim_row(self.cursor());
        if linewise {
            let mut payload = text;
            if !payload.ends_with('\n') {
                payload.push('\n');
            }
            let line_end = self.text.line_end_offset(row);
            let (at, payload) = if before {
                (self.text.line_start_offset(row), payload)
            } else if line_end < self.text.len() {
                (self.next_boundary(line_end), payload)
            } else {
                let mut trailing = String::from("\n");
                trailing.push_str(payload.trim_end_matches('\n'));
                (line_end, trailing)
            };
            let first = at + usize::from(payload.starts_with('\n'));
            self.replace_range(at..at, &payload, false, cx);
            let cursor = motion::first_non_blank(&self.text, self.vim_row(first));
            self.vim_set_mode_and_cursor(VimMode::Normal, cursor, cx);
            return;
        }

        let cursor = self.cursor();
        let at = if before {
            cursor
        } else {
            self.next_boundary(cursor)
                .min(self.text.line_end_offset(row))
        };
        self.replace_range(at..at, &text, false, cx);
        let cursor = self.previous_boundary(at + text.len()).max(at);
        self.vim_set_mode_and_cursor(VimMode::Normal, cursor, cx);
    }

    fn vim_enter_visual(&mut self, kind: VisualKind, cx: &mut Context<Self>) {
        let Some(mode) = self.vim_mode() else {
            return;
        };
        let target = match (mode, kind) {
            (VimMode::Visual, VisualKind::Char) | (VimMode::VisualLine, VisualKind::Line) => {
                VimMode::Normal
            }
            (_, VisualKind::Char) => VimMode::Visual,
            (_, VisualKind::Line) => VimMode::VisualLine,
        };
        if target == VimMode::Normal {
            self.vim_set_mode_and_cursor(VimMode::Normal, self.cursor(), cx);
            return;
        }
        if !mode.is_visual() {
            let cursor = self.cursor();
            if let Some(vim) = self.vim.as_mut() {
                vim.visual_anchor = cursor;
            }
        }
        self.vim_set_mode(target);
        self.vim_move(self.cursor(), None, cx);
    }

    fn vim_swap_visual_ends(&mut self, cx: &mut Context<Self>) {
        let cursor = self.cursor();
        let Some(anchor) = self.vim.as_mut().map(|vim| {
            let anchor = vim.visual_anchor;
            vim.visual_anchor = cursor;
            anchor
        }) else {
            return;
        };
        self.vim_move(anchor, None, cx);
    }

    fn vim_scroll_to(&mut self, align: ScrollAlign, cx: &mut Context<Self>) {
        let Some(layout) = self.editor_layout.as_ref() else {
            return;
        };
        let caret = layout.position_for_offset(self.cursor()).y;
        let viewport = layout.text_bounds.size.height;
        let line_height = layout.line_height;
        let top = match align {
            ScrollAlign::Center => (viewport - line_height) / 2.0,
            ScrollAlign::Top => gpui::Pixels::ZERO,
            ScrollAlign::Bottom => viewport - line_height,
        };
        self.scroll.y = top - caret;
        self.follow_cursor = false;
        cx.notify();
    }

    fn vim_scroll_rows(&mut self, rows: isize) {
        let Some(layout) = self.editor_layout.as_ref() else {
            return;
        };
        self.scroll.y -= layout.line_height * rows as f32;
    }

    fn vim_leave_insert(&mut self, cx: &mut Context<Self>) {
        self.break_typing_group();
        self.vim_set_mode(VimMode::Normal);
        let cursor = self.cursor();
        let start = self.text.line_start_offset(self.vim_row(cursor));
        let target = self.previous_boundary(cursor).max(start);
        self.move_to(target, cx);
    }
}

impl CodeEditorState {
    fn vim_set_mode(&mut self, mode: VimMode) {
        if let Some(vim) = self.vim.as_mut() {
            vim.mode = mode;
        }
    }

    fn vim_set_mode_and_cursor(&mut self, mode: VimMode, offset: usize, cx: &mut Context<Self>) {
        self.vim_set_mode(mode);
        let offset = self.vim_clamp(offset);
        self.move_to(offset, cx);
    }

    fn vim_move(&mut self, offset: usize, goal_column: Option<usize>, cx: &mut Context<Self>) {
        let offset = self.vim_clamp(offset);
        if self.vim_mode().is_some_and(VimMode::is_visual) {
            let anchor = self.vim_clamp(self.vim.as_ref().map_or(offset, |vim| vim.visual_anchor));
            let (range, reversed) = if offset >= anchor {
                (anchor..offset, false)
            } else {
                (offset..anchor, true)
            };
            self.selected_range = range.into();
            self.selection_reversed = reversed;
            self.follow_cursor = true;
            self.pause_blink_cursor(cx);
            cx.notify();
        } else {
            self.move_to_preserving_column(offset, cx);
        }
        self.preferred_column = goal_column;
    }

    fn vim_clamp(&self, offset: usize) -> usize {
        let offset = self
            .text
            .clip_offset(offset.min(self.text.len()), Bias::Left);
        if !self.vim_block_cursor() {
            return offset;
        }
        let row = self.vim_row(offset);
        let end = self.text.line_end_offset(row);
        if offset < end {
            return offset;
        }
        self.previous_boundary(end)
            .max(self.text.line_start_offset(row))
    }

    fn vim_clamp_range(&self, range: Range<usize>) -> Range<usize> {
        let start = self
            .text
            .clip_offset(range.start.min(self.text.len()), Bias::Left);
        let end = self
            .text
            .clip_offset(range.end.min(self.text.len()), Bias::Right);
        start.min(end)..start.max(end)
    }

    fn vim_row(&self, offset: usize) -> usize {
        self.text.offset_to_point(offset).row
    }

    fn vim_last_row(&self, row: usize, count: Option<usize>) -> usize {
        let last = self.text.lines_len().saturating_sub(1);
        row.saturating_add(count.unwrap_or(1).max(1) - 1).min(last)
    }

    fn vim_indent_text(&self, row: usize) -> String {
        self.text
            .slice_line(row)
            .chars()
            .take_while(|character| matches!(character, ' ' | '\t'))
            .collect()
    }

    fn vim_motion_context(&self) -> MotionContext {
        MotionContext {
            viewport_rows: usize::try_from(self.viewport_rows()).unwrap_or(1),
            goal_column: self.preferred_column,
            last_find: self.vim.as_ref().and_then(|vim| vim.last_find),
        }
    }

    fn vim_remember_find(&mut self, motion: Motion) {
        if let (Motion::Find(find), Some(vim)) = (motion, self.vim.as_mut()) {
            vim.last_find = Some(find);
        }
    }

    fn vim_visual_ends(&self) -> Option<(usize, usize)> {
        let vim = self.vim.as_ref()?;
        if !vim.mode.is_visual() {
            return None;
        }
        let cursor = self.cursor();
        let anchor = vim.visual_anchor.min(self.text.len());
        Some((cursor.min(anchor), cursor.max(anchor)))
    }

    fn vim_selection_span(&self) -> Option<OperatorSpan> {
        let (start, end) = self.vim_visual_ends()?;
        Some(if self.vim_mode() == Some(VimMode::VisualLine) {
            OperatorSpan::Linewise {
                first_row: self.vim_row(start),
                last_row: self.vim_row(end),
            }
        } else {
            OperatorSpan::Charwise(start..self.next_boundary(end))
        })
    }

    fn vim_selection_range(&self) -> Option<Range<usize>> {
        match self.vim_selection_span()? {
            OperatorSpan::Charwise(range) => Some(range),
            OperatorSpan::Linewise {
                first_row,
                last_row,
            } => Some(motion::line_span(&self.text, first_row, last_row).content),
        }
    }

    fn vim_store(&mut self, text: String, linewise: bool, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
        if let Some(vim) = self.vim.as_mut() {
            vim.register = Register { text, linewise };
        }
    }

    fn vim_register(&self, cx: &mut Context<Self>) -> Option<(String, bool)> {
        let register = self.vim.as_ref().map(|vim| vim.register.clone())?;
        match cx.read_from_clipboard().and_then(|item| item.text()) {
            Some(text) if text == register.text => Some((text, register.linewise)),
            Some(text) => Some((text, false)),
            None if register.text.is_empty() => None,
            None => Some((register.text, register.linewise)),
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Entity, EntityInputHandler as _, VisualTestContext};

    use super::*;

    fn editor(cx: &mut gpui::TestAppContext) -> (Entity<CodeEditorState>, &mut VisualTestContext) {
        cx.update(crate::init);
        let (editor, cx) = cx.add_window_view(CodeEditorState::new);
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| editor.set_vim_enabled(true, cx));
        });
        (editor, cx)
    }

    fn keys(editor: &Entity<CodeEditorState>, keys: &str, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                for character in keys.chars() {
                    editor.replace_text_in_range(None, &character.to_string(), window, cx);
                }
            });
        });
    }

    fn escape(editor: &Entity<CodeEditorState>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.vim_key(Key::Escape, window, cx);
            });
        });
    }

    fn seed(
        editor: &Entity<CodeEditorState>,
        text: &str,
        cursor: usize,
        cx: &mut VisualTestContext,
    ) {
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_value(text, window, cx);
                editor.set_selected_range(cursor..cursor, cx);
            });
        });
    }

    fn value(editor: &Entity<CodeEditorState>, cx: &mut VisualTestContext) -> String {
        editor.read_with(cx, |editor, _| editor.value().to_string())
    }

    fn cursor(editor: &Entity<CodeEditorState>, cx: &mut VisualTestContext) -> usize {
        editor.read_with(cx, |editor, _| editor.cursor())
    }

    fn mode(editor: &Entity<CodeEditorState>, cx: &mut VisualTestContext) -> Option<VimMode> {
        editor.read_with(cx, |editor, _| editor.vim_mode())
    }

    #[gpui::test]
    fn normal_mode_swallows_printable_keys(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = editor(cx);
        seed(&editor, "abc", 0, cx);
        keys(&editor, "qzQ", cx);
        assert_eq!(value(&editor, cx), "abc", "normal mode never types");
    }

    #[gpui::test]
    fn insert_mode_types_and_escape_clamps_the_cursor(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = editor(cx);
        seed(&editor, "ab\n", 0, cx);
        keys(&editor, "i", cx);
        assert_eq!(mode(&editor, cx), Some(VimMode::Insert));
        keys(&editor, "xy", cx);
        assert_eq!(value(&editor, cx), "xyab\n");
        assert_eq!(cursor(&editor, cx), 2);
        escape(&editor, cx);
        assert_eq!(mode(&editor, cx), Some(VimMode::Normal));
        assert_eq!(cursor(&editor, cx), 1, "escape steps left");

        seed(&editor, "ab", 0, cx);
        keys(&editor, "i", cx);
        escape(&editor, cx);
        assert_eq!(cursor(&editor, cx), 0, "never before the line start");
    }

    #[gpui::test]
    fn delete_word_spares_the_newline_at_the_end_of_a_line(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = editor(cx);
        seed(&editor, "alpha beta\ngamma", 6, cx);
        keys(&editor, "dw", cx);
        assert_eq!(value(&editor, cx), "alpha \ngamma");
    }

    #[gpui::test]
    fn change_word_stops_at_the_word_end(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = editor(cx);
        seed(&editor, "alpha beta", 0, cx);
        keys(&editor, "cw", cx);
        assert_eq!(value(&editor, cx), " beta");
        assert_eq!(mode(&editor, cx), Some(VimMode::Insert));
        keys(&editor, "one", cx);
        assert_eq!(value(&editor, cx), "one beta");
    }

    #[gpui::test]
    fn delete_line_handles_the_last_line(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = editor(cx);
        seed(&editor, "one\ntwo\nthree", 9, cx);
        keys(&editor, "dd", cx);
        assert_eq!(value(&editor, cx), "one\ntwo");
        assert_eq!(cursor(&editor, cx), 4, "the cursor falls back a line");

        seed(&editor, "only", 1, cx);
        keys(&editor, "dd", cx);
        assert_eq!(value(&editor, cx), "");
    }

    #[gpui::test]
    fn counts_compose_across_operator_and_motion(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = editor(cx);
        seed(&editor, "a b c d e f g", 0, cx);
        keys(&editor, "2d3w", cx);
        assert_eq!(value(&editor, cx), "g");

        seed(&editor, "1\n2\n3\n4\n5", 0, cx);
        keys(&editor, "3dd", cx);
        assert_eq!(value(&editor, cx), "4\n5");
    }

    #[gpui::test]
    fn join_takes_a_count_and_inserts_one_space(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = editor(cx);
        seed(&editor, "one\n  two\nthree\nfour", 0, cx);
        keys(&editor, "3J", cx);
        assert_eq!(value(&editor, cx), "one two three\nfour");
        assert_eq!(cursor(&editor, cx), 7, "the cursor sits on the last join");
    }

    #[gpui::test]
    fn text_objects_and_find_drive_operators(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = editor(cx);
        seed(&editor, "say \"hello there\" now", 8, cx);
        keys(&editor, "ci\"", cx);
        assert_eq!(value(&editor, cx), "say \"\" now");
        escape(&editor, cx);

        seed(&editor, "a,b,c", 0, cx);
        keys(&editor, "df,", cx);
        assert_eq!(value(&editor, cx), "b,c", "f is inclusive");
    }

    #[gpui::test]
    fn linewise_and_charwise_pastes_land_differently(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = editor(cx);
        seed(&editor, "one\ntwo\nthree", 0, cx);
        keys(&editor, "yyjp", cx);
        assert_eq!(value(&editor, cx), "one\ntwo\none\nthree");
        assert_eq!(cursor(&editor, cx), 8, "linewise paste lands on the line");

        seed(&editor, "ab", 0, cx);
        keys(&editor, "ylp", cx);
        assert_eq!(value(&editor, cx), "aab", "charwise paste lands after");
        assert_eq!(cursor(&editor, cx), 1);
    }

    #[gpui::test]
    fn visual_mode_selects_inclusively_and_deletes(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = editor(cx);
        seed(&editor, "alpha beta", 0, cx);
        keys(&editor, "vll", cx);
        assert_eq!(mode(&editor, cx), Some(VimMode::Visual));
        keys(&editor, "d", cx);
        assert_eq!(value(&editor, cx), "ha beta", "three characters go");
        assert_eq!(mode(&editor, cx), Some(VimMode::Normal));
    }

    #[gpui::test]
    fn visual_line_mode_deletes_whole_lines(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = editor(cx);
        seed(&editor, "one\ntwo\nthree", 5, cx);
        keys(&editor, "Vd", cx);
        assert_eq!(value(&editor, cx), "one\nthree");
    }

    #[gpui::test]
    fn visual_mode_swaps_ends_with_o(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = editor(cx);
        seed(&editor, "abcdef", 2, cx);
        keys(&editor, "vll", cx);
        assert_eq!(cursor(&editor, cx), 4);
        keys(&editor, "o", cx);
        assert_eq!(cursor(&editor, cx), 2, "the cursor jumps to the anchor");
        keys(&editor, "d", cx);
        assert_eq!(value(&editor, cx), "abf");
    }

    #[gpui::test]
    fn replace_and_toggle_case_respect_counts(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = editor(cx);
        seed(&editor, "abcd", 0, cx);
        keys(&editor, "3rx", cx);
        assert_eq!(value(&editor, cx), "xxxd");
        assert_eq!(cursor(&editor, cx), 2);

        seed(&editor, "abc", 0, cx);
        keys(&editor, "9rx", cx);
        assert_eq!(
            value(&editor, cx),
            "abc",
            "a count past the line is refused"
        );

        seed(&editor, "abc", 0, cx);
        keys(&editor, "2~", cx);
        assert_eq!(value(&editor, cx), "ABc");
        assert_eq!(cursor(&editor, cx), 2, "~ advances past what it flipped");
    }

    #[gpui::test]
    fn open_line_keeps_the_indentation(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = editor(cx);
        seed(&editor, "  body", 3, cx);
        keys(&editor, "o", cx);
        assert_eq!(value(&editor, cx), "  body\n  ");
        assert_eq!(mode(&editor, cx), Some(VimMode::Insert));

        seed(&editor, "  body", 3, cx);
        keys(&editor, "O", cx);
        assert_eq!(value(&editor, cx), "  \n  body");
        assert_eq!(cursor(&editor, cx), 2);
    }

    #[gpui::test]
    fn undo_returns_the_buffer_and_the_cursor(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = editor(cx);
        seed(&editor, "one two", 0, cx);
        keys(&editor, "dw", cx);
        assert_eq!(value(&editor, cx), "two");
        keys(&editor, "u", cx);
        assert_eq!(value(&editor, cx), "one two");
    }

    #[gpui::test]
    fn indent_shifts_whole_lines_by_a_count(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = editor(cx);
        seed(&editor, "a\nb\nc", 0, cx);
        keys(&editor, "2>>", cx);
        assert_eq!(value(&editor, cx), "  a\n  b\nc");
        keys(&editor, "<<", cx);
        assert_eq!(value(&editor, cx), "a\n  b\nc");
    }

    #[gpui::test]
    fn the_cursor_never_sits_on_a_line_break(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = editor(cx);
        seed(&editor, "abc\ndef", 0, cx);
        keys(&editor, "$", cx);
        assert_eq!(cursor(&editor, cx), 2, "$ lands on the last character");
        keys(&editor, "lll", cx);
        assert_eq!(cursor(&editor, cx), 2, "l cannot leave the line");
        keys(&editor, "x", cx);
        assert_eq!(value(&editor, cx), "ab\ndef");
        assert_eq!(cursor(&editor, cx), 1, "deleting at the end steps back");
    }

    #[gpui::test]
    fn substitute_leaves_the_cursor_where_the_characters_were(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = editor(cx);
        seed(&editor, "ab", 1, cx);
        keys(&editor, "s", cx);
        assert_eq!(value(&editor, cx), "a");
        assert_eq!(mode(&editor, cx), Some(VimMode::Insert));
        keys(&editor, "XY", cx);
        assert_eq!(value(&editor, cx), "aXY", "insertion appends, not prepends");
    }

    #[gpui::test]
    fn visual_mode_grows_to_a_text_object(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = editor(cx);
        seed(&editor, "alpha beta gamma", 8, cx);
        keys(&editor, "viwd", cx);
        assert_eq!(value(&editor, cx), "alpha  gamma");
    }

    #[gpui::test]
    fn a_foreign_clipboard_pastes_charwise(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = editor(cx);
        seed(&editor, "one\ntwo", 0, cx);
        keys(&editor, "yy", cx);
        cx.update(|_, cx| {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string("X".into()));
        });
        keys(&editor, "p", cx);
        assert_eq!(
            value(&editor, cx),
            "oXne\ntwo",
            "someone else's clipboard is not linewise"
        );
    }

    #[gpui::test]
    fn escape_in_normal_mode_still_reaches_the_pane(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = editor(cx);
        seed(&editor, "abc", 0, cx);
        let consumed = cx.update(|window, cx| {
            editor.update(cx, |editor, cx| editor.vim_key(Key::Escape, window, cx))
        });
        assert!(!consumed, "a bare escape propagates");

        let consumed = cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.vim_key(Key::Char('2'), window, cx);
                editor.vim_key(Key::Escape, window, cx)
            })
        });
        assert!(consumed, "an escape that cancels a count does not");
    }

    #[gpui::test]
    fn loading_new_content_returns_to_normal_mode(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = editor(cx);
        seed(&editor, "abc", 0, cx);
        keys(&editor, "i", cx);
        assert_eq!(mode(&editor, cx), Some(VimMode::Insert));
        seed(&editor, "different", 0, cx);
        assert_eq!(mode(&editor, cx), Some(VimMode::Normal));
    }

    #[gpui::test]
    fn disabling_vim_restores_plain_typing(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = editor(cx);
        seed(&editor, "", 0, cx);
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| editor.set_vim_enabled(false, cx));
        });
        assert_eq!(mode(&editor, cx), None);
        keys(&editor, "hi", cx);
        assert_eq!(value(&editor, cx), "hi");
    }
}
