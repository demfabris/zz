use std::{
    cell::Cell,
    collections::BTreeMap,
    ffi::OsStr,
    path::{Path, PathBuf},
    ptr,
    rc::Rc,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering},
    },
    time::Instant,
};

use async_channel::{Receiver, Sender};
use base64::Engine as _;
use cef::wrapper::message_router::{
    BrowserSideCallback, BrowserSideHandler, BrowserSideRouter, MessageRouterBrowserSide,
    MessageRouterBrowserSideHandlerCallbacks, MessageRouterConfig, MessageRouterRendererSide,
    MessageRouterRendererSideHandlerCallbacks, RendererSideRouter,
};
use cef::{args::Args, *};
use parking_lot::Mutex;
use serde::Deserialize;
use thiserror::Error;
use url::Url;

#[cfg(target_os = "macos")]
#[path = "mac_app_protocol.rs"]
mod mac_app_protocol;
#[cfg(target_os = "macos")]
#[path = "metal_osr.rs"]
mod metal_osr;
#[cfg(target_os = "macos")]
use metal_osr::{MetalFrameCompletion, MetalFrameError, MetalFrameOutcome, MetalFrameProducer};

#[cfg(target_os = "windows")]
#[path = "d3d11_osr.rs"]
mod d3d11_osr;
#[cfg(target_os = "windows")]
use d3d11_osr::{D3d11FrameError, D3d11FrameOutcome, D3d11FrameProducer};

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use crate::frame::GpuFrameSubmission;
#[cfg(target_os = "macos")]
use crate::frame::MacGpuFrameSubmission;
#[cfg(target_os = "windows")]
use crate::frame::WinGpuFrameSubmission;
#[cfg(test)]
use crate::page_zoom::CHROMIUM_ZOOM_STEP;
use crate::{
    BrowserCookie, BrowserCookiePriority, BrowserCookieSameSite, BrowserCursor, BrowserEvent,
    BrowserGpuContext, BrowserKey, BrowserProfileError, BrowserProfilePaths, ContextMenuRequest,
    CookieImportBatch, CookieImportResult, DEFAULT_BROWSER_PROFILE, EditCommand, EditFlags,
    FrameMailbox, KeyAction, KeyInput, Modifiers, OsrFrame, PointerButton, PointerEvent,
    PointerPhase, RuntimePhase, SessionId, SessionPhase, SiteDataClearResult, Viewport, WheelEvent,
    element_picker::{
        ElementPickOutcome, ElementPickState, PickGeometry, element_picker_start_script,
    },
    frame::{FrameDamage, frame_byte_len},
    page_zoom::{
        chromium_zoom_level, next_page_zoom_factor, page_zoom_percent, sanitized_page_zoom_factor,
    },
    resolve_profile_paths,
};

#[cfg(target_os = "macos")]
type CursorHandle = *mut u8;

const DEFAULT_BROWSER_FRAME_RATE: i32 = 60;
const MAX_BROWSER_FRAME_RATE: i32 = 240;
const ELEMENT_PICKER_QUERY_FUNCTION: &str = "__zzElementPickerQuery";
const ELEMENT_PICKER_CANCEL_FUNCTION: &str = "__zzElementPickerQueryCancel";
const ELEMENT_PICKER_SCRIPT_URL: &str = "zz://browser/element-picker.js";
const ELEMENT_PICKER_SCRIPT: &str = include_str!("../assets/element-picker.js");
const WINDOWS_EPOCH_UNIX_OFFSET_MICROS: i64 = 11_644_473_600_000_000;

fn element_picker_router_config() -> MessageRouterConfig {
    MessageRouterConfig {
        js_query_function: ELEMENT_PICKER_QUERY_FUNCTION.to_owned(),
        js_cancel_function: ELEMENT_PICKER_CANCEL_FUNCTION.to_owned(),
        ..MessageRouterConfig::default()
    }
}

fn diagnostic_timer() -> Option<Instant> {
    log::log_enabled!(target: "zz_browser::diagnostics", log::Level::Trace).then(Instant::now)
}

fn diagnostic_elapsed_us(started: Option<Instant>) -> u128 {
    started.map_or(0, |started| started.elapsed().as_micros())
}

fn popup_opens_in_foreground(disposition: WindowOpenDisposition) -> bool {
    disposition != WindowOpenDisposition::NEW_BACKGROUND_TAB
}

#[must_use]
fn uses_wayland_physical_osr() -> bool {
    cfg!(target_os = "linux") && std::env::var_os("WAYLAND_DISPLAY").is_some()
}

#[must_use]
fn osr_raster_scale(viewport: Viewport) -> f32 {
    if uses_wayland_physical_osr() {
        viewport.scale_factor
    } else {
        1.0
    }
}

fn dirty_rect_damage(dirty_rects: Option<&[Rect]>) -> Option<FrameDamage> {
    let mut dirty_rects = dirty_rects?.iter();
    let first = cef_rect_damage(dirty_rects.next()?)?;
    dirty_rects.try_fold(first, |bounds, rect| {
        let rect = cef_rect_damage(rect)?;
        let left = bounds.x.min(rect.x);
        let top = bounds.y.min(rect.y);
        let right = bounds
            .x
            .saturating_add(bounds.width)
            .max(rect.x.saturating_add(rect.width));
        let bottom = bounds
            .y
            .saturating_add(bounds.height)
            .max(rect.y.saturating_add(rect.height));
        Some(FrameDamage {
            x: left,
            y: top,
            width: right.saturating_sub(left),
            height: bottom.saturating_sub(top),
        })
    })
}

fn cef_rect_damage(rect: &Rect) -> Option<FrameDamage> {
    let width = u32::try_from(rect.width).ok().filter(|width| *width > 0)?;
    let height = u32::try_from(rect.height)
        .ok()
        .filter(|height| *height > 0)?;
    Some(FrameDamage {
        x: u32::try_from(rect.x).ok()?,
        y: u32::try_from(rect.y).ok()?,
        width,
        height,
    })
}

fn effective_chromium_zoom_level(viewport: Viewport, page_zoom_factor: f64) -> f64 {
    let raster_factor = if uses_wayland_physical_osr() {
        f64::from(viewport.scale_factor)
    } else {
        1.0
    };
    chromium_zoom_level(raster_factor * page_zoom_factor)
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "CEF dimensions are saturated to its signed integer range"
)]
#[must_use]
fn scaled_osr_dimension(value: u32, scale_factor: f32) -> i32 {
    (f64::from(value) * f64::from(scale_factor))
        .ceil()
        .clamp(1.0, f64::from(i32::MAX)) as i32
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "CEF coordinates are saturated to its signed integer range"
)]
#[must_use]
fn scaled_osr_coordinate(value: i32, scale_factor: f32) -> i32 {
    (f64::from(value) * f64::from(scale_factor))
        .round()
        .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

const ELEMENT_SCREENSHOT_MARGIN: f64 = 64.0;

#[derive(Clone, Copy, Debug, PartialEq)]
struct ScreenshotClip {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

fn element_screenshot_clip(geometry: PickGeometry) -> Option<ScreenshotClip> {
    let page_left = geometry.x + geometry.scroll_x;
    let page_top = geometry.y + geometry.scroll_y;
    let view_left = geometry.scroll_x;
    let view_top = geometry.scroll_y;
    let view_right = view_left + geometry.viewport_width;
    let view_bottom = view_top + geometry.viewport_height;

    let left = (page_left - ELEMENT_SCREENSHOT_MARGIN).clamp(view_left, view_right);
    let top = (page_top - ELEMENT_SCREENSHOT_MARGIN).clamp(view_top, view_bottom);
    let right =
        (page_left + geometry.width + ELEMENT_SCREENSHOT_MARGIN).clamp(view_left, view_right);
    let bottom =
        (page_top + geometry.height + ELEMENT_SCREENSHOT_MARGIN).clamp(view_top, view_bottom);

    let width = right - left;
    let height = bottom - top;
    (width >= 1.0 && height >= 1.0).then_some(ScreenshotClip {
        x: left,
        y: top,
        width,
        height,
    })
}

fn screenshot_params(clip: ScreenshotClip) -> Option<DictionaryValue> {
    let params = dictionary_value_create()?;
    let mut region = dictionary_value_create()?;
    let submitted = region.set_double(Some(&CefString::from("x")), clip.x) != 0
        && region.set_double(Some(&CefString::from("y")), clip.y) != 0
        && region.set_double(Some(&CefString::from("width")), clip.width) != 0
        && region.set_double(Some(&CefString::from("height")), clip.height) != 0
        // CEF's OSR view ignores `scale` (`SetSize` is a no-op, CEF #3103) and tiles above 1.0.
        && region.set_double(Some(&CefString::from("scale")), 1.0) != 0
        && params.set_string(
            Some(&CefString::from("format")),
            Some(&CefString::from("png")),
        ) != 0
        && params.set_dictionary(Some(&CefString::from("clip")), Some(&mut region)) != 0;
    submitted.then_some(params)
}

#[derive(Deserialize)]
struct ScreenshotResult {
    data: String,
}

fn decode_screenshot_result(result: &[u8]) -> Option<Arc<[u8]>> {
    let result: ScreenshotResult = serde_json::from_slice(result).ok()?;
    let png = base64::engine::general_purpose::STANDARD
        .decode(result.data)
        .ok()?;
    (!png.is_empty()).then(|| Arc::from(png.as_slice()))
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "CEF coordinates are saturated to its signed integer range"
)]
#[must_use]
fn unscaled_osr_coordinate(value: i32, scale_factor: f32) -> i32 {
    (f64::from(value) / f64::from(scale_factor))
        .round()
        .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeSignal {
    ContextInitialized,
    RequestContextInitialized { profile: Arc<str> },
    ScheduleMessagePump(i64),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AcceleratedPaintDiagnostics {
    pub callback_count: u64,
    pub missing_info_count: u64,
    pub view_count: u64,
    pub popup_count: u64,
    pub unique_handle_count: usize,
    pub handle_transition_count: u64,
    pub consecutive_handle_reuse_count: u64,
    pub handles: Vec<AcceleratedPaintHandleDiagnostics>,
    pub last_observation: Option<AcceleratedPaintObservation>,
    pub gpu_import_attempt_count: u64,
    pub gpu_frame_delivered_count: u64,
    pub gpu_import_failure_count: u64,
    pub gpu_helper_fallback_count: u64,
    pub stale_pool_frame_count: u64,
    pub readback_frame_delivered_count: u64,
    pub latest_pool_generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceleratedPaintHandleDiagnostics {
    pub identity: String,
    pub use_count: u64,
    pub first_callback: u64,
    pub last_callback: u64,
    pub minimum_reuse_gap: Option<u64>,
    pub maximum_reuse_gap: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceleratedPaintObservation {
    pub callback: u64,
    pub paint_element: String,
    pub width: i32,
    pub height: i32,
    pub pixel_format: String,
    pub pixel_format_raw: u32,
    pub drm_modifier: Option<u64>,
    pub plane_count: i32,
    pub planes: Vec<AcceleratedPaintPlaneDiagnostics>,
    pub handle_identity: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AcceleratedPaintPlaneDiagnostics {
    pub fd: i32,
    pub stride: u32,
    pub offset: u64,
    pub size: u64,
}

#[allow(
    clippy::large_enum_variant,
    reason = "the bootstrap result is constructed and consumed once at startup; boxing the runtime adds indirection to the steady-state owner"
)]
pub enum BrowserBootstrap {
    SubprocessExit(i32),
    Runtime(BrowserRuntime),
}

#[derive(Debug, Error)]
pub enum BrowserError {
    #[error("could not prepare the browser profile: {0}")]
    Profile(#[from] BrowserProfileError),
    #[error("CEF could not parse the process command line")]
    CommandLine,
    #[error("CEF returned an unexpected subprocess result: {0}")]
    ExecuteProcess(i32),
    #[error("CEF initialization failed; verify that its libraries and resources are installed")]
    Initialize,
    #[error("CEF is still initializing")]
    NotReady,
    #[error("CEF could not create the persistent browser request context")]
    RequestContext,
    #[error("CEF could not create the off-screen browser")]
    CreateBrowser,
    #[error("CEF could not access the persistent cookie store")]
    CookieStore,
    #[error("CEF could not start the browser data operation")]
    BrowserData,
    #[error("browser site data can only be cleared for an HTTP or HTTPS origin")]
    UnsupportedOrigin,
    #[error("CEF cannot shut down while {0} browser session(s) are still open")]
    BrowsersStillOpen(u64),
    #[error("CEF cannot shut down while {0} browser data operation(s) are still active")]
    DataOperationsStillActive(u64),
    #[error("CEF could not configure the browser proxy preference: {0}")]
    ProxyPreference(String),
    #[error("CEF has already shut down")]
    AlreadyShutdown,
    #[cfg(target_os = "macos")]
    #[error("CEF could not load the Chromium Embedded Framework")]
    FrameworkLoad,
}

#[derive(Clone, Copy)]
struct MessagePumpState {
    phase: RuntimePhase,
    initialized: bool,
}

struct BrowserMessagePumpInner {
    state: Cell<MessagePumpState>,
    pumping: Cell<bool>,
    repump: Cell<bool>,
    active_sessions: Arc<AtomicU64>,
}

struct PumpGuard<'a>(&'a Cell<bool>);

impl Drop for PumpGuard<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

/// Main-thread handle for stepping CEF without borrowing the owning runtime.
/// CEF can re-enter the host's `AppKit` callbacks mid-iteration, so the GPUI
/// client drops its app/entity borrow before pumping.
#[derive(Clone)]
pub struct BrowserMessagePump {
    inner: Rc<BrowserMessagePumpInner>,
}

impl BrowserMessagePump {
    fn new(active_sessions: Arc<AtomicU64>) -> Self {
        Self {
            inner: Rc::new(BrowserMessagePumpInner {
                state: Cell::new(MessagePumpState {
                    phase: RuntimePhase::Uninitialized,
                    initialized: false,
                }),
                pumping: Cell::new(false),
                repump: Cell::new(false),
                active_sessions,
            }),
        }
    }

    fn state(&self) -> MessagePumpState {
        self.inner.state.get()
    }

    fn set_phase(&self, phase: RuntimePhase) {
        self.inner.state.set(MessagePumpState {
            phase,
            ..self.state()
        });
    }

    fn enable(&self) {
        self.inner.state.set(MessagePumpState {
            initialized: true,
            ..self.state()
        });
    }

    fn mark_closed(&self) {
        self.inner.state.set(MessagePumpState {
            phase: RuntimePhase::Closed,
            initialized: false,
        });
    }

    fn disable(&self) {
        self.inner.state.set(MessagePumpState {
            initialized: false,
            ..self.state()
        });
    }

    pub fn do_message_loop_work(&self) {
        self.run_message_loop_work(cef::do_message_loop_work);
    }

    fn run_message_loop_work(&self, mut work: impl FnMut()) {
        let state = self.state();
        if !state.initialized || matches!(state.phase, RuntimePhase::Closed | RuntimePhase::Failed)
        {
            return;
        }
        if self.inner.pumping.replace(true) {
            self.inner.repump.set(true);
            return;
        }

        let _guard = PumpGuard(&self.inner.pumping);
        loop {
            self.inner.repump.set(false);
            let started = diagnostic_timer();
            let state = self.state();
            if state.initialized
                && !matches!(state.phase, RuntimePhase::Closed | RuntimePhase::Failed)
            {
                work();
            }
            log::trace!(
                target: "zz_browser::diagnostics::message_pump",
                "work initialized={} phase={:?} active_sessions={} elapsed_us={}",
                state.initialized,
                state.phase,
                self.inner.active_sessions.load(Ordering::Relaxed),
                diagnostic_elapsed_us(started),
            );
            if !self.inner.repump.get() {
                break;
            }
        }
    }
}

pub struct BrowserRuntime {
    message_pump: BrowserMessagePump,
    signal_tx: Sender<RuntimeSignal>,
    signals: Receiver<RuntimeSignal>,
    args: Args,
    app: cef::App,
    sandbox_info: *mut u8,
    profile_paths: BrowserProfilePaths,
    profile_contexts: BTreeMap<String, ProfileContext>,
    next_session: u64,
    active_sessions: Arc<AtomicU64>,
    active_data_operations: Arc<AtomicU64>,
    windowless_frame_rate: i32,
    frame_rate_override: Option<i32>,
    shared_texture_enabled: bool,
    external_begin_frame_enabled: bool,
    begin_frame_adaptive_enabled: bool,
    log_file: Option<PathBuf>,
    #[cfg(target_os = "macos")]
    _loader: cef::library_loader::LibraryLoader,
}

struct ProfileContext {
    context: RequestContext,
    ready: bool,
    proxy_port: Option<u16>,
}

impl BrowserRuntime {
    #[must_use]
    pub fn phase(&self) -> RuntimePhase {
        self.message_pump.state().phase
    }

    #[must_use]
    pub fn message_pump(&self) -> BrowserMessagePump {
        self.message_pump.clone()
    }

    #[must_use]
    pub fn signals(&self) -> Receiver<RuntimeSignal> {
        self.signals.clone()
    }

    #[must_use]
    pub fn profile_paths(&self) -> &BrowserProfilePaths {
        &self.profile_paths
    }

    /// Route CEF's own log output (subprocesses included) to this file
    /// instead of stderr. Takes effect at [`Self::start`].
    pub fn set_log_file(&mut self, path: PathBuf) {
        self.log_file = Some(path);
    }

    #[must_use]
    pub fn windowless_frame_rate(&self) -> i32 {
        self.windowless_frame_rate
    }

    #[must_use]
    pub fn frame_rate_override(&self) -> Option<i32> {
        self.frame_rate_override
    }

    #[must_use]
    pub fn shared_texture_enabled(&self) -> bool {
        self.shared_texture_enabled
    }

    #[must_use]
    pub fn external_begin_frame_enabled(&self) -> bool {
        self.external_begin_frame_enabled
    }

    #[must_use]
    pub fn begin_frame_adaptive_enabled(&self) -> bool {
        self.begin_frame_adaptive_enabled
    }

    #[must_use]
    pub fn active_session_count(&self) -> u64 {
        self.active_sessions.load(Ordering::Relaxed)
    }

    #[must_use]
    pub fn active_data_operation_count(&self) -> u64 {
        self.active_data_operations.load(Ordering::Relaxed)
    }

    /// Initialize CEF on demand, on the process main thread CEF requires.
    /// One-way, and a no-op unless the runtime is still
    /// [`RuntimePhase::Uninitialized`].
    pub fn start(&mut self) -> Result<(), BrowserError> {
        if self.phase() != RuntimePhase::Uninitialized {
            return Ok(());
        }

        let started = diagnostic_timer();
        let settings = Settings {
            no_sandbox: 0,
            external_message_pump: 1,
            windowless_rendering_enabled: 1,
            root_cache_path: path_to_cef_string(&self.profile_paths.root),
            persist_session_cookies: 1,
            log_severity: LogSeverity::WARNING,
            log_file: self
                .log_file
                .as_deref()
                .map(path_to_cef_string)
                .unwrap_or_default(),
            background_color: 0xff10_1318,
            ..Default::default()
        };
        // Native CEF windows send `CefAppProtocol` selectors GPUI's `NSApp` class lacks.
        #[cfg(target_os = "macos")]
        mac_app_protocol::install();
        self.message_pump.set_phase(RuntimePhase::Initializing);
        if initialize(
            Some(self.args.as_main_args()),
            Some(&settings),
            Some(&mut self.app),
            self.sandbox_info,
        ) != 1
        {
            self.message_pump.set_phase(RuntimePhase::Failed);
            self.message_pump.disable();
            return Err(BrowserError::Initialize);
        }
        self.message_pump.enable();

        log::debug!(
            target: "zz_browser::diagnostics::lifecycle",
            "runtime initialized profile_paths={:?} windowless_frame_rate={} shared_texture_enabled={} wayland_physical_osr={} elapsed_us={}",
            self.profile_paths,
            self.windowless_frame_rate,
            self.shared_texture_enabled,
            uses_wayland_physical_osr(),
            diagnostic_elapsed_us(started),
        );
        Ok(())
    }

    /// Create the default persistent request context after CEF global
    /// initialization.
    pub fn handle_context_initialized(&mut self) -> Result<(), BrowserError> {
        if self.phase() == RuntimePhase::Running {
            return Ok(());
        }
        if self.phase() != RuntimePhase::Initializing {
            return Err(BrowserError::NotReady);
        }

        if let Err(error) = self.start_profile_context(DEFAULT_BROWSER_PROFILE) {
            self.message_pump.set_phase(RuntimePhase::Closing);
            return Err(error);
        }
        Ok(())
    }

    /// Mark one persistent request context ready after its CEF callback.
    pub fn handle_request_context_initialized(
        &mut self,
        profile: &str,
    ) -> Result<bool, BrowserError> {
        let Some(profile_context) = self.profile_contexts.get_mut(profile) else {
            return Err(BrowserError::RequestContext);
        };
        profile_context.ready = true;

        if profile == DEFAULT_BROWSER_PROFILE && self.phase() == RuntimePhase::Initializing {
            self.message_pump.set_phase(RuntimePhase::Running);
            return Ok(true);
        }
        if self.phase() != RuntimePhase::Running {
            return Err(BrowserError::RequestContext);
        }
        Ok(false)
    }

    /// Ensure a named persistent request context exists. Returns `true` once
    /// CEF has reported that the context is ready for browser creation.
    pub fn ensure_profile_context(&mut self, profile: &str) -> Result<bool, BrowserError> {
        if self.phase() != RuntimePhase::Running {
            return Err(BrowserError::NotReady);
        }
        let (profile, _) = self.profile_paths.ensure_profile(profile)?;
        if let Some(profile_context) = self.profile_contexts.get(&profile) {
            return Ok(profile_context.ready);
        }
        self.start_profile_context(&profile)?;
        Ok(false)
    }

    /// Ensure a client-local composite egress context exists. Its name carries
    /// an internal suffix past the protocol's name cap and never crosses the wire.
    pub fn ensure_egress_profile_context(&mut self, profile: &str) -> Result<bool, BrowserError> {
        if self.phase() != RuntimePhase::Running {
            return Err(BrowserError::NotReady);
        }
        let profile_path = self.profile_paths.ensure_egress_profile(profile)?;
        if let Some(profile_context) = self.profile_contexts.get(profile) {
            return Ok(profile_context.ready);
        }
        self.start_profile_context_at(profile.to_owned(), &profile_path)?;
        Ok(false)
    }

    fn start_profile_context(&mut self, profile: &str) -> Result<String, BrowserError> {
        let (profile, profile_path) = self.profile_paths.ensure_profile(profile)?;
        if self.profile_contexts.contains_key(&profile) {
            return Ok(profile);
        }
        self.start_profile_context_at(profile.clone(), &profile_path)?;
        Ok(profile)
    }

    fn start_profile_context_at(
        &mut self,
        profile: String,
        profile_path: &std::path::Path,
    ) -> Result<(), BrowserError> {
        let settings = RequestContextSettings {
            cache_path: path_to_cef_string(profile_path),
            persist_session_cookies: 1,
            ..Default::default()
        };
        let mut handler =
            ProfileRequestContextHandler::new(self.signal_tx.clone(), Arc::from(profile.as_str()));
        let Some(context) = request_context_create_context(Some(&settings), Some(&mut handler))
        else {
            return Err(BrowserError::RequestContext);
        };
        self.profile_contexts.insert(
            profile,
            ProfileContext {
                context,
                ready: false,
                proxy_port: None,
            },
        );
        Ok(())
    }

    /// `CefString::default()` marshals to NULL, which libcef out-parameters reject; this passes real storage.
    fn with_error_string<R>(f: impl FnOnce(&mut CefString) -> R) -> (R, String) {
        let mut storage: cef::sys::_cef_string_utf16_t = unsafe { std::mem::zeroed() };
        let mut error = CefString::from(&raw mut storage);
        let result = f(&mut error);
        let message = error.to_string();
        unsafe { cef::sys::cef_string_utf16_clear(&raw mut storage) };
        (result, message)
    }

    /// Report the current proxy value and whether any preference can be written,
    /// separating a policy on `proxy` from a broken `set_preference` dispatch.
    pub fn probe_preference_system(&mut self, profile: &str) -> String {
        let Some(profile_context) = self.profile_contexts.get_mut(profile) else {
            return "context missing".to_owned();
        };
        let context = &mut profile_context.context;
        let proxy_name = CefString::from("proxy");
        let current = context.preference(Some(&proxy_name)).map_or_else(
            || "none".to_owned(),
            |value| {
                format!(
                    "type={:?} valid={}",
                    value.get_type(),
                    value.is_valid() != 0
                )
            },
        );
        let bool_name = CefString::from("credentials_enable_service");
        let can_set_bool = context.can_set_preference(Some(&bool_name)) != 0;
        let Some(mut bool_value) = value_create() else {
            return format!("proxy_current=({current}) value_create failed");
        };
        let bool_built = bool_value.set_bool(0) != 0;
        let (bool_set, error) = Self::with_error_string(|error| {
            context.set_preference(Some(&bool_name), Some(&mut bool_value), Some(error)) != 0
        });
        let (restore_default, restore_error) = Self::with_error_string(|restore_error| {
            context.set_preference(Some(&bool_name), None, Some(restore_error)) != 0
        });
        let global = preference_manager_get_global().map_or_else(
            || "unavailable".to_owned(),
            |manager| {
                let mut global_value = match value_create() {
                    Some(value) => value,
                    None => return "value_create failed".to_owned(),
                };
                let _ = global_value.set_bool(0);
                let (set, global_error) = Self::with_error_string(|global_error| {
                    manager.set_preference(
                        Some(&bool_name),
                        Some(&mut global_value),
                        Some(global_error),
                    ) != 0
                });
                format!("set_ok={set} error={global_error:?}")
            },
        );
        format!(
            "proxy_current=({current}) bool_pref: can_set={can_set_bool} built={bool_built} \
             set_ok={bool_set} error={:?} restore_default_ok={restore_default} \
             restore_error={:?} global_manager: {global}",
            error.to_string(),
            restore_error.to_string(),
        )
    }

    /// Point one ready request context at the `ssh -D` SOCKS5 listener on `port`.
    /// Chromium reads a bare `host:port` as an HTTP proxy, so the `socks5://`
    /// scheme and the `<-loopback>` bypass are both required.
    pub fn set_profile_proxy(&mut self, profile: &str, port: u16) -> Result<(), BrowserError> {
        if self.phase() != RuntimePhase::Running {
            return Err(BrowserError::NotReady);
        }
        let profile_context = self
            .profile_contexts
            .get_mut(profile)
            .filter(|profile| profile.ready)
            .ok_or(BrowserError::RequestContext)?;
        if profile_context.proxy_port == Some(port) {
            return Ok(());
        }

        let mut proxy = dictionary_value_create().ok_or(BrowserError::RequestContext)?;
        let mode_key = CefString::from("mode");
        let fixed_servers = CefString::from("fixed_servers");
        let server_key = CefString::from("server");
        let server = CefString::from(format!("socks5://127.0.0.1:{port}").as_str());
        let bypass_key = CefString::from("bypass_list");
        let bypass = CefString::from("<-loopback>");
        if proxy.set_string(Some(&mode_key), Some(&fixed_servers)) == 0
            || proxy.set_string(Some(&server_key), Some(&server)) == 0
            || proxy.set_string(Some(&bypass_key), Some(&bypass)) == 0
        {
            return Err(BrowserError::ProxyPreference(
                "could not build the proxy preference dictionary".to_owned(),
            ));
        }

        let mut value = value_create().ok_or(BrowserError::RequestContext)?;
        if value.set_dictionary(Some(&mut proxy)) == 0 {
            return Err(BrowserError::ProxyPreference(
                "could not wrap the proxy preference dictionary".to_owned(),
            ));
        }
        let preference = CefString::from("proxy");
        let (rejected, error) = Self::with_error_string(|error| {
            profile_context
                .context
                .set_preference(Some(&preference), Some(&mut value), Some(error))
                == 0
        });
        if rejected {
            return Err(BrowserError::ProxyPreference(if error.is_empty() {
                // An empty reason means CEF's VerifyBrowserContext guard rejected the
                // call before the pref service, silently.
                let on_ui_thread = currently_on(ThreadId::UI) != 0;
                let has_preference = profile_context.context.has_preference(Some(&preference)) != 0;
                let can_set_preference = profile_context
                    .context
                    .can_set_preference(Some(&preference))
                    != 0;
                format!(
                    "CEF rejected the proxy preference without a reason \
                     (on_ui_thread={on_ui_thread} has_preference={has_preference} \
                     can_set_preference={can_set_preference})"
                )
            } else {
                error
            }));
        }
        profile_context.proxy_port = Some(port);
        Ok(())
    }

    pub fn do_message_loop_work(&self) {
        self.message_pump.do_message_loop_work();
    }

    /// Import cookies into the persistent request context and flush its store.
    /// Per-cookie rejections arrive in [`CookieImportResult::rejected`].
    pub fn import_cookies(
        &self,
        profile: &str,
        batch: CookieImportBatch,
    ) -> Result<Receiver<CookieImportResult>, BrowserError> {
        if self.phase() != RuntimePhase::Running {
            return Err(BrowserError::NotReady);
        }
        if batch.cookies.is_empty() {
            return Err(BrowserError::BrowserData);
        }
        let profile_context = self
            .profile_contexts
            .get(profile)
            .filter(|profile| profile.ready)
            .ok_or(BrowserError::RequestContext)?;
        let manager = profile_context
            .context
            .cookie_manager(None)
            .ok_or(BrowserError::CookieStore)?;
        let (results, result_rx) = async_channel::bounded(1);
        let progress = Arc::new(Mutex::new(CookieImportProgress {
            remaining: batch.cookies.len(),
            imported: 0,
            rejected: 0,
            skipped: batch.skipped,
            manager: manager.clone(),
            results,
            operation: ActiveDataOperation::new(Arc::clone(&self.active_data_operations)),
        }));

        for cookie in batch.cookies {
            let Some(cef_cookie) = cef_cookie(&cookie) else {
                finish_cookie_import(&progress, false);
                continue;
            };
            let url = CefString::from(cookie.source_url.as_str());
            let mut callback = ImportCookieCallback::new(Arc::clone(&progress));
            if manager.set_cookie(Some(&url), Some(&cef_cookie), Some(&mut callback)) == 0 {
                finish_cookie_import(&progress, false);
            }
        }

        Ok(result_rx)
    }

    /// Create a windowless browser session in a named persistent profile.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "CEF client wiring, initial ownership, and cross-platform GPU context transfer form one ordered operation"
    )]
    pub fn create_session(
        &mut self,
        profile: &str,
        initial_url: &str,
        viewport: Viewport,
        page_zoom_factor: f64,
        windowless_frame_rate: Option<i32>,
        gpu_context: Option<BrowserGpuContext>,
        allow_shared_texture: bool,
    ) -> Result<BrowserSession, BrowserError> {
        if self.phase() != RuntimePhase::Running {
            return Err(BrowserError::NotReady);
        }
        let profile_context = self
            .profile_contexts
            .get(profile)
            .filter(|profile| profile.ready)
            .ok_or(BrowserError::RequestContext)?;
        let request_context = profile_context.context.clone();

        self.next_session = self.next_session.wrapping_add(1).max(1);
        let id = SessionId(self.next_session);
        let viewport = Arc::new(Mutex::new(viewport.sanitized()));
        let page_zoom_factor = Arc::new(Mutex::new(sanitized_page_zoom_factor(page_zoom_factor)));
        let mailbox = FrameMailbox::default();
        let (events, event_rx) = async_channel::unbounded();
        let element_pick = ElementPickState::default();
        let pending_capture = PendingCapture::default();
        let accelerated_paint = AcceleratedPaintTracker::default();
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let accelerated_frames = AcceleratedFrameProducer::new(gpu_context, *viewport.lock());
        #[cfg(target_os = "windows")]
        let d3d11_frames = D3d11FrameProducer::new(gpu_context, *viewport.lock());
        #[cfg(target_os = "macos")]
        let _ = gpu_context;
        #[cfg(target_os = "macos")]
        let metal_frames = MetalFrameProducer::new(*viewport.lock());
        let bridge = SessionBridge {
            id,
            events,
            viewport: Arc::clone(&viewport),
            page_zoom_factor: Arc::clone(&page_zoom_factor),
            mailbox: mailbox.clone(),
            invalid_frames: Arc::new(AtomicU64::new(0)),
            accelerated_paint: accelerated_paint.clone(),
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            accelerated_frames: accelerated_frames.clone(),
            #[cfg(target_os = "macos")]
            metal_frames: metal_frames.clone(),
            #[cfg(target_os = "windows")]
            d3d11_frames: d3d11_frames.clone(),
            shared_texture_fallback_notified: Arc::new(AtomicBool::new(false)),
            element_pick: element_pick.clone(),
            pending_capture: Arc::clone(&pending_capture),
        };
        let message_router = BrowserSideRouter::new(element_picker_router_config());
        let picker_available = message_router
            .add_handler(
                Arc::new(ElementPickerQueryHandler {
                    bridge: bridge.clone(),
                }),
                false,
            )
            .is_some();
        if !picker_available {
            log::error!("could not register the CEF element picker query handler");
        }

        let render_handler = RenderHandlerBuilder::new(bridge.clone());
        let display_handler = DisplayHandlerBuilder::new(bridge.clone());
        let life_span_handler =
            LifeSpanHandlerBuilder::new(bridge.clone(), Arc::clone(&message_router));
        let load_handler = LoadHandlerBuilder::new(bridge.clone());
        let context_menu_handler = BridgedContextMenuHandler::new(bridge.clone());
        let request_handler = RequestHandlerBuilder::new(bridge, Arc::clone(&message_router));
        let dialog_handler = DeniedDialogHandler::new();
        let download_handler = DeniedDownloadHandler::new();
        let permission_handler = DeniedPermissionHandler::new();
        let mut client = BrowserClient::new(
            render_handler,
            display_handler,
            life_span_handler,
            load_handler,
            request_handler,
            context_menu_handler,
            dialog_handler,
            download_handler,
            permission_handler,
            message_router,
        );

        let window_info = WindowInfo {
            shared_texture_enabled: i32::from(self.shared_texture_enabled && allow_shared_texture),
            external_begin_frame_enabled: i32::from(self.external_begin_frame_enabled),
            ..WindowInfo::default().set_as_windowless(Default::default())
        };
        let browser_settings = BrowserSettings {
            windowless_frame_rate: windowless_frame_rate
                .unwrap_or(self.windowless_frame_rate)
                .clamp(1, MAX_BROWSER_FRAME_RATE),
            background_color: 0xff10_1318,
            ..Default::default()
        };
        let url = CefString::from(initial_url);
        let mut context = request_context.clone();
        let Some(browser) = browser_host_create_browser_sync(
            Some(&window_info),
            Some(&mut client),
            Some(&url),
            Some(&browser_settings),
            None,
            Some(&mut context),
        ) else {
            return Err(BrowserError::CreateBrowser);
        };

        let initial_viewport = *viewport.lock();
        let mut session = BrowserSession {
            id,
            profile: Arc::from(profile),
            phase: SessionPhase::Creating,
            browser,
            viewport: Arc::clone(&viewport),
            page_zoom_factor,
            mailbox,
            events: event_rx,
            element_pick,
            picker_available,
            pending_capture,
            pending_site_data_clears: BTreeMap::new(),
            _request_context: request_context,
            active_sessions: Arc::clone(&self.active_sessions),
            active_data_operations: Arc::clone(&self.active_data_operations),
            accelerated_paint,
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            accelerated_frames,
            #[cfg(target_os = "macos")]
            metal_frames,
            #[cfg(target_os = "windows")]
            d3d11_frames,
            counted_open: true,
        };
        self.active_sessions.fetch_add(1, Ordering::Relaxed);
        // CEF builds the OSR surface before consuming our screen info, so force one
        // ordered screen/resize sync.
        session.apply_viewport(initial_viewport, true);
        Ok(session)
    }

    /// Release the request context and shut CEF down exactly once.
    pub fn shutdown(&mut self) -> Result<(), BrowserError> {
        let state = self.message_pump.state();
        if state.phase == RuntimePhase::Closed {
            return Err(BrowserError::AlreadyShutdown);
        }
        if state.phase == RuntimePhase::Uninitialized {
            self.message_pump.mark_closed();
            log::debug!(
                target: "zz_browser::diagnostics::lifecycle",
                "closed browser runtime without initializing CEF"
            );
            return Ok(());
        }
        if !state.initialized {
            return Ok(());
        }
        ensure_no_live_sessions(&self.active_sessions)?;
        ensure_no_active_data_operations(&self.active_data_operations)?;
        self.message_pump.set_phase(RuntimePhase::Closing);
        self.profile_contexts.clear();
        cef::shutdown();
        self.message_pump.mark_closed();
        Ok(())
    }
}

impl Drop for BrowserRuntime {
    fn drop(&mut self) {
        if self.message_pump.state().initialized {
            self.message_pump.disable();
            log::error!(
                "BrowserRuntime dropped before explicit shutdown; CEF shutdown ordering was violated"
            );
        }
    }
}

struct CookieImportProgress {
    remaining: usize,
    imported: usize,
    rejected: usize,
    skipped: usize,
    manager: CookieManager,
    results: Sender<CookieImportResult>,
    operation: ActiveDataOperation,
}

#[derive(Clone)]
struct ActiveDataOperation(Arc<ActiveDataOperationState>);

struct ActiveDataOperationState {
    active: Arc<AtomicU64>,
    finished: AtomicBool,
}

impl ActiveDataOperation {
    fn new(active: Arc<AtomicU64>) -> Self {
        active.fetch_add(1, Ordering::AcqRel);
        Self(Arc::new(ActiveDataOperationState {
            active,
            finished: AtomicBool::new(false),
        }))
    }

    fn finish(&self) -> bool {
        if self.0.finished.swap(true, Ordering::AcqRel) {
            return false;
        }
        self.0.active.fetch_sub(1, Ordering::AcqRel);
        true
    }
}

impl Drop for ActiveDataOperationState {
    fn drop(&mut self) {
        if !self.finished.swap(true, Ordering::AcqRel) {
            self.active.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

fn cef_cookie(cookie: &BrowserCookie) -> Option<Cookie> {
    let expires = match cookie.expires_unix_micros {
        Some(unix_micros) => Some(cef_expiration(unix_micros)?),
        None => None,
    };
    Some(Cookie {
        name: CefString::from(cookie.name.as_str()),
        value: CefString::from(cookie.value.as_str()),
        domain: CefString::from(cookie.domain.as_str()),
        path: CefString::from(cookie.path.as_str()),
        secure: i32::from(cookie.secure),
        httponly: i32::from(cookie.http_only),
        has_expires: i32::from(expires.is_some()),
        expires: expires.unwrap_or_default(),
        same_site: match cookie.same_site {
            BrowserCookieSameSite::Unspecified => CookieSameSite::UNSPECIFIED,
            BrowserCookieSameSite::NoRestriction => CookieSameSite::NO_RESTRICTION,
            BrowserCookieSameSite::Lax => CookieSameSite::LAX_MODE,
            BrowserCookieSameSite::Strict => CookieSameSite::STRICT_MODE,
        },
        priority: match cookie.priority {
            BrowserCookiePriority::Low => CookiePriority::LOW,
            BrowserCookiePriority::Medium => CookiePriority::MEDIUM,
            BrowserCookiePriority::High => CookiePriority::HIGH,
        },
        ..Cookie::default()
    })
}

fn cef_expiration(unix_micros: i64) -> Option<Basetime> {
    Some(Basetime {
        val: unix_micros.checked_add(WINDOWS_EPOCH_UNIX_OFFSET_MICROS)?,
    })
}

fn finish_cookie_import(progress: &Arc<Mutex<CookieImportProgress>>, success: bool) {
    let flush = {
        let mut progress = progress.lock();
        if progress.remaining == 0 {
            return;
        }
        progress.remaining -= 1;
        if success {
            progress.imported += 1;
        } else {
            progress.rejected += 1;
        }
        (progress.remaining == 0).then(|| {
            (
                progress.manager.clone(),
                progress.results.clone(),
                progress.operation.clone(),
                CookieImportResult {
                    imported: progress.imported,
                    skipped: progress.skipped,
                    rejected: progress.rejected,
                    persisted: false,
                },
            )
        })
    };

    let Some((manager, results, operation, result)) = flush else {
        return;
    };
    let mut callback = CookieImportFlushCallback::new(result, results.clone(), operation.clone());
    if manager.flush_store(Some(&mut callback)) == 0 {
        let _ = results.try_send(result);
        operation.finish();
    }
}

fn site_origin(input: &str) -> Result<String, BrowserError> {
    let url = Url::parse(input).map_err(|_| BrowserError::UnsupportedOrigin)?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(BrowserError::UnsupportedOrigin);
    }
    let origin = url.origin().ascii_serialization();
    (origin != "null")
        .then_some(origin)
        .ok_or(BrowserError::UnsupportedOrigin)
}

pub struct BrowserSession {
    id: SessionId,
    profile: Arc<str>,
    phase: SessionPhase,
    browser: Browser,
    viewport: Arc<Mutex<Viewport>>,
    page_zoom_factor: Arc<Mutex<f64>>,
    mailbox: FrameMailbox,
    events: Receiver<BrowserEvent>,
    element_pick: ElementPickState,
    picker_available: bool,
    pending_capture: PendingCapture,
    pending_site_data_clears: BTreeMap<i32, Registration>,
    _request_context: RequestContext,
    active_sessions: Arc<AtomicU64>,
    active_data_operations: Arc<AtomicU64>,
    accelerated_paint: AcceleratedPaintTracker,
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    accelerated_frames: AcceleratedFrameProducer,
    #[cfg(target_os = "macos")]
    metal_frames: MetalFrameProducer,
    #[cfg(target_os = "windows")]
    d3d11_frames: D3d11FrameProducer,
    counted_open: bool,
}

impl BrowserSession {
    #[must_use]
    pub fn id(&self) -> SessionId {
        self.id
    }

    #[must_use]
    pub fn phase(&self) -> SessionPhase {
        self.phase
    }

    #[must_use]
    pub fn profile(&self) -> &str {
        self.profile.as_ref()
    }

    #[must_use]
    pub fn viewport(&self) -> Viewport {
        *self.viewport.lock()
    }

    #[must_use]
    pub fn page_zoom_factor(&self) -> f64 {
        *self.page_zoom_factor.lock()
    }

    #[must_use]
    pub fn page_zoom_percent(&self) -> u16 {
        page_zoom_percent(self.page_zoom_factor())
    }

    #[must_use]
    pub fn zoom_in(&self) -> u16 {
        let next = next_page_zoom_factor(self.page_zoom_factor(), 1);
        self.set_page_zoom_factor(next)
    }

    #[must_use]
    pub fn zoom_out(&self) -> u16 {
        let next = next_page_zoom_factor(self.page_zoom_factor(), -1);
        self.set_page_zoom_factor(next)
    }

    #[must_use]
    pub fn reset_zoom(&self) -> u16 {
        self.set_page_zoom_factor(1.0)
    }

    fn set_page_zoom_factor(&self, factor: f64) -> u16 {
        let factor = sanitized_page_zoom_factor(factor);
        *self.page_zoom_factor.lock() = factor;
        if let Some(host) = self.browser.host() {
            host.set_zoom_level(effective_chromium_zoom_level(self.viewport(), factor));
        }
        page_zoom_percent(factor)
    }

    #[must_use]
    pub fn frame_mailbox_diagnostics(&self) -> crate::FrameMailboxDiagnostics {
        self.mailbox.diagnostics()
    }

    #[must_use]
    pub fn accelerated_paint_diagnostics(&self) -> AcceleratedPaintDiagnostics {
        self.accelerated_paint.diagnostics()
    }

    #[must_use]
    pub fn events(&self) -> Receiver<BrowserEvent> {
        self.events.clone()
    }

    /// Clear cookies and origin-owned web storage through Chromium's `DevTools`
    /// protocol. The receiver resolves when Chromium reports the method result.
    pub fn clear_site_data(&mut self) -> Result<Receiver<SiteDataClearResult>, BrowserError> {
        if self.phase != SessionPhase::Ready {
            return Err(BrowserError::NotReady);
        }
        let frame = self.browser.main_frame().ok_or(BrowserError::BrowserData)?;
        let frame_url = frame.url();
        let origin = site_origin(&CefString::from(&frame_url).to_string())?;
        let host = self.browser.host().ok_or(BrowserError::BrowserData)?;
        let mut params = dictionary_value_create().ok_or(BrowserError::BrowserData)?;
        let origin_key = CefString::from("origin");
        let origin_value = CefString::from(origin.as_str());
        let storage_types_key = CefString::from("storageTypes");
        let storage_types_value = CefString::from("all");
        if params.set_string(Some(&origin_key), Some(&origin_value)) == 0
            || params.set_string(Some(&storage_types_key), Some(&storage_types_value)) == 0
        {
            return Err(BrowserError::BrowserData);
        }

        let (results, result_rx) = async_channel::bounded(1);
        let expected_id = Arc::new(AtomicI32::new(0));
        let operation = ActiveDataOperation::new(Arc::clone(&self.active_data_operations));
        let mut observer = SiteDataClearObserver::new(Arc::clone(&expected_id), results, operation);
        let registration = host
            .add_dev_tools_message_observer(Some(&mut observer))
            .ok_or(BrowserError::BrowserData)?;
        let method = CefString::from("Storage.clearDataForOrigin");
        let message_id = host.execute_dev_tools_method(0, Some(&method), Some(&mut params));
        if message_id == 0 {
            return Err(BrowserError::BrowserData);
        }
        expected_id.store(message_id, Ordering::Release);
        self.pending_site_data_clears
            .insert(message_id, registration);
        Ok(result_rx)
    }

    pub fn finish_site_data_clear(&mut self, message_id: i32) {
        self.pending_site_data_clears.remove(&message_id);
    }

    /// Release the screenshot observer once its pick has been delivered, so the
    /// `DevTools` agent does not stay attached between picks.
    pub fn finish_element_capture(&self) {
        self.pending_capture.lock().take();
    }

    #[must_use]
    pub fn take_frame(&self) -> Option<OsrFrame> {
        self.mailbox.take()
    }

    pub fn recycle_frame(&self, bgra: Vec<u8>) {
        self.mailbox.recycle(bgra);
    }

    pub fn mark_ready(&mut self) {
        if self.phase == SessionPhase::Creating {
            self.phase = SessionPhase::Ready;
        }
    }

    pub fn mark_crashed(&mut self) {
        if self.phase == SessionPhase::Ready {
            self.phase = SessionPhase::Crashed;
            self.mailbox.clear();
            self.element_pick.cancel();
        }
    }

    pub fn mark_closed(&mut self) {
        self.phase = SessionPhase::Closed;
        if self.counted_open {
            self.active_sessions.fetch_sub(1, Ordering::Release);
            self.counted_open = false;
        }
        self.mailbox.clear();
        self.element_pick.cancel();
    }

    pub fn navigate(&self, url: &str) {
        let Some(frame) = self.browser.main_frame() else {
            log::debug!(
                "dropping navigation for session {} without a main frame",
                self.id.0
            );
            return;
        };
        frame.load_url(Some(&CefString::from(url)));
    }

    #[must_use]
    pub fn current_url(&self) -> Option<String> {
        self.browser
            .main_frame()
            .map(|frame| CefString::from(&frame.url()).to_string())
            .filter(|url| !url.is_empty())
    }

    pub fn go_back(&self) {
        if self.browser.can_go_back() != 0 {
            self.browser.go_back();
        }
    }

    pub fn go_forward(&self) {
        if self.browser.can_go_forward() != 0 {
            self.browser.go_forward();
        }
    }

    pub fn reload(&self) {
        self.browser.reload();
    }

    pub fn edit(&self, command: EditCommand) {
        let Some(frame) = self
            .browser
            .focused_frame()
            .or_else(|| self.browser.main_frame())
        else {
            return;
        };
        match command {
            EditCommand::Cut => frame.cut(),
            EditCommand::Copy => frame.copy(),
            EditCommand::Paste => frame.paste(),
            EditCommand::SelectAll => frame.select_all(),
        }
    }

    /// Open (or focus) `DevTools` with the node at these pane-local logical
    /// coordinates selected.
    pub fn inspect_element_at(&self, x: i32, y: i32) {
        let Some(host) = self.browser.host() else {
            return;
        };
        let scale_factor = osr_raster_scale(*self.viewport.lock());
        let point = Point {
            x: scaled_osr_coordinate(x, scale_factor),
            y: scaled_osr_coordinate(y, scale_factor),
        };
        host.show_dev_tools(None, None, None, Some(&point));
    }

    /// Toggle Chromium's `DevTools`. All-`None` arguments make CEF open and own
    /// a native top-level window instead of a windowless browser zz must render.
    pub fn toggle_dev_tools(&self) {
        let Some(host) = self.browser.host() else {
            return;
        };
        if host.has_dev_tools() == 0 {
            host.show_dev_tools(None, None, None, None);
        } else {
            host.close_dev_tools();
        }
    }

    #[must_use]
    pub fn start_element_pick(&self, appearance: &crate::ElementPickerAppearance) -> bool {
        if self.phase != SessionPhase::Ready || !self.picker_available {
            return false;
        }
        let Some(frame) = self.browser.main_frame() else {
            return false;
        };
        let Some(token) = self.element_pick.begin() else {
            log::error!("could not create a secure element picker token");
            return false;
        };
        let script = match element_picker_start_script(&token, appearance) {
            Ok(script) => script,
            Err(error) => {
                self.element_pick.cancel();
                log::error!("could not serialize element picker appearance: {error}");
                return false;
            }
        };
        frame.execute_java_script(
            Some(&CefString::from(script.as_str())),
            Some(&CefString::from(ELEMENT_PICKER_SCRIPT_URL)),
            1,
        );
        true
    }

    #[must_use]
    pub fn cancel_element_pick(&self) -> bool {
        if !self.element_pick.cancel() {
            return false;
        }
        if let Some(frame) = self.browser.main_frame() {
            frame.execute_java_script(
                Some(&CefString::from("globalThis.__zzElementPicker?.cancel();")),
                Some(&CefString::from(ELEMENT_PICKER_SCRIPT_URL)),
                1,
            );
        }
        true
    }

    pub fn set_viewport(&mut self, next: Viewport) {
        self.apply_viewport(next, false);
    }

    fn apply_viewport(&mut self, next: Viewport, force: bool) {
        let started = diagnostic_timer();
        let next = next.sanitized();
        let previous = self.viewport.lock().to_owned();
        *self.viewport.lock() = next;
        let size_changed = previous.width != next.width || previous.height != next.height;
        let scale_changed = previous.scale_factor.to_bits() != next.scale_factor.to_bits();
        if force || size_changed || scale_changed {
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            self.accelerated_frames.set_viewport(next);
            #[cfg(target_os = "macos")]
            self.metal_frames.set_viewport(next);
            #[cfg(target_os = "windows")]
            self.d3d11_frames.set_viewport(next);
        }
        if let Some(host) = self.browser.host() {
            if force || previous.visible != next.visible {
                host.was_hidden(i32::from(!next.visible));
            }
            let screen_changed =
                previous.screen_x != next.screen_x || previous.screen_y != next.screen_y;

            if force || (uses_wayland_physical_osr() && scale_changed) {
                host.set_zoom_level(effective_chromium_zoom_level(next, self.page_zoom_factor()));
            }

            // CEF reads the screen scale at WasResized, so screen info goes first.
            if force || scale_changed || screen_changed {
                host.notify_screen_info_changed();
            }
            if force || size_changed || scale_changed {
                host.was_resized();
                // Chromium's OSR scheduler can skip the last resize of a burst as
                // damage-free; this forces one paint at the settled size.
                host.invalidate(PaintElementType::VIEW);
            }
            log::trace!(
                target: "zz_browser::diagnostics::viewport",
                "apply session={} force={} previous={previous:?} next={next:?} size_changed={} scale_changed={} screen_changed={} elapsed_us={}",
                self.id.0,
                force,
                size_changed,
                scale_changed,
                screen_changed,
                diagnostic_elapsed_us(started),
            );
        }
    }

    pub fn set_focus(&self, focused: bool) {
        let Some(host) = self.browser.host() else {
            log::debug!(
                "dropping focus change for session {} without a browser host",
                self.id.0
            );
            return;
        };
        host.set_focus(i32::from(focused));
    }

    /// Change the OSR paint ceiling for this session at runtime, clamped to
    /// CEF's supported `1..=MAX_BROWSER_FRAME_RATE` range.
    pub fn set_frame_rate(&self, frames_per_second: i32) {
        if let Some(host) = self.browser.host() {
            host.set_windowless_frame_rate(frames_per_second.clamp(1, MAX_BROWSER_FRAME_RATE));
        }
    }

    /// Drive one Chromium frame from zz's clock. No-op on Windows, where
    /// sessions never set `external_begin_frame_enabled` and CEF keeps its own
    /// scheduler.
    pub fn send_external_begin_frame(&self) {
        if let Some(host) = self.browser.host() {
            host.send_external_begin_frame();
        }
    }

    pub fn send_pointer(&self, event: PointerEvent) {
        let Some(host) = self.browser.host() else {
            return;
        };
        let scale_factor = osr_raster_scale(*self.viewport.lock());
        let mouse = MouseEvent {
            x: scaled_osr_coordinate(event.x, scale_factor),
            y: scaled_osr_coordinate(event.y, scale_factor),
            modifiers: event_flags(event.modifiers),
        };
        match event.phase {
            PointerPhase::Move => host.send_mouse_move_event(Some(&mouse), 0),
            PointerPhase::Leave => host.send_mouse_move_event(Some(&mouse), 1),
            PointerPhase::Down | PointerPhase::Up => {
                let Some(button) = event.button.map(mouse_button) else {
                    return;
                };
                host.send_mouse_click_event(
                    Some(&mouse),
                    button,
                    i32::from(event.phase == PointerPhase::Up),
                    event.click_count.max(1),
                );
            }
        }
    }

    pub fn send_wheel(&self, event: WheelEvent) {
        let Some(host) = self.browser.host() else {
            return;
        };
        let scale_factor = osr_raster_scale(*self.viewport.lock());
        let mut flags = event_flags(event.modifiers);
        if event.precise {
            flags |=
                cef_enum_bits(cef::sys::cef_event_flags_t::EVENTFLAG_PRECISION_SCROLLING_DELTA.0);
        }
        let mouse = MouseEvent {
            x: scaled_osr_coordinate(event.x, scale_factor),
            y: scaled_osr_coordinate(event.y, scale_factor),
            modifiers: flags,
        };
        let (delta_x, delta_y) = if event.precise {
            (
                scaled_osr_coordinate(event.delta_x, scale_factor),
                scaled_osr_coordinate(event.delta_y, scale_factor),
            )
        } else {
            (event.delta_x, event.delta_y)
        };
        host.send_mouse_wheel_event(Some(&mouse), delta_x, delta_y);
    }

    pub fn send_key(&self, input: KeyInput) {
        let Some(host) = self.browser.host() else {
            log::debug!(
                "dropping key event for session {} without a browser host",
                self.id.0
            );
            return;
        };
        let event = key_event(input);
        host.send_key_event(Some(&event));
        if let Some(event) = named_key_character_event(input) {
            host.send_key_event(Some(&event));
        }
    }

    pub fn send_text(&self, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(host) = self.browser.host() {
            for code_unit in text.encode_utf16() {
                let event = text_key_event(code_unit);
                host.send_key_event(Some(&event));
            }
        }
    }

    pub fn commit_composition(&self, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(host) = self.browser.host() {
            host.ime_commit_text(Some(&CefString::from(text)), None, 0);
        }
    }

    pub fn set_composition(&self, text: &str, selection_utf16: std::ops::Range<usize>) {
        let Some(host) = self.browser.host() else {
            return;
        };
        let length = u32::try_from(text.encode_utf16().count()).unwrap_or(u32::MAX);
        let selected = Range {
            from: u32::try_from(selection_utf16.start)
                .unwrap_or(length)
                .min(length),
            to: u32::try_from(selection_utf16.end)
                .unwrap_or(length)
                .min(length),
        };
        let underline = CompositionUnderline {
            range: Range {
                from: 0,
                to: length,
            },
            color: 0xffd8_dee9,
            thick: 0,
            ..Default::default()
        };
        host.ime_set_composition(
            Some(&CefString::from(text)),
            Some(&[underline]),
            None,
            Some(&selected),
        );
    }

    pub fn finish_composition(&self) {
        if let Some(host) = self.browser.host() {
            host.ime_finish_composing_text(0);
        }
    }

    pub fn cancel_composition(&self) {
        if let Some(host) = self.browser.host() {
            host.ime_cancel_composition();
        }
    }

    pub fn close(&mut self, force: bool) {
        if matches!(self.phase, SessionPhase::Closing | SessionPhase::Closed) {
            return;
        }
        self.element_pick.cancel();
        self.phase = SessionPhase::Closing;
        if let Some(host) = self.browser.host() {
            host.close_browser(i32::from(force));
        }
    }
}

impl Drop for BrowserSession {
    fn drop(&mut self) {
        if !matches!(self.phase, SessionPhase::Closed | SessionPhase::Closing) {
            log::warn!(
                "forcing CEF browser closure while dropping session {}",
                self.id.0
            );
            if let Some(host) = self.browser.host() {
                host.close_browser(1);
            }
        }
    }
}

/// Dispatch a CEF subprocess or prepare a cold main browser runtime.
pub fn bootstrap() -> Result<BrowserBootstrap, BrowserError> {
    let profile_paths =
        resolve_profile_paths().map_err(|error| BrowserError::Profile(error.into()))?;
    bootstrap_with_profile_paths(profile_paths)
}

/// Dispatch a CEF subprocess or prepare a cold runtime with an explicit cache root.
pub fn bootstrap_with_profile_paths(
    profile_paths: BrowserProfilePaths,
) -> Result<BrowserBootstrap, BrowserError> {
    bootstrap_args_with_paths(Args::new(), ptr::null_mut(), Some(profile_paths))
}

#[cfg(target_os = "windows")]
/// Dispatch a subprocess or prepare CEF through its sandbox bootstrap executable.
pub fn bootstrap_windows(
    instance: cef::sys::HINSTANCE,
    sandbox_info: *mut u8,
) -> Result<BrowserBootstrap, BrowserError> {
    bootstrap_args(Args::from(MainArgs { instance }), sandbox_info)
}

#[cfg(target_os = "windows")]
fn bootstrap_args(args: Args, sandbox_info: *mut u8) -> Result<BrowserBootstrap, BrowserError> {
    bootstrap_args_with_paths(args, sandbox_info, None)
}

fn bootstrap_args_with_paths(
    args: Args,
    sandbox_info: *mut u8,
    profile_paths: Option<BrowserProfilePaths>,
) -> Result<BrowserBootstrap, BrowserError> {
    let started = diagnostic_timer();
    #[cfg(target_os = "macos")]
    let loader = {
        let loader = cef::library_loader::LibraryLoader::new(
            &std::env::current_exe().map_err(|error| BrowserError::Profile(error.into()))?,
            false,
        );
        if !loader.load() {
            return Err(BrowserError::FrameworkLoad);
        }
        loader
    };

    // cef-rs binds the experimental header layout, so declare `CEF_API_VERSION`
    // and never `CEF_API_VERSION_LAST`.
    let _ = api_hash(cef::sys::CEF_API_VERSION, 0);
    let (signal_tx, signal_rx) = async_channel::unbounded();
    let mut app = RuntimeApp::new(
        signal_tx.clone(),
        RuntimeRenderProcessHandler::new(RendererSideRouter::new(element_picker_router_config())),
    );
    let result = execute_process(Some(args.as_main_args()), Some(&mut app), sandbox_info);
    if result >= 0 {
        return Ok(BrowserBootstrap::SubprocessExit(result));
    }
    if result != -1 {
        return Err(BrowserError::ExecuteProcess(result));
    }

    let profile_paths = match profile_paths {
        Some(profile_paths) => profile_paths,
        None => resolve_profile_paths().map_err(|error| BrowserError::Profile(error.into()))?,
    };
    profile_paths.ensure()?;
    let frame_rate_override = configured_browser_frame_rate();
    let windowless_frame_rate = frame_rate_override.unwrap_or(DEFAULT_BROWSER_FRAME_RATE);
    let gpu_enabled = browser_gpu_enabled();
    let shared_texture_setting = std::env::var_os("ZZ_BROWSER_SHARED_TEXTURE");
    let shared_texture_requested = default_enabled_env_flag(shared_texture_setting.as_deref());
    let shared_texture_enabled = shared_texture_requested && gpu_enabled;
    let external_begin_frame_enabled = browser_external_begin_frame_enabled();
    let begin_frame_adaptive_enabled = browser_begin_frame_adaptive_enabled();
    log::info!("CEF OSR frame-rate ceiling: {windowless_frame_rate} FPS");
    if gpu_enabled && !shared_texture_enabled {
        log::info!(
            "CEF GPU process enabled; shared-texture OSR disabled (ZZ_BROWSER_SHARED_TEXTURE=0), using readback OSR"
        );
    }
    if shared_texture_setting.is_some() && shared_texture_requested && !gpu_enabled {
        log::warn!(
            "ZZ_BROWSER_SHARED_TEXTURE is enabled but ZZ_BROWSER_GPU=0; shared textures are disabled and CEF will use software readback OSR"
        );
    } else if shared_texture_enabled {
        log::info!(
            "CEF shared-texture OSR enabled (GPU import/copy-on-receive active with a GPUI device; atomic readback fallback retained)"
        );
    }
    if external_begin_frame_enabled {
        log::info!("CEF external BeginFrame scheduling enabled");
        if begin_frame_adaptive_enabled {
            log::info!("CEF external BeginFrame adaptive throttle enabled");
        } else {
            log::info!("CEF external BeginFrame adaptive throttle disabled");
        }
    }
    log::debug!(
        target: "zz_browser::diagnostics::lifecycle",
        "cold runtime prepared profile_paths={profile_paths:?} windowless_frame_rate={windowless_frame_rate} shared_texture_enabled={shared_texture_enabled} wayland_physical_osr={} elapsed_us={}",
        uses_wayland_physical_osr(),
        diagnostic_elapsed_us(started),
    );

    let active_sessions = Arc::new(AtomicU64::new(0));
    let message_pump = BrowserMessagePump::new(Arc::clone(&active_sessions));
    Ok(BrowserBootstrap::Runtime(BrowserRuntime {
        message_pump,
        signal_tx: signal_tx.clone(),
        signals: signal_rx,
        args,
        app,
        sandbox_info,
        profile_paths,
        profile_contexts: BTreeMap::new(),
        next_session: 0,
        active_sessions,
        active_data_operations: Arc::new(AtomicU64::new(0)),
        windowless_frame_rate,
        frame_rate_override,
        shared_texture_enabled,
        external_begin_frame_enabled,
        begin_frame_adaptive_enabled,
        log_file: None,
        #[cfg(target_os = "macos")]
        _loader: loader,
    }))
}

#[must_use]
pub fn run_subprocess() -> i32 {
    let args = Args::new();

    #[cfg(target_os = "macos")]
    let _sandbox = {
        let mut sandbox = cef::sandbox::Sandbox::new();
        sandbox.initialize(args.as_main_args());
        sandbox
    };

    #[cfg(target_os = "macos")]
    let _loader = {
        let Ok(executable) = std::env::current_exe() else {
            return 1;
        };
        let loader = cef::library_loader::LibraryLoader::new(&executable, true);
        if !loader.load() {
            return 1;
        }
        loader
    };

    // cef-rs binds the experimental header layout, so declare `CEF_API_VERSION`
    // and never `CEF_API_VERSION_LAST`.
    let _ = api_hash(cef::sys::CEF_API_VERSION, 0);
    let (signal_tx, _signal_rx) = async_channel::unbounded();
    let mut app = RuntimeApp::new(
        signal_tx,
        RuntimeRenderProcessHandler::new(RendererSideRouter::new(element_picker_router_config())),
    );
    let result = execute_process(Some(args.as_main_args()), Some(&mut app), ptr::null_mut());
    if result < 0 { 1 } else { result }
}

fn owned_cef_string(value: &CefStringUserfree) -> Option<Arc<str>> {
    let text = CefString::from(value).to_string();
    (!text.is_empty()).then(|| Arc::from(text.as_str()))
}

fn path_to_cef_string(path: &Path) -> CefString {
    CefString::from(path.to_string_lossy().as_ref())
}

fn configured_browser_frame_rate() -> Option<i32> {
    let value = std::env::var_os("ZZ_BROWSER_FPS")?;
    let Ok(value) = value.into_string() else {
        log::warn!("ZZ_BROWSER_FPS is not valid Unicode; using {DEFAULT_BROWSER_FRAME_RATE} FPS");
        return Some(DEFAULT_BROWSER_FRAME_RATE);
    };
    if let Some(frame_rate) = parse_browser_frame_rate(&value) {
        Some(frame_rate)
    } else {
        log::warn!(
            "ZZ_BROWSER_FPS must be an integer from 1 through {MAX_BROWSER_FRAME_RATE}; using {DEFAULT_BROWSER_FRAME_RATE} FPS"
        );
        Some(DEFAULT_BROWSER_FRAME_RATE)
    }
}

fn parse_browser_frame_rate(value: &str) -> Option<i32> {
    let frame_rate = value.trim().parse::<i32>().ok()?;
    (1..=MAX_BROWSER_FRAME_RATE)
        .contains(&frame_rate)
        .then_some(frame_rate)
}

fn default_enabled_env_flag(value: Option<&OsStr>) -> bool {
    value.is_none_or(|value| value != "0")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExternalBeginFramePlatform {
    MacOs,
    LinuxOrFreeBsd,
    Unsupported,
}

fn external_begin_frame_setting_enabled(
    value: Option<&OsStr>,
    platform: ExternalBeginFramePlatform,
) -> bool {
    match platform {
        ExternalBeginFramePlatform::MacOs => default_enabled_env_flag(value),
        ExternalBeginFramePlatform::LinuxOrFreeBsd => value == Some(OsStr::new("1")),
        ExternalBeginFramePlatform::Unsupported => false,
    }
}

fn browser_external_begin_frame_enabled() -> bool {
    let platform = if cfg!(target_os = "macos") {
        ExternalBeginFramePlatform::MacOs
    } else if cfg!(any(target_os = "linux", target_os = "freebsd")) {
        ExternalBeginFramePlatform::LinuxOrFreeBsd
    } else {
        ExternalBeginFramePlatform::Unsupported
    };
    external_begin_frame_setting_enabled(
        std::env::var_os("ZZ_BROWSER_EXTERNAL_BEGIN_FRAME").as_deref(),
        platform,
    )
}

fn begin_frame_adaptive_setting_enabled(value: Option<&OsStr>) -> bool {
    value == Some(OsStr::new("1"))
}

fn browser_begin_frame_adaptive_enabled() -> bool {
    begin_frame_adaptive_setting_enabled(std::env::var_os("ZZ_BROWSER_BF_ADAPTIVE").as_deref())
}

fn browser_gpu_enabled() -> bool {
    default_enabled_env_flag(std::env::var_os("ZZ_BROWSER_GPU").as_deref())
}

// CEF's generated bindings type unscoped C enums as `i32` on MSVC, `u32` elsewhere.
#[cfg(windows)]
fn cef_enum_bits(value: i32) -> u32 {
    value.cast_unsigned()
}

#[cfg(not(windows))]
fn cef_enum_bits(value: u32) -> u32 {
    value
}

fn event_flags(modifiers: Modifiers) -> u32 {
    let mut flags = 0u32;
    if modifiers.shift() {
        flags |= cef_enum_bits(cef::sys::cef_event_flags_t::EVENTFLAG_SHIFT_DOWN.0);
    }
    if modifiers.control() {
        flags |= cef_enum_bits(cef::sys::cef_event_flags_t::EVENTFLAG_CONTROL_DOWN.0);
    }
    if modifiers.alt() {
        flags |= cef_enum_bits(cef::sys::cef_event_flags_t::EVENTFLAG_ALT_DOWN.0);
    }
    if modifiers.platform() {
        flags |= cef_enum_bits(cef::sys::cef_event_flags_t::EVENTFLAG_COMMAND_DOWN.0);
    }
    if modifiers.left_mouse() {
        flags |= cef_enum_bits(cef::sys::cef_event_flags_t::EVENTFLAG_LEFT_MOUSE_BUTTON.0);
    }
    if modifiers.middle_mouse() {
        flags |= cef_enum_bits(cef::sys::cef_event_flags_t::EVENTFLAG_MIDDLE_MOUSE_BUTTON.0);
    }
    if modifiers.right_mouse() {
        flags |= cef_enum_bits(cef::sys::cef_event_flags_t::EVENTFLAG_RIGHT_MOUSE_BUTTON.0);
    }
    if modifiers.is_repeat() {
        flags |= cef_enum_bits(cef::sys::cef_event_flags_t::EVENTFLAG_IS_REPEAT.0);
    }
    flags
}

fn key_event(input: KeyInput) -> KeyEvent {
    let character = key_character(input.key);
    KeyEvent {
        type_: match input.action {
            KeyAction::Press => KeyEventType::RAWKEYDOWN,
            KeyAction::Release => KeyEventType::KEYUP,
        },
        modifiers: event_flags(input.modifiers),
        windows_key_code: input.key.windows_key_code(),
        native_key_code: native_key_code(input.key),
        is_system_key: i32::from(input.modifiers.alt()),
        character,
        unmodified_character: character,
        ..Default::default()
    }
}

fn named_key_character_event(input: KeyInput) -> Option<KeyEvent> {
    if input.action != KeyAction::Press || matches!(input.key, BrowserKey::Character(_)) {
        return None;
    }
    let mut event = key_event(input);
    (event.character != 0).then(|| {
        event.type_ = KeyEventType::CHAR;
        event
    })
}

fn key_character(key: BrowserKey) -> u16 {
    if let BrowserKey::Character(character) = key {
        return character
            .encode_utf16(&mut [0; 2])
            .first()
            .copied()
            .unwrap_or_default();
    }

    #[cfg(target_os = "macos")]
    {
        match key {
            BrowserKey::Backspace => 0x007f,
            BrowserKey::Tab => 0x0009,
            BrowserKey::Enter => 0x000d,
            BrowserKey::Escape => 0x001b,
            BrowserKey::Space => 0x0020,
            BrowserKey::ArrowUp => 0xf700,
            BrowserKey::ArrowDown => 0xf701,
            BrowserKey::ArrowLeft => 0xf702,
            BrowserKey::ArrowRight => 0xf703,
            BrowserKey::Function(number @ 1..=35) => 0xf704 + u16::from(number) - 1,
            BrowserKey::Delete => 0xf728,
            BrowserKey::Home => 0xf729,
            BrowserKey::End => 0xf72b,
            BrowserKey::PageUp => 0xf72c,
            BrowserKey::PageDown => 0xf72d,
            BrowserKey::Insert => 0xf746,
            BrowserKey::Character(_) | BrowserKey::Function(_) | BrowserKey::Unidentified => 0,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = key;
        0
    }
}

#[cfg(target_os = "macos")]
fn native_key_code(key: BrowserKey) -> i32 {
    match key {
        BrowserKey::Enter => 0x24,
        BrowserKey::Tab => 0x30,
        BrowserKey::Space => 0x31,
        BrowserKey::Backspace => 0x33,
        BrowserKey::Escape => 0x35,
        BrowserKey::Function(1) => 0x7a,
        BrowserKey::Function(2) => 0x78,
        BrowserKey::Function(3) => 0x63,
        BrowserKey::Function(4) => 0x76,
        BrowserKey::Function(5) => 0x60,
        BrowserKey::Function(6) => 0x61,
        BrowserKey::Function(7) => 0x62,
        BrowserKey::Function(8) => 0x64,
        BrowserKey::Function(9) => 0x65,
        BrowserKey::Function(10) => 0x6d,
        BrowserKey::Function(11) => 0x67,
        BrowserKey::Function(12) => 0x6f,
        BrowserKey::Function(13) => 0x69,
        BrowserKey::Function(14) => 0x6b,
        BrowserKey::Function(15) => 0x71,
        BrowserKey::Function(16) => 0x6a,
        BrowserKey::Function(17) => 0x40,
        BrowserKey::Function(18) => 0x4f,
        BrowserKey::Function(19) => 0x50,
        BrowserKey::Function(20) => 0x5a,
        BrowserKey::PageUp => 0x74,
        BrowserKey::PageDown => 0x79,
        BrowserKey::End => 0x77,
        BrowserKey::Home => 0x73,
        BrowserKey::ArrowLeft => 0x7b,
        BrowserKey::ArrowUp => 0x7e,
        BrowserKey::ArrowRight => 0x7c,
        BrowserKey::ArrowDown => 0x7d,
        BrowserKey::Insert => 0x72,
        BrowserKey::Delete => 0x75,
        BrowserKey::Character(character) => mac_character_key_code(character),
        BrowserKey::Function(_) | BrowserKey::Unidentified => 0,
    }
}

#[cfg(target_os = "macos")]
#[allow(
    clippy::match_same_arms,
    reason = "macOS assigns virtual key code zero to A, which is also CEF's unavailable-code sentinel"
)]
fn mac_character_key_code(character: char) -> i32 {
    // macOS ANSI virtual key codes; CEF rebuilds Chromium's key code from them.
    match character {
        'a' | 'A' => 0x00,
        's' | 'S' => 0x01,
        'd' | 'D' => 0x02,
        'f' | 'F' => 0x03,
        'h' | 'H' => 0x04,
        'g' | 'G' => 0x05,
        'z' | 'Z' => 0x06,
        'x' | 'X' => 0x07,
        'c' | 'C' => 0x08,
        'v' | 'V' => 0x09,
        'b' | 'B' => 0x0b,
        'q' | 'Q' => 0x0c,
        'w' | 'W' => 0x0d,
        'e' | 'E' => 0x0e,
        'r' | 'R' => 0x0f,
        'y' | 'Y' => 0x10,
        't' | 'T' => 0x11,
        '1' | '!' => 0x12,
        '2' | '@' => 0x13,
        '3' | '#' => 0x14,
        '4' | '$' => 0x15,
        '6' | '^' => 0x16,
        '5' | '%' => 0x17,
        '=' | '+' => 0x18,
        '9' | '(' => 0x19,
        '7' | '&' => 0x1a,
        '-' | '_' => 0x1b,
        '8' | '*' => 0x1c,
        '0' | ')' => 0x1d,
        ']' | '}' => 0x1e,
        'o' | 'O' => 0x1f,
        'u' | 'U' => 0x20,
        '[' | '{' => 0x21,
        'i' | 'I' => 0x22,
        'p' | 'P' => 0x23,
        'l' | 'L' => 0x25,
        'j' | 'J' => 0x26,
        '\'' | '"' => 0x27,
        'k' | 'K' => 0x28,
        ';' | ':' => 0x29,
        '\\' | '|' => 0x2a,
        ',' | '<' => 0x2b,
        '/' | '?' => 0x2c,
        'n' | 'N' => 0x2d,
        'm' | 'M' => 0x2e,
        '.' | '>' => 0x2f,
        '`' | '~' => 0x32,
        ' ' => 0x31,
        _ => 0,
    }
}

#[cfg(not(target_os = "macos"))]
fn native_key_code(_: BrowserKey) -> i32 {
    0
}

fn text_key_event(code_unit: u16) -> KeyEvent {
    KeyEvent {
        type_: KeyEventType::CHAR,
        windows_key_code: i32::from(code_unit),
        character: code_unit,
        unmodified_character: code_unit,
        ..Default::default()
    }
}

fn mouse_button(button: PointerButton) -> MouseButtonType {
    match button {
        PointerButton::Left => MouseButtonType::LEFT,
        PointerButton::Middle => MouseButtonType::MIDDLE,
        PointerButton::Right => MouseButtonType::RIGHT,
    }
}

#[derive(Clone, Default)]
struct AcceleratedPaintTracker(Arc<Mutex<AcceleratedPaintState>>);

#[derive(Default)]
struct AcceleratedPaintState {
    callback_count: u64,
    missing_info_count: u64,
    view_count: u64,
    popup_count: u64,
    handle_transition_count: u64,
    consecutive_handle_reuse_count: u64,
    previous_handle: Option<AcceleratedPaintHandleIdentity>,
    handles: BTreeMap<AcceleratedPaintHandleIdentity, AcceleratedPaintHandleState>,
    last_observation: Option<(u64, AcceleratedPaintObservationState)>,
    gpu_import_attempt_count: u64,
    gpu_frame_delivered_count: u64,
    gpu_import_failure_count: u64,
    gpu_helper_fallback_count: u64,
    stale_pool_frame_count: u64,
    readback_frame_delivered_count: u64,
    latest_pool_generation: u64,
}

struct AcceleratedPaintHandleState {
    use_count: u64,
    first_callback: u64,
    last_callback: u64,
    minimum_reuse_gap: Option<u64>,
    maximum_reuse_gap: Option<u64>,
}

struct RecordedAcceleratedPaint {
    callback: u64,
    unique_handle_count: usize,
    same_as_previous: bool,
    reuse_gap: Option<u64>,
}

#[derive(Clone, Copy)]
struct AcceleratedPaintObservationState {
    paint_element: &'static str,
    width: i32,
    height: i32,
    pixel_format: &'static str,
    pixel_format_raw: u32,
    platform: AcceleratedPaintPlatformState,
}

#[derive(Clone, Copy)]
struct AcceleratedPaintPlatformState {
    drm_modifier: Option<u64>,
    plane_count: i32,
    planes: [Option<AcceleratedPaintPlaneDiagnostics>; 4],
    handle_identity: AcceleratedPaintHandleIdentity,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
type AcceleratedPaintHandleIdentity = usize;

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct AcceleratedPaintHandleIdentity {
    plane_count: u8,
    planes: [AcceleratedPaintPlaneIdentity; 4],
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct AcceleratedPaintPlaneIdentity {
    device: Option<u64>,
    inode: Option<u64>,
    offset: u64,
    size: u64,
}

impl AcceleratedPaintObservationState {
    fn diagnostics(self, callback: u64) -> AcceleratedPaintObservation {
        AcceleratedPaintObservation {
            callback,
            paint_element: self.paint_element.to_owned(),
            width: self.width,
            height: self.height,
            pixel_format: self.pixel_format.to_owned(),
            pixel_format_raw: self.pixel_format_raw,
            drm_modifier: self.platform.drm_modifier,
            plane_count: self.platform.plane_count,
            planes: self.platform.planes.into_iter().flatten().collect(),
            handle_identity: format_accelerated_handle_identity(self.platform.handle_identity),
        }
    }
}

impl AcceleratedPaintTracker {
    fn record(
        &self,
        type_: PaintElementType,
        observation: AcceleratedPaintObservationState,
    ) -> RecordedAcceleratedPaint {
        let mut state = self.0.lock();
        state.callback_count = state.callback_count.wrapping_add(1).max(1);
        let callback = state.callback_count;
        match type_ {
            PaintElementType::VIEW => state.view_count += 1,
            PaintElementType::POPUP => state.popup_count += 1,
            _ => {}
        }

        let identity = observation.platform.handle_identity;
        let same_as_previous = state.previous_handle == Some(identity);
        if state.previous_handle.is_some() {
            if same_as_previous {
                state.consecutive_handle_reuse_count += 1;
            } else {
                state.handle_transition_count += 1;
            }
        }
        state.previous_handle = Some(identity);

        if !state.handles.contains_key(&identity)
            && state.handles.len() >= MAX_TRACKED_ACCELERATED_HANDLES
            && let Some(oldest) = state
                .handles
                .iter()
                .min_by_key(|(_, handle)| handle.last_callback)
                .map(|(identity, _)| *identity)
        {
            state.handles.remove(&oldest);
        }
        let handle = state
            .handles
            .entry(identity)
            .or_insert(AcceleratedPaintHandleState {
                use_count: 0,
                first_callback: callback,
                last_callback: callback,
                minimum_reuse_gap: None,
                maximum_reuse_gap: None,
            });
        let reuse_gap = (handle.use_count != 0).then(|| callback - handle.last_callback);
        if let Some(reuse_gap) = reuse_gap {
            handle.minimum_reuse_gap = Some(
                handle
                    .minimum_reuse_gap
                    .map_or(reuse_gap, |current| current.min(reuse_gap)),
            );
            handle.maximum_reuse_gap = Some(
                handle
                    .maximum_reuse_gap
                    .map_or(reuse_gap, |current| current.max(reuse_gap)),
            );
        }
        handle.use_count += 1;
        handle.last_callback = callback;
        state.last_observation = Some((callback, observation));

        RecordedAcceleratedPaint {
            callback,
            unique_handle_count: state.handles.len(),
            same_as_previous,
            reuse_gap,
        }
    }

    fn record_missing_info(&self, type_: PaintElementType) -> u64 {
        let mut state = self.0.lock();
        state.callback_count = state.callback_count.wrapping_add(1).max(1);
        state.missing_info_count += 1;
        match type_ {
            PaintElementType::VIEW => state.view_count += 1,
            PaintElementType::POPUP => state.popup_count += 1,
            _ => {}
        }
        state.callback_count
    }

    fn record_gpu_import_attempt(&self) {
        self.0.lock().gpu_import_attempt_count += 1;
    }

    fn record_gpu_frame_delivered(&self, pool_generation: u64) {
        let mut state = self.0.lock();
        state.gpu_frame_delivered_count += 1;
        state.latest_pool_generation = pool_generation;
    }

    fn record_gpu_import_failure(&self, helper_fallback: bool) -> u64 {
        let mut state = self.0.lock();
        state.gpu_import_failure_count += 1;
        if helper_fallback {
            state.gpu_helper_fallback_count += 1;
        }
        state.gpu_import_failure_count
    }

    fn record_stale_pool_frame(&self) -> u64 {
        let mut state = self.0.lock();
        state.stale_pool_frame_count += 1;
        state.stale_pool_frame_count
    }

    fn record_readback_frame_delivered(&self) {
        self.0.lock().readback_frame_delivered_count += 1;
    }

    fn diagnostics(&self) -> AcceleratedPaintDiagnostics {
        let state = self.0.lock();
        AcceleratedPaintDiagnostics {
            callback_count: state.callback_count,
            missing_info_count: state.missing_info_count,
            view_count: state.view_count,
            popup_count: state.popup_count,
            unique_handle_count: state.handles.len(),
            handle_transition_count: state.handle_transition_count,
            consecutive_handle_reuse_count: state.consecutive_handle_reuse_count,
            handles: state
                .handles
                .iter()
                .map(|(identity, handle)| AcceleratedPaintHandleDiagnostics {
                    identity: format_accelerated_handle_identity(*identity),
                    use_count: handle.use_count,
                    first_callback: handle.first_callback,
                    last_callback: handle.last_callback,
                    minimum_reuse_gap: handle.minimum_reuse_gap,
                    maximum_reuse_gap: handle.maximum_reuse_gap,
                })
                .collect(),
            last_observation: state
                .last_observation
                .map(|(callback, observation)| observation.diagnostics(callback)),
            gpu_import_attempt_count: state.gpu_import_attempt_count,
            gpu_frame_delivered_count: state.gpu_frame_delivered_count,
            gpu_import_failure_count: state.gpu_import_failure_count,
            gpu_helper_fallback_count: state.gpu_helper_fallback_count,
            stale_pool_frame_count: state.stale_pool_frame_count,
            readback_frame_delivered_count: state.readback_frame_delivered_count,
            latest_pool_generation: state.latest_pool_generation,
        }
    }
}

fn paint_element_name(type_: PaintElementType) -> &'static str {
    match type_ {
        PaintElementType::VIEW => "view",
        PaintElementType::POPUP => "popup",
        _ => "unknown",
    }
}

fn pixel_format_name(format: ColorType) -> &'static str {
    match format {
        ColorType::RGBA_8888 => "rgba_8888",
        ColorType::BGRA_8888 => "bgra_8888",
        _ => "unknown",
    }
}

fn accelerated_paint_observation(
    type_: PaintElementType,
    info: &AcceleratedPaintInfo,
) -> AcceleratedPaintObservationState {
    let platform = accelerated_paint_platform_info(info);
    AcceleratedPaintObservationState {
        paint_element: paint_element_name(type_),
        width: info.extra.coded_size.width,
        height: info.extra.coded_size.height,
        pixel_format: pixel_format_name(info.format),
        pixel_format_raw: cef_enum_bits(info.format.get_raw()),
        platform,
    }
}

#[cfg(target_os = "linux")]
fn accelerated_paint_platform_info(info: &AcceleratedPaintInfo) -> AcceleratedPaintPlatformState {
    use std::{
        fs::File,
        mem::ManuallyDrop,
        os::{fd::FromRawFd as _, unix::fs::MetadataExt as _},
    };

    let plane_count = usize::try_from(info.plane_count)
        .unwrap_or_default()
        .min(info.planes.len());
    let mut planes = [None; 4];
    let mut identity_planes = [AcceleratedPaintPlaneIdentity::default(); 4];
    for (index, plane) in info.planes.iter().take(plane_count).enumerate() {
        planes[index] = Some(AcceleratedPaintPlaneDiagnostics {
            fd: plane.fd,
            stride: plane.stride,
            offset: plane.offset,
            size: plane.size,
        });
        let metadata = (plane.fd >= 0)
            .then(|| {
                // SAFETY: CEF owns this plane descriptor for the callback, and
                // ManuallyDrop keeps File from closing the borrowed fd.
                let file = ManuallyDrop::new(unsafe { File::from_raw_fd(plane.fd) });
                file.metadata()
            })
            .transpose()
            .ok()
            .flatten();
        identity_planes[index] = AcceleratedPaintPlaneIdentity {
            device: metadata.as_ref().map(|metadata| metadata.dev()),
            inode: metadata.as_ref().map(|metadata| metadata.ino()),
            offset: plane.offset,
            size: plane.size,
        };
    }
    AcceleratedPaintPlatformState {
        drm_modifier: Some(info.modifier),
        plane_count: info.plane_count,
        planes,
        handle_identity: AcceleratedPaintHandleIdentity {
            plane_count: u8::try_from(plane_count)
                .expect("CEF accelerated paint exposes at most four planes"),
            planes: identity_planes,
        },
    }
}

#[cfg(target_os = "linux")]
fn format_accelerated_handle_identity(identity: AcceleratedPaintHandleIdentity) -> String {
    use std::fmt::Write as _;

    let mut formatted = String::from("dmabuf[");
    for (index, plane) in identity
        .planes
        .iter()
        .take(usize::from(identity.plane_count))
        .enumerate()
    {
        if index != 0 {
            formatted.push(';');
        }
        if let (Some(device), Some(inode)) = (plane.device, plane.inode) {
            write!(
                formatted,
                "dev={device:x}:ino={inode:x}:offset={}:size={}",
                plane.offset, plane.size
            )
            .expect("writing to a String cannot fail");
        } else {
            write!(
                formatted,
                "dev=unknown:ino=unknown:offset={}:size={}",
                plane.offset, plane.size
            )
            .expect("writing to a String cannot fail");
        }
    }
    formatted.push(']');
    formatted
}

#[cfg(target_os = "macos")]
fn accelerated_paint_platform_info(info: &AcceleratedPaintInfo) -> AcceleratedPaintPlatformState {
    AcceleratedPaintPlatformState {
        drm_modifier: None,
        plane_count: 0,
        planes: [None; 4],
        handle_identity: info.shared_texture_io_surface as usize,
    }
}

#[cfg(target_os = "macos")]
fn format_accelerated_handle_identity(identity: AcceleratedPaintHandleIdentity) -> String {
    format!("iosurface[{identity:#x}]")
}

#[cfg(target_os = "windows")]
fn accelerated_paint_platform_info(info: &AcceleratedPaintInfo) -> AcceleratedPaintPlatformState {
    AcceleratedPaintPlatformState {
        drm_modifier: None,
        plane_count: 0,
        planes: [None; 4],
        handle_identity: info.shared_texture_handle as usize,
    }
}

#[cfg(target_os = "windows")]
fn format_accelerated_handle_identity(identity: AcceleratedPaintHandleIdentity) -> String {
    format!("d3d[{identity:#x}]")
}

const MAX_TRACKED_ACCELERATED_HANDLES: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ExpectedFrameSize {
    logical_width: u32,
    logical_height: u32,
    device_width: i32,
    device_height: i32,
}

impl ExpectedFrameSize {
    fn from_viewport(viewport: Viewport) -> Self {
        // CEF rasters the shared surface at device pixels on macOS and Windows;
        // Wayland folds that scale into `osr_raster_scale` instead.
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let scale_factor = viewport.scale_factor;
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let scale_factor = osr_raster_scale(viewport);
        Self {
            logical_width: viewport.width,
            logical_height: viewport.height,
            device_width: scaled_osr_dimension(viewport.width, scale_factor),
            device_height: scaled_osr_dimension(viewport.height, scale_factor),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AcceleratedPoolLayout {
    width: i32,
    height: i32,
    pixel_format_raw: u32,
}

impl AcceleratedPoolLayout {
    fn from_info(info: &AcceleratedPaintInfo) -> Result<Self, AcceleratedFrameError> {
        frame_byte_len(info.extra.coded_size.width, info.extra.coded_size.height)
            .map_err(|error| AcceleratedFrameError::InvalidMetadata(error.to_string()))?;
        if !matches!(info.format, ColorType::BGRA_8888 | ColorType::RGBA_8888) {
            return Err(AcceleratedFrameError::InvalidMetadata(format!(
                "unsupported pixel format {}",
                info.format.get_raw()
            )));
        }

        #[cfg(target_os = "linux")]
        {
            let plane_count = usize::try_from(info.plane_count).map_err(|_| {
                AcceleratedFrameError::InvalidMetadata(format!(
                    "negative plane count {}",
                    info.plane_count
                ))
            })?;
            if plane_count == 0 || plane_count > info.planes.len() {
                return Err(AcceleratedFrameError::InvalidMetadata(format!(
                    "invalid plane count {} (capacity {})",
                    info.plane_count,
                    info.planes.len()
                )));
            }
            info.planes.iter().take(plane_count).try_for_each(|plane| {
                if plane.fd < 0 || plane.stride == 0 {
                    return Err(AcceleratedFrameError::InvalidMetadata(format!(
                        "invalid DMA-BUF plane fd={} stride={}",
                        plane.fd, plane.stride
                    )));
                }
                if plane.offset > u64::from(u32::MAX) {
                    return Err(AcceleratedFrameError::InvalidMetadata(format!(
                        "DMA-BUF plane offset {} exceeds the importer limit",
                        plane.offset
                    )));
                }
                Ok(())
            })?;
        }

        Ok(Self {
            width: info.extra.coded_size.width,
            height: info.extra.coded_size.height,
            pixel_format_raw: cef_enum_bits(info.format.get_raw()),
        })
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    fn texture_format(&self) -> wgpu::TextureFormat {
        if self.pixel_format_raw == ColorType::BGRA_8888.get_raw() {
            wgpu::TextureFormat::Bgra8Unorm
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        }
    }
}

#[derive(Debug)]
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
struct PoolGenerationBookkeeper {
    expected: ExpectedFrameSize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum StalePoolFrame {
    Dimensions {
        expected_width: i32,
        expected_height: i32,
        actual_width: i32,
        actual_height: i32,
    },
    #[cfg(target_os = "macos")]
    InFlightLimit { limit: usize },
    #[cfg(target_os = "macos")]
    DestinationInFlight { slot: usize, sequence: u64 },
    #[cfg(target_os = "macos")]
    SupersededGeneration { pool_generation: u64 },
    #[cfg(target_os = "macos")]
    OutOfOrder {
        last_published_sequence: u64,
        sequence: u64,
    },
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl PoolGenerationBookkeeper {
    fn new(viewport: Viewport) -> Self {
        Self {
            expected: ExpectedFrameSize::from_viewport(viewport),
        }
    }

    fn set_viewport(&mut self, viewport: Viewport) -> bool {
        let expected = ExpectedFrameSize::from_viewport(viewport);
        if self.expected == expected {
            return false;
        }
        self.expected = expected;
        true
    }

    fn observe(&self, layout: &AcceleratedPoolLayout) -> Result<(), StalePoolFrame> {
        if layout.width != self.expected.device_width
            || layout.height != self.expected.device_height
        {
            return Err(StalePoolFrame::Dimensions {
                expected_width: self.expected.device_width,
                expected_height: self.expected.device_height,
                actual_width: layout.width,
                actual_height: layout.height,
            });
        }
        Ok(())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
struct DestinationTexturePool {
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    textures: [wgpu::Texture; 2],
    next: usize,
    blitter: wgpu::util::TextureBlitter,
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl DestinationTexturePool {
    fn new(device: &wgpu::Device, width: u32, height: u32, format: wgpu::TextureFormat) -> Self {
        let create_texture = |index| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(if index == 0 {
                    "zz browser accelerated OSR destination 0"
                } else {
                    "zz browser accelerated OSR destination 1"
                }),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        Self {
            width,
            height,
            format,
            textures: [create_texture(0), create_texture(1)],
            next: 0,
            blitter: wgpu::util::TextureBlitter::new(device, format),
        }
    }

    fn matches(&self, width: u32, height: u32, format: wgpu::TextureFormat) -> bool {
        self.width == width && self.height == height && self.format == format
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[derive(Clone)]
struct AcceleratedFrameProducer {
    gpu: Option<BrowserGpuContext>,
    state: Arc<Mutex<AcceleratedFrameProducerState>>,
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
struct AcceleratedFrameProducerState {
    pool: PoolGenerationBookkeeper,
    destination_generation: u64,
    destinations: Option<DestinationTexturePool>,
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
struct ProducedGpuFrame {
    logical_width: u32,
    logical_height: u32,
    device_width: i32,
    device_height: i32,
    pool_generation: u64,
    sequence: u64,
    texture: wgpu::Texture,
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
enum AcceleratedFrameOutcome {
    Frame(ProducedGpuFrame),
    Stale(StalePoolFrame),
}

#[derive(Debug, Error)]
enum AcceleratedFrameError {
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    #[error("GPUI's wgpu device context is unavailable")]
    DeviceUnavailable,
    #[error("invalid accelerated-paint metadata: {0}")]
    InvalidMetadata(String),
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    #[error("CEF shared-texture import failed: {0}")]
    Import(#[source] cef::osr_texture_import::TextureImportError),
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    #[error("CEF's import helper returned its blank fallback texture")]
    HelperFallback,
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    #[error("wgpu rejected the accelerated frame: {0}")]
    WgpuValidation(String),
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl AcceleratedFrameError {
    fn is_helper_fallback(&self) -> bool {
        matches!(self, Self::HelperFallback)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl AcceleratedFrameProducer {
    fn new(gpu: Option<BrowserGpuContext>, viewport: Viewport) -> Self {
        Self {
            gpu,
            state: Arc::new(Mutex::new(AcceleratedFrameProducerState {
                pool: PoolGenerationBookkeeper::new(viewport),
                destination_generation: 0,
                destinations: None,
            })),
        }
    }

    fn set_viewport(&self, viewport: Viewport) {
        let mut state = self.state.lock();
        if state.pool.set_viewport(viewport) {
            state.destinations = None;
        }
    }

    fn produce(
        &self,
        info: &AcceleratedPaintInfo,
        sequence: u64,
    ) -> Result<AcceleratedFrameOutcome, AcceleratedFrameError> {
        let layout = AcceleratedPoolLayout::from_info(info)?;
        let gpu = self
            .gpu
            .as_ref()
            .ok_or(AcceleratedFrameError::DeviceUnavailable)?;
        let mut state = self.state.lock();
        match state.pool.observe(&layout) {
            Ok(()) => {}
            Err(stale_frame) => return Ok(AcceleratedFrameOutcome::Stale(stale_frame)),
        }

        let import_scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let imported = cef::osr_texture_import::SharedTextureHandle::new(info)
            .import_texture(&gpu.device)
            .map_err(AcceleratedFrameError::Import);
        let import_validation = pollster::block_on(import_scope.pop());
        if let Some(error) = import_validation {
            return Err(AcceleratedFrameError::WgpuValidation(error.to_string()));
        }
        let imported = imported?;
        // cef-rs's import helper returns a blank COPY_DST-only fallback; real imports never do.
        if imported.usage().contains(wgpu::TextureUsages::COPY_DST)
            && !imported.usage().contains(wgpu::TextureUsages::COPY_SRC)
        {
            return Err(AcceleratedFrameError::HelperFallback);
        }

        let width = layout.width.cast_unsigned();
        let height = layout.height.cast_unsigned();
        let format = layout.texture_format();
        if imported.width() != width || imported.height() != height || imported.format() != format {
            return Err(AcceleratedFrameError::InvalidMetadata(format!(
                "imported texture is {}x{} {:?}, callback reports {}x{} {:?}",
                imported.width(),
                imported.height(),
                imported.format(),
                width,
                height,
                format
            )));
        }

        if state
            .destinations
            .as_ref()
            .is_none_or(|destinations| !destinations.matches(width, height, format))
        {
            state.destination_generation = state.destination_generation.wrapping_add(1).max(1);
            state.destinations = Some(DestinationTexturePool::new(
                &gpu.device,
                width,
                height,
                format,
            ));
        }

        let pool_generation = state.destination_generation;
        let destinations = state
            .destinations
            .as_mut()
            .expect("destination textures were initialized");
        let destination_index = destinations.next;
        let destination = destinations.textures[destination_index].clone();
        let blit_scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("zz browser accelerated OSR copy-on-receive"),
            });
        if imported.usage().contains(wgpu::TextureUsages::COPY_SRC) {
            encoder.copy_texture_to_texture(
                imported.as_image_copy(),
                destination.as_image_copy(),
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
        } else {
            let source_view = imported.create_view(&wgpu::TextureViewDescriptor::default());
            let destination_view = destination.create_view(&wgpu::TextureViewDescriptor::default());
            destinations
                .blitter
                .copy(&gpu.device, &mut encoder, &source_view, &destination_view);
        }
        gpu.queue.submit([encoder.finish()]);
        if let Some(error) = pollster::block_on(blit_scope.pop()) {
            state.destinations = None;
            return Err(AcceleratedFrameError::WgpuValidation(error.to_string()));
        }
        destinations.next = (destination_index + 1) % destinations.textures.len();
        let expected = state.pool.expected;

        Ok(AcceleratedFrameOutcome::Frame(ProducedGpuFrame {
            logical_width: expected.logical_width,
            logical_height: expected.logical_height,
            device_width: layout.width,
            device_height: layout.height,
            pool_generation,
            sequence,
            texture: destination,
        }))
    }
}

#[derive(Clone)]
struct SessionBridge {
    id: SessionId,
    events: Sender<BrowserEvent>,
    viewport: Arc<Mutex<Viewport>>,
    page_zoom_factor: Arc<Mutex<f64>>,
    mailbox: FrameMailbox,
    invalid_frames: Arc<AtomicU64>,
    accelerated_paint: AcceleratedPaintTracker,
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    accelerated_frames: AcceleratedFrameProducer,
    #[cfg(target_os = "macos")]
    metal_frames: MetalFrameProducer,
    #[cfg(target_os = "windows")]
    d3d11_frames: D3d11FrameProducer,
    shared_texture_fallback_notified: Arc<AtomicBool>,
    element_pick: ElementPickState,
    pending_capture: PendingCapture,
}

type PendingCapture = Arc<Mutex<Option<Registration>>>;

impl SessionBridge {
    fn emit(&self, event: BrowserEvent) {
        let _ = self.events.try_send(event);
    }

    fn apply_page_zoom(&self, browser: &Browser) {
        if let Some(host) = browser.host() {
            host.set_zoom_level(effective_chromium_zoom_level(
                *self.viewport.lock(),
                *self.page_zoom_factor.lock(),
            ));
        }
    }

    fn start_element_screenshot(
        &self,
        browser: &Browser,
        text: &Arc<str>,
        geometry: PickGeometry,
    ) -> bool {
        let Some(clip) = element_screenshot_clip(geometry) else {
            return false;
        };
        let Some(host) = browser.host() else {
            return false;
        };
        let Some(mut params) = screenshot_params(clip) else {
            return false;
        };
        let expected_id = Arc::new(AtomicI32::new(0));
        let mut observer = ElementScreenshotObserver::new(
            Arc::clone(&expected_id),
            Arc::new(AtomicBool::new(false)),
            self.clone(),
            Arc::clone(text),
        );
        let Some(registration) = host.add_dev_tools_message_observer(Some(&mut observer)) else {
            return false;
        };
        let method = CefString::from("Page.captureScreenshot");
        let message_id = host.execute_dev_tools_method(0, Some(&method), Some(&mut params));
        if message_id == 0 {
            return false;
        }
        expected_id.store(message_id, Ordering::Release);
        // CEF runs observers on this thread, so releasing the previous one here is safe.
        *self.pending_capture.lock() = Some(registration);
        true
    }

    fn context_menu_request(&self, params: &ContextMenuParams) -> ContextMenuRequest {
        use cef::sys::cef_context_menu_edit_state_flags_t as EditState;

        let scale_factor = osr_raster_scale(*self.viewport.lock());
        let flags = params.edit_state_flags().as_ref().0;
        let can = |flag: EditState| flags & flag.0 != 0;
        ContextMenuRequest {
            x: unscaled_osr_coordinate(params.xcoord(), scale_factor),
            y: unscaled_osr_coordinate(params.ycoord(), scale_factor),
            link_url: owned_cef_string(&params.link_url()),
            selection_text: owned_cef_string(&params.selection_text()),
            editable: params.is_editable() != 0,
            edit_flags: EditFlags {
                can_cut: can(EditState::CM_EDITFLAG_CAN_CUT),
                can_copy: can(EditState::CM_EDITFLAG_CAN_COPY),
                can_paste: can(EditState::CM_EDITFLAG_CAN_PASTE),
                can_select_all: can(EditState::CM_EDITFLAG_CAN_SELECT_ALL),
            },
        }
    }

    fn log_invalid_frame(&self, error: impl std::fmt::Display) {
        let count = self.invalid_frames.fetch_add(1, Ordering::Relaxed) + 1;
        if count.is_power_of_two() {
            log::warn!("discarded invalid CEF frame #{count}: {error}");
        }
    }

    fn cancel_element_pick(&self) {
        if self.element_pick.cancel() {
            self.emit(BrowserEvent::ElementPickCancelled { session: self.id });
        }
    }

    fn request_shared_texture_fallback(&self, reason: impl Into<Arc<str>>) {
        if !self
            .shared_texture_fallback_notified
            .swap(true, Ordering::AcqRel)
        {
            self.emit(BrowserEvent::SharedTextureFailed {
                session: self.id,
                reason: reason.into(),
            });
        }
    }

    fn shared_texture_fallback_requested(&self) -> bool {
        self.shared_texture_fallback_notified
            .load(Ordering::Acquire)
    }

    #[cfg(target_os = "macos")]
    fn record_stale_metal_frame(&self, sequence: u64, reason: &StalePoolFrame) {
        let count = self.accelerated_paint.record_stale_pool_frame();
        if count.is_power_of_two() {
            log::warn!(
                target: "zz_browser::accelerated_paint",
                "discarded stale macOS accelerated frame #{count} session={} callback={sequence} reason={reason:?}",
                self.id.0,
            );
        }
    }

    #[cfg(target_os = "macos")]
    fn record_metal_frame_failure(&self, sequence: u64, error: &MetalFrameError) {
        self.mailbox.record_gpu_import_failure();
        self.request_shared_texture_fallback(error.to_string());
        let count = self.accelerated_paint.record_gpu_import_failure(false);
        if count.is_power_of_two() {
            log::warn!(
                target: "zz_browser::accelerated_paint",
                "macOS Metal-IOSurface import/blit failed #{count} session={} callback={sequence}: {error}; retaining the last valid frame while awaiting readback fallback",
                self.id.0,
            );
        }
    }

    #[cfg(target_os = "macos")]
    fn publish_metal_frame(&self, frame: metal_osr::ProducedMetalFrame) {
        let pool_generation = frame.pool_generation;
        let sequence = frame.sequence;
        let device_width = frame.device_width;
        let device_height = frame.device_height;
        let publish_result = self.mailbox.publish_mac_gpu(MacGpuFrameSubmission {
            session: self.id,
            logical_width: frame.logical_width,
            logical_height: frame.logical_height,
            device_width,
            device_height,
            pool_generation,
            sequence,
            io_surface: frame.io_surface,
        });
        match publish_result {
            Ok(wake) => {
                self.accelerated_paint
                    .record_gpu_frame_delivered(pool_generation);
                log::trace!(
                    target: "zz_browser::accelerated_paint",
                    "delivered macOS Metal-IOSurface frame session={} callback={} pool_generation={} device={}x{} wake={wake:?}",
                    self.id.0,
                    sequence,
                    pool_generation,
                    device_width,
                    device_height,
                );
                if let Some(generation) = wake {
                    self.emit(BrowserEvent::FrameReady {
                        session: self.id,
                        generation,
                    });
                }
            }
            Err(error) => {
                self.mailbox.record_gpu_import_failure();
                self.request_shared_texture_fallback(error.to_string());
                self.accelerated_paint.record_gpu_import_failure(false);
                self.log_invalid_frame(error);
            }
        }
    }

    #[cfg(target_os = "windows")]
    fn record_stale_d3d11_frame(&self, sequence: u64, reason: &StalePoolFrame) {
        let count = self.accelerated_paint.record_stale_pool_frame();
        if count.is_power_of_two() {
            log::warn!(
                target: "zz_browser::accelerated_paint",
                "discarded stale Windows accelerated frame #{count} session={} callback={sequence} reason={reason:?}",
                self.id.0,
            );
        }
    }

    #[cfg(target_os = "windows")]
    fn record_d3d11_frame_failure(&self, sequence: u64, error: &D3d11FrameError) {
        self.mailbox.record_gpu_import_failure();
        self.request_shared_texture_fallback(error.to_string());
        let count = self.accelerated_paint.record_gpu_import_failure(false);
        if count.is_power_of_two() {
            log::warn!(
                target: "zz_browser::accelerated_paint",
                "Windows D3D11 shared-texture import/copy failed #{count} session={} callback={sequence}: {error}; retaining the last valid frame while awaiting readback fallback",
                self.id.0,
            );
        }
    }

    #[cfg(target_os = "windows")]
    fn publish_d3d11_frame(&self, frame: d3d11_osr::ProducedD3d11Frame) {
        let pool_generation = frame.pool_generation;
        let sequence = frame.sequence;
        let device_width = frame.device_width;
        let device_height = frame.device_height;
        let publish_result = self.mailbox.publish_win_gpu(WinGpuFrameSubmission {
            session: self.id,
            logical_width: frame.logical_width,
            logical_height: frame.logical_height,
            device_width,
            device_height,
            pool_generation,
            sequence,
            texture: frame.texture,
        });
        match publish_result {
            Ok(wake) => {
                self.accelerated_paint
                    .record_gpu_frame_delivered(pool_generation);
                log::trace!(
                    target: "zz_browser::accelerated_paint",
                    "delivered Windows D3D11 frame session={} callback={} pool_generation={} device={}x{} wake={wake:?}",
                    self.id.0,
                    sequence,
                    pool_generation,
                    device_width,
                    device_height,
                );
                if let Some(generation) = wake {
                    self.emit(BrowserEvent::FrameReady {
                        session: self.id,
                        generation,
                    });
                }
            }
            Err(error) => {
                self.mailbox.record_gpu_import_failure();
                self.request_shared_texture_fallback(error.to_string());
                self.accelerated_paint.record_gpu_import_failure(false);
                self.log_invalid_frame(error);
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn handle_metal_frame_completion(&self, completion: MetalFrameCompletion) {
        if self.shared_texture_fallback_requested() {
            return;
        }
        match completion {
            MetalFrameCompletion::Frame(frame) => self.publish_metal_frame(frame),
            MetalFrameCompletion::Stale { sequence, reason } => {
                self.record_stale_metal_frame(sequence, &reason);
            }
            MetalFrameCompletion::Failed { sequence, error } => {
                self.record_metal_frame_failure(sequence, &error);
            }
        }
    }
}

struct ElementPickerQueryHandler {
    bridge: SessionBridge,
}

impl BrowserSideHandler for ElementPickerQueryHandler {
    fn on_query_str(
        &self,
        browser: Option<Browser>,
        frame: Option<Frame>,
        _query_id: i64,
        request: &str,
        persistent: bool,
        callback: Arc<StdMutex<dyn BrowserSideCallback>>,
    ) -> bool {
        let reject = |code: i32, message: &str| {
            if let Ok(callback) = callback.lock() {
                callback.failure(code, message);
            }
        };
        if persistent {
            reject(400, "persistent element picker queries are not supported");
            return true;
        }
        if frame.is_none_or(|frame| frame.is_main() == 0) {
            reject(
                403,
                "element picker results are accepted only from the main frame",
            );
            return true;
        }

        match self.bridge.element_pick.consume(request) {
            Ok(outcome) => {
                if let Ok(callback) = callback.lock() {
                    callback.success_str("");
                }
                let event = match outcome {
                    ElementPickOutcome::Picked(text, geometry) => {
                        if let Some((browser, geometry)) = browser.as_ref().zip(geometry)
                            && self
                                .bridge
                                .start_element_screenshot(browser, &text, geometry)
                        {
                            return true;
                        }
                        BrowserEvent::ElementPicked {
                            session: self.bridge.id,
                            text,
                            screenshot: None,
                        }
                    }
                    ElementPickOutcome::Cancelled => BrowserEvent::ElementPickCancelled {
                        session: self.bridge.id,
                    },
                    ElementPickOutcome::Failed => BrowserEvent::ElementPickFailed {
                        session: self.bridge.id,
                    },
                };
                self.bridge.emit(event);
            }
            Err(error) => {
                log::debug!(
                    target: "zz_browser::diagnostics::element_picker",
                    "rejected element picker query for session {}: {error}",
                    self.bridge.id.0,
                );
                reject(400, &error.to_string());
            }
        }
        true
    }
}

cef::wrap_app! {
    struct RuntimeApp {
        signals: Sender<RuntimeSignal>,
        render_process_handler: RenderProcessHandler,
    }

    impl App {
        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(RuntimeBrowserProcessHandler::new(self.signals.clone()))
        }

        fn render_process_handler(&self) -> Option<RenderProcessHandler> {
            Some(self.render_process_handler.clone())
        }

        fn on_before_command_line_processing(
            &self,
            process_type: Option<&CefString>,
            command_line: Option<&mut CommandLine>,
        ) {
            let Some(command_line) = command_line else {
                return;
            };
            for switch in [
                "no-first-run",
                "no-default-browser-check",
                "disable-component-update",
                "disable-breakpad",
                "hide-crash-restore-bubble",
            ] {
                command_line.append_switch(Some(&CefString::from(switch)));
            }
            let is_browser_process = process_type.is_none_or(|value| value.to_string().is_empty());
            if is_browser_process {
                // Chromium 151's Immersive Reading Mode crashes the browser process on
                // any SPA navigation in a windowless WebContents.
                let disable_features = CefString::from("disable-features");
                let existing =
                    CefString::from(&command_line.switch_value(Some(&disable_features)))
                        .to_string();
                let merged = if existing.is_empty() {
                    "ImmersiveReadAnything".to_owned()
                } else {
                    format!("{existing},ImmersiveReadAnything")
                };
                command_line.append_switch_with_value(
                    Some(&disable_features),
                    Some(&CefString::from(merged.as_str())),
                );
                #[cfg(all(target_os = "macos", debug_assertions))]
                {
                    // Ad-hoc debug signatures re-prompt for the real Safe Storage item,
                    // so debug profiles use Chromium's deterministic test key.
                    command_line
                        .append_switch(Some(&CefString::from("use-mock-keychain")));
                }
                if !browser_gpu_enabled() {
                    command_line.append_switch(Some(&CefString::from("disable-gpu")));
                    command_line
                        .append_switch(Some(&CefString::from("disable-gpu-compositing")));
                }
            }
            #[cfg(target_os = "linux")]
            {
                command_line.append_switch_with_value(
                    Some(&CefString::from("ozone-platform-hint")),
                    Some(&CefString::from("auto")),
                );
                // Dev bundles cannot carry the root-owned setuid helper bit; only the
                // legacy setuid layer goes, and the user-namespace sandbox stays on.
                command_line.append_switch(Some(&CefString::from("disable-setuid-sandbox")));
            }
        }
    }
}

cef::wrap_render_process_handler! {
    struct RuntimeRenderProcessHandler {
        router: Arc<RendererSideRouter>,
    }

    impl RenderProcessHandler {
        fn on_context_created(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            context: Option<&mut V8Context>,
        ) {
            self.router.on_context_created(
                browser.as_deref().cloned(),
                frame.as_deref().cloned(),
                context.as_deref().cloned(),
            );
            let Some(frame) = frame else {
                return;
            };
            if frame.is_main() == 0 {
                return;
            }
            frame.execute_java_script(
                Some(&CefString::from(ELEMENT_PICKER_SCRIPT)),
                Some(&CefString::from(ELEMENT_PICKER_SCRIPT_URL)),
                1,
            );
        }

        fn on_context_released(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            context: Option<&mut V8Context>,
        ) {
            self.router.on_context_released(
                browser.as_deref().cloned(),
                frame.as_deref().cloned(),
                context.as_deref().cloned(),
            );
        }

        fn on_process_message_received(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            source_process: ProcessId,
            message: Option<&mut ProcessMessage>,
        ) -> i32 {
            i32::from(self.router.on_process_message_received(
                browser.as_deref().cloned(),
                frame.as_deref().cloned(),
                Some(source_process),
                message.as_deref().cloned(),
            ))
        }
    }
}

cef::wrap_browser_process_handler! {
    struct RuntimeBrowserProcessHandler {
        signals: Sender<RuntimeSignal>,
    }

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            let _ = self.signals.try_send(RuntimeSignal::ContextInitialized);
        }

        fn on_schedule_message_pump_work(&self, delay_ms: i64) {
            let _ = self
                .signals
                .try_send(RuntimeSignal::ScheduleMessagePump(delay_ms));
        }
    }
}

cef::wrap_request_context_handler! {
    struct ProfileRequestContextHandler {
        signals: Sender<RuntimeSignal>,
        profile: Arc<str>,
    }

    impl RequestContextHandler {
        fn on_request_context_initialized(&self, _request_context: Option<&mut RequestContext>) {
            let _ = self
                .signals
                .try_send(RuntimeSignal::RequestContextInitialized {
                    profile: Arc::clone(&self.profile),
                });
        }
    }
}

cef::wrap_set_cookie_callback! {
    struct ImportCookieCallback {
        progress: Arc<Mutex<CookieImportProgress>>,
    }

    impl SetCookieCallback {
        fn on_complete(&self, success: i32) {
            finish_cookie_import(&self.progress, success != 0);
        }
    }
}

cef::wrap_completion_callback! {
struct CookieImportFlushCallback {
        result: CookieImportResult,
        results: Sender<CookieImportResult>,
        operation: ActiveDataOperation,
    }

    impl CompletionCallback {
        fn on_complete(&self) {
            let mut result = self.result;
            result.persisted = true;
            let _ = self.results.try_send(result);
            self.operation.finish();
        }
    }
}

cef::wrap_dev_tools_message_observer! {
    struct ElementScreenshotObserver {
        expected_id: Arc<AtomicI32>,
        emitted: Arc<AtomicBool>,
        bridge: SessionBridge,
        text: Arc<str>,
    }

    impl DevToolsMessageObserver {
        fn on_dev_tools_method_result(
            &self,
            _browser: Option<&mut Browser>,
            message_id: i32,
            success: i32,
            result: Option<&[u8]>,
        ) {
            if self.expected_id.load(Ordering::Acquire) != message_id {
                return;
            }
            // A protocol error also lands here, with the error body as result.
            let screenshot = (success != 0)
                .then(|| result.and_then(decode_screenshot_result))
                .flatten();
            self.announce(screenshot);
        }

        fn on_dev_tools_agent_detached(&self, _browser: Option<&mut Browser>) {
            self.announce(None);
        }
    }
}

impl ElementScreenshotObserver {
    fn announce(&self, screenshot: Option<Arc<[u8]>>) {
        if self.emitted.swap(true, Ordering::AcqRel) {
            return;
        }
        self.bridge.emit(BrowserEvent::ElementPicked {
            session: self.bridge.id,
            text: Arc::clone(&self.text),
            screenshot,
        });
    }
}

cef::wrap_dev_tools_message_observer! {
struct SiteDataClearObserver {
        expected_id: Arc<AtomicI32>,
        results: Sender<SiteDataClearResult>,
        operation: ActiveDataOperation,
    }

    impl DevToolsMessageObserver {
        fn on_dev_tools_method_result(
            &self,
            _browser: Option<&mut Browser>,
            message_id: i32,
            success: i32,
            _result: Option<&[u8]>,
        ) {
            if self.expected_id.load(Ordering::Acquire) == message_id
                && self.operation.finish()
            {
                let _ = self.results.try_send(SiteDataClearResult {
                    message_id,
                    success: success != 0,
                });
            }
        }

        fn on_dev_tools_agent_detached(&self, _browser: Option<&mut Browser>) {
            if self.operation.finish() {
                let _ = self.results.try_send(SiteDataClearResult {
                    message_id: self.expected_id.load(Ordering::Acquire),
                    success: false,
                });
            }
        }
    }
}

cef::wrap_render_handler! {
    struct RenderHandlerBuilder {
        bridge: SessionBridge,
    }

    impl RenderHandler {
        fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
            if let Some(rect) = rect {
                let viewport = self.bridge.viewport.lock();
                let scale_factor = osr_raster_scale(*viewport);
                rect.x = 0;
                rect.y = 0;
                rect.width = scaled_osr_dimension(viewport.width, scale_factor);
                rect.height = scaled_osr_dimension(viewport.height, scale_factor);
            }
        }

        fn screen_info(
            &self,
            _browser: Option<&mut Browser>,
            info: Option<&mut ScreenInfo>,
        ) -> i32 {
            let Some(info) = info else {
                return 0;
            };
            let viewport = *self.bridge.viewport.lock();
            info.device_scale_factor = if uses_wayland_physical_osr() {
                // The Wayland host already rasters at physical size; reporting
                // the scale again would scale the OSR surface twice.
                1.0
            } else {
                viewport.scale_factor
            };
            1
        }

        fn screen_point(
            &self,
            _browser: Option<&mut Browser>,
            view_x: i32,
            view_y: i32,
            screen_x: Option<&mut i32>,
            screen_y: Option<&mut i32>,
        ) -> i32 {
            let viewport = self.bridge.viewport.lock();
            if let Some(screen_x) = screen_x {
                *screen_x = if uses_wayland_physical_osr() {
                    scaled_osr_coordinate(viewport.screen_x, viewport.scale_factor)
                        .saturating_add(view_x)
                } else {
                    viewport.screen_x.saturating_add(view_x)
                };
            }
            if let Some(screen_y) = screen_y {
                *screen_y = if uses_wayland_physical_osr() {
                    scaled_osr_coordinate(viewport.screen_y, viewport.scale_factor)
                        .saturating_add(view_y)
                } else {
                    viewport.screen_y.saturating_add(view_y)
                };
            }
            1
        }

        fn on_paint(
            &self,
            _browser: Option<&mut Browser>,
            type_: PaintElementType,
            dirty_rects: Option<&[Rect]>,
            buffer: *const u8,
            width: i32,
            height: i32,
        ) {
            let started = diagnostic_timer();
            if type_ != PaintElementType::VIEW || buffer.is_null() {
                return;
            }
            let length = match frame_byte_len(width, height) {
                Ok(length) => length,
                Err(error) => {
                    self.bridge.log_invalid_frame(error);
                    return;
                }
            };
            // SAFETY: CEF guarantees `buffer` holds `width * height * 4` BGRA bytes
            // for the length of this callback, and the dimensions were checked above.
            let copy_started = diagnostic_timer();
            let source = unsafe { std::slice::from_raw_parts(buffer, length) };
            let mut bytes = self.bridge.mailbox.take_buffer(length);
            bytes.clear();
            bytes.extend_from_slice(source);
            let damage = dirty_rect_damage(dirty_rects);
            let publish_result = self
                .bridge
                .mailbox
                .publish(self.bridge.id, width, height, bytes, damage);
            log::trace!(
                target: "zz_browser::diagnostics::cef_paint",
                "on_paint session={} width={width} height={height} bytes={length} dirty_rects={:?} viewport={:?} copy_us={} publish_result={publish_result:?} elapsed_us={}",
                self.bridge.id.0,
                dirty_rects.map(|rects| rects.iter().map(|rect| (rect.x, rect.y, rect.width, rect.height)).collect::<Vec<_>>()),
                *self.bridge.viewport.lock(),
                diagnostic_elapsed_us(copy_started),
                diagnostic_elapsed_us(started),
            );
            match publish_result {
                Ok(Some(generation)) => {
                    self.bridge
                        .accelerated_paint
                        .record_readback_frame_delivered();
                    self.bridge.emit(BrowserEvent::FrameReady {
                        session: self.bridge.id,
                        generation,
                    });
                }
                Ok(None) => self
                    .bridge
                    .accelerated_paint
                    .record_readback_frame_delivered(),
                Err(error) => self.bridge.log_invalid_frame(error),
            }
        }

        fn on_accelerated_paint(
            &self,
            _browser: Option<&mut Browser>,
            type_: PaintElementType,
            dirty_rects: Option<&[Rect]>,
            info: Option<&AcceleratedPaintInfo>,
        ) {
            let Some(info) = info else {
                let callback = self.bridge.accelerated_paint.record_missing_info(type_);
                log::warn!(
                    target: "zz_browser::accelerated_paint",
                    "on_accelerated_paint session={} callback={callback} paint_element={} paint_element_raw={} info=missing",
                    self.bridge.id.0,
                    paint_element_name(type_),
                    type_.get_raw(),
                );
                return;
            };
            let observation = accelerated_paint_observation(type_, info);
            let recorded = self
                .bridge
                .accelerated_paint
                .record(type_, observation);
            if log::log_enabled!(
                target: "zz_browser::accelerated_paint",
                log::Level::Trace
            ) {
                let observation = observation.diagnostics(recorded.callback);
                let drm_modifier = observation.drm_modifier.map_or_else(
                    || "n/a".to_owned(),
                    |modifier| format!("{modifier:#018x}"),
                );
                log::trace!(
                    target: "zz_browser::accelerated_paint",
                    "on_accelerated_paint session={} callback={} paint_element={} paint_element_raw={} width={} height={} pixel_format={} pixel_format_raw={} drm_modifier={} plane_count={} planes={:?} handle_identity={:?} unique_handles={} same_as_previous={} reuse_gap_callbacks={:?} dirty_rects={:?}",
                    self.bridge.id.0,
                    recorded.callback,
                    observation.paint_element,
                    type_.get_raw(),
                    observation.width,
                    observation.height,
                    observation.pixel_format,
                    observation.pixel_format_raw,
                    drm_modifier,
                    observation.plane_count,
                    observation.planes,
                    observation.handle_identity,
                    recorded.unique_handle_count,
                    recorded.same_as_previous,
                    recorded.reuse_gap,
                    dirty_rects.map(|rects| rects.iter().map(|rect| (rect.x, rect.y, rect.width, rect.height)).collect::<Vec<_>>()),
                );
            }
            if type_ != PaintElementType::VIEW {
                return;
            }
            // The accelerated session lives on until CEF acknowledges close, so late
            // GPU frames must not replace the surface held for the readback swap.
            if self.bridge.shared_texture_fallback_requested() {
                return;
            }

            #[cfg(target_os = "macos")]
            {
                self.bridge.accelerated_paint.record_gpu_import_attempt();
                let bridge = self.bridge.clone();
                match self.bridge.metal_frames.produce(
                    info,
                    recorded.callback,
                    move |completion| bridge.handle_metal_frame_completion(completion),
                ) {
                    Ok(MetalFrameOutcome::Submitted) => {}
                    Ok(MetalFrameOutcome::Stale(stale)) => {
                        self.bridge
                            .record_stale_metal_frame(recorded.callback, &stale);
                    }
                    Err(error) => {
                        self.bridge
                            .record_metal_frame_failure(recorded.callback, &error);
                    }
                }
            }

            #[cfg(target_os = "windows")]
            {
                self.bridge.accelerated_paint.record_gpu_import_attempt();
                // CEF returns the shared texture to its pool once this callback
                // returns, so both the import and the copy happen here.
                match self.bridge.d3d11_frames.produce(info, recorded.callback) {
                    Ok(D3d11FrameOutcome::Frame(frame)) => {
                        self.bridge.publish_d3d11_frame(frame);
                    }
                    Ok(D3d11FrameOutcome::Stale(stale)) => {
                        self.bridge
                            .record_stale_d3d11_frame(recorded.callback, &stale);
                    }
                    Err(error) => {
                        self.bridge
                            .record_d3d11_frame_failure(recorded.callback, &error);
                    }
                }
            }

            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            {
            self.bridge.accelerated_paint.record_gpu_import_attempt();
            match self
                .bridge
                .accelerated_frames
                .produce(info, recorded.callback)
            {
                Ok(AcceleratedFrameOutcome::Frame(frame)) => {
                    let pool_generation = frame.pool_generation;
                    let sequence = frame.sequence;
                    let device_width = frame.device_width;
                    let device_height = frame.device_height;
                    let publish_result = self.bridge.mailbox.publish_gpu(GpuFrameSubmission {
                        session: self.bridge.id,
                        logical_width: frame.logical_width,
                        logical_height: frame.logical_height,
                        device_width,
                        device_height,
                        pool_generation,
                        sequence,
                        texture: frame.texture,
                    });
                    match publish_result {
                        Ok(wake) => {
                            self.bridge
                                .accelerated_paint
                                .record_gpu_frame_delivered(pool_generation);
                            log::trace!(
                                target: "zz_browser::accelerated_paint",
                                "delivered GPU frame session={} callback={} pool_generation={} device={}x{} wake={wake:?}",
                                self.bridge.id.0,
                                sequence,
                                pool_generation,
                                device_width,
                                device_height,
                            );
                            if let Some(generation) = wake {
                                self.bridge.emit(BrowserEvent::FrameReady {
                                    session: self.bridge.id,
                                    generation,
                                });
                            }
                        }
                        Err(error) => {
                            self.bridge.mailbox.record_gpu_import_failure();
                            self.bridge
                                .request_shared_texture_fallback(error.to_string());
                            self.bridge
                                .accelerated_paint
                                .record_gpu_import_failure(false);
                            self.bridge.log_invalid_frame(error);
                        }
                    }
                }
                Ok(AcceleratedFrameOutcome::Stale(stale)) => {
                    let count = self
                        .bridge
                        .accelerated_paint
                        .record_stale_pool_frame();
                    if count.is_power_of_two() {
                        log::warn!(
                            target: "zz_browser::accelerated_paint",
                            "discarded stale accelerated frame #{count} session={} callback={} reason={stale:?}",
                            self.bridge.id.0,
                            recorded.callback,
                        );
                    }
                }
                Err(error) => {
                    self.bridge.mailbox.record_gpu_import_failure();
                    self.bridge
                        .request_shared_texture_fallback(error.to_string());
                    let count = self
                        .bridge
                        .accelerated_paint
                        .record_gpu_import_failure(error.is_helper_fallback());
                    if count.is_power_of_two() {
                        log::warn!(
                            target: "zz_browser::accelerated_paint",
                            "accelerated frame import/blit failed #{count} session={} callback={}: {error}; retaining the last valid frame while awaiting readback fallback",
                            self.bridge.id.0,
                            recorded.callback,
                        );
                    }
                }
            }
            }
        }

        fn start_dragging(
            &self,
            _browser: Option<&mut Browser>,
            _drag_data: Option<&mut DragData>,
            _allowed_ops: DragOperationsMask,
            _x: i32,
            _y: i32,
        ) -> i32 {
            0
        }
    }
}

cef::wrap_display_handler! {
    struct DisplayHandlerBuilder {
        bridge: SessionBridge,
    }

    impl DisplayHandler {
        fn on_address_change(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            url: Option<&CefString>,
        ) {
            if frame.is_some_and(|frame| frame.is_main() != 0) {
                if let Some(browser) = browser {
                    self.bridge.apply_page_zoom(browser);
                }
                self.bridge.emit(BrowserEvent::AddressChanged {
                    session: self.bridge.id,
                    url: Arc::from(url.map(ToString::to_string).unwrap_or_default()),
                });
            }
        }

        fn on_title_change(&self, _browser: Option<&mut Browser>, title: Option<&CefString>) {
            self.bridge.emit(BrowserEvent::TitleChanged {
                session: self.bridge.id,
                title: Arc::from(title.map(ToString::to_string).unwrap_or_default()),
            });
        }

        fn on_cursor_change(
            &self,
            _browser: Option<&mut Browser>,
            _cursor: CursorHandle,
            type_: CursorType,
            _custom_cursor_info: Option<&CursorInfo>,
        ) -> i32 {
            self.bridge.emit(BrowserEvent::CursorChanged {
                session: self.bridge.id,
                cursor: browser_cursor(type_),
            });
            1
        }
    }
}

cef::wrap_life_span_handler! {
    struct LifeSpanHandlerBuilder {
        bridge: SessionBridge,
        message_router: Arc<BrowserSideRouter>,
    }

    impl LifeSpanHandler {
        fn on_before_popup(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _popup_id: i32,
            target_url: Option<&CefString>,
            _target_frame_name: Option<&CefString>,
            target_disposition: WindowOpenDisposition,
            _user_gesture: i32,
            _popup_features: Option<&PopupFeatures>,
            _window_info: Option<&mut WindowInfo>,
            _client: Option<&mut Option<Client>>,
            _settings: Option<&mut BrowserSettings>,
            _extra_info: Option<&mut Option<DictionaryValue>>,
            _no_javascript_access: Option<&mut i32>,
        ) -> i32 {
            if let Some(target_url) = target_url {
                self.bridge.emit(BrowserEvent::PopupRequested {
                    session: self.bridge.id,
                    url: Arc::from(target_url.to_string()),
                    foreground: popup_opens_in_foreground(target_disposition),
                });
            }
            1
        }

        fn on_after_created(&self, browser: Option<&mut Browser>) {
            if let Some(browser) = browser {
                self.bridge.apply_page_zoom(browser);
            }
            self.bridge.emit(BrowserEvent::Created {
                session: self.bridge.id,
            });
        }

        fn on_before_close(&self, browser: Option<&mut Browser>) {
            self.message_router
                .on_before_close(browser.as_deref().cloned());
            self.bridge.cancel_element_pick();
            self.bridge.emit(BrowserEvent::Closed {
                session: self.bridge.id,
            });
        }
    }
}

cef::wrap_load_handler! {
    struct LoadHandlerBuilder {
        bridge: SessionBridge,
    }

    impl LoadHandler {
        fn on_loading_state_change(
            &self,
            _browser: Option<&mut Browser>,
            is_loading: i32,
            can_go_back: i32,
            can_go_forward: i32,
        ) {
            self.bridge.emit(BrowserEvent::LoadingChanged {
                session: self.bridge.id,
                loading: is_loading != 0,
                can_go_back: can_go_back != 0,
                can_go_forward: can_go_forward != 0,
            });
        }

        fn on_load_error(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            error_code: Errorcode,
            error_text: Option<&CefString>,
            failed_url: Option<&CefString>,
        ) {
            if error_code == Errorcode::ABORTED
                || frame.is_none_or(|frame| frame.is_main() == 0)
            {
                return;
            }
            self.bridge.emit(BrowserEvent::LoadFailed {
                session: self.bridge.id,
                code: error_code.get_raw(),
                description: Arc::from(error_text.map(ToString::to_string).unwrap_or_default()),
                url: Arc::from(failed_url.map(ToString::to_string).unwrap_or_default()),
            });
        }
    }
}

cef::wrap_request_handler! {
    struct RequestHandlerBuilder {
        bridge: SessionBridge,
        message_router: Arc<BrowserSideRouter>,
    }

    impl RequestHandler {
        fn on_before_browse(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _request: Option<&mut Request>,
            _user_gesture: i32,
            _is_redirect: i32,
        ) -> i32 {
            let is_main = frame.as_deref().is_some_and(|frame| frame.is_main() != 0);
            self.message_router.on_before_browse(
                browser.as_deref().cloned(),
                frame.as_deref().cloned(),
            );
            if is_main {
                self.bridge.cancel_element_pick();
            }
            0
        }

        fn on_open_urlfrom_tab(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            target_url: Option<&CefString>,
            target_disposition: WindowOpenDisposition,
            _user_gesture: i32,
        ) -> i32 {
            if let Some(target_url) = target_url {
                self.bridge.emit(BrowserEvent::PopupRequested {
                    session: self.bridge.id,
                    url: Arc::from(target_url.to_string()),
                    foreground: popup_opens_in_foreground(target_disposition),
                });
            }
            1
        }

        fn on_render_process_terminated(
            &self,
            browser: Option<&mut Browser>,
            status: TerminationStatus,
            error_code: i32,
            error_string: Option<&CefString>,
        ) {
            self.message_router
                .on_render_process_terminated(browser.as_deref().cloned());
            self.bridge.cancel_element_pick();
            let detail = error_string.map_or_else(
                || format!("renderer termination status {}", status.get_raw()),
                ToString::to_string,
            );
            self.bridge.emit(BrowserEvent::RenderProcessTerminated {
                session: self.bridge.id,
                status: Arc::from(detail),
                error_code,
            });
        }
    }
}

cef::wrap_context_menu_handler! {
    struct BridgedContextMenuHandler {
        bridge: SessionBridge,
    }

    impl ContextMenuHandler {
        fn run_context_menu(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            params: Option<&mut ContextMenuParams>,
            _model: Option<&mut MenuModel>,
            callback: Option<&mut RunContextMenuCallback>,
        ) -> i32 {
            // `params` dies with this callback, so copy the request out first.
            if let Some(params) = params {
                self.bridge.emit(BrowserEvent::ContextMenuRequested {
                    session: self.bridge.id,
                    request: self.bridge.context_menu_request(params),
                });
            }
            if let Some(callback) = callback {
                callback.cancel();
            }
            1
        }
    }
}

cef::wrap_dialog_handler! {
    struct DeniedDialogHandler;

    impl DialogHandler {
        fn on_file_dialog(
            &self,
            _browser: Option<&mut Browser>,
            _mode: FileDialogMode,
            _title: Option<&CefString>,
            _default_file_path: Option<&CefString>,
            _accept_filters: Option<&mut CefStringList>,
            _accept_extensions: Option<&mut CefStringList>,
            _accept_descriptions: Option<&mut CefStringList>,
            callback: Option<&mut FileDialogCallback>,
        ) -> i32 {
            if let Some(callback) = callback {
                callback.cancel();
            }
            1
        }
    }
}

cef::wrap_download_handler! {
    struct DeniedDownloadHandler;

    impl DownloadHandler {
        fn can_download(
            &self,
            _browser: Option<&mut Browser>,
            _url: Option<&CefString>,
            _request_method: Option<&CefString>,
        ) -> i32 {
            0
        }
    }
}

cef::wrap_permission_handler! {
    struct DeniedPermissionHandler;

    impl PermissionHandler {
        fn on_request_media_access_permission(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _requesting_origin: Option<&CefString>,
            _requested_permissions: u32,
            callback: Option<&mut MediaAccessCallback>,
        ) -> i32 {
            if let Some(callback) = callback {
                callback.cancel();
            }
            1
        }

        fn on_show_permission_prompt(
            &self,
            _browser: Option<&mut Browser>,
            _prompt_id: u64,
            _requesting_origin: Option<&CefString>,
            _requested_permissions: u32,
            callback: Option<&mut PermissionPromptCallback>,
        ) -> i32 {
            if let Some(callback) = callback {
                callback.cont(PermissionRequestResult::DENY);
            }
            1
        }
    }
}

cef::wrap_client! {
    struct BrowserClient {
        render_handler: RenderHandler,
        display_handler: DisplayHandler,
        life_span_handler: LifeSpanHandler,
        load_handler: LoadHandler,
        request_handler: RequestHandler,
        context_menu_handler: ContextMenuHandler,
        dialog_handler: DialogHandler,
        download_handler: DownloadHandler,
        permission_handler: PermissionHandler,
        message_router: Arc<BrowserSideRouter>,
    }

    impl Client {
        fn render_handler(&self) -> Option<RenderHandler> {
            Some(self.render_handler.clone())
        }

        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(self.display_handler.clone())
        }

        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(self.life_span_handler.clone())
        }

        fn load_handler(&self) -> Option<LoadHandler> {
            Some(self.load_handler.clone())
        }

        fn request_handler(&self) -> Option<RequestHandler> {
            Some(self.request_handler.clone())
        }

        fn context_menu_handler(&self) -> Option<ContextMenuHandler> {
            Some(self.context_menu_handler.clone())
        }

        fn dialog_handler(&self) -> Option<DialogHandler> {
            Some(self.dialog_handler.clone())
        }

        fn download_handler(&self) -> Option<DownloadHandler> {
            Some(self.download_handler.clone())
        }

        fn permission_handler(&self) -> Option<PermissionHandler> {
            Some(self.permission_handler.clone())
        }

        fn on_process_message_received(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            source_process: ProcessId,
            message: Option<&mut ProcessMessage>,
        ) -> i32 {
            i32::from(self.message_router.on_process_message_received(
                browser.as_deref().cloned(),
                frame.as_deref().cloned(),
                source_process,
                message.as_deref().cloned(),
            ))
        }
    }
}

fn browser_cursor(value: CursorType) -> BrowserCursor {
    if value == CursorType::IBEAM || value == CursorType::VERTICALTEXT {
        BrowserCursor::IBeam
    } else if value == CursorType::HAND {
        BrowserCursor::PointingHand
    } else if value == CursorType::CROSS || value == CursorType::CELL {
        BrowserCursor::Crosshair
    } else if value == CursorType::WAIT || value == CursorType::PROGRESS {
        BrowserCursor::Wait
    } else if value == CursorType::HELP {
        BrowserCursor::Help
    } else if value == CursorType::MOVE {
        BrowserCursor::Move
    } else if matches!(
        value,
        CursorType::EASTRESIZE | CursorType::WESTRESIZE | CursorType::EASTWESTRESIZE
    ) {
        BrowserCursor::ResizeHorizontal
    } else if matches!(
        value,
        CursorType::NORTHRESIZE | CursorType::SOUTHRESIZE | CursorType::NORTHSOUTHRESIZE
    ) {
        BrowserCursor::ResizeVertical
    } else if matches!(
        value,
        CursorType::NORTHEASTRESIZE
            | CursorType::SOUTHWESTRESIZE
            | CursorType::NORTHEASTSOUTHWESTRESIZE
    ) {
        BrowserCursor::ResizeNorthEastSouthWest
    } else if matches!(
        value,
        CursorType::NORTHWESTRESIZE
            | CursorType::SOUTHEASTRESIZE
            | CursorType::NORTHWESTSOUTHEASTRESIZE
    ) {
        BrowserCursor::ResizeNorthWestSouthEast
    } else if value == CursorType::GRAB {
        BrowserCursor::Grab
    } else if value == CursorType::GRABBING {
        BrowserCursor::Grabbing
    } else if value == CursorType::NONE {
        BrowserCursor::None
    } else if value == CursorType::NOTALLOWED || value == CursorType::NODROP {
        BrowserCursor::NotAllowed
    } else {
        BrowserCursor::Arrow
    }
}

fn ensure_no_live_sessions(active_sessions: &AtomicU64) -> Result<(), BrowserError> {
    let count = active_sessions.load(Ordering::Acquire);
    if count == 0 {
        Ok(())
    } else {
        Err(BrowserError::BrowsersStillOpen(count))
    }
}

fn ensure_no_active_data_operations(active_operations: &AtomicU64) -> Result<(), BrowserError> {
    let count = active_operations.load(Ordering::Acquire);
    if count == 0 {
        Ok(())
    } else {
        Err(BrowserError::DataOperationsStillActive(count))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popup_disposition_preserves_background_tab_intent() {
        assert!(!popup_opens_in_foreground(
            WindowOpenDisposition::NEW_BACKGROUND_TAB,
        ));
        assert!(popup_opens_in_foreground(
            WindowOpenDisposition::NEW_FOREGROUND_TAB,
        ));
        assert!(popup_opens_in_foreground(WindowOpenDisposition::NEW_WINDOW,));
    }

    #[test]
    fn message_pump_starts_uninitialized_and_noops_before_start() {
        let pump = BrowserMessagePump::new(Arc::new(AtomicU64::new(0)));
        assert_eq!(pump.state().phase, RuntimePhase::Uninitialized);
        assert!(!pump.state().initialized);

        pump.do_message_loop_work();
        let calls = Cell::new(0);
        pump.run_message_loop_work(|| calls.set(calls.get() + 1));
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn detached_message_pump_tracks_lifecycle_and_coalesces_reentry() {
        let pump = BrowserMessagePump::new(Arc::new(AtomicU64::new(0)));
        let detached = pump.clone();
        pump.set_phase(RuntimePhase::Initializing);
        pump.enable();
        pump.set_phase(RuntimePhase::Running);
        assert_eq!(detached.state().phase, RuntimePhase::Running);

        let calls = Cell::new(0);
        let nested = detached.clone();
        pump.run_message_loop_work(|| {
            let call = calls.get() + 1;
            calls.set(call);
            if call == 1 {
                nested.run_message_loop_work(|| panic!("nested CEF pump must be coalesced"));
            }
        });
        assert_eq!(calls.get(), 2);

        pump.mark_closed();
        assert!(!detached.state().initialized);
        assert_eq!(detached.state().phase, RuntimePhase::Closed);
    }

    #[test]
    fn live_sessions_prevent_global_shutdown() {
        let active = AtomicU64::new(1);
        assert!(matches!(
            ensure_no_live_sessions(&active),
            Err(BrowserError::BrowsersStillOpen(1))
        ));
        active.store(0, Ordering::Release);
        assert!(ensure_no_live_sessions(&active).is_ok());
    }

    #[test]
    fn active_data_operations_prevent_global_shutdown() {
        let active = Arc::new(AtomicU64::new(0));
        let operation = ActiveDataOperation::new(Arc::clone(&active));
        assert!(matches!(
            ensure_no_active_data_operations(&active),
            Err(BrowserError::DataOperationsStillActive(1))
        ));
        assert!(operation.finish());
        assert!(!operation.finish());
        assert!(ensure_no_active_data_operations(&active).is_ok());
    }

    #[test]
    fn dropped_data_operation_releases_shutdown_guard() {
        let active = Arc::new(AtomicU64::new(0));
        drop(ActiveDataOperation::new(Arc::clone(&active)));
        assert!(ensure_no_active_data_operations(&active).is_ok());
    }

    #[test]
    fn site_data_origin_is_limited_to_http_and_https() {
        assert_eq!(
            site_origin("https://user:pass@example.com:8443/account?tab=1")
                .expect("valid HTTPS origin"),
            "https://example.com:8443"
        );
        for unsupported in [
            "about:blank",
            "file:///tmp/page.html",
            "data:text/plain,hello",
        ] {
            assert!(matches!(
                site_origin(unsupported),
                Err(BrowserError::UnsupportedOrigin)
            ));
        }
    }

    #[test]
    fn cookie_expiration_uses_the_windows_epoch() {
        assert_eq!(
            cef_expiration(1_000_000)
                .expect("representable timestamp")
                .val,
            WINDOWS_EPOCH_UNIX_OFFSET_MICROS + 1_000_000
        );
        assert!(cef_expiration(i64::MAX).is_none());
    }

    #[test]
    fn validates_browser_frame_rate_configuration() {
        assert_eq!(parse_browser_frame_rate("1"), Some(1));
        assert_eq!(parse_browser_frame_rate(" 120 "), Some(120));
        assert_eq!(parse_browser_frame_rate("240"), Some(240));
        assert_eq!(parse_browser_frame_rate("0"), None);
        assert_eq!(parse_browser_frame_rate("241"), None);
        assert_eq!(parse_browser_frame_rate("fast"), None);
    }

    #[test]
    fn gpu_features_are_enabled_unless_explicitly_disabled() {
        assert!(default_enabled_env_flag(None));
        assert!(default_enabled_env_flag(Some(OsStr::new("1"))));
        assert!(!default_enabled_env_flag(Some(OsStr::new("0"))));
    }

    #[test]
    fn external_begin_frames_macos_default_on_with_zero_kill_switch() {
        let platform = ExternalBeginFramePlatform::MacOs;
        assert!(external_begin_frame_setting_enabled(None, platform));
        assert!(external_begin_frame_setting_enabled(
            Some(OsStr::new("1")),
            platform
        ));
        assert!(external_begin_frame_setting_enabled(
            Some(OsStr::new("enabled")),
            platform
        ));
        assert!(!external_begin_frame_setting_enabled(
            Some(OsStr::new("0")),
            platform
        ));
    }

    #[test]
    fn external_begin_frames_linux_and_freebsd_require_exact_opt_in() {
        let platform = ExternalBeginFramePlatform::LinuxOrFreeBsd;
        assert!(!external_begin_frame_setting_enabled(None, platform));
        assert!(external_begin_frame_setting_enabled(
            Some(OsStr::new("1")),
            platform
        ));
        for value in ["0", "true", " 1 ", "01"] {
            assert!(!external_begin_frame_setting_enabled(
                Some(OsStr::new(value)),
                platform
            ));
        }
    }

    #[test]
    fn external_begin_frames_stay_disabled_on_unsupported_platforms() {
        let platform = ExternalBeginFramePlatform::Unsupported;
        assert!(!external_begin_frame_setting_enabled(None, platform));
        assert!(!external_begin_frame_setting_enabled(
            Some(OsStr::new("1")),
            platform
        ));
    }

    #[test]
    fn adaptive_begin_frame_throttle_requires_exact_opt_in() {
        assert!(!begin_frame_adaptive_setting_enabled(None));
        assert!(begin_frame_adaptive_setting_enabled(Some(OsStr::new("1"))));
        assert!(!begin_frame_adaptive_setting_enabled(Some(OsStr::new(
            "enabled"
        ))));
        assert!(!begin_frame_adaptive_setting_enabled(Some(OsStr::new("0"))));
    }

    #[test]
    fn committed_text_uses_cef_character_events() {
        let event = text_key_event('A' as u16);
        assert_eq!(event.type_, KeyEventType::CHAR);
        assert_eq!(event.windows_key_code, 0x41);
        assert_eq!(event.character, 0x41);
        assert_eq!(event.unmodified_character, 0x41);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn named_keys_keep_their_macos_character_and_hardware_identity() {
        let tab = key_event(KeyInput {
            action: KeyAction::Press,
            key: BrowserKey::Tab,
            modifiers: Modifiers::default(),
        });
        assert_eq!(tab.type_, KeyEventType::RAWKEYDOWN);
        assert_eq!(tab.windows_key_code, 0x09);
        assert_eq!(tab.native_key_code, 0x30);
        assert_eq!(tab.character, 0x09);
        assert_eq!(tab.unmodified_character, 0x09);

        let enter = named_key_character_event(KeyInput {
            action: KeyAction::Press,
            key: BrowserKey::Enter,
            modifiers: Modifiers::default(),
        })
        .expect("Return produces CEF's keypress companion event");
        assert_eq!(enter.type_, KeyEventType::CHAR);
        assert_eq!(enter.windows_key_code, 0x0d);
        assert_eq!(enter.native_key_code, 0x24);
        assert_eq!(enter.character, 0x0d);

        let copy = key_event(KeyInput {
            action: KeyAction::Press,
            key: BrowserKey::Character('c'),
            modifiers: Modifiers::new(false, false, false, true),
        });
        assert_eq!(copy.native_key_code, 0x08);
        assert_ne!(
            copy.modifiers & cef_enum_bits(cef::sys::cef_event_flags_t::EVENTFLAG_COMMAND_DOWN.0),
            0
        );
    }

    #[test]
    fn releases_and_committed_characters_do_not_duplicate_character_events() {
        assert!(
            named_key_character_event(KeyInput {
                action: KeyAction::Release,
                key: BrowserKey::Enter,
                modifiers: Modifiers::default(),
            })
            .is_none()
        );
        assert!(
            named_key_character_event(KeyInput {
                action: KeyAction::Press,
                key: BrowserKey::Character('a'),
                modifiers: Modifiers::default(),
            })
            .is_none()
        );
    }

    #[test]
    fn packed_modifiers_preserve_cef_event_flags() {
        let flags = event_flags(
            Modifiers::new(true, true, true, true)
                .with_pointer_button(Some(PointerButton::Middle))
                .with_repeat(true),
        );
        assert_ne!(
            flags & cef_enum_bits(cef::sys::cef_event_flags_t::EVENTFLAG_SHIFT_DOWN.0),
            0
        );
        assert_ne!(
            flags & cef_enum_bits(cef::sys::cef_event_flags_t::EVENTFLAG_CONTROL_DOWN.0),
            0
        );
        assert_ne!(
            flags & cef_enum_bits(cef::sys::cef_event_flags_t::EVENTFLAG_ALT_DOWN.0),
            0
        );
        assert_ne!(
            flags & cef_enum_bits(cef::sys::cef_event_flags_t::EVENTFLAG_COMMAND_DOWN.0),
            0
        );
        assert_ne!(
            flags & cef_enum_bits(cef::sys::cef_event_flags_t::EVENTFLAG_MIDDLE_MOUSE_BUTTON.0),
            0
        );
        assert_ne!(
            flags & cef_enum_bits(cef::sys::cef_event_flags_t::EVENTFLAG_IS_REPEAT.0),
            0
        );
    }

    #[test]
    fn converts_fractional_wayland_scale_to_physical_osr_values() {
        assert_eq!(scaled_osr_dimension(1080, 1.25), 1350);
        assert_eq!(scaled_osr_dimension(638, 1.25), 798);
        assert_eq!(scaled_osr_coordinate(100, 1.25), 125);
        let zoom_factor = CHROMIUM_ZOOM_STEP.powf(chromium_zoom_level(1.25));
        assert!((zoom_factor - 1.25).abs() < f64::EPSILON);
    }

    fn geometry(x: f64, y: f64, scroll_y: f64) -> PickGeometry {
        PickGeometry {
            x,
            y,
            width: 300.0,
            height: 150.0,
            scroll_x: 0.0,
            scroll_y,
            viewport_width: 1280.0,
            viewport_height: 800.0,
        }
    }

    #[test]
    fn element_clip_expands_by_the_margin_in_page_coordinates() {
        let clip = element_screenshot_clip(geometry(100.0, 200.0, 50.0)).expect("on-screen clip");
        assert_eq!(
            clip,
            ScreenshotClip {
                x: 36.0,
                y: 186.0,
                width: 428.0,
                height: 278.0,
            }
        );
    }

    fn assert_px(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < f64::EPSILON,
            "{actual} != {expected}"
        );
    }

    #[test]
    fn element_clip_stays_inside_the_visible_viewport() {
        let clip = element_screenshot_clip(geometry(10.0, 10.0, 0.0)).expect("on-screen clip");
        assert_px(clip.x, 0.0);
        assert_px(clip.y, 0.0);
        assert_px(clip.width, 374.0);
        assert_px(clip.height, 224.0);

        assert_eq!(
            element_screenshot_clip(geometry(-5000.0, -5000.0, 0.0)),
            None
        );
    }

    #[test]
    fn decodes_a_base64_screenshot_and_rejects_junk() {
        let png = [0x89_u8, 0x50, 0x4e, 0x47];
        let encoded = base64::engine::general_purpose::STANDARD.encode(png);
        let body = serde_json::json!({ "data": encoded }).to_string();
        assert_eq!(
            decode_screenshot_result(body.as_bytes()).as_deref(),
            Some(png.as_slice())
        );

        assert_eq!(decode_screenshot_result(b"{"), None);
        assert_eq!(
            decode_screenshot_result(br#"{"data":"not base64!!"}"#),
            None
        );
        assert_eq!(decode_screenshot_result(br#"{"data":""}"#), None);
    }

    #[test]
    fn osr_coordinates_survive_a_round_trip_through_the_raster_scale() {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            for value in [0, 1, 37, 638, 1920] {
                assert_eq!(
                    unscaled_osr_coordinate(scaled_osr_coordinate(value, scale), scale),
                    value,
                    "scale={scale} value={value}"
                );
            }
        }
    }

    #[test]
    fn page_zoom_and_raster_scale_compose_multiplicatively() {
        let viewport = Viewport {
            scale_factor: 1.25,
            ..Viewport::default()
        };
        let expected_factor = if uses_wayland_physical_osr() {
            1.25 * 1.5
        } else {
            1.5
        };
        let effective_factor =
            CHROMIUM_ZOOM_STEP.powf(effective_chromium_zoom_level(viewport, 1.5));
        assert!((effective_factor - expected_factor).abs() < 1e-12);
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn test_accelerated_handle_identity(value: usize) -> AcceleratedPaintHandleIdentity {
        value
    }

    #[cfg(target_os = "linux")]
    fn test_accelerated_handle_identity(value: usize) -> AcceleratedPaintHandleIdentity {
        let mut planes = [AcceleratedPaintPlaneIdentity::default(); 4];
        planes[0] = AcceleratedPaintPlaneIdentity {
            device: Some(1),
            inode: Some(u64::try_from(value).expect("test identity fits in u64")),
            offset: 0,
            size: 800 * 600 * 4,
        };
        AcceleratedPaintHandleIdentity {
            plane_count: 1,
            planes,
        }
    }

    #[test]
    fn accelerated_paint_diagnostics_track_pool_reuse() {
        let tracker = AcceleratedPaintTracker::default();
        let observation = |identity| AcceleratedPaintObservationState {
            paint_element: "view",
            width: 800,
            height: 600,
            pixel_format: "bgra_8888",
            pixel_format_raw: cef_enum_bits(ColorType::BGRA_8888.get_raw()),
            platform: AcceleratedPaintPlatformState {
                drm_modifier: Some(0),
                plane_count: 0,
                planes: [None; 4],
                handle_identity: test_accelerated_handle_identity(identity),
            },
        };

        tracker.record(PaintElementType::VIEW, observation(1));
        tracker.record(PaintElementType::VIEW, observation(2));
        let third = tracker.record(PaintElementType::VIEW, observation(1));

        assert_eq!(third.reuse_gap, Some(2));
        let diagnostics = tracker.diagnostics();
        assert_eq!(diagnostics.callback_count, 3);
        assert_eq!(diagnostics.unique_handle_count, 2);
        assert_eq!(diagnostics.handle_transition_count, 2);
        assert_eq!(diagnostics.handles[0].minimum_reuse_gap, Some(2));
        assert_eq!(
            diagnostics
                .last_observation
                .expect("last observation")
                .handle_identity,
            format_accelerated_handle_identity(test_accelerated_handle_identity(1))
        );
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    fn test_pool_layout(width: i32, height: i32) -> AcceleratedPoolLayout {
        AcceleratedPoolLayout {
            width,
            height,
            pixel_format_raw: cef_enum_bits(ColorType::BGRA_8888.get_raw()),
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    fn assert_accelerated_identities_accepted(
        pool: &PoolGenerationBookkeeper,
        layout: &AcceleratedPoolLayout,
        identities: &[AcceleratedPaintHandleIdentity],
        observations: usize,
    ) {
        assert!(!identities.is_empty());
        let rejections = (0..observations)
            .filter_map(|callback| {
                let identity = identities[callback % identities.len()];
                pool.observe(layout)
                    .err()
                    .map(|error| (callback, identity, error))
            })
            .collect::<Vec<_>>();
        assert!(
            rejections.is_empty(),
            "same-sized accelerated identities were rejected: {rejections:?}"
        );
    }

    fn test_viewport(width: u32, height: u32) -> Viewport {
        Viewport {
            width,
            height,
            scale_factor: 1.0,
            visible: true,
            ..Viewport::default()
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    #[test]
    fn accelerated_pool_rejects_only_mismatched_dimensions() {
        let mut pool = PoolGenerationBookkeeper::new(test_viewport(800, 600));
        let first_layout = test_pool_layout(800, 600);
        assert_eq!(pool.observe(&first_layout), Ok(()));

        let same_dimensions_different_format = AcceleratedPoolLayout {
            pixel_format_raw: cef_enum_bits(ColorType::RGBA_8888.get_raw()),
            ..first_layout
        };
        assert_eq!(pool.observe(&same_dimensions_different_format), Ok(()));

        assert!(pool.set_viewport(test_viewport(1088, 720)));
        assert_eq!(
            pool.observe(&first_layout),
            Err(StalePoolFrame::Dimensions {
                expected_width: 1088,
                expected_height: 720,
                actual_width: 800,
                actual_height: 600,
            })
        );
        let second_layout = test_pool_layout(1088, 720);
        assert_eq!(pool.observe(&second_layout), Ok(()));
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn accelerated_frames_expect_device_pixels_on_the_default_osr_path() {
        let viewport = Viewport {
            scale_factor: 2.0,
            ..test_viewport(873, 955)
        };
        let expected = ExpectedFrameSize::from_viewport(viewport);
        assert_eq!(expected.device_width, 1746);
        assert_eq!(expected.device_height, 1910);
        assert_eq!(expected.logical_width, 873);
        assert_eq!(expected.logical_height, 955);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn accelerated_pool_accepts_reused_dmabuf_inodes_at_expected_dimensions() {
        let pool = PoolGenerationBookkeeper::new(test_viewport(800, 600));
        let layout = test_pool_layout(800, 600);
        let physical_slots = [
            test_accelerated_handle_identity(31),
            test_accelerated_handle_identity(32),
        ];

        assert_accelerated_identities_accepted(&pool, &layout, &physical_slots, 512);
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    #[test]
    fn accelerated_pool_accepts_three_rotating_identities_without_rejections() {
        let pool = PoolGenerationBookkeeper::new(test_viewport(800, 600));
        let layout = test_pool_layout(800, 600);
        let rotating_slots = [
            test_accelerated_handle_identity(101),
            test_accelerated_handle_identity(102),
            test_accelerated_handle_identity(103),
        ];

        assert_accelerated_identities_accepted(&pool, &layout, &rotating_slots, 1536);
    }

    #[test]
    fn shared_texture_fallback_is_a_one_way_one_event_latch() {
        let viewport = test_viewport(800, 600);
        let (events, event_rx) = async_channel::unbounded();
        let bridge = SessionBridge {
            id: SessionId(7),
            events,
            viewport: Arc::new(Mutex::new(viewport)),
            page_zoom_factor: Arc::new(Mutex::new(1.0)),
            mailbox: FrameMailbox::default(),
            invalid_frames: Arc::new(AtomicU64::new(0)),
            accelerated_paint: AcceleratedPaintTracker::default(),
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            accelerated_frames: AcceleratedFrameProducer::new(None, viewport),
            #[cfg(target_os = "macos")]
            metal_frames: MetalFrameProducer::new(viewport),
            #[cfg(target_os = "windows")]
            d3d11_frames: D3d11FrameProducer::new(None, viewport),
            shared_texture_fallback_notified: Arc::new(AtomicBool::new(false)),
            element_pick: ElementPickState::default(),
            pending_capture: PendingCapture::default(),
        };

        assert!(!bridge.shared_texture_fallback_requested());
        bridge.request_shared_texture_fallback("first import failed");
        bridge.request_shared_texture_fallback("late validation failure");
        assert!(bridge.shared_texture_fallback_requested());
        assert!(matches!(
            event_rx.try_recv(),
            Ok(BrowserEvent::SharedTextureFailed { session, reason })
                if session == SessionId(7) && reason.as_ref() == "first import failed"
        ));
        assert!(event_rx.try_recv().is_err());
    }
}
