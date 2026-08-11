use gpui::{
    App, Entity, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, RenderOnce,
    StyleRefinement, Styled, Window, div, prelude::FluentBuilder as _,
};

use crate::{ActiveTheme as _, Colorize as _, StyledExt as _};

use super::{
    CodeEditorState,
    state::{CONTEXT, VIM_CONTEXT},
};

/// A full-height editor element bound to a [`CodeEditorState`].
#[derive(IntoElement)]
pub struct CodeEditor {
    state: Entity<CodeEditorState>,
    style: StyleRefinement,
    bordered: bool,
    focus_bordered: bool,
    disabled: bool,
    tab_index: isize,
}

impl CodeEditor {
    pub fn new(state: &Entity<CodeEditorState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
            bordered: false,
            focus_bordered: false,
            disabled: false,
            tab_index: 0,
        }
    }

    #[must_use]
    pub fn bordered(mut self, bordered: bool) -> Self {
        self.bordered = bordered;
        self
    }

    #[must_use]
    pub fn focus_bordered(mut self, bordered: bool) -> Self {
        self.focus_bordered = bordered;
        self
    }

    #[must_use]
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    #[must_use]
    pub fn tab_index(mut self, tab_index: isize) -> Self {
        self.tab_index = tab_index;
        self
    }
}

impl Styled for CodeEditor {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for CodeEditor {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.state.update(cx, |state, _| {
            state.disabled = self.disabled;
        });
        let focus_handle = self.state.read(cx).focus_handle_ref().clone();
        let focused = focus_handle.is_focused(window) && !self.disabled;
        let key_context = if self.state.read(cx).vim_mode().is_some() {
            VIM_CONTEXT
        } else {
            CONTEXT
        };
        let border = if focused && self.focus_bordered {
            cx.theme().foreground.outline()
        } else {
            cx.theme().border
        };

        div()
            .id(("code-editor", self.state.entity_id()))
            .refine_style(&self.style)
            .relative()
            .flex()
            .size_full()
            .min_w_0()
            .overflow_hidden()
            .key_context(key_context)
            .track_focus(&focus_handle)
            .tab_index(self.tab_index)
            .when(self.bordered || self.focus_bordered, |this| {
                this.border_1().border_color(border)
            })
            .when(!self.disabled, |this| {
                this.on_action(window.listener_for(&self.state, CodeEditorState::backspace))
                    .on_action(window.listener_for(&self.state, CodeEditorState::delete))
                    .on_action(
                        window.listener_for(
                            &self.state,
                            CodeEditorState::delete_to_beginning_of_line,
                        ),
                    )
                    .on_action(
                        window.listener_for(&self.state, CodeEditorState::delete_to_end_of_line),
                    )
                    .on_action(
                        window.listener_for(&self.state, CodeEditorState::delete_previous_word),
                    )
                    .on_action(window.listener_for(&self.state, CodeEditorState::delete_next_word))
                    .on_action(window.listener_for(&self.state, CodeEditorState::enter))
                    .on_action(window.listener_for(&self.state, CodeEditorState::indent_inline))
                    .on_action(window.listener_for(&self.state, CodeEditorState::outdent_inline))
                    .on_action(window.listener_for(&self.state, CodeEditorState::indent_block))
                    .on_action(window.listener_for(&self.state, CodeEditorState::outdent_block))
                    .on_action(window.listener_for(&self.state, CodeEditorState::cut))
                    .on_action(window.listener_for(&self.state, CodeEditorState::paste))
                    .on_action(window.listener_for(&self.state, CodeEditorState::undo))
                    .on_action(window.listener_for(&self.state, CodeEditorState::redo))
            })
            .on_action(window.listener_for(&self.state, CodeEditorState::left))
            .on_action(window.listener_for(&self.state, CodeEditorState::right))
            .on_action(window.listener_for(&self.state, CodeEditorState::up))
            .on_action(window.listener_for(&self.state, CodeEditorState::down))
            .on_action(window.listener_for(&self.state, CodeEditorState::page_up))
            .on_action(window.listener_for(&self.state, CodeEditorState::page_down))
            .on_action(window.listener_for(&self.state, CodeEditorState::home))
            .on_action(window.listener_for(&self.state, CodeEditorState::end))
            .on_action(window.listener_for(&self.state, CodeEditorState::move_to_start))
            .on_action(window.listener_for(&self.state, CodeEditorState::move_to_end))
            .on_action(window.listener_for(&self.state, CodeEditorState::move_to_previous_word))
            .on_action(window.listener_for(&self.state, CodeEditorState::move_to_next_word))
            .on_action(window.listener_for(&self.state, CodeEditorState::select_left))
            .on_action(window.listener_for(&self.state, CodeEditorState::select_right))
            .on_action(window.listener_for(&self.state, CodeEditorState::select_up))
            .on_action(window.listener_for(&self.state, CodeEditorState::select_down))
            .on_action(window.listener_for(&self.state, CodeEditorState::select_to_start))
            .on_action(window.listener_for(&self.state, CodeEditorState::select_to_end))
            .on_action(window.listener_for(&self.state, CodeEditorState::select_to_start_of_line))
            .on_action(window.listener_for(&self.state, CodeEditorState::select_to_end_of_line))
            .on_action(window.listener_for(&self.state, CodeEditorState::select_to_previous_word))
            .on_action(window.listener_for(&self.state, CodeEditorState::select_to_next_word))
            .on_action(window.listener_for(&self.state, CodeEditorState::select_all))
            .on_action(window.listener_for(&self.state, CodeEditorState::copy))
            .on_action(window.listener_for(&self.state, CodeEditorState::escape))
            .on_action(window.listener_for(&self.state, CodeEditorState::show_character_palette))
            .on_action(window.listener_for(&self.state, CodeEditorState::vim_half_page_down))
            .on_action(window.listener_for(&self.state, CodeEditorState::vim_half_page_up))
            .on_action(window.listener_for(&self.state, CodeEditorState::vim_page_down))
            .on_action(window.listener_for(&self.state, CodeEditorState::vim_page_up))
            .on_action(window.listener_for(&self.state, CodeEditorState::vim_redo))
            .on_key_down(window.listener_for(&self.state, CodeEditorState::on_key_down))
            .on_mouse_down(
                MouseButton::Left,
                window.listener_for(&self.state, CodeEditorState::on_mouse_down),
            )
            .on_mouse_down(
                MouseButton::Right,
                window.listener_for(&self.state, CodeEditorState::on_mouse_down),
            )
            .on_scroll_wheel(window.listener_for(&self.state, CodeEditorState::on_scroll_wheel))
            .child(self.state)
    }
}
