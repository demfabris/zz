use std::collections::VecDeque;

use gpui::{SpringConfig, SpringState, TouchPhase};
use web_time::{Duration, Instant};

const COMMIT_FRACTION: f32 = 0.25;
const FLING_VELOCITY: f32 = 350.0;
const RUBBER_BAND: f32 = 0.35;
const MOTION_FRACTION: f32 = 0.15;
const VELOCITY_WINDOW: Duration = Duration::from_millis(100);
const FRAME: Duration = Duration::from_millis(16);
const WHEEL_RELEASE: Duration = Duration::from_millis(120);
const PENDING_TIMEOUT: Duration = Duration::from_millis(1500);
const SETTLE_EPSILON: f32 = 0.0005;
const SPRING: SpringConfig = SpringConfig::new(900.0, 60.0, 1.0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PagerEvent {
    Commit { index: usize },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PagerResponse {
    pub consumed: bool,
    pub event: Option<PagerEvent>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PagerLayout {
    pub pages: Vec<(usize, f32)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Motion {
    Idle,
    Drag { raw: f32, wheel: bool },
    Settle { velocity: f32, at: Instant },
}

#[derive(Clone, Debug)]
pub struct Pager {
    index: usize,
    count: usize,
    shift: f32,
    width: f32,
    motion: Motion,
    touch_down: bool,
    momentum: bool,
    last_event: Option<Instant>,
    samples: VecDeque<(Instant, f32)>,
    pending: Option<(usize, Instant)>,
    deferred: Option<usize>,
    event: Option<PagerEvent>,
}

impl Default for Pager {
    fn default() -> Self {
        Self::new()
    }
}

impl Pager {
    pub fn new() -> Self {
        Self {
            index: 0,
            count: 0,
            shift: 0.0,
            width: 0.0,
            motion: Motion::Idle,
            touch_down: false,
            momentum: false,
            last_event: None,
            samples: VecDeque::new(),
            pending: None,
            deferred: None,
            event: None,
        }
    }

    pub fn sync(&mut self, index: usize, count: usize) {
        if count != self.count {
            self.pending = None;
        }
        self.count = count;
        let last = count.saturating_sub(1);
        let index = index.min(last);
        self.index = self.index.min(last);
        if let Some((from, at)) = self.pending
            && from == index
            && index != self.index
            && Instant::now().saturating_duration_since(at) < PENDING_TIMEOUT
        {
            return;
        }
        self.pending = None;
        match self.motion {
            Motion::Drag { .. } => self.deferred = Some(index),
            Motion::Settle { .. } if index != self.index => self.retarget(index),
            Motion::Settle { .. } => {}
            Motion::Idle => {
                self.index = index;
                self.shift = 0.0;
            }
        }
    }

    pub fn scroll(
        &mut self,
        delta_x: f32,
        phase: TouchPhase,
        width: f32,
        now: Instant,
    ) -> PagerResponse {
        self.release_stale_wheel(now);
        if self
            .last_event
            .is_none_or(|last| now.saturating_duration_since(last) >= WHEEL_RELEASE)
        {
            self.momentum = false;
        }
        let usable = delta_x != 0.0 && width > 0.0 && self.count > 0;
        let dragging = matches!(self.motion, Motion::Drag { .. });
        let consumed = match phase {
            TouchPhase::Started => {
                self.momentum = false;
                self.touch_down = true;
                if usable {
                    self.grab(false);
                    self.drag(delta_x, width, now);
                }
                usable
            }
            TouchPhase::Moved if self.momentum => delta_x != 0.0,
            TouchPhase::Moved => {
                if usable {
                    if !dragging {
                        self.grab(!self.touch_down);
                    }
                    self.drag(delta_x, width, now);
                }
                usable
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                self.touch_down = false;
                if dragging {
                    if usable {
                        self.drag(delta_x, width, now);
                    }
                    let cancel = phase == TouchPhase::Cancelled;
                    self.release(cancel, now, now);
                    self.momentum = !cancel;
                    true
                } else if self.momentum {
                    self.momentum = false;
                    true
                } else {
                    false
                }
            }
        };
        self.last_event = Some(now);
        PagerResponse {
            consumed,
            event: self.event.take(),
        }
    }

    pub fn tick(&mut self, now: Instant) -> bool {
        self.release_stale_wheel(now);
        match self.motion {
            Motion::Idle => false,
            Motion::Drag { wheel, .. } => wheel,
            Motion::Settle { velocity, at } => {
                let elapsed = now.saturating_duration_since(at).as_secs_f32();
                let state = SPRING.step(
                    SpringState {
                        position: self.shift,
                        velocity,
                    },
                    0.0,
                    elapsed,
                );
                if SPRING.is_settled(state, 0.0, SETTLE_EPSILON) {
                    self.shift = 0.0;
                    self.motion = Motion::Idle;
                    false
                } else {
                    self.shift = state.position.clamp(-1.0, 1.0);
                    self.motion = Motion::Settle {
                        velocity: state.velocity,
                        at: now,
                    };
                    true
                }
            }
        }
    }

    pub fn take_event(&mut self) -> Option<PagerEvent> {
        self.event.take()
    }

    pub fn go_to(&mut self, index: usize, now: Instant) {
        if self.count == 0 {
            return;
        }
        let index = index.min(self.count - 1);
        if index != self.index {
            self.pending = Some((self.index, now));
        }
        let velocity = match self.motion {
            Motion::Settle { velocity, .. } => velocity,
            _ => 0.0,
        };
        self.touch_down = false;
        self.samples.clear();
        self.shift = (self.shift + index as f32 - self.index as f32).clamp(-1.0, 1.0);
        self.index = index;
        self.motion = if self.shift == 0.0 {
            Motion::Idle
        } else {
            Motion::Settle { velocity, at: now }
        };
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn is_moving(&self) -> bool {
        !matches!(self.motion, Motion::Idle)
    }

    pub fn motion(&self, width: f32) -> f32 {
        if width <= 0.0 {
            return 0.0;
        }
        (self.shift.abs() * width / (MOTION_FRACTION * width)).min(1.0)
    }

    pub fn layout(&self, width: f32, gap: f32) -> PagerLayout {
        let mut pages = Vec::new();
        if self.count == 0 || width <= 0.0 {
            return PagerLayout { pages };
        }
        let unit = width + gap;
        let first = self.index.saturating_sub(1);
        let last = (self.index + 1).min(self.count - 1);
        for page in first..=last {
            let x = (page as f32 - self.index as f32 + self.shift) * unit;
            if x < width && x + width > 0.0 {
                pages.push((page, x));
            }
        }
        PagerLayout { pages }
    }

    fn retarget(&mut self, index: usize) {
        self.shift = (self.shift + index as f32 - self.index as f32).clamp(-1.0, 1.0);
        self.index = index;
    }

    fn grab(&mut self, wheel: bool) {
        let raw = match self.motion {
            Motion::Drag { raw, .. } => raw,
            _ if self.resisted(self.shift) => self.shift / RUBBER_BAND,
            _ => self.shift,
        };
        self.samples.clear();
        self.motion = Motion::Drag { raw, wheel };
    }

    fn drag(&mut self, delta_x: f32, width: f32, now: Instant) {
        let Motion::Drag { raw, wheel } = self.motion else {
            return;
        };
        self.width = width;
        let raw = raw + delta_x / width;
        self.shift = self.band(raw);
        self.motion = Motion::Drag { raw, wheel };
        self.samples.push_back((now, delta_x));
        while self
            .samples
            .front()
            .is_some_and(|(at, _)| now.saturating_duration_since(*at) > VELOCITY_WINDOW)
        {
            self.samples.pop_front();
        }
    }

    fn resisted(&self, shift: f32) -> bool {
        (shift > 0.0 && self.index == 0) || (shift < 0.0 && self.index + 1 >= self.count)
    }

    fn band(&self, raw: f32) -> f32 {
        let shift = if self.resisted(raw) {
            raw * RUBBER_BAND
        } else {
            raw
        };
        shift.clamp(-1.0, 1.0)
    }

    fn velocity(&self, reference: Instant) -> f32 {
        let recent = self
            .samples
            .iter()
            .filter(|(at, _)| reference.saturating_duration_since(*at) <= VELOCITY_WINDOW);
        let mut first = None;
        let mut total = 0.0;
        for (at, delta) in recent {
            first.get_or_insert(*at);
            total += delta;
        }
        let Some(first) = first else {
            return 0.0;
        };
        let span = reference.saturating_duration_since(first).max(FRAME);
        total / span.as_secs_f32()
    }

    fn release_target(&self, velocity: f32) -> usize {
        let fling = if velocity <= -FLING_VELOCITY {
            -1.0
        } else if velocity >= FLING_VELOCITY {
            1.0
        } else {
            0.0
        };
        let side = if self.shift > 0.0 {
            1.0
        } else if self.shift < 0.0 {
            -1.0
        } else {
            0.0
        };
        let direction = if fling == 0.0 {
            if self.shift.abs() > COMMIT_FRACTION {
                side
            } else {
                0.0
            }
        } else if side == 0.0 || side == fling {
            fling
        } else {
            0.0
        };
        if direction < 0.0 && self.index + 1 < self.count {
            self.index + 1
        } else if direction > 0.0 && self.index > 0 {
            self.index - 1
        } else {
            self.index
        }
    }

    fn release(&mut self, cancel: bool, reference: Instant, now: Instant) {
        let velocity = if self.width > 0.0 {
            self.velocity(reference)
        } else {
            0.0
        };
        let target = if cancel {
            self.index
        } else {
            self.release_target(velocity)
        };
        let deferred = self.deferred.take();
        if target != self.index {
            self.pending = Some((self.index, now));
            self.retarget(target);
            self.event = Some(PagerEvent::Commit { index: target });
        } else if let Some(index) = deferred {
            self.retarget(index.min(self.count.saturating_sub(1)));
        }
        self.samples.clear();
        let (frequency, _) = SPRING.canonical();
        let pages = if self.width > 0.0 {
            velocity / self.width
        } else {
            0.0
        };
        let velocity = if pages * self.shift < 0.0 {
            let limit = frequency * self.shift.abs();
            pages.clamp(-limit, limit)
        } else {
            0.0
        };
        self.motion = if self.shift == 0.0 && velocity == 0.0 {
            Motion::Idle
        } else {
            Motion::Settle { velocity, at: now }
        };
    }

    fn release_stale_wheel(&mut self, now: Instant) {
        if let Motion::Drag { wheel: true, .. } = self.motion
            && let Some(last) = self.last_event
            && now.saturating_duration_since(last) >= WHEEL_RELEASE
        {
            self.release(false, last, now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDTH: f32 = 400.0;

    struct Clock(Instant);

    impl Clock {
        fn new() -> Self {
            Self(Instant::now())
        }

        fn at(&self, ms: u64) -> Instant {
            self.0 + Duration::from_millis(ms)
        }
    }

    fn pager(index: usize, count: usize) -> Pager {
        let mut pager = Pager::new();
        pager.sync(index, count);
        pager
    }

    fn settle(pager: &mut Pager, clock: &Clock, from: u64) -> u64 {
        let mut ms = from;
        while pager.tick(clock.at(ms)) {
            ms += 16;
            assert!(ms < from + 2000, "pager never settled");
        }
        ms
    }

    fn slow_drag(pager: &mut Pager, clock: &Clock, deltas: &[f32]) -> u64 {
        let mut ms = 0;
        for (step, delta) in deltas.iter().enumerate() {
            let phase = if step == 0 {
                TouchPhase::Started
            } else {
                TouchPhase::Moved
            };
            assert!(pager.scroll(*delta, phase, WIDTH, clock.at(ms)).consumed);
            ms += 150;
        }
        ms + 200
    }

    #[test]
    fn content_follows_the_finger() {
        let clock = Clock::new();
        let mut pager = pager(1, 3);
        pager.scroll(50.0, TouchPhase::Started, WIDTH, clock.at(0));
        assert_eq!(
            pager.layout(WIDTH, 0.0).pages,
            vec![(0, 50.0 - WIDTH), (1, 50.0)]
        );
        pager.scroll(-100.0, TouchPhase::Moved, WIDTH, clock.at(16));
        assert_eq!(
            pager.layout(WIDTH, 0.0).pages,
            vec![(1, -50.0), (2, WIDTH - 50.0)]
        );
    }

    #[test]
    fn a_drag_past_a_quarter_commits() {
        let clock = Clock::new();
        let mut pager = pager(1, 3);
        let release = slow_drag(&mut pager, &clock, &[-40.0, -40.0, -40.0]);
        let response = pager.scroll(0.0, TouchPhase::Ended, WIDTH, clock.at(release));
        assert!(response.consumed);
        assert_eq!(response.event, Some(PagerEvent::Commit { index: 2 }));
        assert_eq!(pager.index(), 2);
        assert!(pager.is_moving());
        settle(&mut pager, &clock, release);
        assert!(!pager.is_moving());
        assert_eq!(pager.layout(WIDTH, 8.0).pages, vec![(2, 0.0)]);
    }

    #[test]
    fn a_short_drag_returns() {
        let clock = Clock::new();
        let mut pager = pager(1, 3);
        let release = slow_drag(&mut pager, &clock, &[-30.0, -30.0]);
        let response = pager.scroll(0.0, TouchPhase::Ended, WIDTH, clock.at(release));
        assert_eq!(response.event, None);
        assert_eq!(pager.index(), 1);
        settle(&mut pager, &clock, release);
        assert_eq!(pager.layout(WIDTH, 8.0).pages, vec![(1, 0.0)]);
    }

    #[test]
    fn a_flick_commits_by_velocity() {
        let clock = Clock::new();
        let mut pager = pager(1, 3);
        pager.scroll(10.0, TouchPhase::Started, WIDTH, clock.at(0));
        for ms in [16, 32, 48] {
            pager.scroll(10.0, TouchPhase::Moved, WIDTH, clock.at(ms));
        }
        let response = pager.scroll(0.0, TouchPhase::Ended, WIDTH, clock.at(56));
        assert_eq!(response.event, Some(PagerEvent::Commit { index: 0 }));
        assert_eq!(pager.index(), 0);
    }

    #[test]
    fn a_flick_against_the_drag_stays() {
        let clock = Clock::new();
        let mut pager = pager(1, 3);
        let mut ms = slow_drag(&mut pager, &clock, &[-60.0, -60.0]);
        for _ in 0..3 {
            pager.scroll(15.0, TouchPhase::Moved, WIDTH, clock.at(ms));
            ms += 16;
        }
        let response = pager.scroll(0.0, TouchPhase::Ended, WIDTH, clock.at(ms));
        assert_eq!(response.event, None);
        assert_eq!(pager.index(), 1);
    }

    #[test]
    fn edges_rubber_band_and_never_commit_out_of_range() {
        let clock = Clock::new();
        let mut pager = pager(0, 2);
        pager.scroll(100.0, TouchPhase::Started, WIDTH, clock.at(0));
        pager.scroll(100.0, TouchPhase::Moved, WIDTH, clock.at(16));
        let shown = pager.layout(WIDTH, 0.0).pages;
        assert_eq!(shown.len(), 1);
        assert!((shown[0].1 - 200.0 * RUBBER_BAND).abs() < 0.01, "{shown:?}");
        let response = pager.scroll(0.0, TouchPhase::Ended, WIDTH, clock.at(24));
        assert_eq!(response.event, None);
        assert_eq!(pager.index(), 0);
        settle(&mut pager, &clock, 24);

        let mut pager = pager_at_end();
        pager.scroll(-200.0, TouchPhase::Started, WIDTH, clock.at(0));
        pager.scroll(-200.0, TouchPhase::Moved, WIDTH, clock.at(16));
        let response = pager.scroll(-200.0, TouchPhase::Ended, WIDTH, clock.at(24));
        assert_eq!(response.event, None);
        assert_eq!(pager.index(), 1);
        assert!(
            pager
                .layout(WIDTH, 0.0)
                .pages
                .iter()
                .all(|(page, _)| *page <= 1)
        );
    }

    fn pager_at_end() -> Pager {
        pager(1, 2)
    }

    #[test]
    fn momentum_after_release_is_ignored() {
        let clock = Clock::new();
        let mut pager = pager(0, 4);
        pager.scroll(-60.0, TouchPhase::Started, WIDTH, clock.at(0));
        pager.scroll(-60.0, TouchPhase::Moved, WIDTH, clock.at(16));
        let response = pager.scroll(0.0, TouchPhase::Ended, WIDTH, clock.at(24));
        assert_eq!(response.event, Some(PagerEvent::Commit { index: 1 }));
        let mut ms = 32;
        for _ in 0..30 {
            let response = pager.scroll(-80.0, TouchPhase::Moved, WIDTH, clock.at(ms));
            assert!(response.consumed);
            assert_eq!(response.event, None);
            pager.tick(clock.at(ms));
            ms += 16;
        }
        let response = pager.scroll(0.0, TouchPhase::Ended, WIDTH, clock.at(ms));
        assert!(response.consumed);
        assert_eq!(pager.index(), 1);
        settle(&mut pager, &clock, ms);
        assert_eq!(pager.layout(WIDTH, 0.0).pages, vec![(1, 0.0)]);
        let next = pager.scroll(-10.0, TouchPhase::Started, WIDTH, clock.at(ms + 400));
        assert!(next.consumed);
        assert!(pager.is_moving());
    }

    #[test]
    fn a_wheel_without_phases_releases_after_a_quiet_spell() {
        let clock = Clock::new();
        let mut pager = pager(0, 3);
        for ms in [0, 50, 100] {
            let response = pager.scroll(-50.0, TouchPhase::Moved, WIDTH, clock.at(ms));
            assert!(response.consumed);
        }
        assert!(pager.tick(clock.at(150)));
        assert_eq!(pager.take_event(), None);
        assert_eq!(pager.index(), 0);
        assert!(pager.tick(clock.at(220)));
        assert_eq!(pager.take_event(), Some(PagerEvent::Commit { index: 1 }));
        assert_eq!(pager.index(), 1);
        settle(&mut pager, &clock, 220);
        assert!(!pager.is_moving());
    }

    #[test]
    fn a_stale_wheel_drag_releases_on_the_next_scroll() {
        let clock = Clock::new();
        let mut pager = pager(0, 3);
        pager.scroll(-150.0, TouchPhase::Moved, WIDTH, clock.at(0));
        let response = pager.scroll(-10.0, TouchPhase::Moved, WIDTH, clock.at(300));
        assert_eq!(response.event, Some(PagerEvent::Commit { index: 1 }));
        assert!(response.consumed);
    }

    #[test]
    fn vertical_events_are_not_consumed() {
        let clock = Clock::new();
        let mut pager = pager(0, 3);
        assert!(
            !pager
                .scroll(0.0, TouchPhase::Started, WIDTH, clock.at(0))
                .consumed
        );
        assert!(
            !pager
                .scroll(0.0, TouchPhase::Moved, WIDTH, clock.at(16))
                .consumed
        );
        assert!(
            !pager
                .scroll(0.0, TouchPhase::Ended, WIDTH, clock.at(32))
                .consumed
        );
        assert!(!pager.is_moving());
    }

    #[test]
    fn a_zero_delta_start_still_marks_a_touch() {
        let clock = Clock::new();
        let mut pager = pager(0, 3);
        pager.scroll(0.0, TouchPhase::Started, WIDTH, clock.at(0));
        pager.scroll(-150.0, TouchPhase::Moved, WIDTH, clock.at(150));
        assert!(!pager.tick(clock.at(400)));
        assert_eq!(pager.index(), 0);
        let response = pager.scroll(0.0, TouchPhase::Ended, WIDTH, clock.at(500));
        assert_eq!(response.event, Some(PagerEvent::Commit { index: 1 }));
    }

    #[test]
    fn cancel_returns_without_committing() {
        let clock = Clock::new();
        let mut pager = pager(0, 3);
        pager.scroll(-300.0, TouchPhase::Started, WIDTH, clock.at(0));
        let response = pager.scroll(0.0, TouchPhase::Cancelled, WIDTH, clock.at(16));
        assert_eq!(response.event, None);
        assert_eq!(pager.index(), 0);
        let response = pager.scroll(-10.0, TouchPhase::Moved, WIDTH, clock.at(32));
        assert!(response.consumed);
        assert!(matches!(pager.motion, Motion::Drag { wheel: true, .. }));
    }

    #[test]
    fn sync_while_idle_jumps() {
        let mut pager = pager(0, 5);
        pager.sync(3, 5);
        assert_eq!(pager.index(), 3);
        assert!(!pager.is_moving());
        assert_eq!(pager.layout(WIDTH, 8.0).pages, vec![(3, 0.0)]);
        pager.sync(9, 2);
        assert_eq!(pager.index(), 1);
    }

    #[test]
    fn sync_while_settling_retargets() {
        let clock = Clock::new();
        let mut pager = pager(1, 4);
        pager.scroll(-30.0, TouchPhase::Started, WIDTH, clock.at(0));
        pager.scroll(0.0, TouchPhase::Ended, WIDTH, clock.at(500));
        assert!(pager.is_moving());
        pager.sync(2, 4);
        assert_eq!(pager.index(), 2);
        assert!(pager.is_moving());
        let shown = pager.layout(WIDTH, 0.0).pages;
        assert_eq!(shown.first().map(|page| page.0), Some(1));
        settle(&mut pager, &clock, 500);
        assert_eq!(pager.layout(WIDTH, 0.0).pages, vec![(2, 0.0)]);
    }

    #[test]
    fn sync_while_the_finger_is_down_only_clamps() {
        let clock = Clock::new();
        let mut pager = pager(2, 4);
        pager.scroll(-30.0, TouchPhase::Started, WIDTH, clock.at(0));
        pager.sync(0, 4);
        assert_eq!(pager.index(), 2);
        pager.sync(0, 2);
        assert_eq!(pager.index(), 1);
    }

    #[test]
    fn a_sync_during_a_drag_lands_after_a_release_that_stays() {
        let clock = Clock::new();
        let mut pager = pager(1, 4);
        pager.scroll(-30.0, TouchPhase::Started, WIDTH, clock.at(0));
        pager.sync(3, 4);
        assert_eq!(pager.index(), 1);
        let response = pager.scroll(0.0, TouchPhase::Ended, WIDTH, clock.at(400));
        assert_eq!(response.event, None);
        assert_eq!(pager.index(), 3);
        settle(&mut pager, &clock, 400);
        assert_eq!(pager.layout(WIDTH, 0.0).pages, vec![(3, 0.0)]);

        pager.scroll(-200.0, TouchPhase::Started, WIDTH, clock.at(3000));
        pager.sync(0, 4);
        let response = pager.scroll(0.0, TouchPhase::Ended, WIDTH, clock.at(3400));
        assert_eq!(response.event, None);
        assert_eq!(pager.index(), 0);
    }

    #[test]
    fn a_vertical_touch_does_not_block_sync() {
        let clock = Clock::new();
        let mut pager = pager(0, 4);
        pager.scroll(0.0, TouchPhase::Started, WIDTH, clock.at(0));
        pager.sync(2, 4);
        assert_eq!(pager.index(), 2);
        assert!(!pager.is_moving());
    }

    #[test]
    fn sync_ignores_the_stale_index_after_a_commit() {
        let clock = Clock::new();
        let mut pager = pager(0, 3);
        pager.scroll(-200.0, TouchPhase::Started, WIDTH, clock.at(0));
        pager.scroll(0.0, TouchPhase::Ended, WIDTH, clock.at(400));
        assert_eq!(pager.index(), 1);
        pager.sync(0, 3);
        assert_eq!(pager.index(), 1);
        pager.sync(1, 3);
        pager.sync(0, 3);
        assert_eq!(pager.index(), 0);
    }

    #[test]
    fn one_page_per_gesture() {
        let clock = Clock::new();
        let mut pager = pager(0, 5);
        pager.scroll(-300.0, TouchPhase::Started, WIDTH, clock.at(0));
        for ms in [16, 32, 48, 64] {
            pager.scroll(-300.0, TouchPhase::Moved, WIDTH, clock.at(ms));
        }
        assert_eq!(pager.layout(WIDTH, 0.0).pages, vec![(1, 0.0)]);
        let response = pager.scroll(-300.0, TouchPhase::Ended, WIDTH, clock.at(72));
        assert_eq!(response.event, Some(PagerEvent::Commit { index: 1 }));
        settle(&mut pager, &clock, 72);
        assert_eq!(pager.index(), 1);
    }

    #[test]
    fn layout_shows_the_neighbour_only_while_moving() {
        let clock = Clock::new();
        let mut pager = pager(1, 3);
        assert_eq!(pager.layout(WIDTH, 8.0).pages, vec![(1, 0.0)]);
        assert_eq!(pager.motion(WIDTH), 0.0);
        pager.scroll(-100.0, TouchPhase::Started, WIDTH, clock.at(0));
        assert_eq!(
            pager.layout(WIDTH, 8.0).pages,
            vec![(1, -102.0), (2, 306.0)]
        );
        assert_eq!(pager.motion(WIDTH), 1.0);
        pager.scroll(0.0, TouchPhase::Ended, WIDTH, clock.at(400));
        settle(&mut pager, &clock, 400);
        assert_eq!(pager.layout(WIDTH, 8.0).pages, vec![(1, 0.0)]);
    }

    #[test]
    fn go_to_animates_without_an_event() {
        let clock = Clock::new();
        let mut pager = pager(0, 3);
        pager.go_to(1, clock.at(0));
        assert_eq!(pager.index(), 1);
        assert!(pager.is_moving());
        assert_eq!(pager.layout(WIDTH, 0.0).pages, vec![(0, 0.0)]);
        assert!(pager.tick(clock.at(16)));
        assert_eq!(pager.layout(WIDTH, 0.0).pages.len(), 2);
        settle(&mut pager, &clock, 16);
        assert_eq!(pager.take_event(), None);
        assert_eq!(pager.layout(WIDTH, 0.0).pages, vec![(1, 0.0)]);
    }

    #[test]
    fn an_empty_pager_consumes_nothing() {
        let clock = Clock::new();
        let mut pager = Pager::new();
        assert!(
            !pager
                .scroll(-50.0, TouchPhase::Started, WIDTH, clock.at(0))
                .consumed
        );
        assert!(pager.layout(WIDTH, 0.0).pages.is_empty());
    }
}
