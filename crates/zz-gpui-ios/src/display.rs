use crate::CGRect;
use anyhow::Result;
use gpui::{Bounds, DisplayId, Pixels, PlatformDisplay, point, px, size};
use objc::{class, msg_send, runtime::Object, sel, sel_impl};
use uuid::Uuid;

/// iOS has exactly one display: the device screen.
#[derive(Debug)]
pub(crate) struct IosDisplay;

impl IosDisplay {
    fn screen() -> *mut Object {
        unsafe { msg_send![class!(UIScreen), mainScreen] }
    }

    pub(crate) fn scale_factor() -> f32 {
        let scale: f64 = unsafe { msg_send![Self::screen(), scale] };
        scale as f32
    }
}

impl PlatformDisplay for IosDisplay {
    fn id(&self) -> DisplayId {
        DisplayId::from(0u64)
    }

    fn uuid(&self) -> Result<Uuid> {
        Ok(Uuid::from_u128(0x5a5a_0000_0000_0000_0000_0000_0000_0001))
    }

    fn bounds(&self) -> Bounds<Pixels> {
        let rect: CGRect = unsafe { msg_send![Self::screen(), bounds] };
        Bounds {
            origin: point(px(rect.origin.x as f32), px(rect.origin.y as f32)),
            size: size(px(rect.size.width as f32), px(rect.size.height as f32)),
        }
    }

    fn refresh_rate(&self) -> Option<f32> {
        let fps: isize = unsafe { msg_send![Self::screen(), maximumFramesPerSecond] };
        Some(fps as f32)
    }
}
