//! The select's entity: what is picked, and whether the menu is open.

use zpui::{
    Anchor, AnyElement, App, AppContext as _, Bounds, Context, DismissEvent, Entity, EventEmitter,
    FocusHandle, Focusable, InteractiveElement as _, IntoElement, Length, MouseButton,
    ParentElement as _, Pixels, Render, SharedString, StatefulInteractiveElement as _,
    StyleRefinement, Styled as _, Subscription, Window, anchored, canvas, deferred, div,
    prelude::FluentBuilder as _, px,
};

use crate::{
    ActiveTheme as _, Colorize as _, Disableable as _, Icon, IconName, IndexPath, Selectable as _,
    Sizable as _, Size, StyledExt as _,
    button::Button,
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{PopupMenu, PopupMenuItem},
    sheet::{bottom_sheet, floating_sheet, sheet_close, sheet_inset, sheet_option},
};

use super::{
    actions::{Cancel, Confirm, SelectNext, SelectPrev},
    delegate::{SelectDelegate, SelectItem},
};

const WINDOW_MARGIN: Pixels = px(8.);
const SEARCH_FROM: usize = 12;

pub(super) type EmptyBuilder = Box<dyn Fn(&mut Window, &App) -> AnyElement + 'static>;

#[derive(Default)]
pub(super) struct SelectOptions {
    pub(super) style: StyleRefinement,
    pub(super) size: Size,
    pub(super) placeholder: Option<SharedString>,
    pub(super) title: Option<SharedString>,
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
    sheet: bool,
    search: Option<(Entity<InputState>, Subscription)>,
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
            sheet: false,
            search: None,
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
        if self.menu.is_some() || self.sheet || self.options.disabled {
            return;
        }
        if crate::touch::CoarsePointer::get(cx) {
            self.open_sheet(window, cx);
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

    fn open_sheet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.delegate.items_count() > SEARCH_FROM {
            if let Some((search, _)) = &self.search {
                search.update(cx, |search, cx| search.set_value("", window, cx));
            } else {
                let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
                let subscription = cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                });
                self.search = Some((search, subscription));
            }
        }
        self.sheet = true;
        cx.notify();
    }

    fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            self._subscriptions.clear();
            cx.notify();
        }
        if std::mem::take(&mut self.sheet) {
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
        if self.menu.is_none() && !self.sheet {
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
        let open = self.menu.is_some() || self.sheet;
        let slop = crate::touch::control_slop(self.options.size);
        let toggle = move |this: &mut Self,
                           _: &zpui::MouseDownEvent,
                           window: &mut Window,
                           cx: &mut Context<Self>| {
            cx.stop_propagation();
            if open {
                this.close_menu(cx);
            } else {
                this.open_menu(window, cx);
            }
        };
        let button = Button::new("select-trigger")
            .tab_stop(false)
            .with_size(self.options.size)
            .label(title)
            .dropdown_caret(true)
            .disabled(self.options.disabled)
            .selected(open)
            .refine_style(&self.options.style)
            .on_mouse_down(MouseButton::Left, cx.listener(toggle));
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
            .when(
                !self.options.disabled && slop > 0.0 && crate::touch::CoarsePointer::get(cx),
                |this| {
                    this.child(
                        crate::touch::hit_area("select-trigger-touch", 0.0, slop)
                            .on_mouse_down(MouseButton::Left, cx.listener(toggle)),
                    )
                },
            )
            .into_any_element()
    }

    fn render_sheet(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let query = self
            .search
            .as_ref()
            .map(|(search, _)| search.read(cx).value().to_lowercase())
            .unwrap_or_default();
        let selected = self.selected.as_ref().map(|item| item.value().clone());
        let rows = (0..self.delegate.items_count())
            .filter_map(|ix| {
                let item = self.delegate.item(ix)?;
                let title = item.title();
                if !query.is_empty() && !title.to_lowercase().contains(&query) {
                    return None;
                }
                Some(
                    sheet_option(
                        ("select-option", ix),
                        title,
                        selected.as_ref() == Some(item.value()),
                        cx,
                    )
                    .debug_selector(move || format!("select-option-{ix}"))
                    .when_some(item.font_family(), zpui::Styled::font_family)
                    .on_click(cx.listener(move |this, _, window, cx| this.commit(ix, window, cx))),
                )
            })
            .collect::<Vec<_>>();
        let title = self
            .options
            .title
            .clone()
            .or_else(|| self.options.placeholder.clone())
            .unwrap_or_else(|| SharedString::new_static("Select"));
        let dismiss = cx.entity().downgrade();
        let sheet = bottom_sheet(
            ("select-sheet", cx.entity_id()),
            title,
            [sheet_close("select-sheet-close")
                .on_click(cx.listener(|this, _, _, cx| this.close_menu(cx)))
                .into_any_element()],
            div()
                .flex()
                .flex_col()
                .pb(px(8.0))
                .children(self.search.as_ref().map(|(search, _)| {
                    div()
                        .px(px(16.0))
                        .pb(px(8.0))
                        .child(Input::new(search).small().cleanable(true))
                }))
                .child(div().flex().flex_col().px(px(8.0)).children(rows)),
            sheet_inset(window),
            move |_, cx| {
                _ = dismiss.update(cx, Self::close_menu);
            },
        );
        floating_sheet(sheet, window)
    }

    fn render_menu(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.sheet {
            return Some(self.render_sheet(window, cx));
        }
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let trigger = self.render_trigger(cx);
        let menu = self.render_menu(window, cx);

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
    use zpui::{AppContext as _, TestAppContext};

    use super::{IndexPath, SelectState};

    struct Preview {
        state: zpui::Entity<SelectState<Vec<String>>>,
        disabled: bool,
    }

    impl zpui::Render for Preview {
        fn render(
            &mut self,
            _: &mut zpui::Window,
            _: &mut zpui::Context<Self>,
        ) -> impl zpui::IntoElement {
            use crate::Sizable as _;
            use zpui::{ParentElement as _, Styled as _};
            zpui::div().flex().w(zpui::px(200.0)).child(
                super::super::Select::new(&self.state)
                    .small()
                    .disabled(self.disabled),
            )
        }
    }

    #[zpui::test]
    fn popup_picker_preserves_pointer_keyboard_and_disabled_behavior(cx: &mut TestAppContext) {
        use std::{cell::RefCell, rc::Rc};
        use zpui::Focusable as _;
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
        assert!(bounds.size.width < zpui::px(200.0));
        cx.simulate_mouse_move(bounds.center(), None, zpui::Modifiers::default());
        cx.simulate_click(bounds.center(), zpui::Modifiers::default());
        assert!(state.read_with(cx, |state, _| state.menu.is_some()));
        cx.simulate_click(bounds.center(), zpui::Modifiers::default());
        assert!(state.read_with(cx, |state, _| state.menu.is_none()));
        cx.simulate_click(bounds.center(), zpui::Modifiers::default());
        cx.simulate_keystrokes("up enter");
        assert_eq!(
            state.read_with(cx, |state, _| state.selected_value().cloned()),
            Some("vi".into())
        );
        assert_eq!(*events.borrow(), vec![Some("vi".into())]);
        assert!(state.read_with(cx, |state, _| state.menu.is_none()));
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        assert!(
            state.read_with(cx, |state, _| state.trigger_bounds.size.width) < bounds.size.width
        );
        cx.simulate_click(bounds.center(), zpui::Modifiers::default());
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
        cx.simulate_click(bounds.center(), zpui::Modifiers::default());
        cx.update(|window, cx| {
            state.update(cx, |state, cx| {
                state.set_selected_index(Some(IndexPath::new(0)), window, cx);
            });
        });
        assert!(state.read_with(cx, |state, _| state.menu.is_none()));
        assert_eq!(events.borrow().len(), 2);
        cx.simulate_click(bounds.center(), zpui::Modifiers::default());
        preview.update(cx, |preview, cx| {
            preview.disabled = true;
            cx.notify();
        });
        cx.run_until_parked();
        cx.simulate_click(bounds.center(), zpui::Modifiers::default());
        assert!(state.read_with(cx, |state, _| state.menu.is_none()));
    }

    #[zpui::test]
    fn long_popup_picker_reopens_at_the_committed_value(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::init(cx);
            cx.set_reduce_motion(true);
        });
        let (preview, cx) = cx.add_window_view(|window, cx| Preview {
            state: cx.new(|cx| {
                SelectState::new(
                    (0..1000)
                        .map(|ix| {
                            if ix == 500 {
                                "A much wider font family in the middle of the list".into()
                            } else {
                                format!("Font {ix}")
                            }
                        })
                        .collect(),
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
        cx.simulate_click(bounds.center(), zpui::Modifiers::default());
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
                    quad.background == zpui::solid_background(cx.theme().selection_background())
                })
                .expect("selected font row");
            assert!(
                selected.bounds.size.width.0
                    > f32::from(bounds.size.width) * window.scale_factor() * 2.0
            );
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
        cx.simulate_click(bounds.center(), zpui::Modifiers::default());
        cx.simulate_keystrokes("down enter");
        assert_eq!(
            state.read_with(cx, |state, _| state.selected_value().cloned()),
            Some("Font 0".into())
        );
    }

    #[zpui::test]
    fn a_touch_screen_picks_from_a_sheet(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::init(cx);
            cx.set_reduce_motion(true);
            crate::touch::CoarsePointer::set(true, cx);
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
        let draw = |cx: &mut zpui::VisualTestContext| {
            cx.run_until_parked();
            cx.update(|window, cx| {
                _ = window.draw(cx);
            });
        };
        draw(cx);
        let bounds = state.read_with(cx, |state, _| state.trigger_bounds);
        cx.simulate_click(bounds.center(), zpui::Modifiers::default());
        draw(cx);
        assert!(state.read_with(cx, |state, _| state.sheet && state.menu.is_none()));
        let row = cx
            .debug_bounds("select-option-0")
            .expect("the sheet lists the options");
        cx.simulate_click(row.center(), zpui::Modifiers::default());
        draw(cx);
        assert_eq!(
            state.read_with(cx, |state, _| state.selected_value().cloned()),
            Some("vi".into())
        );
        assert!(state.read_with(cx, |state, _| !state.sheet));
    }

    #[zpui::test]
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

    #[zpui::test]
    fn out_of_range_initial_index_starts_empty(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let cx = cx.add_empty_window();

        cx.update(|window, cx| {
            let state =
                cx.new(|cx| SelectState::new(vec!["Rust"], Some(IndexPath::new(9)), window, cx));

            assert_eq!(state.read(cx).selected_value(), None);
        });
    }

    #[zpui::test]
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
