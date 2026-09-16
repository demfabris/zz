//! Process logging, resource sampling, and application diagnostics.

// The sampler's consumers (browser HUD, app-shell toggle) are desktop-only.
#[cfg_attr(target_os = "ios", allow(dead_code))]
pub(crate) mod fps;

use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use gpui::{
    App, Entity, KeyBinding,
    profiler::{self, FrameTiming, FrameTimingCollector},
};
use zz_protocol::{ClientMessageKind, CommandInvocation};
use zz_ui::ROOT_KEY_CONTEXT;

use crate::{browser::controller::BrowserController, mux::client::MuxClient};

pub(crate) use zz_cli::diagnostics::{
    TimingDistribution, VERBOSE_LOG, capture_stall_sample, elapsed_us, elapsed_us_from, enabled,
    format_hundredths, format_thousandths, frames_per_second_thousandths, platform_log_dir,
    rounded_scaled_ratio, timer,
};
#[cfg(not(target_os = "ios"))]
pub(crate) use zz_cli::diagnostics::{application_args, cef_log_file};

const APP_STATE_SAMPLE_INTERVAL: Duration = Duration::from_secs(5);

pub fn start_app_state_sampler(
    controller: Entity<BrowserController>,
    mux: Entity<MuxClient>,
    cx: &mut App,
) {
    if !enabled() {
        return;
    }
    let trace_state_changed = profiler::set_trace_enabled(true);
    let mut frame_collector = FrameTimingCollector::new();
    let mut frame_sample_started = Instant::now();
    log::info!(
        target: "zz::diagnostics::frame_trace",
        "enabled=true state_changed={trace_state_changed} interval_ms={}",
        APP_STATE_SAMPLE_INTERVAL.as_millis()
    );
    log_app_state(&controller, &mux, cx, "startup");
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor()
                .timer(APP_STATE_SAMPLE_INTERVAL)
                .await;
            let frames: Vec<_> = frame_collector
                .collect_unseen()
                .into_iter()
                .filter_map(|event| match event {
                    profiler::FrameEvent::Draw(timing) => Some(timing),
                    profiler::FrameEvent::Present(_) => None,
                })
                .collect();
            let sample_ended = Instant::now();
            let sample_duration = sample_ended.duration_since(frame_sample_started);
            frame_sample_started = sample_ended;
            log_frame_trace(&frames, sample_duration);
            cx.update(|cx| log_app_state(&controller, &mux, cx, "periodic"));
        }
    })
    .detach();
}

gpui::actions!(zz, [DebugMark]);

#[cfg(any(target_os = "macos", target_os = "ios"))]
const DEBUG_MARK_KEYSTROKE: &str = "cmd-shift-m";
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
const DEBUG_MARK_KEYSTROKE: &str = "ctrl-shift-m";

static DEBUG_MARK_SEQ: AtomicU64 = AtomicU64::new(0);

/// Binds a key that stamps a marker plus a state snapshot into the log,
/// forwards the marker to the daemon, and shows a toast.
pub fn init_debug_mark(
    controller: Entity<BrowserController>,
    mux: Entity<MuxClient>,
    cx: &mut App,
) {
    cx.bind_keys([KeyBinding::new(
        DEBUG_MARK_KEYSTROKE,
        DebugMark,
        Some(ROOT_KEY_CONTEXT),
    )]);
    cx.on_action(move |_: &DebugMark, cx| {
        let seq = DEBUG_MARK_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
        log::info!(target: "zz::diagnostics::marker", "user_marker seq={seq}");
        log_app_state(&controller, &mux, cx, "user_marker");
        mux.read(cx).execute(CommandInvocation::new(
            "debug-marker",
            [format!("seq={seq}")],
        ));
        mux.update(cx, |_, cx| {
            MuxClient::emit_notification(ClientMessageKind::Info, format!("log marker #{seq}"), cx);
        });
        if let Err(error) = thread::Builder::new()
            .name("zz-stall-sample".to_owned())
            .spawn(|| capture_stall_sample("user_marker"))
        {
            log::warn!(
                target: "zz::diagnostics::stall",
                "stall_sample_failed reason=user_marker error={error}"
            );
        }
        log::logger().flush();
    });
}

const STALL_THRESHOLD_US: u64 = 500_000;
const STALL_HEARTBEAT_INTERVAL: Duration = Duration::from_millis(100);

/// Logs main-thread freezes and recoveries from a watchdog thread.
pub fn start_main_thread_watchdog(cx: &mut App) {
    let epoch = Instant::now();
    let heartbeat = Arc::new(AtomicU64::new(0));
    let beat = Arc::clone(&heartbeat);
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor()
                .timer(STALL_HEARTBEAT_INTERVAL)
                .await;
            beat.store(elapsed_us_from(epoch), Ordering::Relaxed);
        }
    })
    .detach();
    if let Err(error) = thread::Builder::new()
        .name("zz-stall-watchdog".to_owned())
        .spawn(move || watch_main_thread_heartbeat(&heartbeat, epoch))
    {
        log::error!(
            target: "zz::diagnostics::stall",
            "could not start stall watchdog: {error}"
        );
    }
}

fn watch_main_thread_heartbeat(heartbeat: &AtomicU64, epoch: Instant) {
    let mut stalled_since = None;
    loop {
        thread::sleep(STALL_HEARTBEAT_INTERVAL);
        let now = elapsed_us_from(epoch);
        let beat = heartbeat.load(Ordering::Relaxed);
        let age = now.saturating_sub(beat);
        if age > STALL_THRESHOLD_US {
            if stalled_since.is_none() {
                stalled_since = Some(beat);
                log::warn!(
                    target: "zz::diagnostics::stall",
                    "main_thread_stall stalled_us={age}"
                );
                log::logger().flush();
                capture_stall_sample("watchdog");
            }
        } else if let Some(since) = stalled_since.take() {
            log::warn!(
                target: "zz::diagnostics::stall",
                "main_thread_stall_recovered duration_us={}",
                beat.saturating_sub(since)
            );
        }
    }
}

#[derive(Debug, Default, Eq, PartialEq)]
struct FrameTraceSummary {
    frame_count: usize,
    draw: TimingDistribution,
    dirty_to_draw: TimingDistribution,
    invalidations_total: u128,
    invalidations_max: u64,
}

impl FrameTraceSummary {
    fn from_timings<'a>(timings: impl IntoIterator<Item = &'a FrameTiming>) -> Self {
        let mut frame_count = 0;
        let mut draw_us = Vec::new();
        let mut dirty_to_draw_us = Vec::new();
        let mut invalidations_total = 0_u128;
        let mut invalidations_max = 0;

        for timing in timings {
            frame_count += 1;
            draw_us.push(timing.draw_duration().as_micros());
            if let Some(duration) = timing.dirty_to_draw_duration() {
                dirty_to_draw_us.push(duration.as_micros());
            }
            invalidations_total =
                invalidations_total.saturating_add(u128::from(timing.invalidations));
            invalidations_max = invalidations_max.max(timing.invalidations);
        }

        Self {
            frame_count,
            draw: TimingDistribution::from_microseconds(draw_us),
            dirty_to_draw: TimingDistribution::from_microseconds(dirty_to_draw_us),
            invalidations_total,
            invalidations_max,
        }
    }

    fn invalidations_per_frame_hundredths(&self) -> u128 {
        rounded_scaled_ratio(self.invalidations_total, self.frame_count as u128, 100)
    }
}

fn log_frame_trace(frames: &[FrameTiming], sample_duration: Duration) {
    let mut frames_by_window = BTreeMap::<u64, Vec<&FrameTiming>>::new();
    for timing in frames {
        frames_by_window
            .entry(timing.window_id.as_u64())
            .or_default()
            .push(timing);
    }

    let observed_windows = frames_by_window.len();
    log_frame_trace_summary(
        None,
        observed_windows,
        sample_duration,
        &FrameTraceSummary::from_timings(frames),
    );
    for (window_id, timings) in frames_by_window {
        log_frame_trace_summary(
            Some(window_id),
            observed_windows,
            sample_duration,
            &FrameTraceSummary::from_timings(timings),
        );
    }
}

fn log_frame_trace_summary(
    window_id: Option<u64>,
    observed_windows: usize,
    sample_duration: Duration,
    summary: &FrameTraceSummary,
) {
    let window = window_id.map_or_else(|| "all".to_owned(), |id| id.to_string());
    let fps = format_thousandths(frames_per_second_thousandths(
        summary.frame_count,
        sample_duration,
    ));
    let invalidations_per_frame = format_hundredths(summary.invalidations_per_frame_hundredths());
    log::info!(
        target: "zz::diagnostics::frame_trace",
        "sample window={window} observed_windows={observed_windows} interval_us={} frames={} fps={fps} draw_p50_us={} draw_p95_us={} draw_max_us={} dirty_to_draw_samples={} dirty_to_draw_p50_us={} dirty_to_draw_p95_us={} dirty_to_draw_max_us={} invalidations_total={} invalidations_per_frame={invalidations_per_frame} invalidations_max={}",
        sample_duration.as_micros(),
        summary.frame_count,
        summary.draw.p50_us,
        summary.draw.p95_us,
        summary.draw.max_us,
        summary.dirty_to_draw.sample_count,
        summary.dirty_to_draw.p50_us,
        summary.dirty_to_draw.p95_us,
        summary.dirty_to_draw.max_us,
        summary.invalidations_total,
        summary.invalidations_max,
    );
}

fn log_app_state(
    controller: &Entity<BrowserController>,
    mux: &Entity<MuxClient>,
    cx: &App,
    reason: &str,
) {
    log::info!(
        target: "zz::diagnostics::app_state",
        "snapshot reason={reason} windows={}",
        cx.windows().len()
    );
    mux.read(cx).log_diagnostic_snapshot(reason);
    controller.read(cx).log_diagnostic_snapshot(reason);
}

pub(crate) fn open_logs(cx: &App) {
    let path = VERBOSE_LOG
        .get()
        .and_then(Clone::clone)
        .unwrap_or_else(|| platform_log_dir().join("zz.app.log"));
    if path.exists() {
        cx.reveal_path(&path);
    } else if let Some(directory) = path.parent()
        && let Ok(url) = url::Url::from_directory_path(directory)
    {
        cx.open_url(url.as_str());
    }
}
