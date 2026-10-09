//! GPUI's browser platform. Each window draws into its own canvas: either the whole page, or a
//! page element named by `WindowOptions::mount`, so one page can host several windows among its
//! own content. Browser WebGPU is preferred by default, with an automatic WebGL2 fallback;
//! WebGL2 can draw only the first window. Applications can force either backend with
//! `WebBackendPreference`.
//!
//! Every window can mirror its accessibility tree into the page, and `globalThis.zzGpui` lets
//! agents and tests find, click and capture windows; see the `automation` module.

pub mod canvas_fallback;

#[cfg(target_family = "wasm")]
mod a11y;
#[cfg(target_family = "wasm")]
mod automation;

pub use canvas_fallback::CanvasFontFallback;

#[cfg(any(target_family = "wasm", test))]
mod canvas_size;
#[cfg(target_family = "wasm")]
mod canvas_text;
#[cfg(target_family = "wasm")]
mod dispatcher;
#[cfg(target_family = "wasm")]
mod display;
#[cfg(target_family = "wasm")]
mod events;
#[cfg(any(target_family = "wasm", test))]
mod glyph_cache;
#[cfg(target_family = "wasm")]
mod http_client;
#[cfg(target_family = "wasm")]
mod ime_mirror;
#[cfg(target_family = "wasm")]
mod keyboard;
#[cfg(target_family = "wasm")]
mod logging;
#[cfg(target_family = "wasm")]
mod platform;
#[cfg(any(target_family = "wasm", test))]
mod run_replacements;
#[cfg(target_family = "wasm")]
mod text_system;
#[cfg(target_family = "wasm")]
mod viewport;
#[cfg(target_family = "wasm")]
mod window;

#[cfg(target_family = "wasm")]
pub use crate::wgpu::WebBackendPreference;
#[cfg(target_family = "wasm")]
pub use dispatcher::WebDispatcher;
#[cfg(target_family = "wasm")]
pub use display::WebDisplay;
#[cfg(target_family = "wasm")]
pub use http_client::{FetchCredentials, FetchHttpClient};
#[cfg(target_family = "wasm")]
pub use keyboard::WebKeyboardLayout;
#[cfg(target_family = "wasm")]
pub use logging::init_logging;
#[cfg(target_family = "wasm")]
pub use platform::{WebPlatform, WebWindowError};
#[cfg(target_family = "wasm")]
pub use window::WebWindow;
