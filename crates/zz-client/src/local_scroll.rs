use std::time::Duration;

use web_time::Instant;
use zz_terminal::{ScrollbarState, TerminalMode, TerminalViewport};

use crate::scrollback::RetainedTerminalViewport;

pub const LOCAL_SCROLL_DEBOUNCE: Duration = Duration::from_millis(120);
pub const LOCAL_SCROLL_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug)]
pub struct LocalScroll {
    target_offset: u32,
    requested_offset: Option<u32>,
    request_floor: Option<u32>,
    started: Instant,
}

impl LocalScroll {
    pub fn new(target_offset: u32, started: Instant) -> Self {
        Self {
            target_offset,
            requested_offset: None,
            request_floor: None,
            started,
        }
    }

    pub fn settled_at(self, server_offset: u32) -> bool {
        server_offset == self.target_offset
            && self
                .requested_offset
                .is_none_or(|requested| requested == self.target_offset)
    }

    pub fn record_request(&mut self, offset: u32) {
        self.requested_offset = Some(offset);
        self.request_floor = Some(self.request_floor.map_or(offset, |floor| floor.min(offset)));
    }

    pub fn observe_server(&mut self, server_offset: u32) {
        if self.request_floor == Some(server_offset) || self.requested_offset == Some(server_offset)
        {
            self.request_floor = None;
        }
    }

    pub fn reach(self, server_offset: u32) -> u32 {
        self.request_floor
            .map_or(server_offset, |floor| floor.min(server_offset))
    }
}

pub fn local_scroll_gate(viewport: &TerminalViewport, history_rows: usize) -> bool {
    matches!(viewport.mode, TerminalMode::Live)
        && !viewport.mouse_tracking
        && viewport.scrollbar.total > viewport.scrollbar.len
        && history_rows != 0
}

pub fn local_scroll_should_retire(
    local_scroll: LocalScroll,
    server_offset: u32,
    expected_history_invalidations: u64,
    history_invalidations: u64,
    now: Instant,
) -> bool {
    local_scroll.settled_at(server_offset)
        || expected_history_invalidations != history_invalidations
        || now.saturating_duration_since(local_scroll.started) >= LOCAL_SCROLL_TIMEOUT
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PixelScrollPosition {
    pub target: u32,
    pub sub_row: f32,
}

pub fn pixel_scroll_position(
    target: u32,
    sub_row: f32,
    delta: f32,
    line_height: f32,
    oldest: u32,
    newest: u32,
) -> PixelScrollPosition {
    let height = f64::from(line_height);
    let lower = (f64::from(oldest) - f64::from(target)) * height;
    let upper = (f64::from(newest) - f64::from(target)) * height;
    let relative = (-f64::from(sub_row) - f64::from(delta))
        .max(lower)
        .min(upper);
    let mut rows = (relative / height).ceil();
    let mut sub_row = rows * height - relative;
    if sub_row >= height {
        rows -= 1.0;
        sub_row -= height;
    }
    if sub_row < 1e-3 {
        sub_row = 0.0;
    }
    let target = (f64::from(target) + rows).clamp(0.0, f64::from(u32::MAX)) as u32;
    PixelScrollPosition {
        target,
        sub_row: sub_row as f32,
    }
}

pub fn daemon_follow_offset(
    target: u32,
    reference: u32,
    rows: u16,
    maximum: u32,
    toward_newer: bool,
) -> Option<u32> {
    let rows = u32::from(rows.max(1));
    let desired = target.saturating_add((rows / 2).max(1)).min(maximum);
    let lead = reference.saturating_sub(target);
    let needed = if toward_newer {
        reference < target || lead < (rows / 4).max(1)
    } else {
        lead > (rows * 3 / 4).max(1)
    };
    (needed && reference != desired).then_some(desired)
}

pub fn scroll_fraction_offset(fraction: u32, maximum: u32) -> u32 {
    u32::try_from(u128::from(maximum).saturating_mul(u128::from(fraction)) / u128::from(u32::MAX))
        .unwrap_or(maximum)
}

pub fn local_scroll_needs_prefetch(
    target_offset: u32,
    scrollbar: ScrollbarState,
    history_rows: usize,
) -> bool {
    let Ok(history_rows) = u32::try_from(history_rows) else {
        return false;
    };
    let front = scrollbar.offset.saturating_sub(history_rows);
    target_offset < front.saturating_add(scrollbar.len.saturating_mul(2))
}

pub fn local_scroll_available(retained: &RetainedTerminalViewport) -> bool {
    local_scroll_gate(&retained.viewport, retained.history.len())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalScrollEffect {
    ScrollToOffset(u32),
    Prefetch(u32),
    Sync(u64),
}

#[derive(Debug, Default, PartialEq)]
pub struct LocalScrollStep {
    pub redraw: bool,
    pub effects: Vec<LocalScrollEffect>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalScrollSync {
    Retired,
    Requested(u32),
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LocalScrollState {
    local: Option<LocalScroll>,
    generation: u64,
    sub_row: f32,
    observed_invalidations: u64,
}

impl LocalScrollState {
    pub fn new(observed_invalidations: u64) -> Self {
        Self {
            observed_invalidations,
            ..Self::default()
        }
    }

    pub fn target(&self) -> Option<u32> {
        self.local.map(|local| local.target_offset)
    }

    pub const fn sub_row(&self) -> f32 {
        self.sub_row
    }

    pub fn clear(&mut self) -> bool {
        let cleared = self.local.take().is_some();
        if cleared {
            self.generation = self.generation.wrapping_add(1);
        }
        cleared
    }

    pub fn cancel(&mut self) -> bool {
        self.generation = self.generation.wrapping_add(1);
        let had_sub_row = self.clear_sub_row();
        self.local.take().is_some() || had_sub_row
    }

    pub fn clear_sub_row(&mut self) -> bool {
        std::mem::replace(&mut self.sub_row, 0.0) > 0.0
    }

    pub fn flush(&mut self) -> Option<u32> {
        let local = self.local.as_mut()?;
        if local.requested_offset == Some(local.target_offset) {
            return None;
        }
        let target = local.target_offset;
        local.record_request(target);
        Some(target)
    }

    pub fn release(&mut self) -> Option<u32> {
        let target = self.target()?;
        self.clear();
        Some(target)
    }

    pub fn scroll_by_pixels(
        &mut self,
        delta: f32,
        line_height: f32,
        retained: &RetainedTerminalViewport,
        now: Instant,
    ) -> Option<LocalScrollStep> {
        if line_height <= 0.0 {
            return None;
        }
        let mut step = LocalScrollStep::default();
        if delta == 0.0 {
            return Some(step);
        }
        let scrollbar = retained.viewport.scrollbar;
        let server_offset = scrollbar.offset;
        let maximum_offset = scrollbar.total.saturating_sub(scrollbar.len);
        let coverage_start = scrollbar
            .offset
            .saturating_sub(u32::try_from(retained.history.len()).unwrap_or(u32::MAX));
        if let Some(local) = self.local.as_mut() {
            local.observe_server(server_offset);
        }
        let local = self.local;
        let position = pixel_scroll_position(
            local.map_or(server_offset, |local| local.target_offset),
            self.sub_row,
            delta,
            line_height,
            coverage_start,
            local
                .map_or(server_offset, |local| local.reach(server_offset))
                .min(maximum_offset),
        );
        let follow = daemon_follow_offset(
            position.target,
            local
                .and_then(|local| local.requested_offset)
                .unwrap_or(server_offset),
            retained.viewport.rows,
            maximum_offset,
            delta < 0.0,
        );
        if self.sub_row != position.sub_row {
            self.sub_row = position.sub_row;
            step.redraw = true;
        }
        self.pixel_scroll_to(position.target, follow, retained, now, &mut step);
        Some(step)
    }

    fn pixel_scroll_to(
        &mut self,
        target: u32,
        follow: Option<u32>,
        retained: &RetainedTerminalViewport,
        now: Instant,
        step: &mut LocalScrollStep,
    ) {
        let server_offset = retained.viewport.scrollbar.offset;
        let previous = self.local;
        let moved = previous.is_none_or(|local| local.target_offset != target);
        let mut next = match previous {
            Some(local) if !moved => local,
            Some(local) => LocalScroll {
                target_offset: target,
                started: now,
                ..local
            },
            None => LocalScroll::new(target, now),
        };
        if let Some(offset) = follow {
            next.record_request(offset);
            step.effects.push(LocalScrollEffect::ScrollToOffset(offset));
        }
        if next.settled_at(server_offset) {
            step.redraw |= self.clear();
            return;
        }
        if !moved && follow.is_none() {
            return;
        }
        self.observed_invalidations = retained.history_invalidations;
        self.generation = self.generation.wrapping_add(1);
        self.local = Some(next);
        step.redraw = true;
        if moved {
            push_prefetch(target, retained, step);
        }
        step.effects.push(LocalScrollEffect::Sync(self.generation));
    }

    pub fn scroll_by_rows(
        &mut self,
        delta: i64,
        retained: &RetainedTerminalViewport,
        now: Instant,
    ) -> LocalScrollStep {
        let scrollbar = retained.viewport.scrollbar;
        let base = self.target().unwrap_or(scrollbar.offset);
        let target = i128::from(base)
            .saturating_add(i128::from(delta))
            .clamp(0, i128::from(scrollbar.total.saturating_sub(scrollbar.len)));
        self.scroll_to(
            u32::try_from(target).unwrap_or(scrollbar.offset),
            retained,
            now,
        )
    }

    pub fn scroll_to(
        &mut self,
        target: u32,
        retained: &RetainedTerminalViewport,
        now: Instant,
    ) -> LocalScrollStep {
        let mut step = LocalScrollStep::default();
        let scrollbar = retained.viewport.scrollbar;
        let coverage_start = scrollbar
            .offset
            .saturating_sub(u32::try_from(retained.history.len()).unwrap_or(u32::MAX));
        let target = target.min(scrollbar.total.saturating_sub(scrollbar.len));
        let server_offset = scrollbar.offset;
        let at_tail = scrollbar.offset.saturating_add(scrollbar.len) >= scrollbar.total;
        self.observed_invalidations = retained.history_invalidations;
        if target < coverage_start {
            push_prefetch(target, retained, &mut step);
            step.redraw = self.clear();
            step.effects.push(LocalScrollEffect::ScrollToOffset(target));
            return step;
        }
        if target > server_offset {
            step.redraw = self.clear();
            step.effects.push(LocalScrollEffect::ScrollToOffset(target));
            return step;
        }
        if target == server_offset {
            step.redraw = self.clear();
            if at_tail {
                step.effects.push(LocalScrollEffect::ScrollToOffset(target));
            }
            return step;
        }
        self.generation = self.generation.wrapping_add(1);
        self.local = Some(LocalScroll {
            target_offset: target,
            started: now,
            ..self.local.unwrap_or_else(|| LocalScroll::new(target, now))
        });
        step.redraw = true;
        push_prefetch(target, retained, &mut step);
        step.effects.push(LocalScrollEffect::Sync(self.generation));
        step
    }

    pub fn sync(
        &mut self,
        generation: u64,
        retained: &RetainedTerminalViewport,
    ) -> Option<LocalScrollSync> {
        if self.generation != generation {
            return None;
        }
        let server_offset = retained.viewport.scrollbar.offset;
        let local = self.local.as_mut()?;
        local.observe_server(server_offset);
        let local = *local;
        if local.settled_at(server_offset) || !local_scroll_available(retained) {
            self.clear();
            return Some(LocalScrollSync::Retired);
        }
        if let Some(local) = self.local.as_mut() {
            local.record_request(local.target_offset);
        }
        Some(LocalScrollSync::Requested(local.target_offset))
    }

    pub fn expire(&mut self, generation: u64, now: Instant) -> bool {
        if self.generation != generation {
            return false;
        }
        self.local.is_some_and(|local| {
            now.saturating_duration_since(local.started) >= LOCAL_SCROLL_TIMEOUT
        }) && self.clear()
    }

    pub fn observe(
        &mut self,
        retained: &RetainedTerminalViewport,
        replaced: bool,
        now: Instant,
    ) -> bool {
        let server_offset = retained.viewport.scrollbar.offset;
        let available = local_scroll_available(retained);
        let invalidations = retained.history_invalidations;
        if let Some(local) = self.local.as_mut() {
            local.observe_server(server_offset);
        }
        let mut changed = false;
        if self.local.is_some_and(|local| {
            replaced
                || !available
                || local_scroll_should_retire(
                    local,
                    server_offset,
                    self.observed_invalidations,
                    invalidations,
                    now,
                )
        }) {
            self.clear();
            changed = true;
        }
        if self.sub_row > 0.0
            && (replaced || !available || invalidations != self.observed_invalidations)
        {
            self.sub_row = 0.0;
            changed = true;
        }
        self.observed_invalidations = invalidations;
        changed
    }

    pub fn overscroll_room(
        &self,
        retained: &RetainedTerminalViewport,
        line_height: f32,
    ) -> (f32, f32) {
        let scrollbar = retained.viewport.scrollbar;
        let target = self.target().unwrap_or(scrollbar.offset);
        let maximum = scrollbar.total.saturating_sub(scrollbar.len);
        let height = f64::from(line_height);
        let sub_row = f64::from(self.sub_row);
        (
            (f64::from(maximum.saturating_sub(target)) * height + sub_row) as f32,
            (f64::from(target) * height - sub_row).max(0.0) as f32,
        )
    }
}

fn push_prefetch(target: u32, retained: &RetainedTerminalViewport, step: &mut LocalScrollStep) {
    if local_scroll_needs_prefetch(target, retained.viewport.scrollbar, retained.history.len()) {
        step.effects.push(LocalScrollEffect::Prefetch(target));
    }
}

#[cfg(test)]
mod tests {
    use zz_terminal::SessionStatus;

    use super::*;

    #[test]
    fn local_scroll_gate_requires_live_untracked_scrollback_and_a_warm_ring() {
        let mut viewport = TerminalViewport::blank(80, 24, SessionStatus::Running);
        viewport.scrollbar = ScrollbarState {
            total: 100,
            offset: 76,
            len: 24,
        };
        assert!(local_scroll_gate(&viewport, 1));

        viewport.mode = TerminalMode::Copy {
            position: 1,
            total: 100,
            hide_position: false,
        };
        assert!(!local_scroll_gate(&viewport, 1));
        viewport.mode = TerminalMode::View {
            position: 1,
            total: 100,
        };
        assert!(!local_scroll_gate(&viewport, 1));
        viewport.mode = TerminalMode::Live;

        viewport.mouse_tracking = true;
        assert!(!local_scroll_gate(&viewport, 1));
        viewport.mouse_tracking = false;

        viewport.scrollbar.total = viewport.scrollbar.len;
        assert!(!local_scroll_gate(&viewport, 1));
        viewport.scrollbar.total = 100;
        assert!(!local_scroll_gate(&viewport, 0));
    }

    #[test]
    fn local_scroll_retires_on_convergence_timeout_or_ring_invalidation() {
        let started = Instant::now();
        let local_scroll = LocalScroll {
            started,
            ..LocalScroll::new(40, started)
        };
        assert!(!local_scroll_should_retire(
            local_scroll,
            60,
            7,
            7,
            (started + LOCAL_SCROLL_TIMEOUT)
                .checked_sub(Duration::from_millis(1))
                .unwrap(),
        ));
        assert!(local_scroll_should_retire(local_scroll, 40, 7, 7, started,));
        assert!(local_scroll_should_retire(
            local_scroll,
            60,
            7,
            7,
            started + LOCAL_SCROLL_TIMEOUT,
        ));
        assert!(local_scroll_should_retire(local_scroll, 60, 7, 8, started,));

        let mut following = local_scroll;
        following.record_request(60);
        assert!(!local_scroll_should_retire(following, 40, 7, 7, started));
        following.record_request(40);
        assert!(local_scroll_should_retire(following, 40, 7, 7, started));
    }

    fn at(target: u32, sub_row: f32) -> PixelScrollPosition {
        PixelScrollPosition { target, sub_row }
    }

    #[test]
    fn trackpad_pixels_accumulate_and_carry_whole_rows_both_ways() {
        let step =
            |target, sub_row, delta| pixel_scroll_position(target, sub_row, delta, 20.0, 50, 100);
        assert_eq!(step(100, 0.0, 5.0), at(100, 5.0));
        assert_eq!(step(100, 5.0, 20.0), at(99, 5.0));
        assert_eq!(step(99, 5.0, -10.0), at(100, 15.0));
        assert_eq!(step(100, 0.0, 45.0), at(98, 5.0));
        assert_eq!(step(98, 5.0, -45.0), at(100, 0.0));
        assert_eq!(step(100, 0.0, 20.0), at(99, 0.0));
    }

    #[test]
    fn trackpad_pixels_clamp_at_the_live_bottom_and_the_rings_oldest_row() {
        let step = |target, sub_row, delta, oldest| {
            pixel_scroll_position(target, sub_row, delta, 20.0, oldest, 100)
        };
        assert_eq!(step(100, 0.0, -30.0, 50), at(100, 0.0));
        assert_eq!(step(100, 5.0, -30.0, 50), at(100, 0.0));
        assert_eq!(step(99, 5.0, 50.0, 98), at(98, 0.0));
        assert_eq!(step(98, 0.0, 7.0, 98), at(98, 0.0));
        assert_eq!(step(0, 0.0, 10.0, 0), at(0, 0.0));
    }

    #[test]
    fn trackpad_offsets_stay_exact_deep_in_scrollback() {
        let mut position = at(3_000_000, 0.0);
        for _ in 0..80 {
            position =
                pixel_scroll_position(position.target, position.sub_row, 0.25, 17.5, 0, 3_000_000);
        }
        assert_eq!(position.target, 2_999_999);
        assert!((position.sub_row - 2.5).abs() < 1e-3);
    }

    #[test]
    fn the_daemon_follows_a_gesture_in_steps_smaller_than_a_screen() {
        assert_eq!(daemon_follow_offset(1000, 1000, 24, 1000, false), None);
        assert_eq!(daemon_follow_offset(982, 1000, 24, 1000, false), None);
        assert_eq!(daemon_follow_offset(981, 1000, 24, 1000, false), Some(993));
        assert_eq!(daemon_follow_offset(500, 500, 24, 1000, false), None);

        assert_eq!(daemon_follow_offset(500, 500, 24, 1000, true), Some(512));
        assert_eq!(daemon_follow_offset(506, 512, 24, 1000, true), None);
        assert_eq!(daemon_follow_offset(507, 512, 24, 1000, true), Some(519));
        assert_eq!(daemon_follow_offset(995, 995, 24, 1000, true), Some(1000));
        assert_eq!(daemon_follow_offset(999, 1000, 24, 1000, true), None);
        assert_eq!(daemon_follow_offset(10, 9, 1, 1000, true), Some(11));
    }

    #[test]
    fn outstanding_requests_cap_the_reach_until_the_daemon_passes_them() {
        let mut local_scroll = LocalScroll::new(480, Instant::now());
        local_scroll.record_request(480);
        local_scroll.record_request(500);
        assert_eq!(local_scroll.reach(510), 480);
        local_scroll.observe_server(510);
        assert_eq!(local_scroll.reach(510), 480);
        local_scroll.observe_server(480);
        assert_eq!(local_scroll.reach(480), 480);
        assert_eq!(local_scroll.reach(500), 500);
        assert!(!local_scroll.settled_at(480));

        let mut coalesced = LocalScroll::new(480, Instant::now());
        coalesced.record_request(480);
        coalesced.record_request(500);
        coalesced.observe_server(500);
        assert_eq!(coalesced.reach(500), 500);
    }

    #[test]
    fn local_scroll_prefetches_only_near_the_cold_edge() {
        let scrollbar = ScrollbarState {
            total: 110,
            offset: 100,
            len: 10,
        };
        assert!(local_scroll_needs_prefetch(99, scrollbar, 20));
        assert!(!local_scroll_needs_prefetch(100, scrollbar, 20));
    }

    fn scrolled_back(history_rows: u64) -> RetainedTerminalViewport {
        let mut next_revision = 1;
        let mut viewport = TerminalViewport::blank(80, 24, SessionStatus::Running);
        viewport.scrollbar = ScrollbarState {
            total: 1024,
            offset: 1000,
            len: 24,
        };
        let mut retained = crate::scrollback::new_retained_viewport(viewport, &mut next_revision);
        retained.history.rows = (0..history_rows)
            .map(|revision| crate::scrollback::HistoryRow {
                cells: Box::from([]),
                dictionary: std::sync::Arc::default(),
                revision,
            })
            .collect();
        retained
    }

    #[test]
    fn pixel_steps_stay_local_and_lead_the_daemon_by_half_a_screen() {
        let retained = scrolled_back(200);
        let now = Instant::now();
        let mut state = LocalScrollState::new(retained.history_invalidations);

        let step = state.scroll_by_pixels(5.0, 20.0, &retained, now).unwrap();
        assert!(step.redraw);
        assert!(step.effects.is_empty());
        assert_eq!((state.target(), state.sub_row()), (None, 5.0));

        let step = state.scroll_by_pixels(20.0, 20.0, &retained, now).unwrap();
        assert_eq!((state.target(), state.sub_row()), (Some(999), 5.0));
        assert!(matches!(step.effects[..], [LocalScrollEffect::Sync(_)]));

        let step = state.scroll_by_pixels(580.0, 20.0, &retained, now).unwrap();
        assert_eq!((state.target(), state.sub_row()), (Some(970), 5.0));
        assert!(matches!(
            step.effects[..],
            [
                LocalScrollEffect::ScrollToOffset(982),
                LocalScrollEffect::Sync(_)
            ]
        ));
        assert_eq!(state.overscroll_room(&retained, 20.0), (605.0, 19_395.0));
        assert_eq!(state.flush(), Some(970));
        assert_eq!(state.flush(), None);
        state.scroll_by_pixels(-700.0, 20.0, &retained, now);
        assert_eq!((state.target(), state.sub_row()), (Some(970), 0.0));

        assert!(state.cancel());
        assert_eq!(state.overscroll_room(&retained, 20.0), (0.0, 20_000.0));
        assert!(state.scroll_by_pixels(1.0, 0.0, &retained, now).is_none());
    }

    #[test]
    fn pixel_steps_prefetch_once_the_view_nears_the_oldest_retained_row() {
        let retained = scrolled_back(40);
        let mut state = LocalScrollState::new(retained.history_invalidations);
        let step = state
            .scroll_by_pixels(200.0, 20.0, &retained, Instant::now())
            .unwrap();
        assert_eq!(state.target(), Some(990));
        assert!(step.effects.contains(&LocalScrollEffect::Prefetch(990)));
    }

    #[test]
    fn a_settled_or_stale_gesture_retires_and_a_stuck_one_times_out() {
        let mut retained = scrolled_back(200);
        let now = Instant::now();
        let mut state = LocalScrollState::new(retained.history_invalidations);
        let step = state.scroll_to(990, &retained, now);
        let Some(&LocalScrollEffect::Sync(generation)) = step.effects.last() else {
            panic!("a local target schedules a sync");
        };
        assert_eq!(
            state.sync(generation, &retained),
            Some(LocalScrollSync::Requested(990))
        );
        assert!(!state.expire(generation, now));
        assert!(state.expire(generation, now + LOCAL_SCROLL_TIMEOUT));
        assert_eq!(state.target(), None);

        let step = state.scroll_to(990, &retained, now);
        let Some(&LocalScrollEffect::Sync(generation)) = step.effects.last() else {
            panic!("a local target schedules a sync");
        };
        retained.viewport.scrollbar.offset = 990;
        assert_eq!(
            state.sync(generation, &retained),
            Some(LocalScrollSync::Retired)
        );
        assert_eq!(state.sync(generation, &retained), None);

        retained.viewport.scrollbar.offset = 1000;
        state.scroll_by_pixels(30.0, 20.0, &retained, now);
        assert_eq!((state.target(), state.sub_row()), (Some(999), 10.0));
        assert!(!state.observe(&retained, false, now));
        retained.history_invalidations += 1;
        assert!(state.observe(&retained, false, now));
        assert_eq!((state.target(), state.sub_row()), (None, 0.0));

        retained.viewport.mouse_tracking = true;
        assert!(!local_scroll_available(&retained));
    }
}
