#![cfg(target_os = "ios")]
#![allow(unsafe_op_in_unsafe_fn)]
#![allow(non_camel_case_types, non_upper_case_globals)]

//! iOS backend for GPUI: a UIKit windowing layer over copies of `gpui_macos`'s
//! Metal renderer, CoreText text system and GCD dispatcher. Keep the copies
//! diff-minimal against `gpui_macos` so a gpui bump can re-sync them.

mod dispatcher;
mod display;
mod metal_atlas;
// The surface/video half is stubbed on iOS: no core-video.
#[allow(dead_code, unused_imports)]
pub mod metal_renderer;
#[cfg(feature = "font-kit")]
mod open_type;
mod platform;
mod system_traits;
#[cfg(feature = "font-kit")]
mod text_system;
mod window;

pub(crate) use metal_renderer as renderer;

pub(crate) use dispatcher::*;
pub(crate) use display::*;
#[cfg(feature = "font-kit")]
pub(crate) use text_system::*;
pub(crate) use window::*;

pub use platform::IosPlatform;
/// The factor the system text size has moved by since the app last looked.
pub use system_traits::take_content_size_scale;
/// The user's motion and transparency preferences, read live.
pub use system_traits::{reduce_motion, reduce_transparency};
/// How much of the window the software keyboard covers. The app pads its
/// bottom edge by it; UIKit resizes nothing on its own.
pub use window::keyboard_inset;
/// The factor a pinch has accumulated since the app last looked. Taking it clears it.
pub use window::take_pinch_scale;

use objc::runtime::Object;

pub(crate) type id = *mut Object;
pub(crate) const nil: id = std::ptr::null_mut();

// UIKit geometry, hand-declared: `cocoa` is AppKit-only. Layouts match
// CoreGraphics on 64-bit.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CGPoint {
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CGSize {
    pub width: f64,
    pub height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CGRect {
    pub origin: CGPoint,
    pub size: CGSize,
}

unsafe impl objc::Encode for CGPoint {
    fn encode() -> objc::Encoding {
        unsafe { objc::Encoding::from_str("{CGPoint=dd}") }
    }
}

unsafe impl objc::Encode for CGSize {
    fn encode() -> objc::Encoding {
        unsafe { objc::Encoding::from_str("{CGSize=dd}") }
    }
}

unsafe impl objc::Encode for CGRect {
    fn encode() -> objc::Encoding {
        unsafe { objc::Encoding::from_str("{CGRect={CGPoint=dd}{CGSize=dd}}") }
    }
}

/// Returns an autoreleased NSString.
pub(crate) unsafe fn ns_string(string: &str) -> id {
    use objc::{class, msg_send, sel, sel_impl};
    let c = std::ffi::CString::new(string).unwrap();
    let ns: id = msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()];
    ns
}

/// Copies an NSString into Rust. `nil` and non-UTF-8 strings read as `None`.
pub(crate) unsafe fn nsstring_to_string(string: id) -> Option<String> {
    use objc::{msg_send, sel, sel_impl};
    if string.is_null() {
        return None;
    }
    let bytes: *const std::ffi::c_char = msg_send![string, UTF8String];
    if bytes.is_null() {
        return None;
    }
    Some(
        std::ffi::CStr::from_ptr(bytes)
            .to_string_lossy()
            .into_owned(),
    )
}
