#[cfg(any(feature = "cef-runtime", test))]
use zz_client::element_picker;
mod event;
// Without the CEF runtime nothing reaches the GPU-import and egress-profile halves.
#[cfg_attr(not(feature = "cef-runtime"), allow(dead_code))]
mod frame;
mod input;
mod lifecycle;
#[cfg(any(feature = "cef-runtime", test))]
mod page_zoom;
#[cfg_attr(not(feature = "cef-runtime"), allow(dead_code, unused_imports))]
mod profile;
mod url_input;

#[cfg(feature = "cef-runtime")]
#[allow(
    unsafe_code,
    clippy::transmute_ptr_to_ptr,
    reason = "CEF callbacks expose a scoped raw BGRA paint buffer"
)]
mod cef_runtime;

#[cfg(any(feature = "cef-runtime", test))]
pub use element_picker::ElementPickerAppearance;
pub use event::{BrowserCursor, BrowserEvent, ContextMenuRequest, EditFlags, SessionId};
pub use frame::{
    BrowserGpuContext, FrameError, FrameMailbox, FrameMailboxDiagnostics, FrameTier, GpuFrame,
    OsrFrame, OwnedBgraFrame,
};
#[cfg(target_os = "macos")]
pub use frame::{MacFramePresenter, MacGpuFrame, MacIoSurface};
#[cfg(target_os = "windows")]
pub use frame::{WinGpuFrame, WinGpuTexture};
pub use input::{
    BrowserKey, EditCommand, KeyAction, KeyInput, Modifiers, PointerButton, PointerEvent,
    PointerPhase, Viewport, WheelEvent, named_key_input, terminal_key_input,
};
pub use lifecycle::{RuntimePhase, SessionPhase};
pub use profile::{BrowserProfileError, BrowserProfilePaths, resolve_profile_paths};
pub use url_input::{
    SearchProvider, UrlInputError, diagnostic_url, normalize_url, resolve_address,
};
pub use zz_protocol::app_identity::recent_pages_path;
pub use zz_protocol::{
    BrowserCookie, BrowserCookiePriority, BrowserCookieSameSite, BrowserProfileNameError,
    CookieImportBatch, CookieImportError, CookieImportResult, DEFAULT_BROWSER_PROFILE,
    MAX_BROWSER_PROFILE_NAME_BYTES, MAX_COOKIE_IMPORT_BYTES, MAX_COOKIE_IMPORT_COUNT,
    MAX_FAVICON_BYTES, SiteDataClearResult, normalize_browser_profile_name, parse_cookie_import,
};

#[cfg(feature = "cef-runtime")]
pub use cef_runtime::{
    AcceleratedPaintDiagnostics, AcceleratedPaintHandleDiagnostics, AcceleratedPaintObservation,
    AcceleratedPaintPlaneDiagnostics, BrowserBootstrap, BrowserCommandSink, BrowserError,
    BrowserMessagePump, BrowserRuntime, BrowserSession, RuntimeSignal, bootstrap,
    bootstrap_with_profile_paths, run_subprocess,
};

#[cfg(all(feature = "cef-runtime", target_os = "windows"))]
pub use cef_runtime::bootstrap_windows;
