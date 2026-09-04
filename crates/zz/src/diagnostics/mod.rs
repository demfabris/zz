//! Process logging, resource sampling, and application diagnostics.

// The sampler's consumers (browser HUD, app-shell toggle) are desktop-only.
#[cfg_attr(target_os = "ios", allow(dead_code))]
pub(crate) mod fps;

/// iOS spawns nothing and has no `/usr/bin/sample`, so no `Command` survives.
#[cfg(not(target_os = "ios"))]
use std::process::Command;
#[cfg(target_os = "macos")]
use std::sync::{Mutex, PoisonError};
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fs::{self, File, OpenOptions},
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use env_logger::{Builder, Env, Target, WriteStyle};
use gpui::{
    App, Entity, KeyBinding,
    profiler::{self, FrameTiming, FrameTimingCollector},
};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind, get_current_pid};
use zz_protocol::{ClientMessageKind, CommandInvocation, RawText};
use zz_ui::ROOT_KEY_CONTEXT;

use crate::{browser::controller::BrowserController, mux::client::MuxClient};

const INTERNAL_LOG_ARGUMENT: &str = "--zz-verbose-log";
pub(crate) const SOCKET_ARGUMENT: &str = "--socket";
const PROCESS_SAMPLE_INTERVAL: Duration = Duration::from_secs(2);
const APP_STATE_SAMPLE_INTERVAL: Duration = Duration::from_secs(5);
const VERBOSE_FILTER: &str = concat!(
    "zz=trace,",
    "zz_browser=trace,",
    "zz_daemon=trace,",
    "zz_terminal=trace,",
    "zz_mux=trace,",
    "cef=debug,",
    "gpui=debug,",
    "wgpu=info"
);
const NORMAL_FILTER: &str =
    "zz=info,zz_browser=info,zz_daemon=info,zz_mux=info,zz_terminal=info,cef=warn";
const RING_LOG_GENERATION_BYTES: u64 = 8 * 1024 * 1024;

static VERBOSE_LOG: OnceLock<Option<PathBuf>> = OnceLock::new();

pub fn init() {
    let arguments = std::env::args_os().collect::<Vec<_>>();
    let verbose = verbose_requested(&arguments);
    let role = process_role(&arguments);

    if !verbose {
        init_production(&role);
        return;
    }

    let requested_path = verbose_log_path(&arguments);
    let (path, target) = match open_log_file(requested_path.as_deref()) {
        Ok((path, file)) => {
            eprintln!("zz verbose log: {}", path.display());
            (Some(path), Target::Pipe(Box::new(file)))
        }
        Err(error) => {
            eprintln!("zz: could not create verbose log file: {error}");
            (None, Target::Stderr)
        }
    };
    install_formatted_logger(
        Env::default().default_filter_or(VERBOSE_FILTER),
        target,
        role.clone(),
    );

    let _ = VERBOSE_LOG.set(path.clone());
    install_panic_hook();
    log_process_start(&role, path.as_deref());
    start_process_sampler();
}

fn init_production(role: &str) {
    if !matches!(role, "app" | "daemon" | "tui") {
        let _ = Builder::from_env(Env::default().default_filter_or(NORMAL_FILTER)).try_init();
        return;
    }
    let writer = match RingLogWriter::open(&platform_log_dir().join(format!("zz.{role}.log"))) {
        Ok(writer) => writer,
        Err(error) => {
            eprintln!("zz: could not open log file: {error}");
            let _ = Builder::from_env(Env::default().default_filter_or(NORMAL_FILTER)).try_init();
            return;
        }
    };
    let path = writer.path.clone();
    install_formatted_logger(
        Env::default().default_filter_or(NORMAL_FILTER),
        Target::Pipe(Box::new(writer)),
        role.to_owned(),
    );
    install_panic_hook();
    log_process_start(role, Some(&path));
}

fn install_formatted_logger(env: Env, target: Target, role: String) {
    let started = Instant::now();
    let pid = std::process::id();
    let mut builder = Builder::from_env(env);
    builder
        .target(target)
        .write_style(WriteStyle::Never)
        .format(move |buffer, record| {
            let current = thread::current();
            let thread_name = current.name().unwrap_or("unnamed");
            writeln!(
                buffer,
                "{} elapsed_us={} level={} role={} pid={} thread={thread_name:?} thread_id={:?} target={} {}",
                buffer.timestamp_micros(),
                started.elapsed().as_micros(),
                record.level(),
                role,
                pid,
                current.id(),
                record.target(),
                record.args(),
            )
        });
    if let Err(error) = builder.try_init() {
        eprintln!("zz: could not initialize logger: {error}");
    }
}

struct RingLogWriter {
    path: PathBuf,
    file: File,
    written: u64,
    limit: u64,
    mirror: Option<io::Stderr>,
}

impl RingLogWriter {
    fn open(path: &Path) -> io::Result<Self> {
        let (path, file) = open_log_file(Some(path))?;
        let written = file.metadata().map_or(0, |metadata| metadata.len());
        Ok(Self {
            path,
            file,
            written,
            limit: RING_LOG_GENERATION_BYTES,
            mirror: io::stderr().is_terminal().then(io::stderr),
        })
    }

    fn rotate(&mut self) -> io::Result<()> {
        fs::rename(&self.path, self.path.with_extension("log.old"))?;
        let (_, file) = open_log_file(Some(&self.path))?;
        self.file = file;
        self.written = 0;
        Ok(())
    }
}

impl Write for RingLogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.written >= self.limit {
            let _ = self.rotate();
        }
        if let Some(mirror) = &mut self.mirror {
            let _ = mirror.write_all(buf);
        }
        let written = self.file.write(buf)?;
        self.written += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

/// Path CEF should write its log to; its subprocesses inherit the setting.
/// Renames the previous session's log aside, because CEF truncates on startup.
#[cfg(not(target_os = "ios"))]
pub(crate) fn cef_log_file() -> PathBuf {
    let path = platform_log_dir().join("cef.log");
    if path.exists() {
        let _ = fs::rename(&path, path.with_extension("log.old"));
    }
    path
}

#[must_use]
pub(crate) fn enabled() -> bool {
    VERBOSE_LOG.get().is_some()
}

/// Returns a start instant only when `target` is tracing, so hot paths skip the
/// clock read otherwise.
pub(crate) fn timer(target: &str) -> Option<Instant> {
    log::log_enabled!(target: target, log::Level::Trace).then(Instant::now)
}

pub(crate) fn elapsed_us(started: Option<Instant>) -> u128 {
    started.map_or(0, |started| started.elapsed().as_micros())
}

#[cfg(not(target_os = "ios"))]
pub(crate) fn application_args() -> Vec<RawText> {
    application_args_from(std::env::args_os().skip(1))
}

#[cfg(not(target_os = "ios"))]
fn application_args_from(arguments: impl IntoIterator<Item = OsString>) -> Vec<RawText> {
    let mut output = Vec::new();
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        if argument == OsStr::new("--verbose") {
            continue;
        }
        if argument == OsStr::new(INTERNAL_LOG_ARGUMENT) {
            let _ = arguments.next();
            continue;
        }
        if argument
            .to_string_lossy()
            .starts_with(&format!("{INTERNAL_LOG_ARGUMENT}="))
        {
            continue;
        }
        if argument == OsStr::new(crate::DAEMON_BOOTSTRAP_CLIENT_CWD_ARGUMENT) {
            output.push(crate::DAEMON_BOOTSTRAP_CLIENT_CWD_ARGUMENT.into());
            if let Some(value) = arguments.next() {
                output.push(RawText::from_os_str(&value));
            }
            continue;
        }
        output.push(RawText::from_os_str(&argument));
    }
    output
}

#[cfg(not(target_os = "ios"))]
pub(crate) fn configure_spawned_process(command: &mut Command) {
    let Some(shared_log) = VERBOSE_LOG.get() else {
        return;
    };
    command.arg("--verbose");
    if let Some(path) = shared_log {
        command.arg(INTERNAL_LOG_ARGUMENT).arg(path);
    }
    log::debug!(
        target: "zz::diagnostics::process",
        "configured child process verbose=true shared_log={shared_log:?}"
    );
}

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

fn elapsed_us_from(epoch: Instant) -> u64 {
    u64::try_from(epoch.elapsed().as_micros()).unwrap_or(u64::MAX)
}

#[cfg(target_os = "macos")]
const STALL_SAMPLE_COOLDOWN: Duration = Duration::from_mins(1);
#[cfg(target_os = "macos")]
const STALL_SAMPLE_KEEP: usize = 8;

/// Writes two seconds of 10 ms stacks to `zz.stall-<unix-seconds>.sample.txt`
/// beside the ring logs. `/usr/bin/sample` can only attach because the installed
/// bundle is signed without hardened runtime.
#[cfg(target_os = "macos")]
pub(crate) fn capture_stall_sample(reason: &str) {
    static LAST_CAPTURE: Mutex<Option<Instant>> = Mutex::new(None);
    {
        let mut last = LAST_CAPTURE.lock().unwrap_or_else(PoisonError::into_inner);
        if last.is_some_and(|last| last.elapsed() < STALL_SAMPLE_COOLDOWN) {
            return;
        }
        *last = Some(Instant::now());
    }
    let dir = platform_log_dir();
    prune_stall_samples(&dir);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let path = dir.join(format!("zz.stall-{timestamp}.sample.txt"));
    log::warn!(
        target: "zz::diagnostics::stall",
        "stall_sample_capturing reason={reason} file={}",
        path.display()
    );
    log::logger().flush();
    let output = Command::new("/usr/bin/sample")
        .arg(std::process::id().to_string())
        .args(["2", "10", "-mayDie", "-file"])
        .arg(&path)
        .output();
    match output {
        Ok(output) if output.status.success() => {
            log::warn!(
                target: "zz::diagnostics::stall",
                "stall_sample_captured reason={reason} file={}",
                path.display()
            );
        }
        Ok(output) => log::warn!(
            target: "zz::diagnostics::stall",
            "stall_sample_failed reason={reason} status={} stderr={:?}",
            output.status,
            String::from_utf8_lossy(&output.stderr),
        ),
        Err(error) => log::warn!(
            target: "zz::diagnostics::stall",
            "stall_sample_failed reason={reason} error={error}"
        ),
    }
    log::logger().flush();
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn capture_stall_sample(_reason: &str) {}

#[cfg(target_os = "macos")]
fn prune_stall_samples(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut samples = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|name| name.starts_with("zz.stall-") && name.ends_with(".sample.txt"))
        })
        .collect::<Vec<_>>();
    samples.sort();
    for stale in samples.iter().rev().skip(STALL_SAMPLE_KEEP - 1) {
        let _ = fs::remove_file(stale);
    }
}

#[derive(Debug, Default, Eq, PartialEq)]
struct TimingDistribution {
    sample_count: usize,
    p50_us: u128,
    p95_us: u128,
    max_us: u128,
}

impl TimingDistribution {
    fn from_microseconds(mut values: Vec<u128>) -> Self {
        values.sort_unstable();
        Self {
            sample_count: values.len(),
            p50_us: nearest_rank_percentile(&values, 50),
            p95_us: nearest_rank_percentile(&values, 95),
            max_us: values.last().copied().unwrap_or_default(),
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

fn nearest_rank_percentile(sorted_values: &[u128], percentile: usize) -> u128 {
    debug_assert!((1..=100).contains(&percentile));
    if sorted_values.is_empty() {
        return 0;
    }
    let rank = sorted_values.len().saturating_mul(percentile).div_ceil(100);
    sorted_values[rank.saturating_sub(1).min(sorted_values.len() - 1)]
}

fn frames_per_second_thousandths(frame_count: usize, sample_duration: Duration) -> u128 {
    rounded_scaled_ratio(
        (frame_count as u128).saturating_mul(Duration::from_secs(1).as_nanos()),
        sample_duration.as_nanos(),
        1_000,
    )
}

fn rounded_scaled_ratio(numerator: u128, denominator: u128, scale: u128) -> u128 {
    if denominator == 0 {
        return 0;
    }
    numerator
        .saturating_mul(scale)
        .saturating_add(denominator / 2)
        / denominator
}

fn format_thousandths(value: u128) -> String {
    format!("{}.{:03}", value / 1_000, value % 1_000)
}

fn format_hundredths(value: u128) -> String {
    format!("{}.{:02}", value / 100, value % 100)
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

fn verbose_requested(arguments: &[OsString]) -> bool {
    arguments.iter().skip(1).any(|argument| {
        argument == OsStr::new("--verbose")
            || argument == OsStr::new(INTERNAL_LOG_ARGUMENT)
            || argument
                .to_string_lossy()
                .starts_with(&format!("{INTERNAL_LOG_ARGUMENT}="))
    })
}

fn verbose_log_path(arguments: &[OsString]) -> Option<PathBuf> {
    let mut arguments = arguments.iter().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == OsStr::new(INTERNAL_LOG_ARGUMENT) {
            return arguments.next().map(PathBuf::from);
        }
        let argument = argument.to_string_lossy();
        if let Some(path) = argument.strip_prefix(&format!("{INTERNAL_LOG_ARGUMENT}=")) {
            return Some(PathBuf::from(path));
        }
    }
    None
}

fn process_role(arguments: &[OsString]) -> String {
    if let Some(process_type) = process_type(arguments) {
        return format!("cef-{process_type}");
    }
    if application_arguments(arguments)
        .next()
        .is_some_and(|argument| argument == OsStr::new("daemon"))
    {
        return "daemon".to_owned();
    }
    let app_arguments = application_arguments(arguments).collect::<Vec<_>>();
    if app_arguments.as_slice() == [OsStr::new("app")] {
        return "app".to_owned();
    }
    if application_arguments(arguments).any(|argument| argument == OsStr::new("attach"))
        && std::io::IsTerminal::is_terminal(&std::io::stdout())
    {
        return "tui".to_owned();
    }
    let executable = arguments
        .first()
        .and_then(|path| Path::new(path).file_stem())
        .and_then(OsStr::to_str)
        .unwrap_or("zz");
    if executable.contains("helper") {
        "cef-helper".to_owned()
    } else if application_arguments(arguments).next().is_some() {
        "command".to_owned()
    } else {
        "app".to_owned()
    }
}

fn process_type(arguments: &[OsString]) -> Option<String> {
    let mut arguments = arguments.iter().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == OsStr::new("--type") {
            return arguments
                .next()
                .map(|value| value.to_string_lossy().into_owned());
        }
        if let Some(value) = argument.to_string_lossy().strip_prefix("--type=") {
            return Some(value.to_owned());
        }
    }
    None
}

fn application_arguments(arguments: &[OsString]) -> impl Iterator<Item = &OsString> {
    let mut skip_path = false;
    arguments.iter().skip(1).filter(move |argument| {
        if skip_path {
            skip_path = false;
            return false;
        }
        if *argument == OsStr::new("--verbose") {
            return false;
        }
        if *argument == OsStr::new(INTERNAL_LOG_ARGUMENT)
            || *argument == OsStr::new(SOCKET_ARGUMENT)
        {
            skip_path = true;
            return false;
        }
        let argument = argument.to_string_lossy();
        !argument.starts_with(&format!("{INTERNAL_LOG_ARGUMENT}="))
            && !argument.starts_with(&format!("{SOCKET_ARGUMENT}="))
    })
}

fn open_log_file(requested: Option<&Path>) -> io::Result<(PathBuf, File)> {
    let path = requested.map_or_else(default_log_path, Path::to_owned);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        restrict_directory(parent)?;
    }
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    restrict_file(&path)?;
    Ok((path, file))
}

fn default_log_path() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    platform_log_dir().join(format!("zz-{timestamp}-{}.verbose.log", std::process::id()))
}

fn platform_log_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("ZZ_LOG_DIR") {
        return PathBuf::from(path);
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
            return PathBuf::from(path).join("zz").join("logs");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join(".local")
                .join("state")
                .join("zz")
                .join("logs");
        }
    }
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join("Library").join("Logs").join("zz");
    }
    #[cfg(target_os = "windows")]
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(local).join("zz").join("logs");
    }
    std::env::temp_dir().join("zz").join("logs")
}

#[cfg(unix)]
fn restrict_directory(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn restrict_directory(_: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn restrict_file(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict_file(_: &Path) -> io::Result<()> {
    Ok(())
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        log::error!(
            target: "zz::diagnostics::panic",
            "panic={panic} backtrace={}",
            std::backtrace::Backtrace::force_capture()
        );
        log::logger().flush();
        previous(panic);
    }));
}

#[allow(
    clippy::unnecessary_debug_formatting,
    reason = "raw diagnostics preserve quoting, escapes, and non-Unicode OS strings"
)]
fn log_process_start(role: &str, log_path: Option<&Path>) {
    log::info!(
        target: "zz::diagnostics::lifecycle",
        "process_start role={} package_version={} os={} arch={} pointer_width={} cpus={:?} executable={:?} cwd={:?} args={:?} log_path={:?}",
        role,
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        usize::BITS,
        thread::available_parallelism(),
        std::env::current_exe(),
        std::env::current_dir(),
        std::env::args_os().collect::<Vec<_>>(),
        log_path,
    );
    for (name, value) in std::env::vars_os() {
        log::trace!(
            target: "zz::diagnostics::environment",
            "environment name={name:?} value={value:?}"
        );
    }
}

fn start_process_sampler() {
    if let Err(error) = thread::Builder::new()
        .name("zz-diagnostics".to_owned())
        .spawn(process_sampler)
    {
        log::error!(
            target: "zz::diagnostics::process",
            "could not start process sampler: {error}"
        );
    }
}

fn process_sampler() {
    let Ok(pid) = get_current_pid() else {
        log::error!(
            target: "zz::diagnostics::process",
            "could not resolve current process id"
        );
        return;
    };
    let mut system = System::new();
    let refresh = ProcessRefreshKind::new()
        .with_memory()
        .with_cpu()
        .with_disk_usage()
        .with_cmd(UpdateKind::OnlyIfNotSet)
        .with_exe(UpdateKind::OnlyIfNotSet);
    loop {
        system.refresh_processes_specifics(ProcessesToUpdate::All, refresh);
        let Some(process) = system.process(pid) else {
            return;
        };
        let disk = process.disk_usage();
        let log_file_bytes = VERBOSE_LOG
            .get()
            .and_then(|path| path.as_deref())
            .and_then(|path| path.metadata().ok())
            .map(|metadata| metadata.len());
        log::info!(
            target: "zz::diagnostics::process",
            "sample rss_bytes={} virtual_bytes={} cpu_percent={:.3} run_seconds={} status={:?} parent_pid={:?} task_count={:?} disk_read_bytes={} disk_written_bytes={} disk_total_read_bytes={} disk_total_written_bytes={} log_file_bytes={log_file_bytes:?}",
            process.memory(),
            process.virtual_memory(),
            process.cpu_usage(),
            process.run_time(),
            process.status(),
            process.parent(),
            process.tasks().map(std::collections::HashSet::len),
            disk.read_bytes,
            disk.written_bytes,
            disk.total_read_bytes,
            disk.total_written_bytes,
        );

        let mut descendants = system
            .processes()
            .iter()
            .filter(|(candidate, process)| {
                process.thread_kind().is_none() && is_descendant(&system, **candidate, pid)
            })
            .collect::<Vec<_>>();
        descendants.sort_unstable_by_key(|(child_pid, _)| child_pid.as_u32());

        let tree_rss_bytes = descendants
            .iter()
            .fold(process.memory(), |total, (_, child)| {
                total.saturating_add(child.memory())
            });
        let tree_virtual_bytes = descendants
            .iter()
            .fold(process.virtual_memory(), |total, (_, child)| {
                total.saturating_add(child.virtual_memory())
            });
        let tree_cpu_percent = descendants
            .iter()
            .fold(process.cpu_usage(), |total, (_, child)| {
                total + child.cpu_usage()
            });
        log::info!(
            target: "zz::diagnostics::process_tree",
            "tree_sample root_pid={pid} process_count={} descendant_count={} rss_bytes={tree_rss_bytes} virtual_bytes={tree_virtual_bytes} cpu_percent={tree_cpu_percent:.3}",
            descendants.len() + 1,
            descendants.len(),
        );
        for (child_pid, child) in descendants {
            let disk = child.disk_usage();
            log::info!(
                target: "zz::diagnostics::process_tree",
                "descendant_sample pid={child_pid} parent_pid={:?} name={} exe={:?} cmd={:?} rss_bytes={} virtual_bytes={} cpu_percent={:.3} run_seconds={} status={:?} task_count={:?} disk_read_bytes={} disk_written_bytes={} disk_total_read_bytes={} disk_total_written_bytes={}",
                child.parent(),
                child.name().display(),
                child.exe(),
                child.cmd(),
                child.memory(),
                child.virtual_memory(),
                child.cpu_usage(),
                child.run_time(),
                child.status(),
                child.tasks().map(std::collections::HashSet::len),
                disk.read_bytes,
                disk.written_bytes,
                disk.total_read_bytes,
                disk.total_written_bytes,
            );
        }
        log::logger().flush();
        thread::sleep(PROCESS_SAMPLE_INTERVAL);
    }
}

fn is_descendant(system: &System, candidate: Pid, root: Pid) -> bool {
    if candidate == root {
        return false;
    }
    let mut cursor = candidate;
    for _ in 0..64 {
        let Some(process) = system.process(cursor) else {
            return false;
        };
        let Some(parent) = process.parent() else {
            return false;
        };
        if parent == root {
            return true;
        }
        if parent == cursor {
            return false;
        }
        cursor = parent;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn detects_verbose_and_shared_log_arguments() {
        let values = arguments(&["zz", "--verbose", "--zz-verbose-log", "/tmp/a.log"]);
        assert!(verbose_requested(&values));
        assert_eq!(verbose_log_path(&values), Some(PathBuf::from("/tmp/a.log")));
    }

    #[test]
    fn detects_equals_form_of_shared_log_argument() {
        let values = arguments(&["zz", "--zz-verbose-log=/tmp/a.log"]);
        assert!(verbose_requested(&values));
        assert_eq!(verbose_log_path(&values), Some(PathBuf::from("/tmp/a.log")));
    }

    #[test]
    fn classifies_process_roles_after_global_flags() {
        assert_eq!(
            process_role(&arguments(&["zz", "--verbose", "daemon"])),
            "daemon"
        );
        assert_eq!(process_role(&arguments(&["zz", "--verbose"])), "app");
        assert_eq!(process_role(&arguments(&["zz", "app"])), "app");
        assert_eq!(
            process_role(&arguments(&["zz", "--type=renderer", "--verbose"])),
            "cef-renderer"
        );
        assert_eq!(
            process_role(&arguments(&["zz", "--type", "utility", "--verbose"])),
            "cef-utility"
        );
        assert_eq!(
            process_role(&arguments(&["zz", "--socket", "/tmp/zz.sock"])),
            "app"
        );
        assert_eq!(
            process_role(&arguments(&["zz", "--socket", "daemon"])),
            "app"
        );
        assert_eq!(
            process_role(&arguments(&["zz", "--socket", "/tmp/zz.sock", "daemon"])),
            "daemon"
        );
    }

    /// `--bootstrap-client-cwd` hands a daemon zz spawns the working directory
    /// the invoking client was in. Before protocol v98 the argument list was
    /// `Vec<String>` and `into_string` dropped a directory that only exists as
    /// bytes to an empty string, which
    /// `validated_bootstrap_client_working_directory` then refused, so the
    /// daemon started with no client cwd at all. The list is `RawText` now, so
    /// the bytes survive argv and `PathBuf::from(cwd.to_os_string())` rebuilds
    /// the same directory, matching `ClientHello.working_directory`, byte
    /// preserving since v92.
    #[cfg(all(unix, not(target_os = "ios")))]
    #[test]
    fn non_utf8_bootstrap_client_cwd_reaches_the_daemon_as_bytes() {
        use std::os::unix::ffi::OsStringExt as _;

        let arguments = application_args_from([
            OsString::from("daemon"),
            OsString::from(crate::DAEMON_BOOTSTRAP_CLIENT_CWD_ARGUMENT),
            OsString::from_vec(b"/tmp/client-\xff".to_vec()),
        ]);
        assert_eq!(
            arguments.iter().map(RawText::as_bytes).collect::<Vec<_>>(),
            [
                b"daemon".as_slice(),
                crate::DAEMON_BOOTSTRAP_CLIENT_CWD_ARGUMENT.as_bytes(),
                b"/tmp/client-\xff".as_slice(),
            ]
        );
    }

    #[test]
    fn computes_nearest_rank_timing_percentiles() {
        let distribution = TimingDistribution::from_microseconds(vec![40, 10, 50, 30, 20]);
        assert_eq!(
            distribution,
            TimingDistribution {
                sample_count: 5,
                p50_us: 30,
                p95_us: 50,
                max_us: 50,
            }
        );
        assert_eq!(
            TimingDistribution::from_microseconds(Vec::new()),
            TimingDistribution::default()
        );
    }

    #[test]
    fn ring_log_rotates_one_old_generation() {
        let dir = std::env::temp_dir().join(format!("zz-ring-log-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("zz.app.log");
        let mut writer = RingLogWriter::open(&path).expect("ring log opens");
        writer.limit = 32;

        for _ in 0..3 {
            writer
                .write_all(b"0123456789abcdef0123456789abcdef\n")
                .unwrap();
        }

        let old = path.with_extension("log.old");
        assert!(old.exists(), "previous generation was kept");
        assert!(fs::metadata(&path).unwrap().len() <= 33 * 2);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn formats_frame_rates_and_invalidation_averages() {
        assert_eq!(
            frames_per_second_thousandths(1, Duration::from_secs(5)),
            200
        );
        assert_eq!(
            frames_per_second_thousandths(120, Duration::from_secs(5)),
            24_000
        );
        assert_eq!(frames_per_second_thousandths(1, Duration::ZERO), 0);
        assert_eq!(rounded_scaled_ratio(3, 2, 100), 150);
        assert_eq!(format_thousandths(24_001), "24.001");
        assert_eq!(format_hundredths(150), "1.50");
    }
}
