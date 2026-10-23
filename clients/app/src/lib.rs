//! The thin-client app the web and iOS clients share: shell, sidebar, status bar,
//! settings, command palette, overlays, panes, and the connection reducer. Each
//! client owns its entry point, fonts, window lifecycle, and platform backend.

#[cfg(target_os = "android")]
pub mod android_browser;
pub mod app;
pub mod attachments;
pub mod command_palette;
pub mod connection;
pub mod input;
#[cfg(target_os = "ios")]
pub mod ios_browser;
#[cfg(target_os = "android")]
use android_browser as native_browser;
#[cfg(target_os = "ios")]
use ios_browser as native_browser;
pub mod preferences;
pub mod terminal;
pub mod terminal_images;
#[cfg(any(target_os = "ios", target_os = "android"))]
pub mod transport;
