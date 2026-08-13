//! The select's entity: what is picked, and whether the menu is open.

use std::ops::Range;

use crate::StyleSized as _;
use gpui::{
    AnyElement, App, Bounds, ClickEvent, Context, Div, EventEmitter, FocusHandle, Focusable, Hsla,
    InteractiveElement as _, IntoElement, Length, ListSizingBehavior, ParentElement as _, Pixels,
    Render, ScrollStrategy, SharedString, StatefulInteractiveElement as _, StyleRefinement,
    Styled as _, Subscription, UniformListScrollHandle, Window, anchored, canvas, deferred, div,
    prelude::FluentBuilder as _, px, rems, uniform_list,
};

use crate::{
    ActiveTheme as _, Colorize as _, Icon, IconName, IndexPath, Sizable as _, Size, StyledExt as _,
    control_shadow, h_flex, scroll::ScrollableElement as _, v_flex,
};

use super::{
    actions::{Cancel, Confirm, SelectNext, SelectPrev},
    delegate::{SelectDelegate, SelectItem},
};

const WINDOW_MARGIN: Pixels = px(8.);

const CARET_SIZE: Pixels = px(12.);

pub(super) type EmptyBuilder = Box<dyn Fn(&mut Window, &App) -> AnyElement + 'static>;

#[derive(Default)]
pub(super) struct SelectOptions {
    pub(super) style: StyleRefinement,
    pub(super) size: Size,
    pub(super) placeholder: Option<SharedString>,
    pub(super) menu_max_h: Option<Length>,
    pub(super) disabled: bool,
}

/// What a [`SelectState`] reports to its subscribers.
pub enum SelectEvent<D: SelectDelegate> {
    /// A row was picked from the menu; always `Some`.
    Confirm(Option<<D::Item as SelectItem>::Value>),
}

/// The state behind one [`super::Select`]: build it inside `cx.new`, hand the
/// entity to the element every render.
pub struct SelectState<D: SelectDelegate> {
    focus_handle: FocusHandle,
    delegate: D,
    selected: Option<D::Item>,
    cursor: Option<usize>,
    open: bool,
    trigger_bounds: Bounds<Pixels>,
    scroll: UniformListScrollHandle,
    options: SelectOptions,
    empty: Option<EmptyBuilder>,
    _subscriptions: Vec<Subscription>,
}

impl<D: SelectDelegate> SelectState<D> {
    /// A select over `delegate`, optionally starting on a row. Only
    /// `selected_index.row` is read; an out-of-range row starts it empty.
    #[must_use]
    pub fn new(
        delegate: D,
        selected_index: Option<IndexPath>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        let selected = selected_index.and_then(|ix| delegate.item(ix.row).cloned());

        let subscriptions = vec![cx.on_blur(&focus_handle, window, Self::on_blur)];

        Self {
            focus_handle,
            delegate,
            selected,
            cursor: None,
            open: false,
            trigger_bounds: Bounds::default(),
            scroll: UniformListScrollHandle::new(),
            options: SelectOptions::default(),
            empty: None,
            _subscriptions: subscriptions,
        }
    }

    #[must_use]
    pub fn selected_value(&self) -> Option<&<D::Item as SelectItem>::Value> {
        self.selected.as_ref().map(SelectItem::value)
    }

    /// Commit the row at `selected_index`, or clear the selection with `None`.
    /// Emits no [`SelectEvent::Confirm`], and notifies only when the pick moves.
    pub fn set_selected_index(
        &mut self,
        selected_index: Option<IndexPath>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let next = selected_index.and_then(|ix| self.delegate.item(ix.row).cloned());
        let changed =
            self.selected.as_ref().map(SelectItem::value) != next.as_ref().map(SelectItem::value);

        self.selected = next;

        if changed {
            cx.notify();
        }
    }

    /// Commit the row carrying `value`, clearing the selection when the
    /// delegate has no such row.
    pub fn set_selected_value(
        &mut self,
        value: &<D::Item as SelectItem>::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let index = self.delegate.position(value).map(IndexPath::new);
        self.set_selected_index(index, window, cx);
    }

    pub(super) fn apply(
        &mut self,
        options: SelectOptions,
        empty: Option<EmptyBuilder>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
        self.options = options;

        if empty.is_some() {
            self.empty = empty;
        }
    }

    fn open_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open || self.options.disabled {
            return;
        }

        self.open = true;
        self.cursor = self
            .selected
            .as_ref()
            .and_then(|item| self.delegate.position(item.value()));

        if let Some(ix) = self.cursor {
            self.scroll.scroll_to_item(ix, ScrollStrategy::Center);
        }

        self.focus_handle.focus(window, cx);

        cx.notify();
    }

    fn close_menu(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }

        self.open = false;
        self.cursor = None;

        cx.notify();
    }

    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menu(window, cx);
        self.focus_handle.focus(window, cx);
    }

    fn move_cursor(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.options.disabled {
            return;
        }

        if !self.open {
            self.open_menu(window, cx);
            return;
        }

        let count = self.delegate.items_count();
        let Some(last) = count.checked_sub(1) else {
            self.cursor = None;
            return;
        };

        let next = match self.cursor {
            None if forward => 0,
            Some(ix) if forward => {
                if ix >= last {
                    0
                } else {
                    ix + 1
                }
            }
            None | Some(0) => last,
            Some(ix) => ix - 1,
        };

        self.cursor = Some(next);
        self.scroll.scroll_to_item(next, ScrollStrategy::Nearest);
        cx.notify();
    }

    fn commit(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.delegate.item(ix).cloned() else {
            return;
        };

        let value = item.value().clone();
        self.selected = Some(item);
        self.dismiss(window, cx);
        cx.emit(SelectEvent::Confirm(Some(value)));
        cx.notify();
    }

    fn on_blur(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menu(window, cx);
    }

    fn on_trigger_click(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();

        if self.open {
            self.dismiss(window, cx);
        } else {
            self.open_menu(window, cx);
        }
    }

    pub(super) fn on_select_prev(
        &mut self,
        _: &SelectPrev,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_cursor(false, window, cx);
    }

    pub(super) fn on_select_next(
        &mut self,
        _: &SelectNext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_cursor(true, window, cx);
    }

    pub(super) fn on_confirm(&mut self, _: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
        if self.options.disabled {
            cx.propagate();
            return;
        }

        if !self.open {
            self.open_menu(window, cx);
            return;
        }

        if let Some(ix) = self.cursor {
            self.commit(ix, window, cx);
        }
    }

    pub(super) fn on_cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            cx.propagate();
            return;
        }

        cx.stop_propagation();
        self.dismiss(window, cx);
    }
}

fn trigger_colors(disabled: bool, cx: &App) -> (Hsla, Hsla) {
    if disabled {
        (
            cx.theme().border.mix_oklab(cx.theme().transparent, 0.8),
            cx.theme().foreground.muted(),
        )
    } else {
        (
            cx.theme().background.raised(1).opaque(),
            cx.theme().foreground,
        )
    }
}

impl<D: SelectDelegate> SelectState<D> {
    fn render_trigger(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let disabled = self.options.disabled;
        let outlined = self.open || (!disabled && self.focus_handle.is_focused(window));
        let (background, foreground) = trigger_colors(disabled, cx);
        let entity = cx.entity();

        div()
            .id("select-trigger")
            .relative()
            .flex()
            .items_center()
            .justify_between()
            .overflow_hidden()
            .bg(background)
            .text_color(foreground)
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius)
            .when(cx.theme().shadow && !disabled, |this| {
                this.shadow(control_shadow(cx))
            })
            .when(disabled, |this| this.opacity(0.5))
            .input_size(self.options.size)
            .input_text_size(self.options.size)
            .refine_style(&self.options.style)
            .when(outlined, |this| this.focused_border(cx))
            .when(!self.open && !disabled, |this| {
                this.on_click(cx.listener(Self::on_trigger_click))
            })
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap_1()
                    .child(div().flex_none().size(CARET_SIZE))
                    .child(
                        div()
                            .w_full()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .truncate()
                            .text_center()
                            .child(self.render_title(cx)),
                    )
                    .child(
                        Icon::new(IconName::ChevronDown)
                            .size(CARET_SIZE)
                            .text_color(cx.theme().foreground.muted()),
                    ),
            )
            .child(
                canvas(
                    move |bounds, _, cx| {
                        entity.update(cx, |this, _| this.trigger_bounds = bounds);
                    },
                    |_, (), _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .into_any_element()
    }

    fn render_title(&self, cx: &App) -> Div {
        let muted = div().text_color(cx.theme().foreground.muted());

        let Some(item) = self.selected.as_ref() else {
            return muted.child(
                self.options
                    .placeholder
                    .clone()
                    .unwrap_or_else(|| SharedString::new_static("Select")),
            );
        };

        if self.options.disabled {
            muted.child(item.title())
        } else {
            div().child(item.title())
        }
    }

    fn render_menu(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let body = self.render_body(window, cx);

        deferred(
            anchored().snap_to_window_with_margin(WINDOW_MARGIN).child(
                div()
                    .id("select-menu")
                    .occlude()
                    .w(self.trigger_bounds.size.width + px(2.))
                    .child(
                        v_flex()
                            .occlude()
                            .mt_1p5()
                            .overflow_hidden()
                            .popover_style(cx)
                            .rounded(cx.theme().radius)
                            .child(body),
                    )
                    .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                        this.dismiss(window, cx);
                    })),
            ),
        )
        .with_priority(1)
        .into_any_element()
    }

    fn render_body(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let count = self.delegate.items_count();
        if count == 0 {
            return self.render_empty(window, cx);
        }

        let max_h = self.options.menu_max_h.unwrap_or_else(|| rems(20.).into());
        let rows = uniform_list(
            "select-rows",
            count,
            cx.processor(|this, range: Range<usize>, _, cx| {
                range
                    .filter_map(|ix| this.render_row(ix, cx))
                    .collect::<Vec<_>>()
            }),
        )
        .w_full()
        .with_sizing_behavior(ListSizingBehavior::Infer)
        .track_scroll(&self.scroll);

        v_flex()
            .relative()
            .p_1()
            .max_h(max_h)
            .child(rows)
            .vertical_scrollbar(&self.scroll)
            .into_any_element()
    }

    fn render_row(&self, ix: usize, cx: &mut Context<Self>) -> Option<AnyElement> {
        let item = self.delegate.item(ix)?;
        let title = item.title();
        let checked = self
            .selected
            .as_ref()
            .is_some_and(|selected| selected.value() == item.value());
        let highlighted = self.cursor == Some(ix);

        Some(
            h_flex()
                .id(("select-row", ix))
                .w_full()
                .items_center()
                .justify_between()
                .gap_x_1()
                .rounded(cx.theme().radius)
                .list_size(self.options.size)
                .text_color(cx.theme().foreground)
                .when(!highlighted, |this| {
                    this.hover(|this| this.bg(cx.theme().background.raised(2).opacity(0.7)))
                })
                .when(highlighted, |this| this.bg(cx.theme().background.raised(2)))
                .on_click(cx.listener(move |this, _, window, cx| this.commit(ix, window, cx)))
                .child(div().truncate().child(title))
                .child(
                    Icon::new(IconName::Check)
                        .xsmall()
                        .when(!checked, gpui::Styled::invisible),
                )
                .into_any_element(),
        )
    }

    fn render_empty(&self, window: &mut Window, cx: &mut App) -> AnyElement {
        if let Some(empty) = self.empty.as_ref() {
            return empty(window, cx);
        }

        h_flex()
            .justify_center()
            .py_6()
            .text_color(cx.theme().foreground.muted().opacity(0.6))
            .child(Icon::new(IconName::Inbox).size(px(28.)))
            .into_any_element()
    }
}

impl<D: SelectDelegate> Render for SelectState<D> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.open;
        let trigger = self.render_trigger(window, cx);
        let menu = open.then(|| self.render_menu(window, cx));

        div().relative().child(trigger).children(menu)
    }
}

impl<D: SelectDelegate> Focusable for SelectState<D> {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl<D: SelectDelegate> EventEmitter<SelectEvent<D>> for SelectState<D> {}

#[cfg(test)]
mod tests {
    use gpui::{AppContext as _, TestAppContext};

    use super::{IndexPath, SelectState};

    #[gpui::test]
    fn initial_index_seeds_the_committed_value(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let cx = cx.add_empty_window();

        cx.update(|window, cx| {
            let state = cx.new(|cx| {
                SelectState::new(
                    vec!["Rust", "Go", "C++"],
                    Some(IndexPath::new(1)),
                    window,
                    cx,
                )
            });

            assert_eq!(state.read(cx).selected_value(), Some(&"Go"));
        });
    }

    #[gpui::test]
    fn out_of_range_initial_index_starts_empty(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let cx = cx.add_empty_window();

        cx.update(|window, cx| {
            let state =
                cx.new(|cx| SelectState::new(vec!["Rust"], Some(IndexPath::new(9)), window, cx));

            assert_eq!(state.read(cx).selected_value(), None);
        });
    }

    #[gpui::test]
    fn set_selected_value_resolves_through_the_delegate(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let cx = cx.add_empty_window();

        cx.update(|window, cx| {
            let state = cx.new(|cx| SelectState::new(vec!["Rust", "Go"], None, window, cx));

            state.update(cx, |this, cx| {
                this.set_selected_value(&"Go", window, cx);
                assert_eq!(this.selected_value(), Some(&"Go"));

                this.set_selected_value(&"Zig", window, cx);
                assert_eq!(this.selected_value(), None);
            });
        });
    }
}
