#![allow(unsafe_op_in_unsafe_fn)]
#![allow(non_camel_case_types, non_upper_case_globals)]

pub mod keyboard;
pub mod momentum;
pub mod pinch;

#[cfg(target_os = "ios")]
mod dispatcher;
#[cfg(target_os = "ios")]
mod display;
#[cfg(target_os = "ios")]
mod drop;
#[cfg(target_os = "ios")]
mod ffi;
#[cfg(target_os = "ios")]
mod keyboard_inset;
#[cfg(target_os = "ios")]
mod menu;
#[cfg(target_os = "ios")]
mod perf;
#[cfg(target_os = "ios")]
mod platform;
#[cfg(target_os = "ios")]
mod text_input;
#[cfg(target_os = "ios")]
mod window;

#[cfg(target_os = "ios")]
pub(crate) use dispatcher::*;
#[cfg(target_os = "ios")]
pub use display::phone;
#[cfg(target_os = "ios")]
pub(crate) use display::*;
#[cfg(target_os = "ios")]
pub use ffi::*;
#[cfg(target_os = "ios")]
pub use menu::MenuCommand;
#[cfg(target_os = "ios")]
pub use platform::IosPlatform;
#[cfg(target_os = "ios")]
pub(crate) use window::*;
#[cfg(target_os = "ios")]
pub use window::{
    Accessibility, accessibility, request_paste, set_compact_keyboard, set_status_bar_on_dark,
    show_edit_menu,
};
