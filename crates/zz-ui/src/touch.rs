use gpui::{App, Div, ElementId, Global, InteractiveElement as _, Stateful, Styled as _, div};

use crate::{Size, rems_from_px};

pub const TOUCH_TARGET: f32 = 44.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CoarsePointer(pub bool);

impl Global for CoarsePointer {}

impl CoarsePointer {
    #[must_use]
    pub fn get(cx: &App) -> bool {
        cx.try_global::<Self>().is_some_and(|pointer| pointer.0)
    }

    pub fn set(coarse: bool, cx: &mut App) {
        if Self::get(cx) != coarse {
            cx.set_global(Self(coarse));
        }
    }
}

#[must_use]
pub fn control_slop(size: Size) -> f32 {
    let height = match size {
        Size::XSmall => 24.0,
        Size::Small => 28.0,
        Size::Medium => 36.0,
        Size::Large => 40.0,
        Size::Size(_) => TOUCH_TARGET,
    };
    (TOUCH_TARGET - height) / 2.0
}

#[must_use]
pub fn hit_area(id: impl Into<ElementId>, slop_x: f32, slop_y: f32) -> Stateful<Div> {
    div()
        .id(id)
        .absolute()
        .top(rems_from_px(-slop_y))
        .bottom(rems_from_px(-slop_y))
        .left(rems_from_px(-slop_x))
        .right(rems_from_px(-slop_x))
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use gpui::{
        Context, IntoElement, Modifiers, ParentElement as _, Render,
        StatefulInteractiveElement as _, TestAppContext, Window, point, prelude::*, px,
    };

    use super::*;
    use crate::{
        IconName,
        button::{Button, ButtonVariants as _},
    };

    struct Host {
        clicks: Rc<Cell<[usize; 3]>>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let count = |index: usize, clicks: &Rc<Cell<[usize; 3]>>| {
                let clicks = Rc::clone(clicks);
                move |_: &gpui::ClickEvent, _: &mut Window, _: &mut App| {
                    let mut all = clicks.get();
                    all[index] += 1;
                    clicks.set(all);
                }
            };
            div()
                .size_full()
                .id("row")
                .on_click(count(2, &self.clicks))
                .child(
                    div()
                        .absolute()
                        .top(px(100.0))
                        .left(px(100.0))
                        .flex()
                        .gap(px(4.0))
                        .child(
                            Button::compact_icon("first", IconName::Plus)
                                .debug_selector(|| "first".to_owned())
                                .hit_slop(2.0, 10.0)
                                .on_click(count(0, &self.clicks)),
                        )
                        .child(
                            Button::compact_icon("second", IconName::Xmark)
                                .ghost()
                                .debug_selector(|| "second".to_owned())
                                .hit_slop(2.0, 10.0)
                                .on_click(count(1, &self.clicks)),
                        ),
                )
        }
    }

    #[gpui::test]
    fn coarse_buttons_take_taps_beside_the_glyph(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let clicks = Rc::new(Cell::new([0; 3]));
        let shared = Rc::clone(&clicks);
        let (_, cx) = cx.add_window_view(move |_, _| Host { clicks: shared });
        let tap = |cx: &mut gpui::VisualTestContext, at| {
            cx.simulate_click(at, Modifiers::none());
            cx.run_until_parked();
        };
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        let first = cx.debug_bounds("first").expect("first");
        assert_eq!(first.size.height, px(24.0));
        let above = point(first.center().x, first.top() - px(8.0));
        tap(cx, above);
        assert_eq!(clicks.get(), [0, 0, 1]);

        cx.update(|window, cx| {
            CoarsePointer::set(true, cx);
            window.refresh();
            _ = window.draw(cx);
        });
        let first = cx.debug_bounds("first").expect("first");
        let second = cx.debug_bounds("second").expect("second");
        assert_eq!(first.size.height, px(24.0));
        assert_eq!(first.left(), px(100.0));
        assert_eq!(first.top(), px(100.0));
        assert_eq!(second.left() - first.right(), px(4.0));
        tap(cx, point(first.center().x, first.top() - px(8.0)));
        tap(cx, point(first.center().x, first.bottom() + px(8.0)));
        tap(cx, first.center());
        assert_eq!(clicks.get(), [3, 0, 1]);
        tap(cx, point(first.right() + px(1.0), first.center().y));
        tap(cx, point(second.left() - px(1.0), first.center().y));
        assert_eq!(clicks.get(), [4, 1, 1]);
        tap(cx, point(first.center().x, first.top() - px(12.0)));
        assert_eq!(clicks.get(), [4, 1, 2]);
    }
}
