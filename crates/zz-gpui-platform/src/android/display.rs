use crate::android::bridge;
use anyhow::Result;
use ndk_sys::{AChoreographer_getInstance, AChoreographer_postFrameCallback64};
use std::{cell::Cell, ffi::c_void};
use uuid::Uuid;
use zz_gpui::{Bounds, DisplayId, Pixels, PlatformDisplay, point, px, size};

const IDLE_FRAMES_BEFORE_PAUSE: u8 = 3;

#[derive(Debug)]
pub(crate) struct AndroidDisplay;

thread_local! {
    static SCREEN: Cell<(i32, i32)> = const { Cell::new((0, 0)) };
}

pub(crate) fn set_screen_pixels(width: i32, height: i32) {
    SCREEN.set((width, height));
}

impl PlatformDisplay for AndroidDisplay {
    fn id(&self) -> DisplayId {
        DisplayId::from(0u64)
    }

    fn uuid(&self) -> Result<Uuid> {
        Ok(Uuid::from_u128(0x5a5a_0000_0000_0000_0000_0000_0000_0002))
    }

    fn bounds(&self) -> Bounds<Pixels> {
        let (width, height) = SCREEN.get();
        let density = bridge::configuration().density;
        Bounds {
            origin: point(px(0.0), px(0.0)),
            size: size(px(width as f32 / density), px(height as f32 / density)),
        }
    }

    fn refresh_rate(&self) -> Option<f32> {
        Some(bridge::configuration().refresh_rate)
    }
}

/// Paces frames with the Choreographer, the way the iOS backend uses its
/// display link: a frame callback runs while there is work and stops three
/// idle frames after the last request.
pub(crate) struct FrameLink {
    key: usize,
    callback: unsafe extern "C" fn(i64, *mut c_void),
    posted: Cell<bool>,
    demand: Cell<bool>,
    idle: Cell<u8>,
}

impl FrameLink {
    pub(crate) fn new(key: usize, callback: unsafe extern "C" fn(i64, *mut c_void)) -> Self {
        Self {
            key,
            callback,
            posted: Cell::new(false),
            demand: Cell::new(true),
            idle: Cell::new(0),
        }
    }

    fn post(&self) {
        if self.posted.replace(true) {
            return;
        }
        unsafe {
            let choreographer = AChoreographer_getInstance();
            if choreographer.is_null() {
                self.posted.set(false);
                return;
            }
            AChoreographer_postFrameCallback64(
                choreographer,
                Some(self.callback),
                self.key as *mut c_void,
            );
        }
    }

    pub(crate) fn wake(&self) {
        self.demand.set(true);
        self.idle.set(0);
        self.post();
    }

    pub(crate) fn begin_frame(&self) {
        self.posted.set(false);
        self.demand.set(false);
    }

    pub(crate) fn end_frame(&self, busy: bool) {
        if busy || self.demand.get() {
            self.idle.set(0);
            self.post();
            return;
        }
        let idle = self.idle.get().saturating_add(1);
        self.idle.set(idle);
        if idle < IDLE_FRAMES_BEFORE_PAUSE {
            self.post();
        }
    }
}
