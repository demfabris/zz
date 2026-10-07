use std::sync::Arc;

use gpui::{
    AnyElement, App, DispatchPhase, ElementId, Entity, EntityId, Global, Hitbox, HitboxBehavior,
    Hsla, IntoElement, LongPressEvent, MouseButton, MouseDownEvent, MouseExitEvent, MouseUpEvent,
    Pixels, Point, ScrollWheelEvent, Styled, TouchDragEvent, TouchPhase, Window, canvas, fill,
};
use web_time::{Duration, Instant};

const DELAY: Duration = Duration::from_millis(100);
const RISE: Duration = Duration::from_millis(120);
const FALL: Duration = Duration::from_millis(180);
const MIN_PRESS: Duration = Duration::from_millis(130);

#[derive(Clone, Copy, Default)]
struct Contact {
    start: Option<Point<Pixels>>,
    owner: Option<EntityId>,
    live: bool,
    mouse: bool,
}

impl Global for Contact {}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Press {
    from: Option<Instant>,
    until: Option<Instant>,
}

fn ease_out(elapsed: Duration, span: Duration) -> f32 {
    let t = (elapsed.as_secs_f32() / span.as_secs_f32()).clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

impl Press {
    fn level(self, now: Instant) -> f32 {
        let Some(from) = self.from else {
            return 0.0;
        };
        let rise = |at: Instant| ease_out(at.saturating_duration_since(from), RISE);
        match self.until {
            Some(until) if now >= until => {
                rise(until) * (1.0 - ease_out(now.saturating_duration_since(until), FALL))
            }
            _ => rise(now),
        }
    }

    fn moving(self, now: Instant) -> bool {
        match (self.from, self.until) {
            (None, _) => false,
            (Some(from), None) => now < from + RISE,
            (Some(_), Some(until)) => now < until + FALL,
        }
    }

    fn settle(&mut self, now: Instant) {
        if self.until.is_some_and(|until| now >= until + FALL) {
            *self = Self::default();
        }
    }

    fn down(&mut self, now: Instant, delay: Duration) {
        *self = Self {
            from: Some(now + delay),
            until: None,
        };
    }

    fn up(&mut self, now: Instant) {
        let Some(from) = self.from.filter(|_| self.until.is_none()) else {
            return;
        };
        let from = from.min(now);
        self.from = Some(from);
        self.until = Some(now.max(from + MIN_PRESS));
    }

    fn cancel(&mut self, now: Instant) {
        match self.from {
            Some(from) if from > now => *self = Self::default(),
            Some(_) if self.until.is_none() => self.until = Some(now),
            _ => {}
        }
    }
}

/// How far a control is pressed, from 0 to 1, and the listener that tracks
/// it. Add [`PressFeedback::listener`] as an absolute child covering the
/// control's hit area, then draw the press from [`PressFeedback::amount`].
pub struct PressFeedback {
    pub amount: f32,
    listener: AnyElement,
}

impl PressFeedback {
    #[must_use]
    pub fn listener(self) -> AnyElement {
        self.listener
    }
}

/// Tracks presses the way touch platforms show them: a touch lights the
/// control after a short delay unless it turns into a scroll first, a quick
/// tap still flashes, and the highlight fades in and out. The innermost
/// pressable under the finger takes the press.
pub fn press_feedback(
    id: impl Into<ElementId>,
    window: &mut Window,
    cx: &mut App,
) -> PressFeedback {
    let (state, amount) = press_state(id.into(), window, cx);
    PressFeedback {
        amount,
        listener: canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
            move |_, hitbox, window, _| listen(&state, hitbox, DELAY, window),
        )
        .absolute()
        .inset_0()
        .into_any_element(),
    }
}

/// [`press_feedback`] for builders without a window: a layer that tracks
/// presses over its own bounds and paints `color` at the press amount. It
/// fills its parent unless positioned; put it under the content.
pub fn press_highlight(
    id: impl Into<ElementId>,
    color: Hsla,
    radius: Pixels,
) -> impl IntoElement + Styled {
    highlight(id.into(), color, radius, DELAY)
}

/// [`press_highlight`] that lights at touch-down, for keys that never scroll.
pub fn instant_press_highlight(
    id: impl Into<ElementId>,
    color: Hsla,
    radius: Pixels,
) -> impl IntoElement + Styled {
    highlight(id.into(), color, radius, Duration::ZERO)
}

fn highlight(
    id: ElementId,
    color: Hsla,
    radius: Pixels,
    delay: Duration,
) -> impl IntoElement + Styled {
    canvas(
        move |bounds, window, cx| {
            let (state, amount) = press_state(id, window, cx);
            (
                window.insert_hitbox(bounds, HitboxBehavior::Normal),
                state,
                amount,
            )
        },
        move |bounds, (hitbox, state, amount), window, _| {
            if amount > 0.0 {
                window.paint_quad(fill(bounds, color.opacity(amount)).corner_radii(radius));
            }
            listen(&state, hitbox, delay, window);
        },
    )
    .absolute()
    .inset_0()
}

fn press_state(id: ElementId, window: &mut Window, cx: &mut App) -> (Entity<Press>, f32) {
    let key = ElementId::NamedChild(Arc::new(id), "press".into());
    let state = window.use_keyed_state(key, cx, |_, _| Press::default());
    let now = cx.background_executor().now();
    let press = state.update(cx, |press, _| {
        press.settle(now);
        *press
    });
    if press.moving(now) {
        window.request_animation_frame();
    }
    (state, press.level(now))
}

fn listen(state: &Entity<Press>, hitbox: Hitbox, delay: Duration, window: &mut Window) {
    let me = state.entity_id();
    let update = {
        let state = state.clone();
        move |cx: &mut App, f: &dyn Fn(&mut Press, Instant)| {
            let now = cx.background_executor().now();
            state.update(cx, |press, cx| {
                let before = *press;
                f(press, now);
                if *press != before {
                    cx.notify();
                }
            });
        }
    };
    window.on_mouse_event({
        let update = update.clone();
        let hitbox = hitbox.clone();
        move |event: &TouchDragEvent, phase, window, cx| {
            if event.phase != TouchPhase::Started {
                return;
            }
            let contact = *cx.default_global::<Contact>();
            if phase == DispatchPhase::Capture {
                if contact.owner == Some(me) && contact.start != Some(event.start_position) {
                    update(cx, &|press, now| press.cancel(now));
                }
                cx.set_global(Contact {
                    start: Some(event.start_position),
                    owner: None,
                    live: true,
                    mouse: false,
                });
            } else if contact.owner.is_none()
                && !window.default_prevented()
                && hitbox.is_hovered_at(event.start_position, window)
            {
                cx.global_mut::<Contact>().owner = Some(me);
                update(cx, &|press, now| press.down(now, delay));
            }
        }
    });
    window.on_mouse_event({
        let update = update.clone();
        move |event: &ScrollWheelEvent, phase, _, cx| {
            if phase != DispatchPhase::Capture || event.touch_phase != TouchPhase::Started {
                return;
            }
            let contact = *cx.default_global::<Contact>();
            if contact.start == Some(event.position) {
                if contact.owner == Some(me) {
                    update(cx, &|press, now| press.cancel(now));
                }
                cx.global_mut::<Contact>().live = false;
            }
        }
    });
    window.on_mouse_event({
        let update = update.clone();
        move |event: &MouseDownEvent, phase, window, cx| {
            if event.button != MouseButton::Left {
                return;
            }
            let contact = *cx.default_global::<Contact>();
            if phase == DispatchPhase::Capture {
                if !contact.live {
                    cx.set_global(Contact {
                        start: Some(event.position),
                        owner: None,
                        live: true,
                        mouse: true,
                    });
                }
            } else if contact.mouse && contact.owner.is_none() && hitbox.is_hovered(window) {
                cx.global_mut::<Contact>().owner = Some(me);
                update(cx, &|press, now| press.down(now, Duration::ZERO));
            }
        }
    });
    window.on_mouse_event({
        let update = update.clone();
        move |event: &MouseUpEvent, phase, _, cx| {
            if phase != DispatchPhase::Capture || event.button != MouseButton::Left {
                return;
            }
            if cx.default_global::<Contact>().owner == Some(me) {
                update(cx, &|press, now| press.up(now));
            }
            cx.global_mut::<Contact>().live = false;
        }
    });
    window.on_mouse_event({
        let update = update.clone();
        move |_: &MouseExitEvent, phase, _, cx| {
            let contact = *cx.default_global::<Contact>();
            if phase == DispatchPhase::Capture && contact.live && contact.owner == Some(me) {
                update(cx, &|press, now| press.cancel(now));
                cx.global_mut::<Contact>().live = false;
            }
        }
    });
    window.on_mouse_event(move |event: &LongPressEvent, phase, _, cx| {
        if phase != DispatchPhase::Capture
            || !matches!(event.phase, TouchPhase::Ended | TouchPhase::Cancelled)
        {
            return;
        }
        if cx.default_global::<Contact>().owner == Some(me) {
            update(cx, &|press, now| press.up(now));
        }
        cx.global_mut::<Contact>().live = false;
    });
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use gpui::{
        Context, Modifiers, ParentElement as _, Render, ScrollDelta, TestAppContext,
        VisualTestContext, div, point, prelude::*, px,
    };

    use super::*;

    struct Host {
        outer: Rc<Cell<f32>>,
        inner: Rc<Cell<f32>>,
    }

    impl Render for Host {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let outer = press_feedback("outer", window, cx);
            let inner = press_feedback("inner", window, cx);
            self.outer.set(outer.amount);
            self.inner.set(inner.amount);
            div().size_full().relative().child(outer.listener()).child(
                div()
                    .debug_selector(|| "inner".to_owned())
                    .absolute()
                    .left(px(10.0))
                    .top(px(10.0))
                    .size(px(60.0))
                    .child(inner.listener()),
            )
        }
    }

    fn host(cx: &mut TestAppContext) -> (Rc<Cell<f32>>, Rc<Cell<f32>>, &mut VisualTestContext) {
        cx.update(crate::init);
        let outer = Rc::new(Cell::new(0.0));
        let inner = Rc::new(Cell::new(0.0));
        let (o, i) = (Rc::clone(&outer), Rc::clone(&inner));
        let (_, cx) = cx.add_window_view(move |_, _| Host { outer: o, inner: i });
        frame(cx, 0);
        (outer, inner, cx)
    }

    fn frame(cx: &mut VisualTestContext, ms: u64) {
        cx.executor().advance_clock(Duration::from_millis(ms));
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.refresh();
            _ = window.draw(cx);
        });
    }

    fn touch(cx: &mut VisualTestContext, at: Point<Pixels>) {
        cx.simulate_event(TouchDragEvent {
            phase: TouchPhase::Started,
            start_position: at,
            position: at,
        });
    }

    fn tap(cx: &mut VisualTestContext, at: Point<Pixels>) {
        cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    }

    #[gpui::test]
    fn a_held_touch_lights_the_innermost_control_after_the_delay(cx: &mut TestAppContext) {
        let (outer, inner, cx) = host(cx);
        let at = point(px(30.0), px(30.0));
        touch(cx, at);
        frame(cx, 50);
        assert_eq!(inner.get(), 0.0);
        frame(cx, 200);
        assert_eq!(inner.get(), 1.0);
        assert_eq!(outer.get(), 0.0);
        tap(cx, at);
        frame(cx, 90);
        assert!(inner.get() > 0.0 && inner.get() < 1.0);
        frame(cx, 200);
        assert_eq!(inner.get(), 0.0);
    }

    #[gpui::test]
    fn a_touch_that_ends_without_a_tap_lets_go(cx: &mut TestAppContext) {
        let (_, inner, cx) = host(cx);
        let at = point(px(30.0), px(30.0));
        touch(cx, at);
        frame(cx, 250);
        assert_eq!(inner.get(), 1.0);
        cx.simulate_event(MouseExitEvent {
            position: point(px(-1.0), px(-1.0)),
            pressed_button: None,
            modifiers: Modifiers::none(),
        });
        frame(cx, 400);
        assert_eq!(inner.get(), 0.0);
    }

    #[gpui::test]
    fn a_quick_tap_still_flashes(cx: &mut TestAppContext) {
        let (_, inner, cx) = host(cx);
        let at = point(px(30.0), px(30.0));
        touch(cx, at);
        frame(cx, 30);
        tap(cx, at);
        frame(cx, 100);
        assert!(inner.get() > 0.5);
        frame(cx, 400);
        assert_eq!(inner.get(), 0.0);
    }

    #[gpui::test]
    fn a_scroll_before_the_delay_never_lights(cx: &mut TestAppContext) {
        let (outer, inner, cx) = host(cx);
        let at = point(px(30.0), px(30.0));
        touch(cx, at);
        frame(cx, 40);
        cx.simulate_event(ScrollWheelEvent {
            position: at,
            delta: ScrollDelta::Pixels(point(px(0.0), px(-20.0))),
            modifiers: Modifiers::none(),
            touch_phase: TouchPhase::Started,
        });
        for _ in 0..10 {
            frame(cx, 30);
            assert_eq!(inner.get(), 0.0);
            assert_eq!(outer.get(), 0.0);
        }
    }

    #[gpui::test]
    fn a_mouse_press_lights_at_once(cx: &mut TestAppContext) {
        let (outer, inner, cx) = host(cx);
        let at = point(px(200.0), px(200.0));
        cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
        frame(cx, 150);
        assert_eq!(outer.get(), 1.0);
        assert_eq!(inner.get(), 0.0);
        cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
        frame(cx, 400);
        assert_eq!(outer.get(), 0.0);
    }
}
