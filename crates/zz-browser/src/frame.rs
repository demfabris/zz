use std::sync::Arc;

use parking_lot::Mutex;
use thiserror::Error;

use crate::SessionId;

#[cfg(target_os = "windows")]
use gpui::windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11DeviceContext};
#[cfg(target_os = "macos")]
use objc2_core_foundation::CFRetained;
#[cfg(target_os = "macos")]
use objc2_io_surface::IOSurfaceRef;

const BYTES_PER_PIXEL: usize = 4;
const MAX_FRAME_BYTES: usize = 512 * 1024 * 1024;
const MAX_RECYCLED_BUFFERS: usize = 3;
const MAX_RECYCLED_CAPACITY_SCALE: usize = 2;

/// GPUI's renderer device, handed to zz-browser at session creation: wgpu
/// everywhere except Windows, where GPUI owns a native D3D11 device.
#[derive(Clone, Debug)]
pub struct BrowserGpuContext {
    #[cfg(not(target_os = "windows"))]
    #[cfg_attr(
        target_os = "macos",
        allow(dead_code, reason = "macOS uses the native Metal-IOSurface producer")
    )]
    pub(crate) device: Arc<wgpu::Device>,
    #[cfg(not(target_os = "windows"))]
    #[cfg_attr(
        target_os = "macos",
        allow(dead_code, reason = "macOS uses the native Metal-IOSurface producer")
    )]
    pub(crate) queue: Arc<wgpu::Queue>,
    #[cfg(target_os = "windows")]
    pub(crate) device: ID3D11Device,
    #[cfg(target_os = "windows")]
    pub(crate) device_context: ID3D11DeviceContext,
}

impl BrowserGpuContext {
    #[cfg(not(target_os = "windows"))]
    #[must_use]
    pub fn new(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>) -> Self {
        Self { device, queue }
    }

    /// Adopt GPUI's DirectX device and immediate context, bumping their COM
    /// refcounts.
    #[cfg(target_os = "windows")]
    #[must_use]
    pub fn from_directx(context: gpui::DirectXDeviceContext) -> Self {
        Self {
            device: context.device,
            device_context: context.device_context,
        }
    }
}

fn diagnostic_timer() -> Option<std::time::Instant> {
    log::log_enabled!(
        target: "zz_browser::diagnostics::frame_mailbox",
        log::Level::Trace
    )
    .then(std::time::Instant::now)
}

fn diagnostic_elapsed_us(started: Option<std::time::Instant>) -> u128 {
    started.map_or(0, |started| started.elapsed().as_micros())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameTier {
    OwnedBgra,
    Gpu,
    #[cfg(target_os = "macos")]
    MacGpu,
    #[cfg(target_os = "windows")]
    WinGpu,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameDamage {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl FrameDamage {
    fn union(self, other: Self) -> Self {
        let left = self.x.min(other.x);
        let top = self.y.min(other.y);
        let right = self
            .x
            .saturating_add(self.width)
            .max(other.x.saturating_add(other.width));
        let bottom = self
            .y
            .saturating_add(self.height)
            .max(other.y.saturating_add(other.height));
        Self {
            x: left,
            y: top,
            width: right.saturating_sub(left),
            height: bottom.saturating_sub(top),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedBgraFrame {
    pub session: SessionId,
    pub generation: u64,
    pub delivery_generation: u64,
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
    pub damage: Option<FrameDamage>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpuFrame {
    pub session: SessionId,
    pub generation: u64,
    pub delivery_generation: u64,
    pub pool_generation: u64,
    pub sequence: u64,
    pub logical_width: u32,
    pub logical_height: u32,
    pub device_width: u32,
    pub device_height: u32,
    pub texture: wgpu::Texture,
}

/// A retained `IOSurface` that can be imported by GPUI's Metal device.
#[cfg(target_os = "macos")]
#[derive(Clone)]
pub struct MacIoSurface {
    inner: CFRetained<IOSurfaceRef>,
}

#[cfg(target_os = "macos")]
impl MacIoSurface {
    pub(crate) fn new(inner: CFRetained<IOSurfaceRef>) -> Self {
        Self { inner }
    }

    #[must_use]
    pub fn as_ptr(&self) -> *mut core::ffi::c_void {
        CFRetained::as_ptr(&self.inner).as_ptr().cast()
    }
}

#[cfg(target_os = "macos")]
impl std::fmt::Debug for MacIoSurface {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("MacIoSurface")
            .field(&self.as_ptr())
            .finish()
    }
}

#[cfg(target_os = "macos")]
impl PartialEq for MacIoSurface {
    fn eq(&self, other: &Self) -> bool {
        self.as_ptr() == other.as_ptr()
    }
}

#[cfg(target_os = "macos")]
impl Eq for MacIoSurface {}

// SAFETY: IOSurface objects are explicitly shareable across threads and
// processes, and `CFRetained` keeps this immutable handle alive.
#[cfg(target_os = "macos")]
#[allow(
    unsafe_code,
    reason = "IOSurface is a thread-safe cross-device sharing primitive"
)]
unsafe impl Send for MacIoSurface {}

// SAFETY: IOSurface's CoreFoundation object is safe to reference concurrently;
// access synchronization belongs to the GPU producers and consumers.
#[cfg(target_os = "macos")]
#[allow(
    unsafe_code,
    reason = "IOSurface is a thread-safe cross-device sharing primitive"
)]
unsafe impl Sync for MacIoSurface {}

#[cfg(target_os = "macos")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacGpuFrame {
    pub session: SessionId,
    pub generation: u64,
    pub delivery_generation: u64,
    pub pool_generation: u64,
    pub sequence: u64,
    pub logical_width: u32,
    pub logical_height: u32,
    pub device_width: u32,
    pub device_height: u32,
    pub io_surface: MacIoSurface,
}

/// A zz-owned D3D11 destination texture on GPUI's device, ready for
/// `gpui::external_texture`. GPUI's `windows` crate version, not zz's: only the
/// re-exported one is type-compatible with the renderer.
#[cfg(target_os = "windows")]
pub type WinGpuTexture = gpui::windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;

/// A browser frame living in a zz-owned texture on GPUI's DirectX device.
#[cfg(target_os = "windows")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WinGpuFrame {
    pub session: SessionId,
    pub generation: u64,
    pub delivery_generation: u64,
    pub pool_generation: u64,
    pub sequence: u64,
    pub logical_width: u32,
    pub logical_height: u32,
    pub device_width: u32,
    pub device_height: u32,
    pub texture: WinGpuTexture,
}

#[cfg(all(
    feature = "cef-runtime",
    not(any(target_os = "macos", target_os = "windows"))
))]
pub(crate) struct GpuFrameSubmission {
    pub session: SessionId,
    pub logical_width: u32,
    pub logical_height: u32,
    pub device_width: i32,
    pub device_height: i32,
    pub pool_generation: u64,
    pub sequence: u64,
    pub texture: wgpu::Texture,
}

#[cfg(feature = "cef-runtime")]
#[derive(Clone, Copy)]
struct GpuPublishFields {
    session: SessionId,
    logical_width: u32,
    logical_height: u32,
    device_width: u32,
    device_height: u32,
    pool_generation: u64,
    sequence: u64,
}

#[cfg(all(feature = "cef-runtime", target_os = "macos"))]
pub(crate) struct MacGpuFrameSubmission {
    pub session: SessionId,
    pub logical_width: u32,
    pub logical_height: u32,
    pub device_width: i32,
    pub device_height: i32,
    pub pool_generation: u64,
    pub sequence: u64,
    pub io_surface: MacIoSurface,
}

#[cfg(all(feature = "cef-runtime", target_os = "windows"))]
pub(crate) struct WinGpuFrameSubmission {
    pub session: SessionId,
    pub logical_width: u32,
    pub logical_height: u32,
    pub device_width: i32,
    pub device_height: i32,
    pub pool_generation: u64,
    pub sequence: u64,
    pub texture: WinGpuTexture,
}

/// The newest complete browser frame, either owned BGRA bytes or a zz-owned GPU texture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OsrFrame {
    OwnedBgra(OwnedBgraFrame),
    Gpu(GpuFrame),
    #[cfg(target_os = "macos")]
    MacGpu(MacGpuFrame),
    #[cfg(target_os = "windows")]
    WinGpu(WinGpuFrame),
}

impl OsrFrame {
    #[must_use]
    pub fn session(&self) -> SessionId {
        match self {
            Self::OwnedBgra(frame) => frame.session,
            Self::Gpu(frame) => frame.session,
            #[cfg(target_os = "macos")]
            Self::MacGpu(frame) => frame.session,
            #[cfg(target_os = "windows")]
            Self::WinGpu(frame) => frame.session,
        }
    }

    #[must_use]
    pub fn generation(&self) -> u64 {
        match self {
            Self::OwnedBgra(frame) => frame.generation,
            Self::Gpu(frame) => frame.generation,
            #[cfg(target_os = "macos")]
            Self::MacGpu(frame) => frame.generation,
            #[cfg(target_os = "windows")]
            Self::WinGpu(frame) => frame.generation,
        }
    }

    #[must_use]
    pub fn delivery_generation(&self) -> u64 {
        match self {
            Self::OwnedBgra(frame) => frame.delivery_generation,
            Self::Gpu(frame) => frame.delivery_generation,
            #[cfg(target_os = "macos")]
            Self::MacGpu(frame) => frame.delivery_generation,
            #[cfg(target_os = "windows")]
            Self::WinGpu(frame) => frame.delivery_generation,
        }
    }

    #[must_use]
    pub fn tier(&self) -> FrameTier {
        match self {
            Self::OwnedBgra(_) => FrameTier::OwnedBgra,
            Self::Gpu(_) => FrameTier::Gpu,
            #[cfg(target_os = "macos")]
            Self::MacGpu(_) => FrameTier::MacGpu,
            #[cfg(target_os = "windows")]
            Self::WinGpu(_) => FrameTier::WinGpu,
        }
    }

    #[must_use]
    pub fn device_width(&self) -> u32 {
        match self {
            Self::OwnedBgra(frame) => frame.width,
            Self::Gpu(frame) => frame.device_width,
            #[cfg(target_os = "macos")]
            Self::MacGpu(frame) => frame.device_width,
            #[cfg(target_os = "windows")]
            Self::WinGpu(frame) => frame.device_width,
        }
    }

    #[must_use]
    pub fn device_height(&self) -> u32 {
        match self {
            Self::OwnedBgra(frame) => frame.height,
            Self::Gpu(frame) => frame.device_height,
            #[cfg(target_os = "macos")]
            Self::MacGpu(frame) => frame.device_height,
            #[cfg(target_os = "windows")]
            Self::WinGpu(frame) => frame.device_height,
        }
    }

    fn pixel_bytes(&self) -> usize {
        match self {
            Self::OwnedBgra(frame) => frame.bgra.len(),
            Self::Gpu(_) => 0,
            #[cfg(target_os = "macos")]
            Self::MacGpu(_) => 0,
            #[cfg(target_os = "windows")]
            Self::WinGpu(_) => 0,
        }
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum FrameError {
    #[error("browser frame dimensions must be positive")]
    NonPositiveDimensions,
    #[error("browser frame dimensions overflow the address space")]
    DimensionOverflow,
    #[error("browser frame exceeds the {MAX_FRAME_BYTES} byte safety limit")]
    TooLarge,
    #[error("browser frame has {actual} bytes, expected {expected}")]
    InvalidLength { expected: usize, actual: usize },
    #[error(
        "browser GPU texture is {actual_width}x{actual_height}, expected {expected_width}x{expected_height}"
    )]
    InvalidTextureDimensions {
        expected_width: u32,
        expected_height: u32,
        actual_width: u32,
        actual_height: u32,
    },
}

#[derive(Default)]
struct FrameDeliveryState {
    active_tier: Option<FrameTier>,
    generation: u64,
    transition_count: u64,
    fallback_pending: bool,
    gpu_import_failure_count: u64,
}

impl FrameDeliveryState {
    fn publish(&mut self, tier: FrameTier) -> u64 {
        if self.active_tier != Some(tier) {
            if self.active_tier.is_some() {
                self.transition_count += 1;
            }
            self.generation = self.generation.wrapping_add(1).max(1);
            self.active_tier = Some(tier);
        }
        self.fallback_pending = false;
        self.generation
    }

    fn record_gpu_import_failure(&mut self) {
        self.gpu_import_failure_count += 1;
        self.fallback_pending = true;
    }
}

#[derive(Default)]
struct MailboxState {
    generation: u64,
    pending: Option<OsrFrame>,
    recycled: Vec<Vec<u8>>,
    frame_bytes: usize,
    wake_pending: bool,
    delivery: FrameDeliveryState,
    owned_bgra_published: u64,
    gpu_published: u64,
    #[cfg(target_os = "macos")]
    mac_gpu_published: u64,
    #[cfg(target_os = "windows")]
    win_gpu_published: u64,
    owned_bgra_taken: u64,
    gpu_taken: u64,
    #[cfg(target_os = "macos")]
    mac_gpu_taken: u64,
    #[cfg(target_os = "windows")]
    win_gpu_taken: u64,
}

/// A one-slot mailbox that replaces stale browser frames under load.
#[derive(Clone, Default)]
pub struct FrameMailbox {
    state: Arc<Mutex<MailboxState>>,
}

impl FrameMailbox {
    /// Publish one complete BGRA frame and report whether the consumer needs a
    /// wake. The pixel allocation passes through untouched, so the consumer owns it.
    pub fn publish(
        &self,
        session: SessionId,
        width: i32,
        height: i32,
        bgra: Vec<u8>,
        mut damage: Option<FrameDamage>,
    ) -> Result<Option<u64>, FrameError> {
        let expected = frame_byte_len(width, height)?;
        if bgra.len() != expected {
            return Err(FrameError::InvalidLength {
                expected,
                actual: bgra.len(),
            });
        }

        let started = diagnostic_timer();
        let mut state = self.state.lock();
        let replaced_pending = state.pending.is_some();
        state.frame_bytes = expected;
        state.generation = state.generation.wrapping_add(1).max(1);
        let generation = state.generation;
        let delivery_generation = state.delivery.publish(FrameTier::OwnedBgra);
        state.owned_bgra_published += 1;
        let replaced = state.pending.take();
        if let Some(replaced) = replaced.as_ref() {
            let displaced_damage = match replaced {
                OsrFrame::OwnedBgra(frame) => frame.damage,
                OsrFrame::Gpu(_) => None,
                #[cfg(target_os = "macos")]
                OsrFrame::MacGpu(_) => None,
                #[cfg(target_os = "windows")]
                OsrFrame::WinGpu(_) => None,
            };
            damage = conflate_damage(displaced_damage, damage);
        }
        state.pending = Some(OsrFrame::OwnedBgra(OwnedBgraFrame {
            session,
            generation,
            delivery_generation,
            width: width.cast_unsigned(),
            height: height.cast_unsigned(),
            bgra,
            damage,
        }));
        if let Some(replaced) = replaced {
            recycle_displaced_frame(&mut state, replaced);
        }
        let wake = if state.wake_pending {
            None
        } else {
            state.wake_pending = true;
            Some(generation)
        };
        log::trace!(
            target: "zz_browser::diagnostics::frame_mailbox",
            "publish session={} generation={} width={} height={} bytes={} replaced_pending={} wake={wake:?} elapsed_us={}",
            session.0,
            generation,
            width,
            height,
            expected,
            replaced_pending,
            diagnostic_elapsed_us(started),
        );
        Ok(wake)
    }

    #[cfg(all(
        feature = "cef-runtime",
        not(any(target_os = "macos", target_os = "windows"))
    ))]
    pub(crate) fn publish_gpu(
        &self,
        submission: GpuFrameSubmission,
    ) -> Result<Option<u64>, FrameError> {
        let GpuFrameSubmission {
            session,
            logical_width,
            logical_height,
            device_width,
            device_height,
            pool_generation,
            sequence,
            texture,
        } = submission;
        validate_frame_dimensions(device_width, device_height)?;
        if logical_width == 0 || logical_height == 0 {
            return Err(FrameError::NonPositiveDimensions);
        }
        let expected_width = device_width.cast_unsigned();
        let expected_height = device_height.cast_unsigned();
        if texture.width() != expected_width || texture.height() != expected_height {
            return Err(FrameError::InvalidTextureDimensions {
                expected_width,
                expected_height,
                actual_width: texture.width(),
                actual_height: texture.height(),
            });
        }

        Ok(self.publish_gpu_tier(
            "publish_gpu",
            FrameTier::Gpu,
            GpuPublishFields {
                session,
                logical_width,
                logical_height,
                device_width: expected_width,
                device_height: expected_height,
                pool_generation,
                sequence,
            },
            |generation, delivery_generation| {
                OsrFrame::Gpu(GpuFrame {
                    session,
                    generation,
                    delivery_generation,
                    pool_generation,
                    sequence,
                    logical_width,
                    logical_height,
                    device_width: expected_width,
                    device_height: expected_height,
                    texture,
                })
            },
        ))
    }

    #[cfg(all(feature = "cef-runtime", target_os = "macos"))]
    pub(crate) fn publish_mac_gpu(
        &self,
        submission: MacGpuFrameSubmission,
    ) -> Result<Option<u64>, FrameError> {
        let MacGpuFrameSubmission {
            session,
            logical_width,
            logical_height,
            device_width,
            device_height,
            pool_generation,
            sequence,
            io_surface,
        } = submission;
        validate_frame_dimensions(device_width, device_height)?;
        if logical_width == 0 || logical_height == 0 {
            return Err(FrameError::NonPositiveDimensions);
        }

        let expected_width = device_width.cast_unsigned();
        let expected_height = device_height.cast_unsigned();
        Ok(self.publish_gpu_tier(
            "publish_mac_gpu",
            FrameTier::MacGpu,
            GpuPublishFields {
                session,
                logical_width,
                logical_height,
                device_width: expected_width,
                device_height: expected_height,
                pool_generation,
                sequence,
            },
            |generation, delivery_generation| {
                OsrFrame::MacGpu(MacGpuFrame {
                    session,
                    generation,
                    delivery_generation,
                    pool_generation,
                    sequence,
                    logical_width,
                    logical_height,
                    device_width: expected_width,
                    device_height: expected_height,
                    io_surface,
                })
            },
        ))
    }

    #[cfg(all(feature = "cef-runtime", target_os = "windows"))]
    pub(crate) fn publish_win_gpu(
        &self,
        submission: WinGpuFrameSubmission,
    ) -> Result<Option<u64>, FrameError> {
        let WinGpuFrameSubmission {
            session,
            logical_width,
            logical_height,
            device_width,
            device_height,
            pool_generation,
            sequence,
            texture,
        } = submission;
        validate_frame_dimensions(device_width, device_height)?;
        if logical_width == 0 || logical_height == 0 {
            return Err(FrameError::NonPositiveDimensions);
        }

        let expected_width = device_width.cast_unsigned();
        let expected_height = device_height.cast_unsigned();
        Ok(self.publish_gpu_tier(
            "publish_win_gpu",
            FrameTier::WinGpu,
            GpuPublishFields {
                session,
                logical_width,
                logical_height,
                device_width: expected_width,
                device_height: expected_height,
                pool_generation,
                sequence,
            },
            |generation, delivery_generation| {
                OsrFrame::WinGpu(WinGpuFrame {
                    session,
                    generation,
                    delivery_generation,
                    pool_generation,
                    sequence,
                    logical_width,
                    logical_height,
                    device_width: expected_width,
                    device_height: expected_height,
                    texture,
                })
            },
        ))
    }

    #[cfg(feature = "cef-runtime")]
    fn publish_gpu_tier(
        &self,
        label: &str,
        tier: FrameTier,
        fields: GpuPublishFields,
        build: impl FnOnce(u64, u64) -> OsrFrame,
    ) -> Option<u64> {
        let started = diagnostic_timer();
        let mut state = self.state.lock();
        let replaced_pending = state.pending.is_some();
        state.generation = state.generation.wrapping_add(1).max(1);
        let generation = state.generation;
        let delivery_generation = state.delivery.publish(tier);
        match tier {
            FrameTier::OwnedBgra => state.owned_bgra_published += 1,
            FrameTier::Gpu => state.gpu_published += 1,
            #[cfg(target_os = "macos")]
            FrameTier::MacGpu => state.mac_gpu_published += 1,
            #[cfg(target_os = "windows")]
            FrameTier::WinGpu => state.win_gpu_published += 1,
        }
        let replaced = state
            .pending
            .replace(build(generation, delivery_generation));
        if let Some(replaced) = replaced {
            recycle_displaced_frame(&mut state, replaced);
        }
        let wake = if state.wake_pending {
            None
        } else {
            state.wake_pending = true;
            Some(generation)
        };
        log::trace!(
            target: "zz_browser::diagnostics::frame_mailbox",
            "{label} session={} generation={} delivery_generation={} pool_generation={} sequence={} logical={}x{} device={}x{} replaced_pending={} wake={wake:?} elapsed_us={}",
            fields.session.0,
            generation,
            delivery_generation,
            fields.pool_generation,
            fields.sequence,
            fields.logical_width,
            fields.logical_height,
            fields.device_width,
            fields.device_height,
            replaced_pending,
            diagnostic_elapsed_us(started),
        );
        wake
    }

    #[cfg(feature = "cef-runtime")]
    pub(crate) fn record_gpu_import_failure(&self) {
        self.state.lock().delivery.record_gpu_import_failure();
    }

    #[cfg(any(feature = "cef-runtime", test))]
    pub(crate) fn take_buffer(&self, frame_bytes: usize) -> Vec<u8> {
        let mut state = self.state.lock();
        state.frame_bytes = frame_bytes;
        state
            .recycled
            .retain(|buffer| buffer_capacity_fits(buffer.capacity(), frame_bytes));
        let index = state
            .recycled
            .iter()
            .enumerate()
            .filter(|(_, buffer)| buffer.capacity() >= frame_bytes)
            .min_by_key(|(_, buffer)| buffer.capacity())
            .or_else(|| {
                state
                    .recycled
                    .iter()
                    .enumerate()
                    .max_by_key(|(_, buffer)| buffer.capacity())
            })
            .map(|(index, _)| index);
        let buffer = index.map_or_else(Vec::new, |index| state.recycled.swap_remove(index));
        log::trace!(
            target: "zz_browser::diagnostics::frame_mailbox",
            "take_buffer frame_bytes={frame_bytes} pool_hit={} pooled_remaining={}",
            buffer.capacity() > 0,
            state.recycled.len(),
        );
        buffer
    }

    /// Return a consumed pixel buffer for reuse by a future paint callback.
    pub fn recycle(&self, bgra: Vec<u8>) {
        recycle_buffer(&mut self.state.lock(), bgra);
    }

    #[must_use]
    pub fn take(&self) -> Option<OsrFrame> {
        let started = diagnostic_timer();
        let mut state = self.state.lock();
        state.wake_pending = false;
        let frame = state.pending.take();
        match frame.as_ref().map(OsrFrame::tier) {
            Some(FrameTier::OwnedBgra) => state.owned_bgra_taken += 1,
            Some(FrameTier::Gpu) => state.gpu_taken += 1,
            #[cfg(target_os = "macos")]
            Some(FrameTier::MacGpu) => state.mac_gpu_taken += 1,
            #[cfg(target_os = "windows")]
            Some(FrameTier::WinGpu) => state.win_gpu_taken += 1,
            None => {}
        }
        log::trace!(
            target: "zz_browser::diagnostics::frame_mailbox",
            "take generation={} returned_generation={:?} returned_tier={:?} bytes={} elapsed_us={}",
            state.generation,
            frame.as_ref().map(OsrFrame::generation),
            frame.as_ref().map(OsrFrame::tier),
            frame.as_ref().map_or(0, OsrFrame::pixel_bytes),
            diagnostic_elapsed_us(started),
        );
        frame
    }

    #[must_use]
    pub fn diagnostics(&self) -> FrameMailboxDiagnostics {
        let state = self.state.lock();
        FrameMailboxDiagnostics {
            generation: state.generation,
            pending_generation: state.pending.as_ref().map(OsrFrame::generation),
            pending_tier: state.pending.as_ref().map(OsrFrame::tier),
            pending_bytes: state.pending.as_ref().map_or(0, OsrFrame::pixel_bytes),
            wake_pending: state.wake_pending,
            active_tier: state.delivery.active_tier,
            delivery_generation: state.delivery.generation,
            tier_transition_count: state.delivery.transition_count,
            fallback_pending: state.delivery.fallback_pending,
            gpu_import_failure_count: state.delivery.gpu_import_failure_count,
            owned_bgra_published: state.owned_bgra_published,
            gpu_published: state.gpu_published,
            #[cfg(target_os = "macos")]
            mac_gpu_published: state.mac_gpu_published,
            #[cfg(target_os = "windows")]
            win_gpu_published: state.win_gpu_published,
            owned_bgra_taken: state.owned_bgra_taken,
            gpu_taken: state.gpu_taken,
            #[cfg(target_os = "macos")]
            mac_gpu_taken: state.mac_gpu_taken,
            #[cfg(target_os = "windows")]
            win_gpu_taken: state.win_gpu_taken,
        }
    }

    pub fn clear(&self) {
        let mut state = self.state.lock();
        state.pending = None;
        state.recycled.clear();
        state.frame_bytes = 0;
        state.wake_pending = false;
        state.delivery.fallback_pending = false;
    }
}

fn conflate_damage(
    displaced: Option<FrameDamage>,
    incoming: Option<FrameDamage>,
) -> Option<FrameDamage> {
    Some(displaced?.union(incoming?))
}

fn recycle_displaced_frame(state: &mut MailboxState, frame: OsrFrame) {
    if let OsrFrame::OwnedBgra(frame) = frame {
        recycle_buffer(state, frame.bgra);
    }
}

fn recycle_buffer(state: &mut MailboxState, mut buffer: Vec<u8>) {
    if buffer.capacity() == 0
        || state.recycled.len() >= MAX_RECYCLED_BUFFERS
        || !buffer_capacity_fits(buffer.capacity(), state.frame_bytes)
    {
        return;
    }
    buffer.clear();
    state.recycled.push(buffer);
}

fn buffer_capacity_fits(capacity: usize, frame_bytes: usize) -> bool {
    frame_bytes != 0 && capacity <= frame_bytes.saturating_mul(MAX_RECYCLED_CAPACITY_SCALE)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameMailboxDiagnostics {
    pub generation: u64,
    pub pending_generation: Option<u64>,
    pub pending_tier: Option<FrameTier>,
    pub pending_bytes: usize,
    pub wake_pending: bool,
    pub active_tier: Option<FrameTier>,
    pub delivery_generation: u64,
    pub tier_transition_count: u64,
    pub fallback_pending: bool,
    pub gpu_import_failure_count: u64,
    pub owned_bgra_published: u64,
    pub gpu_published: u64,
    #[cfg(target_os = "macos")]
    pub mac_gpu_published: u64,
    #[cfg(target_os = "windows")]
    pub win_gpu_published: u64,
    pub owned_bgra_taken: u64,
    pub gpu_taken: u64,
    #[cfg(target_os = "macos")]
    pub mac_gpu_taken: u64,
    #[cfg(target_os = "windows")]
    pub win_gpu_taken: u64,
}

#[cfg(feature = "cef-runtime")]
fn validate_frame_dimensions(width: i32, height: i32) -> Result<(), FrameError> {
    frame_byte_len(width, height).map(|_| ())
}

pub(crate) fn frame_byte_len(width: i32, height: i32) -> Result<usize, FrameError> {
    let width = usize::try_from(width).map_err(|_| FrameError::NonPositiveDimensions)?;
    let height = usize::try_from(height).map_err(|_| FrameError::NonPositiveDimensions)?;
    if width == 0 || height == 0 {
        return Err(FrameError::NonPositiveDimensions);
    }
    let bytes = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(BYTES_PER_PIXEL))
        .ok_or(FrameError::DimensionOverflow)?;
    if bytes > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_frame_dimensions_and_length() {
        assert_eq!(frame_byte_len(2, 3), Ok(24));
        assert_eq!(frame_byte_len(0, 3), Err(FrameError::NonPositiveDimensions));
        assert_eq!(
            frame_byte_len(-1, 3),
            Err(FrameError::NonPositiveDimensions)
        );
        assert_eq!(
            frame_byte_len(i32::MAX, i32::MAX),
            Err(FrameError::TooLarge)
        );
    }

    #[test]
    fn replaces_stale_frames() {
        let mailbox = FrameMailbox::default();
        let first_wake = mailbox
            .publish(SessionId(1), 1, 1, vec![1; 4], None)
            .unwrap();
        let second_wake = mailbox
            .publish(SessionId(1), 1, 1, vec![2; 4], None)
            .unwrap();

        let OsrFrame::OwnedBgra(frame) = mailbox.take().unwrap() else {
            panic!("expected an owned BGRA frame");
        };
        assert_eq!(first_wake, Some(1));
        assert_eq!(second_wake, None);
        assert_eq!(frame.generation, 2);
        assert_eq!(frame.bgra, vec![2; 4]);
        assert!(mailbox.take().is_none());
        assert_eq!(
            mailbox.publish(SessionId(1), 1, 1, vec![3; 4], None),
            Ok(Some(3))
        );
    }

    #[test]
    fn conflated_owned_frames_union_damage_bounds() {
        let mailbox = FrameMailbox::default();
        mailbox
            .publish(
                SessionId(1),
                4,
                3,
                vec![1; 48],
                Some(FrameDamage {
                    x: 2,
                    y: 1,
                    width: 2,
                    height: 1,
                }),
            )
            .unwrap();
        mailbox
            .publish(
                SessionId(1),
                4,
                3,
                vec![2; 48],
                Some(FrameDamage {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 3,
                }),
            )
            .unwrap();

        let OsrFrame::OwnedBgra(frame) = mailbox.take().unwrap() else {
            panic!("expected an owned BGRA frame");
        };
        assert_eq!(
            frame.damage,
            Some(FrameDamage {
                x: 0,
                y: 0,
                width: 4,
                height: 3,
            })
        );
    }

    #[test]
    fn unknown_damage_poisons_conflated_owned_frames() {
        for (first, second) in [
            (
                Some(FrameDamage {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                }),
                None,
            ),
            (
                None,
                Some(FrameDamage {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                }),
            ),
        ] {
            let mailbox = FrameMailbox::default();
            mailbox
                .publish(SessionId(1), 1, 1, vec![1; 4], first)
                .unwrap();
            mailbox
                .publish(SessionId(1), 1, 1, vec![2; 4], second)
                .unwrap();
            let OsrFrame::OwnedBgra(frame) = mailbox.take().unwrap() else {
                panic!("expected an owned BGRA frame");
            };
            assert_eq!(frame.damage, None);
        }
    }

    #[test]
    fn preserves_the_published_pixel_allocation() {
        let mailbox = FrameMailbox::default();
        let bgra = vec![1, 2, 3, 4];
        let allocation = bgra.as_ptr();

        mailbox.publish(SessionId(1), 1, 1, bgra, None).unwrap();

        let OsrFrame::OwnedBgra(frame) = mailbox.take().unwrap() else {
            panic!("expected an owned BGRA frame");
        };
        assert_eq!(frame.bgra.as_ptr(), allocation);
    }

    #[test]
    fn reuses_the_displaced_pending_frame_buffer() {
        let mailbox = FrameMailbox::default();
        let first = vec![1; 16];
        let allocation = first.as_ptr();
        mailbox.publish(SessionId(1), 2, 2, first, None).unwrap();
        mailbox
            .publish(SessionId(1), 2, 2, vec![2; 16], None)
            .unwrap();

        let recycled = mailbox.take_buffer(16);
        assert_eq!(recycled.as_ptr(), allocation);
        assert_eq!(recycled.capacity(), 16);
        assert!(recycled.is_empty());
    }

    #[test]
    fn bounds_recycled_buffers_and_drops_oversized_resize_allocations() {
        let mailbox = FrameMailbox::default();
        let _ = mailbox.take_buffer(16);
        for _ in 0..=MAX_RECYCLED_BUFFERS {
            mailbox.recycle(Vec::with_capacity(16));
        }
        assert_eq!(mailbox.state.lock().recycled.len(), MAX_RECYCLED_BUFFERS);

        let resized = mailbox.take_buffer(4);
        assert_eq!(resized.capacity(), 0);
        assert!(mailbox.state.lock().recycled.is_empty());
    }

    #[test]
    fn rejects_invalid_buffer_length() {
        let mailbox = FrameMailbox::default();
        assert_eq!(
            mailbox.publish(SessionId(1), 2, 2, vec![0; 15], None),
            Err(FrameError::InvalidLength {
                expected: 16,
                actual: 15,
            })
        );
    }

    #[test]
    fn owned_frame_exposes_enum_metadata_without_copying() {
        let mailbox = FrameMailbox::default();
        mailbox
            .publish(SessionId(9), 2, 1, vec![7; 8], None)
            .unwrap();

        let frame = mailbox.take().unwrap();
        assert_eq!(frame.tier(), FrameTier::OwnedBgra);
        assert_eq!(frame.session(), SessionId(9));
        assert_eq!(frame.generation(), 1);
        assert_eq!(frame.delivery_generation(), 1);
        assert_eq!(frame.device_width(), 2);
        assert_eq!(frame.device_height(), 1);
        assert!(matches!(frame, OsrFrame::OwnedBgra(_)));
    }

    #[test]
    fn delivery_state_keeps_the_last_tier_while_fallback_is_pending() {
        let mut state = FrameDeliveryState::default();
        assert_eq!(state.publish(FrameTier::Gpu), 1);
        state.record_gpu_import_failure();
        assert_eq!(state.active_tier, Some(FrameTier::Gpu));
        assert!(state.fallback_pending);

        assert_eq!(state.publish(FrameTier::OwnedBgra), 2);
        assert_eq!(state.active_tier, Some(FrameTier::OwnedBgra));
        assert!(!state.fallback_pending);
        assert_eq!(state.transition_count, 1);
        assert_eq!(state.publish(FrameTier::Gpu), 3);
        assert_eq!(state.transition_count, 2);
    }
}
