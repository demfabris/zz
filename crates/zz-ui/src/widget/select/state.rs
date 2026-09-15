//! The select's entity: what is picked, and whether the menu is open.

use gpui::{
    Anchor, AnyElement, App, Bounds, Context, DismissEvent, Entity, EventEmitter, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, Length, MouseButton, ParentElement as _,
    Pixels, Render, SharedString, StyleRefinement, Styled as _, Subscription, Window, anchored,
    canvas, deferred, div, px,
};

use crate::{
    ActiveTheme as _, Colorize as _, Disableable as _, Icon, IconName, IndexPath, Selectable as _,
    Sizable as _, Size, StyledExt as _,
    button::Button,
    h_flex,
    menu::{PopupMenu, PopupMenuItem},
};

use super::{
    actions::{Cancel, Confirm, SelectNext, SelectPrev},
    delegate::{SelectDelegate, SelectItem},
};

const WINDOW_MARGIN: Pixels = px(8.);

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
    menu: Option<Entity<PopupMenu>>,
    trigger_bounds: Bounds<Pixels>,
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
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        let selected = selected_index.and_then(|ix| delegate.item(ix.row).cloned());

        Self {
            focus_handle,
            delegate,
            selected,
            menu: None,
            trigger_bounds: Bounds::default(),
            options: SelectOptions::default(),
            empty: None,
            _subscriptions: Vec::new(),
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
            self.close_menu(cx);
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
        cx: &mut Context<Self>,
    ) {
        if options.disabled {
            self.close_menu(cx);
        }
        self.options = options;

        if empty.is_some() {
            self.empty = empty;
        }
    }

    fn open_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.is_some() || self.options.disabled {
            return;
        }

        let selected_index = self
            .selected
            .as_ref()
            .and_then(|item| self.delegate.position(item.value()));
        let state = cx.entity().downgrade();
        let menu = PopupMenu::build(window, cx, |menu, window, cx| {
            let mut menu = menu.scrollable(true).min_w(self.trigger_bounds.size.width);
            if let Some(Length::Definite(height)) = self.options.menu_max_h {
                menu = menu.max_h(
                    height.to_pixels(window.viewport_size().height.into(), window.rem_size()),
                );
            }
            for ix in 0..self.delegate.items_count() {
                let Some(item) = self.delegate.item(ix) else {
                    continue;
                };
                let state = state.clone();
                menu = menu.item(
                    PopupMenuItem::new(item.title())
                        .checked(selected_index == Some(ix))
                        .on_click(move |_, window, cx| {
                            _ = state.update(cx, |state, cx| state.commit(ix, window, cx));
                        }),
                );
            }
            if self.delegate.items_count() == 0 {
                menu = menu.item(
                    PopupMenuItem::element(move |_, window, cx| {
                        state.upgrade().map_or_else(
                            || div().into_any_element(),
                            |state| state.read(cx).render_empty(window, cx),
                        )
                    })
                    .disabled(true),
                );
            }
            menu.set_previous_focus(window.focused(cx), cx);
            if let Some(ix) = selected_index {
                menu.set_selected_index(ix, cx);
            }
            menu
        });
        let focus = menu.focus_handle(cx);
        self._subscriptions = vec![
            cx.subscribe_in(&menu, window, |this, _, _: &DismissEvent, _, cx| {
                this.close_menu(cx);
            }),
            cx.on_blur(&focus, window, |this, _, cx| this.close_menu(cx)),
        ];
        self.menu = Some(menu);
        focus.focus(window, cx);
        cx.notify();
    }

    fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            self._subscriptions.clear();
            cx.notify();
        }
    }

    fn commit(&mut self, ix: usize, _: &mut Window, cx: &mut Context<Self>) {
        if self.options.disabled {
            return;
        }
        let Some(item) = self.delegate.item(ix).cloned() else {
            return;
        };

        let value = item.value().clone();
        self.selected = Some(item);
        self.close_menu(cx);
        cx.emit(SelectEvent::Confirm(Some(value)));
        cx.notify();
    }

    pub(super) fn on_select_prev(
        &mut self,
        _: &SelectPrev,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_menu(window, cx);
    }

    pub(super) fn on_select_next(
        &mut self,
        _: &SelectNext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_menu(window, cx);
    }

    pub(super) fn on_confirm(&mut self, _: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
        if self.options.disabled {
            cx.propagate();
            return;
        }

        self.open_menu(window, cx);
    }

    pub(super) fn on_cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.is_none() {
            cx.propagate();
            return;
        }

        cx.stop_propagation();
        self.close_menu(cx);
        self.focus_handle.focus(window, cx);
    }
}

impl<D: SelectDelegate> SelectState<D> {
    fn render_trigger(&self, cx: &mut Context<Self>) -> AnyElement {
        let entity = cx.entity();
        let title = self
            .selected
            .as_ref()
            .map(SelectItem::title)
            .or_else(|| self.options.placeholder.clone())
            .unwrap_or_else(|| SharedString::new_static("Select"));
        let open = self.menu.is_some();
        let button = Button::new("select-trigger")
            .tab_stop(false)
            .w_full()
            .with_size(self.options.size)
            .label(title)
            .dropdown_caret(true)
            .disabled(self.options.disabled)
            .selected(self.menu.is_some())
            .refine_style(&self.options.style)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    if open {
                        this.close_menu(cx);
                    } else {
                        this.open_menu(window, cx);
                    }
                }),
            );
        div()
            .relative()
            .child(button)
            .child(
                canvas(
                    move |bounds, _, cx| {
                        entity.update(cx, |this, _| this.trigger_bounds = bounds);
                    },
                    |_, (), _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .into_any_element()
    }

    fn render_menu(&self) -> Option<AnyElement> {
        let menu = self.menu.clone()?;
        Some(
            deferred(
                anchored()
                    .anchor(Anchor::TopRight)
                    .position(self.trigger_bounds.bottom_right())
                    .snap_to_window_with_margin(WINDOW_MARGIN)
                    .child(div().mt_1().child(menu)),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    fn render_empty(&self, window: &mut Window, cx: &App) -> AnyElement {
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let trigger = self.render_trigger(cx);
        let menu = self.render_menu();

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

    struct Preview {
        state: gpui::Entity<SelectState<Vec<String>>>,
        disabled: bool,
    }

    impl gpui::Render for Preview {
        fn render(
            &mut self,
            _: &mut gpui::Window,
            _: &mut gpui::Context<Self>,
        ) -> impl gpui::IntoElement {
            use crate::Sizable as _;
            use gpui::{ParentElement as _, Styled as _};
            gpui::div().w(gpui::px(200.0)).child(
                super::super::Select::new(&self.state)
                    .small()
                    .disabled(self.disabled),
            )
        }
    }

    #[gpui::test]
    fn popup_picker_preserves_pointer_keyboard_and_disabled_behavior(cx: &mut TestAppContext) {
        use gpui::Focusable as _;
        use std::{cell::RefCell, rc::Rc};
        cx.update(|cx| {
            crate::init(cx);
            cx.set_reduce_motion(true);
        });
        let (preview, cx) = cx.add_window_view(|window, cx| Preview {
            state: cx.new(|cx| {
                SelectState::new(
                    vec!["vi".into(), "emacs".into()],
                    Some(IndexPath::new(1)),
                    window,
                    cx,
                )
            }),
            disabled: false,
        });
        let state = preview.read_with(cx, |preview, _| preview.state.clone());
        let events = Rc::new(RefCell::new(Vec::new()));
        let captured = events.clone();
        let _subscription = cx.update(|window, cx| {
            window.subscribe(
                &state,
                cx,
                move |_, event: &super::SelectEvent<Vec<String>>, _, _| {
                    let super::SelectEvent::Confirm(value) = event;
                    captured.borrow_mut().push(value.clone());
                },
            )
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        let bounds = state.read_with(cx, |state, _| state.trigger_bounds);
        cx.simulate_mouse_move(bounds.center(), None, gpui::Modifiers::default());
        cx.simulate_click(bounds.center(), gpui::Modifiers::default());
        assert!(state.read_with(cx, |state, _| state.menu.is_some()));
        cx.simulate_click(bounds.center(), gpui::Modifiers::default());
        assert!(state.read_with(cx, |state, _| state.menu.is_none()));
        cx.simulate_click(bounds.center(), gpui::Modifiers::default());
        cx.simulate_keystrokes("up enter");
        assert_eq!(
            state.read_with(cx, |state, _| state.selected_value().cloned()),
            Some("vi".into())
        );
        assert_eq!(*events.borrow(), vec![Some("vi".into())]);
        assert!(state.read_with(cx, |state, _| state.menu.is_none()));
        cx.simulate_click(bounds.center(), gpui::Modifiers::default());
        cx.simulate_keystrokes("down escape");
        assert_eq!(events.borrow().len(), 1);
        assert!(state.read_with(cx, |state, _| state.menu.is_none()));
        cx.update(|window, cx| state.focus_handle(cx).focus(window, cx));
        cx.simulate_keystrokes("down down enter");
        assert_eq!(
            state.read_with(cx, |state, _| state.selected_value().cloned()),
            Some("emacs".into())
        );
        assert_eq!(events.borrow().len(), 2);
        cx.simulate_click(bounds.center(), gpui::Modifiers::default());
        cx.update(|window, cx| {
            state.update(cx, |state, cx| {
                state.set_selected_index(Some(IndexPath::new(0)), window, cx);
            });
        });
        assert!(state.read_with(cx, |state, _| state.menu.is_none()));
        assert_eq!(events.borrow().len(), 2);
        cx.simulate_click(bounds.center(), gpui::Modifiers::default());
        preview.update(cx, |preview, cx| {
            preview.disabled = true;
            cx.notify();
        });
        cx.run_until_parked();
        cx.simulate_click(bounds.center(), gpui::Modifiers::default());
        assert!(state.read_with(cx, |state, _| state.menu.is_none()));
    }

    #[gpui::test]
    fn long_popup_picker_reopens_at_the_committed_value(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::init(cx);
            cx.set_reduce_motion(true);
        });
        let (preview, cx) = cx.add_window_view(|window, cx| Preview {
            state: cx.new(|cx| {
                SelectState::new(
                    (0..1000).map(|ix| format!("Font {ix}")).collect(),
                    Some(IndexPath::new(999)),
                    window,
                    cx,
                )
            }),
            disabled: false,
        });
        let state = preview.read_with(cx, |preview, _| preview.state.clone());
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        let bounds = state.read_with(cx, |state, _| state.trigger_bounds);
        cx.simulate_click(bounds.center(), gpui::Modifiers::default());
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            use crate::ActiveTheme as _;
            _ = window.draw(cx);
            let quads = window.painted_quads();
            let selected = quads
                .iter()
                .find(|quad| {
                    quad.background == gpui::solid_background(cx.theme().selection_background())
                })
                .expect("selected font row");
            assert!(
                selected
                    .content_mask
                    .bounds
                    .contains(&selected.bounds.center())
            );
        });
        cx.simulate_keystrokes("enter");
        assert_eq!(
            state.read_with(cx, |state, _| state.selected_value().cloned()),
            Some("Font 999".into())
        );
        cx.simulate_click(bounds.center(), gpui::Modifiers::default());
        cx.simulate_keystrokes("down enter");
        assert_eq!(
            state.read_with(cx, |state, _| state.selected_value().cloned()),
            Some("Font 0".into())
        );
    }

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
