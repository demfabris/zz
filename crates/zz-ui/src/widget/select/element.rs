//! The `RenderOnce` half of the select: the per-frame builder a call site writes.

use gpui::{
    App, ElementId, Entity, Focusable as _, InteractiveElement as _, IntoElement, Length,
    ParentElement as _, RenderOnce, SharedString, StyleRefinement, Styled, Window, div,
    prelude::FluentBuilder as _,
};

use crate::{Sizable, Size};

use super::{
    CONTEXT,
    delegate::SelectDelegate,
    state::{EmptyBuilder, SelectOptions, SelectState},
};

/// A dropdown bound to a [`SelectState`]. Build it fresh every render; the
/// pick and the open menu live in the state entity.
#[derive(IntoElement)]
pub struct Select<D: SelectDelegate> {
    id: ElementId,
    state: Entity<SelectState<D>>,
    options: SelectOptions,
    empty: Option<EmptyBuilder>,
}

impl<D: SelectDelegate> Select<D> {
    #[must_use]
    pub fn new(state: &Entity<SelectState<D>>) -> Self {
        Self {
            id: ("select", state.entity_id()).into(),
            state: state.clone(),
            options: SelectOptions::default(),
            empty: None,
        }
    }

    /// Text shown in the trigger while nothing is picked, default `"Select"`.
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.options.placeholder = Some(placeholder.into());
        self
    }

    /// Cap on the dropdown's height, default 20rem. Rows scroll past it.
    #[must_use]
    pub fn menu_max_h(mut self, max_h: impl Into<Length>) -> Self {
        self.options.menu_max_h = Some(max_h.into());
        self
    }

    #[must_use]
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.options.disabled = disabled;
        self
    }

    /// Replace the element shown when the delegate has no rows. The default is
    /// a muted inbox glyph.
    #[must_use]
    pub fn empty<E: IntoElement + 'static>(
        mut self,
        builder: impl Fn(&mut Window, &App) -> E + 'static,
    ) -> Self {
        self.empty = Some(Box::new(move |window, cx| {
            builder(window, cx).into_any_element()
        }));
        self
    }
}

impl<D: SelectDelegate> Styled for Select<D> {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.options.style
    }
}

impl<D: SelectDelegate> Sizable for Select<D> {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.options.size = size.into();
        self
    }
}

impl<D: SelectDelegate> RenderOnce for Select<D> {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self {
            id,
            state,
            options,
            empty,
        } = self;

        let disabled = options.disabled;
        let focus_handle = state.focus_handle(cx);

        state.update(cx, |this, cx| this.apply(options, empty, window, cx));

        div()
            .id(id)
            .key_context(CONTEXT)
            .flex_none()
            .when(!disabled, |this| {
                this.track_focus(&focus_handle.tab_stop(true))
            })
            .on_action(window.listener_for(&state, SelectState::on_select_prev))
            .on_action(window.listener_for(&state, SelectState::on_select_next))
            .on_action(window.listener_for(&state, SelectState::on_confirm))
            .on_action(window.listener_for(&state, SelectState::on_cancel))
            .child(state)
    }
}
