#![allow(unsafe_op_in_unsafe_fn)]

pub mod editor;
pub mod keyboard;

#[cfg(target_os = "android")]
mod bridge;
#[cfg(target_os = "android")]
mod dispatcher;
#[cfg(target_os = "android")]
mod display;
#[cfg(target_os = "android")]
mod platform;
#[cfg(target_os = "android")]
mod text_input;
#[cfg(target_os = "android")]
mod window;

#[cfg(target_os = "android")]
pub use bridge::{
    Accessibility, accessibility, display_corner_radius, files_dir, load_class, on_load, phone,
    set_status_bar_on_dark, with_activity,
};
#[cfg(target_os = "android")]
pub use jni;
#[cfg(target_os = "android")]
pub use platform::AndroidPlatform;
#[cfg(target_os = "android")]
pub use window::{request_paste, set_compact_keyboard, show_edit_menu};
