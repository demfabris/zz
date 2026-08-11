use std::ptr::{null, null_mut};

use core_foundation::base::{TCFType, kCFAllocatorDefault};
use core_video::{
    pixel_buffer::{
        CVPixelBuffer, CVPixelBufferRef, kCVPixelBufferLock_ReadOnly, kCVPixelFormatType_32BGRA,
    },
    pixel_buffer_io_surface::CVPixelBufferCreateWithIOSurface,
    r#return::{CVReturn, kCVReturnSuccess},
};
use thiserror::Error;
use zz_browser::{MacIoSurface, SessionId};

use crate::browser::screenshot::Screenshot;

#[derive(Debug, Error)]
pub(crate) enum MacBrowserSurfaceError {
    #[error("CVPixelBufferCreateWithIOSurface failed with status {0}")]
    Create(CVReturn),
    #[error("IOSurface pixel format is {actual:#x}, expected 32BGRA")]
    PixelFormat { actual: u32 },
}

struct CachedPixelBuffer {
    io_surface_address: usize,
    pixel_buffer: CVPixelBuffer,
}

#[derive(Default)]
pub(crate) struct MacBrowserSurfaceCache {
    session: Option<SessionId>,
    pool_generation: Option<u64>,
    entries: Vec<CachedPixelBuffer>,
}

impl MacBrowserSurfaceCache {
    pub(crate) fn pixel_buffer(
        &mut self,
        session: SessionId,
        pool_generation: u64,
        io_surface: &MacIoSurface,
    ) -> Result<CVPixelBuffer, MacBrowserSurfaceError> {
        if self.session != Some(session) || self.pool_generation != Some(pool_generation) {
            self.session = Some(session);
            self.pool_generation = Some(pool_generation);
            self.entries.clear();
        }
        let address = io_surface.as_ptr().addr();
        if let Some(entry) = self
            .entries
            .iter()
            .find(|entry| entry.io_surface_address == address)
        {
            return Ok(entry.pixel_buffer.clone());
        }

        let pixel_buffer = create_pixel_buffer(io_surface)?;
        self.entries.push(CachedPixelBuffer {
            io_surface_address: address,
            pixel_buffer: pixel_buffer.clone(),
        });
        Ok(pixel_buffer)
    }

    pub(crate) fn clear(&mut self) {
        self.session = None;
        self.pool_generation = None;
        self.entries.clear();
    }
}

/// Copy a shared-texture frame back to the CPU so `capture-browser` can encode it.
#[allow(
    unsafe_code,
    reason = "CoreVideo exposes a locked IOSurface's pixels as a raw base address"
)]
pub(crate) fn read_pixel_buffer(pixel_buffer: &CVPixelBuffer) -> Result<Screenshot, String> {
    if pixel_buffer.is_planar() {
        return Err("browser frame is planar; expected packed 32BGRA".to_owned());
    }
    if pixel_buffer.lock_base_address(kCVPixelBufferLock_ReadOnly) != kCVReturnSuccess {
        return Err("could not lock the browser frame for reading".to_owned());
    }
    let stride = pixel_buffer.get_bytes_per_row();
    let height = pixel_buffer.get_height();
    let width = pixel_buffer.get_width();
    // SAFETY: the buffer is locked for reading until `unlock_base_address`
    // below, and CoreVideo guarantees `stride * height` readable bytes behind
    // the base address of a non-planar buffer.
    let copied = unsafe {
        let base = pixel_buffer.get_base_address();
        if base.is_null() {
            None
        } else {
            Some(
                std::slice::from_raw_parts(base.cast::<u8>(), stride.saturating_mul(height))
                    .to_vec(),
            )
        }
    };
    pixel_buffer.unlock_base_address(kCVPixelBufferLock_ReadOnly);
    let copied = copied.ok_or_else(|| "the browser frame has no pixel data".to_owned())?;
    Screenshot::from_bgra_rows(
        u32::try_from(width).map_err(|_| "invalid frame width".to_owned())?,
        u32::try_from(height).map_err(|_| "invalid frame height".to_owned())?,
        stride,
        &copied,
    )
}

#[allow(
    unsafe_code,
    reason = "CoreVideo exposes IOSurface-backed pixel buffers through a C create-rule API"
)]
fn create_pixel_buffer(io_surface: &MacIoSurface) -> Result<CVPixelBuffer, MacBrowserSurfaceError> {
    let mut pixel_buffer: CVPixelBufferRef = null_mut();
    // SAFETY: MacIoSurface retains the IOSurface for this call, the output
    // pointer is valid, and CoreVideo accepts null pixel-buffer attributes.
    let status = unsafe {
        CVPixelBufferCreateWithIOSurface(
            kCFAllocatorDefault,
            io_surface.as_ptr().cast(),
            null(),
            &raw mut pixel_buffer,
        )
    };
    if status != kCVReturnSuccess {
        return Err(MacBrowserSurfaceError::Create(status));
    }
    // SAFETY: a successful CoreVideo create-rule call returned a non-null +1
    // CVPixelBuffer reference owned by the caller.
    let pixel_buffer = unsafe { CVPixelBuffer::wrap_under_create_rule(pixel_buffer) };
    let actual = pixel_buffer.get_pixel_format();
    if actual != kCVPixelFormatType_32BGRA {
        return Err(MacBrowserSurfaceError::PixelFormat { actual });
    }
    Ok(pixel_buffer)
}
