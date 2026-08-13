use std::{collections::BTreeSet, ptr::NonNull, sync::Arc};

use block2::RcBlock;
use cef::{AcceleratedPaintInfo, ColorType};
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFType};
use objc2_io_surface::{
    IOSurfaceRef, kIOSurfaceBytesPerElement, kIOSurfaceHeight, kIOSurfacePixelFormat,
    kIOSurfaceWidth,
};
use objc2_metal::{
    MTLBlitCommandEncoder, MTLCommandBuffer, MTLCommandEncoder, MTLCommandQueue,
    MTLCreateSystemDefaultDevice, MTLDevice, MTLOrigin, MTLPixelFormat, MTLSize, MTLStorageMode,
    MTLTexture, MTLTextureDescriptor, MTLTextureUsage,
};
use parking_lot::Mutex;
use thiserror::Error;

use crate::{
    MacIoSurface, Viewport,
    cef_runtime::{AcceleratedPoolLayout, ExpectedFrameSize, StalePoolFrame},
};

// Five: GPUI's CAMetalLayer allows three drawables in flight plus two producer writes.
const DESTINATION_POOL_SLOT_COUNT: usize = 5;
const MAX_IN_FLIGHT_BLITS: usize = 2;
const BGRA_PIXEL_FORMAT: i64 = 0x4247_5241;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {}

#[derive(Clone)]
pub(super) struct MetalFrameProducer {
    state: Arc<Mutex<MetalFrameProducerState>>,
}

struct MetalFrameProducerState {
    expected: ExpectedFrameSize,
    generation: u64,
    context: Option<MetalContext>,
    initialization_error: Option<String>,
    destinations: Option<DestinationSurfacePool>,
    in_flight_sequences: BTreeSet<u64>,
    last_published_sequence: Option<u64>,
}

struct MetalContext {
    device: Retained<ProtocolObject<dyn MTLDevice>>,
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
}

struct DestinationSurface {
    io_surface: CFRetained<IOSurfaceRef>,
    texture: Retained<ProtocolObject<dyn MTLTexture>>,
    in_flight_sequence: Option<u64>,
}

struct RetainedSourceSurface(CFRetained<IOSurfaceRef>);

// SAFETY: Metal devices and queues are documented for concurrent command
// creation and submission. The producer mutex serializes its own state.
unsafe impl Send for MetalContext {}
unsafe impl Sync for MetalContext {}

// SAFETY: IOSurface is explicitly shareable across threads and processes.
unsafe impl Send for RetainedSourceSurface {}
unsafe impl Sync for RetainedSourceSurface {}

impl RetainedSourceSurface {
    fn as_ref(&self) -> &IOSurfaceRef {
        &self.0
    }
}

// SAFETY: Metal resources and IOSurfaces support cross-thread use, and every
// mutation of these handles is serialized by MetalFrameProducer's mutex.
unsafe impl Send for DestinationSurface {}
unsafe impl Sync for DestinationSurface {}

struct DestinationSurfacePool {
    generation: u64,
    width: u32,
    height: u32,
    surfaces: Vec<DestinationSurface>,
    next: usize,
}

pub(super) struct ProducedMetalFrame {
    pub logical_width: u32,
    pub logical_height: u32,
    pub device_width: i32,
    pub device_height: i32,
    pub pool_generation: u64,
    pub sequence: u64,
    pub io_surface: MacIoSurface,
}

pub(super) enum MetalFrameOutcome {
    Submitted,
    Stale(StalePoolFrame),
}

pub(super) enum MetalFrameCompletion {
    Frame(ProducedMetalFrame),
    Stale {
        sequence: u64,
        reason: StalePoolFrame,
    },
    Failed {
        sequence: u64,
        error: MetalFrameError,
    },
}

#[derive(Debug, Error)]
pub(super) enum MetalFrameError {
    #[error("the system Metal device is unavailable")]
    DeviceUnavailable,
    #[error("Metal initialization failed: {0}")]
    Initialization(String),
    #[error("could not create a Metal command queue")]
    CommandQueueUnavailable,
    #[error("invalid accelerated-paint metadata: {0}")]
    InvalidMetadata(String),
    #[error("could not create an IOSurface-backed browser texture")]
    DestinationTexture,
    #[error("could not create a Metal texture for CEF's IOSurface")]
    SourceTexture,
    #[error("could not create a Metal command buffer")]
    CommandBuffer,
    #[error("could not create a Metal blit command encoder")]
    BlitEncoder,
    #[error("Metal rejected the browser frame blit: {0}")]
    Blit(String),
}

impl MetalContext {
    fn new() -> Result<Self, MetalFrameError> {
        let device = MTLCreateSystemDefaultDevice().ok_or(MetalFrameError::DeviceUnavailable)?;
        let queue = device
            .newCommandQueue()
            .ok_or(MetalFrameError::CommandQueueUnavailable)?;
        Ok(Self { device, queue })
    }

    fn texture_for_surface(
        &self,
        surface: &IOSurfaceRef,
        width: u32,
        height: u32,
        pixel_format: MTLPixelFormat,
        storage_mode: MTLStorageMode,
    ) -> Option<Retained<ProtocolObject<dyn MTLTexture>>> {
        let descriptor = MTLTextureDescriptor::new();
        // SAFETY: the validated frame dimensions fit NSUInteger and describe
        // one non-mipmapped, single-sample 2D texture.
        unsafe {
            descriptor.setWidth(width as _);
            descriptor.setHeight(height as _);
            descriptor.setMipmapLevelCount(1);
            descriptor.setSampleCount(1);
            descriptor.setPixelFormat(pixel_format);
            descriptor.setUsage(MTLTextureUsage::ShaderRead);
            descriptor.setStorageMode(storage_mode);
        }
        self.device
            .newTextureWithDescriptor_iosurface_plane(&descriptor, surface, 0)
    }

    fn create_destination(
        &self,
        width: u32,
        height: u32,
    ) -> Result<DestinationSurface, MetalFrameError> {
        let width_number = CFNumber::new_i64(i64::from(width));
        let height_number = CFNumber::new_i64(i64::from(height));
        let bytes_per_element = CFNumber::new_i64(4);
        let pixel_format = CFNumber::new_i64(BGRA_PIXEL_FORMAT);
        // SAFETY: these are immutable IOSurface framework constants available
        // for the lifetime of the process.
        let keys = unsafe {
            [
                kIOSurfaceWidth.as_ref(),
                kIOSurfaceHeight.as_ref(),
                kIOSurfaceBytesPerElement.as_ref(),
                kIOSurfacePixelFormat.as_ref(),
            ]
        };
        let properties = CFDictionary::<CFType, CFType>::from_slices(
            &keys,
            &[
                width_number.as_ref(),
                height_number.as_ref(),
                bytes_per_element.as_ref(),
                pixel_format.as_ref(),
            ],
        );
        // SAFETY: all required IOSurface dimensions and packed BGRA layout
        // properties are present and remain alive for the duration of the call.
        let io_surface = unsafe { IOSurfaceRef::new(properties.as_opaque()) }
            .ok_or(MetalFrameError::DestinationTexture)?;
        let texture = self
            .texture_for_surface(
                &io_surface,
                width,
                height,
                MTLPixelFormat::BGRA8Unorm,
                MTLStorageMode::Shared,
            )
            .ok_or(MetalFrameError::DestinationTexture)?;
        Ok(DestinationSurface {
            io_surface,
            texture,
            in_flight_sequence: None,
        })
    }
}

impl DestinationSurfacePool {
    fn new(
        context: &MetalContext,
        generation: u64,
        width: u32,
        height: u32,
    ) -> Result<Self, MetalFrameError> {
        let mut surfaces = Vec::with_capacity(DESTINATION_POOL_SLOT_COUNT);
        for _ in 0..DESTINATION_POOL_SLOT_COUNT {
            surfaces.push(context.create_destination(width, height)?);
        }
        Ok(Self {
            generation,
            width,
            height,
            surfaces,
            next: 0,
        })
    }

    fn matches_size(&self, width: u32, height: u32) -> bool {
        self.width == width && self.height == height
    }
}

fn accept_completed_sequence(last_published_sequence: &mut Option<u64>, sequence: u64) -> bool {
    let accepted = last_published_sequence.is_none_or(|last| {
        let distance = sequence.wrapping_sub(last);
        distance != 0 && distance < (1_u64 << 63)
    });
    if accepted {
        *last_published_sequence = Some(sequence);
    }
    accepted
}

impl MetalFrameProducer {
    pub(super) fn new(viewport: Viewport) -> Self {
        let (context, initialization_error) = match MetalContext::new() {
            Ok(context) => (Some(context), None),
            Err(error) => (None, Some(error.to_string())),
        };
        Self {
            state: Arc::new(Mutex::new(MetalFrameProducerState {
                expected: ExpectedFrameSize::from_viewport(viewport),
                generation: 0,
                context,
                initialization_error,
                destinations: None,
                in_flight_sequences: BTreeSet::new(),
                last_published_sequence: None,
            })),
        }
    }

    pub(super) fn set_viewport(&self, viewport: Viewport) {
        let mut state = self.state.lock();
        let expected = ExpectedFrameSize::from_viewport(viewport);
        if state.expected != expected {
            state.expected = expected;
            state.destinations = None;
        }
    }

    pub(super) fn produce<F>(
        &self,
        info: &AcceleratedPaintInfo,
        sequence: u64,
        completion: F,
    ) -> Result<MetalFrameOutcome, MetalFrameError>
    where
        F: Fn(MetalFrameCompletion) + Send + Sync + 'static,
    {
        let layout = AcceleratedPoolLayout::from_info(info)
            .map_err(|error| MetalFrameError::InvalidMetadata(error.to_string()))?;
        if info.format != ColorType::BGRA_8888 {
            return Err(MetalFrameError::InvalidMetadata(format!(
                "macOS zero-copy requires BGRA_8888, received {}",
                info.format.get_raw()
            )));
        }
        let source_surface = NonNull::new(info.shared_texture_io_surface.cast::<IOSurfaceRef>())
            .ok_or_else(|| {
                MetalFrameError::InvalidMetadata("CEF supplied a null IOSurface".to_owned())
            })?;

        let mut state = self.state.lock();
        if layout.width != state.expected.device_width
            || layout.height != state.expected.device_height
        {
            return Ok(MetalFrameOutcome::Stale(StalePoolFrame::Dimensions {
                expected_width: state.expected.device_width,
                expected_height: state.expected.device_height,
                actual_width: layout.width,
                actual_height: layout.height,
            }));
        }
        if state.in_flight_sequences.len() >= MAX_IN_FLIGHT_BLITS {
            return Ok(MetalFrameOutcome::Stale(StalePoolFrame::InFlightLimit {
                limit: MAX_IN_FLIGHT_BLITS,
            }));
        }
        if state.in_flight_sequences.contains(&sequence) {
            return Err(MetalFrameError::InvalidMetadata(format!(
                "duplicate accelerated frame sequence {sequence}"
            )));
        }
        let Some(context) = state.context.as_ref() else {
            return Err(MetalFrameError::Initialization(
                state
                    .initialization_error
                    .clone()
                    .unwrap_or_else(|| MetalFrameError::DeviceUnavailable.to_string()),
            ));
        };
        let context = MetalContext {
            device: context.device.clone(),
            queue: context.queue.clone(),
        };
        // SAFETY: CEF guarantees this Get-rule IOSurface pointer is valid for
        // the accelerated-paint callback. Retaining it here extends that
        // lifetime through the asynchronous Metal completion block.
        let source_surface = RetainedSourceSurface(unsafe { CFRetained::retain(source_surface) });
        // Apple-silicon GPUs reject managed textures; discrete-GPU Macs need them.
        let source_storage_mode = if context.device.hasUnifiedMemory() {
            MTLStorageMode::Shared
        } else {
            MTLStorageMode::Managed
        };
        let source_texture = context
            .texture_for_surface(
                source_surface.as_ref(),
                layout.width.cast_unsigned(),
                layout.height.cast_unsigned(),
                MTLPixelFormat::BGRA8Unorm,
                source_storage_mode,
            )
            .ok_or(MetalFrameError::SourceTexture)?;

        let width = layout.width.cast_unsigned();
        let height = layout.height.cast_unsigned();
        if state
            .destinations
            .as_ref()
            .is_none_or(|destinations| !destinations.matches_size(width, height))
        {
            state.generation = state.generation.wrapping_add(1).max(1);
            let generation = state.generation;
            state.destinations = Some(DestinationSurfacePool::new(
                &context, generation, width, height,
            )?);
        }
        let destinations = state
            .destinations
            .as_mut()
            .expect("destination IOSurfaces were initialized");
        let destination_index = destinations.next;
        let destination = &mut destinations.surfaces[destination_index];
        if let Some(active_sequence) = destination.in_flight_sequence {
            return Ok(MetalFrameOutcome::Stale(
                StalePoolFrame::DestinationInFlight {
                    slot: destination_index,
                    sequence: active_sequence,
                },
            ));
        }
        let destination_texture = destination.texture.clone();
        let destination_surface = destination.io_surface.clone();
        let pool_generation = destinations.generation;
        let expected = state.expected;

        let command_buffer = context
            .queue
            .commandBuffer()
            .ok_or(MetalFrameError::CommandBuffer)?;
        let encoder = command_buffer
            .blitCommandEncoder()
            .ok_or(MetalFrameError::BlitEncoder)?;
        // SAFETY: both textures are validated packed BGRA textures with the
        // same dimensions, and they remain retained until completion below.
        unsafe {
            encoder.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
                &source_texture,
                0,
                0,
                MTLOrigin { x: 0, y: 0, z: 0 },
                MTLSize {
                    width: width as _,
                    height: height as _,
                    depth: 1,
                },
                &destination_texture,
                0,
                0,
                MTLOrigin { x: 0, y: 0, z: 0 },
            );
        }
        encoder.endEncoding();
        let destinations = state
            .destinations
            .as_mut()
            .expect("destination IOSurfaces remain initialized");
        destinations.surfaces[destination_index].in_flight_sequence = Some(sequence);
        destinations.next = (destination_index + 1) % destinations.surfaces.len();
        state.in_flight_sequences.insert(sequence);
        drop(state);

        let producer_state = Arc::clone(&self.state);
        let completion_block = RcBlock::new(
            move |command_buffer: NonNull<ProtocolObject<dyn MTLCommandBuffer>>| {
                // Keeps CEF's source IOSurface captured until Metal finishes the blit.
                let _ = CFRetained::as_ptr(&source_surface.0);
                // SAFETY: Metal invokes completion handlers with the command
                // buffer that retained and scheduled this block.
                let command_buffer = unsafe { command_buffer.as_ref() };
                let error = command_buffer
                    .error()
                    .map(|error| MetalFrameError::Blit(format!("{error:?}")));
                let outcome = {
                    let mut state = producer_state.lock();
                    if !state.in_flight_sequences.remove(&sequence) {
                        return;
                    }
                    if let Some(destinations) = state
                        .destinations
                        .as_mut()
                        .filter(|destinations| destinations.generation == pool_generation)
                        && let Some(destination) = destinations.surfaces.get_mut(destination_index)
                        && destination.in_flight_sequence == Some(sequence)
                    {
                        destination.in_flight_sequence = None;
                    }

                    if let Some(error) = error {
                        if state
                            .destinations
                            .as_ref()
                            .is_some_and(|destinations| destinations.generation == pool_generation)
                        {
                            state.destinations = None;
                        }
                        MetalFrameCompletion::Failed { sequence, error }
                    } else if state.expected != expected {
                        MetalFrameCompletion::Stale {
                            sequence,
                            reason: StalePoolFrame::Dimensions {
                                expected_width: state.expected.device_width,
                                expected_height: state.expected.device_height,
                                actual_width: layout.width,
                                actual_height: layout.height,
                            },
                        }
                    } else if state
                        .destinations
                        .as_ref()
                        .is_none_or(|destinations| destinations.generation != pool_generation)
                    {
                        MetalFrameCompletion::Stale {
                            sequence,
                            reason: StalePoolFrame::SupersededGeneration { pool_generation },
                        }
                    } else {
                        let last_published_sequence = state.last_published_sequence;
                        if accept_completed_sequence(&mut state.last_published_sequence, sequence) {
                            MetalFrameCompletion::Frame(ProducedMetalFrame {
                                logical_width: expected.logical_width,
                                logical_height: expected.logical_height,
                                device_width: layout.width,
                                device_height: layout.height,
                                pool_generation,
                                sequence,
                                io_surface: MacIoSurface::new(destination_surface.clone()),
                            })
                        } else {
                            MetalFrameCompletion::Stale {
                                sequence,
                                reason: StalePoolFrame::OutOfOrder {
                                    last_published_sequence: last_published_sequence
                                        .unwrap_or(sequence),
                                    sequence,
                                },
                            }
                        }
                    }
                };
                completion(outcome);
            },
        );
        // SAFETY: RcBlock owns a valid heap block for this call. Metal copies
        // and retains it until the command buffer completes.
        unsafe {
            command_buffer.addCompletedHandler(RcBlock::as_ptr(&completion_block));
        }
        command_buffer.commit();
        Ok(MetalFrameOutcome::Submitted)
    }
}

#[cfg(test)]
mod tests {
    use super::accept_completed_sequence;

    #[test]
    fn completion_sequence_guard_rejects_older_and_duplicate_frames() {
        let mut last_published_sequence = None;
        assert!(accept_completed_sequence(&mut last_published_sequence, 4));
        assert_eq!(last_published_sequence, Some(4));
        assert!(!accept_completed_sequence(&mut last_published_sequence, 2));
        assert!(!accept_completed_sequence(&mut last_published_sequence, 4));
        assert_eq!(last_published_sequence, Some(4));
        assert!(accept_completed_sequence(&mut last_published_sequence, 5));
        assert_eq!(last_published_sequence, Some(5));
    }
}
