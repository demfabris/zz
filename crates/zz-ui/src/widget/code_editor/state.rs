//! Rope, selection, history, focus, and IME state. Offsets are UTF-8 bytes on grapheme boundaries.

use std::{cell::Cell, ops::Range, time::Duration};

use gpui::{
    Action, App, AppContext as _, Bounds, ClipboardItem, Context, Corners, Edges, Entity,
    EntityInputHandler, EventEmitter, FocusHandle, Focusable, IntoElement, KeyBinding,
    KeyDownEvent, MouseButton, MouseDownEvent, ParentElement as _, Pixels, Point, Render,
    ScrollWheelEvent, SharedString, Styled as _, Subscription, TextRun, TextStyle, UTF16Selection,
    Window, actions, div, point,
};
use ropey::Rope;
use sum_tree::Bias;
use unicode_segmentation::UnicodeSegmentation as _;

use crate::{ActiveTheme as _, highlighter::SyntaxHighlighter, text::suppress_text_selection};

use super::{
    DisplayMap, InputEdit, RopeExt as _, Selection,
    blink_cursor::BlinkCursor,
    change::Change,
    element::{EditorLayout, LastLayout, ShapedCache, TextElement, WhitespaceIndicators},
    history::History,
    mode::EditorMode,
    vim::{VimKey, VimState},
};

#[derive(Action, Clone, PartialEq, Eq, serde::Deserialize)]
#[action(namespace = code_editor, no_json)]
pub(super) struct Enter;

actions!(
    code_editor,
    [
        Backspace,
        Delete,
        DeleteToBeginningOfLine,
        DeleteToEndOfLine,
        DeleteToPreviousWordStart,
        DeleteToNextWordEnd,
        Indent,
        Outdent,
        IndentInline,
        OutdentInline,
        MoveUp,
        MoveDown,
        MoveLeft,
        MoveRight,
        MoveHome,
        MoveEnd,
        MovePageUp,
        MovePageDown,
        SelectAll,
        SelectToStartOfLine,
        SelectToEndOfLine,
        SelectToStart,
        SelectToEnd,
        SelectToPreviousWordStart,
        SelectToNextWordEnd,
        SelectUp,
        SelectDown,
        SelectLeft,
        SelectRight,
        ShowCharacterPalette,
        Copy,
        Cut,
        Paste,
        Undo,
        Redo,
        MoveToStart,
        MoveToEnd,
        MoveToPreviousWord,
        MoveToNextWord,
        Escape,
        VimHalfPageDown,
        VimHalfPageUp,
        VimPageDown,
        VimPageUp,
        VimRedo,
    ]
);

pub(super) const CONTEXT: &str = "CodeEditor";
pub(super) const VIM_CONTEXT: &str = "CodeEditor vim";
const VIM_PREDICATE: &str = "CodeEditor && vim";

pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some(CONTEXT)),
        KeyBinding::new("shift-backspace", Backspace, Some(CONTEXT)),
        KeyBinding::new("delete", Delete, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("cmd-backspace", DeleteToBeginningOfLine, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("cmd-delete", DeleteToEndOfLine, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("alt-backspace", DeleteToPreviousWordStart, Some(CONTEXT)),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        KeyBinding::new("ctrl-backspace", DeleteToPreviousWordStart, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("alt-delete", DeleteToNextWordEnd, Some(CONTEXT)),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        KeyBinding::new("ctrl-delete", DeleteToNextWordEnd, Some(CONTEXT)),
        KeyBinding::new("enter", Enter, Some(CONTEXT)),
        KeyBinding::new("tab", IndentInline, Some(CONTEXT)),
        KeyBinding::new("shift-tab", OutdentInline, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("cmd-]", Indent, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("cmd-[", Outdent, Some(CONTEXT)),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        KeyBinding::new("ctrl-]", Indent, Some(CONTEXT)),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        KeyBinding::new("ctrl-[", Outdent, Some(CONTEXT)),
        KeyBinding::new("escape", Escape, Some(CONTEXT)),
        KeyBinding::new("left", MoveLeft, Some(CONTEXT)),
        KeyBinding::new("right", MoveRight, Some(CONTEXT)),
        KeyBinding::new("up", MoveUp, Some(CONTEXT)),
        KeyBinding::new("down", MoveDown, Some(CONTEXT)),
        KeyBinding::new("pageup", MovePageUp, Some(CONTEXT)),
        KeyBinding::new("pagedown", MovePageDown, Some(CONTEXT)),
        KeyBinding::new("home", MoveHome, Some(CONTEXT)),
        KeyBinding::new("end", MoveEnd, Some(CONTEXT)),
        KeyBinding::new("shift-left", SelectLeft, Some(CONTEXT)),
        KeyBinding::new("shift-right", SelectRight, Some(CONTEXT)),
        KeyBinding::new("shift-up", SelectUp, Some(CONTEXT)),
        KeyBinding::new("shift-down", SelectDown, Some(CONTEXT)),
        KeyBinding::new("shift-home", SelectToStartOfLine, Some(CONTEXT)),
        KeyBinding::new("shift-end", SelectToEndOfLine, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("cmd-left", MoveHome, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("cmd-right", MoveEnd, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("alt-left", MoveToPreviousWord, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("alt-right", MoveToNextWord, Some(CONTEXT)),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        KeyBinding::new("ctrl-left", MoveToPreviousWord, Some(CONTEXT)),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        KeyBinding::new("ctrl-right", MoveToNextWord, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("shift-cmd-left", SelectToStartOfLine, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("shift-cmd-right", SelectToEndOfLine, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("alt-shift-left", SelectToPreviousWordStart, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("alt-shift-right", SelectToNextWordEnd, Some(CONTEXT)),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        KeyBinding::new("ctrl-shift-left", SelectToPreviousWordStart, Some(CONTEXT)),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        KeyBinding::new("ctrl-shift-right", SelectToNextWordEnd, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("cmd-up", MoveToStart, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("cmd-down", MoveToEnd, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("cmd-shift-up", SelectToStart, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("cmd-shift-down", SelectToEnd, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("cmd-a", SelectAll, Some(CONTEXT)),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        KeyBinding::new("ctrl-a", SelectAll, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("cmd-c", Copy, Some(CONTEXT)),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        KeyBinding::new("ctrl-c", Copy, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("cmd-x", Cut, Some(CONTEXT)),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        KeyBinding::new("ctrl-x", Cut, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("cmd-v", Paste, Some(CONTEXT)),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        KeyBinding::new("ctrl-v", Paste, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("cmd-z", Undo, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("cmd-shift-z", Redo, Some(CONTEXT)),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        KeyBinding::new("ctrl-z", Undo, Some(CONTEXT)),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        KeyBinding::new("ctrl-y", Redo, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("ctrl-cmd-space", ShowCharacterPalette, Some(CONTEXT)),
        KeyBinding::new("ctrl-d", VimHalfPageDown, Some(VIM_PREDICATE)),
        KeyBinding::new("ctrl-u", VimHalfPageUp, Some(VIM_PREDICATE)),
        KeyBinding::new("ctrl-f", VimPageDown, Some(VIM_PREDICATE)),
        KeyBinding::new("ctrl-b", VimPageUp, Some(VIM_PREDICATE)),
        KeyBinding::new("ctrl-r", VimRedo, Some(VIM_PREDICATE)),
    ]);
}

#[derive(Clone, Debug)]
pub enum CodeEditorEvent {
    Change,
    Focus,
    Blur,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DragGranularity {
    Character,
    Word,
    Line,
}

pub struct CodeEditorState {
    pub(super) focus_handle: FocusHandle,
    pub(super) mode: EditorMode,
    pub(super) text: Rope,
    pub(super) render_text: SharedString,
    pub(super) placeholder: SharedString,
    pub(super) display_map: DisplayMap,
    highlighter: Option<SyntaxHighlighter>,
    history: History<Change>,
    typing_group: bool,
    pub(super) blink_cursor: Entity<BlinkCursor>,

    pub(super) selected_range: Selection,
    pub(super) selected_word_range: Option<Selection>,
    pub(super) selection_reversed: bool,
    ime_marked_range: Option<Selection>,
    pub(super) preferred_column: Option<usize>,

    pub(super) editor_layout: Option<EditorLayout>,
    pub(super) shaped_cache: Option<ShapedCache>,
    pub(super) layout_generation: u64,
    pub(super) scroll: Point<Pixels>,
    pub(super) follow_cursor: bool,
    pub(super) reset_scroll: bool,
    pub(super) soft_wrap: bool,
    pub(super) disabled: bool,
    pub(super) corner_radii: Corners<Pixels>,
    pub(super) vim: Option<VimState>,

    selecting: bool,
    drag_granularity: DragGranularity,
    drag_anchor: Selection,
    pub(super) _last_layout: Option<LastLayout>,
    pub(super) _whitespace_indicators: WhitespaceIndicators,
    pub(super) _editor_scrollbar_paddings: Cell<Edges<Pixels>>,
    pub(super) _subscriptions: Vec<Subscription>,
}

impl EventEmitter<CodeEditorEvent> for CodeEditorState {}

impl Focusable for CodeEditorState {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl CodeEditorState {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle().tab_stop(true);
        let blink_cursor = cx.new(|_| BlinkCursor::new());
        let text = Rope::new();
        let text_style = window.text_style();
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let display_map = DisplayMap::new(text_style.font(), font_size, None);

        let subscriptions = vec![
            cx.observe(&blink_cursor, |_, _, cx| cx.notify()),
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() && this.focus_handle.is_focused(window) {
                    this.blink_cursor.update(cx, BlinkCursor::start);
                }
            }),
            cx.on_focus(&focus_handle, window, Self::on_focus),
            cx.on_blur(&focus_handle, window, Self::on_blur),
        ];

        let mut state = Self {
            focus_handle,
            mode: EditorMode::default(),
            text,
            render_text: SharedString::default(),
            placeholder: SharedString::default(),
            display_map,
            highlighter: None,
            history: History::new().group_interval(Duration::ZERO),
            typing_group: false,
            blink_cursor,
            selected_range: Selection::default(),
            selected_word_range: None,
            selection_reversed: false,
            ime_marked_range: None,
            preferred_column: None,
            editor_layout: None,
            shaped_cache: None,
            layout_generation: 0,
            scroll: Point::default(),
            follow_cursor: false,
            reset_scroll: false,
            soft_wrap: true,
            disabled: false,
            corner_radii: Corners::default(),
            vim: None,
            selecting: false,
            drag_granularity: DragGranularity::Character,
            drag_anchor: Selection::default(),
            _last_layout: None,
            _whitespace_indicators: WhitespaceIndicators::default(),
            _editor_scrollbar_paddings: Cell::new(Edges::all(Pixels::ZERO)),
            _subscriptions: subscriptions,
        };
        state.rebuild_highlighter();
        state
    }

    #[must_use]
    pub fn default_value(mut self, value: impl AsRef<str>) -> Self {
        self.text = Rope::from(value.as_ref());
        self.render_text = value.as_ref().to_string().into();
        let end = self.text.len();
        self.selected_range = (end..end).into();
        self.rebuild_highlighter();
        self.invalidate_shaping();
        self
    }

    #[must_use]
    pub fn language(mut self, language: impl Into<SharedString>) -> Self {
        self.mode.set_language(language);
        self.rebuild_highlighter();
        self.invalidate_shaping();
        self
    }

    pub fn set_language(
        &mut self,
        language: impl Into<SharedString>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.mode.set_language(language);
        self.rebuild_highlighter();
        self.invalidate_shaping();
        cx.notify();
    }

    pub fn language_name(&self) -> &str {
        self.mode.language().as_ref()
    }

    #[must_use]
    pub fn soft_wrap(mut self, enabled: bool) -> Self {
        self.soft_wrap = enabled;
        self
    }

    pub fn set_soft_wrap(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.soft_wrap == enabled {
            return;
        }
        self.soft_wrap = enabled;
        self.follow_cursor = true;
        cx.notify();
    }

    pub fn set_line_numbers(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.mode.line_numbers() == enabled {
            return;
        }
        self.mode.set_line_numbers(enabled);
        cx.notify();
    }

    pub fn set_corner_radii(&mut self, radii: Corners<Pixels>, cx: &mut Context<Self>) {
        if self.corner_radii == radii {
            return;
        }
        self.corner_radii = radii;
        cx.notify();
    }

    /// Numbers the rail by distance from the cursor, which keeps its absolute number.
    pub fn set_relative_line_numbers(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.mode.relative_line_numbers() == enabled {
            return;
        }
        self.mode.set_relative_line_numbers(enabled);
        cx.notify();
    }

    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn value(&self) -> SharedString {
        self.render_text.clone()
    }

    pub fn text(&self) -> &Rope {
        &self.text
    }

    pub fn set_value(&mut self, value: impl AsRef<str>, _: &mut Window, cx: &mut Context<Self>) {
        if self.render_text.as_ref() == value.as_ref() {
            return;
        }
        self.text = Rope::from(value.as_ref());
        self.render_text = value.as_ref().to_string().into();
        let end = self.text.len();
        self.selected_range = (end..end).into();
        self.selection_reversed = false;
        self.ime_marked_range = None;
        self.preferred_column = None;
        if let Some(vim) = self.vim.as_mut() {
            vim.reset();
        }
        self.history.clear();
        self.break_typing_group();
        self.scroll = Point::default();
        self.reset_scroll = true;
        self.follow_cursor = false;
        self.display_map.set_text(&self.text, cx);
        self.rebuild_highlighter();
        self.invalidate_shaping();
        cx.notify();
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_handle.focus(window, cx);
    }

    pub fn cursor(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    pub fn selected_range(&self) -> Range<usize> {
        self.selected_range.into()
    }

    pub fn set_selected_range(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        let range = self.clamp_range(range);
        self.selected_range = range.into();
        self.selection_reversed = false;
        self.preferred_column = None;
        self.break_typing_group();
        self.follow_cursor = true;
        self.pause_blink_cursor(cx);
        cx.notify();
    }

    pub fn insert(&mut self, text: impl AsRef<str>, _: &mut Window, cx: &mut Context<Self>) {
        let cursor = self.cursor();
        self.replace_range(cursor..cursor, text.as_ref(), false, cx);
    }

    pub fn replace(&mut self, text: impl AsRef<str>, _: &mut Window, cx: &mut Context<Self>) {
        self.replace_range(self.selected_range.into(), text.as_ref(), false, cx);
    }

    pub(super) fn focus_handle_ref(&self) -> &FocusHandle {
        &self.focus_handle
    }

    pub(super) fn show_cursor(&self, window: &Window, cx: &App) -> bool {
        self.focus_handle.is_focused(window)
            && !self.disabled
            && self.blink_cursor.read(cx).visible()
            && window.is_window_active()
    }

    pub(super) fn text_runs(&self, style: &TextStyle, cx: &App) -> Vec<TextRun> {
        let base = TextRun {
            color: style.color,
            ..style.to_run(self.render_text.len())
        };
        let Some(highlighter) = self.highlighter.as_ref() else {
            return vec![base];
        };
        let styles = highlighter.styles(&(0..self.render_text.len()), &cx.theme().highlight_theme);
        if styles.is_empty() {
            return vec![base];
        }
        styles
            .into_iter()
            .filter(|(range, _)| !range.is_empty())
            .map(|(range, highlight)| style.clone().highlight(highlight).to_run(range.len()))
            .collect()
    }

    pub(super) fn sync_display_map_layout(
        &mut self,
        wrap_width: Option<Pixels>,
        font: gpui::Font,
        font_size: Pixels,
        cx: &mut Context<Self>,
    ) {
        self.display_map.set_font(font, font_size, cx);
        self.display_map.on_layout_changed(wrap_width, cx);
        self.display_map.ensure_text_prepared(&self.text, cx);
    }

    fn invalidate_shaping(&mut self) {
        self.layout_generation = self.layout_generation.wrapping_add(1);
        self.shaped_cache = None;
    }

    fn rebuild_highlighter(&mut self) {
        let mut highlighter = SyntaxHighlighter::new(self.mode.language());
        #[cfg(all(feature = "tree-sitter", not(target_family = "wasm")))]
        {
            highlighter.update(None, &self.text, None);
        }
        #[cfg(any(not(feature = "tree-sitter"), target_family = "wasm"))]
        highlighter.update(self.render_text.as_ref());
        self.highlighter = Some(highlighter);
    }

    fn update_highlighter(&mut self, edit: InputEdit) {
        let Some(highlighter) = self.highlighter.as_mut() else {
            return;
        };
        #[cfg(all(feature = "tree-sitter", not(target_family = "wasm")))]
        {
            highlighter.update(Some(edit), &self.text, None);
        }
        #[cfg(any(not(feature = "tree-sitter"), target_family = "wasm"))]
        {
            let _ = edit;
            highlighter.update(self.render_text.as_ref());
        }
    }

    pub(super) fn replace_range(
        &mut self,
        range: Range<usize>,
        inserted: &str,
        typing: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.disabled {
            return false;
        }
        let range = self.clamp_range(range);
        let inserted = sanitize(inserted);
        if range.is_empty() && inserted.is_empty() {
            return false;
        }
        if typing {
            if !self.typing_group {
                self.history.start_grouping();
                self.typing_group = true;
            }
        } else {
            self.break_typing_group();
        }

        let old_text = self.text.clone();
        let old_slice = old_text.slice(range.clone()).to_string();
        self.history.push(Change::new(
            range.clone(),
            &old_slice,
            range.start..range.start + inserted.len(),
            &inserted,
        ));

        let start_position = old_text.offset_to_point(range.start);
        let old_end_position = old_text.offset_to_point(range.end);
        self.display_map
            .adjust_folds_for_edit(&old_text, &range, &inserted);
        self.text.remove(range.clone());
        self.text.insert(range.start, &inserted);
        self.render_text = self.text.to_string().into();
        let new_end = range.start + inserted.len();
        let new_end_position = self.text.offset_to_point(new_end);
        let edit = InputEdit {
            start_byte: range.start,
            old_end_byte: range.end,
            new_end_byte: new_end,
            start_position,
            old_end_position,
            new_end_position,
        };
        self.display_map
            .on_text_changed(&old_text, &range, &self.text, cx);
        self.update_highlighter(edit);
        self.invalidate_shaping();
        self.selected_range = (new_end..new_end).into();
        self.selection_reversed = false;
        self.selected_word_range = None;
        self.ime_marked_range = None;
        self.preferred_column = None;
        self.follow_cursor = true;
        self.pause_blink_cursor(cx);
        cx.emit(CodeEditorEvent::Change);
        cx.notify();
        true
    }

    fn apply_without_history(
        &mut self,
        range: Range<usize>,
        inserted: &str,
        cx: &mut Context<Self>,
    ) {
        let range = self.clamp_range(range);
        let old_text = self.text.clone();
        let start_position = old_text.offset_to_point(range.start);
        let old_end_position = old_text.offset_to_point(range.end);
        self.display_map
            .adjust_folds_for_edit(&old_text, &range, inserted);
        self.text.remove(range.clone());
        self.text.insert(range.start, inserted);
        self.render_text = self.text.to_string().into();
        let new_end = range.start + inserted.len();
        let edit = InputEdit {
            start_byte: range.start,
            old_end_byte: range.end,
            new_end_byte: new_end,
            start_position,
            old_end_position,
            new_end_position: self.text.offset_to_point(new_end),
        };
        self.display_map
            .on_text_changed(&old_text, &range, &self.text, cx);
        self.update_highlighter(edit);
        self.invalidate_shaping();
        self.selected_range = (new_end..new_end).into();
        self.selection_reversed = false;
    }

    pub(super) fn break_typing_group(&mut self) {
        if self.typing_group {
            self.history.end_grouping();
            self.typing_group = false;
        }
        self.history.break_group();
    }

    pub(super) fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.preferred_column = None;
        self.move_to_preserving_column(offset, cx);
    }

    pub(super) fn move_to_preserving_column(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = self
            .text
            .clip_offset(offset.min(self.text.len()), Bias::Left);
        self.selected_range = (offset..offset).into();
        self.selection_reversed = false;
        self.selected_word_range = None;
        self.break_typing_group();
        self.follow_cursor = true;
        self.pause_blink_cursor(cx);
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = self
            .text
            .clip_offset(offset.min(self.text.len()), Bias::Left);
        let anchor = if self.selection_reversed {
            self.selected_range.end
        } else {
            self.selected_range.start
        };
        self.selection_reversed = offset < anchor;
        self.selected_range = if offset < anchor {
            (offset..anchor).into()
        } else {
            (anchor..offset).into()
        };
        self.break_typing_group();
        self.follow_cursor = true;
        self.pause_blink_cursor(cx);
        cx.notify();
    }

    pub(super) fn previous_boundary(&self, offset: usize) -> usize {
        self.render_text[..offset.min(self.render_text.len())]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(index, _)| index)
    }

    pub(super) fn next_boundary(&self, offset: usize) -> usize {
        let offset = offset.min(self.render_text.len());
        self.render_text[offset..]
            .grapheme_indices(true)
            .nth(1)
            .map_or(self.render_text.len(), |(index, _)| offset + index)
    }

    pub(super) fn start_of_line(&self) -> usize {
        let point = self.text.offset_to_point(self.cursor());
        self.text.line_start_offset(point.row)
    }

    pub(super) fn end_of_line(&self) -> usize {
        let point = self.text.offset_to_point(self.cursor());
        self.text.line_end_offset(point.row)
    }

    pub(super) fn previous_word_start(&self, offset: usize) -> usize {
        let text = self.render_text.as_ref();
        let mut result = 0;
        for (start, word) in text.split_word_bound_indices() {
            if start >= offset {
                break;
            }
            if word.chars().any(char::is_alphanumeric) {
                result = start;
            }
        }
        result
    }

    pub(super) fn next_word_end(&self, offset: usize) -> usize {
        let text = self.render_text.as_ref();
        text.split_word_bound_indices()
            .find_map(|(start, word)| {
                let end = start + word.len();
                (end > offset && word.chars().any(char::is_alphanumeric)).then_some(end)
            })
            .unwrap_or(text.len())
    }

    fn clamp_range(&self, range: Range<usize>) -> Range<usize> {
        let start = self
            .text
            .clip_offset(range.start.min(self.text.len()), Bias::Left);
        let end = self
            .text
            .clip_offset(range.end.min(self.text.len()), Bias::Right);
        start.min(end)..start.max(end)
    }

    pub(super) fn pause_blink_cursor(&mut self, cx: &mut Context<Self>) {
        self.blink_cursor.update(cx, BlinkCursor::pause);
    }
}

fn sanitize(text: &str) -> String {
    let mut sanitized = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\r' {
            if characters.peek() == Some(&'\n') {
                characters.next();
            }
            sanitized.push('\n');
        } else if !character.is_control() || matches!(character, '\n' | '\t') {
            sanitized.push(character);
        }
    }
    sanitized
}

impl CodeEditorState {
    pub(super) fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.vim_key(VimKey::Backspace, window, cx) {
            return;
        }
        let range: Range<usize> = if self.selected_range.is_empty() {
            self.previous_boundary(self.cursor())..self.cursor()
        } else {
            self.selected_range.into()
        };
        self.replace_range(range, "", true, cx);
    }

    pub(super) fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        if self.vim_intercepts_text() {
            return;
        }
        let range: Range<usize> = if self.selected_range.is_empty() {
            self.cursor()..self.next_boundary(self.cursor())
        } else {
            self.selected_range.into()
        };
        self.replace_range(range, "", true, cx);
    }

    pub(super) fn delete_to_beginning_of_line(
        &mut self,
        _: &DeleteToBeginningOfLine,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = if self.selected_range.is_empty() {
            self.start_of_line()..self.cursor()
        } else {
            self.selected_range.into()
        };
        self.replace_range(range, "", false, cx);
    }

    pub(super) fn delete_to_end_of_line(
        &mut self,
        _: &DeleteToEndOfLine,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = if self.selected_range.is_empty() {
            self.cursor()..self.end_of_line()
        } else {
            self.selected_range.into()
        };
        self.replace_range(range, "", false, cx);
    }

    pub(super) fn delete_previous_word(
        &mut self,
        _: &DeleteToPreviousWordStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = if self.selected_range.is_empty() {
            self.previous_word_start(self.cursor())..self.cursor()
        } else {
            self.selected_range.into()
        };
        self.replace_range(range, "", false, cx);
    }

    pub(super) fn delete_next_word(
        &mut self,
        _: &DeleteToNextWordEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = if self.selected_range.is_empty() {
            self.cursor()..self.next_word_end(self.cursor())
        } else {
            self.selected_range.into()
        };
        self.replace_range(range, "", false, cx);
    }

    pub(super) fn enter(&mut self, _: &Enter, window: &mut Window, cx: &mut Context<Self>) {
        if self.vim_key(VimKey::Enter, window, cx) {
            return;
        }
        let point = self.text.offset_to_point(self.cursor());
        let line = self.text.slice_line(point.row);
        let indent = line
            .chars()
            .take_while(|character| matches!(character, ' ' | '\t'))
            .collect::<String>();
        self.replace_range(self.selected_range.into(), &format!("\n{indent}"), true, cx);
    }

    pub(super) fn escape(&mut self, _: &Escape, window: &mut Window, cx: &mut Context<Self>) {
        self.ime_marked_range = None;
        self.break_typing_group();
        if self.vim_key(VimKey::Escape, window, cx) {
            return;
        }
        cx.propagate();
    }

    pub(super) fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(
            self.text.slice(self.selected_range).to_string(),
        ));
    }

    pub(super) fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            return;
        }
        let range: Range<usize> = self.selected_range.into();
        cx.write_to_clipboard(ClipboardItem::new_string(
            self.text.slice(range.clone()).to_string(),
        ));
        self.replace_range(range, "", false, cx);
    }

    pub(super) fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        self.replace_range(self.selected_range.into(), &text, false, cx);
    }

    pub(super) fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        self.break_typing_group();
        self.history.ignore = true;
        if let Some(changes) = self.history.undo() {
            for change in changes {
                self.apply_without_history(change.new_range.into(), &change.old_text, cx);
            }
            self.follow_cursor = true;
            cx.emit(CodeEditorEvent::Change);
            cx.notify();
        }
        self.history.ignore = false;
    }

    pub(super) fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        self.break_typing_group();
        self.history.ignore = true;
        if let Some(changes) = self.history.redo() {
            for change in changes {
                self.apply_without_history(change.old_range.into(), &change.new_text, cx);
            }
            self.follow_cursor = true;
            cx.emit(CodeEditorEvent::Change);
            cx.notify();
        }
        self.history.ignore = false;
    }

    pub(super) fn show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    pub(super) fn on_key_down(&mut self, _: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.pause_blink_cursor(cx);
    }

    pub(super) fn vim_half_page_down(
        &mut self,
        _: &VimHalfPageDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.vim_chord(VimKey::Ctrl('d'), window, cx);
    }

    pub(super) fn vim_half_page_up(
        &mut self,
        _: &VimHalfPageUp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.vim_chord(VimKey::Ctrl('u'), window, cx);
    }

    pub(super) fn vim_page_down(
        &mut self,
        _: &VimPageDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.vim_chord(VimKey::Ctrl('f'), window, cx);
    }

    pub(super) fn vim_page_up(
        &mut self,
        _: &VimPageUp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.vim_chord(VimKey::Ctrl('b'), window, cx);
    }

    pub(super) fn vim_redo(&mut self, _: &VimRedo, window: &mut Window, cx: &mut Context<Self>) {
        self.vim_chord(VimKey::Ctrl('r'), window, cx);
    }

    fn vim_chord(&mut self, key: VimKey, window: &mut Window, cx: &mut Context<Self>) {
        if !self.vim_key(key, window, cx) {
            cx.propagate();
        }
    }
}

impl CodeEditorState {
    pub(super) fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor()), cx);
    }

    pub(super) fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor()), cx);
    }

    pub(super) fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        self.select_vertical(-1, cx);
    }

    pub(super) fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        self.select_vertical(1, cx);
    }

    fn select_vertical(&mut self, delta: isize, cx: &mut Context<Self>) {
        let point = self.text.offset_to_point(self.cursor());
        let column = self.preferred_column.unwrap_or(point.column);
        self.preferred_column = Some(column);
        let row = point
            .row
            .saturating_add_signed(delta)
            .min(self.text.lines_len().saturating_sub(1));
        let target = self.text.line_start_offset(row) + column.min(self.text.slice_line(row).len());
        self.select_to(target, cx);
        self.preferred_column = Some(column);
    }

    pub(super) fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.selected_range = (0..self.text.len()).into();
        self.selection_reversed = false;
        self.break_typing_group();
        cx.notify();
    }

    pub(super) fn select_to_start(
        &mut self,
        _: &SelectToStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(0, cx);
    }

    pub(super) fn select_to_end(
        &mut self,
        _: &SelectToEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(self.text.len(), cx);
    }

    pub(super) fn select_to_start_of_line(
        &mut self,
        _: &SelectToStartOfLine,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(self.start_of_line(), cx);
    }

    pub(super) fn select_to_end_of_line(
        &mut self,
        _: &SelectToEndOfLine,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(self.end_of_line(), cx);
    }

    pub(super) fn select_to_previous_word(
        &mut self,
        _: &SelectToPreviousWordStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(self.previous_word_start(self.cursor()), cx);
    }

    pub(super) fn select_to_next_word(
        &mut self,
        _: &SelectToNextWordEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(self.next_word_end(self.cursor()), cx);
    }
}

impl CodeEditorState {
    pub(super) fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        suppress_text_selection(cx);
        let Some(layout) = self.editor_layout.clone() else {
            return;
        };
        let offset = layout.offset_for_position(event.position);

        if event.button == MouseButton::Right {
            cx.propagate();
            return;
        }
        if event.button != MouseButton::Left {
            return;
        }

        self.focus_handle.focus(window, cx);
        self.selecting = true;
        self.drag_granularity = match event.click_count {
            count if count >= 3 => DragGranularity::Line,
            2 => DragGranularity::Word,
            _ => DragGranularity::Character,
        };

        match self.drag_granularity {
            DragGranularity::Line => self.select_line(offset, window, cx),
            DragGranularity::Word => self.select_word(offset, window, cx),
            DragGranularity::Character if event.modifiers.shift => self.select_to(offset, cx),
            DragGranularity::Character => self.move_to(offset, cx),
        }
        self.vim_on_pointer();
        self.drag_anchor = self.selected_range;
    }

    pub(super) fn on_drag(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if !self.selecting {
            return;
        }
        let Some(layout) = self.editor_layout.clone() else {
            return;
        };
        let offset = layout.offset_for_position(position);
        let anchor = self.drag_anchor;
        if offset < anchor.start {
            self.selected_range = (offset..anchor.end).into();
            self.selection_reversed = true;
        } else {
            self.selected_range = (anchor.start..offset).into();
            self.selection_reversed = false;
        }
        self.follow_cursor = true;
        cx.notify();
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
        let Some(layout) = self.editor_layout.as_ref() else {
            return;
        };
        let delta = event.delta.pixel_delta(layout.line_height);
        let overflow_y =
            (layout.content_size.height - layout.text_bounds.size.height).max(Pixels::ZERO);
        let overflow_x =
            (layout.content_size.width - layout.text_bounds.size.width).max(Pixels::ZERO);
        let next = point(
            (self.scroll.x + delta.x).clamp(-overflow_x, Pixels::ZERO),
            (self.scroll.y + delta.y).clamp(-overflow_y, Pixels::ZERO),
        );
        if next != self.scroll {
            self.scroll = next;
            self.follow_cursor = false;
            cx.stop_propagation();
            cx.notify();
        }
    }
}

impl CodeEditorState {
    fn on_focus(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.blink_cursor.update(cx, BlinkCursor::start);
        cx.emit(CodeEditorEvent::Focus);
        cx.notify();
    }

    fn on_blur(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.selecting = false;
        self.break_typing_group();
        self.ime_marked_range = None;
        self.blink_cursor.update(cx, BlinkCursor::stop);
        cx.emit(CodeEditorEvent::Blur);
        cx.notify();
    }
}

impl Render for CodeEditorState {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .min_w_0()
            .child(TextElement::new(cx.entity()))
    }
}

impl EntityInputHandler for CodeEditorState {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.clamp_range(self.range_from_utf16(&range_utf16));
        adjusted_range.replace(self.range_to_utf16(&range));
        Some(self.text.slice(range).to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range.into()),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.ime_marked_range
            .map(Into::<Range<usize>>::into)
            .map(|range| self.range_to_utf16(&range))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if self.ime_marked_range.take().is_some() {
            cx.emit(CodeEditorEvent::Change);
            cx.notify();
        }
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.vim_intercepts_text() {
            for character in new_text.chars() {
                self.vim_key(VimKey::Char(character), window, cx);
            }
            return;
        }
        let range = range_utf16
            .map(|range| self.range_from_utf16(&range))
            .or_else(|| self.ime_marked_range.map(Into::into))
            .unwrap_or_else(|| self.selected_range.into());
        self.replace_range(range, new_text, true, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.vim_intercepts_text() {
            return;
        }
        let range = range_utf16
            .map(|range| self.range_from_utf16(&range))
            .or_else(|| self.ime_marked_range.map(Into::into))
            .unwrap_or_else(|| self.selected_range.into());
        let start = range.start;
        if !self.replace_range(range, new_text, true, cx) {
            return;
        }
        let marked = start..start + sanitize(new_text).len();
        self.ime_marked_range = Some(marked.clone().into());
        if let Some(selected) = new_selected_range_utf16 {
            let inserted = sanitize(new_text);
            self.selected_range = (start + utf8_offset(&inserted, selected.start)
                ..start + utf8_offset(&inserted, selected.end))
                .into();
        }
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.editor_layout.as_ref()?;
        let range = self.clamp_range(self.range_from_utf16(&range_utf16));
        let start = layout.position_for_offset(range.start);
        let end = layout.position_for_offset(range.end);
        let right = if start.y == end.y {
            end.x.max(start.x)
        } else {
            layout.text_bounds.size.width.max(start.x)
        };
        Some(Bounds::from_corners(
            layout.text_origin + start,
            layout.text_origin + point(right, start.y + layout.line_height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let layout = self.editor_layout.as_ref()?;
        Some(self.offset_to_utf16(layout.offset_for_position(point)))
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.offset_to_utf16(self.text.len()))
    }

    fn accepts_text_input(&self, _: &mut Window, _: &mut Context<Self>) -> bool {
        !self.disabled
    }
}

impl CodeEditorState {
    fn offset_to_utf16(&self, offset: usize) -> usize {
        utf16_offset(self.render_text.as_ref(), offset)
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        utf8_offset(self.render_text.as_ref(), offset)
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }
}

fn utf16_offset(text: &str, byte_offset: usize) -> usize {
    text[..floor_char_boundary(text, byte_offset.min(text.len()))]
        .encode_utf16()
        .count()
}

fn utf8_offset(text: &str, utf16_offset: usize) -> usize {
    let mut units = 0;
    for (byte, character) in text.char_indices() {
        let next = units + character.len_utf16();
        if next > utf16_offset {
            return byte;
        }
        units = next;
    }
    text.len()
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

    fn with_editor(
        cx: &mut gpui::TestAppContext,
    ) -> (Entity<CodeEditorState>, &mut gpui::VisualTestContext) {
        cx.update(crate::init);
        cx.add_window_view(CodeEditorState::new)
    }

    fn type_text(editor: &Entity<CodeEditorState>, text: &str, cx: &mut gpui::VisualTestContext) {
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.replace_text_in_range(None, text, window, cx);
            });
        });
    }

    #[gpui::test]
    fn typing_and_backspace_are_grapheme_safe(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = with_editor(cx);
        type_text(&editor, "a", cx);
        type_text(&editor, "e\u{301}", cx);

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.backspace(&Backspace, window, cx);
            });
        });

        assert_eq!(editor.read_with(cx, |editor, _| editor.value()), "a");
        assert_eq!(editor.read_with(cx, |editor, _| editor.cursor()), 1);
    }

    #[gpui::test]
    fn adjacent_typing_is_one_undo_redo_group(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = with_editor(cx);
        for character in ["a", "b", "c"] {
            type_text(&editor, character, cx);
        }

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| editor.undo(&Undo, window, cx));
        });
        assert_eq!(editor.read_with(cx, |editor, _| editor.value()), "");

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| editor.redo(&Redo, window, cx));
        });
        assert_eq!(editor.read_with(cx, |editor, _| editor.value()), "abc");
    }

    #[gpui::test]
    fn shaping_generation_tracks_content_and_language_but_not_selection(
        cx: &mut gpui::TestAppContext,
    ) {
        let (editor, cx) = with_editor(cx);
        let generation = |cx: &mut gpui::VisualTestContext| {
            editor.read_with(cx, |editor, _| editor.layout_generation)
        };

        let initial = generation(cx);
        type_text(&editor, "fn main() {}", cx);
        let after_edit = generation(cx);
        assert_ne!(initial, after_edit);

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_language("rust", window, cx);
            });
        });
        let after_language = generation(cx);
        assert_ne!(after_edit, after_language);

        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_selected_range(0..2, cx);
            });
        });
        assert_eq!(after_language, generation(cx));
    }

    #[gpui::test]
    fn selection_and_basic_movement_follow_rope_lines(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = with_editor(cx);
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_value("one\ntwo", window, cx);
                editor.set_selected_range(1..3, cx);
                editor.right(&MoveRight, window, cx);
                assert_eq!(editor.cursor(), 3);
                editor.down(&MoveDown, window, cx);
                assert_eq!(editor.cursor(), 7);
                editor.select_to_start(&SelectToStart, window, cx);
                assert_eq!(editor.selected_range(), 0..7);
            });
        });
    }
}
