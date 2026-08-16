use std::time::{Duration, Instant};

use gpui::{
    Context, Entity, IntoElement, Render, Task, Window, div,
    prelude::*,
    profiler::{self, FrameTimingCollector},
    px,
};
use zz_ui::pane::frame_rate_badge;

use crate::diagnostics;

pub(crate) const FPS_SAMPLE_INTERVAL: Duration = Duration::from_millis(250);

pub(crate) struct FrameRateSampler {
    enabled: bool,
    frames_since_sample: u32,
    previous_frames: u32,
    previous_elapsed: Duration,
    sampled_at: Instant,
    fps: Option<f64>,
}

impl FrameRateSampler {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            enabled: false,
            frames_since_sample: 0,
            previous_frames: 0,
            previous_elapsed: Duration::ZERO,
            sampled_at: Instant::now(),
            fps: None,
        }
    }

    pub(crate) fn set_enabled(&mut self, enabled: bool, now: Instant) {
        if self.enabled == enabled {
            return;
        }
        self.enabled = enabled;
        self.frames_since_sample = 0;
        self.previous_frames = 0;
        self.previous_elapsed = Duration::ZERO;
        self.sampled_at = now;
        self.fps = None;
    }

    pub(crate) fn record_frame(&mut self) {
        if self.enabled {
            self.frames_since_sample = self.frames_since_sample.saturating_add(1);
        }
    }

    #[must_use]
    pub(crate) fn sample(&mut self, now: Instant) -> bool {
        if !self.enabled {
            return false;
        }
        let elapsed = now.saturating_duration_since(self.sampled_at);
        self.fps = Some(frames_per_second(
            self.frames_since_sample
                .saturating_add(self.previous_frames),
            elapsed.saturating_add(self.previous_elapsed),
        ));
        self.previous_frames = self.frames_since_sample;
        self.previous_elapsed = elapsed;
        self.frames_since_sample = 0;
        self.sampled_at = now;
        true
    }

    #[must_use]
    pub(crate) fn fps(&self) -> Option<f64> {
        self.fps
    }
}

pub struct AppFpsMeter {
    window_id: u64,
    collector: FrameTimingCollector,
    sampled_at: Instant,
    previous_frames: u32,
    previous_elapsed: Duration,
    fps: Option<f64>,
    enabled: bool,
    _sample_task: Task<()>,
}

impl AppFpsMeter {
    #[must_use]
    pub fn new(window: &Window, _: &mut Context<Self>) -> Self {
        Self {
            window_id: window.window_handle().window_id().as_u64(),
            collector: FrameTimingCollector::new(),
            sampled_at: Instant::now(),
            previous_frames: 0,
            previous_elapsed: Duration::ZERO,
            fps: None,
            enabled: false,
            _sample_task: Task::ready(()),
        }
    }

    pub fn set_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.enabled == enabled {
            return;
        }
        self.enabled = enabled;
        self.fps = None;
        self.previous_frames = 0;
        self.previous_elapsed = Duration::ZERO;
        self.sampled_at = Instant::now();
        self._sample_task = Task::ready(());
        if enabled {
            profiler::set_trace_enabled(true);
            self._sample_task = cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(FPS_SAMPLE_INTERVAL).await;
                    if this.update(cx, AppFpsMeter::sample).is_err() {
                        break;
                    }
                }
            });
        } else if !diagnostics::enabled() {
            profiler::set_trace_enabled(false);
        }
        self.collector = FrameTimingCollector::new();
    }

    fn sample(&mut self, cx: &mut Context<Self>) {
        if !self.enabled {
            return;
        }
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.sampled_at);
        let frame_count = self
            .collector
            .collect_unseen()
            .into_iter()
            .filter_map(|event| match event {
                profiler::FrameEvent::Draw(timing) => Some(timing),
                profiler::FrameEvent::Present(_) => None,
            })
            .filter(|timing| timing.window_id.as_u64() == self.window_id)
            .count();
        self.sampled_at = now;
        let frame_count = u32::try_from(frame_count).unwrap_or(u32::MAX);
        self.fps = Some(frames_per_second(
            frame_count.saturating_add(self.previous_frames),
            elapsed.saturating_add(self.previous_elapsed),
        ));
        self.previous_frames = frame_count;
        self.previous_elapsed = elapsed;
        cx.notify();
    }
}

impl Render for AppFpsMeter {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        frame_rate_badge("GPUI", self.fps, cx)
    }
}

#[must_use]
pub(crate) fn app_fps_overlay(meter: Entity<AppFpsMeter>) -> gpui::Div {
    div().absolute().top(px(6.0)).right(px(8.0)).child(meter)
}

fn frames_per_second(frames: u32, elapsed: Duration) -> f64 {
    if elapsed.is_zero() {
        return 0.0;
    }
    f64::from(frames) / elapsed.as_secs_f64()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_rate_sampler_tracks_only_enabled_intervals() {
        let started = Instant::now();
        let mut sampler = FrameRateSampler::new();
        sampler.record_frame();
        sampler.set_enabled(true, started);
        for _ in 0..120 {
            sampler.record_frame();
        }
        assert!(sampler.sample(started + Duration::from_secs(2)));
        assert_eq!(sampler.fps(), Some(60.0));

        sampler.set_enabled(false, started + Duration::from_secs(2));
        sampler.record_frame();
        assert!(!sampler.sample(started + Duration::from_secs(3)));
        assert_eq!(sampler.fps(), None);
    }

    #[test]
    fn zero_length_sample_is_bounded() {
        assert_eq!(frames_per_second(1, Duration::ZERO), 0.0);
    }

    #[test]
    fn sliding_window_reaches_steady_rate_within_two_samples() {
        let started = Instant::now();
        let mut sampler = FrameRateSampler::new();
        sampler.set_enabled(true, started);
        assert!(sampler.sample(started + FPS_SAMPLE_INTERVAL));
        assert_eq!(sampler.fps(), Some(0.0));
        for _ in 0..30 {
            sampler.record_frame();
        }
        assert!(sampler.sample(started + FPS_SAMPLE_INTERVAL * 2));
        assert_eq!(sampler.fps(), Some(60.0));
        for _ in 0..30 {
            sampler.record_frame();
        }
        assert!(sampler.sample(started + FPS_SAMPLE_INTERVAL * 3));
        assert_eq!(sampler.fps(), Some(120.0));
    }
}
