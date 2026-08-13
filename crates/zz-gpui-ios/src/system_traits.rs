//! System accessibility traits: the user's text size, published as a relative
//! factor the app multiplies its zoom by, and their motion and transparency
//! preferences. Atomics only, so a trait change never re-enters gpui.

use crate::id;
use objc::{
    class, msg_send,
    runtime::{BOOL, YES},
    sel, sel_impl,
};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

#[link(name = "UIKit", kind = "framework")]
unsafe extern "C" {
    #[link_name = "UIAccessibilityIsReduceMotionEnabled"]
    fn reduce_motion_enabled() -> BOOL;
    #[link_name = "UIAccessibilityIsReduceTransparencyEnabled"]
    fn reduce_transparency_enabled() -> BOOL;
}

/// Body text at the default content size category, so a scaled 17 over 17 is
/// the factor iOS applies to text everywhere else.
const BODY_POINT_SIZE: f64 = 17.0;

/// `1.0f32`'s bit pattern: nothing has moved since the app last looked.
const IDENTITY: u32 = 0x3f80_0000;

/// The text-size factor as last measured. The app is told the change from it,
/// so the first refresh publishes whatever the device is set to.
static MEASURED: AtomicU32 = AtomicU32::new(IDENTITY);
/// Product of every text-size change the app has not applied yet.
static PENDING: AtomicU32 = AtomicU32::new(IDENTITY);
/// Set when the pending factor moved, cleared by the frame that acts on it.
static CHANGED: AtomicBool = AtomicBool::new(false);

/// The factor the system text size has moved by since the app last looked, or
/// `None`. Taking it resets it, so the caller must apply what it took.
pub fn take_content_size_scale() -> Option<f32> {
    let bits = PENDING.swap(IDENTITY, Ordering::Relaxed);
    if bits == IDENTITY {
        return None;
    }
    let scale = f32::from_bits(bits);
    (scale.is_finite() && scale > 0.0).then_some(scale)
}

/// Whether the user asked for less motion. Read live, not cached.
pub fn reduce_motion() -> bool {
    unsafe { reduce_motion_enabled() == YES }
}

/// Whether the user asked for less transparency.
pub fn reduce_transparency() -> bool {
    unsafe { reduce_transparency_enabled() == YES }
}

/// Re-reads the system text size and publishes what it moved by. A trait change
/// that left the text size alone publishes nothing.
pub(crate) fn refresh() {
    let Some(scale) = measure() else {
        return;
    };
    let previous = f32::from_bits(MEASURED.swap(scale.to_bits(), Ordering::Relaxed));
    if previous == scale || !previous.is_finite() || previous <= 0.0 {
        return;
    }
    let pending = f32::from_bits(PENDING.load(Ordering::Relaxed)) * (scale / previous);
    PENDING.store(pending.to_bits(), Ordering::Relaxed);
    CHANGED.store(true, Ordering::Relaxed);
}

/// Whether a text-size change is waiting for a frame, clearing the flag.
pub(crate) fn take_changed() -> bool {
    CHANGED.swap(false, Ordering::Relaxed)
}

/// The system text-size factor, 1.0 at the default content size category. Read
/// from the application: a view's traits are only trustworthy once in a window.
fn measure() -> Option<f32> {
    unsafe {
        let application: id = msg_send![class!(UIApplication), sharedApplication];
        if application.is_null() {
            return None;
        }
        let category: id = msg_send![application, preferredContentSizeCategory];
        if category.is_null() {
            return None;
        }
        let traits: id = msg_send![
            class!(UITraitCollection),
            traitCollectionWithPreferredContentSizeCategory: category
        ];
        let metrics: id = msg_send![class!(UIFontMetrics), defaultMetrics];
        let scaled: f64 = msg_send![
            metrics,
            scaledValueForValue: BODY_POINT_SIZE
            compatibleWithTraitCollection: traits
        ];
        let scale = (scaled / BODY_POINT_SIZE) as f32;
        (scale.is_finite() && scale > 0.0).then_some(scale)
    }
}
