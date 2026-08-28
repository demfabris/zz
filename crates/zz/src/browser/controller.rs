use std::{
    collections::{BTreeMap, BTreeSet, btree_map::Entry},
    ops::RangeInclusive,
    sync::Arc,
    time::Duration,
    time::Instant,
};

use gpui::{App, Context, EventEmitter, RenderImage, Task};
use image::{Frame as ImageFrame, ImageBuffer, Rgba};
#[cfg(target_os = "macos")]
use zz_browser::MacIoSurface;
#[cfg(target_os = "windows")]
use zz_browser::WinGpuTexture;
use zz_browser::{
    BrowserError, BrowserEvent, BrowserGpuContext, BrowserRuntime, BrowserSession,
    CookieImportBatch, CookieImportResult, EditCommand, ElementPickerAppearance, KeyInput,
    OsrFrame, PointerEvent, RuntimePhase, RuntimeSignal, SessionId, SessionPhase,
    SiteDataClearResult, Viewport, WheelEvent,
};
use zz_protocol::PaneId;

use crate::diagnostics;

const GRACEFUL_CLOSE_TIMEOUT: Duration = Duration::from_secs(2);
const FORCED_CLOSE_TIMEOUT: Duration = Duration::from_secs(2);
const ACTIVE_PUMP_INTERVAL: Duration = Duration::from_millis(16);
const DEFAULT_DISPLAY_FRAME_RATE_CEILING: i32 = 60;
#[cfg(any(not(target_os = "macos"), test))]
const MAX_DISPLAY_FRAME_RATE_CEILING: i32 = 240;
const UNFOCUSED_SCROLL_FRAME_RATE_DECAY: Duration = Duration::from_secs(1);
const EXTERNAL_BEGIN_FRAME_HOT_WINDOW: Duration = Duration::from_millis(500);
const ADAPTIVE_BEGIN_FRAME_SAMPLE_WINDOW: Duration = Duration::from_secs(1);
const ADAPTIVE_BEGIN_FRAME_PROBE_WINDOW: Duration = Duration::from_secs(2);
const ADAPTIVE_BEGIN_FRAME_DOWNSHIFT_PERCENT: u64 = 85;
const ADAPTIVE_BEGIN_FRAME_PROBE_PERCENT: u64 = 95;
const MIN_ADAPTIVE_BEGIN_FRAME_RATE: i32 = 30;
const PUMP_HOT_WINDOW: Duration = Duration::from_millis(500);
const SHARED_TEXTURE_FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(2);
// Mirrors cefclient's 30 Hz `kMaxTimerDelay` watchdog.
const VISIBLE_PUMP_WATCHDOG_INTERVAL: Duration = Duration::from_millis(1000 / 30);
const UNFOCUSED_FRAME_RATE_CAP: i32 = 30;

/// Desktop builds include the CEF browser runtime.
pub(crate) fn is_available(_cx: &App) -> bool {
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct TabId(pub u64);

type BrowserKey = (PaneId, TabId);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FirstFrameWatchdog {
    session: SessionId,
    generation: u64,
    armed: bool,
}

impl FirstFrameWatchdog {
    fn new(session: SessionId) -> Self {
        Self {
            session,
            generation: 0,
            armed: false,
        }
    }

    fn arm(&mut self) -> Option<u64> {
        if self.armed {
            return None;
        }
        self.generation = self.generation.wrapping_add(1).max(1);
        self.armed = true;
        Some(self.generation)
    }

    fn pause(&mut self) {
        self.generation = self.generation.wrapping_add(1).max(1);
        self.armed = false;
    }

    fn matches(self, session: SessionId, generation: u64) -> bool {
        self.session == session && self.generation == generation && self.armed
    }
}

fn browser_key_range(pane: PaneId) -> RangeInclusive<BrowserKey> {
    (pane, TabId(0))..=(pane, TabId(u64::MAX))
}

type CefWork = Box<dyn FnOnce()>;

#[cfg(any(not(target_os = "macos"), test))]
#[allow(
    clippy::cast_possible_truncation,
    reason = "display rates are intentionally rounded to integer FPS before the CEF bounds clamp"
)]
fn select_display_frame_rate_ceiling(refresh_rates: Vec<Option<f32>>) -> i32 {
    refresh_rates
        .into_iter()
        .flatten()
        .reduce(f32::max)
        .map_or(DEFAULT_DISPLAY_FRAME_RATE_CEILING, |refresh_rate| {
            refresh_rate.round() as i32
        })
        .clamp(1, MAX_DISPLAY_FRAME_RATE_CEILING)
}

#[cfg(any(not(target_os = "macos"), test))]
fn reported_display_frame_rate_ceiling(refresh_rates: Vec<Option<f32>>) -> Option<i32> {
    if refresh_rates.iter().all(Option::is_none) {
        None
    } else {
        Some(select_display_frame_rate_ceiling(refresh_rates))
    }
}

fn resolve_frame_rate_ceiling_value(
    resolved: Option<i32>,
    detected: Option<i32>,
) -> (i32, Option<i32>) {
    let resolved = resolved.or(detected);
    (
        resolved.unwrap_or(DEFAULT_DISPLAY_FRAME_RATE_CEILING),
        resolved,
    )
}

#[cfg(target_os = "macos")]
fn display_frame_rate_ceiling(_cx: &gpui::App) -> Option<i32> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSScreen;

    let Some(main_thread) = MainThreadMarker::new() else {
        log::warn!("could not query macOS display frame rates outside the main thread");
        return None;
    };
    NSScreen::screens(main_thread)
        .iter()
        .filter_map(|screen| i32::try_from(screen.maximumFramesPerSecond()).ok())
        .filter(|frame_rate| *frame_rate > 0)
        .max()
}

#[cfg(not(target_os = "macos"))]
fn display_frame_rate_ceiling(cx: &gpui::App) -> Option<i32> {
    reported_display_frame_rate_ceiling(
        cx.displays()
            .into_iter()
            .map(|display| display.refresh_rate())
            .collect(),
    )
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum PumpFallbackWork {
    #[default]
    Idle,
    VisibleSession,
    AnimatingSession,
    ActiveTransition,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct PumpFallbackState {
    runtime_phase: Option<RuntimePhase>,
    work: PumpFallbackWork,
    shutting_down: bool,
    frame_interval: Duration,
}

impl PumpFallbackState {
    fn interval(self) -> Option<Duration> {
        if self.shutting_down {
            return None;
        }
        match self.runtime_phase {
            Some(RuntimePhase::Initializing) => Some(ACTIVE_PUMP_INTERVAL),
            Some(RuntimePhase::Running) if self.work == PumpFallbackWork::ActiveTransition => {
                Some(ACTIVE_PUMP_INTERVAL)
            }
            Some(RuntimePhase::Running) if self.work == PumpFallbackWork::AnimatingSession => {
                Some(self.frame_interval.min(VISIBLE_PUMP_WATCHDOG_INTERVAL))
            }
            Some(RuntimePhase::Running) if self.work == PumpFallbackWork::VisibleSession => {
                Some(VISIBLE_PUMP_WATCHDOG_INTERVAL)
            }
            _ => None,
        }
    }
}

fn pump_frame_interval(frame_rate_ceiling: i32) -> Duration {
    Duration::from_micros(1_000_000 / u64::try_from(frame_rate_ceiling.max(1)).unwrap_or(1))
}

fn effective_pane_frame_rate(frame_rate_ceiling: i32, focused: bool, wheel_boosted: bool) -> i32 {
    if focused || wheel_boosted {
        frame_rate_ceiling
    } else {
        frame_rate_ceiling.min(UNFOCUSED_FRAME_RATE_CAP)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ExternalBeginFrameDeadline {
    next: Instant,
    interval: Duration,
    hot: bool,
}

impl ExternalBeginFrameDeadline {
    fn anchored(now: Instant, interval: Duration, hot: bool) -> Self {
        debug_assert!(!interval.is_zero());
        Self {
            next: now.checked_add(interval).unwrap_or(now),
            interval,
            hot,
        }
    }

    fn should_send(&mut self, now: Instant, interval: Duration, hot: bool) -> bool {
        if self.interval != interval || self.hot != hot {
            let newly_hot = hot && !self.hot;
            *self = Self::anchored(now, interval, hot);
            return newly_hot;
        }
        if now < self.next {
            return false;
        }
        while self.next <= now {
            let Some(next) = self.next.checked_add(self.interval) else {
                self.next = now.checked_add(self.interval).unwrap_or(now);
                break;
            };
            self.next = next;
        }
        true
    }
}

fn external_begin_frame_due(
    deadlines: &mut BTreeMap<BrowserKey, ExternalBeginFrameDeadline>,
    key: BrowserKey,
    now: Instant,
    interval: Duration,
    hot: bool,
) -> bool {
    match deadlines.entry(key) {
        Entry::Occupied(mut entry) => entry.get_mut().should_send(now, interval, hot),
        Entry::Vacant(entry) => {
            entry.insert(ExternalBeginFrameDeadline::anchored(now, interval, hot));
            true
        }
    }
}

fn earliest_external_begin_frame_deadline(
    deadlines: &BTreeMap<BrowserKey, ExternalBeginFrameDeadline>,
) -> Option<Instant> {
    deadlines.values().map(|deadline| deadline.next).min()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AdaptiveBeginFrameThrottle {
    divisor: u32,
    sample_started_at: Option<Instant>,
    begin_frames_sent: u64,
    frames_delivered: u64,
    healthy_for: Duration,
}

impl Default for AdaptiveBeginFrameThrottle {
    fn default() -> Self {
        Self {
            divisor: 1,
            sample_started_at: None,
            begin_frames_sent: 0,
            frames_delivered: 0,
            healthy_for: Duration::ZERO,
        }
    }
}

impl AdaptiveBeginFrameThrottle {
    fn set_hot(&mut self, hot: bool, now: Instant) {
        if hot {
            if self.sample_started_at.is_none() {
                self.start_sample(now);
            }
        } else {
            self.divisor = 1;
            self.sample_started_at = None;
            self.begin_frames_sent = 0;
            self.frames_delivered = 0;
            self.healthy_for = Duration::ZERO;
        }
    }

    fn record_begin_frame(&mut self) {
        if self.sample_started_at.is_some() {
            self.begin_frames_sent = self.begin_frames_sent.saturating_add(1);
        }
    }

    fn record_frame_delivered(&mut self) {
        if self.sample_started_at.is_some() {
            self.frames_delivered = self.frames_delivered.saturating_add(1);
        }
    }

    fn update_tier(&mut self, now: Instant, frame_rate_ceiling: i32) -> bool {
        let Some(sample_started_at) = self.sample_started_at else {
            return false;
        };
        let elapsed = now.saturating_duration_since(sample_started_at);
        if elapsed < ADAPTIVE_BEGIN_FRAME_SAMPLE_WINDOW {
            return false;
        }

        let sent = self.begin_frames_sent;
        let delivered = self.frames_delivered;
        self.start_sample(now);
        if sent == 0 || delivered == 0 {
            self.healthy_for = Duration::ZERO;
            return false;
        }
        if delivered.saturating_mul(100)
            < sent.saturating_mul(ADAPTIVE_BEGIN_FRAME_DOWNSHIFT_PERCENT)
        {
            self.healthy_for = Duration::ZERO;
            return self.slow_down(frame_rate_ceiling);
        }
        if self.divisor > 1
            && delivered.saturating_mul(100)
                >= sent.saturating_mul(ADAPTIVE_BEGIN_FRAME_PROBE_PERCENT)
        {
            self.healthy_for = self.healthy_for.saturating_add(elapsed);
            if self.healthy_for >= ADAPTIVE_BEGIN_FRAME_PROBE_WINDOW {
                self.healthy_for = Duration::ZERO;
                self.divisor = (self.divisor / 2).max(1);
                return true;
            }
        } else {
            self.healthy_for = Duration::ZERO;
        }
        false
    }

    fn effective_frame_rate(self, frame_rate_ceiling: i32) -> i32 {
        let frame_rate_ceiling = frame_rate_ceiling.max(1);
        let minimum = frame_rate_ceiling.min(MIN_ADAPTIVE_BEGIN_FRAME_RATE);
        let divisor = i32::try_from(self.divisor).unwrap_or(i32::MAX);
        (frame_rate_ceiling / divisor).max(minimum)
    }

    fn interval(self, frame_rate_ceiling: i32) -> Duration {
        pump_frame_interval(self.effective_frame_rate(frame_rate_ceiling))
    }

    fn start_sample(&mut self, now: Instant) {
        self.sample_started_at = Some(now);
        self.begin_frames_sent = 0;
        self.frames_delivered = 0;
    }

    fn slow_down(&mut self, frame_rate_ceiling: i32) -> bool {
        let current_interval = self.interval(frame_rate_ceiling);
        let slower_divisor = self.divisor.saturating_mul(2);
        let slower = Self {
            divisor: slower_divisor,
            ..*self
        };
        if slower.interval(frame_rate_ceiling) <= current_interval {
            return false;
        }
        self.divisor = slower_divisor;
        true
    }
}

const DIAGNOSTIC_TARGET: &str = "zz::diagnostics::browser";

#[derive(Clone, Debug)]
pub(crate) enum ControllerEvent {
    RuntimeReady,
    Browser {
        pane: PaneId,
        tab: TabId,
        event: BrowserEvent,
    },
    CookiesImported {
        pane: PaneId,
        result: CookieImportResult,
    },
    SiteDataCleared {
        pane: PaneId,
        result: SiteDataClearResult,
    },
    BrowserDataFailed {
        pane: PaneId,
        message: Arc<str>,
    },
    BrowserFailed {
        pane: PaneId,
        message: Arc<str>,
    },
    Failed(Arc<str>),
}

#[derive(Debug)]
struct PendingBrowser {
    url: String,
    profile: String,
    egress: Option<EgressSpec>,
    viewport: Viewport,
    page_zoom_factor: f64,
    gpu_context: Option<BrowserGpuContext>,
    allow_shared_texture: bool,
}

/// One pending browser resolved into the arguments of a `create_session`
/// call, so the creation can run with the runtime detached.
struct BrowserCreateSpec {
    key: BrowserKey,
    url: String,
    profile: String,
    viewport: Viewport,
    page_zoom_factor: f64,
    gpu_context: Option<BrowserGpuContext>,
    allow_shared_texture: bool,
    frame_rate_ceiling: i32,
    watch_first_frame: bool,
}

/// How a browser pane routes its traffic through the ssh host it is attached to.
#[derive(Clone, Debug)]
pub(crate) struct EgressSpec {
    pub(crate) composite_profile: String,
    pub(crate) egress_host: String,
    pub(crate) socks_port: u16,
}

pub(crate) struct BrowserSessionRequest {
    url: String,
    profile: String,
    egress: Option<EgressSpec>,
    viewport: Viewport,
    page_zoom_factor: f64,
    gpu_context: Option<BrowserGpuContext>,
}

impl BrowserSessionRequest {
    pub(crate) fn new(
        url: String,
        profile: String,
        viewport: Viewport,
        page_zoom_factor: f64,
        gpu_context: Option<BrowserGpuContext>,
    ) -> Self {
        Self {
            url,
            profile,
            egress: None,
            viewport,
            page_zoom_factor,
            gpu_context,
        }
    }

    pub(crate) fn with_egress(mut self, egress: Option<EgressSpec>) -> Self {
        self.egress = egress;
        self
    }
}

/// The paintable payload for the newest browser frame.
#[derive(Clone)]
pub(crate) enum BrowserPaneFrameContent {
    OwnedBgra(Arc<RenderImage>),
    Gpu(wgpu::Texture),
    #[cfg(target_os = "macos")]
    MacGpu(MacIoSurface),
    #[cfg(target_os = "windows")]
    WinGpu(WinGpuTexture),
}

/// The newest OSR frame for a browser tab: a decoded GPUI image or a GPU texture.
#[derive(Clone)]
pub(crate) struct BrowserPaneFrame {
    pub(crate) session: SessionId,
    pub(crate) generation: u64,
    pub(crate) delivery_generation: u64,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) logical_width: u32,
    pub(crate) logical_height: u32,
    pub(crate) pool_generation: Option<u64>,
    pub(crate) sequence: Option<u64>,
    pub(crate) content: BrowserPaneFrameContent,
}

impl BrowserPaneFrame {
    fn from_frame(frame: OsrFrame) -> Option<Self> {
        match frame {
            OsrFrame::OwnedBgra(frame) => {
                let buffer = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(
                    frame.width,
                    frame.height,
                    frame.bgra,
                )?;
                Some(Self {
                    session: frame.session,
                    generation: frame.generation,
                    delivery_generation: frame.delivery_generation,
                    width: frame.width,
                    height: frame.height,
                    logical_width: frame.width,
                    logical_height: frame.height,
                    pool_generation: None,
                    sequence: None,
                    content: BrowserPaneFrameContent::OwnedBgra(Arc::new(RenderImage::new(vec![
                        ImageFrame::new(buffer),
                    ]))),
                })
            }
            OsrFrame::Gpu(frame) => Some(Self {
                session: frame.session,
                generation: frame.generation,
                delivery_generation: frame.delivery_generation,
                width: frame.device_width,
                height: frame.device_height,
                logical_width: frame.logical_width,
                logical_height: frame.logical_height,
                pool_generation: Some(frame.pool_generation),
                sequence: Some(frame.sequence),
                content: BrowserPaneFrameContent::Gpu(frame.texture),
            }),
            #[cfg(target_os = "macos")]
            OsrFrame::MacGpu(frame) => Some(Self {
                session: frame.session,
                generation: frame.generation,
                delivery_generation: frame.delivery_generation,
                width: frame.device_width,
                height: frame.device_height,
                logical_width: frame.logical_width,
                logical_height: frame.logical_height,
                pool_generation: Some(frame.pool_generation),
                sequence: Some(frame.sequence),
                content: BrowserPaneFrameContent::MacGpu(frame.io_surface),
            }),
            #[cfg(target_os = "windows")]
            OsrFrame::WinGpu(frame) => Some(Self {
                session: frame.session,
                generation: frame.generation,
                delivery_generation: frame.delivery_generation,
                width: frame.device_width,
                height: frame.device_height,
                logical_width: frame.logical_width,
                logical_height: frame.logical_height,
                pool_generation: Some(frame.pool_generation),
                sequence: Some(frame.sequence),
                content: BrowserPaneFrameContent::WinGpu(frame.texture),
            }),
        }
    }
}

pub struct BrowserController {
    runtime: Option<BrowserRuntime>,
    startup_error: Option<Arc<str>>,
    sessions: BTreeMap<BrowserKey, BrowserSession>,
    latest_frames: BTreeMap<BrowserKey, BrowserPaneFrame>,
    pending_browsers: BTreeMap<BrowserKey, PendingBrowser>,
    gpu_contexts: BTreeMap<BrowserKey, BrowserGpuContext>,
    browser_egress: BTreeMap<BrowserKey, EgressSpec>,
    forced_readback: BTreeSet<BrowserKey>,
    first_frame_watchdogs: BTreeMap<BrowserKey, FirstFrameWatchdog>,
    recreate_after_close: BTreeSet<BrowserKey>,
    active_tabs: BTreeMap<PaneId, TabId>,
    pane_viewports: BTreeMap<PaneId, Viewport>,
    focused_panes: BTreeSet<PaneId>,
    wheel_decay_generations: BTreeMap<BrowserKey, u64>,
    wheel_decay_generation: u64,
    external_begin_frame_hot_until: BTreeMap<BrowserKey, Instant>,
    next_external_begin_frame: BTreeMap<BrowserKey, ExternalBeginFrameDeadline>,
    adaptive_begin_frame_throttles: BTreeMap<BrowserKey, AdaptiveBeginFrameThrottle>,
    frame_rate_ceiling: Option<i32>,
    pump_hot_until: Option<Instant>,
    pump_deadline: Option<Instant>,
    pump_generation: u64,
    queued_cef_work: Vec<CefWork>,
    detached_runtime_phase: Option<RuntimePhase>,
    deferred_runtime_signals: Vec<RuntimeSignal>,
    shutting_down: bool,
}

impl BrowserController {
    pub fn new(runtime: Result<BrowserRuntime, BrowserError>, cx: &mut Context<Self>) -> Self {
        match runtime {
            Ok(runtime) => {
                let frame_rate_ceiling = runtime.frame_rate_override();
                if let Some(frame_rate_ceiling) = frame_rate_ceiling {
                    log::info!("CEF OSR effective frame-rate ceiling: {frame_rate_ceiling} FPS");
                }
                let signals = runtime.signals();
                cx.spawn(async move |this, cx| {
                    while let Ok(signal) = signals.recv().await {
                        if this
                            .update(cx, |controller, cx| {
                                controller.handle_runtime_signal(signal, cx);
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                })
                .detach();

                let mut controller = Self {
                    runtime: Some(runtime),
                    startup_error: None,
                    sessions: BTreeMap::new(),
                    latest_frames: BTreeMap::new(),
                    pending_browsers: BTreeMap::new(),
                    gpu_contexts: BTreeMap::new(),
                    browser_egress: BTreeMap::new(),
                    forced_readback: BTreeSet::new(),
                    first_frame_watchdogs: BTreeMap::new(),
                    recreate_after_close: BTreeSet::new(),
                    active_tabs: BTreeMap::new(),
                    pane_viewports: BTreeMap::new(),
                    focused_panes: BTreeSet::new(),
                    wheel_decay_generations: BTreeMap::new(),
                    wheel_decay_generation: 0,
                    external_begin_frame_hot_until: BTreeMap::new(),
                    next_external_begin_frame: BTreeMap::new(),
                    adaptive_begin_frame_throttles: BTreeMap::new(),
                    frame_rate_ceiling,
                    pump_hot_until: None,
                    pump_deadline: None,
                    pump_generation: 0,
                    queued_cef_work: Vec::new(),
                    detached_runtime_phase: None,
                    deferred_runtime_signals: Vec::new(),
                    shutting_down: false,
                };
                if controller.runtime_phase() != Some(RuntimePhase::Uninitialized) {
                    controller.schedule_pump(0, cx);
                }
                controller
            }
            Err(error) => Self {
                runtime: None,
                startup_error: Some(Arc::from(error.to_string())),
                sessions: BTreeMap::new(),
                latest_frames: BTreeMap::new(),
                pending_browsers: BTreeMap::new(),
                gpu_contexts: BTreeMap::new(),
                browser_egress: BTreeMap::new(),
                forced_readback: BTreeSet::new(),
                first_frame_watchdogs: BTreeMap::new(),
                recreate_after_close: BTreeSet::new(),
                active_tabs: BTreeMap::new(),
                pane_viewports: BTreeMap::new(),
                focused_panes: BTreeSet::new(),
                wheel_decay_generations: BTreeMap::new(),
                wheel_decay_generation: 0,
                external_begin_frame_hot_until: BTreeMap::new(),
                next_external_begin_frame: BTreeMap::new(),
                adaptive_begin_frame_throttles: BTreeMap::new(),
                frame_rate_ceiling: None,
                pump_hot_until: None,
                pump_deadline: None,
                pump_generation: 0,
                queued_cef_work: Vec::new(),
                detached_runtime_phase: None,
                deferred_runtime_signals: Vec::new(),
                shutting_down: false,
            },
        }
    }

    #[must_use]
    pub(crate) fn startup_error(&self) -> Option<Arc<str>> {
        self.startup_error.clone()
    }

    #[must_use]
    pub(crate) fn active_tab(&self, pane: PaneId) -> Option<TabId> {
        self.active_tabs.get(&pane).copied()
    }

    fn queue_cef_work(&mut self, work: impl FnOnce() + 'static, cx: &mut Context<Self>) {
        let schedule_drain = self.queued_cef_work.is_empty();
        self.queued_cef_work.push(Box::new(work));
        if !schedule_drain {
            return;
        }
        cx.spawn(async move |this, cx| {
            // Take the burst under GPUI, then enter CEF only after that borrow is gone.
            let work = this.update(cx, |controller, _| {
                std::mem::take(&mut controller.queued_cef_work)
            });
            if let Ok(work) = work {
                for work in work {
                    work();
                }
            }
        })
        .detach();
    }

    pub(crate) fn set_active_tab(&mut self, pane: PaneId, tab: TabId, cx: &mut Context<Self>) {
        let previous = self.active_tabs.insert(pane, tab);
        if previous == Some(tab) {
            return;
        }

        if let Some(previous) = previous {
            let previous_key = (pane, previous);
            self.discard_latest_frame(previous_key);
            if let Some(pending) = self.pending_browsers.get_mut(&previous_key) {
                pending.viewport.visible = false;
            }
            if let Some(session) = self.sessions.get_mut(&previous_key) {
                let mut viewport = session.viewport();
                viewport.visible = false;
                session.set_focus(false);
                session.set_viewport(viewport);
            }
            self.wheel_decay_generations.remove(&previous_key);
            self.external_begin_frame_hot_until.remove(&previous_key);
            self.next_external_begin_frame.remove(&previous_key);
            self.adaptive_begin_frame_throttles.remove(&previous_key);
            self.pause_first_frame_watchdog(previous_key);
        }

        let key = (pane, tab);
        let pane_viewport = self.pane_viewports.get(&pane).copied();
        if let Some(viewport) = pane_viewport
            && let Some(pending) = self.pending_browsers.get_mut(&key)
        {
            pending.viewport = viewport;
        }

        let focused = self.focused_panes.contains(&pane);
        let wheel_boosted = self.wheel_decay_generations.contains_key(&key);
        let frame_rate =
            effective_pane_frame_rate(self.frame_rate_ceiling(), focused, wheel_boosted);
        let became_visible = if let Some(session) = self.sessions.get_mut(&key) {
            let was_visible = session.viewport().visible;
            if let Some(viewport) = pane_viewport {
                session.set_viewport(viewport);
            }
            session.set_focus(focused);
            session.set_frame_rate(frame_rate);
            !was_visible && session.viewport().visible
        } else {
            false
        };

        if pane_viewport.is_some_and(|viewport| viewport.visible) {
            self.mark_browser_activity(key);
        }
        if became_visible {
            self.schedule_pump(0, cx);
            self.arm_first_frame_watchdog(key, cx);
        }
    }

    #[must_use]
    pub(crate) fn latest_frame(&self, pane: PaneId, tab: TabId) -> Option<BrowserPaneFrame> {
        self.latest_frames.get(&(pane, tab)).cloned()
    }

    /// Return a retired frame's pixel buffer to the tab's paint pool.
    pub(crate) fn recycle_frame(&self, pane: PaneId, tab: TabId, bgra: Vec<u8>) {
        if let Some(session) = self.sessions.get(&(pane, tab)) {
            session.recycle_frame(bgra);
        }
    }

    fn discard_latest_frame(&mut self, key: BrowserKey) {
        let Some(frame) = self.latest_frames.remove(&key) else {
            return;
        };
        if let BrowserPaneFrameContent::OwnedBgra(image) = frame.content
            && let Ok(image) = Arc::try_unwrap(image)
            && let Some(session) = self.sessions.get(&key)
        {
            for frame in image.into_frames() {
                session.recycle_frame(frame.into_buffer().into_raw());
            }
        }
    }

    fn arm_first_frame_watchdog(&mut self, key: BrowserKey, cx: &mut Context<Self>) {
        let Some(session) = self.sessions.get(&key) else {
            return;
        };
        let viewport = session.viewport();
        if session.phase() != SessionPhase::Ready
            || !viewport.visible
            || viewport.width == 0
            || viewport.height == 0
        {
            return;
        }
        let session_id = session.id();
        let Some(watchdog) = self.first_frame_watchdogs.get_mut(&key) else {
            return;
        };
        if watchdog.session != session_id {
            self.first_frame_watchdogs.remove(&key);
            return;
        }
        let Some(generation) = watchdog.arm() else {
            return;
        };

        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(SHARED_TEXTURE_FIRST_FRAME_TIMEOUT)
                .await;
            let _ = this.update(cx, |controller, _| {
                let timer_is_current = controller
                    .first_frame_watchdogs
                    .get(&key)
                    .is_some_and(|watchdog| watchdog.matches(session_id, generation));
                if !timer_is_current || controller.shutting_down {
                    return;
                }
                let session_is_visible = controller.sessions.get(&key).is_some_and(|session| {
                    let viewport = session.viewport();
                    session.id() == session_id
                        && session.phase() == SessionPhase::Ready
                        && viewport.visible
                        && viewport.width > 0
                        && viewport.height > 0
                });
                if !session_is_visible {
                    controller.pause_first_frame_watchdog(key);
                    return;
                }
                controller.request_readback_fallback(
                    key,
                    "no first OSR frame arrived within 2 seconds of creating a visible shared-texture browser",
                );
            });
        })
        .detach();
    }

    fn pause_first_frame_watchdog(&mut self, key: BrowserKey) {
        if let Some(watchdog) = self.first_frame_watchdogs.get_mut(&key) {
            watchdog.pause();
        }
    }

    fn request_readback_fallback(&mut self, key: BrowserKey, reason: &str) -> bool {
        if self.forced_readback.contains(&key) {
            self.first_frame_watchdogs.remove(&key);
            return false;
        }
        let Some(session) = self.sessions.get(&key) else {
            self.first_frame_watchdogs.remove(&key);
            return false;
        };
        let session_id = session.id();
        let url = session
            .current_url()
            .unwrap_or_else(|| "about:blank".to_owned());
        let viewport = session.viewport();
        let profile = session.profile().to_owned();
        let page_zoom_factor = session.page_zoom_factor();
        let gpu_context = self.gpu_contexts.get(&key).cloned();

        log::warn!(
            target: "zz_browser::accelerated_paint",
            "shared-texture session {} is falling back to readback: {reason}",
            session_id.0,
        );
        self.forced_readback.insert(key);
        self.first_frame_watchdogs.remove(&key);
        self.pending_browsers.insert(
            key,
            PendingBrowser {
                url,
                profile,
                egress: self.browser_egress.get(&key).cloned(),
                viewport,
                page_zoom_factor,
                gpu_context,
                allow_shared_texture: false,
            },
        );
        self.recreate_after_close.insert(key);
        if let Some(session) = self.sessions.get_mut(&key)
            && session.id() == session_id
        {
            session.close(true);
        }
        true
    }

    #[must_use]
    pub(crate) fn runtime_phase(&self) -> Option<RuntimePhase> {
        self.detached_runtime_phase
            .or_else(|| self.runtime.as_ref().map(BrowserRuntime::phase))
    }

    fn frame_rate_ceiling(&self) -> i32 {
        resolve_frame_rate_ceiling_value(self.frame_rate_ceiling, None).0
    }

    fn resolve_frame_rate_ceiling(&mut self, cx: &gpui::App) {
        if self.frame_rate_ceiling.is_some() {
            return;
        }
        let (_, resolved) = resolve_frame_rate_ceiling_value(
            self.frame_rate_ceiling,
            display_frame_rate_ceiling(cx),
        );
        self.frame_rate_ceiling = resolved;
        if let Some(frame_rate_ceiling) = resolved {
            log::info!("CEF OSR effective frame-rate ceiling: {frame_rate_ceiling} FPS");
        }
    }

    #[must_use]
    pub(crate) fn external_begin_frame_enabled(&self) -> bool {
        self.runtime
            .as_ref()
            .is_some_and(BrowserRuntime::external_begin_frame_enabled)
    }

    #[must_use]
    fn begin_frame_adaptive_enabled(&self) -> bool {
        self.runtime
            .as_ref()
            .is_some_and(BrowserRuntime::begin_frame_adaptive_enabled)
    }

    fn mark_browser_activity(&mut self, key: BrowserKey) {
        self.pump_hot_until = Some(Instant::now() + PUMP_HOT_WINDOW);
        if self.external_begin_frame_enabled() {
            self.external_begin_frame_hot_until
                .insert(key, Instant::now() + EXTERNAL_BEGIN_FRAME_HOT_WINDOW);
        }
    }

    fn kick_pump_if_cold(&mut self, was_hot: bool, cx: &mut Context<Self>) {
        if !was_hot {
            self.schedule_pump(0, cx);
        }
    }

    fn pump_is_hot(&self) -> bool {
        self.pump_hot_until
            .is_some_and(|until| until > Instant::now())
    }

    fn send_due_external_begin_frames(&mut self) {
        if !self.external_begin_frame_enabled() {
            return;
        }
        let now = Instant::now();
        let adaptive_enabled = self.begin_frame_adaptive_enabled();
        let frame_rate_ceiling = self.frame_rate_ceiling();
        for (key, session) in &self.sessions {
            if !(session.viewport().visible
                && matches!(
                    session.phase(),
                    SessionPhase::Creating | SessionPhase::Ready
                ))
            {
                self.next_external_begin_frame.remove(key);
                if let Some(throttle) = self.adaptive_begin_frame_throttles.get_mut(key) {
                    throttle.set_hot(false, now);
                }
                continue;
            }
            let hot = self
                .external_begin_frame_hot_until
                .get(key)
                .is_some_and(|until| *until > now);
            let pane_frame_rate = effective_pane_frame_rate(
                frame_rate_ceiling,
                self.focused_panes.contains(&key.0),
                self.wheel_decay_generations.contains_key(key),
            );
            let interval = if hot {
                if adaptive_enabled {
                    let throttle = self.adaptive_begin_frame_throttles.entry(*key).or_default();
                    throttle.set_hot(true, now);
                    if throttle.update_tier(now, pane_frame_rate) {
                        log::debug!(
                            "CEF external BeginFrame adaptive tier for {} tab {}: divisor={} effective_rate={} FPS",
                            key.0,
                            key.1.0,
                            throttle.divisor,
                            throttle.effective_frame_rate(pane_frame_rate),
                        );
                    }
                    throttle.interval(pane_frame_rate)
                } else {
                    pump_frame_interval(pane_frame_rate)
                }
            } else {
                if let Some(throttle) = self.adaptive_begin_frame_throttles.get_mut(key) {
                    throttle.set_hot(false, now);
                }
                VISIBLE_PUMP_WATCHDOG_INTERVAL
            };
            if external_begin_frame_due(
                &mut self.next_external_begin_frame,
                *key,
                now,
                interval,
                hot,
            ) {
                session.send_external_begin_frame();
                if hot && adaptive_enabled {
                    self.adaptive_begin_frame_throttles
                        .get_mut(key)
                        .expect("hot adaptive browser tab has a throttle")
                        .record_begin_frame();
                }
            }
        }
    }

    fn record_external_begin_frame_delivery(&mut self, key: BrowserKey) {
        if self.begin_frame_adaptive_enabled()
            && let Some(throttle) = self.adaptive_begin_frame_throttles.get_mut(&key)
        {
            throttle.record_frame_delivered();
        }
    }

    pub(crate) fn request_browser(
        &mut self,
        pane: PaneId,
        tab: TabId,
        request: BrowserSessionRequest,
        cx: &mut Context<Self>,
    ) {
        let BrowserSessionRequest {
            url,
            profile,
            egress,
            mut viewport,
            page_zoom_factor,
            gpu_context,
        } = request;
        let key = (pane, tab);
        let pane_viewport = *self.pane_viewports.entry(pane).or_insert(viewport);
        viewport = pane_viewport;
        viewport.visible &= self.active_tab(pane) == Some(tab);
        log::trace!(
            target: "zz::diagnostics::browser",
            "request_browser pane={pane} tab={} initial_url={url:?} profile={profile:?} viewport={viewport:?} page_zoom_factor={page_zoom_factor} shutting_down={} existing_session={}",
            tab.0,
            self.shutting_down,
            self.sessions.contains_key(&key),
        );
        if self.sessions.contains_key(&key) || self.shutting_down {
            return;
        }
        if let Some(gpu_context) = gpu_context.clone() {
            self.gpu_contexts.insert(key, gpu_context);
        }
        let allow_shared_texture = !self.forced_readback.contains(&key);
        self.pending_browsers.insert(
            key,
            PendingBrowser {
                url,
                profile,
                egress,
                viewport,
                page_zoom_factor,
                gpu_context,
                allow_shared_texture,
            },
        );
        if !self.ensure_runtime_started(cx) {
            return;
        }
        self.try_create_browsers(cx);
    }

    pub(crate) fn retry(
        &mut self,
        pane: PaneId,
        tab: TabId,
        request: BrowserSessionRequest,
        cx: &mut Context<Self>,
    ) {
        if self.shutting_down {
            return;
        }
        let BrowserSessionRequest {
            url,
            profile,
            egress,
            mut viewport,
            page_zoom_factor,
            gpu_context,
        } = request;
        let key = (pane, tab);
        let pane_viewport = *self.pane_viewports.entry(pane).or_insert(viewport);
        viewport = pane_viewport;
        viewport.visible &= self.active_tab(pane) == Some(tab);
        if let Some(gpu_context) = gpu_context.clone() {
            self.gpu_contexts.insert(key, gpu_context);
        }
        let allow_shared_texture = !self.forced_readback.contains(&key);
        self.pending_browsers.insert(
            key,
            PendingBrowser {
                url,
                profile,
                egress,
                viewport,
                page_zoom_factor,
                gpu_context,
                allow_shared_texture,
            },
        );
        if !self.ensure_runtime_started(cx) {
            return;
        }
        self.discard_latest_frame(key);
        if let Some(session) = self.sessions.get_mut(&key) {
            self.recreate_after_close.insert(key);
            session.close(true);
        } else {
            self.try_create_browsers(cx);
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn force_readback(&mut self, pane: PaneId, tab: TabId, reason: &str) {
        self.request_readback_fallback((pane, tab), reason);
    }

    pub(crate) fn navigate(&mut self, pane: PaneId, tab: TabId, url: &str, cx: &mut Context<Self>) {
        log::trace!(
            target: "zz::diagnostics::browser",
            "navigate pane={pane} tab={} url={url:?}",
            tab.0,
        );
        let Some(browser) = self
            .sessions
            .get(&(pane, tab))
            .map(BrowserSession::command_sink)
        else {
            return;
        };
        let url = url.to_owned();
        self.queue_cef_work(move || browser.navigate(&url), cx);
    }

    pub(crate) fn go_back(&mut self, pane: PaneId, tab: TabId, cx: &mut Context<Self>) {
        let Some(browser) = self
            .sessions
            .get(&(pane, tab))
            .map(BrowserSession::command_sink)
        else {
            return;
        };
        self.queue_cef_work(move || browser.go_back(), cx);
    }

    pub(crate) fn go_forward(&mut self, pane: PaneId, tab: TabId, cx: &mut Context<Self>) {
        let Some(browser) = self
            .sessions
            .get(&(pane, tab))
            .map(BrowserSession::command_sink)
        else {
            return;
        };
        self.queue_cef_work(move || browser.go_forward(), cx);
    }

    pub(crate) fn reload(&mut self, pane: PaneId, tab: TabId, cx: &mut Context<Self>) {
        let Some(browser) = self
            .sessions
            .get(&(pane, tab))
            .map(BrowserSession::command_sink)
        else {
            return;
        };
        self.queue_cef_work(move || browser.reload(), cx);
    }

    pub(crate) fn toggle_dev_tools(&self, pane: PaneId, tab: TabId) {
        if let Some(session) = self.sessions.get(&(pane, tab)) {
            session.toggle_dev_tools();
        }
    }

    pub(crate) fn inspect_element_at(&self, pane: PaneId, tab: TabId, x: i32, y: i32) {
        if let Some(session) = self.sessions.get(&(pane, tab)) {
            session.inspect_element_at(x, y);
        }
    }

    pub(crate) fn edit(
        &mut self,
        pane: PaneId,
        tab: TabId,
        command: EditCommand,
        cx: &mut Context<Self>,
    ) {
        let key = (pane, tab);
        self.mark_browser_activity(key);
        let Some(input) = self.sessions.get(&key).map(BrowserSession::command_sink) else {
            return;
        };
        self.queue_cef_work(move || input.edit(command), cx);
    }

    pub(crate) fn zoom_in(&self, pane: PaneId, tab: TabId) -> Option<(f64, u16)> {
        self.sessions.get(&(pane, tab)).map(|session| {
            let percent = session.zoom_in();
            (session.page_zoom_factor(), percent)
        })
    }

    pub(crate) fn zoom_out(&self, pane: PaneId, tab: TabId) -> Option<(f64, u16)> {
        self.sessions.get(&(pane, tab)).map(|session| {
            let percent = session.zoom_out();
            (session.page_zoom_factor(), percent)
        })
    }

    pub(crate) fn reset_zoom(&self, pane: PaneId, tab: TabId) -> Option<(f64, u16)> {
        self.sessions.get(&(pane, tab)).map(|session| {
            let percent = session.reset_zoom();
            (session.page_zoom_factor(), percent)
        })
    }

    pub(crate) fn import_cookies(
        &mut self,
        pane: PaneId,
        tab: TabId,
        batch: CookieImportBatch,
        cx: &mut Context<Self>,
    ) {
        if !self.ensure_runtime_started(cx) {
            cx.emit(ControllerEvent::BrowserDataFailed {
                pane,
                message: self.runtime_unavailable_message(),
            });
            return;
        }
        let result = self
            .sessions
            .get(&(pane, tab))
            .ok_or(BrowserError::NotReady)
            .and_then(|session| {
                self.runtime
                    .as_ref()
                    .ok_or(BrowserError::AlreadyShutdown)
                    .and_then(|runtime| runtime.import_cookies(session.profile(), batch))
            });
        let results = match result {
            Ok(results) => results,
            Err(error) => {
                cx.emit(ControllerEvent::BrowserDataFailed {
                    pane,
                    message: Arc::from(error.to_string()),
                });
                return;
            }
        };

        cx.spawn(async move |this, cx| {
            let event = match results.recv().await {
                Ok(result) => ControllerEvent::CookiesImported { pane, result },
                Err(_) => ControllerEvent::BrowserDataFailed {
                    pane,
                    message: Arc::from("the cookie import was interrupted"),
                },
            };
            let _ = this.update(cx, |_, cx| cx.emit(event));
        })
        .detach();
        self.schedule_pump(0, cx);
    }

    pub(crate) fn clear_site_data(&mut self, pane: PaneId, tab: TabId, cx: &mut Context<Self>) {
        if !self.ensure_runtime_started(cx) {
            cx.emit(ControllerEvent::BrowserDataFailed {
                pane,
                message: self.runtime_unavailable_message(),
            });
            return;
        }
        let key = (pane, tab);
        let Some(session) = self.sessions.get_mut(&key) else {
            cx.emit(ControllerEvent::BrowserDataFailed {
                pane,
                message: Arc::from("the browser is not ready"),
            });
            return;
        };
        let session_id = session.id();
        let results = match session.clear_site_data() {
            Ok(results) => results,
            Err(error) => {
                cx.emit(ControllerEvent::BrowserDataFailed {
                    pane,
                    message: Arc::from(error.to_string()),
                });
                return;
            }
        };

        cx.spawn(async move |this, cx| {
            let result = results.recv().await;
            let _ = this.update(cx, |controller, cx| match result {
                Ok(result) => {
                    if let Some(session) = controller.sessions.get_mut(&key)
                        && session.id() == session_id
                    {
                        session.finish_site_data_clear(result.message_id);
                    }
                    cx.emit(ControllerEvent::SiteDataCleared { pane, result });
                }
                Err(_) => cx.emit(ControllerEvent::BrowserDataFailed {
                    pane,
                    message: Arc::from("clearing site data was interrupted"),
                }),
            });
        })
        .detach();
        self.schedule_pump(0, cx);
    }

    #[must_use]
    pub(crate) fn start_element_pick(
        &self,
        pane: PaneId,
        tab: TabId,
        appearance: &ElementPickerAppearance,
    ) -> bool {
        self.sessions
            .get(&(pane, tab))
            .is_some_and(|session| session.start_element_pick(appearance))
    }

    #[must_use]
    pub(crate) fn cancel_element_pick(&self, pane: PaneId, tab: TabId) -> bool {
        self.sessions
            .get(&(pane, tab))
            .is_some_and(BrowserSession::cancel_element_pick)
    }

    pub(crate) fn set_viewport(
        &mut self,
        pane: PaneId,
        viewport: Viewport,
        cx: &mut Context<Self>,
    ) {
        let started = diagnostics::timer(DIAGNOSTIC_TARGET);
        self.pane_viewports.insert(pane, viewport);
        let active_tab = self.active_tab(pane);
        let key = active_tab.map(|tab| (pane, tab));
        let previous = started
            .is_some()
            .then(|| key.and_then(|key| self.sessions.get(&key).map(BrowserSession::viewport)))
            .flatten();
        let became_visible = key.is_some_and(|key| {
            self.sessions
                .get(&key)
                .is_some_and(|session| !session.viewport().visible && viewport.visible)
        });
        if let Some(key) = key {
            if let Some(pending) = self.pending_browsers.get_mut(&key) {
                pending.viewport = viewport;
            }
            if let Some(session) = self.sessions.get_mut(&key) {
                session.set_viewport(viewport);
            }
            if viewport.visible {
                self.mark_browser_activity(key);
                self.arm_first_frame_watchdog(key, cx);
            } else {
                self.external_begin_frame_hot_until.remove(&key);
                self.next_external_begin_frame.remove(&key);
                self.pause_first_frame_watchdog(key);
                if let Some(throttle) = self.adaptive_begin_frame_throttles.get_mut(&key) {
                    throttle.set_hot(false, Instant::now());
                }
            }
        }
        if became_visible {
            self.schedule_pump(0, cx);
        }
        log::trace!(
            target: "zz::diagnostics::browser",
            "set_viewport pane={pane} active_tab={active_tab:?} previous={previous:?} next={viewport:?} pending={} session={} elapsed_us={}",
            key.is_some_and(|key| self.pending_browsers.contains_key(&key)),
            key.is_some_and(|key| self.sessions.contains_key(&key)),
            diagnostics::elapsed_us(started),
        );
    }

    pub(crate) fn set_focus(&mut self, pane: PaneId, focused: bool) {
        log::trace!(target: "zz::diagnostics::browser", "set_focus pane={pane} focused={focused}");
        if focused {
            self.focused_panes.insert(pane);
        } else {
            self.focused_panes.remove(&pane);
        }
        let Some(tab) = self.active_tab(pane) else {
            return;
        };
        let key = (pane, tab);
        if focused {
            self.wheel_decay_generations.remove(&key);
        }
        let frame_rate_ceiling = self.frame_rate_ceiling();
        let wheel_boosted = self.wheel_decay_generations.contains_key(&key);
        if let Some(session) = self.sessions.get(&key) {
            session.set_focus(focused);
            session.set_frame_rate(effective_pane_frame_rate(
                frame_rate_ceiling,
                focused,
                wheel_boosted,
            ));
        }
        if focused {
            self.mark_browser_activity(key);
        }
    }

    pub(crate) fn send_pointer(&mut self, pane: PaneId, tab: TabId, event: PointerEvent) {
        log::trace!(
            target: "zz::diagnostics::browser",
            "send_pointer pane={pane} tab={} event={event:?}",
            tab.0,
        );
        let key = (pane, tab);
        self.mark_browser_activity(key);
        if let Some(session) = self.sessions.get(&key) {
            session.send_pointer(event);
        }
    }

    pub(crate) fn send_wheel(
        &mut self,
        pane: PaneId,
        tab: TabId,
        event: WheelEvent,
        cx: &mut Context<Self>,
    ) {
        log::trace!(
            target: "zz::diagnostics::browser",
            "send_wheel pane={pane} tab={} event={event:?}",
            tab.0,
        );
        let key = (pane, tab);
        let was_hot = self.pump_is_hot();
        self.mark_browser_activity(key);
        self.kick_pump_if_cold(was_hot, cx);
        let frame_rate_ceiling = self.frame_rate_ceiling();
        let Some(session) = self.sessions.get(&key) else {
            return;
        };
        session.send_wheel(event);
        if self.focused_panes.contains(&pane) {
            return;
        }
        session.set_frame_rate(frame_rate_ceiling);
        self.wheel_decay_generation = self.wheel_decay_generation.wrapping_add(1).max(1);
        let generation = self.wheel_decay_generation;
        self.wheel_decay_generations.insert(key, generation);
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(UNFOCUSED_SCROLL_FRAME_RATE_DECAY)
                .await;
            let _ = this.update(cx, |controller, _| {
                if controller.wheel_decay_generations.get(&key) != Some(&generation) {
                    return;
                }
                controller.wheel_decay_generations.remove(&key);
                if controller.focused_panes.contains(&pane) {
                    return;
                }
                if let Some(session) = controller.sessions.get(&key) {
                    session.set_frame_rate(effective_pane_frame_rate(
                        controller.frame_rate_ceiling(),
                        false,
                        false,
                    ));
                }
            });
        })
        .detach();
    }

    pub(crate) fn send_key(
        &mut self,
        pane: PaneId,
        tab: TabId,
        event: KeyInput,
        cx: &mut Context<Self>,
    ) {
        log::trace!(
            target: "zz::diagnostics::browser",
            "send_key pane={pane} tab={} event={event:?}",
            tab.0,
        );
        let key = (pane, tab);
        self.mark_browser_activity(key);
        let Some(input) = self.sessions.get(&key).map(BrowserSession::command_sink) else {
            return;
        };
        self.queue_cef_work(move || input.send_key(event), cx);
    }

    pub(crate) fn send_text(
        &mut self,
        pane: PaneId,
        tab: TabId,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        log::trace!(
            target: "zz::diagnostics::browser",
            "send_text pane={pane} tab={} text={text:?}",
            tab.0,
        );
        let key = (pane, tab);
        self.mark_browser_activity(key);
        let Some(input) = self.sessions.get(&key).map(BrowserSession::command_sink) else {
            return;
        };
        let text = text.to_owned();
        self.queue_cef_work(move || input.send_text(&text), cx);
    }

    pub(crate) fn commit_composition(
        &mut self,
        pane: PaneId,
        tab: TabId,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        let key = (pane, tab);
        self.mark_browser_activity(key);
        let Some(input) = self.sessions.get(&key).map(BrowserSession::command_sink) else {
            return;
        };
        let text = text.to_owned();
        self.queue_cef_work(move || input.commit_composition(&text), cx);
    }

    pub(crate) fn set_composition(
        &mut self,
        pane: PaneId,
        tab: TabId,
        text: &str,
        selection: std::ops::Range<usize>,
        cx: &mut Context<Self>,
    ) {
        let key = (pane, tab);
        self.mark_browser_activity(key);
        let Some(input) = self.sessions.get(&key).map(BrowserSession::command_sink) else {
            return;
        };
        let text = text.to_owned();
        self.queue_cef_work(move || input.set_composition(&text, selection), cx);
    }

    pub(crate) fn finish_composition(&mut self, pane: PaneId, tab: TabId, cx: &mut Context<Self>) {
        let key = (pane, tab);
        self.mark_browser_activity(key);
        let Some(input) = self.sessions.get(&key).map(BrowserSession::command_sink) else {
            return;
        };
        self.queue_cef_work(move || input.finish_composition(), cx);
    }

    pub(crate) fn cancel_composition(&mut self, pane: PaneId, tab: TabId, cx: &mut Context<Self>) {
        let key = (pane, tab);
        self.mark_browser_activity(key);
        let Some(input) = self.sessions.get(&key).map(BrowserSession::command_sink) else {
            return;
        };
        self.queue_cef_work(move || input.cancel_composition(), cx);
    }

    #[allow(
        dead_code,
        reason = "the tab-strip follow-up is the first production caller"
    )]
    pub(crate) fn close_tab(&mut self, pane: PaneId, tab: TabId) {
        if self.active_tab(pane) == Some(tab) {
            log::error!(
                "attempted to close active browser tab {} in {pane}; activate its replacement first",
                tab.0,
            );
            debug_assert_ne!(
                self.active_tab(pane),
                Some(tab),
                "the caller must activate another browser tab before closing the active tab",
            );
        }
        self.close_tab_state((pane, tab));
    }

    fn close_tab_state(&mut self, key: BrowserKey) {
        self.pending_browsers.remove(&key);
        self.browser_egress.remove(&key);
        self.discard_latest_frame(key);
        self.gpu_contexts.remove(&key);
        self.forced_readback.remove(&key);
        self.first_frame_watchdogs.remove(&key);
        self.recreate_after_close.remove(&key);
        self.wheel_decay_generations.remove(&key);
        self.external_begin_frame_hot_until.remove(&key);
        self.next_external_begin_frame.remove(&key);
        self.adaptive_begin_frame_throttles.remove(&key);
        if let Some(session) = self.sessions.get_mut(&key) {
            session.close(false);
        }
    }

    fn tab_keys_for_pane(&self, pane: PaneId) -> BTreeSet<BrowserKey> {
        let range = browser_key_range(pane);
        let mut keys = BTreeSet::new();
        keys.extend(self.sessions.range(range.clone()).map(|(key, _)| *key));
        keys.extend(self.latest_frames.range(range.clone()).map(|(key, _)| *key));
        keys.extend(
            self.pending_browsers
                .range(range.clone())
                .map(|(key, _)| *key),
        );
        keys.extend(self.gpu_contexts.range(range.clone()).map(|(key, _)| *key));
        keys.extend(
            self.browser_egress
                .range(range.clone())
                .map(|(key, _)| *key),
        );
        keys.extend(self.forced_readback.range(range.clone()).copied());
        keys.extend(
            self.first_frame_watchdogs
                .range(range.clone())
                .map(|(key, _)| *key),
        );
        keys.extend(self.recreate_after_close.range(range.clone()).copied());
        keys.extend(
            self.wheel_decay_generations
                .range(range.clone())
                .map(|(key, _)| *key),
        );
        keys.extend(
            self.external_begin_frame_hot_until
                .range(range.clone())
                .map(|(key, _)| *key),
        );
        keys.extend(
            self.next_external_begin_frame
                .range(range.clone())
                .map(|(key, _)| *key),
        );
        keys.extend(
            self.adaptive_begin_frame_throttles
                .range(range)
                .map(|(key, _)| *key),
        );
        keys
    }

    /// Point every route this pane recorded at the SOCKS port a reconnect brought
    /// up. Best effort: callers run this on every snapshot refresh.
    pub(crate) fn refresh_egress(&mut self, pane: PaneId, spec: Option<EgressSpec>) {
        let Some(spec) = spec else {
            return;
        };
        for (_, pending) in self.pending_browsers.range_mut(browser_key_range(pane)) {
            if let Some(current) = pending.egress.as_mut()
                && current.composite_profile == spec.composite_profile
            {
                current.socks_port = spec.socks_port;
            }
        }
        let mut routed = false;
        for (_, current) in self.browser_egress.range_mut(browser_key_range(pane)) {
            if current.composite_profile == spec.composite_profile {
                current.socks_port = spec.socks_port;
                routed = true;
            }
        }
        if !routed {
            return;
        }
        let Some(runtime) = self.runtime.as_mut() else {
            return;
        };
        if let Err(error) = runtime.set_profile_proxy(&spec.composite_profile, spec.socks_port) {
            log::debug!(
                target: "zz::browser::egress",
                "could not repoint profile={:?} egress_host={:?} at socks_port={}: {error}",
                spec.composite_profile,
                spec.egress_host,
                spec.socks_port,
            );
        }
    }

    pub(crate) fn close_pane(&mut self, pane: PaneId) {
        for key in self.tab_keys_for_pane(pane) {
            self.close_tab_state(key);
        }
        self.active_tabs.remove(&pane);
        self.pane_viewports.remove(&pane);
        self.focused_panes.remove(&pane);
    }

    #[must_use]
    pub(crate) fn is_shutting_down(&self) -> bool {
        self.shutting_down
    }

    #[must_use]
    pub(crate) fn is_shutdown_complete(&self) -> bool {
        self.detached_runtime_phase.is_none()
            && self.runtime.as_ref().is_none_or(|runtime| {
                matches!(runtime.phase(), RuntimePhase::Closed | RuntimePhase::Failed)
            })
    }

    pub(crate) fn log_diagnostic_snapshot(&self, reason: &str) {
        let runtime_phase = self.runtime.as_ref().map(BrowserRuntime::phase);
        let pump_fallback_ms = self
            .pump_fallback_interval()
            .map(|interval| interval.as_millis());
        let active_sessions = self
            .runtime
            .as_ref()
            .map(BrowserRuntime::active_session_count);
        let frame_rate = self.runtime.as_ref().map(|_| self.frame_rate_ceiling());
        let profile_paths = self.runtime.as_ref().map(BrowserRuntime::profile_paths);
        log::info!(
            target: "zz::diagnostics::browser_state",
            "snapshot reason={reason} runtime_phase={runtime_phase:?} active_runtime_sessions={active_sessions:?} frame_rate={frame_rate:?} profile_paths={profile_paths:?} controller_sessions={} latest_frames={} pending_browsers={} forced_readback={} first_frame_watchdogs={} recreate_after_close={} pump_fallback_ms={pump_fallback_ms:?} pump_deadline_us={:?} pump_generation={} shutting_down={} startup_error={:?}",
            self.sessions.len(),
            self.latest_frames.len(),
            self.pending_browsers.len(),
            self.forced_readback.len(),
            self.first_frame_watchdogs.len(),
            self.recreate_after_close.len(),
            self.pump_deadline
                .map(|deadline| deadline.saturating_duration_since(Instant::now()).as_micros()),
            self.pump_generation,
            self.shutting_down,
            self.startup_error,
        );
        for ((pane, tab), session) in &self.sessions {
            log::info!(
                target: "zz::diagnostics::browser_state",
                "snapshot reason={reason} pane={pane} tab={} session={} phase={:?} viewport={:?} mailbox={:?}",
                tab.0,
                session.id().0,
                session.phase(),
                session.viewport(),
                session.frame_mailbox_diagnostics(),
            );
        }
        for ((pane, tab), frame) in &self.latest_frames {
            let (tier, bytes, image_strong_count) = match &frame.content {
                BrowserPaneFrameContent::OwnedBgra(image) => (
                    "owned_bgra",
                    usize::try_from(frame.width)
                        .unwrap_or(usize::MAX)
                        .saturating_mul(usize::try_from(frame.height).unwrap_or(usize::MAX))
                        .saturating_mul(4),
                    Some(Arc::strong_count(image)),
                ),
                BrowserPaneFrameContent::Gpu(_) => ("gpu", 0, None),
                #[cfg(target_os = "macos")]
                BrowserPaneFrameContent::MacGpu(_) => ("mac_gpu", 0, None),
                #[cfg(target_os = "windows")]
                BrowserPaneFrameContent::WinGpu(_) => ("win_gpu", 0, None),
            };
            log::info!(
                target: "zz::diagnostics::browser_state",
                "snapshot reason={reason} pane={pane} tab={} latest_frame_session={} generation={} delivery_generation={} tier={tier} logical={}x{} device={}x{} bytes={bytes} image_strong_count={image_strong_count:?} pool_generation={:?} sequence={:?}",
                tab.0,
                frame.session.0,
                frame.generation,
                frame.delivery_generation,
                frame.logical_width,
                frame.logical_height,
                frame.width,
                frame.height,
                frame.pool_generation,
                frame.sequence,
            );
        }
        log::trace!(
            target: "zz::diagnostics::browser_state",
            "snapshot reason={reason} pending_browsers={:#?} recreate_after_close={:#?}",
            self.pending_browsers,
            self.recreate_after_close,
        );
    }

    pub(crate) fn shutdown(&mut self, cx: &mut Context<Self>) -> Task<bool> {
        if self.shutting_down {
            return Task::ready(self.is_shutdown_complete());
        }
        self.shutting_down = true;
        self.pending_browsers.clear();
        self.first_frame_watchdogs.clear();
        self.recreate_after_close.clear();
        self.browser_egress.clear();
        let data_operations_active = self
            .runtime
            .as_ref()
            .is_some_and(|runtime| runtime.active_data_operation_count() > 0);
        if self.sessions.is_empty()
            && !data_operations_active
            && self.detached_runtime_phase.is_none()
        {
            self.shutdown_runtime();
            return Task::ready(self.is_shutdown_complete());
        }
        for session in self.sessions.values_mut() {
            session.close(false);
        }

        cx.spawn(async move |this, cx| {
            let graceful_deadline = Instant::now() + GRACEFUL_CLOSE_TIMEOUT;
            let fatal_deadline = graceful_deadline + FORCED_CLOSE_TIMEOUT;
            let mut forced = false;
            loop {
                let timer = cx.background_executor().timer(Duration::from_millis(10));
                timer.await;
                let pump = match this.update(cx, |controller, _| {
                    controller
                        .runtime
                        .as_ref()
                        .map(BrowserRuntime::message_pump)
                }) {
                    Ok(pump) => pump,
                    Err(error) => {
                        log::error!("lost the browser controller during CEF shutdown: {error}");
                        return false;
                    }
                };
                if let Some(pump) = pump {
                    pump.do_message_loop_work();
                }
                let result = this.update(cx, |controller, _| {
                    let sessions_closed = controller
                        .sessions
                        .values()
                        .all(|session| session.phase() == SessionPhase::Closed);
                    let data_operations_finished = controller
                        .runtime
                        .as_ref()
                        .is_none_or(|runtime| runtime.active_data_operation_count() == 0);
                    if sessions_closed
                        && data_operations_finished
                        && controller.detached_runtime_phase.is_none()
                    {
                        controller.sessions.clear();
                        controller.shutdown_runtime();
                        return ShutdownProgress::Complete;
                    }
                    let now = Instant::now();
                    if !forced && now >= graceful_deadline {
                        for session in controller.sessions.values_mut() {
                            session.close(true);
                        }
                        forced = true;
                    }
                    if now >= fatal_deadline {
                        return ShutdownProgress::TimedOut;
                    }
                    ShutdownProgress::Waiting
                });
                match result {
                    Ok(ShutdownProgress::Waiting) => {}
                    Ok(ShutdownProgress::Complete) => return true,
                    Err(error) => {
                        log::error!("lost the browser controller during CEF shutdown: {error}");
                        return false;
                    }
                    Ok(ShutdownProgress::TimedOut) => {
                        log::error!(
                            "CEF browser or data operation did not finish before the shutdown deadline"
                        );
                        return false;
                    }
                }
            }
        })
    }

    fn handle_runtime_signal(&mut self, signal: RuntimeSignal, cx: &mut Context<Self>) {
        if self.detached_runtime_phase.is_some() {
            self.deferred_runtime_signals.push(signal);
            return;
        }
        match signal {
            RuntimeSignal::ScheduleMessagePump(delay_ms) => {
                self.schedule_pump(delay_ms, cx);
            }
            RuntimeSignal::ContextInitialized => {
                log::debug!("CEF global context initialized");
                let result = self
                    .runtime
                    .as_mut()
                    .ok_or(BrowserError::AlreadyShutdown)
                    .and_then(BrowserRuntime::handle_context_initialized);
                if let Err(error) = result {
                    self.fail(error.to_string(), cx);
                }
            }
            RuntimeSignal::RequestContextInitialized { profile } => {
                log::debug!("CEF persistent request context initialized: {profile}");
                let result = self
                    .runtime
                    .as_mut()
                    .ok_or(BrowserError::AlreadyShutdown)
                    .and_then(|runtime| runtime.handle_request_context_initialized(&profile));
                match result {
                    Ok(runtime_became_ready) => {
                        if runtime_became_ready {
                            cx.emit(ControllerEvent::RuntimeReady);
                        }
                        self.try_create_browsers(cx);
                    }
                    Err(error) => self.fail(error.to_string(), cx),
                }
            }
        }
    }

    fn ensure_runtime_started(&mut self, cx: &mut Context<Self>) -> bool {
        if let Some(phase) = self.detached_runtime_phase {
            return !matches!(
                phase,
                RuntimePhase::Closing | RuntimePhase::Closed | RuntimePhase::Failed
            );
        }
        let Some(phase) = self.runtime.as_ref().map(BrowserRuntime::phase) else {
            let message = self.runtime_unavailable_message();
            self.fail_pending_browsers(message.as_ref(), cx);
            return false;
        };
        match phase {
            RuntimePhase::Initializing | RuntimePhase::Running => true,
            RuntimePhase::Uninitialized => {
                self.start_runtime(cx);
                true
            }
            RuntimePhase::Closing | RuntimePhase::Closed | RuntimePhase::Failed => {
                let message = self.runtime_unavailable_message();
                self.fail_pending_browsers(message.as_ref(), cx);
                false
            }
        }
    }

    /// `cef::initialize` runs Chromium startup work on this thread, and that
    /// work can service the GCD main queue: a gpui runnable drained there must
    /// find the App borrow free or it panics with `RefCell already borrowed`.
    /// So, like the pump calls, the runtime leaves the controller and
    /// initializes at task top level, outside every borrow.
    fn start_runtime(&mut self, cx: &mut Context<Self>) {
        let Some(mut runtime) = self.detach_runtime(RuntimePhase::Initializing) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let result = runtime.start();
            let mut payload = Some((runtime, result));
            let updated = this.update(cx, |controller, cx| {
                let (runtime, result) = payload
                    .take()
                    .expect("the reattach closure runs at most once");
                controller.reattach_runtime(runtime);
                if let Err(error) = result {
                    controller.deferred_runtime_signals.clear();
                    controller.fail(error.to_string(), cx);
                } else {
                    controller.replay_deferred_runtime_signals(cx);
                    controller.schedule_pump(0, cx);
                }
            });
            if updated.is_err()
                && let Some((mut runtime, _)) = payload.take()
                && let Err(error) = runtime.shutdown()
            {
                log::error!("failed to shut down CEF after losing the browser controller: {error}");
            }
        })
        .detach();
    }

    /// Takes the runtime out of the controller for a borrow-free CEF call.
    /// While it is out, [`Self::runtime_phase`] reports `phase` and runtime
    /// signals are deferred until [`Self::reattach_runtime`].
    fn detach_runtime(&mut self, phase: RuntimePhase) -> Option<BrowserRuntime> {
        let runtime = self.runtime.take()?;
        self.detached_runtime_phase = Some(phase);
        Some(runtime)
    }

    fn reattach_runtime(&mut self, runtime: BrowserRuntime) {
        self.runtime = Some(runtime);
        self.detached_runtime_phase = None;
    }

    fn replay_deferred_runtime_signals(&mut self, cx: &mut Context<Self>) {
        for signal in std::mem::take(&mut self.deferred_runtime_signals) {
            self.handle_runtime_signal(signal, cx);
        }
    }

    fn runtime_unavailable_message(&self) -> Arc<str> {
        self.startup_error
            .clone()
            .unwrap_or_else(|| Arc::from(BrowserError::NotReady.to_string()))
    }

    fn schedule_pump(&mut self, delay_ms: i64, cx: &mut Context<Self>) {
        let requested = Duration::from_millis(u64::try_from(delay_ms.max(0)).unwrap_or_default());
        self.schedule_pump_after(requested, cx);
    }

    fn schedule_pump_after(&mut self, requested: Duration, cx: &mut Context<Self>) {
        if self.shutting_down
            || self.runtime.as_ref().is_none_or(|runtime| {
                matches!(
                    runtime.phase(),
                    RuntimePhase::Uninitialized | RuntimePhase::Closed | RuntimePhase::Failed
                )
            })
        {
            return;
        }
        let now = Instant::now();
        let fallback_interval = self.pump_fallback_interval();
        let mut delay = fallback_interval.map_or(requested, |interval| requested.min(interval));
        if self.external_begin_frame_enabled()
            && let Some(deadline) =
                earliest_external_begin_frame_deadline(&self.next_external_begin_frame)
        {
            delay = delay.min(deadline.saturating_duration_since(now));
        }
        let deadline = now.checked_add(delay).unwrap_or(now);
        self.pump_deadline = Some(deadline);
        self.pump_generation = self.pump_generation.wrapping_add(1).max(1);
        let generation = self.pump_generation;
        cx.spawn(async move |this, cx| {
            let timer = cx.background_executor().timer(delay);
            timer.await;
            let pump = this.update(cx, |controller, cx| {
                if controller.pump_generation != generation {
                    return None;
                }
                controller.pump_deadline = None;
                controller.send_due_external_begin_frames();
                let pump = controller
                    .runtime
                    .as_ref()
                    .map(BrowserRuntime::message_pump);
                if let Some(delay) = controller.next_pump_delay(Instant::now()) {
                    controller.schedule_pump_after(delay, cx);
                }
                pump
            });
            if let Ok(Some(pump)) = pump {
                pump.do_message_loop_work();
            }
        })
        .detach();
    }

    fn next_pump_delay(&self, now: Instant) -> Option<Duration> {
        let fallback = self.pump_fallback_interval();
        let begin_frame = self.external_begin_frame_enabled().then(|| {
            earliest_external_begin_frame_deadline(&self.next_external_begin_frame)
                .map(|deadline| deadline.saturating_duration_since(now))
        });
        match (fallback, begin_frame.flatten()) {
            (Some(fallback), Some(begin_frame)) => Some(fallback.min(begin_frame)),
            (Some(fallback), None) => Some(fallback),
            (None, Some(begin_frame)) => Some(begin_frame),
            (None, None) => None,
        }
    }

    fn pump_fallback_interval(&self) -> Option<Duration> {
        let has_active_transition = !self.pending_browsers.is_empty()
            || self.sessions.values().any(|session| {
                matches!(
                    session.phase(),
                    SessionPhase::Creating | SessionPhase::Closing
                )
            })
            || self
                .runtime
                .as_ref()
                .is_some_and(|runtime| runtime.active_data_operation_count() > 0);
        let has_visible_ready_session = self
            .sessions
            .values()
            .any(|session| session.phase() == SessionPhase::Ready && session.viewport().visible);
        let has_hot_frames = self
            .pump_hot_until
            .is_some_and(|until| until > Instant::now());
        let work = if has_active_transition {
            PumpFallbackWork::ActiveTransition
        } else if has_visible_ready_session && has_hot_frames {
            PumpFallbackWork::AnimatingSession
        } else if has_visible_ready_session {
            PumpFallbackWork::VisibleSession
        } else {
            PumpFallbackWork::Idle
        };
        PumpFallbackState {
            runtime_phase: self.runtime.as_ref().map(BrowserRuntime::phase),
            work,
            shutting_down: self.shutting_down,
            frame_interval: pump_frame_interval(self.frame_rate_ceiling()),
        }
        .interval()
    }

    /// Like `cef::initialize`, `browser_host_create_browser_sync` runs
    /// Chromium work that can service the GCD main queue, so the creation
    /// calls happen with the runtime detached, outside the App borrow. The
    /// in-flight entries stay in `pending_browsers`: an entry gone by
    /// completion means the tab closed and the fresh browser is discarded.
    fn try_create_browsers(&mut self, cx: &mut Context<Self>) {
        if self.shutting_down || self.detached_runtime_phase.is_some() {
            return;
        }
        let Some(runtime) = self.runtime.as_ref() else {
            return;
        };
        if runtime.phase() != RuntimePhase::Running {
            return;
        }
        let keys: Vec<BrowserKey> = self.pending_browsers.keys().copied().collect();
        let mut specs = Vec::new();
        for key in keys {
            if self.sessions.contains_key(&key) {
                continue;
            }
            let Some(pending) = self.pending_browsers.get(&key) else {
                continue;
            };
            let profile = pending.profile.clone();
            let url = pending.url.clone();
            let viewport = pending.viewport;
            let page_zoom_factor = pending.page_zoom_factor;
            let gpu_context = pending.gpu_context.clone();
            let allow_shared_texture = pending.allow_shared_texture;
            let has_egress = pending.egress.is_some();
            let proxy_port = pending.egress.as_ref().map(|egress| {
                debug_assert_eq!(pending.profile, egress.composite_profile);
                egress.socks_port
            });

            let context_ready = {
                let runtime = self.runtime.as_mut().expect("runtime was checked above");
                if has_egress {
                    runtime.ensure_egress_profile_context(&profile)
                } else {
                    runtime.ensure_profile_context(&profile)
                }
            };
            match context_ready {
                Ok(true) => {}
                Ok(false) => continue,
                Err(error) => {
                    self.pending_browsers.remove(&key);
                    Self::fail_pane(key.0, error.to_string(), cx);
                    continue;
                }
            }
            if let Some(port) = proxy_port
                && let Err(error) = self
                    .runtime
                    .as_mut()
                    .expect("runtime was checked above")
                    .set_profile_proxy(&profile, port)
            {
                self.pending_browsers.remove(&key);
                Self::fail_pane(key.0, error.to_string(), cx);
                continue;
            }
            let watch_first_frame = cfg!(target_os = "linux")
                && allow_shared_texture
                && self
                    .runtime
                    .as_ref()
                    .is_some_and(BrowserRuntime::shared_texture_enabled);
            self.resolve_frame_rate_ceiling(cx);
            let frame_rate_ceiling = self.frame_rate_ceiling();
            specs.push(BrowserCreateSpec {
                key,
                url,
                profile,
                viewport,
                page_zoom_factor,
                gpu_context,
                allow_shared_texture,
                frame_rate_ceiling,
                watch_first_frame,
            });
        }
        if specs.is_empty() {
            self.schedule_pump(0, cx);
            return;
        }
        let Some(mut runtime) = self.detach_runtime(RuntimePhase::Running) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let created: Vec<_> = specs
                .into_iter()
                .map(|spec| {
                    let result = runtime.create_session(
                        &spec.profile,
                        &spec.url,
                        spec.viewport,
                        spec.page_zoom_factor,
                        Some(spec.frame_rate_ceiling),
                        spec.gpu_context.clone(),
                        spec.allow_shared_texture,
                    );
                    (spec, result)
                })
                .collect();
            let mut payload = Some((runtime, created));
            let updated = this.update(cx, |controller, cx| {
                let (runtime, created) = payload
                    .take()
                    .expect("the reattach closure runs at most once");
                controller.reattach_runtime(runtime);
                for (spec, result) in created {
                    controller.finish_browser_creation(&spec, result, cx);
                }
                controller.replay_deferred_runtime_signals(cx);
                controller.try_create_browsers(cx);
                controller.schedule_pump(0, cx);
            });
            if updated.is_err()
                && let Some((mut runtime, _)) = payload.take()
                && let Err(error) = runtime.shutdown()
            {
                log::error!("failed to shut down CEF after losing the browser controller: {error}");
            }
        })
        .detach();
    }

    fn finish_browser_creation(
        &mut self,
        spec: &BrowserCreateSpec,
        result: Result<BrowserSession, BrowserError>,
        cx: &mut Context<Self>,
    ) {
        let key = spec.key;
        let mut session = match result {
            Ok(session) => session,
            Err(error) => {
                self.pending_browsers.remove(&key);
                Self::fail_pane(key.0, error.to_string(), cx);
                return;
            }
        };
        if self.shutting_down
            || self.sessions.contains_key(&key)
            || !self.pending_browsers.contains_key(&key)
        {
            session.close(true);
            return;
        }
        let pending = self
            .pending_browsers
            .remove(&key)
            .expect("presence was checked above");
        if pending.url != spec.url {
            session.navigate(&pending.url);
        }
        match pending.egress {
            Some(egress) => {
                self.browser_egress.insert(key, egress);
            }
            None => {
                self.browser_egress.remove(&key);
            }
        }
        let session_id = session.id();
        let events = session.events();
        let focused = self.focused_panes.contains(&key.0);
        session.set_focus(focused);
        session.set_frame_rate(effective_pane_frame_rate(
            spec.frame_rate_ceiling,
            focused,
            self.wheel_decay_generations.contains_key(&key),
        ));
        self.sessions.insert(key, session);
        if spec.watch_first_frame {
            self.first_frame_watchdogs
                .insert(key, FirstFrameWatchdog::new(session_id));
        }
        self.mark_browser_activity(key);
        cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv().await {
                if this
                    .update(cx, |controller, cx| {
                        controller.handle_browser_event(event, cx);
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    fn handle_browser_event(&mut self, event: BrowserEvent, cx: &mut Context<Self>) {
        let started = diagnostics::timer(DIAGNOSTIC_TARGET);
        log::trace!(
            target: "zz::diagnostics::browser",
            "handle_event begin event={event:#?}"
        );
        let Some(key) = self
            .sessions
            .iter()
            .find_map(|(key, session)| (event.session() == session.id()).then_some(*key))
        else {
            return;
        };
        let (pane, tab) = key;
        let active = self.active_tab(pane) == Some(tab);
        let frame_ready = matches!(&event, BrowserEvent::FrameReady { .. });
        if frame_ready {
            self.first_frame_watchdogs.remove(&key);
        }
        if active && frame_ready {
            let was_hot = self.pump_is_hot();
            self.mark_browser_activity(key);
            self.record_external_begin_frame_delivery(key);
            self.kick_pump_if_cold(was_hot, cx);
        }

        let mut closed = false;
        let mut recreate = false;
        let mut arm_first_frame_watchdog = false;
        let mut readback_reason = None;
        {
            let session = self.sessions.get_mut(&key).expect("matched session exists");
            match &event {
                BrowserEvent::Created { .. } => log::debug!("CEF browser session created"),
                BrowserEvent::LoadingChanged { loading, .. } => {
                    log::debug!("CEF browser loading state changed: {loading}");
                }
                BrowserEvent::FrameReady { generation, .. } => {
                    log::trace!("CEF OSR frame ready: generation {generation}");
                }
                BrowserEvent::LoadFailed {
                    code, description, ..
                } => {
                    log::debug!("CEF browser load failed ({code}): {description}");
                }
                BrowserEvent::TitleChanged { .. }
                | BrowserEvent::AddressChanged { .. }
                | BrowserEvent::CursorChanged { .. }
                | BrowserEvent::ElementPicked { .. }
                | BrowserEvent::ElementPickCancelled { .. }
                | BrowserEvent::ElementPickFailed { .. }
                | BrowserEvent::ContextMenuRequested { .. }
                | BrowserEvent::PopupRequested { .. }
                | BrowserEvent::SharedTextureFailed { .. }
                | BrowserEvent::RenderProcessTerminated { .. }
                | BrowserEvent::Closed { .. } => {}
            }

            match &event {
                BrowserEvent::Created { .. } => {
                    session.mark_ready();
                    arm_first_frame_watchdog = true;
                }
                BrowserEvent::SharedTextureFailed { reason, .. } => {
                    readback_reason = Some(Arc::clone(reason));
                }
                BrowserEvent::FrameReady { .. } => {
                    if let Some(frame) = session.take_frame() {
                        if active {
                            match BrowserPaneFrame::from_frame(frame) {
                                Some(frame) => {
                                    if let Some(previous) = self.latest_frames.insert(key, frame)
                                        && let BrowserPaneFrameContent::OwnedBgra(image) =
                                            previous.content
                                        && let Ok(image) = Arc::try_unwrap(image)
                                    {
                                        for frame in image.into_frames() {
                                            session.recycle_frame(frame.into_buffer().into_raw());
                                        }
                                    }
                                }
                                None => log::error!("CEF produced an invalid browser frame"),
                            }
                        } else if let OsrFrame::OwnedBgra(frame) = frame {
                            session.recycle_frame(frame.bgra);
                        }
                    }
                }
                BrowserEvent::ElementPicked { .. } => session.finish_element_capture(),
                BrowserEvent::RenderProcessTerminated { .. } => session.mark_crashed(),
                BrowserEvent::Closed { .. } => {
                    session.mark_closed();
                    closed = true;
                    recreate = self.recreate_after_close.remove(&key);
                }
                _ => {}
            }
        }
        if arm_first_frame_watchdog {
            self.arm_first_frame_watchdog(key, cx);
        }
        if let Some(reason) = readback_reason {
            self.request_readback_fallback(key, &reason);
        }
        if closed {
            self.first_frame_watchdogs.remove(&key);
        }
        if closed && !self.shutting_down {
            self.sessions.remove(&key);
            self.wheel_decay_generations.remove(&key);
            self.external_begin_frame_hot_until.remove(&key);
            self.next_external_begin_frame.remove(&key);
            self.adaptive_begin_frame_throttles.remove(&key);
            if recreate {
                self.try_create_browsers(cx);
            }
        }
        cx.emit(ControllerEvent::Browser { pane, tab, event });
        log::trace!(
            target: "zz::diagnostics::browser",
            "handle_event end pane={pane} tab={} sessions={} latest_frames={} elapsed_us={}",
            tab.0,
            self.sessions.len(),
            self.latest_frames.len(),
            diagnostics::elapsed_us(started),
        );
    }

    fn fail(&mut self, error: String, cx: &mut Context<Self>) {
        self.fail_pending_browsers(&error, cx);
        let error: Arc<str> = Arc::from(error);
        self.startup_error = Some(error.clone());
        cx.emit(ControllerEvent::Failed(error));
    }

    fn fail_pending_browsers(&mut self, error: &str, cx: &mut Context<Self>) {
        let pending = std::mem::take(&mut self.pending_browsers);
        for (pane, _) in pending.keys() {
            Self::fail_pane(*pane, error.to_owned(), cx);
        }
    }

    fn fail_pane(pane: PaneId, error: String, cx: &mut Context<Self>) {
        cx.emit(ControllerEvent::BrowserFailed {
            pane,
            message: Arc::from(error),
        });
    }

    fn shutdown_runtime(&mut self) {
        if let Some(runtime) = self.runtime.as_mut()
            && let Err(error) = runtime.shutdown()
        {
            log::error!("failed to shut down CEF: {error}");
        }
    }
}

impl EventEmitter<ControllerEvent> for BrowserController {}

enum ShutdownProgress {
    Waiting,
    Complete,
    TimedOut,
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use gpui::{AppContext as _, TestAppContext};

    use super::*;

    #[gpui::test(iterations = 20)]
    fn cef_work_runs_after_the_app_update_in_submission_order(cx: &mut TestAppContext) {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let app = Rc::clone(&cx.app);
        let controller = cx.update(|cx| {
            cx.new(|cx| BrowserController::new(Err(BrowserError::AlreadyShutdown), cx))
        });

        cx.update(|cx| {
            assert!(app.try_borrow_mut().is_err());
            let held = calls.borrow_mut();
            for call in [1, 2] {
                let calls = Rc::clone(&calls);
                let app = Rc::clone(&app);
                controller.update(cx, |controller, cx| {
                    controller.queue_cef_work(
                        move || {
                            assert!(app.try_borrow_mut().is_ok());
                            calls.borrow_mut().push(call);
                        },
                        cx,
                    );
                });
            }
            assert!(held.is_empty());
        });

        cx.run_until_parked();
        assert_eq!(*calls.borrow(), vec![1, 2]);
    }

    #[test]
    fn display_frame_rate_ceiling_uses_fastest_reported_rate_and_rounds() {
        assert_eq!(
            select_display_frame_rate_ceiling(vec![None, Some(59.94), Some(143.856)]),
            144
        );
    }

    #[test]
    fn display_frame_rate_ceiling_falls_back_without_reported_rates() {
        assert_eq!(
            select_display_frame_rate_ceiling(vec![None, None]),
            DEFAULT_DISPLAY_FRAME_RATE_CEILING
        );
        assert_eq!(
            select_display_frame_rate_ceiling(Vec::new()),
            DEFAULT_DISPLAY_FRAME_RATE_CEILING
        );
    }

    #[test]
    fn display_frame_rate_ceiling_clamps_to_cef_bounds() {
        assert_eq!(select_display_frame_rate_ceiling(vec![Some(0.4)]), 1);
        assert_eq!(
            select_display_frame_rate_ceiling(vec![Some(360.0)]),
            MAX_DISPLAY_FRAME_RATE_CEILING
        );
    }

    #[test]
    fn pane_frame_rate_caps_unfocused_animation_unless_wheel_boosted() {
        assert_eq!(effective_pane_frame_rate(120, true, false), 120);
        assert_eq!(effective_pane_frame_rate(120, false, true), 120);
        assert_eq!(effective_pane_frame_rate(120, false, false), 30);
        assert_eq!(effective_pane_frame_rate(24, false, false), 24);
    }

    #[test]
    fn first_frame_watchdog_pauses_and_rearms_without_stale_timeouts() {
        let session = SessionId(17);
        let mut watchdog = FirstFrameWatchdog::new(session);

        let first_generation = watchdog.arm().expect("first arm starts a timer");
        assert!(watchdog.matches(session, first_generation));
        assert_eq!(watchdog.arm(), None);

        watchdog.pause();
        assert!(!watchdog.matches(session, first_generation));
        let second_generation = watchdog.arm().expect("resume starts a new timer");
        assert_ne!(second_generation, first_generation);
        assert!(watchdog.matches(session, second_generation));
        assert!(!watchdog.matches(SessionId(18), second_generation));
    }

    #[test]
    fn display_frame_rate_ceiling_retries_after_empty_probe() {
        for unavailable_rates in [Vec::new(), vec![None, None]] {
            let first_probe = reported_display_frame_rate_ceiling(unavailable_rates);
            let (fallback, cached) = resolve_frame_rate_ceiling_value(None, first_probe);
            assert_eq!(fallback, DEFAULT_DISPLAY_FRAME_RATE_CEILING);
            assert_eq!(cached, None);

            let retry_probe = reported_display_frame_rate_ceiling(vec![Some(239.914)]);
            let (resolved, cached) = resolve_frame_rate_ceiling_value(cached, retry_probe);
            assert_eq!(resolved, MAX_DISPLAY_FRAME_RATE_CEILING);
            assert_eq!(cached, Some(MAX_DISPLAY_FRAME_RATE_CEILING));
        }
    }

    fn record_adaptive_window(
        throttle: &mut AdaptiveBeginFrameThrottle,
        begin_frames_sent: u64,
        frames_delivered: u64,
    ) {
        for _ in 0..begin_frames_sent {
            throttle.record_begin_frame();
        }
        for _ in 0..frames_delivered {
            throttle.record_frame_delivered();
        }
    }

    fn test_instant_after(started_at: Instant, elapsed: Duration) -> Instant {
        started_at
            .checked_add(elapsed)
            .expect("test instant is representable")
    }

    const fn test_browser_key(pane: u64, tab: u64) -> BrowserKey {
        (PaneId(pane), TabId(tab))
    }

    fn pending_browser(viewport: Viewport) -> PendingBrowser {
        PendingBrowser {
            url: "about:blank".to_owned(),
            profile: "default".to_owned(),
            egress: None,
            viewport,
            page_zoom_factor: 1.0,
            gpu_context: None,
            allow_shared_texture: true,
        }
    }

    fn test_egress_spec(socks_port: u16) -> EgressSpec {
        EgressSpec {
            composite_profile: "default@egress-0badc0de".to_owned(),
            egress_host: "build.internal".to_owned(),
            socks_port,
        }
    }

    #[gpui::test]
    fn browser_egress_records_the_route_until_its_tabs_close(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let controller =
                cx.new(|cx| BrowserController::new(Err(BrowserError::AlreadyShutdown), cx));
            let pane = PaneId(1);
            let other_pane = PaneId(2);
            let first = TabId(1);
            let second = TabId(2);
            let pending = TabId(3);

            controller.update(cx, |controller, _| {
                for key in [(pane, first), (pane, second), (other_pane, first)] {
                    controller
                        .browser_egress
                        .insert(key, test_egress_spec(41_080));
                }
                controller.pending_browsers.insert(
                    (pane, pending),
                    PendingBrowser {
                        egress: Some(test_egress_spec(41_080)),
                        ..pending_browser(Viewport::default())
                    },
                );

                controller.refresh_egress(pane, Some(test_egress_spec(41_081)));
                assert_eq!(
                    controller.pending_browsers[&(pane, pending)]
                        .egress
                        .as_ref()
                        .expect("the pending request kept its route")
                        .socks_port,
                    41_081,
                );
                assert_eq!(controller.browser_egress[&(pane, first)].socks_port, 41_081);
                assert_eq!(
                    controller.browser_egress[&(pane, second)].socks_port,
                    41_081
                );
                assert_eq!(
                    controller.browser_egress[&(other_pane, first)].socks_port,
                    41_080,
                );

                controller.refresh_egress(pane, None);
                controller.refresh_egress(
                    pane,
                    Some(EgressSpec {
                        composite_profile: "other@egress-0badc0de".to_owned(),
                        ..test_egress_spec(41_082)
                    }),
                );
                assert_eq!(controller.browser_egress[&(pane, first)].socks_port, 41_081);

                controller.close_tab(pane, second);
                assert!(!controller.browser_egress.contains_key(&(pane, second)));
                assert!(controller.browser_egress.contains_key(&(pane, first)));

                controller.close_pane(pane);
                assert!(
                    controller
                        .browser_egress
                        .range(browser_key_range(pane))
                        .next()
                        .is_none()
                );
                assert!(controller.browser_egress.contains_key(&(other_pane, first)));
            });
        });
    }

    #[gpui::test]
    fn active_tab_flips_pending_session_visibility(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let controller =
                cx.new(|cx| BrowserController::new(Err(BrowserError::AlreadyShutdown), cx));
            let pane = PaneId(7);
            let first = TabId(3);
            let second = TabId(9);
            let visible = Viewport {
                width: 800,
                height: 500,
                visible: true,
                ..Viewport::default()
            };

            controller.update(cx, |controller, cx| {
                controller.active_tabs.insert(pane, first);
                controller.pane_viewports.insert(pane, visible);
                controller
                    .pending_browsers
                    .insert((pane, first), pending_browser(visible));
                controller.pending_browsers.insert(
                    (pane, second),
                    pending_browser(Viewport {
                        visible: false,
                        ..visible
                    }),
                );

                controller.set_active_tab(pane, second, cx);

                assert_eq!(controller.active_tab(pane), Some(second));
                assert!(!controller.pending_browsers[&(pane, first)].viewport.visible);
                assert!(
                    controller.pending_browsers[&(pane, second)]
                        .viewport
                        .visible
                );
            });
        });
    }

    #[gpui::test]
    fn close_tab_and_close_pane_keep_other_tab_keys_isolated(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let controller =
                cx.new(|cx| BrowserController::new(Err(BrowserError::AlreadyShutdown), cx));
            let pane = PaneId(7);
            let other_pane = PaneId(8);
            let first = TabId(1);
            let active = TabId(2);
            let orphaned_bookkeeping = TabId(3);
            let other = TabId(1);
            let viewport = Viewport::default();

            controller.update(cx, |controller, _| {
                for key in [(pane, first), (pane, active), (other_pane, other)] {
                    controller
                        .pending_browsers
                        .insert(key, pending_browser(viewport));
                    controller.forced_readback.insert(key);
                }
                controller.next_external_begin_frame.insert(
                    (pane, orphaned_bookkeeping),
                    ExternalBeginFrameDeadline::anchored(
                        Instant::now(),
                        Duration::from_millis(10),
                        false,
                    ),
                );
                controller.first_frame_watchdogs.insert(
                    (pane, orphaned_bookkeeping),
                    FirstFrameWatchdog::new(SessionId(41)),
                );
                controller.active_tabs.insert(pane, active);
                controller.active_tabs.insert(other_pane, other);
                controller.pane_viewports.insert(pane, viewport);
                controller.pane_viewports.insert(other_pane, viewport);

                assert_eq!(
                    controller
                        .pending_browsers
                        .range(browser_key_range(pane))
                        .count(),
                    2,
                );

                controller.close_tab(pane, first);
                assert!(!controller.pending_browsers.contains_key(&(pane, first)));
                assert!(!controller.forced_readback.contains(&(pane, first)));
                assert!(controller.pending_browsers.contains_key(&(pane, active)));
                assert!(
                    controller
                        .pending_browsers
                        .contains_key(&(other_pane, other))
                );
                assert_eq!(controller.active_tab(pane), Some(active));

                controller.close_pane(pane);
                assert!(
                    controller
                        .pending_browsers
                        .range(browser_key_range(pane))
                        .next()
                        .is_none()
                );
                assert!(
                    controller
                        .forced_readback
                        .range(browser_key_range(pane))
                        .next()
                        .is_none()
                );
                assert!(
                    controller
                        .next_external_begin_frame
                        .range(browser_key_range(pane))
                        .next()
                        .is_none()
                );
                assert!(
                    controller
                        .first_frame_watchdogs
                        .range(browser_key_range(pane))
                        .next()
                        .is_none()
                );
                assert_eq!(controller.active_tab(pane), None);
                assert!(!controller.pane_viewports.contains_key(&pane));

                assert!(
                    controller
                        .pending_browsers
                        .contains_key(&(other_pane, other))
                );
                assert!(controller.forced_readback.contains(&(other_pane, other)));
                assert_eq!(controller.active_tab(other_pane), Some(other));
                assert!(controller.pane_viewports.contains_key(&other_pane));
            });
        });
    }

    #[test]
    fn adaptive_begin_frame_throttle_downshifts_after_sustained_shortfall() {
        let started_at = Instant::now();
        let mut throttle = AdaptiveBeginFrameThrottle::default();
        throttle.set_hot(true, started_at);
        record_adaptive_window(&mut throttle, 100, 80);

        assert!(throttle.update_tier(
            test_instant_after(started_at, ADAPTIVE_BEGIN_FRAME_SAMPLE_WINDOW),
            240,
        ));
        assert_eq!(throttle.divisor, 2);
        assert_eq!(throttle.effective_frame_rate(240), 120);
    }

    #[test]
    fn adaptive_begin_frame_throttle_does_not_flap_on_one_slow_frame() {
        let started_at = Instant::now();
        let mut throttle = AdaptiveBeginFrameThrottle::default();
        throttle.set_hot(true, started_at);
        record_adaptive_window(&mut throttle, 120, 119);

        assert!(!throttle.update_tier(
            test_instant_after(started_at, ADAPTIVE_BEGIN_FRAME_SAMPLE_WINDOW),
            240,
        ));
        assert_eq!(throttle.divisor, 1);
    }

    #[test]
    fn adaptive_begin_frame_throttle_probes_faster_after_stability() {
        let started_at = Instant::now();
        let mut throttle = AdaptiveBeginFrameThrottle::default();
        throttle.set_hot(true, started_at);
        record_adaptive_window(&mut throttle, 100, 80);
        let downshifted_at = test_instant_after(started_at, ADAPTIVE_BEGIN_FRAME_SAMPLE_WINDOW);
        assert!(throttle.update_tier(downshifted_at, 240));
        assert_eq!(throttle.divisor, 2);

        record_adaptive_window(&mut throttle, 120, 120);
        let first_healthy_window =
            test_instant_after(downshifted_at, ADAPTIVE_BEGIN_FRAME_SAMPLE_WINDOW);
        assert!(!throttle.update_tier(first_healthy_window, 240));
        assert_eq!(throttle.divisor, 2);

        record_adaptive_window(&mut throttle, 120, 114);
        let second_healthy_window =
            test_instant_after(first_healthy_window, ADAPTIVE_BEGIN_FRAME_SAMPLE_WINDOW);
        assert!(throttle.update_tier(second_healthy_window, 240));
        assert_eq!(throttle.divisor, 1);
    }

    #[test]
    fn adaptive_begin_frame_throttle_respects_rate_bounds() {
        let started_at = Instant::now();
        let mut throttle = AdaptiveBeginFrameThrottle::default();
        throttle.set_hot(true, started_at);
        let mut sampled_at = started_at;
        for expected_divisor in [2, 4, 8] {
            record_adaptive_window(&mut throttle, 100, 50);
            sampled_at = test_instant_after(sampled_at, ADAPTIVE_BEGIN_FRAME_SAMPLE_WINDOW);
            assert!(throttle.update_tier(sampled_at, 240));
            assert_eq!(throttle.divisor, expected_divisor);
        }
        assert_eq!(throttle.effective_frame_rate(240), 30);
        record_adaptive_window(&mut throttle, 100, 50);
        sampled_at = test_instant_after(sampled_at, ADAPTIVE_BEGIN_FRAME_SAMPLE_WINDOW);
        assert!(!throttle.update_tier(sampled_at, 240));
        assert_eq!(throttle.divisor, 8);

        let mut low_ceiling = AdaptiveBeginFrameThrottle::default();
        low_ceiling.set_hot(true, started_at);
        record_adaptive_window(&mut low_ceiling, 24, 10);
        assert!(!low_ceiling.update_tier(
            test_instant_after(started_at, ADAPTIVE_BEGIN_FRAME_SAMPLE_WINDOW),
            24,
        ));
        assert_eq!(low_ceiling.effective_frame_rate(24), 24);
    }

    #[test]
    fn adaptive_begin_frame_throttle_ignores_idle_windows() {
        let started_at = Instant::now();
        let mut throttle = AdaptiveBeginFrameThrottle::default();
        throttle.set_hot(true, started_at);
        record_adaptive_window(&mut throttle, 200, 0);

        assert!(!throttle.update_tier(
            test_instant_after(started_at, ADAPTIVE_BEGIN_FRAME_SAMPLE_WINDOW),
            240,
        ));
        assert_eq!(throttle.divisor, 1);
    }

    #[test]
    fn adaptive_begin_frame_throttle_resets_divisor_when_cold() {
        let started_at = Instant::now();
        let mut throttle = AdaptiveBeginFrameThrottle::default();
        throttle.set_hot(true, started_at);
        record_adaptive_window(&mut throttle, 100, 50);
        let downshifted_at = test_instant_after(started_at, ADAPTIVE_BEGIN_FRAME_SAMPLE_WINDOW);
        assert!(throttle.update_tier(downshifted_at, 240));
        assert_eq!(throttle.divisor, 2);

        throttle.set_hot(false, downshifted_at);
        throttle.set_hot(true, downshifted_at);
        assert_eq!(throttle.divisor, 1);
        assert_eq!(throttle.effective_frame_rate(240), 240);
    }

    #[test]
    fn begin_frame_deadline_skips_missed_intervals_without_bursting() {
        let started_at = Instant::now();
        let interval = Duration::from_millis(10);
        let mut deadlines = BTreeMap::new();

        assert!(external_begin_frame_due(
            &mut deadlines,
            test_browser_key(1, 0),
            started_at,
            interval,
            true,
        ));
        let late_turn = started_at
            .checked_add(Duration::from_millis(95))
            .expect("test instant is representable");
        assert!(external_begin_frame_due(
            &mut deadlines,
            test_browser_key(1, 0),
            late_turn,
            interval,
            true,
        ));
        assert_eq!(
            deadlines[&test_browser_key(1, 0)].next,
            started_at
                .checked_add(Duration::from_millis(100))
                .expect("test instant is representable")
        );
        assert!(!external_begin_frame_due(
            &mut deadlines,
            test_browser_key(1, 0),
            late_turn,
            interval,
            true,
        ));
    }

    #[test]
    fn begin_frame_deadline_sends_and_anchors_on_hot_edges() {
        let started_at = Instant::now();
        let cold_interval = VISIBLE_PUMP_WATCHDOG_INTERVAL;
        let hot_interval = pump_frame_interval(240);
        let mut deadlines = BTreeMap::new();

        assert!(external_begin_frame_due(
            &mut deadlines,
            test_browser_key(1, 0),
            started_at,
            cold_interval,
            false,
        ));
        let hot_at = started_at
            .checked_add(Duration::from_millis(5))
            .expect("test instant is representable");
        assert!(external_begin_frame_due(
            &mut deadlines,
            test_browser_key(1, 0),
            hot_at,
            hot_interval,
            true,
        ));
        assert_eq!(
            deadlines[&test_browser_key(1, 0)].next,
            hot_at
                .checked_add(hot_interval)
                .expect("test instant is representable")
        );
    }

    #[test]
    fn begin_frame_deadline_reanchors_hot_cold_and_ceiling_changes() {
        let started_at = Instant::now();
        let hot_interval = pump_frame_interval(240);
        let slower_hot_interval = pump_frame_interval(120);
        let mut deadlines = BTreeMap::new();

        assert!(external_begin_frame_due(
            &mut deadlines,
            test_browser_key(1, 0),
            started_at,
            hot_interval,
            true,
        ));
        let ceiling_changed_at = started_at
            .checked_add(Duration::from_millis(1))
            .expect("test instant is representable");
        assert!(!external_begin_frame_due(
            &mut deadlines,
            test_browser_key(1, 0),
            ceiling_changed_at,
            slower_hot_interval,
            true,
        ));
        assert_eq!(
            deadlines[&test_browser_key(1, 0)].next,
            ceiling_changed_at
                .checked_add(slower_hot_interval)
                .expect("test instant is representable")
        );

        let cold_at = ceiling_changed_at
            .checked_add(Duration::from_millis(1))
            .expect("test instant is representable");
        assert!(!external_begin_frame_due(
            &mut deadlines,
            test_browser_key(1, 0),
            cold_at,
            VISIBLE_PUMP_WATCHDOG_INTERVAL,
            false,
        ));
        assert_eq!(
            deadlines[&test_browser_key(1, 0)].next,
            cold_at
                .checked_add(VISIBLE_PUMP_WATCHDOG_INTERVAL)
                .expect("test instant is representable")
        );

        let hot_again_at = cold_at
            .checked_add(Duration::from_millis(1))
            .expect("test instant is representable");
        assert!(external_begin_frame_due(
            &mut deadlines,
            test_browser_key(1, 0),
            hot_again_at,
            slower_hot_interval,
            true,
        ));
    }

    #[test]
    fn begin_frame_deadline_selects_earliest_browser_tab() {
        let started_at = Instant::now();
        let mut deadlines = BTreeMap::new();
        let later = started_at
            .checked_add(Duration::from_millis(12))
            .expect("test instant is representable");
        let earlier = started_at
            .checked_add(Duration::from_millis(4))
            .expect("test instant is representable");
        deadlines.insert(
            test_browser_key(1, 0),
            ExternalBeginFrameDeadline {
                next: later,
                interval: Duration::from_millis(4),
                hot: true,
            },
        );
        deadlines.insert(
            test_browser_key(1, 1),
            ExternalBeginFrameDeadline {
                next: earlier,
                interval: Duration::from_millis(8),
                hot: true,
            },
        );

        assert_eq!(
            earliest_external_begin_frame_deadline(&deadlines),
            Some(earlier)
        );
    }

    #[test]
    fn begin_frame_deadline_preserves_exact_cadence_across_late_turns() {
        let started_at = Instant::now();
        let interval = Duration::from_millis(10);
        let mut deadlines = BTreeMap::new();
        assert!(external_begin_frame_due(
            &mut deadlines,
            test_browser_key(1, 0),
            started_at,
            interval,
            true,
        ));

        for (late_ms, next_ms) in [(12, 20), (23, 30), (31, 40), (44, 50)] {
            let late_turn = started_at
                .checked_add(Duration::from_millis(late_ms))
                .expect("test instant is representable");
            assert!(external_begin_frame_due(
                &mut deadlines,
                test_browser_key(1, 0),
                late_turn,
                interval,
                true,
            ));
            assert_eq!(
                deadlines[&test_browser_key(1, 0)].next,
                started_at
                    .checked_add(Duration::from_millis(next_ms))
                    .expect("test instant is representable")
            );
        }
    }

    #[test]
    fn pump_fallback_policy_keeps_only_live_browser_work_active() {
        assert_eq!(
            PumpFallbackState {
                runtime_phase: Some(RuntimePhase::Initializing),
                ..PumpFallbackState::default()
            }
            .interval(),
            Some(ACTIVE_PUMP_INTERVAL)
        );

        assert_eq!(
            PumpFallbackState {
                runtime_phase: Some(RuntimePhase::Running),
                work: PumpFallbackWork::ActiveTransition,
                ..PumpFallbackState::default()
            }
            .interval(),
            Some(ACTIVE_PUMP_INTERVAL)
        );

        assert_eq!(
            PumpFallbackState {
                runtime_phase: Some(RuntimePhase::Running),
                work: PumpFallbackWork::VisibleSession,
                ..PumpFallbackState::default()
            }
            .interval(),
            Some(VISIBLE_PUMP_WATCHDOG_INTERVAL)
        );
    }

    #[test]
    fn pump_fallback_policy_paces_animating_sessions_at_the_frame_interval() {
        assert_eq!(
            PumpFallbackState {
                runtime_phase: Some(RuntimePhase::Running),
                work: PumpFallbackWork::AnimatingSession,
                frame_interval: pump_frame_interval(240),
                ..PumpFallbackState::default()
            }
            .interval(),
            Some(Duration::from_micros(4166))
        );
        assert_eq!(
            PumpFallbackState {
                runtime_phase: Some(RuntimePhase::Running),
                work: PumpFallbackWork::AnimatingSession,
                frame_interval: pump_frame_interval(24),
                ..PumpFallbackState::default()
            }
            .interval(),
            Some(VISIBLE_PUMP_WATCHDOG_INTERVAL)
        );
    }

    #[test]
    fn pump_fallback_policy_leaves_inactive_states_lazy() {
        for state in [
            PumpFallbackState::default(),
            PumpFallbackState {
                runtime_phase: Some(RuntimePhase::Running),
                ..PumpFallbackState::default()
            },
            PumpFallbackState {
                runtime_phase: Some(RuntimePhase::Closed),
                work: PumpFallbackWork::VisibleSession,
                ..PumpFallbackState::default()
            },
            PumpFallbackState {
                runtime_phase: Some(RuntimePhase::Running),
                work: PumpFallbackWork::VisibleSession,
                shutting_down: true,
                ..PumpFallbackState::default()
            },
        ] {
            assert_eq!(state.interval(), None);
        }
    }
}
