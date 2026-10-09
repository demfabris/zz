//! Browser panes.

pub(crate) mod controller;
pub(crate) mod element;
#[cfg(target_os = "macos")]
pub(crate) mod macos_surface;
pub(crate) mod recent_pages;
pub(crate) mod screenshot;
#[cfg(not(target_os = "windows"))]
pub(crate) mod tui;
#[cfg(target_os = "macos")]
pub(crate) mod underlay;
pub(crate) mod view;
