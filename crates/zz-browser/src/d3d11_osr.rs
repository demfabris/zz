use std::sync::Arc;

use cef::AcceleratedPaintInfo;
use gpui::windows::{
    Win32::{
        Foundation::HANDLE,
        Graphics::{
            Direct3D11::{
                D3D11_BIND_SHADER_RESOURCE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
                ID3D11Device1, ID3D11DeviceContext, ID3D11Texture2D,
            },
            Dxgi::Common::{
                DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM,
                DXGI_SAMPLE_DESC,
            },
        },
    },
    core::Interface as _,
};
use parking_lot::Mutex;
use thiserror::Error;

use crate::{
    BrowserGpuContext, Viewport, WinGpuTexture,
    cef_runtime::{AcceleratedPoolLayout, ExpectedFrameSize, StalePoolFrame},
};

// Five: GPUI's flip-model swap chain can still be sampling four published frames.
const DESTINATION_POOL_SLOT_COUNT: usize = 5;

#[derive(Clone)]
pub(super) struct D3d11FrameProducer {
    state: Arc<Mutex<D3d11FrameProducerState>>,
}

struct D3d11FrameProducerState {
    expected: ExpectedFrameSize,
    generation: u64,
    context: Option<D3d11Context>,
    initialization_error: Option<String>,
    destinations: Option<DestinationTexturePool>,
}

struct D3d11Context {
    device: ID3D11Device1,
    device_context: ID3D11DeviceContext,
}

struct DestinationTexturePool {
    generation: u64,
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
    textures: Vec<ID3D11Texture2D>,
    next: usize,
}

pub(super) struct ProducedD3d11Frame {
    pub logical_width: u32,
    pub logical_height: u32,
    pub device_width: i32,
    pub device_height: i32,
    pub pool_generation: u64,
    pub sequence: u64,
    pub texture: WinGpuTexture,
}

pub(super) enum D3d11FrameOutcome {
    Frame(ProducedD3d11Frame),
    Stale(StalePoolFrame),
}

#[derive(Debug, Error)]
pub(super) enum D3d11FrameError {
    #[error("GPUI's DirectX device context is unavailable")]
    DeviceUnavailable,
    #[error("GPUI's D3D11 device does not implement ID3D11Device1: {0}")]
    DeviceInterface(String),
    #[error("D3D11 initialization failed: {0}")]
    Initialization(String),
    #[error("invalid accelerated-paint metadata: {0}")]
    InvalidMetadata(String),
    #[error("GPUI's D3D11 device was removed: {0}")]
    DeviceRemoved(String),
    #[error("could not open CEF's shared texture handle: {0}")]
    OpenSharedResource(String),
    #[error(
        "CEF's shared texture is {actual_width}x{actual_height} format {actual_format:?}, callback reports {expected_width}x{expected_height}"
    )]
    SourceMismatch {
        expected_width: u32,
        expected_height: u32,
        actual_width: u32,
        actual_height: u32,
        actual_format: DXGI_FORMAT,
    },
    #[error("CEF's shared texture uses unsupported DXGI format {0:?}")]
    UnsupportedFormat(DXGI_FORMAT),
    #[error("could not create a D3D11 destination texture: {0}")]
    DestinationTexture(String),
}

impl D3d11Context {
    fn new(gpu: BrowserGpuContext) -> Result<Self, D3d11FrameError> {
        // Only ID3D11Device1's `OpenSharedResource1` accepts Chromium's NT handles.
        let device = gpu
            .device
            .cast::<ID3D11Device1>()
            .map_err(|error| D3d11FrameError::DeviceInterface(error.to_string()))?;
        Ok(Self {
            device,
            device_context: gpu.device_context,
        })
    }

    #[allow(
        unsafe_code,
        reason = "querying D3D11 device state is an unsafe COM call"
    )]
    fn check_device(&self) -> Result<(), D3d11FrameError> {
        // SAFETY: the device interface is live for the lifetime of this
        // context; the call only reads driver state.
        unsafe { self.device.GetDeviceRemovedReason() }
            .map_err(|error| D3d11FrameError::DeviceRemoved(error.to_string()))
    }

    #[allow(
        unsafe_code,
        reason = "opening a shared D3D11 resource is an unsafe COM call"
    )]
    fn open_shared(
        &self,
        handle: *mut core::ffi::c_void,
    ) -> Result<ID3D11Texture2D, D3d11FrameError> {
        // SAFETY: `handle` is CEF's shared-texture NT handle, non-null and valid
        // for the paint callback; D3D11 returns an owned +1 interface reference.
        unsafe {
            self.device
                .OpenSharedResource1::<ID3D11Texture2D>(HANDLE(handle))
        }
        .map_err(|error| D3d11FrameError::OpenSharedResource(error.to_string()))
    }

    #[allow(
        unsafe_code,
        reason = "reading a D3D11 texture description is an unsafe COM call"
    )]
    fn describe(texture: &ID3D11Texture2D) -> D3D11_TEXTURE2D_DESC {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: `texture` is a live interface and `desc` is a valid,
        // fully-initialized destination D3D11 only writes to.
        unsafe { texture.GetDesc(&raw mut desc) };
        desc
    }

    #[allow(unsafe_code, reason = "creating a D3D11 texture is an unsafe COM call")]
    fn create_destination(
        &self,
        width: u32,
        height: u32,
        format: DXGI_FORMAT,
    ) -> Result<ID3D11Texture2D, D3d11FrameError> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            // `CopyResource` needs no usage flag; GPUI only binds as a shader resource.
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0.cast_unsigned(),
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut texture = None;
        // SAFETY: the descriptor is fully initialized, `None` initial data is
        // valid for a default-usage texture, and `texture` is a valid output
        // slot D3D11 fills with an owned +1 reference on success.
        unsafe {
            self.device
                .CreateTexture2D(&raw const desc, None, Some(&raw mut texture))
        }
        .map_err(|error| D3d11FrameError::DestinationTexture(error.to_string()))?;
        texture.ok_or_else(|| {
            D3d11FrameError::DestinationTexture(
                "CreateTexture2D succeeded without returning a texture".to_owned(),
            )
        })
    }

    #[allow(
        unsafe_code,
        reason = "recording and submitting D3D11 device-context work is unsafe"
    )]
    fn copy_and_flush(&self, destination: &ID3D11Texture2D, source: &ID3D11Texture2D) {
        // SAFETY: both interfaces are live 2D textures of identical dimensions and
        // format, as CopyResource requires, and GPUI's immediate context is only
        // ever touched from the main thread that drives CEF's pump.
        unsafe {
            self.device_context.CopyResource(destination, source);
            self.device_context.Flush();
        }
    }
}

impl DestinationTexturePool {
    fn new(
        context: &D3d11Context,
        generation: u64,
        width: u32,
        height: u32,
        format: DXGI_FORMAT,
    ) -> Result<Self, D3d11FrameError> {
        let mut textures = Vec::with_capacity(DESTINATION_POOL_SLOT_COUNT);
        for _ in 0..DESTINATION_POOL_SLOT_COUNT {
            textures.push(context.create_destination(width, height, format)?);
        }
        Ok(Self {
            generation,
            width,
            height,
            format,
            textures,
            next: 0,
        })
    }

    fn matches(&self, width: u32, height: u32, format: DXGI_FORMAT) -> bool {
        self.width == width && self.height == height && self.format == format
    }
}

fn supported_destination_format(format: DXGI_FORMAT) -> Result<DXGI_FORMAT, D3d11FrameError> {
    if format == DXGI_FORMAT_B8G8R8A8_UNORM || format == DXGI_FORMAT_R8G8B8A8_UNORM {
        Ok(format)
    } else {
        Err(D3d11FrameError::UnsupportedFormat(format))
    }
}

impl D3d11FrameProducer {
    pub(super) fn new(gpu: Option<BrowserGpuContext>, viewport: Viewport) -> Self {
        let (context, initialization_error) = match gpu {
            Some(gpu) => match D3d11Context::new(gpu) {
                Ok(context) => (Some(context), None),
                Err(error) => (None, Some(error.to_string())),
            },
            None => (None, Some(D3d11FrameError::DeviceUnavailable.to_string())),
        };
        Self {
            state: Arc::new(Mutex::new(D3d11FrameProducerState {
                expected: ExpectedFrameSize::from_viewport(viewport),
                generation: 0,
                context,
                initialization_error,
                destinations: None,
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

    pub(super) fn produce(
        &self,
        info: &AcceleratedPaintInfo,
        sequence: u64,
    ) -> Result<D3d11FrameOutcome, D3d11FrameError> {
        let layout = AcceleratedPoolLayout::from_info(info)
            .map_err(|error| D3d11FrameError::InvalidMetadata(error.to_string()))?;
        if info.shared_texture_handle.is_null() {
            return Err(D3d11FrameError::InvalidMetadata(
                "CEF supplied a null shared texture handle".to_owned(),
            ));
        }

        let mut state = self.state.lock();
        if layout.width != state.expected.device_width
            || layout.height != state.expected.device_height
        {
            return Ok(D3d11FrameOutcome::Stale(StalePoolFrame::Dimensions {
                expected_width: state.expected.device_width,
                expected_height: state.expected.device_height,
                actual_width: layout.width,
                actual_height: layout.height,
            }));
        }
        let Some(context) = state.context.as_ref() else {
            return Err(D3d11FrameError::Initialization(
                state
                    .initialization_error
                    .clone()
                    .unwrap_or_else(|| D3d11FrameError::DeviceUnavailable.to_string()),
            ));
        };
        let context = D3d11Context {
            device: context.device.clone(),
            device_context: context.device_context.clone(),
        };

        context.check_device()?;

        let width = layout.width.cast_unsigned();
        let height = layout.height.cast_unsigned();
        let imported = context.open_shared(info.shared_texture_handle)?;
        let source = D3d11Context::describe(&imported);
        if source.Width != width || source.Height != height {
            return Err(D3d11FrameError::SourceMismatch {
                expected_width: width,
                expected_height: height,
                actual_width: source.Width,
                actual_height: source.Height,
                actual_format: source.Format,
            });
        }
        // `CopyResource` requires identical formats, so take the source's.
        let format = supported_destination_format(source.Format)?;

        if state
            .destinations
            .as_ref()
            .is_none_or(|destinations| !destinations.matches(width, height, format))
        {
            state.generation = state.generation.wrapping_add(1).max(1);
            let generation = state.generation;
            state.destinations = Some(DestinationTexturePool::new(
                &context, generation, width, height, format,
            )?);
        }
        let destinations = state
            .destinations
            .as_mut()
            .expect("destination textures were initialized");
        let destination_index = destinations.next;
        let destination = destinations.textures[destination_index].clone();
        let pool_generation = destinations.generation;
        destinations.next = (destination_index + 1) % destinations.textures.len();
        let expected = state.expected;

        context.copy_and_flush(&destination, &imported);
        // CEF returns the resource to its pool when the callback returns.
        drop(imported);

        Ok(D3d11FrameOutcome::Frame(ProducedD3d11Frame {
            logical_width: expected.logical_width,
            logical_height: expected.logical_height,
            device_width: layout.width,
            device_height: layout.height,
            pool_generation,
            sequence,
            texture: destination,
        }))
    }
}
