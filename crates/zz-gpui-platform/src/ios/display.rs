use crate::ios::{CGRect, id, nil};
use anyhow::Result;
use objc::{
    class, msg_send,
    runtime::{NO, Object, YES},
    sel, sel_impl,
};
use std::cell::Cell;
use uuid::Uuid;
use zz_gpui::{Bounds, DisplayId, Pixels, PlatformDisplay, point, px, size};

const IDLE_FRAMES_BEFORE_PAUSE: u8 = 3;

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

    fn maximum_frames_per_second() -> f32 {
        let fps: isize = unsafe { msg_send![Self::screen(), maximumFramesPerSecond] };
        fps as f32
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CAFrameRateRange {
    minimum: f32,
    maximum: f32,
    preferred: f32,
}

unsafe impl objc::Encode for CAFrameRateRange {
    fn encode() -> objc::Encoding {
        unsafe { objc::Encoding::from_str("{CAFrameRateRange=fff}") }
    }
}

pub(crate) struct DisplayLink {
    link: Cell<id>,
    paused: Cell<bool>,
    demand: Cell<bool>,
    idle: Cell<u8>,
}

impl DisplayLink {
    pub(crate) fn new() -> Self {
        Self {
            link: Cell::new(nil),
            paused: Cell::new(false),
            demand: Cell::new(true),
            idle: Cell::new(0),
        }
    }

    pub(crate) fn attach(&self, link: id) {
        Self::prefer_maximum_rate(link);
        self.link.set(link);
    }

    pub(crate) fn invalidate(&self) {
        let link = self.link.replace(nil);
        if !link.is_null() {
            unsafe {
                let _: () = msg_send![link, invalidate];
            }
        }
    }

    pub(crate) fn wake(&self) {
        self.demand.set(true);
        self.idle.set(0);
        let link = self.link.get();
        if self.paused.get() && !link.is_null() {
            self.paused.set(false);
            Self::prefer_maximum_rate(link);
            unsafe {
                let _: () = msg_send![link, setPaused: NO];
            }
        }
    }

    pub(crate) fn begin_frame(&self) {
        self.demand.set(false);
    }

    pub(crate) fn end_frame(&self, busy: bool) {
        if busy || self.demand.get() {
            self.idle.set(0);
            return;
        }
        let idle = self.idle.get().saturating_add(1);
        self.idle.set(idle);
        let link = self.link.get();
        if idle >= IDLE_FRAMES_BEFORE_PAUSE && !self.paused.get() && !link.is_null() {
            self.paused.set(true);
            unsafe {
                let _: () = msg_send![link, setPaused: YES];
            }
        }
    }

    fn prefer_maximum_rate(link: id) {
        let maximum = IosDisplay::maximum_frames_per_second().max(60.0);
        let range = CAFrameRateRange {
            minimum: (maximum / 2.0).max(60.0),
            maximum,
            preferred: maximum,
        };
        unsafe {
            let _: () = msg_send![link, setPreferredFrameRateRange: range];
        }
    }
}

pub fn phone() -> bool {
    let idiom: isize = unsafe {
        let device: *mut Object = msg_send![class!(UIDevice), currentDevice];
        msg_send![device, userInterfaceIdiom]
    };
    idiom == 0
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
        Some(Self::maximum_frames_per_second())
    }
}
