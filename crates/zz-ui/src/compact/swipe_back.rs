use std::rc::Rc;

use gpui::{
    AnyElement, App, BoxShadow, Context, DispatchPhase, ElementId, HitboxBehavior, IntoElement,
    ParentElement as _, Pixels, Point, RenderOnce, ScrollWheelEvent, Styled as _, TouchPhase,
    Window, canvas, div, point, prelude::*, px,
};
use web_time::Instant;

use super::dismissal::{Dismissal, Overdrag, Tick, coast, coasting};
use crate::{ActiveTheme as _, Colorize as _};

const EDGE: f32 = 20.0;
const FLING_WIDTHS: f32 = 1.0;
const COMMIT_FRACTION: f32 = 0.5;
const PARALLAX: f32 = 1.0 / 3.0;
const DIM: f32 = 0.094;
const SHADOW: f32 = 0.3;

type Back = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct SwipeBack {
    id: ElementId,
    page: AnyElement,
    under: Option<AnyElement>,
    on_back: Back,
}

pub fn swipe_back(
    id: impl Into<ElementId>,
    page: impl IntoElement,
    on_back: impl Fn(&mut Window, &mut App) + 'static,
) -> SwipeBack {
    SwipeBack {
        id: id.into(),
        page: page.into_any_element(),
        under: None,
        on_back: Rc::new(on_back),
    }
}

impl SwipeBack {
    #[must_use]
    pub fn under(mut self, under: impl IntoElement) -> Self {
        self.under = Some(under.into_any_element());
        self
    }
}

struct SwipeState {
    dismissal: Dismissal,
    left: f32,
    width: f32,
    gesture: Option<Point<Pixels>>,
}

impl SwipeState {
    fn pan(
        &mut self,
        event: &ScrollWheelEvent,
        delta: f32,
        hovered: bool,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> bool {
        let start = event.position;
        let ours = self.gesture == Some(start);
        let claimed = match event.touch_phase {
            TouchPhase::Started => {
                self.gesture = None;
                if hovered && delta > 0.0 && f32::from(start.x) - self.left <= EDGE {
                    self.dismissal.grab(self.width);
                    self.dismissal.drag(delta, now);
                    self.gesture = Some(start);
                    true
                } else {
                    false
                }
            }
            TouchPhase::Moved if ours => {
                self.dismissal.drag(delta, now);
                true
            }
            TouchPhase::Ended if ours => {
                self.gesture = None;
                self.dismissal.drag(delta, now);
                self.dismissal
                    .release(FLING_WIDTHS * self.width, COMMIT_FRACTION, now);
                coast(start, cx);
                true
            }
            TouchPhase::Cancelled if ours => {
                self.gesture = None;
                self.dismissal.cancel(now);
                true
            }
            _ => false,
        };
        if claimed {
            cx.notify();
        }
        claimed
    }
}

impl RenderOnce for SwipeBack {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id.clone(), cx, |_, _| SwipeState {
            dismissal: Dismissal::new(Overdrag::Clamp),
            left: 0.0,
            width: 0.0,
            gesture: None,
        });
        let now = cx.background_executor().now();
        match state.update(cx, |state, _| state.dismissal.tick(now)) {
            Tick::Moving => window.request_animation_frame(),
            Tick::Dismissed => {
                let on_back = Rc::clone(&self.on_back);
                window.defer(cx, move |window, cx| on_back(window, cx));
            }
            Tick::Still => {}
        }
        let (offset, progress, width) = {
            let state = state.read(cx);
            (
                state.dismissal.offset(),
                state.dismissal.progress(),
                state.width,
            )
        };
        let moving = offset > 0.0;
        let theme = cx.theme();
        let listener = canvas(
            {
                let state = state.clone();
                move |bounds, window, cx| {
                    state.update(cx, |state, _| {
                        state.left = f32::from(bounds.left());
                        state.width = f32::from(bounds.size.width);
                    });
                    window.insert_hitbox(bounds, HitboxBehavior::Normal)
                }
            },
            move |_, hitbox, window, _| {
                window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
                    if phase != DispatchPhase::Capture {
                        return;
                    }
                    if coasting(event, cx) {
                        cx.stop_propagation();
                        return;
                    }
                    let hovered = hitbox.should_handle_scroll(window);
                    let delta = f32::from(event.delta.pixel_delta(window.line_height()).x);
                    let now = cx.background_executor().now();
                    if state.update(cx, |state, cx| state.pan(event, delta, hovered, now, cx)) {
                        cx.stop_propagation();
                    }
                });
            },
        )
        .absolute()
        .inset_0();
        div()
            .id(self.id)
            .relative()
            .size_full()
            .overflow_hidden()
            .child(listener)
            .when_some(self.under.filter(|_| moving), |this, under| {
                this.child(
                    div()
                        .id("swipe-back-under")
                        .debug_selector(|| "swipe-back-under".to_owned())
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .w_full()
                        .left(px(-(width - offset) * PARALLAX))
                        .child(under)
                        .child(
                            div()
                                .absolute()
                                .inset_0()
                                .occlude()
                                .bg(theme.scrim.alpha(DIM * (1.0 - progress))),
                        ),
                )
            })
            .child(
                div()
                    .id("swipe-back-page")
                    .debug_selector(|| "swipe-back-page".to_owned())
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .w_full()
                    .left(px(offset))
                    .bg(theme.background.opaque())
                    .when(moving, |page| {
                        page.shadow(vec![BoxShadow {
                            color: theme.scrim.alpha(SHADOW * (1.0 - progress)),
                            offset: point(px(-1.0), px(1.0)),
                            blur_radius: px(5.0),
                            spread_radius: px(0.0),
                            inset: false,
                        }])
                    })
                    .child(self.page),
            )
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use gpui::{
        Bounds, Context, Modifiers, Render, ScrollDelta, TestAppContext, VisualTestContext,
    };
    use web_time::Duration;

    use super::*;

    struct Host {
        backs: Rc<Cell<usize>>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let backs = Rc::clone(&self.backs);
            div().size_full().child(
                swipe_back(
                    "pushed",
                    div()
                        .id("page-body")
                        .size_full()
                        .overflow_y_scroll()
                        .child(div().h(px(4000.0))),
                    move |_, _| backs.set(backs.get() + 1),
                )
                .under(div().size_full()),
            )
        }
    }

    fn host(cx: &mut TestAppContext) -> (Rc<Cell<usize>>, &mut VisualTestContext) {
        cx.update(crate::init);
        let backs = Rc::new(Cell::new(0));
        let count = Rc::clone(&backs);
        let (_, cx) = cx.add_window_view(move |_, _| Host { backs: count });
        redraw(cx);
        (backs, cx)
    }

    fn redraw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
    }

    fn page(cx: &mut VisualTestContext) -> Bounds<Pixels> {
        cx.debug_bounds("swipe-back-page").expect("page")
    }

    fn pan(cx: &mut VisualTestContext, at: Point<Pixels>, steps: &[(u64, TouchPhase, f32)]) {
        for (wait, phase, dx) in steps {
            cx.executor().advance_clock(Duration::from_millis(*wait));
            cx.simulate_event(ScrollWheelEvent {
                position: at,
                delta: ScrollDelta::Pixels(point(px(*dx), px(0.0))),
                modifiers: Modifiers::none(),
                touch_phase: *phase,
            });
            redraw(cx);
        }
    }

    fn settle(cx: &mut VisualTestContext) {
        for _ in 0..120 {
            cx.executor().advance_clock(Duration::from_millis(16));
            redraw(cx);
        }
    }

    #[gpui::test]
    fn an_edge_drag_past_half_pops_and_a_short_one_returns(cx: &mut TestAppContext) {
        let (backs, cx) = host(cx);
        let rest = page(cx);
        let edge = point(px(8.0), rest.center().y);
        let half = f32::from(rest.size.width) / 2.0;

        pan(
            cx,
            edge,
            &[
                (0, TouchPhase::Started, 40.0),
                (150, TouchPhase::Moved, 60.0),
            ],
        );
        assert_eq!(page(cx).left(), px(100.0));
        let under = cx.debug_bounds("swipe-back-under").expect("under");
        assert!(under.left() < px(0.0) && under.left() > -rest.size.width / 3.0);
        pan(cx, edge, &[(300, TouchPhase::Ended, 0.0)]);
        settle(cx);
        assert_eq!(page(cx).left(), px(0.0));
        assert!(cx.debug_bounds("swipe-back-under").is_none());
        assert_eq!(backs.get(), 0);

        pan(
            cx,
            edge,
            &[
                (500, TouchPhase::Started, half / 2.0 + 10.0),
                (150, TouchPhase::Moved, half / 2.0 + 10.0),
                (300, TouchPhase::Ended, 0.0),
            ],
        );
        settle(cx);
        assert_eq!(backs.get(), 1);
        assert_eq!(page(cx).left(), px(0.0));
    }

    #[gpui::test]
    fn a_flick_from_the_edge_pops(cx: &mut TestAppContext) {
        let (backs, cx) = host(cx);
        let edge = point(px(8.0), page(cx).center().y);
        pan(
            cx,
            edge,
            &[
                (0, TouchPhase::Started, 20.0),
                (8, TouchPhase::Moved, 20.0),
                (8, TouchPhase::Ended, 0.0),
            ],
        );
        settle(cx);
        assert_eq!(backs.get(), 1);
    }

    #[gpui::test]
    fn drags_away_from_the_edge_or_leftward_are_ignored(cx: &mut TestAppContext) {
        let (backs, cx) = host(cx);
        let rest = page(cx);
        let inside = point(px(40.0), rest.center().y);
        pan(
            cx,
            inside,
            &[
                (0, TouchPhase::Started, 200.0),
                (150, TouchPhase::Ended, 0.0),
            ],
        );
        assert_eq!(page(cx).left(), px(0.0));
        let edge = point(px(8.0), rest.center().y);
        pan(
            cx,
            edge,
            &[
                (500, TouchPhase::Started, -20.0),
                (150, TouchPhase::Ended, 0.0),
            ],
        );
        assert_eq!(page(cx).left(), px(0.0));
        settle(cx);
        assert_eq!(backs.get(), 0);
    }
}
