use std::rc::Rc;

use gpui::{
    Animation, AnimationExt as _, AnyElement, App, Context, DispatchPhase, ElementId,
    HitboxBehavior, IntoElement, MouseButton, ParentElement as _, Pixels, Point, RenderOnce,
    ScrollHandle, ScrollWheelEvent, SharedString, Styled, TouchPhase, Window, canvas, div,
    ease_out_quint, prelude::*, px, relative,
};
use web_time::{Duration, Instant};

use super::dismissal::{Dismissal, Overdrag, Tick, coast, coasting};
use crate::{ActiveTheme as _, Colorize as _, StyledExt as _, rems_from_px};

const ENTER: Duration = Duration::from_millis(240);
const CORNER: f32 = 28.0;
const GRABBER_WIDTH: f32 = 36.0;
const GRABBER_HEIGHT: f32 = 5.0;
const HEADER_HEIGHT: f32 = 44.0;
const PADDING_X: f32 = 16.0;
const MAX_HEIGHT: f32 = 0.85;
const RISE: f32 = 0.3;
const FLING_HEIGHTS: f32 = 2.0;
const COMMIT_FRACTION: f32 = 0.48;

type Dismiss = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct BottomSheet {
    id: ElementId,
    title: SharedString,
    actions: Vec<AnyElement>,
    content: AnyElement,
    bottom_inset: Pixels,
    on_dismiss: Dismiss,
    content_scroll: Option<ScrollHandle>,
}

pub fn bottom_sheet(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    actions: impl IntoIterator<Item = AnyElement>,
    content: impl IntoElement,
    bottom_inset: Pixels,
    on_dismiss: impl Fn(&mut Window, &mut App) + 'static,
) -> BottomSheet {
    BottomSheet {
        id: id.into(),
        title: title.into(),
        actions: actions.into_iter().collect(),
        content: content.into_any_element(),
        bottom_inset,
        on_dismiss: Rc::new(on_dismiss),
        content_scroll: None,
    }
}

impl BottomSheet {
    #[must_use]
    pub fn content_scroll(mut self, handle: ScrollHandle) -> Self {
        self.content_scroll = Some(handle);
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Gesture {
    Idle,
    Content(Point<Pixels>),
    Sheet(Point<Pixels>),
}

struct SheetState {
    dismissal: Dismissal,
    scroll: ScrollHandle,
    content_scroll: Option<ScrollHandle>,
    extent: f32,
    gesture: Gesture,
}

impl SheetState {
    fn content(&self) -> &ScrollHandle {
        self.content_scroll.as_ref().unwrap_or(&self.scroll)
    }

    fn at_top(&self) -> bool {
        self.content().offset().y >= px(-0.5)
    }

    fn claim(&mut self, start: Point<Pixels>, delta: f32, now: Instant) {
        self.dismissal.grab(self.extent);
        self.dismissal.drag(delta, now);
        self.gesture = Gesture::Sheet(start);
    }

    fn pan(
        &mut self,
        event: &ScrollWheelEvent,
        delta: f32,
        hovered: bool,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> bool {
        let start = event.position;
        let claimed = match (event.touch_phase, self.gesture) {
            (TouchPhase::Started, _) => {
                self.gesture = Gesture::Idle;
                if !hovered || delta == 0.0 {
                    false
                } else if start.y < self.content().bounds().top() || delta > 0.0 && self.at_top() {
                    self.claim(start, delta, now);
                    true
                } else {
                    self.gesture = Gesture::Content(start);
                    false
                }
            }
            (TouchPhase::Moved, Gesture::Sheet(origin)) if origin == start => {
                self.dismissal.drag(delta, now);
                true
            }
            (TouchPhase::Moved, Gesture::Content(origin))
                if origin == start && delta > 0.0 && self.at_top() =>
            {
                self.claim(start, delta, now);
                true
            }
            (TouchPhase::Ended, Gesture::Sheet(origin)) if origin == start => {
                self.gesture = Gesture::Idle;
                self.dismissal.drag(delta, now);
                self.dismissal
                    .release(FLING_HEIGHTS * self.extent, COMMIT_FRACTION, now);
                coast(start, cx);
                true
            }
            (TouchPhase::Cancelled, Gesture::Sheet(origin)) if origin == start => {
                self.gesture = Gesture::Idle;
                self.dismissal.cancel(now);
                true
            }
            (TouchPhase::Ended | TouchPhase::Cancelled, _) => {
                self.gesture = Gesture::Idle;
                false
            }
            _ => false,
        };
        if claimed {
            cx.notify();
        }
        claimed
    }
}

impl RenderOnce for BottomSheet {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id.clone(), cx, |_, _| SheetState {
            dismissal: Dismissal::new(Overdrag::Band),
            scroll: ScrollHandle::new(),
            content_scroll: None,
            extent: 0.0,
            gesture: Gesture::Idle,
        });
        let now = cx.background_executor().now();
        let content_scroll = self.content_scroll;
        let tick = state.update(cx, |state, _| {
            state.content_scroll = content_scroll;
            state.dismissal.tick(now)
        });
        match tick {
            Tick::Moving => window.request_animation_frame(),
            Tick::Dismissed => {
                let on_dismiss = Rc::clone(&self.on_dismiss);
                window.defer(cx, move |window, cx| on_dismiss(window, cx));
            }
            Tick::Still => {}
        }
        let (offset, fade, scroll) = {
            let state = state.read(cx);
            (
                px(state.dismissal.offset()),
                1.0 - state.dismissal.progress(),
                state.scroll.clone(),
            )
        };
        let theme = cx.theme();
        let corner = rems_from_px(CORNER);
        let on_dismiss = self.on_dismiss;
        let scrim = div()
            .id("bottom-sheet-scrim")
            .debug_selector(|| "bottom-sheet-scrim".to_owned())
            .absolute()
            .inset_0()
            .bg(theme.scrim)
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_click(move |_, window, cx| on_dismiss(window, cx))
            .with_animation(
                "bottom-sheet-scrim-enter",
                Animation::new(ENTER).with_easing(ease_out_quint()),
                move |scrim, delta| scrim.opacity(delta * fade),
            );
        let listener = canvas(
            {
                let state = state.clone();
                move |bounds, window, cx| {
                    state.update(cx, |state, _| state.extent = f32::from(bounds.size.height));
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
                    let delta = f32::from(event.delta.pixel_delta(window.line_height()).y);
                    let now = cx.background_executor().now();
                    if state.update(cx, |state, cx| state.pan(event, delta, hovered, now, cx)) {
                        cx.stop_propagation();
                    }
                });
            },
        )
        .absolute()
        .inset_0();
        let panel = div()
            .id("bottom-sheet-panel")
            .debug_selector(|| "bottom-sheet-panel".to_owned())
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .max_h(relative(MAX_HEIGHT))
            .flex()
            .flex_col()
            .occlude()
            .rounded_tl(corner)
            .rounded_tr(corner)
            .bg(theme.background.raised(1).opaque())
            .text_color(theme.foreground)
            .font_family(theme.font_family.clone())
            .pb(self.bottom_inset)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(listener)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .justify_center()
                    .pt(rems_from_px(6.0))
                    .child(
                        div()
                            .w(rems_from_px(GRABBER_WIDTH))
                            .h(rems_from_px(GRABBER_HEIGHT))
                            .rounded_full()
                            .bg(theme.foreground.opacity(0.25)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(rems_from_px(8.0))
                    .h(rems_from_px(HEADER_HEIGHT))
                    .px(rems_from_px(PADDING_X))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_size(rems_from_px(17.0))
                            .line_height(rems_from_px(22.0))
                            .font_semibold()
                            .child(self.title),
                    )
                    .children(self.actions),
            )
            .child(
                div()
                    .id("bottom-sheet-content")
                    .flex()
                    .flex_col()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_none()
                            .w_full()
                            .child(self.content),
                    ),
            )
            .with_animation(
                "bottom-sheet-panel-enter",
                Animation::new(ENTER).with_easing(ease_out_quint()),
                |panel, delta| panel.bottom(relative(-RISE * (1.0 - delta))),
            );
        div()
            .id(self.id)
            .absolute()
            .inset_0()
            .occlude()
            .child(scrim)
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(offset)
                    .bottom(-offset)
                    .child(panel),
            )
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use gpui::{
        Bounds, Context, Modifiers, Render, ScrollDelta, TestAppContext, VisualTestContext, point,
    };

    use super::*;

    struct Host {
        dismissed: Rc<Cell<usize>>,
        body: Pixels,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let dismissed = Rc::clone(&self.dismissed);
            div().size_full().child(bottom_sheet(
                "sheet",
                "Panes",
                Vec::new(),
                div()
                    .debug_selector(|| "sheet-body".to_owned())
                    .h(self.body)
                    .w_full(),
                px(20.0),
                move |_, _| dismissed.set(dismissed.get() + 1),
            ))
        }
    }

    fn host(body: f32, cx: &mut TestAppContext) -> (Rc<Cell<usize>>, &mut VisualTestContext) {
        cx.update(|cx| {
            crate::init(cx);
            cx.set_reduce_motion(true);
        });
        let dismissed = Rc::new(Cell::new(0));
        let count = Rc::clone(&dismissed);
        let (_, cx) = cx.add_window_view(move |_, _| Host {
            dismissed: count,
            body: px(body),
        });
        redraw(cx);
        (dismissed, cx)
    }

    fn redraw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
    }

    fn panel(cx: &mut VisualTestContext) -> Bounds<Pixels> {
        cx.debug_bounds("bottom-sheet-panel").expect("panel")
    }

    fn pan(cx: &mut VisualTestContext, at: Point<Pixels>, steps: &[(u64, TouchPhase, f32)]) {
        for (wait, phase, dy) in steps {
            cx.executor().advance_clock(Duration::from_millis(*wait));
            cx.simulate_event(ScrollWheelEvent {
                position: at,
                delta: ScrollDelta::Pixels(point(px(0.0), px(*dy))),
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
    fn the_scrim_dismisses_and_the_panel_does_not(cx: &mut TestAppContext) {
        let (dismissed, cx) = host(120.0, cx);
        let panel = panel(cx);
        let scrim = cx.debug_bounds("bottom-sheet-scrim").expect("scrim");
        assert!(panel.top() > scrim.top());
        assert_eq!(panel.bottom(), scrim.bottom());
        let body = cx.debug_bounds("sheet-body").expect("body");
        assert_eq!(body.size.height, px(120.0));
        assert!(panel.bottom() - body.bottom() >= px(20.0));

        cx.simulate_click(panel.center(), Modifiers::none());
        assert_eq!(dismissed.get(), 0);

        cx.simulate_click(
            point(scrim.center().x, scrim.top() + px(10.0)),
            Modifiers::none(),
        );
        assert_eq!(dismissed.get(), 1);
    }

    #[gpui::test]
    fn a_tall_sheet_stops_at_most_of_the_window(cx: &mut TestAppContext) {
        let (_, cx) = host(5000.0, cx);
        let panel = panel(cx);
        let scrim = cx.debug_bounds("bottom-sheet-scrim").expect("scrim");
        assert!(panel.size.height <= scrim.size.height * MAX_HEIGHT + px(0.5));
        assert!(panel.size.height >= scrim.size.height * MAX_HEIGHT - px(0.5));
        let body = cx.debug_bounds("sheet-body").expect("body");
        assert_eq!(body.size.height, px(5000.0));
    }

    #[gpui::test]
    fn a_short_drag_follows_the_finger_and_springs_back(cx: &mut TestAppContext) {
        let (dismissed, cx) = host(300.0, cx);
        let rest = panel(cx);
        let grip = point(rest.center().x, rest.top() + px(20.0));
        pan(
            cx,
            grip,
            &[
                (0, TouchPhase::Started, 30.0),
                (150, TouchPhase::Moved, 30.0),
            ],
        );
        assert_eq!(panel(cx).top(), rest.top() + px(60.0));
        pan(cx, grip, &[(300, TouchPhase::Ended, 0.0)]);
        settle(cx);
        assert_eq!(panel(cx).top(), rest.top());
        assert_eq!(dismissed.get(), 0);
    }

    #[gpui::test]
    fn a_long_drag_or_a_flick_dismisses(cx: &mut TestAppContext) {
        let (dismissed, cx) = host(300.0, cx);
        let rest = panel(cx);
        let grip = point(rest.center().x, rest.top() + px(20.0));
        let half = f32::from(rest.size.height) / 2.0;
        pan(
            cx,
            grip,
            &[
                (0, TouchPhase::Started, half / 2.0 + 10.0),
                (150, TouchPhase::Moved, half / 2.0 + 10.0),
                (300, TouchPhase::Ended, 0.0),
            ],
        );
        settle(cx);
        assert_eq!(dismissed.get(), 1);

        pan(
            cx,
            grip,
            &[
                (500, TouchPhase::Started, 12.0),
                (8, TouchPhase::Moved, 12.0),
                (8, TouchPhase::Ended, 0.0),
            ],
        );
        settle(cx);
        assert_eq!(dismissed.get(), 2);
    }

    #[gpui::test]
    fn dragging_up_resists(cx: &mut TestAppContext) {
        let (_, cx) = host(300.0, cx);
        let rest = panel(cx);
        let grip = point(rest.center().x, rest.top() + px(20.0));
        pan(cx, grip, &[(0, TouchPhase::Started, -80.0)]);
        let lifted = rest.top() - panel(cx).top();
        assert!(lifted > px(0.0) && lifted < px(40.0), "{lifted:?}");
        pan(cx, grip, &[(300, TouchPhase::Ended, 0.0)]);
        settle(cx);
        assert_eq!(panel(cx).top(), rest.top());
    }

    #[gpui::test]
    fn content_scrolls_first_and_hands_over_at_its_top(cx: &mut TestAppContext) {
        let (dismissed, cx) = host(5000.0, cx);
        let rest = panel(cx);
        let body = point(rest.center().x, rest.center().y);
        let top = cx.debug_bounds("sheet-body").expect("body").top();
        pan(
            cx,
            body,
            &[
                (0, TouchPhase::Started, -50.0),
                (150, TouchPhase::Ended, 0.0),
            ],
        );
        assert_eq!(panel(cx).top(), rest.top());
        let content = cx.debug_bounds("sheet-body").expect("body");
        assert_eq!(content.top(), top - px(50.0));

        pan(
            cx,
            body,
            &[
                (500, TouchPhase::Started, 40.0),
                (150, TouchPhase::Moved, 30.0),
            ],
        );
        assert_eq!(panel(cx).top(), rest.top());
        assert_eq!(cx.debug_bounds("sheet-body").expect("body").top(), top);
        pan(cx, body, &[(150, TouchPhase::Moved, 30.0)]);
        assert_eq!(panel(cx).top(), rest.top() + px(30.0));
        pan(cx, body, &[(300, TouchPhase::Ended, 0.0)]);
        settle(cx);
        assert_eq!(panel(cx).top(), rest.top());
        assert_eq!(dismissed.get(), 0);
    }
}
