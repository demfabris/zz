//! Easing curves.

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
