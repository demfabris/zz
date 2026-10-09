//! The thin-client app the web and iOS clients share: shell, sidebar, status bar,
//! settings, command palette, overlays, panes, and the connection reducer. Each
//! client owns its entry point, fonts, window lifecycle, and platform backend.

pub mod app;
pub mod attachments;
pub mod command_palette;
pub mod connection;
pub mod input;
#[cfg(target_os = "ios")]
pub mod ios_browser;
pub mod preferences;
pub mod terminal;
pub mod terminal_images;
#[cfg(target_os = "ios")]
pub mod transport;
