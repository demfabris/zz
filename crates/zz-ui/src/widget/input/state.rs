//! [`InputState`]: everything a text field remembers between frames.

// Fields prefixed with `_` hold a `Subscription` or `Task` only for its `Drop`,
// so they are assigned and never read.
#![allow(clippy::used_underscore_binding)]

use std::{borrow::Cow, ops::Range, rc::Rc, sync::Arc};

use gpui::{
    App, AppContext as _, Bounds, ClipboardEntry, ClipboardItem, Context, DismissEvent, Entity,
    EntityInputHandler, EventEmitter, FocusHandle, Focusable, Image, IntoElement, KeyDownEvent,
    MouseButton, MouseDownEvent, ParentElement as _, Pixels, Point, Render, ScrollWheelEvent,
    SharedString, Styled as _, Subscription, Task, TextAlign, UTF16Selection, Window, anchored,
    deferred, div, point, prelude::FluentBuilder as _, px,
};
use unicode_segmentation::UnicodeSegmentation as _;

use crate::{
    Size,
    menu::{PopupMenu, PopupMenuItem},
};

use super::{
    actions,
    element::{LastLayout, TextElement},
    history::{EditKind, History, Snapshot},
    number,
};

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(super) const CURSOR_WIDTH: Pixels = px(1.5);
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
pub(super) const CURSOR_WIDTH: Pixels = px(2.);

pub(super) const NEWLINE_SELECTION_WIDTH: Pixels = px(4.);

/// What the app hears from a field.
#[derive(Clone, Debug)]
pub enum InputEvent {
    /// The value changed. Not emitted mid-IME-composition, nor by
    /// [`InputState::set_value`].
    Change,
    /// Enter was pressed. `shift` distinguishes the newline chord in a
    /// multi-line field that submits on Enter.
    PressEnter {
        /// Whether Shift was held.
        shift: bool,
    },
    /// The field took keyboard focus.
    Focus,
    /// The field lost keyboard focus.
    Blur,
    /// The clipboard held images and no text. The field forwards them for a view
    /// that can carry images; one that cannot ignores the event.
    PasteImages(Vec<Arc<Image>>),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum InputMode {
    #[default]
    SingleLine,
    AutoGrow {
        min_rows: usize,
        max_rows: usize,
    },
}

impl InputMode {
    pub(super) const fn is_multi_line(self) -> bool {
        matches!(self, Self::AutoGrow { .. })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DragGranularity {
    Character,
    Word,
    Line,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum StepDirection {
    Increment,
    Decrement,
}

type ValidateFn = Rc<dyn Fn(&str, &mut Context<InputState>) -> bool>;

/// The editing state behind an [`Input`](super::Input). Build one per field and
/// keep it in your view; the element is rebuilt every frame from it.
pub struct InputState {
    focus_handle: FocusHandle,

    text: SharedString,
    placeholder: SharedString,

    selection: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,

    mode: InputMode,
    submit_on_enter: bool,
    enable_context_menu: bool,

    pub(super) disabled: bool,
    pub(super) loading: bool,
    pub(super) size: Size,
    pub(super) align: TextAlign,
    pub(super) masked: bool,

    validate: Option<ValidateFn>,
    numeric: bool,
    step: Option<f64>,
    minimum: Option<f64>,
    maximum: Option<f64>,

    history: History,

    selecting: bool,
    drag_granularity: DragGranularity,
    drag_anchor: Range<usize>,
    preferred_x: Option<Pixels>,

    pub(super) last_layout: Option<LastLayout>,
    pub(super) scroll: Point<Pixels>,
    pub(super) measured_rows: usize,
    pub(super) follow_cursor: bool,
    pub(super) reset_scroll: bool,

    blink: Entity<BlinkCursor>,
    context_menu: Option<Entity<PopupMenu>>,
    context_menu_position: Point<Pixels>,
    _context_menu_subscription: Option<Subscription>,
    menu_focus_round_trip: bool,

    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<InputEvent> for InputState {}

impl Focusable for InputState {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl InputState {
    /// A new single-line field.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle().tab_stop(true);
        let blink = cx.new(|_| BlinkCursor::new());

        let subscriptions = vec![
            cx.observe(&blink, |_, _, cx| cx.notify()),
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() && this.focus_handle.is_focused(window) {
                    this.blink.update(cx, BlinkCursor::start);
                }
            }),
            cx.on_focus(&focus_handle, window, Self::on_focus),
            cx.on_blur(&focus_handle, window, Self::on_blur),
        ];

        Self {
            focus_handle,
            text: SharedString::default(),
            placeholder: SharedString::default(),
            selection: 0..0,
            selection_reversed: false,
            marked_range: None,
            mode: InputMode::default(),
            submit_on_enter: false,
            enable_context_menu: false,
            disabled: false,
            loading: false,
            size: Size::default(),
            align: TextAlign::Left,
            masked: false,
            validate: None,
            numeric: false,
            step: None,
            minimum: None,
            maximum: None,
            history: History::default(),
            selecting: false,
            drag_granularity: DragGranularity::Character,
            drag_anchor: 0..0,
            preferred_x: None,
            last_layout: None,
            scroll: Point::default(),
            measured_rows: 1,
            follow_cursor: false,
            reset_scroll: false,
            blink,
            context_menu: None,
            context_menu_position: Point::default(),
            _context_menu_subscription: None,
            menu_focus_round_trip: false,
            _subscriptions: subscriptions,
        }
    }

    /// Grow with the content, between `min_rows` and `max_rows` text rows.
    /// This is also what makes the field multi-line.
    #[must_use]
    pub fn auto_grow(mut self, min_rows: usize, max_rows: usize) -> Self {
        let min_rows = min_rows.max(1);
        self.mode = InputMode::AutoGrow {
            min_rows,
            max_rows: max_rows.max(min_rows),
        };
        self.measured_rows = min_rows;
        self
    }

    /// In a multi-line field, treat plain Enter as submit and `shift-enter` as
    /// "insert a newline". Single-line fields always submit on Enter.
    #[must_use]
    pub fn submit_on_enter(mut self, submit: bool) -> Self {
        self.submit_on_enter = submit;
        self
    }

    /// Offer a copy/cut/paste/select-all menu on right click.
    #[must_use]
    pub fn context_menu(mut self, enable: bool) -> Self {
        self.enable_context_menu = enable;
        self
    }

    /// Grey text shown while the value is empty.
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// The value the field starts with. Not recorded in the undo history.
    #[must_use]
    pub fn default_value(mut self, value: impl Into<SharedString>) -> Self {
        self.text = value.into();
        let end = self.text.len();
        self.selection = end..end;
        self
    }

    /// Reject any edit that would make the value fail this predicate. An
    /// already-invalid value stays editable, so the user can type their way out
    /// of it.
    #[must_use]
    pub fn validate(mut self, f: impl Fn(&str, &mut Context<Self>) -> bool + 'static) -> Self {
        self.validate = Some(Rc::new(f));
        self
    }

    /// Amount a [`NumberInput`](super::NumberInput) stepper moves the value.
    /// Setting it also restricts typing to number-shaped text.
    #[must_use]
    pub fn step(mut self, step: f64) -> Self {
        self.numeric = true;
        self.step = Some(step);
        self
    }

    /// Lower bound for the steppers. See [`Self::step`].
    #[must_use]
    pub fn min(mut self, min: f64) -> Self {
        self.numeric = true;
        self.minimum = Some(min);
        self
    }

    /// Upper bound for the steppers. See [`Self::step`].
    #[must_use]
    pub fn max(mut self, max: f64) -> Self {
        self.numeric = true;
        self.maximum = Some(max);
        self
    }
}

impl InputState {
    #[must_use]
    pub fn value(&self) -> SharedString {
        self.text.clone()
    }

    /// Replace the value without touching the undo history and without emitting
    /// [`InputEvent::Change`], for a view syncing its own field. The caret lands
    /// at the end, the view scrolls back to the start.
    pub fn set_value(
        &mut self,
        value: impl Into<SharedString>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value: SharedString = value.into();
        if value == self.text {
            return;
        }
        self.text = value;
        let end = self.text.len();
        self.selection = end..end;
        self.selection_reversed = false;
        self.marked_range = None;
        self.preferred_x = None;
        self.history.clear();
        self.scroll = Point::default();
        self.reset_scroll = true;
        self.follow_cursor = false;
        cx.notify();
    }

    /// Show or hide the trailing spinner.
    pub fn set_loading(&mut self, loading: bool, _: &mut Window, cx: &mut Context<Self>) {
        if self.loading == loading {
            return;
        }
        self.loading = loading;
        cx.notify();
    }

    /// Byte offset of the caret.
    #[must_use]
    pub fn cursor(&self) -> usize {
        if self.selection_reversed {
            self.selection.start
        } else {
            self.selection.end
        }
    }

    /// Byte range of the selection.
    #[must_use]
    pub fn selected_range(&self) -> Range<usize> {
        self.selection.clone()
    }

    /// Move the caret / set the selection. Offsets are clamped into the value
    /// and onto character boundaries.
    pub fn set_selected_range(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        self.selection = self.clamp_range(range);
        self.selection_reversed = false;
        self.preferred_x = None;
        self.history.break_group();
        self.follow_cursor = true;
        self.pause_blink(cx);
        cx.notify();
    }

    /// Insert `text` at the caret, replacing nothing. The caret ends up after
    /// the inserted text.
    pub fn insert(
        &mut self,
        text: impl Into<SharedString>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cursor = self.cursor();
        let text: SharedString = text.into();
        self.edit(cursor..cursor, &text, EditKind::Other, cx);
    }

    /// Replace the selection with `text`. The caret ends up after it.
    pub fn replace(
        &mut self,
        text: impl Into<SharedString>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.selection.clone();
        let text: SharedString = text.into();
        self.edit(range, &text, EditKind::Other, cx);
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_handle.focus(window, cx);
    }
}

impl InputState {
    pub(super) fn mode(&self) -> InputMode {
        self.mode
    }

    pub(super) fn text(&self) -> &SharedString {
        &self.text
    }

    pub(super) fn placeholder_text(&self) -> &SharedString {
        &self.placeholder
    }

    pub(super) fn selection(&self) -> Range<usize> {
        self.selection.clone()
    }

    pub(super) fn focus_handle_ref(&self) -> &FocusHandle {
        &self.focus_handle
    }

    pub(super) fn show_cursor(&self, window: &Window, cx: &App) -> bool {
        (self.focus_handle.is_focused(window) || self.context_menu.is_some())
            && !self.disabled
            && self.blink.read(cx).visible()
            && window.is_window_active()
    }

    pub(super) fn mark_numeric(&mut self, _: &mut Context<Self>) {
        self.numeric = true;
    }

    pub(super) fn clear(&mut self, cx: &mut Context<Self>) {
        let all = 0..self.text.len();
        self.edit(all, "", EditKind::Other, cx);
    }
}

impl InputState {
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.text.clone(),
            selection: self.selection.clone(),
            reversed: self.selection_reversed,
        }
    }

    fn restore(&mut self, snapshot: Snapshot, cx: &mut Context<Self>) {
        self.text = snapshot.text;
        self.selection = self.clamp_range(snapshot.selection);
        self.selection_reversed = snapshot.reversed;
        self.marked_range = None;
        self.preferred_x = None;
        self.follow_cursor = true;
        self.pause_blink(cx);
        cx.emit(InputEvent::Change);
        cx.notify();
    }

    fn sanitize<'a>(&self, text: &'a str) -> Cow<'a, str> {
        let multi_line = self.mode.is_multi_line();
        let keep = |c: char| !c.is_control() || (multi_line && (c == '\n' || c == '\t'));

        if text.chars().all(|c| keep(c) && c != '\r') {
            return Cow::Borrowed(text);
        }

        let mut out = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\r' {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                if multi_line {
                    out.push('\n');
                }
            } else if keep(c) {
                out.push(c);
            }
        }
        Cow::Owned(out)
    }

    fn is_valid(&self, candidate: &str, cx: &mut Context<Self>) -> bool {
        if candidate.is_empty() {
            return true;
        }
        if self.numeric && !number::is_number_like(candidate) {
            return false;
        }
        let Some(validate) = self.validate.clone() else {
            return true;
        };
        validate(candidate, cx)
    }

    fn edit(
        &mut self,
        range: Range<usize>,
        new_text: &str,
        kind: EditKind,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.disabled {
            return false;
        }

        let range = self.clamp_range(range);
        let inserted = self.sanitize(new_text);
        if range.is_empty() && inserted.is_empty() {
            return false;
        }

        let candidate = {
            let text: &str = &self.text;
            let mut candidate = String::with_capacity(text.len() + inserted.len());
            candidate.push_str(&text[..range.start]);
            candidate.push_str(&inserted);
            candidate.push_str(&text[range.end..]);
            candidate
        };

        if !self.is_valid(&candidate, cx) {
            let current = self.text.clone();
            if self.is_valid(&current, cx) {
                return false;
            }
        }

        let before = self.snapshot();
        let cursor = range.start + inserted.len();
        self.text = candidate.into();
        self.selection = cursor..cursor;
        self.selection_reversed = false;
        self.marked_range = None;
        self.preferred_x = None;
        self.history.push(before, kind);
        self.follow_cursor = true;
        self.pause_blink(cx);
        self.hide_context_menu(cx);
        cx.emit(InputEvent::Change);
        cx.notify();
        true
    }

    pub(super) fn replace_all(&mut self, value: &str, cx: &mut Context<Self>) -> bool {
        let all = 0..self.text.len();
        self.edit(all, value, EditKind::Other, cx)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CharClass {
    Whitespace,
    Word,
    Punctuation,
}

fn class_of(c: char) -> CharClass {
    if c.is_whitespace() {
        CharClass::Whitespace
    } else if c.is_alphanumeric() || c == '_' {
        CharClass::Word
    } else {
        CharClass::Punctuation
    }
}

fn utf8_offset(text: &str, utf16: usize) -> usize {
    let mut units = 0;
    let mut bytes = 0;
    for c in text.chars() {
        let next = units + c.len_utf16();
        if next > utf16 {
            break;
        }
        units = next;
        bytes += c.len_utf8();
    }
    bytes
}

fn utf16_offset(text: &str, bytes: usize) -> usize {
    let mut units = 0;
    let mut seen = 0;
    for c in text.chars() {
        if seen >= bytes {
            break;
        }
        seen += c.len_utf8();
        units += c.len_utf16();
    }
    units
}

impl InputState {
    fn clamp_offset(&self, offset: usize) -> usize {
        let text: &str = &self.text;
        let mut offset = offset.min(text.len());
        while !text.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }

    fn clamp_range(&self, range: Range<usize>) -> Range<usize> {
        let start = self.clamp_offset(range.start);
        let end = self.clamp_offset(range.end.max(range.start));
        start..end
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        let text: &str = &self.text;
        text[..self.clamp_offset(offset)]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        let text: &str = &self.text;
        let offset = self.clamp_offset(offset);
        text[offset..]
            .grapheme_indices(true)
            .next()
            .map_or(text.len(), |(_, g)| offset + g.len())
    }

    fn previous_word_start(&self, from: usize) -> usize {
        let text: &str = &self.text;
        let from = self.clamp_offset(from);
        let mut offset = from;
        let mut chars = text[..from].char_indices().rev().peekable();

        while let Some(&(i, c)) = chars.peek() {
            if class_of(c) == CharClass::Whitespace {
                offset = i;
                chars.next();
            } else {
                break;
            }
        }
        let Some(&(_, first)) = chars.peek() else {
            return offset;
        };
        let class = class_of(first);
        while let Some(&(i, c)) = chars.peek() {
            if class_of(c) == class {
                offset = i;
                chars.next();
            } else {
                break;
            }
        }
        offset
    }

    fn next_word_end(&self, from: usize) -> usize {
        let text: &str = &self.text;
        let from = self.clamp_offset(from);
        let mut offset = from;
        let mut chars = text[from..].char_indices().peekable();

        while let Some(&(i, c)) = chars.peek() {
            if class_of(c) == CharClass::Whitespace {
                offset = from + i + c.len_utf8();
                chars.next();
            } else {
                break;
            }
        }
        let Some(&(_, first)) = chars.peek() else {
            return offset;
        };
        let class = class_of(first);
        while let Some(&(i, c)) = chars.peek() {
            if class_of(c) == class {
                offset = from + i + c.len_utf8();
                chars.next();
            } else {
                break;
            }
        }
        offset
    }

    fn start_of_line(&self, offset: usize) -> usize {
        let text: &str = &self.text;
        let offset = self.clamp_offset(offset);
        text[..offset].rfind('\n').map_or(0, |i| i + 1)
    }

    fn end_of_line(&self, offset: usize) -> usize {
        let text: &str = &self.text;
        let offset = self.clamp_offset(offset);
        text[offset..].find('\n').map_or(text.len(), |i| offset + i)
    }

    fn word_range_at(&self, offset: usize) -> Range<usize> {
        let text: &str = &self.text;
        let offset = self.clamp_offset(offset);
        let before = text[..offset].chars().next_back().map(class_of);
        let after = text[offset..].chars().next().map(class_of);

        let class = match (before, after) {
            (Some(before), Some(after)) => {
                if after == CharClass::Whitespace && before != CharClass::Whitespace {
                    before
                } else {
                    after
                }
            }
            (Some(class), None) | (None, Some(class)) => class,
            (None, None) => return offset..offset,
        };

        let mut start = offset;
        for (i, c) in text[..offset].char_indices().rev() {
            if class_of(c) == class {
                start = i;
            } else {
                break;
            }
        }
        let mut end = offset;
        for (i, c) in text[offset..].char_indices() {
            if class_of(c) == class {
                end = offset + i + c.len_utf8();
            } else {
                break;
            }
        }
        start..end
    }

    fn line_range_at(&self, offset: usize) -> Range<usize> {
        self.start_of_line(offset)..self.end_of_line(offset)
    }

    fn offset_from_utf16(&self, utf16: usize) -> usize {
        utf8_offset(&self.text, utf16)
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        utf16_offset(&self.text, offset)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }
}

impl InputState {
    fn anchor(&self) -> usize {
        if self.selection_reversed {
            self.selection.end
        } else {
            self.selection.start
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = self.clamp_offset(offset);
        self.selection = offset..offset;
        self.selection_reversed = false;
        self.after_caret_move(cx);
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = self.clamp_offset(offset);
        let anchor = self.anchor();
        if offset < anchor {
            self.selection = offset..anchor;
            self.selection_reversed = true;
        } else {
            self.selection = anchor..offset;
            self.selection_reversed = false;
        }
        self.after_caret_move(cx);
    }

    fn set_selection(&mut self, range: Range<usize>, reversed: bool, cx: &mut Context<Self>) {
        self.selection = self.clamp_range(range);
        self.selection_reversed = reversed;
        self.after_caret_move(cx);
    }

    fn after_caret_move(&mut self, cx: &mut Context<Self>) {
        self.preferred_x = None;
        self.history.break_group();
        self.follow_cursor = true;
        self.pause_blink(cx);
        self.hide_context_menu(cx);
        cx.notify();
    }

    fn move_vertical(&mut self, rows: isize, select: bool, cx: &mut Context<Self>) {
        if !self.mode.is_multi_line() {
            return;
        }
        let Some(layout) = self.last_layout.clone() else {
            return;
        };

        let cursor = self.cursor();
        let current = layout.position_for_offset(cursor);
        let target_x = self.preferred_x.unwrap_or(current.x);
        let target_y = current.y + layout.line_height * rows as f32;

        let offset = if target_y < Pixels::ZERO {
            0
        } else if target_y >= layout.content_size.height {
            self.text.len()
        } else {
            layout.offset_for_local_position(point(target_x, target_y))
        };

        if select {
            self.select_to(offset, cx);
        } else {
            self.move_to(offset, cx);
        }
        self.preferred_x = Some(target_x);
    }
}

impl InputState {
    pub(super) fn backspace(
        &mut self,
        _: &actions::Backspace,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = if self.selection.is_empty() {
            self.previous_boundary(self.selection.start)..self.selection.end
        } else {
            self.selection.clone()
        };
        self.edit(range, "", EditKind::Delete, cx);
    }

    pub(super) fn delete(&mut self, _: &actions::Delete, _: &mut Window, cx: &mut Context<Self>) {
        let range = if self.selection.is_empty() {
            self.selection.start..self.next_boundary(self.selection.end)
        } else {
            self.selection.clone()
        };
        self.edit(range, "", EditKind::Delete, cx);
    }

    pub(super) fn delete_to_previous_word_start(
        &mut self,
        _: &actions::DeleteToPreviousWordStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = if self.selection.is_empty() {
            self.previous_word_start(self.selection.start)..self.selection.end
        } else {
            self.selection.clone()
        };
        self.edit(range, "", EditKind::Delete, cx);
    }

    pub(super) fn delete_to_next_word_end(
        &mut self,
        _: &actions::DeleteToNextWordEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = if self.selection.is_empty() {
            self.selection.start..self.next_word_end(self.selection.end)
        } else {
            self.selection.clone()
        };
        self.edit(range, "", EditKind::Delete, cx);
    }

    pub(super) fn delete_to_beginning_of_line(
        &mut self,
        _: &actions::DeleteToBeginningOfLine,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.start_of_line(self.selection.start)..self.selection.end;
        self.edit(range, "", EditKind::Delete, cx);
    }

    pub(super) fn delete_to_end_of_line(
        &mut self,
        _: &actions::DeleteToEndOfLine,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.selection.start..self.end_of_line(self.selection.end);
        self.edit(range, "", EditKind::Delete, cx);
    }

    pub(super) fn move_left(
        &mut self,
        _: &actions::MoveLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let offset = if self.selection.is_empty() {
            self.previous_boundary(self.selection.start)
        } else {
            self.selection.start
        };
        self.move_to(offset, cx);
    }

    pub(super) fn move_right(
        &mut self,
        _: &actions::MoveRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let offset = if self.selection.is_empty() {
            self.next_boundary(self.selection.end)
        } else {
            self.selection.end
        };
        self.move_to(offset, cx);
    }

    pub(super) fn select_left(
        &mut self,
        _: &actions::SelectLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let offset = self.previous_boundary(self.cursor());
        self.select_to(offset, cx);
    }

    pub(super) fn select_right(
        &mut self,
        _: &actions::SelectRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let offset = self.next_boundary(self.cursor());
        self.select_to(offset, cx);
    }

    pub(super) fn move_up(&mut self, _: &actions::MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(-1, false, cx);
    }

    pub(super) fn move_down(
        &mut self,
        _: &actions::MoveDown,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_vertical(1, false, cx);
    }

    pub(super) fn select_up(
        &mut self,
        _: &actions::SelectUp,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_vertical(-1, true, cx);
    }

    pub(super) fn select_down(
        &mut self,
        _: &actions::SelectDown,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_vertical(1, true, cx);
    }

    pub(super) fn move_home(
        &mut self,
        _: &actions::MoveHome,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let offset = self.start_of_line(self.cursor());
        self.move_to(offset, cx);
    }

    pub(super) fn move_end(
        &mut self,
        _: &actions::MoveEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let offset = self.end_of_line(self.cursor());
        self.move_to(offset, cx);
    }

    pub(super) fn select_to_start_of_line(
        &mut self,
        _: &actions::SelectToStartOfLine,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let offset = self.start_of_line(self.cursor());
        self.select_to(offset, cx);
    }

    pub(super) fn select_to_end_of_line(
        &mut self,
        _: &actions::SelectToEndOfLine,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let offset = self.end_of_line(self.cursor());
        self.select_to(offset, cx);
    }

    pub(super) fn move_to_start(
        &mut self,
        _: &actions::MoveToStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_to(0, cx);
    }

    pub(super) fn move_to_end(
        &mut self,
        _: &actions::MoveToEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let end = self.text.len();
        self.move_to(end, cx);
    }

    pub(super) fn select_to_start(
        &mut self,
        _: &actions::SelectToStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(0, cx);
    }

    pub(super) fn select_to_end(
        &mut self,
        _: &actions::SelectToEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let end = self.text.len();
        self.select_to(end, cx);
    }

    pub(super) fn move_to_previous_word(
        &mut self,
        _: &actions::MoveToPreviousWord,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let offset = self.previous_word_start(self.cursor());
        self.move_to(offset, cx);
    }

    pub(super) fn move_to_next_word(
        &mut self,
        _: &actions::MoveToNextWord,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let offset = self.next_word_end(self.cursor());
        self.move_to(offset, cx);
    }

    pub(super) fn select_to_previous_word_start(
        &mut self,
        _: &actions::SelectToPreviousWordStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let offset = self.previous_word_start(self.cursor());
        self.select_to(offset, cx);
    }

    pub(super) fn select_to_next_word_end(
        &mut self,
        _: &actions::SelectToNextWordEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let offset = self.next_word_end(self.cursor());
        self.select_to(offset, cx);
    }

    pub(super) fn select_all(
        &mut self,
        _: &actions::SelectAll,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let end = self.text.len();
        self.set_selection(0..end, false, cx);
    }

    pub(super) fn copy(&mut self, _: &actions::Copy, _: &mut Window, cx: &mut Context<Self>) {
        self.copy_selection(cx);
    }

    pub(super) fn cut(&mut self, _: &actions::Cut, _: &mut Window, cx: &mut Context<Self>) {
        self.cut_selection(cx);
    }

    pub(super) fn paste(&mut self, _: &actions::Paste, _: &mut Window, cx: &mut Context<Self>) {
        self.paste_clipboard(cx);
    }

    pub(super) fn undo(&mut self, _: &actions::Undo, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let current = self.snapshot();
        if let Some(previous) = self.history.undo(current) {
            self.restore(previous, cx);
        }
    }

    pub(super) fn redo(&mut self, _: &actions::Redo, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let current = self.snapshot();
        if let Some(next) = self.history.redo(current) {
            self.restore(next, cx);
        }
    }

    // `cx.listener(Self::method)` fixes the `&mut self` receiver.
    #[allow(clippy::unused_self)]
    pub(super) fn show_character_palette(
        &mut self,
        _: &actions::ShowCharacterPalette,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    pub(super) fn enter(
        &mut self,
        action: &actions::Enter,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let insert_newline = self.mode.is_multi_line() && (!self.submit_on_enter || action.shift);

        if insert_newline {
            let range = self.selection.clone();
            self.edit(range, "\n", EditKind::Other, cx);
        } else {
            cx.propagate();
        }

        cx.emit(InputEvent::PressEnter {
            shift: action.shift,
        });
    }

    pub(super) fn escape(&mut self, _: &actions::Escape, _: &mut Window, cx: &mut Context<Self>) {
        if self.context_menu.take().is_some() {
            self.menu_focus_round_trip = false;
            cx.notify();
            return;
        }
        if self.end_composition(cx) {
            return;
        }
        cx.propagate();
    }

    pub(super) fn on_key_down(&mut self, _: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.pause_blink(cx);
    }
}

impl InputState {
    fn selected_text(&self) -> Option<String> {
        if self.selection.is_empty() {
            return None;
        }
        let text: &str = &self.text;
        Some(text[self.selection.clone()].to_owned())
    }

    fn copy_selection(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = self.selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn cut_selection(&mut self, cx: &mut Context<Self>) {
        let Some(text) = self.selected_text() else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        let range = self.selection.clone();
        self.edit(range, "", EditKind::Other, cx);
    }

    fn paste_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        if let Some(text) = item.text() {
            let range = self.selection.clone();
            self.edit(range, &text, EditKind::Other, cx);
        }
        let images = item
            .into_entries()
            .filter_map(|entry| match entry {
                ClipboardEntry::Image(image) => Some(Arc::new(image)),
                ClipboardEntry::String(_) | ClipboardEntry::ExternalPaths(_) => None,
            })
            .collect::<Vec<_>>();
        if !images.is_empty() {
            cx.emit(InputEvent::PasteImages(images));
        }
    }
}

impl InputState {
    pub(super) fn step_value(&mut self, direction: StepDirection, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let step = self.step.unwrap_or(1.0);
        let Some(stepped) =
            number::stepped(&self.text, direction, step, self.minimum, self.maximum)
        else {
            return;
        };
        self.replace_all(&stepped, cx);
    }
}

impl InputState {
    pub(super) fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        let Some(layout) = self.last_layout.clone() else {
            return;
        };
        let offset = layout.offset_for_position(event.position);

        if event.button == MouseButton::Right {
            if !self.selection.contains(&offset) {
                self.move_to(offset, cx);
            }
            if self.enable_context_menu {
                self.open_context_menu(event.position, window, cx);
            }
            return;
        }

        match event.click_count {
            0 | 1 => {
                self.drag_granularity = DragGranularity::Character;
                if event.modifiers.shift {
                    self.drag_anchor = self.anchor()..self.anchor();
                    self.select_to(offset, cx);
                } else {
                    self.drag_anchor = offset..offset;
                    self.move_to(offset, cx);
                }
            }
            2 => {
                self.drag_granularity = DragGranularity::Word;
                self.drag_anchor = self.word_range_at(offset);
                let range = self.drag_anchor.clone();
                self.set_selection(range, false, cx);
            }
            _ => {
                self.drag_granularity = DragGranularity::Line;
                self.drag_anchor = self.line_range_at(offset);
                let range = self.drag_anchor.clone();
                self.set_selection(range, false, cx);
            }
        }
        self.selecting = true;
    }

    pub(super) fn on_drag(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if !self.selecting {
            return;
        }
        let Some(layout) = self.last_layout.clone() else {
            return;
        };
        let offset = layout.offset_for_position(position);
        let target = match self.drag_granularity {
            DragGranularity::Character => offset..offset,
            DragGranularity::Word => self.word_range_at(offset),
            DragGranularity::Line => self.line_range_at(offset),
        };

        let anchor = self.drag_anchor.clone();
        if target.start < anchor.start {
            self.set_selection(target.start..anchor.end, true, cx);
        } else {
            self.set_selection(anchor.start..target.end, false, cx);
        }
    }

    pub(super) fn end_drag(&mut self) {
        self.selecting = false;
    }

    pub(super) fn on_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(layout) = self.last_layout.as_ref() else {
            return;
        };
        if !self.mode.is_multi_line() {
            return;
        }
        let overflow = layout.content_size.height - layout.bounds.size.height;
        if overflow <= Pixels::ZERO {
            return;
        }
        let delta = event.delta.pixel_delta(layout.line_height).y;
        self.scroll.y = (self.scroll.y + delta).clamp(-overflow, Pixels::ZERO);
        self.follow_cursor = false;
        cx.notify();
    }
}

impl InputState {
    fn on_focus(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.blink.update(cx, BlinkCursor::start);
        cx.notify();
        if self.menu_focus_round_trip {
            self.menu_focus_round_trip = false;
            return;
        }
        cx.emit(InputEvent::Focus);
    }

    fn on_blur(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.selecting = false;
        self.history.break_group();
        if self.menu_focus_round_trip {
            return;
        }
        self.marked_range = None;
        self.blink.update(cx, BlinkCursor::stop);
        cx.emit(InputEvent::Blur);
        cx.notify();
    }

    fn pause_blink(&mut self, cx: &mut Context<Self>) {
        self.blink.update(cx, BlinkCursor::pause);
    }

    fn hide_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.context_menu.take().is_some() {
            cx.notify();
        }
    }

    fn open_context_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = cx.entity();
        let focus_handle = self.focus_handle.clone();
        let editable = !self.disabled;
        let has_selection = !self.selection.is_empty();
        let has_text = !self.text.is_empty();
        let can_paste = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .is_some_and(|text| !text.is_empty());

        let menu = PopupMenu::build(window, cx, move |menu, _, _| {
            menu.action_context(focus_handle)
                .item(
                    PopupMenuItem::new("Cut")
                        .disabled(!editable || !has_selection)
                        .on_click({
                            let state = state.clone();
                            move |_, _, cx| state.update(cx, InputState::cut_selection)
                        }),
                )
                .item(
                    PopupMenuItem::new("Copy")
                        .disabled(!has_selection)
                        .on_click({
                            let state = state.clone();
                            move |_, _, cx| state.update(cx, InputState::copy_selection)
                        }),
                )
                .item(
                    PopupMenuItem::new("Paste")
                        .disabled(!editable || !can_paste)
                        .on_click({
                            let state = state.clone();
                            move |_, _, cx| state.update(cx, InputState::paste_clipboard)
                        }),
                )
                .item(PopupMenuItem::separator())
                .item(
                    PopupMenuItem::new("Select All")
                        .disabled(!has_text)
                        .on_click({
                            let state = state.clone();
                            move |_, _, cx| {
                                state.update(cx, |this, cx| {
                                    let end = this.text.len();
                                    this.set_selection(0..end, false, cx);
                                });
                            }
                        }),
                )
        });

        let subscription = cx.subscribe(&menu, |this, _, _: &DismissEvent, cx| {
            this.context_menu = None;
            cx.notify();
        });

        // Beats gpui's own focus-on-mouse-down listener to the event.
        window.prevent_default();

        self.menu_focus_round_trip = self.focus_handle.is_focused(window);
        self.context_menu_position = position;
        self.context_menu = Some(menu.clone());
        self._context_menu_subscription = Some(subscription);
        menu.focus_handle(cx).focus(window, cx);
        cx.notify();
    }
}

impl Render for InputState {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let position = self.context_menu_position;
        let menu = self.context_menu.clone().map(|menu| {
            deferred(
                anchored()
                    .position(position)
                    .snap_to_window_with_margin(px(8.))
                    .child(menu),
            )
            .with_priority(1)
        });

        div()
            .flex_1()
            .min_w_0()
            .when(self.mode.is_multi_line(), gpui::Styled::h_full)
            .child(TextElement::new(cx.entity()))
            .children(menu)
    }
}

impl EntityInputHandler for InputState {
    /// Hands the OS IME the real text, masked field or not: masking it would
    /// break composition outright.
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.clamp_range(self.range_from_utf16(&range_utf16));
        adjusted_range.replace(self.range_to_utf16(&range));
        let text: &str = &self.text;
        Some(text[range].to_owned())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selection),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.end_composition(cx);
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.edit_target(range_utf16);
        let kind = if new_text.is_empty() {
            EditKind::Delete
        } else if range.is_empty() || self.marked_range.is_some() {
            EditKind::Insert
        } else {
            EditKind::Other
        };
        self.edit(range, new_text, kind, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }

        let range = self.edit_target(range_utf16);
        let inserted = self.sanitize(new_text).into_owned();

        let candidate = {
            let text: &str = &self.text;
            let mut candidate = String::with_capacity(text.len() + inserted.len());
            candidate.push_str(&text[..range.start]);
            candidate.push_str(&inserted);
            candidate.push_str(&text[range.end..]);
            candidate
        };

        if !self.is_valid(&candidate, cx) {
            let current = self.text.clone();
            if self.is_valid(&current, cx) {
                return;
            }
        }

        let before = self.snapshot();
        self.text = candidate.into();

        if inserted.is_empty() {
            self.marked_range = None;
            self.selection = range.start..range.start;
        } else {
            let marked = range.start..range.start + inserted.len();
            self.selection = new_selected_range_utf16.map_or_else(
                || marked.end..marked.end,
                |selected| {
                    let start = range.start + utf8_offset(&inserted, selected.start);
                    let end = range.start + utf8_offset(&inserted, selected.end);
                    start..end
                },
            );
            self.marked_range = Some(marked);
        }
        self.selection_reversed = false;
        self.history.push(before, EditKind::Insert);
        self.follow_cursor = true;
        self.pause_blink(cx);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.last_layout.as_ref()?;
        let range = self.clamp_range(self.range_from_utf16(&range_utf16));
        let start = layout.position_for_offset(range.start);
        let end = layout.position_for_offset(range.end);

        let viewport_right = layout.bounds.origin.x + layout.bounds.size.width - layout.origin.x;
        let right = if start.y == end.y {
            end.x.max(start.x)
        } else {
            viewport_right.max(start.x)
        };

        Some(Bounds::from_corners(
            layout.origin + start,
            layout.origin + point(right, start.y + layout.line_height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let layout = self.last_layout.as_ref()?;
        Some(self.offset_to_utf16(layout.offset_for_position(point)))
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.offset_to_utf16(self.text.len()))
    }

    fn accepts_text_input(&self, _: &mut Window, _: &mut Context<Self>) -> bool {
        !self.disabled
    }
}

impl InputState {
    fn end_composition(&mut self, cx: &mut Context<Self>) -> bool {
        if self.marked_range.take().is_none() {
            return false;
        }
        cx.emit(InputEvent::Change);
        cx.notify();
        true
    }

    fn edit_target(&self, range_utf16: Option<Range<usize>>) -> Range<usize> {
        let range = range_utf16
            .map(|range| self.range_from_utf16(&range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selection.clone());
        self.clamp_range(range)
    }
}

pub(super) struct BlinkCursor {
    visible: bool,
    paused: bool,
    epoch: usize,
    _task: Task<()>,
}

impl BlinkCursor {
    const INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);
    const PAUSE: std::time::Duration = std::time::Duration::from_millis(300);

    fn new() -> Self {
        Self {
            visible: false,
            paused: false,
            epoch: 0,
            _task: Task::ready(()),
        }
    }

    fn visible(&self) -> bool {
        self.paused || self.visible
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        self.blink(self.epoch, cx);
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        self.epoch = 0;
        self.visible = false;
        self.paused = false;
        cx.notify();
    }

    fn next_epoch(&mut self) -> usize {
        self.epoch += 1;
        self.epoch
    }

    fn blink(&mut self, epoch: usize, cx: &mut Context<Self>) {
        if self.paused || epoch != self.epoch {
            self.visible = true;
            return;
        }

        self.visible = !self.visible;
        cx.notify();

        let epoch = self.next_epoch();
        self._task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Self::INTERVAL).await;
            if let Some(this) = this.upgrade() {
                () = this.update(cx, |this, cx| this.blink(epoch, cx));
            }
        });
    }

    fn pause(&mut self, cx: &mut Context<Self>) {
        self.paused = true;
        self.visible = true;
        cx.notify();

        let epoch = self.next_epoch();
        self._task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Self::PAUSE).await;
            if let Some(this) = this.upgrade() {
                () = this.update(cx, |this, cx| {
                    this.paused = false;
                    this.blink(epoch, cx);
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_round_trips_through_astral_characters() {
        let text = "a😀b";
        assert_eq!(utf16_offset(text, 0), 0);
        assert_eq!(utf16_offset(text, 1), 1);
        assert_eq!(utf16_offset(text, 5), 3);
        assert_eq!(utf16_offset(text, 6), 4);

        assert_eq!(utf8_offset(text, 0), 0);
        assert_eq!(utf8_offset(text, 1), 1);
        assert_eq!(utf8_offset(text, 2), 1);
        assert_eq!(utf8_offset(text, 3), 5);
        assert_eq!(utf8_offset(text, 4), 6);
    }

    #[test]
    fn character_classes_split_words_from_punctuation() {
        assert_eq!(class_of('a'), CharClass::Word);
        assert_eq!(class_of('_'), CharClass::Word);
        assert_eq!(class_of('9'), CharClass::Word);
        assert_eq!(class_of(' '), CharClass::Whitespace);
        assert_eq!(class_of('/'), CharClass::Punctuation);
    }

    /// A 1x1 PNG: the smallest thing a pasteboard can call an image.
    const PNG_PIXEL: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    fn paste_into_a_field(
        item: gpui::ClipboardItem,
        cx: &mut gpui::TestAppContext,
    ) -> (String, Vec<InputEvent>) {
        cx.update(crate::init);
        let (state, cx) = cx.add_window_view(InputState::new);
        let cx: &mut gpui::VisualTestContext = cx;
        let heard = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        cx.update(|_, cx| {
            cx.write_to_clipboard(item);
            let heard = std::rc::Rc::clone(&heard);
            cx.subscribe(&state, move |_, event: &InputEvent, _| {
                heard.borrow_mut().push(event.clone());
            })
            .detach();
        });

        state.update(cx, InputState::paste_clipboard);
        cx.run_until_parked();
        let value = state.read_with(cx, |state, _| state.value().to_string());
        let heard = heard.borrow().clone();
        (value, heard)
    }

    #[gpui::test]
    fn an_image_only_clipboard_is_forwarded_instead_of_dropped(cx: &mut gpui::TestAppContext) {
        let image = gpui::Image::from_bytes(gpui::ImageFormat::Png, PNG_PIXEL.to_vec());
        let (value, heard) = paste_into_a_field(gpui::ClipboardItem::new_image(&image), cx);

        assert!(value.is_empty(), "an image inserts no text");
        let [InputEvent::PasteImages(images)] = heard.as_slice() else {
            panic!("the field should forward the image, not swallow it: {heard:?}");
        };
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].bytes, PNG_PIXEL);
    }

    #[gpui::test]
    fn a_text_clipboard_still_pastes_as_text(cx: &mut gpui::TestAppContext) {
        let (value, heard) =
            paste_into_a_field(gpui::ClipboardItem::new_string("hello".to_owned()), cx);

        assert_eq!(value, "hello");
        assert!(
            !heard
                .iter()
                .any(|event| matches!(event, InputEvent::PasteImages(_))),
            "a text paste is not an image paste"
        );
    }
}
