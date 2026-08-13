//! Paint-scoped globals for text selection: the `TextView` stack and the
//! press-suppression flag.

use gpui::{App, Entity, Global};

use super::{state::TextViewState, window_selection::SelectionScope};

pub(super) fn init(cx: &mut App) {
    cx.set_global(TextGlobal::default());
}

#[derive(Default)]
pub(super) struct TextGlobal {
    view_stack: Vec<Entity<TextViewState>>,
    suppressed: bool,
}

impl Global for TextGlobal {}

/// Suppress window text selection for the current mouse down. Call from the
/// bubble-phase mouse-down handler of a widget that owns its own press.
///
/// ```ignore
/// .on_mouse_down(MouseButton::Left, |_, _, cx| {
///     zz_ui::text::suppress_text_selection(cx);
/// })
/// ```
pub fn suppress_text_selection(cx: &mut App) {
    TextGlobal::get_mut(cx).suppressed = true;
}

impl TextGlobal {
    pub(super) fn get(cx: &App) -> &Self {
        cx.global::<Self>()
    }

    pub(super) fn get_mut(cx: &mut App) -> &mut Self {
        cx.global_mut::<Self>()
    }

    pub(super) fn current_view(cx: &App) -> Option<&Entity<TextViewState>> {
        Self::get(cx).view_stack.last()
    }

    pub(super) fn push_view(cx: &mut App, state: Entity<TextViewState>) {
        Self::get_mut(cx).view_stack.push(state);
    }

    pub(super) fn pop_view(cx: &mut App) {
        Self::get_mut(cx).view_stack.pop();
    }

    pub(super) fn current_scope(_cx: &App) -> SelectionScope {
        SelectionScope::Base
    }

    pub(super) fn is_suppressed(cx: &App) -> bool {
        Self::get(cx).suppressed
    }

    pub(super) fn clear_suppressed(cx: &mut App) {
        Self::get_mut(cx).suppressed = false;
    }
}
