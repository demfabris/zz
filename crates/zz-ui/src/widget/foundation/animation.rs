//! Easing curves.

use std::time::Duration;

use gpui::{
    Animation, AnimationElement, AnimationExt as _, ElementId, IntoElement, Pixels, Styled,
    ease_out_quint, px,
};

pub(crate) const SURFACE_ENTER_DURATION: Duration = Duration::from_millis(160);

pub(crate) fn surface_enter<E: IntoElement + Styled + 'static>(
    surface: E,
    id: impl Into<ElementId>,
    resting_top: Pixels,
) -> AnimationElement<E> {
    surface.with_animation(
        id,
        Animation::new(SURFACE_ENTER_DURATION).with_easing(ease_out_quint()),
        move |surface, delta| {
            surface
                .top(resting_top + px(6.0 * (1.0 - delta)))
                .opacity(delta)
        },
    )
}

/// A cubic easing function from the two interior ordinates `y1` and `y2` of a
/// Bézier curve with endpoints 0 and 1. Evaluates *y* against `t` directly,
/// unlike CSS `cubic-bezier`, which solves for `t` from `x`.
pub fn cubic_ease(y1: f32, y2: f32) -> impl Fn(f32) -> f32 {
    move |t: f32| {
        let one_t = 1.0 - t;
        let one_t2 = one_t * one_t;
        let t2 = t * t;
        let t3 = t2 * t;

        3.0 * y1 * one_t2 * t + 3.0 * y2 * one_t * t2 + t3
    }
}
