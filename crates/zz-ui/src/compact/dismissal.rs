use std::collections::VecDeque;

use gpui::{
    App, DispatchPhase, Global, IntoElement, Pixels, Point, ScrollWheelEvent, SpringConfig,
    SpringState, Styled as _, TouchPhase, canvas,
};
use web_time::{Duration, Instant};

const RUBBER_BAND: f32 = 0.55;
const VELOCITY_WINDOW: Duration = Duration::from_millis(100);
const FRAME: Duration = Duration::from_millis(16);
const SETTLE_EPSILON: f32 = 0.5;
const SPRING: SpringConfig = SpringConfig::new(522.35, 45.709_955, 1.0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Overdrag {
    Clamp,
    Band,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tick {
    Still,
    Moving,
    Dismissed,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Motion {
    Idle,
    Drag,
    Settle {
        target: f32,
        velocity: f32,
        at: Instant,
    },
    Gone,
}

#[derive(Clone, Debug)]
pub(super) struct Dismissal {
    offset: f32,
    raw: f32,
    extent: f32,
    overdrag: Overdrag,
    motion: Motion,
    samples: VecDeque<(Instant, f32)>,
}

impl Dismissal {
    pub(super) fn new(overdrag: Overdrag) -> Self {
        Self {
            offset: 0.0,
            raw: 0.0,
            extent: 0.0,
            overdrag,
            motion: Motion::Idle,
            samples: VecDeque::new(),
        }
    }

    pub(super) fn offset(&self) -> f32 {
        self.offset
    }

    pub(super) fn progress(&self) -> f32 {
        if self.extent > 0.0 {
            (self.offset / self.extent).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    #[cfg(test)]
    fn is_dragging(&self) -> bool {
        self.motion == Motion::Drag
    }

    #[cfg(test)]
    fn is_moving(&self) -> bool {
        !matches!(self.motion, Motion::Idle)
    }

    pub(super) fn grab(&mut self, extent: f32) {
        if self.motion == Motion::Gone {
            self.offset = 0.0;
        }
        self.extent = extent.max(1.0);
        self.raw = self.unband(self.offset);
        self.samples.clear();
        self.motion = Motion::Drag;
    }

    pub(super) fn drag(&mut self, delta: f32, now: Instant) {
        if self.motion != Motion::Drag {
            return;
        }
        self.raw += delta;
        self.offset = self.band(self.raw);
        self.samples.push_back((now, delta));
        while self
            .samples
            .front()
            .is_some_and(|(at, _)| now.saturating_duration_since(*at) > VELOCITY_WINDOW)
        {
            self.samples.pop_front();
        }
    }

    pub(super) fn release(&mut self, fling: f32, commit_fraction: f32, now: Instant) -> bool {
        if self.motion != Motion::Drag {
            return false;
        }
        let velocity = self.velocity(now);
        let commit = if velocity >= fling {
            true
        } else if velocity <= -fling {
            false
        } else {
            self.offset > self.extent * commit_fraction
        };
        let target = if commit { self.extent } else { 0.0 };
        self.settle(target, velocity, now);
        commit
    }

    pub(super) fn cancel(&mut self, now: Instant) {
        if self.motion == Motion::Drag {
            self.settle(0.0, 0.0, now);
        }
    }

    pub(super) fn finish(&mut self) {
        if let Motion::Settle { target, at, .. } = self.motion {
            self.offset = target;
            self.motion = Motion::Settle {
                target,
                velocity: 0.0,
                at,
            };
        }
    }

    pub(super) fn tick(&mut self, now: Instant) -> Tick {
        match self.motion {
            Motion::Idle | Motion::Drag => Tick::Still,
            Motion::Gone => {
                self.offset = 0.0;
                self.motion = Motion::Idle;
                Tick::Still
            }
            Motion::Settle {
                target,
                velocity,
                at,
            } => {
                let elapsed = now.saturating_duration_since(at).as_secs_f32();
                let state = SPRING.step(
                    SpringState {
                        position: self.offset,
                        velocity,
                    },
                    target,
                    elapsed,
                );
                let dismissing = target > 0.0;
                let arrived = SPRING.is_settled(state, target, SETTLE_EPSILON)
                    || (dismissing && state.position >= target - SETTLE_EPSILON);
                if arrived {
                    self.offset = target;
                    if dismissing {
                        self.motion = Motion::Gone;
                        Tick::Dismissed
                    } else {
                        self.motion = Motion::Idle;
                        Tick::Still
                    }
                } else {
                    self.offset = state.position.clamp(self.band(-self.extent), self.extent);
                    self.motion = Motion::Settle {
                        target,
                        velocity: state.velocity,
                        at: now,
                    };
                    Tick::Moving
                }
            }
        }
    }

    fn settle(&mut self, target: f32, velocity: f32, now: Instant) {
        self.samples.clear();
        self.motion = Motion::Settle {
            target,
            velocity,
            at: now,
        };
    }

    fn band(&self, raw: f32) -> f32 {
        if raw >= 0.0 {
            return raw.min(self.extent);
        }
        match self.overdrag {
            Overdrag::Clamp => 0.0,
            Overdrag::Band => {
                let stretch = -raw;
                -(1.0 - 1.0 / (stretch * RUBBER_BAND / self.extent + 1.0)) * self.extent
            }
        }
    }

    fn unband(&self, offset: f32) -> f32 {
        if offset >= 0.0 || self.overdrag == Overdrag::Clamp {
            return offset.max(0.0);
        }
        let fraction = (-offset / self.extent).min(0.99);
        -(1.0 / (1.0 - fraction) - 1.0) * self.extent / RUBBER_BAND
    }

    fn velocity(&self, now: Instant) -> f32 {
        let mut first = None;
        let mut total = 0.0;
        for (at, delta) in &self.samples {
            if now.saturating_duration_since(*at) <= VELOCITY_WINDOW {
                first.get_or_insert(*at);
                total += delta;
            }
        }
        let Some(first) = first else {
            return 0.0;
        };
        total
            / now
                .saturating_duration_since(first)
                .max(FRAME)
                .as_secs_f32()
    }
}

#[derive(Default)]
struct Coast(Option<Point<Pixels>>);

impl Global for Coast {}

pub(super) fn coast(start: Point<Pixels>, cx: &mut App) {
    cx.set_global(Coast(Some(start)));
}

pub(super) fn coasting(event: &ScrollWheelEvent, cx: &mut App) -> bool {
    let Some(start) = cx.try_global::<Coast>().and_then(|coast| coast.0) else {
        return false;
    };
    match event.touch_phase {
        TouchPhase::Started => {
            cx.set_global(Coast(None));
            false
        }
        TouchPhase::Moved => event.position == start,
        TouchPhase::Ended | TouchPhase::Cancelled => {
            if event.position == start {
                cx.set_global(Coast(None));
                true
            } else {
                false
            }
        }
    }
}

pub fn coast_guard() -> impl IntoElement {
    canvas(
        |_, _, _| {},
        |_, (), window, _| {
            window.on_mouse_event(|event: &ScrollWheelEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture && coasting(event, cx) {
                    window.end_touch_momentum();
                    cx.stop_propagation();
                }
            });
        },
    )
    .absolute()
    .size_0()
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXTENT: f32 = 400.0;
    const FLING: f32 = 700.0;
    const COMMIT_FRACTION: f32 = 0.5;

    struct Clock(Instant);

    impl Clock {
        fn at(&self, ms: u64) -> Instant {
            self.0 + Duration::from_millis(ms)
        }
    }

    fn drag(dismissal: &mut Dismissal, clock: &Clock, steps: &[(u64, f32)]) {
        dismissal.grab(EXTENT);
        for (ms, delta) in steps {
            dismissal.drag(*delta, clock.at(*ms));
        }
    }

    fn settle(dismissal: &mut Dismissal, clock: &Clock, from: u64) -> Tick {
        let mut ms = from;
        loop {
            match dismissal.tick(clock.at(ms)) {
                Tick::Moving => ms += 16,
                tick => return tick,
            }
            assert!(ms < from + 3000, "never settled");
        }
    }

    #[test]
    fn a_short_slow_drag_springs_back() {
        let clock = Clock(Instant::now());
        let mut dismissal = Dismissal::new(Overdrag::Band);
        drag(
            &mut dismissal,
            &clock,
            &[(0, 40.0), (150, 40.0), (300, 40.0)],
        );
        assert_eq!(dismissal.offset(), 120.0);
        assert!(!dismissal.release(FLING, COMMIT_FRACTION, clock.at(500)));
        assert_eq!(settle(&mut dismissal, &clock, 500), Tick::Still);
        assert_eq!(dismissal.offset(), 0.0);
        assert!(!dismissal.is_moving());
    }

    #[test]
    fn past_half_the_extent_dismisses() {
        let clock = Clock(Instant::now());
        let mut dismissal = Dismissal::new(Overdrag::Band);
        drag(&mut dismissal, &clock, &[(0, 110.0), (150, 110.0)]);
        assert!(dismissal.release(FLING, COMMIT_FRACTION, clock.at(500)));
        assert_eq!(settle(&mut dismissal, &clock, 500), Tick::Dismissed);
        assert_eq!(dismissal.offset(), EXTENT);
        assert_eq!(dismissal.tick(clock.at(2000)), Tick::Still);
        assert_eq!(dismissal.offset(), 0.0);
    }

    #[test]
    fn finishing_lands_the_release_on_the_next_tick() {
        let clock = Clock(Instant::now());
        let mut dismissal = Dismissal::new(Overdrag::Band);
        drag(&mut dismissal, &clock, &[(0, 110.0), (150, 110.0)]);
        assert!(dismissal.release(FLING, COMMIT_FRACTION, clock.at(500)));
        dismissal.finish();
        assert_eq!(dismissal.tick(clock.at(501)), Tick::Dismissed);
    }

    #[test]
    fn a_flick_dismisses_and_a_flick_back_stays() {
        let clock = Clock(Instant::now());
        let mut dismissal = Dismissal::new(Overdrag::Band);
        drag(&mut dismissal, &clock, &[(0, 12.0), (16, 12.0), (32, 12.0)]);
        assert!(dismissal.release(FLING, COMMIT_FRACTION, clock.at(40)));
        assert_eq!(settle(&mut dismissal, &clock, 40), Tick::Dismissed);

        let mut dismissal = Dismissal::new(Overdrag::Band);
        drag(
            &mut dismissal,
            &clock,
            &[(0, 150.0), (150, 150.0), (300, -15.0), (316, -15.0)],
        );
        assert!(dismissal.offset() > EXTENT * COMMIT_FRACTION);
        assert!(!dismissal.release(FLING, COMMIT_FRACTION, clock.at(324)));
        assert_eq!(settle(&mut dismissal, &clock, 324), Tick::Still);
    }

    #[test]
    fn dragging_backwards_resists_or_stops() {
        let clock = Clock(Instant::now());
        let mut band = Dismissal::new(Overdrag::Band);
        drag(&mut band, &clock, &[(0, -100.0)]);
        assert!(band.offset() < 0.0 && band.offset() > -100.0 * RUBBER_BAND);
        band.drag(100.0, clock.at(16));
        assert!(band.offset().abs() < 0.01);

        let mut clamp = Dismissal::new(Overdrag::Clamp);
        drag(&mut clamp, &clock, &[(0, -100.0), (16, 30.0)]);
        assert_eq!(clamp.offset(), 0.0);
        clamp.drag(1000.0, clock.at(32));
        assert_eq!(clamp.offset(), EXTENT);
    }

    #[test]
    fn a_grab_catches_the_settle_where_it_is() {
        let clock = Clock(Instant::now());
        let mut dismissal = Dismissal::new(Overdrag::Band);
        drag(&mut dismissal, &clock, &[(0, 100.0), (150, 50.0)]);
        dismissal.release(FLING, COMMIT_FRACTION, clock.at(400));
        assert_eq!(dismissal.tick(clock.at(450)), Tick::Moving);
        let caught = dismissal.offset();
        assert!(caught > 0.0 && caught < 150.0);
        dismissal.grab(EXTENT);
        dismissal.drag(10.0, clock.at(460));
        assert!((dismissal.offset() - caught - 10.0).abs() < 0.01);
        assert!(dismissal.is_dragging());
    }

    #[test]
    fn cancel_returns_home() {
        let clock = Clock(Instant::now());
        let mut dismissal = Dismissal::new(Overdrag::Clamp);
        drag(&mut dismissal, &clock, &[(0, 300.0)]);
        dismissal.cancel(clock.at(10));
        assert_eq!(settle(&mut dismissal, &clock, 10), Tick::Still);
        assert_eq!(dismissal.offset(), 0.0);
    }
}
