//! Browser panes. iOS swaps in [`stub`], which re-exports the same module paths.

#[cfg(not(target_os = "ios"))]
pub(crate) mod controller;
#[cfg(not(target_os = "ios"))]
pub(crate) mod element;
#[cfg(target_os = "macos")]
pub(crate) mod macos_surface;
#[cfg(not(target_os = "ios"))]
pub(crate) mod recent_pages;
#[cfg(not(target_os = "ios"))]
pub(crate) mod screenshot;
#[cfg(not(any(target_os = "ios", target_os = "windows")))]
pub(crate) mod tui;
#[cfg(not(target_os = "ios"))]
pub(crate) mod view;

#[cfg(target_os = "ios")]
mod stub;
#[cfg(target_os = "ios")]
pub(crate) use stub::{controller, recent_pages, view};
