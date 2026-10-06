use gpui::{
    Context, HitboxBehavior, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, Styled as _, TouchDragEvent, TouchPhase, WeakEntity, canvas,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Grip {
    Touch,
    Mouse,
}

pub(super) trait Press: Sized + 'static {
    fn grip(&self) -> Option<Grip>;
    fn grab(&mut self, at: Point<Pixels>, grip: Grip, cx: &mut Context<Self>);
    fn track(&mut self, at: Point<Pixels>, cx: &mut Context<Self>);
    fn release(&mut self, cancelled: bool, cx: &mut Context<Self>);
}

pub(super) fn press_listeners<T: Press>(target: WeakEntity<T>) -> impl IntoElement {
    canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |_, hitbox, window, _| {
            window.on_mouse_event({
                let target = target.clone();
                let hitbox = hitbox.clone();
                move |event: &TouchDragEvent, phase, window, cx| {
                    if !phase.bubble() {
                        return;
                    }
                    let Some(target) = target.upgrade() else {
                        return;
                    };
                    match event.phase {
                        TouchPhase::Started => {
                            if window.default_prevented()
                                || !hitbox.is_hovered_at(event.start_position, window)
                            {
                                return;
                            }
                            window.prevent_default();
                            target.update(cx, |target, cx| {
                                target.grab(event.start_position, Grip::Touch, cx);
                                target.track(event.position, cx);
                            });
                        }
                        TouchPhase::Moved => target.update(cx, |target, cx| {
                            if target.grip() == Some(Grip::Touch) {
                                target.track(event.position, cx);
                            }
                        }),
                        TouchPhase::Ended | TouchPhase::Cancelled => {
                            target.update(cx, |target, cx| {
                                if target.grip() == Some(Grip::Touch) {
                                    let cancelled = event.phase == TouchPhase::Cancelled;
                                    if !cancelled {
                                        target.track(event.position, cx);
                                    }
                                    target.release(cancelled, cx);
                                }
                            });
                        }
                    }
                }
            });
            window.on_mouse_event({
                let target = target.clone();
                move |event: &MouseDownEvent, phase, window, cx| {
                    if !phase.bubble()
                        || event.button != MouseButton::Left
                        || !hitbox.is_hovered(window)
                    {
                        return;
                    }
                    let Some(target) = target.upgrade() else {
                        return;
                    };
                    window.prevent_default();
                    cx.stop_propagation();
                    target.update(cx, |target, cx| {
                        target.grab(event.position, Grip::Mouse, cx);
                    });
                }
            });
            window.on_mouse_event({
                let target = target.clone();
                move |event: &MouseMoveEvent, phase, _, cx| {
                    if !phase.bubble() {
                        return;
                    }
                    if let Some(target) = target.upgrade() {
                        target.update(cx, |target, cx| {
                            if target.grip() == Some(Grip::Mouse) {
                                target.track(event.position, cx);
                            }
                        });
                    }
                }
            });
            window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                if !phase.bubble() || event.button != MouseButton::Left {
                    return;
                }
                if let Some(target) = target.upgrade() {
                    target.update(cx, |target, cx| {
                        if target.grip() == Some(Grip::Mouse) {
                            target.track(event.position, cx);
                            target.release(false, cx);
                        }
                    });
                }
            });
        },
    )
    .absolute()
    .inset_0()
}
