//! Process logging, resource sampling, and application diagnostics.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use gpui::{App, Entity, KeyBinding};
use zz_protocol::{ClientMessageKind, CommandInvocation};
use zz_ui::ROOT_KEY_CONTEXT;

use crate::{browser::controller::BrowserController, mux::client::MuxClient};

pub(crate) use zz_cli::diagnostics::{
    VERBOSE_LOG, capture_stall_sample, elapsed_us, elapsed_us_from, enabled, platform_log_dir,
    timer,
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
    log_app_state(&controller, &mux, cx, "startup");
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor()
                .timer(APP_STATE_SAMPLE_INTERVAL)
                .await;
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
