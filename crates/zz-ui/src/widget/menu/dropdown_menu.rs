use std::rc::Rc;

use gpui::{
    Anchor, Context, DismissEvent, ElementId, Entity, Focusable, InteractiveElement, IntoElement,
    RenderOnce, SharedString, StyleRefinement, Styled, Window,
};

use super::PopupMenu;
use crate::popover::Popover;
use crate::{Selectable, button::Button};

pub trait DropdownMenu: Styled + Selectable + InteractiveElement + IntoElement + 'static {
    fn dropdown_menu(
        self,
        f: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
    ) -> DropdownMenuPopover<Self> {
        self.dropdown_menu_with_anchor(Anchor::TopLeft, f)
    }

    fn dropdown_menu_with_anchor(
        mut self,
        anchor: impl Into<Anchor>,
        f: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
    ) -> DropdownMenuPopover<Self> {
        let style = self.style().clone();
        let id = self.interactivity().element_id.clone();

        DropdownMenuPopover::new(id.unwrap_or(0.into()), anchor, self, f).trigger_style(style)
    }
}

impl DropdownMenu for Button {}

#[derive(IntoElement)]
pub struct DropdownMenuPopover<T: Selectable + IntoElement + 'static> {
    id: ElementId,
    style: StyleRefinement,
    anchor: Anchor,
    trigger: T,
    builder: Rc<dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>,
}

impl<T> DropdownMenuPopover<T>
where
    T: Selectable + IntoElement + 'static,
{
    fn new(
        id: ElementId,
        anchor: impl Into<Anchor>,
        trigger: T,
        builder: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
    ) -> Self {
        Self {
            id: SharedString::from(format!("dropdown-menu:{:?}", id)).into(),
            style: StyleRefinement::default(),
            anchor: anchor.into(),
            trigger,
            builder: Rc::new(builder),
        }
    }

    pub fn anchor(mut self, anchor: impl Into<Anchor>) -> Self {
        self.anchor = anchor.into();
        self
    }

    fn trigger_style(mut self, style: StyleRefinement) -> Self {
        self.style = style;
        self
    }
}

#[derive(Default)]
struct DropdownMenuState {
    menu: Option<Entity<PopupMenu>>,
}

impl<T> RenderOnce for DropdownMenuPopover<T>
where
    T: Selectable + IntoElement + 'static,
{
    fn render(self, window: &mut Window, cx: &mut gpui::App) -> impl IntoElement {
        let builder = self.builder.clone();
        let menu_state =
            window.use_keyed_state(self.id.clone(), cx, |_, _| DropdownMenuState::default());

        Popover::new(SharedString::from(format!("popover:{}", self.id)))
            .appearance(false)
            .overlay_closable(false)
            .trigger(self.trigger)
            .trigger_style(self.style)
            .anchor(self.anchor)
            .content(move |_, window, cx| {
                let menu = match menu_state.read(cx).menu.clone() {
                    Some(menu) => menu,
                    None => {
                        let builder = builder.clone();
                        let menu = PopupMenu::build(window, cx, move |menu, window, cx| {
                            builder(menu, window, cx)
                        });
                        menu_state.update(cx, |state, _| {
                            state.menu = Some(menu.clone());
                        });
                        menu.focus_handle(cx).focus(window, cx);

                        let popover_state = cx.entity().downgrade();
                        window
                            .subscribe(&menu, cx, {
                                let menu_state = menu_state.downgrade();
                                move |_, _: &DismissEvent, window, cx| {
                                    _ = popover_state.update(cx, |state, cx| {
                                        state.dismiss(window, cx);
                                    });
                                    _ = menu_state.update(cx, |state, _| {
                                        state.menu = None;
                                    });
                                }
                            })
                            .detach();

                        menu.clone()
                    }
                };

                menu.clone()
            })
    }
}

#[cfg(test)]
mod tests {
    use gpui::{
        Modifiers, ParentElement as _, Render, TestAppContext, VisualTestContext, div, point, px,
    };

    use super::*;
    use crate::menu::PopupMenuItem;

    struct Host;

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                Button::new("more")
                    .label("More")
                    .dropdown_menu(|menu, _, _| menu.item(PopupMenuItem::new("Open"))),
            )
        }
    }

    #[gpui::test]
    fn closing_the_window_with_the_menu_open_releases_the_menu(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let before = cx.update(|cx| cx.leak_detector_snapshot());

        {
            let (_, cx) = cx.add_window_view(|_, _| Host);
            let draw = |cx: &mut VisualTestContext| {
                cx.run_until_parked();
                cx.update(|window, cx| _ = window.draw(cx));
            };
            draw(cx);
            cx.simulate_click(point(px(8.), px(8.)), Modifiers::default());
            draw(cx);
            draw(cx);
            assert!(cx.update(|window, cx| window.focused(cx).is_some()));

            cx.update(|window, _| window.remove_window());
            cx.run_until_parked();
        }

        cx.update(|cx| cx.assert_no_new_leaks(&before));
    }
}
