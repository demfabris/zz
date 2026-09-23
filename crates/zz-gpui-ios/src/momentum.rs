use gpui::{Pixels, Point, point, px};
use std::time::Instant;

const DECELERATION_PER_MS: f64 = 0.998;
const STOP_SPEED: f64 = 10.0;
const MIN_SPEED: f64 = 50.0;

pub(crate) struct Momentum {
    pub(crate) position: Point<Pixels>,
    horizontal: bool,
    velocity: f64,
    started: Instant,
    emitted: f64,
}

impl Momentum {
    pub(crate) fn fling(
        position: Point<Pixels>,
        velocity: Point<f64>,
        started: Instant,
    ) -> Option<Self> {
        let horizontal = velocity.x.abs() > velocity.y.abs();
        let velocity = if horizontal { velocity.x } else { velocity.y };
        (velocity.abs() >= MIN_SPEED).then_some(Self {
            position,
            horizontal,
            velocity,
            started,
            emitted: 0.0,
        })
    }

    pub(crate) fn step(&mut self, now: Instant) -> (Point<Pixels>, bool) {
        let speed = self.velocity.abs();
        let end = (STOP_SPEED / speed).ln() / DECELERATION_PER_MS.ln();
        let elapsed = (now.duration_since(self.started).as_secs_f64() * 1000.0).min(end);
        let distance =
            speed / 1000.0 * (DECELERATION_PER_MS.powf(elapsed) - 1.0) / DECELERATION_PER_MS.ln();
        let step = ((distance - self.emitted) * self.velocity.signum()) as f32;
        self.emitted = distance;
        let delta = if self.horizontal {
            point(px(step), px(0.0))
        } else {
            point(px(0.0), px(step))
        };
        (delta, elapsed >= end)
    }
}
