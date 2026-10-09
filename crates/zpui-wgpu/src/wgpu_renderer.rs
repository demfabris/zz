use crate::wgpu_glass::{GlassResources, glass_count};
use crate::{CompositorGpuHint, DeviceErrorState, WgpuAtlas, WgpuContext};
use anyhow::{Context as _, Result};
use bytemuck::{Pod, Zeroable};
use collections::FxHashMap;
use log::warn;
#[cfg(not(target_family = "wasm"))]
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use smallvec::SmallVec;
use std::cell::RefCell;
use std::num::NonZeroU64;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use zpui::PaintSurface;
use zpui::{
    AtlasTextureId, Background, Bounds, CustomShader, DevicePixels, GpuSpecs, Path, Point,
    PrimitiveBatch, ScaledPixels, Scene, ShaderLayer, Size, get_gamma_correction_ratios,
};

const MAX_INSTANCE_BUFFER_SIZE: u64 = 256 * 1024 * 1024;

const INITIAL_INSTANCE_DATA_SIZE: u64 = 2 * 1024 * 1024;

const INSTANCE_TEXTURE_TEXEL_SIZE: u64 = 16;

/// Shader variant for backends with storage buffer support: the shared shader
/// logic plus the storage-buffer instance transport.
const STORAGE_BUFFER_SHADERS: &str = concat!(
    include_str!("shaders.wgsl"),
    include_str!("shaders_storage.wgsl"),
);

/// Shader variant for WebGL2, which has no storage buffers: the shared shader
/// logic plus the texture-based instance transport.
const WEBGL_SHADERS: &str = concat!(
    include_str!("shaders.wgsl"),
    include_str!("shaders_webgl.wgsl"),
);

/// Subpixel text rendering requires dual-source blending, which WebGL2 lacks, so
/// this variant only ever runs with the storage-buffer transport. The `enable`
/// directive must precede all declarations.
const SUBPIXEL_SHADERS: &str = concat!(
    "enable dual_source_blending;\n",
    include_str!("shaders.wgsl"),
    include_str!("shaders_storage.wgsl"),
    include_str!("shaders_subpixel.wgsl"),
);

fn instance_part<T>(instances: &[T]) -> (&[u8], u64) {
    let bytes = unsafe {
        std::slice::from_raw_parts(
            instances.as_ptr() as *const u8,
            std::mem::size_of_val(instances),
        )
    };
    (bytes, (std::mem::size_of::<T>() as u64).max(1))
}

fn least_common_multiple(left: u64, right: u64) -> u64 {
    let mut first = left;
    let mut second = right;
    while second != 0 {
        let remainder = first % second;
        first = second;
        second = remainder;
    }
    left / first * right
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GlobalParams {
    viewport_size: [f32; 2],
    premultiplied_alpha: u32,
    // Scene-wide rounded clip (Scene::window_corner_mask); zero size disables
    // it. The smoothing rides in what was alignment padding.
    window_mask_smoothing: f32,
    window_mask_origin: [f32; 2],
    window_mask_size: [f32; 2],
    window_mask_radii: [f32; 4],
    clip_window_shadows: u32,
    _window_mask_pad: [u32; 3],
}

#[repr(C)]
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PodBounds {
    origin: [f32; 2],
    size: [f32; 2],
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
impl From<Bounds<ScaledPixels>> for PodBounds {
    fn from(bounds: Bounds<ScaledPixels>) -> Self {
        Self {
            origin: [bounds.origin.x.0, bounds.origin.y.0],
            size: [bounds.size.width.0, bounds.size.height.0],
        }
    }
}

#[repr(C)]
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SurfaceParams {
    bounds: PodBounds,
    content_mask: PodBounds,
    // WGSL field order: top_left, top_right, bottom_right, bottom_left.
    corner_radii: [f32; 4],
    corner_smoothing: f32,
    // The WGSL struct rounds up to its 8-byte alignment.
    pad: f32,
}

/// The vertex half of every shader layer pipeline; the fragment half is the
/// layer's [`CustomShader`].
const SHADER_LAYER_VERTEX: &str = r#"
struct ShaderLayerParams {
    bounds: vec4<f32>,
    viewport_size: vec2<f32>,
    pad: vec2<f32>,
}

@group(1) @binding(0) var<uniform> shader_layer: ShaderLayerParams;

struct ShaderLayerVarying {
    @builtin(position) position: vec4<f32>,
    @location(0) layer_position: vec2<f32>,
}

@vertex
fn shader_layer_vertex(@builtin(vertex_index) vertex_id: u32) -> ShaderLayerVarying {
    let unit = vec2<f32>(f32(vertex_id & 1u), 0.5 * f32(vertex_id & 2u));
    let size = shader_layer.bounds.zw;
    let position = shader_layer.bounds.xy + unit * size;
    let device = position / shader_layer.viewport_size * vec2<f32>(2.0, -2.0)
        + vec2<f32>(-1.0, 1.0);
    var out: ShaderLayerVarying;
    out.position = vec4<f32>(device, 0.0, 1.0);
    out.layer_position = unit * size;
    return out;
}
"#;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ShaderLayerParams {
    bounds: [f32; 4],
    viewport_size: [f32; 2],
    pad: [f32; 2],
}

const MAX_SHADER_LAYER_INPUTS: usize = 8;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GammaParams {
    gamma_ratios: [f32; 4],
    grayscale_enhanced_contrast: f32,
    subpixel_enhanced_contrast: f32,
    is_bgr: u32,
    _pad: u32,
}

#[derive(Clone, Debug)]
#[repr(C)]
struct PathSprite {
    bounds: Bounds<ScaledPixels>,
}

#[derive(Clone, Debug)]
#[repr(C)]
struct PathRasterizationVertex {
    xy_position: Point<ScaledPixels>,
    st_position: Point<f32>,
    color: Background,
    bounds: Bounds<ScaledPixels>,
}

pub struct WgpuSurfaceConfig {
    pub size: Size<DevicePixels>,
    pub transparent: bool,
    /// Preferred presentation mode. When `Some`, the renderer will use this
    /// mode if supported by the surface, falling back to `Fifo`.
    /// When `None`, defaults to `Fifo` (VSync).
    ///
    /// Mobile platforms may prefer `Mailbox` (triple-buffering) to avoid
    /// blocking in `get_current_texture()` during lifecycle transitions.
    pub preferred_present_mode: Option<wgpu::PresentMode>,
}

struct WgpuPipelines {
    quads: wgpu::RenderPipeline,
    shadows: wgpu::RenderPipeline,
    path_rasterization: wgpu::RenderPipeline,
    paths: wgpu::RenderPipeline,
    underlines: wgpu::RenderPipeline,
    mono_sprites: wgpu::RenderPipeline,
    subpixel_sprites: Option<wgpu::RenderPipeline>,
    poly_sprites: wgpu::RenderPipeline,
    #[allow(dead_code)]
    surfaces: wgpu::RenderPipeline,
}

/// One frame allocation of instance data, ready to bind.
struct InstanceBinding {
    bind_group: wgpu::BindGroup,
    /// Index of the allocation's first instance within the bound data. The
    /// bind group covers the whole instance buffer or texture and the shader
    /// indexes it absolutely, so draws offset their instance (or vertex)
    /// ranges by this value.
    first_instance: u32,
}

struct InstanceBindings {
    quads: InstanceBinding,
    shadows: InstanceBinding,
    underlines: InstanceBinding,
    monochrome_sprites: InstanceBinding,
    subpixel_sprites: InstanceBinding,
    polychrome_sprites: InstanceBinding,
}

struct WgpuBindGroupLayouts {
    globals: wgpu::BindGroupLayout,
    instances: wgpu::BindGroupLayout,
    texture: wgpu::BindGroupLayout,
}

/// Shared GPU context reference, used to coordinate device recovery across multiple windows.
pub type GpuContext = Rc<RefCell<Option<WgpuContext>>>;

enum InstanceData {
    Storage(wgpu::Buffer),
    // WebGL2 has no storage buffers. A uint texture keeps the records available to both shader
    // stages while preserving integer and floating-point bit patterns exactly.
    Texture {
        texture: wgpu::Texture,
        view: wgpu::TextureView,
        width: u32,
        height: u32,
    },
}

/// GPU resources that must be dropped together during device recovery.
struct WgpuResources {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    pipelines: WgpuPipelines,
    bind_group_layouts: WgpuBindGroupLayouts,
    atlas_sampler: wgpu::Sampler,
    atlas_texture_bind_groups: FxHashMap<AtlasTextureId, CachedTextureBindGroup>,
    globals_buffer: wgpu::Buffer,
    globals_bind_group: wgpu::BindGroup,
    path_globals_bind_group: wgpu::BindGroup,
    instance_data: InstanceData,
    instance_bind_group: wgpu::BindGroup,
    staging_belt: wgpu::util::StagingBelt,
    /// Copies this frame's instance data out of the staging belt. Submitted
    /// ahead of the frame's draws.
    upload_encoder: Option<wgpu::CommandEncoder>,
    path_intermediate_texture: Option<wgpu::Texture>,
    path_intermediate_view: Option<wgpu::TextureView>,
    path_msaa_texture: Option<wgpu::Texture>,
    path_msaa_view: Option<wgpu::TextureView>,
    shader_layers: ShaderLayerResources,
    /// Created when a scene first paints glass.
    glass: Option<GlassResources>,
}

struct CachedTextureBindGroup {
    texture_generation: u64,
    bind_group: wgpu::BindGroup,
}

struct ShaderLayerResources {
    content_layout: wgpu::BindGroupLayout,
    params_layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    vertex_module: wgpu::ShaderModule,
    sampler: wgpu::Sampler,
    pipelines: FxHashMap<u64, Option<ShaderLayerPipeline>>,
    targets: Vec<RenderTexture>,
    inputs: Vec<RenderTexture>,
    drawn: bool,
}

#[derive(Clone)]
struct ShaderLayerPipeline {
    pipeline: wgpu::RenderPipeline,
    uniform_size: u64,
}

struct RenderTexture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl RenderTexture {
    fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        size: Size<DevicePixels>,
        usage: wgpu::TextureUsages,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shader_layer_texture"),
            size: wgpu::Extent3d {
                width: size.width.0 as u32,
                height: size.height.0 as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self { texture, view }
    }

    fn has_size(&self, size: Size<DevicePixels>) -> bool {
        self.texture.width() == size.width.0 as u32 && self.texture.height() == size.height.0 as u32
    }
}

impl ShaderLayerResources {
    fn new(device: &wgpu::Device) -> Self {
        let content_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shader_layer_content_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let params_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shader_layer_params_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(
                        std::mem::size_of::<ShaderLayerParams>() as u64
                    ),
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shader_layer_pipeline_layout"),
            bind_group_layouts: &[Some(&content_layout), Some(&params_layout)],
            immediate_size: 0,
        });
        let vertex_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shader_layer_vertex"),
            source: wgpu::ShaderSource::Wgsl(SHADER_LAYER_VERTEX.into()),
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shader_layer_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            content_layout,
            params_layout,
            pipeline_layout,
            vertex_module,
            sampler,
            pipelines: FxHashMap::default(),
            targets: Vec::new(),
            inputs: Vec::new(),
            drawn: false,
        }
    }

    fn release_textures(&mut self) {
        self.targets.clear();
        self.inputs.clear();
    }
}

impl WgpuResources {
    fn invalidate_intermediate_textures(&mut self) {
        self.path_intermediate_texture = None;
        self.path_intermediate_view = None;
        self.path_msaa_texture = None;
        self.path_msaa_view = None;
        self.shader_layers.release_textures();
        if let Some(glass) = &mut self.glass {
            glass.release_textures();
        }
    }
}

struct WgpuRendererCore {
    resources: WgpuResources,
    uploaded_globals: Option<(GlobalParams, GlobalParams, GammaParams)>,
    atlas: Arc<WgpuAtlas>,
    path_globals_offset: u64,
    gamma_offset: u64,
    instance_data_capacity: u64,
    max_instance_data_size: u64,
    instance_data_alignment: u64,
    uses_webgl_instance_data: bool,
    rendering_params: RenderingParameters,
    is_bgr: bool,
    dual_source_blending: bool,
    adapter_info: wgpu::AdapterInfo,
    target_format: wgpu::TextureFormat,
    max_texture_size: u32,
    clip_window_shadows: bool,
}

/// GPU resources of a windowed renderer. A surface is only ever configured against the
/// device that owns `core`, so it cannot outlive it: there is no surface-only state.
enum RendererState {
    /// Frames can be drawn.
    Ready {
        surface: wgpu::Surface<'static>,
        core: WgpuRendererCore,
    },
    /// The native surface is gone (Android `TerminateWindow`, browser context loss) but
    /// the device, pipelines, and atlas remain so `replace_surface` can resume without
    /// re-uploading cached textures.
    Unconfigured { core: WgpuRendererCore },
    /// Released by `destroy`, or dropped for a device recovery that has not succeeded yet.
    Released,
}

pub struct WgpuRenderer {
    /// Shared GPU context for device recovery coordination (unused on WASM).
    #[allow(dead_code)]
    context: Option<GpuContext>,
    /// Compositor GPU hint for adapter selection (unused on WASM).
    #[allow(dead_code)]
    compositor_gpu: Option<CompositorGpuHint>,
    state: RendererState,
    surface_config: wgpu::SurfaceConfiguration,
    atlas: Arc<WgpuAtlas>,
    transparent_alpha_mode: wgpu::CompositeAlphaMode,
    opaque_alpha_mode: wgpu::CompositeAlphaMode,
    max_texture_size: u32,
    is_bgr: bool,
    failed_frame_count: u32,
    device_errors: Arc<DeviceErrorState>,
    observed_error_generation: u64,
    last_surface_error: Option<String>,
    needs_redraw: bool,
}

impl WgpuRenderer {
    fn core(&self) -> Option<&WgpuRendererCore> {
        match &self.state {
            RendererState::Ready { core, .. } | RendererState::Unconfigured { core } => Some(core),
            RendererState::Released => None,
        }
    }

    fn core_mut(&mut self) -> Option<&mut WgpuRendererCore> {
        match &mut self.state {
            RendererState::Ready { core, .. } | RendererState::Unconfigured { core } => Some(core),
            RendererState::Released => None,
        }
    }

    /// Creates a new WgpuRenderer from raw window handles.
    ///
    /// The `gpu_context` is a shared reference that coordinates GPU context across
    /// multiple windows. The first window to create a renderer will initialize the
    /// context; subsequent windows will share it.
    ///
    /// # Safety
    /// The caller must ensure that the window handle remains valid for the lifetime
    /// of the returned renderer.
    #[cfg(not(target_family = "wasm"))]
    pub fn new<W>(
        gpu_context: GpuContext,
        window: &W,
        config: WgpuSurfaceConfig,
        compositor_gpu: Option<CompositorGpuHint>,
    ) -> anyhow::Result<Self>
    where
        W: HasWindowHandle + HasDisplayHandle + std::fmt::Debug + Send + Sync + Clone + 'static,
    {
        let window_handle = window
            .window_handle()
            .map_err(|e| anyhow::anyhow!("Failed to get window handle: {e}"))?;

        let mut ctx_ref = gpu_context.borrow_mut();
        let (context, surface) = match ctx_ref.as_mut() {
            Some(context) => {
                let surface = create_surface(&context.instance, window_handle.as_raw())?;
                context.check_compatible_with_surface(&surface)?;
                (context, surface)
            }
            None => {
                let (context, surface) = create_context_and_surface(
                    window,
                    window_handle.as_raw(),
                    compositor_gpu,
                    false,
                )?;
                (ctx_ref.insert(context), surface)
            }
        };

        let atlas = Arc::new(WgpuAtlas::from_context(context));

        Self::new_internal(
            Some(Rc::clone(&gpu_context)),
            context,
            surface,
            config,
            compositor_gpu,
            atlas,
        )
    }

    #[cfg(target_family = "wasm")]
    pub fn new_from_canvas(
        context: &WgpuContext,
        canvas: &web_sys::HtmlCanvasElement,
        config: WgpuSurfaceConfig,
    ) -> anyhow::Result<Self> {
        let surface = context
            .instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
            .map_err(|e| anyhow::anyhow!("Failed to create surface: {e}"))?;
        Self::new_from_surface(context, surface, config)
    }

    #[cfg(target_family = "wasm")]
    #[allow(clippy::arc_with_non_send_sync)]
    pub fn new_from_surface(
        context: &WgpuContext,
        surface: wgpu::Surface<'static>,
        config: WgpuSurfaceConfig,
    ) -> anyhow::Result<Self> {
        let atlas = Arc::new(WgpuAtlas::from_context(context));
        Self::new_internal(None, context, surface, config, None, atlas)
    }

    fn new_internal(
        gpu_context: Option<GpuContext>,
        context: &WgpuContext,
        surface: wgpu::Surface<'static>,
        config: WgpuSurfaceConfig,
        compositor_gpu: Option<CompositorGpuHint>,
        atlas: Arc<WgpuAtlas>,
    ) -> anyhow::Result<Self> {
        let surface_caps = surface.get_capabilities(&context.adapter);
        let preferred_formats = [
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureFormat::Rgba8Unorm,
        ];
        let surface_format = preferred_formats
            .iter()
            .find(|f| surface_caps.formats.contains(f))
            .copied()
            .or_else(|| surface_caps.formats.iter().find(|f| !f.is_srgb()).copied())
            .or_else(|| surface_caps.formats.first().copied())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Surface reports no supported texture formats for adapter {:?}",
                    context.adapter.get_info().name
                )
            })?;

        let pick_alpha_mode =
            |preferences: &[wgpu::CompositeAlphaMode]| -> anyhow::Result<wgpu::CompositeAlphaMode> {
                preferences
                    .iter()
                    .find(|p| surface_caps.alpha_modes.contains(p))
                    .copied()
                    .or_else(|| surface_caps.alpha_modes.first().copied())
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "Surface reports no supported alpha modes for adapter {:?}",
                            context.adapter.get_info().name
                        )
                    })
            };

        let transparent_alpha_mode = pick_alpha_mode(&[
            wgpu::CompositeAlphaMode::PreMultiplied,
            wgpu::CompositeAlphaMode::Inherit,
        ])?;

        let opaque_alpha_mode = pick_alpha_mode(&[
            wgpu::CompositeAlphaMode::Opaque,
            wgpu::CompositeAlphaMode::Inherit,
        ])?;

        let alpha_mode = if config.transparent {
            transparent_alpha_mode
        } else {
            opaque_alpha_mode
        };

        let device = Arc::clone(&context.device);
        let max_texture_size = device.limits().max_texture_dimension_2d;

        let requested_width = config.size.width.0 as u32;
        let requested_height = config.size.height.0 as u32;
        let clamped_width = requested_width.min(max_texture_size);
        let clamped_height = requested_height.min(max_texture_size);

        if clamped_width != requested_width || clamped_height != requested_height {
            warn!(
                "Requested surface size ({}, {}) exceeds maximum texture dimension {}. \
                 Clamping to ({}, {}). Window content may not fill the entire window.",
                requested_width, requested_height, max_texture_size, clamped_width, clamped_height
            );
        }

        // Glass reads back what the frame holds so far by copying out of it.
        // wgpu reports only RENDER_ATTACHMENT for a browser canvas, but WebGPU
        // canvases accept any usage.
        let copyable = surface_caps.usages.contains(wgpu::TextureUsages::COPY_SRC)
            || context.adapter.get_info().backend == wgpu::Backend::BrowserWebGpu;
        let usage = if copyable {
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC
        } else {
            wgpu::TextureUsages::RENDER_ATTACHMENT
        };
        let surface_config = wgpu::SurfaceConfiguration {
            usage,
            format: surface_format,
            width: clamped_width.max(1),
            height: clamped_height.max(1),
            present_mode: config
                .preferred_present_mode
                .filter(|mode| surface_caps.present_modes.contains(mode))
                .unwrap_or(wgpu::PresentMode::Fifo),
            desired_maximum_frame_latency: 2,
            alpha_mode,
            color_space: wgpu::SurfaceColorSpace::Auto,
            view_formats: vec![],
        };
        // Configure the surface immediately. The adapter selection process already validated
        // that this adapter can successfully configure this surface.
        surface.configure(&context.device, &surface_config);

        let core = WgpuRendererCore::new(context, atlas.clone(), surface_format, alpha_mode);

        Ok(Self {
            context: gpu_context,
            compositor_gpu,
            state: RendererState::Ready { surface, core },
            surface_config,
            atlas,
            transparent_alpha_mode,
            opaque_alpha_mode,
            max_texture_size,
            is_bgr: false,
            failed_frame_count: 0,
            device_errors: Arc::clone(context.errors()),
            observed_error_generation: 0,
            last_surface_error: None,
            needs_redraw: false,
        })
    }
}

impl WgpuRendererCore {
    fn create_bind_group_layouts(
        device: &wgpu::Device,
        uses_webgl_instance_data: bool,
    ) -> WgpuBindGroupLayouts {
        let globals =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("globals_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(
                                std::mem::size_of::<GlobalParams>() as u64
                            ),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(
                                std::mem::size_of::<GammaParams>() as u64
                            ),
                        },
                        count: None,
                    },
                ],
            });

        let instance_data_entry = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: if uses_webgl_instance_data {
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Uint,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                }
            } else {
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                }
            },
            count: None,
        };

        let instances = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("instances_layout"),
            entries: &[instance_data_entry],
        });

        let texture = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        WgpuBindGroupLayouts {
            globals,
            instances,
            texture,
        }
    }

    fn create_instance_texture(
        device: &wgpu::Device,
        requested_capacity: u64,
        max_texture_dimension: u32,
    ) -> (InstanceData, u64) {
        let texel_count = requested_capacity.div_ceil(INSTANCE_TEXTURE_TEXEL_SIZE);
        let width = texel_count.min(u64::from(max_texture_dimension)).max(1) as u32;
        let height = texel_count
            .div_ceil(u64::from(width))
            .min(u64::from(max_texture_dimension))
            .max(1) as u32;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("instance_texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let capacity = u64::from(width) * u64::from(height) * INSTANCE_TEXTURE_TEXEL_SIZE;
        (
            InstanceData::Texture {
                texture,
                view,
                width,
                height,
            },
            capacity,
        )
    }

    fn create_pipelines(
        device: &wgpu::Device,
        layouts: &WgpuBindGroupLayouts,
        surface_format: wgpu::TextureFormat,
        alpha_mode: wgpu::CompositeAlphaMode,
        path_sample_count: u32,
        dual_source_blending: bool,
        uses_webgl_instance_data: bool,
    ) -> WgpuPipelines {
        // Diagnostic guard: verify the device actually has
        // DUAL_SOURCE_BLENDING. We have a crash report (ZED-5G1) where a
        // feature mismatch caused a wgpu-hal abort, but we haven't
        // identified the code path that produces the mismatch. This
        // guard prevents the crash and logs more evidence.
        // Remove this check once:
        // a) We find and fix the root cause, or
        // b) There are no reports of this warning appearing for some time.
        let device_has_feature = device
            .features()
            .contains(wgpu::Features::DUAL_SOURCE_BLENDING);
        if dual_source_blending && !device_has_feature {
            log::error!(
                "BUG: dual_source_blending flag is true but device does not \
                 have DUAL_SOURCE_BLENDING enabled (device features: {:?}). \
                 Falling back to mono text rendering. Please report this at \
                 https://github.com/zed-industries/zed/issues",
                device.features(),
            );
        }
        let dual_source_blending =
            dual_source_blending && device_has_feature && !uses_webgl_instance_data;

        let shader_source = if uses_webgl_instance_data {
            WEBGL_SHADERS
        } else {
            STORAGE_BUFFER_SHADERS
        };
        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gpui_shaders"),
            source: wgpu::ShaderSource::Wgsl(shader_source.into()),
        });

        let subpixel_shader_module = if dual_source_blending {
            Some(device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("gpui_subpixel_shaders"),
                source: wgpu::ShaderSource::Wgsl(SUBPIXEL_SHADERS.into()),
            }))
        } else {
            None
        };

        let blend_mode = match alpha_mode {
            wgpu::CompositeAlphaMode::PreMultiplied => {
                wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING
            }
            _ => wgpu::BlendState::ALPHA_BLENDING,
        };

        let color_target = wgpu::ColorTargetState {
            format: surface_format,
            blend: Some(blend_mode),
            write_mask: wgpu::ColorWrites::ALL,
        };

        let create_pipeline = |name: &str,
                               vs_entry: &str,
                               fs_entry: &str,
                               globals_layout: &wgpu::BindGroupLayout,
                               data_layout: &wgpu::BindGroupLayout,
                               texture_layout: Option<&wgpu::BindGroupLayout>,
                               topology: wgpu::PrimitiveTopology,
                               color_targets: &[Option<wgpu::ColorTargetState>],
                               sample_count: u32,
                               module: &wgpu::ShaderModule| {
            let mut bind_group_layouts = vec![Some(globals_layout), Some(data_layout)];
            bind_group_layouts.extend(texture_layout.map(Some));
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(&format!("{name}_layout")),
                bind_group_layouts: &bind_group_layouts,
                immediate_size: 0,
            });

            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(name),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some(vs_entry),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some(fs_entry),
                    targets: color_targets,
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState {
                    count: sample_count,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                multiview_mask: None,
                cache: None,
            })
        };

        let quads = create_pipeline(
            "quads",
            "vs_quad",
            "fs_quad",
            &layouts.globals,
            &layouts.instances,
            None,
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(color_target.clone())],
            1,
            &shader_module,
        );

        let shadows = create_pipeline(
            "shadows",
            "vs_shadow",
            "fs_shadow",
            &layouts.globals,
            &layouts.instances,
            None,
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(color_target.clone())],
            1,
            &shader_module,
        );

        let path_rasterization = create_pipeline(
            "path_rasterization",
            "vs_path_rasterization",
            "fs_path_rasterization",
            &layouts.globals,
            &layouts.instances,
            None,
            wgpu::PrimitiveTopology::TriangleList,
            &[Some(wgpu::ColorTargetState {
                format: surface_format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            path_sample_count,
            &shader_module,
        );

        let paths_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        };

        let paths = create_pipeline(
            "paths",
            "vs_path",
            "fs_path",
            &layouts.globals,
            &layouts.instances,
            Some(&layouts.texture),
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(wgpu::ColorTargetState {
                format: surface_format,
                blend: Some(paths_blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            1,
            &shader_module,
        );

        let underlines = create_pipeline(
            "underlines",
            "vs_underline",
            "fs_underline",
            &layouts.globals,
            &layouts.instances,
            None,
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(color_target.clone())],
            1,
            &shader_module,
        );

        let mono_sprites = create_pipeline(
            "mono_sprites",
            "vs_mono_sprite",
            "fs_mono_sprite",
            &layouts.globals,
            &layouts.instances,
            Some(&layouts.texture),
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(color_target.clone())],
            1,
            &shader_module,
        );

        let subpixel_sprites = if let Some(subpixel_module) = &subpixel_shader_module {
            let subpixel_blend = wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::Src1,
                    dst_factor: wgpu::BlendFactor::OneMinusSrc1,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
            };

            Some(create_pipeline(
                "subpixel_sprites",
                "vs_subpixel_sprite",
                "fs_subpixel_sprite",
                &layouts.globals,
                &layouts.instances,
                Some(&layouts.texture),
                wgpu::PrimitiveTopology::TriangleStrip,
                &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(subpixel_blend),
                    write_mask: wgpu::ColorWrites::COLOR,
                })],
                1,
                subpixel_module,
            ))
        } else {
            None
        };

        let poly_sprites = create_pipeline(
            "poly_sprites",
            "vs_poly_sprite",
            "fs_poly_sprite",
            &layouts.globals,
            &layouts.instances,
            Some(&layouts.texture),
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(color_target.clone())],
            1,
            &shader_module,
        );

        let surfaces = create_pipeline(
            "surfaces",
            "vs_surface",
            "fs_surface",
            &layouts.globals,
            &layouts.instances,
            Some(&layouts.texture),
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(color_target)],
            1,
            &shader_module,
        );

        WgpuPipelines {
            quads,
            shadows,
            path_rasterization,
            paths,
            underlines,
            mono_sprites,
            subpixel_sprites,
            poly_sprites,
            surfaces,
        }
    }

    fn create_path_intermediate(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("path_intermediate"),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        (texture, view)
    }

    fn create_msaa_if_needed(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        sample_count: u32,
    ) -> Option<(wgpu::Texture, wgpu::TextureView)> {
        if sample_count <= 1 {
            return None;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("path_msaa"),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Some((texture, view))
    }
}

impl WgpuRenderer {
    /// Records the new drawable size and reconfigures the surface when one is present.
    /// The size is kept even without GPU resources so that recovery restores it.
    pub fn update_drawable_size(&mut self, size: Size<DevicePixels>) {
        let width = size.width.0 as u32;
        let height = size.height.0 as u32;

        if width == self.surface_config.width && height == self.surface_config.height {
            return;
        }

        let clamped_width = width.min(self.max_texture_size);
        let clamped_height = height.min(self.max_texture_size);
        if clamped_width != width || clamped_height != height {
            warn!(
                "Requested surface size ({}, {}) exceeds maximum texture dimension {}. \
                 Clamping to ({}, {}). Window content may not fill the entire window.",
                width, height, self.max_texture_size, clamped_width, clamped_height
            );
        }
        self.surface_config.width = clamped_width.max(1);
        self.surface_config.height = clamped_height.max(1);

        let Some(core) = self.core_mut() else {
            return;
        };
        let resources = &mut core.resources;

        // Wait for any in-flight GPU work to complete before destroying textures
        if let Err(e) = resources.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        }) {
            warn!("Failed to poll device during resize: {e:?}");
        }

        // Destroy old textures before allocating new ones to avoid GPU memory spikes
        if let Some(ref texture) = resources.path_intermediate_texture {
            texture.destroy();
        }
        if let Some(ref texture) = resources.path_msaa_texture {
            texture.destroy();
        }

        // Invalidate intermediate textures - the next frame that draws paths
        // recreates them at the new size, so windows without paths never
        // allocate them.
        resources.invalidate_intermediate_textures();

        if let RendererState::Ready { surface, core } = &self.state {
            surface.configure(&core.resources.device, &self.surface_config);
        }
    }

    pub fn set_subpixel_layout(&mut self, is_bgr: bool) {
        self.is_bgr = is_bgr;
        if let Some(core) = self.core_mut() {
            core.is_bgr = is_bgr;
        }
    }

    pub fn update_transparency(&mut self, transparent: bool) {
        let new_alpha_mode = if transparent {
            self.transparent_alpha_mode
        } else {
            self.opaque_alpha_mode
        };
        if new_alpha_mode == self.surface_config.alpha_mode {
            return;
        }
        self.surface_config.alpha_mode = new_alpha_mode;
        let format = self.surface_config.format;

        let Some(core) = self.core_mut() else {
            return;
        };
        let resources = &mut core.resources;
        resources.pipelines = WgpuRendererCore::create_pipelines(
            &resources.device,
            &resources.bind_group_layouts,
            format,
            new_alpha_mode,
            core.rendering_params.path_sample_count,
            core.dual_source_blending,
            core.uses_webgl_instance_data,
        );

        if let RendererState::Ready { surface, core } = &self.state {
            surface.configure(&core.resources.device, &self.surface_config);
        }
    }

    #[allow(dead_code)]
    pub fn viewport_size(&self) -> Size<DevicePixels> {
        Size {
            width: DevicePixels(self.surface_config.width as i32),
            height: DevicePixels(self.surface_config.height as i32),
        }
    }

    pub fn sprite_atlas(&self) -> &Arc<WgpuAtlas> {
        &self.atlas
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    pub fn device_context(&self) -> Option<zpui::WgpuDeviceContext> {
        let resources = &self.core()?.resources;
        Some(zpui::WgpuDeviceContext {
            device: resources.device.clone(),
            queue: resources.queue.clone(),
        })
    }

    /// Whether frames can be read back mid-frame, which glass needs.
    pub fn supports_backdrop_sampling(&self) -> bool {
        self.surface_config
            .usage
            .contains(wgpu::TextureUsages::COPY_SRC)
    }

    pub fn supports_dual_source_blending(&self) -> bool {
        self.core().is_some_and(|core| core.dual_source_blending)
    }

    /// Clips drop shadows to the scene-wide window mask. Wayland compositors
    /// cannot blur a transparent CSD shadow margin without also exposing the
    /// effect in its corner wedges, so an ext-background-effect client can use
    /// this to present a clean shadowless rounded window instead.
    pub fn set_clip_window_shadows(&mut self, clip: bool) {
        if let Some(core) = self.core_mut() {
            core.clip_window_shadows = clip;
        }
    }

    /// Returns `None` once GPU resources have been released by `destroy` or a pending
    /// device recovery.
    pub fn gpu_specs(&self) -> Option<GpuSpecs> {
        let adapter_info = &self.core()?.adapter_info;
        Some(GpuSpecs {
            is_software_emulated: adapter_info.device_type == wgpu::DeviceType::Cpu,
            device_name: adapter_info.name.clone(),
            driver_name: adapter_info.driver.clone(),
            driver_info: adapter_info.driver_info.clone(),
        })
    }

    pub fn max_texture_size(&self) -> u32 {
        self.max_texture_size
    }

    pub fn draw(&mut self, scene: &Scene) -> bool {
        #[cfg(target_family = "wasm")]
        if self.device_lost() {
            if matches!(self.state, RendererState::Ready { .. }) {
                log::error!(
                    "Browser graphics context was lost; rendering has stopped. Reload the page to recover."
                );
                self.unconfigure_surface();
            }
            return false;
        }

        // Bail out early if the surface has been unconfigured (e.g. during
        // Android background/rotation transitions).  Attempting to acquire
        // a texture from an unconfigured surface can block indefinitely on
        // some drivers (Adreno).
        let RendererState::Ready { surface, core } = &mut self.state else {
            return false;
        };

        if let Some(error) = self.last_surface_error.take().or_else(|| {
            self.device_errors
                .observe_error(&mut self.observed_error_generation)
        }) {
            self.failed_frame_count += 1;
            log::error!(
                "GPU error during frame (failure {} of 10): {error}",
                self.failed_frame_count
            );

            // TBD. Does retrying more actually help?
            if self.failed_frame_count > 10 {
                panic!("Too many consecutive GPU errors. Last error: {error}");
            } else if self.failed_frame_count > 5 {
                core.resources.invalidate_intermediate_textures();
                self.atlas.clear();
                self.needs_redraw = true;
                self.failed_frame_count = 0;
                return false;
            }
        } else {
            self.failed_frame_count = 0;
        }

        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                // Textures must be destroyed before the surface can be reconfigured.
                drop(frame);
                surface.configure(&core.resources.device, &self.surface_config);
                return false;
            }
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                surface.configure(&core.resources.device, &self.surface_config);
                return false;
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return false;
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                self.last_surface_error = Some("Surface texture validation error".to_string());
                return false;
            }
        };

        // The acquired texture is the authority on frame dimensions; the surface
        // configuration is only a request.
        let size = Size {
            width: DevicePixels(frame.texture.width() as i32),
            height: DevicePixels(frame.texture.height() as i32),
        };
        let frame_view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let premultiplied_alpha =
            self.surface_config.alpha_mode == wgpu::CompositeAlphaMode::PreMultiplied;
        if let Err(error) = core.render_frame(
            scene,
            &frame.texture,
            &frame_view,
            size,
            premultiplied_alpha,
            wgpu::Color::TRANSPARENT,
        ) {
            log::error!("{error:#}");
            // Dropping an acquired image does not release native Vulkan
            // swapchain images. Reconfigure after all texture views are gone.
            drop(frame_view);
            drop(frame);
            surface.configure(&core.resources.device, &self.surface_config);
            return false;
        }

        core.resources.queue.present(frame);
        true
    }
}

impl WgpuRendererCore {
    fn new(
        context: &WgpuContext,
        atlas: Arc<WgpuAtlas>,
        target_format: wgpu::TextureFormat,
        alpha_mode: wgpu::CompositeAlphaMode,
    ) -> Self {
        let device = Arc::clone(&context.device);
        let queue = Arc::clone(&context.queue);
        let rendering_params = RenderingParameters::new(&context.adapter, target_format);
        let uses_webgl_instance_data = context.uses_webgl_instance_data();
        let dual_source_blending =
            context.supports_dual_source_blending() && !uses_webgl_instance_data;
        let bind_group_layouts = Self::create_bind_group_layouts(&device, uses_webgl_instance_data);
        let pipelines = Self::create_pipelines(
            &device,
            &bind_group_layouts,
            target_format,
            alpha_mode,
            rendering_params.path_sample_count,
            dual_source_blending,
            uses_webgl_instance_data,
        );
        let atlas_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atlas_sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform_alignment = device.limits().min_uniform_buffer_offset_alignment as u64;
        let globals_size = std::mem::size_of::<GlobalParams>() as u64;
        let gamma_size = std::mem::size_of::<GammaParams>() as u64;
        let path_globals_offset = globals_size.next_multiple_of(uniform_alignment);
        let gamma_offset = (path_globals_offset + globals_size).next_multiple_of(uniform_alignment);
        let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals_buffer"),
            size: gamma_offset + gamma_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let (
            instance_data,
            instance_data_capacity,
            max_instance_data_size,
            instance_data_alignment,
        ) = if uses_webgl_instance_data {
            let max_texture_dimension = device.limits().max_texture_dimension_2d;
            let max_instance_data_size = (u64::from(max_texture_dimension).pow(2)
                * INSTANCE_TEXTURE_TEXEL_SIZE)
                .min(MAX_INSTANCE_BUFFER_SIZE);
            let initial_capacity = INITIAL_INSTANCE_DATA_SIZE.min(max_instance_data_size);
            let (instance_data, capacity) =
                Self::create_instance_texture(&device, initial_capacity, max_texture_dimension);
            (
                instance_data,
                capacity,
                max_instance_data_size,
                INSTANCE_TEXTURE_TEXEL_SIZE,
            )
        } else {
            let max_buffer_size = device
                .limits()
                .max_buffer_size
                .min(device.limits().max_storage_buffer_binding_size)
                .min(MAX_INSTANCE_BUFFER_SIZE);
            let initial_capacity = INITIAL_INSTANCE_DATA_SIZE.min(max_buffer_size);
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("instance_buffer"),
                size: initial_capacity,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            (
                InstanceData::Storage(buffer),
                initial_capacity,
                max_buffer_size,
                wgpu::COPY_BUFFER_ALIGNMENT,
            )
        };
        let instance_bind_group =
            Self::create_instance_bind_group(&device, &bind_group_layouts, &instance_data);
        let staging_belt =
            wgpu::util::StagingBelt::new((*device).clone(), INITIAL_INSTANCE_DATA_SIZE);
        let globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals_bind_group"),
            layout: &bind_group_layouts.globals,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &globals_buffer,
                        offset: 0,
                        size: NonZeroU64::new(globals_size),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &globals_buffer,
                        offset: gamma_offset,
                        size: NonZeroU64::new(gamma_size),
                    }),
                },
            ],
        });
        let path_globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("path_globals_bind_group"),
            layout: &bind_group_layouts.globals,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &globals_buffer,
                        offset: path_globals_offset,
                        size: NonZeroU64::new(globals_size),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &globals_buffer,
                        offset: gamma_offset,
                        size: NonZeroU64::new(gamma_size),
                    }),
                },
            ],
        });
        let max_texture_size = device.limits().max_texture_dimension_2d;
        let shader_layers = ShaderLayerResources::new(&device);

        Self {
            resources: WgpuResources {
                device,
                queue,
                pipelines,
                bind_group_layouts,
                atlas_sampler,
                atlas_texture_bind_groups: FxHashMap::default(),
                globals_buffer,
                globals_bind_group,
                path_globals_bind_group,
                instance_data,
                instance_bind_group,
                staging_belt,
                upload_encoder: None,
                path_intermediate_texture: None,
                path_intermediate_view: None,
                path_msaa_texture: None,
                path_msaa_view: None,
                shader_layers,
                glass: None,
            },
            uploaded_globals: None,
            atlas,
            path_globals_offset,
            gamma_offset,
            instance_data_capacity,
            max_instance_data_size,
            instance_data_alignment,
            uses_webgl_instance_data,
            rendering_params,
            is_bgr: false,
            dual_source_blending,
            adapter_info: context.adapter.get_info(),
            target_format,
            max_texture_size,
            clip_window_shadows: false,
        }
    }

    fn create_instance_bind_group(
        device: &wgpu::Device,
        layouts: &WgpuBindGroupLayouts,
        instance_data: &InstanceData,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("instances_bind_group"),
            layout: &layouts.instances,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: match instance_data {
                    InstanceData::Storage(buffer) => buffer.as_entire_binding(),
                    InstanceData::Texture { view, .. } => wgpu::BindingResource::TextureView(view),
                },
            }],
        })
    }

    fn resources(&self) -> &WgpuResources {
        &self.resources
    }

    fn resources_mut(&mut self) -> &mut WgpuResources {
        &mut self.resources
    }

    /// Path rendering goes through an intermediate texture that must match the frame
    /// target's dimensions, so the textures are keyed by size and rebuilt on mismatch.
    fn ensure_intermediate_textures(&mut self, size: Size<DevicePixels>) {
        let width = size.width.0 as u32;
        let height = size.height.0 as u32;
        if self
            .resources
            .path_intermediate_texture
            .as_ref()
            .is_some_and(|texture| texture.width() == width && texture.height() == height)
        {
            return;
        }

        let format = self.target_format;
        let path_sample_count = self.rendering_params.path_sample_count;
        let resources = &mut self.resources;

        let (texture, view) =
            Self::create_path_intermediate(&resources.device, format, width, height);
        resources.path_intermediate_texture = Some(texture);
        resources.path_intermediate_view = Some(view);

        let (path_msaa_texture, path_msaa_view) = Self::create_msaa_if_needed(
            &resources.device,
            format,
            width,
            height,
            path_sample_count,
        )
        .map(|(texture, view)| (Some(texture), Some(view)))
        .unwrap_or((None, None));
        resources.path_msaa_texture = path_msaa_texture;
        resources.path_msaa_view = path_msaa_view;
    }

    fn render_frame(
        &mut self,
        scene: &Scene,
        target: &wgpu::Texture,
        target_view: &wgpu::TextureView,
        size: Size<DevicePixels>,
        premultiplied_alpha: bool,
        clear_color: wgpu::Color,
    ) -> Result<wgpu::SubmissionIndex> {
        anyhow::ensure!(
            size.width.0 > 0 && size.height.0 > 0,
            "invalid render target size: {size:?}"
        );
        anyhow::ensure!(
            size.width.0 as u32 <= self.max_texture_size
                && size.height.0 as u32 <= self.max_texture_size,
            "render target size {size:?} exceeds maximum texture dimension {}",
            self.max_texture_size
        );

        self.atlas.before_frame();

        let gamma_params = GammaParams {
            gamma_ratios: self.rendering_params.gamma_ratios,
            grayscale_enhanced_contrast: self.rendering_params.grayscale_enhanced_contrast,
            subpixel_enhanced_contrast: self.rendering_params.subpixel_enhanced_contrast,
            is_bgr: self.is_bgr as u32,
            _pad: 0,
        };

        let window_mask = scene.window_corner_mask.unwrap_or_default();
        let globals = GlobalParams {
            viewport_size: [size.width.0 as f32, size.height.0 as f32],
            premultiplied_alpha: premultiplied_alpha as u32,
            window_mask_smoothing: window_mask.corner_smoothing,
            window_mask_origin: [window_mask.bounds.origin.x.0, window_mask.bounds.origin.y.0],
            window_mask_size: [
                window_mask.bounds.size.width.0,
                window_mask.bounds.size.height.0,
            ],
            window_mask_radii: [
                window_mask.corner_radii.top_left.0,
                window_mask.corner_radii.top_right.0,
                window_mask.corner_radii.bottom_right.0,
                window_mask.corner_radii.bottom_left.0,
            ],
            clip_window_shadows: self.clip_window_shadows as u32,
            _window_mask_pad: [0; 3],
        };
        let path_globals = GlobalParams {
            premultiplied_alpha: 0,
            ..globals
        };
        let uploaded_globals = self.uploaded_globals.as_ref();
        if uploaded_globals.is_none_or(|(previous, _, _)| {
            bytemuck::bytes_of(previous) != bytemuck::bytes_of(&globals)
        }) {
            self.resources.queue.write_buffer(
                &self.resources.globals_buffer,
                0,
                bytemuck::bytes_of(&globals),
            );
        }
        if uploaded_globals.is_none_or(|(_, previous, _)| {
            bytemuck::bytes_of(previous) != bytemuck::bytes_of(&path_globals)
        }) {
            self.resources.queue.write_buffer(
                &self.resources.globals_buffer,
                self.path_globals_offset,
                bytemuck::bytes_of(&path_globals),
            );
        }
        if uploaded_globals.is_none_or(|(_, _, previous)| {
            bytemuck::bytes_of(previous) != bytemuck::bytes_of(&gamma_params)
        }) {
            self.resources.queue.write_buffer(
                &self.resources.globals_buffer,
                self.gamma_offset,
                bytemuck::bytes_of(&gamma_params),
            );
        }
        self.uploaded_globals = Some((globals, path_globals, gamma_params));

        self.record_frame(scene, target, target_view, size, clear_color)
            .inspect_err(|_| {
                // Queue writes are staged before encoding; flush them even if the frame fails.
                self.resources.queue.submit(std::iter::empty());
            })
    }

    fn record_frame(
        &mut self,
        scene: &Scene,
        frame: &wgpu::Texture,
        frame_view: &wgpu::TextureView,
        size: Size<DevicePixels>,
        clear_color: wgpu::Color,
    ) -> Result<wgpu::SubmissionIndex> {
        let mut instance_offset = 0;
        self.prepare_texture_bind_groups(scene);
        self.resources.shader_layers.drawn = false;
        let glasses = glass_count(scene);
        if glasses > 0 && self.resources.glass.is_none() {
            self.resources.glass = Some(GlassResources::new(
                &self.resources.device,
                self.target_format,
            ));
        }
        if let Some(glass) = &mut self.resources.glass {
            glass.begin_frame(&self.resources.device, glasses);
        }

        let mut encoder =
            self.resources()
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("main_encoder"),
                });
        let encoded = self.encode_scene(
            &mut encoder,
            scene,
            frame,
            frame_view,
            size,
            wgpu::LoadOp::Clear(clear_color),
            &mut instance_offset,
            0,
        );
        if encoded.is_ok() && !self.resources.shader_layers.drawn {
            self.resources.shader_layers.release_textures();
        }
        if let Some(glass) = &mut self.resources.glass {
            glass.finish_frame(&self.resources.queue);
        }

        let resources = &mut self.resources;
        let uploads = resources
            .upload_encoder
            .take()
            .map(|uploads| uploads.finish());
        resources.staging_belt.finish();
        let submission = resources.queue.submit(
            uploads
                .into_iter()
                .chain(encoded.is_ok().then(|| encoder.finish())),
        );
        resources.staging_belt.recall();
        encoded?;
        Ok(submission)
    }

    #[allow(clippy::too_many_arguments)]
    fn encode_scene(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        scene: &Scene,
        frame: &wgpu::Texture,
        frame_view: &wgpu::TextureView,
        size: Size<DevicePixels>,
        load: wgpu::LoadOp<wgpu::Color>,
        instance_offset: &mut u64,
        depth: usize,
    ) -> Result<()> {
        let instance_bindings = self
            .write_instances(scene, instance_offset)
            .with_context(|| {
                format!(
                    "scene too large: {} paths, {} shadows, {} quads, {} underlines, {} monochrome sprites, {} subpixel sprites, {} polychrome sprites",
                    scene.paths.len(),
                    scene.shadows.len(),
                    scene.quads.len(),
                    scene.underlines.len(),
                    scene.monochrome_sprites.len(),
                    scene.subpixel_sprites.len(),
                    scene.polychrome_sprites.len(),
                )
            })?;

        let mut pass = begin_pass(encoder, frame_view, load, "main_pass");
        for batch in scene.batches() {
            match batch {
                PrimitiveBatch::Quads(range) => self.draw_instances(
                    &instance_bindings.quads,
                    &self.resources().pipelines.quads,
                    instance_range(range),
                    &mut pass,
                ),
                PrimitiveBatch::Shadows(range) => self.draw_instances(
                    &instance_bindings.shadows,
                    &self.resources().pipelines.shadows,
                    instance_range(range),
                    &mut pass,
                ),
                PrimitiveBatch::Paths(range) => {
                    let paths = &scene.paths[range];
                    if paths.is_empty() {
                        continue;
                    }

                    drop(pass);
                    let rasterized =
                        self.draw_paths_to_intermediate(encoder, paths, size, instance_offset)?;

                    pass = begin_pass(
                        encoder,
                        frame_view,
                        wgpu::LoadOp::Load,
                        "main_pass_continued",
                    );

                    if rasterized {
                        self.draw_paths_from_intermediate(paths, instance_offset, &mut pass)?;
                    }
                }
                PrimitiveBatch::Underlines(range) => self.draw_instances(
                    &instance_bindings.underlines,
                    &self.resources().pipelines.underlines,
                    instance_range(range),
                    &mut pass,
                ),
                PrimitiveBatch::MonochromeSprites { texture_id, range } => {
                    self.draw_sprites(
                        &instance_bindings.monochrome_sprites,
                        texture_id,
                        &self.resources().pipelines.mono_sprites,
                        instance_range(range),
                        &mut pass,
                    )?;
                }
                PrimitiveBatch::SubpixelSprites { texture_id, range } => {
                    let resources = self.resources();
                    self.draw_sprites(
                        &instance_bindings.subpixel_sprites,
                        texture_id,
                        resources
                            .pipelines
                            .subpixel_sprites
                            .as_ref()
                            .unwrap_or(&resources.pipelines.mono_sprites),
                        instance_range(range),
                        &mut pass,
                    )?;
                }
                PrimitiveBatch::PolychromeSprites { texture_id, range } => {
                    self.draw_sprites(
                        &instance_bindings.polychrome_sprites,
                        texture_id,
                        &self.resources().pipelines.poly_sprites,
                        instance_range(range),
                        &mut pass,
                    )?;
                }
                #[cfg(any(target_os = "linux", target_os = "freebsd"))]
                PrimitiveBatch::Surfaces(range) => {
                    self.draw_surfaces(&scene.surfaces[range], instance_offset, &mut pass)?
                }
                #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
                PrimitiveBatch::Surfaces(_) => {}
                PrimitiveBatch::ShaderLayers(range) => {
                    drop(pass);
                    for layer in &scene.shader_layers[range] {
                        self.draw_shader_layer(
                            encoder,
                            layer,
                            frame,
                            frame_view,
                            size,
                            instance_offset,
                            depth,
                        )?;
                    }
                    pass = begin_pass(
                        encoder,
                        frame_view,
                        wgpu::LoadOp::Load,
                        "main_pass_continued",
                    );
                }
                PrimitiveBatch::Glass(range) => {
                    let Some(glass) = self.resources.glass.as_mut() else {
                        continue;
                    };
                    if !frame.usage().contains(wgpu::TextureUsages::COPY_SRC) {
                        continue;
                    }
                    drop(pass);
                    let pending = glass.draw(
                        &self.resources.device,
                        encoder,
                        &scene.glasses[range],
                        frame,
                        frame_view,
                        scene.window_corner_mask,
                    );
                    pass = begin_pass(
                        encoder,
                        frame_view,
                        wgpu::LoadOp::Load,
                        "main_pass_continued",
                    );
                    if let (Some(pending), Some(glass)) = (pending, self.resources.glass.as_ref()) {
                        glass.draw_pending(
                            &mut pass,
                            pending,
                            Size {
                                width: size.width.0,
                                height: size.height.0,
                            },
                        );
                    }
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_shader_layer(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        layer: &ShaderLayer,
        frame: &wgpu::Texture,
        frame_view: &wgpu::TextureView,
        size: Size<DevicePixels>,
        instance_offset: &mut u64,
        depth: usize,
    ) -> Result<()> {
        let Some(region) = shader_layer_region(layer.bounds, size) else {
            return Ok(());
        };
        let Some(pipeline) = self.shader_layer_pipeline(&layer.shader) else {
            return self.encode_scene(
                encoder,
                &layer.scene,
                frame,
                frame_view,
                size,
                wgpu::LoadOp::Load,
                instance_offset,
                depth + 1,
            );
        };
        self.resources.shader_layers.drawn = true;

        let format = self.target_format;
        let device = self.resources.device.clone();
        let layers = &mut self.resources.shader_layers;
        if layers
            .targets
            .first()
            .is_some_and(|target| !target.has_size(size))
        {
            layers.targets.clear();
        }
        while layers.targets.len() <= depth {
            layers.targets.push(RenderTexture::new(
                &device,
                format,
                size,
                wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
            ));
        }
        let target_view = layers.targets[depth].view.clone();
        let target_texture = layers.targets[depth].texture.clone();
        self.encode_scene(
            encoder,
            &layer.scene,
            &target_texture,
            &target_view,
            size,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            instance_offset,
            depth + 1,
        )?;

        let layers = &mut self.resources.shader_layers;
        let input_index = match layers
            .inputs
            .iter()
            .position(|input| input.has_size(region.size))
        {
            Some(index) => index,
            None => {
                if layers.inputs.len() >= MAX_SHADER_LAYER_INPUTS {
                    layers.inputs.clear();
                }
                layers.inputs.push(RenderTexture::new(
                    &device,
                    format,
                    region.size,
                    wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                ));
                layers.inputs.len() - 1
            }
        };
        let input_texture = layers.inputs[input_index].texture.clone();
        let input_view = layers.inputs[input_index].view.clone();
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &target_texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: region.origin.x.0 as u32,
                    y: region.origin.y.0 as u32,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: &input_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: region.size.width.0 as u32,
                height: region.size.height.0 as u32,
                depth_or_array_layers: 1,
            },
        );

        let Some(scissor) =
            shader_layer_region(layer.content_mask.bounds.intersect(&layer.bounds), size)
        else {
            return Ok(());
        };
        let params = ShaderLayerParams {
            bounds: [
                region.origin.x.0 as f32,
                region.origin.y.0 as f32,
                region.size.width.0 as f32,
                region.size.height.0 as f32,
            ],
            viewport_size: [size.width.0 as f32, size.height.0 as f32],
            pad: [0.0; 2],
        };
        let params_size = std::mem::size_of::<ShaderLayerParams>() as u64;
        let uniforms_offset = params_size
            .next_multiple_of(device.limits().min_uniform_buffer_offset_alignment as u64);
        let uniforms_size = pipeline.uniform_size.max(16).next_multiple_of(16);
        let mut uniforms = vec![0u8; uniforms_size as usize];
        let len = layer.uniforms.len().min(uniforms.len());
        uniforms[..len].copy_from_slice(&layer.uniforms[..len]);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shader_layer_uniforms"),
            size: uniforms_offset + uniforms_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let queue = &self.resources.queue;
        queue.write_buffer(&buffer, 0, bytemuck::bytes_of(&params));
        queue.write_buffer(&buffer, uniforms_offset, &uniforms);

        let layers = &self.resources.shader_layers;
        let content = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shader_layer_content"),
            layout: &layers.content_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&input_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &buffer,
                        offset: uniforms_offset,
                        size: NonZeroU64::new(uniforms_size),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&layers.sampler),
                },
            ],
        });
        let params = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shader_layer_params"),
            layout: &layers.params_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    size: NonZeroU64::new(params_size),
                }),
            }],
        });

        let mut pass = begin_pass(encoder, frame_view, wgpu::LoadOp::Load, "shader_layer_pass");
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, &content, &[]);
        pass.set_bind_group(1, &params, &[]);
        pass.set_scissor_rect(
            scissor.origin.x.0 as u32,
            scissor.origin.y.0 as u32,
            scissor.size.width.0 as u32,
            scissor.size.height.0 as u32,
        );
        pass.draw(0..4, 0..1);
        Ok(())
    }

    fn shader_layer_pipeline(&mut self, shader: &CustomShader) -> Option<ShaderLayerPipeline> {
        if let Some(pipeline) = self.resources.shader_layers.pipelines.get(&shader.id()) {
            return pipeline.clone();
        }
        let pipeline = build_shader_layer_pipeline(
            &self.resources.device,
            &self.resources.shader_layers,
            self.target_format,
            shader,
        )
        .inspect_err(|error| log::error!("custom shader {shader:?} failed: {error:#}"))
        .ok();
        self.resources
            .shader_layers
            .pipelines
            .insert(shader.id(), pipeline.clone());
        pipeline
    }

    fn write_instances(
        &mut self,
        scene: &Scene,
        instance_offset: &mut u64,
    ) -> Result<InstanceBindings> {
        let [
            quads,
            shadows,
            underlines,
            monochrome_sprites,
            subpixel_sprites,
            polychrome_sprites,
        ] = self.write_instance_bindings(
            instance_offset,
            [
                instance_part(&scene.quads),
                instance_part(&scene.shadows),
                instance_part(&scene.underlines),
                instance_part(&scene.monochrome_sprites),
                instance_part(&scene.subpixel_sprites),
                instance_part(&scene.polychrome_sprites),
            ],
        )?;
        Ok(InstanceBindings {
            quads,
            shadows,
            underlines,
            monochrome_sprites,
            subpixel_sprites,
            polychrome_sprites,
        })
    }

    fn create_texture_bind_group(
        &self,
        label: &str,
        texture_view: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        let resources = self.resources();
        resources
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &resources.bind_group_layouts.texture,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(texture_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&resources.atlas_sampler),
                    },
                ],
            })
    }

    fn prepare_texture_bind_groups(&mut self, scene: &Scene) {
        fn collect(scene: &Scene, texture_ids: &mut SmallVec<[AtlasTextureId; 8]>) {
            for batch in scene.batches() {
                let texture_id = match batch {
                    PrimitiveBatch::MonochromeSprites { texture_id, .. }
                    | PrimitiveBatch::SubpixelSprites { texture_id, .. }
                    | PrimitiveBatch::PolychromeSprites { texture_id, .. } => texture_id,
                    _ => continue,
                };
                if !texture_ids.contains(&texture_id) {
                    texture_ids.push(texture_id);
                }
            }
            for layer in &scene.shader_layers {
                collect(&layer.scene, texture_ids);
            }
        }

        let mut texture_ids = SmallVec::<[AtlasTextureId; 8]>::new();
        collect(scene, &mut texture_ids);

        self.resources_mut()
            .atlas_texture_bind_groups
            .retain(|texture_id, _| texture_ids.contains(texture_id));

        for texture_id in texture_ids {
            let Some(texture_info) = self.atlas.get_texture_info(texture_id) else {
                self.resources_mut()
                    .atlas_texture_bind_groups
                    .remove(&texture_id);
                continue;
            };
            let is_current = self
                .resources()
                .atlas_texture_bind_groups
                .get(&texture_id)
                .is_some_and(|cached| cached.texture_generation == texture_info.generation);
            if is_current {
                continue;
            }

            let bind_group =
                self.create_texture_bind_group("atlas_texture_bind_group", &texture_info.view);
            self.resources_mut().atlas_texture_bind_groups.insert(
                texture_id,
                CachedTextureBindGroup {
                    texture_generation: texture_info.generation,
                    bind_group,
                },
            );
        }
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    fn draw_surfaces(
        &mut self,
        surfaces: &[PaintSurface],
        instance_offset: &mut u64,
        pass: &mut wgpu::RenderPass<'_>,
    ) -> Result<()> {
        for surface in surfaces {
            let params = SurfaceParams {
                bounds: surface.bounds.into(),
                content_mask: surface.content_mask.bounds.into(),
                corner_radii: [
                    surface.corner_radii.top_left.0,
                    surface.corner_radii.top_right.0,
                    surface.corner_radii.bottom_right.0,
                    surface.corner_radii.bottom_left.0,
                ],
                corner_smoothing: surface.corner_smoothing,
                pad: 0.0,
            };
            let instances = self.write_instance_binding(instance_offset, &[params])?;
            let texture =
                self.create_texture_bind_group("surface_texture_bind_group", &surface.texture_view);
            let resources = self.resources();
            pass.set_pipeline(&resources.pipelines.surfaces);
            pass.set_bind_group(0, &resources.globals_bind_group, &[]);
            pass.set_bind_group(1, &instances.bind_group, &[]);
            pass.set_bind_group(2, &texture, &[]);
            pass.draw(0..4, instances.first_instance..instances.first_instance + 1);
        }
        Ok(())
    }

    fn draw_instances(
        &self,
        instances: &InstanceBinding,
        pipeline: &wgpu::RenderPipeline,
        range: Range<u32>,
        pass: &mut wgpu::RenderPass<'_>,
    ) {
        if range.is_empty() {
            return;
        }
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.resources().globals_bind_group, &[]);
        pass.set_bind_group(1, &instances.bind_group, &[]);
        pass.draw(
            0..4,
            instances.first_instance + range.start..instances.first_instance + range.end,
        );
    }

    fn draw_sprites(
        &self,
        sprite_instances: &InstanceBinding,
        texture_id: AtlasTextureId,
        pipeline: &wgpu::RenderPipeline,
        range: Range<u32>,
        pass: &mut wgpu::RenderPass<'_>,
    ) -> Result<()> {
        if range.is_empty() {
            return Ok(());
        }
        let resources = self.resources();
        // The atlas has released this texture; the batch belongs to a stale
        // paint that will be replaced once its view re-renders.
        let Some(texture) = resources.atlas_texture_bind_groups.get(&texture_id) else {
            return Ok(());
        };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &resources.globals_bind_group, &[]);
        pass.set_bind_group(1, &sprite_instances.bind_group, &[]);
        pass.set_bind_group(2, &texture.bind_group, &[]);
        pass.draw(
            0..4,
            sprite_instances.first_instance + range.start
                ..sprite_instances.first_instance + range.end,
        );
        Ok(())
    }

    fn draw_paths_from_intermediate(
        &mut self,
        paths: &[Path<ScaledPixels>],
        instance_offset: &mut u64,
        pass: &mut wgpu::RenderPass<'_>,
    ) -> Result<()> {
        let first_path = &paths[0];
        let sprites: Vec<PathSprite> = if paths.last().map(|p| &p.order) == Some(&first_path.order)
        {
            paths
                .iter()
                .map(|p| PathSprite {
                    bounds: p.clipped_bounds(),
                })
                .collect()
        } else {
            let mut bounds = first_path.clipped_bounds();
            for path in paths.iter().skip(1) {
                bounds = bounds.union(&path.clipped_bounds());
            }
            vec![PathSprite { bounds }]
        };

        let Some(path_intermediate_view) = self.resources().path_intermediate_view.clone() else {
            return Ok(());
        };
        let instances = self.write_instance_binding(instance_offset, &sprites)?;
        let texture = self.create_texture_bind_group(
            "path_intermediate_texture_bind_group",
            &path_intermediate_view,
        );
        let resources = self.resources();
        pass.set_pipeline(&resources.pipelines.paths);
        pass.set_bind_group(0, &resources.globals_bind_group, &[]);
        pass.set_bind_group(1, &instances.bind_group, &[]);
        pass.set_bind_group(2, &texture, &[]);
        pass.draw(
            0..4,
            instances.first_instance..instances.first_instance + sprites.len() as u32,
        );
        Ok(())
    }

    fn draw_paths_to_intermediate(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        paths: &[Path<ScaledPixels>],
        size: Size<DevicePixels>,
        instance_offset: &mut u64,
    ) -> Result<bool> {
        let mut vertices = Vec::new();
        for path in paths {
            let bounds = path.clipped_bounds();
            vertices.extend(path.vertices.iter().map(|v| PathRasterizationVertex {
                xy_position: v.xy_position,
                st_position: v.st_position,
                color: path.color,
                bounds,
            }));
        }

        if vertices.is_empty() {
            return Ok(false);
        }

        self.ensure_intermediate_textures(size);

        let vertex_binding = self.write_instance_binding(instance_offset, &vertices)?;

        let resources = self.resources();
        let Some(path_intermediate_view) = resources.path_intermediate_view.as_ref() else {
            return Ok(false);
        };

        let (target_view, resolve_target) = if let Some(ref msaa_view) = resources.path_msaa_view {
            (msaa_view, Some(path_intermediate_view))
        } else {
            (path_intermediate_view, None)
        };

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("path_rasterization_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    resolve_target,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });

            pass.set_pipeline(&resources.pipelines.path_rasterization);
            pass.set_bind_group(0, &resources.path_globals_bind_group, &[]);
            pass.set_bind_group(1, &vertex_binding.bind_group, &[]);
            // The path rasterization shader loads records by vertex index
            // rather than instance index, so the allocation's base shifts the
            // vertex range here.
            pass.draw(
                vertex_binding.first_instance
                    ..vertex_binding.first_instance + vertices.len() as u32,
                0..1,
            );
        }

        Ok(true)
    }

    fn write_instance_binding<T>(
        &mut self,
        instance_offset: &mut u64,
        instances: &[T],
    ) -> Result<InstanceBinding> {
        let [binding] =
            self.write_instance_bindings(instance_offset, [instance_part(instances)])?;
        Ok(binding)
    }

    /// Places each part (its bytes and record stride) after `instance_offset`
    /// and uploads them together. Every allocation starts on a whole record so
    /// the shader can index the shared buffer or texture absolutely, and ends
    /// on the transport's alignment so its padding cannot overlap the next.
    fn write_instance_bindings<const N: usize>(
        &mut self,
        instance_offset: &mut u64,
        parts: [(&[u8], u64); N],
    ) -> Result<[InstanceBinding; N]> {
        let alignment = self.instance_data_alignment;
        let place = |start: u64| {
            let mut end = start;
            let offsets = parts.map(|(data, stride)| {
                if data.is_empty() {
                    return end;
                }
                let offset = end.next_multiple_of(least_common_multiple(alignment, stride));
                end = offset + (data.len() as u64).next_multiple_of(alignment);
                offset
            });
            (offsets, end)
        };
        let (mut offsets, mut end) = place(*instance_offset);
        if end > self.instance_data_capacity {
            self.grow_instance_data(place(0).1)?;
            (offsets, end) = place(0);
        }
        *instance_offset = end;

        let resources = &mut self.resources;
        match &resources.instance_data {
            InstanceData::Storage(buffer) => {
                let start = parts
                    .iter()
                    .zip(offsets)
                    .find(|((data, _), _)| !data.is_empty())
                    .map(|(_, offset)| offset);
                if let Some(start) = start
                    && let Some(size) = NonZeroU64::new(end - start)
                {
                    let encoder = resources.upload_encoder.get_or_insert_with(|| {
                        resources
                            .device
                            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                label: Some("instance_upload_encoder"),
                            })
                    });
                    let mut view = resources
                        .staging_belt
                        .write_buffer(encoder, buffer, start, size);
                    for ((data, _), offset) in parts.iter().zip(offsets) {
                        let at = (offset - start) as usize;
                        view.slice(at..at + data.len()).copy_from_slice(data);
                    }
                }
            }
            InstanceData::Texture { .. } => {
                for ((data, _), offset) in parts.iter().zip(offsets) {
                    if !data.is_empty() {
                        Self::write_instance_texture(resources, offset, data);
                    }
                }
            }
        }

        let mut first_instances = [0; N];
        for (first_instance, ((_, stride), offset)) in
            first_instances.iter_mut().zip(parts.iter().zip(offsets))
        {
            *first_instance =
                u32::try_from(offset / stride).context("instance index exceeds u32 range")?;
        }
        Ok(first_instances.map(|first_instance| InstanceBinding {
            bind_group: resources.instance_bind_group.clone(),
            first_instance,
        }))
    }

    fn write_instance_texture(resources: &WgpuResources, offset: u64, data: &[u8]) {
        let InstanceData::Texture {
            texture,
            width,
            height,
            ..
        } = &resources.instance_data
        else {
            return;
        };
        let mut byte_offset = 0usize;
        let mut texel_offset = offset / INSTANCE_TEXTURE_TEXEL_SIZE;
        while byte_offset < data.len() {
            let x = (texel_offset % u64::from(*width)) as u32;
            let y = (texel_offset / u64::from(*width)) as u32;
            if y >= *height {
                // The capacity check in write_instance_binding should make this
                // unreachable. Truncating silently would leave stale bytes in the
                // texture and draw garbage for the remaining instances.
                debug_assert!(
                    false,
                    "instance texture write out of bounds: row {y} >= height {}",
                    *height
                );
                log::error!(
                    "instance texture write out of bounds; dropping {} bytes of instance data",
                    data.len() - byte_offset
                );
                return;
            }
            let available_texels = u64::from(*width - x);
            let remaining_bytes = data.len() - byte_offset;
            let complete_texels = remaining_bytes as u64 / INSTANCE_TEXTURE_TEXEL_SIZE;
            let texels = complete_texels.min(available_texels);
            if texels > 0 {
                let byte_count = (texels * INSTANCE_TEXTURE_TEXEL_SIZE) as usize;
                resources.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x, y, z: 0 },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &data[byte_offset..byte_offset + byte_count],
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(byte_count as u32),
                        rows_per_image: None,
                    },
                    wgpu::Extent3d {
                        width: texels as u32,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                );
                byte_offset += byte_count;
                texel_offset += texels;
                continue;
            }

            let mut final_texel = [0; INSTANCE_TEXTURE_TEXEL_SIZE as usize];
            final_texel[..remaining_bytes].copy_from_slice(&data[byte_offset..]);
            resources.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x, y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &final_texel,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(INSTANCE_TEXTURE_TEXEL_SIZE as u32),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            break;
        }
    }

    fn grow_instance_data(&mut self, required: u64) -> Result<()> {
        let capacity = (self.instance_data_capacity * 2)
            .max(required.next_power_of_two())
            .min(self.max_instance_data_size);
        anyhow::ensure!(
            capacity >= required,
            "instance data needs {required} bytes, above the maximum of {}",
            self.max_instance_data_size
        );
        anyhow::ensure!(
            capacity > self.instance_data_capacity,
            "frame instance data exceeds the {}-byte maximum",
            self.max_instance_data_size
        );
        log::debug!(
            "instance data grown from {} to {capacity}",
            self.instance_data_capacity
        );
        // Bind groups created earlier in the frame keep the previous buffer or
        // texture alive, so allocations written before the grow remain valid;
        // only subsequent writes land in the new allocation.
        let uses_webgl_instance_data = self.uses_webgl_instance_data;
        let resources = self.resources_mut();
        if uses_webgl_instance_data {
            let max_texture_dimension = resources.device.limits().max_texture_dimension_2d;
            let (instance_data, actual_capacity) =
                Self::create_instance_texture(&resources.device, capacity, max_texture_dimension);
            resources.instance_data = instance_data;
            self.instance_data_capacity = actual_capacity;
        } else {
            resources.instance_data =
                InstanceData::Storage(resources.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("instance_buffer"),
                    size: capacity,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            self.instance_data_capacity = capacity;
        }
        let resources = &mut self.resources;
        resources.instance_bind_group = Self::create_instance_bind_group(
            &resources.device,
            &resources.bind_group_layouts,
            &resources.instance_data,
        );
        Ok(())
    }
}

impl WgpuRenderer {
    /// Mark the surface as unconfigured so rendering is skipped until a new
    /// surface is provided via [`replace_surface`](Self::replace_surface).
    ///
    /// This does **not** drop the renderer — the device, queue, atlas, and
    /// pipelines stay alive.  Use this when the native window is destroyed
    /// (e.g. Android `TerminateWindow`) but you intend to re-create the
    /// surface later without losing cached atlas textures.
    pub fn unconfigure_surface(&mut self) {
        self.state = match std::mem::replace(&mut self.state, RendererState::Released) {
            RendererState::Ready { core, surface } => {
                drop(surface);
                RendererState::Unconfigured { core }
            }
            state @ (RendererState::Unconfigured { .. } | RendererState::Released) => state,
        };
        // Drop intermediate textures since they reference the old surface size.
        if let Some(core) = self.core_mut() {
            core.resources.invalidate_intermediate_textures();
        }
    }

    /// Replace the wgpu surface with a new one (e.g. after Android destroys
    /// and recreates the native window).  Keeps the device, queue, atlas, and
    /// all pipelines intact so cached `AtlasTextureId`s remain valid.
    ///
    /// The `instance` **must** be the same [`wgpu::Instance`] that was used to
    /// create the adapter and device (i.e. from the [`WgpuContext`]).  Using a
    /// different instance will cause a "Device does not exist" panic because
    /// the wgpu device is bound to its originating instance.
    #[cfg(not(target_family = "wasm"))]
    pub fn replace_surface<W: HasWindowHandle>(
        &mut self,
        window: &W,
        config: WgpuSurfaceConfig,
        instance: &wgpu::Instance,
    ) -> anyhow::Result<()> {
        let window_handle = window
            .window_handle()
            .map_err(|e| anyhow::anyhow!("Failed to get window handle: {e}"))?;

        let surface = create_surface(instance, window_handle.as_raw())?;

        let width = (config.size.width.0 as u32).max(1);
        let height = (config.size.height.0 as u32).max(1);

        let alpha_mode = if config.transparent {
            self.transparent_alpha_mode
        } else {
            self.opaque_alpha_mode
        };

        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface_config.alpha_mode = alpha_mode;
        if let Some(mode) = config.preferred_present_mode {
            self.surface_config.present_mode = mode;
        }

        let mut core = match std::mem::replace(&mut self.state, RendererState::Released) {
            RendererState::Ready {
                core,
                surface: old_surface,
            } => {
                drop(old_surface);
                core
            }
            RendererState::Unconfigured { core } => core,
            RendererState::Released => {
                anyhow::bail!("Cannot replace the surface: GPU resources have been released")
            }
        };
        surface.configure(&core.resources.device, &self.surface_config);
        core.resources.invalidate_intermediate_textures();
        self.state = RendererState::Ready { surface, core };

        Ok(())
    }

    pub fn destroy(&mut self) {
        // Release surface-bound GPU resources eagerly so the underlying native
        // window can be destroyed before the renderer itself is dropped.
        self.state = RendererState::Released;
    }

    /// Returns true if the GPU device was lost and recovery is needed.
    pub fn device_lost(&self) -> bool {
        self.device_errors.device_lost()
    }

    /// Returns true if a redraw is needed because GPU state was cleared.
    /// Calling this method clears the flag.
    pub fn needs_redraw(&mut self) -> bool {
        std::mem::take(&mut self.needs_redraw)
    }

    /// Recovers from a lost GPU device by recreating the renderer with a new context.
    ///
    /// Call this after detecting `device_lost()` returns true.
    ///
    /// This method coordinates recovery across multiple windows:
    /// - The first window to call this will recreate the shared context
    /// - Subsequent windows will adopt the already-recovered context
    #[cfg(not(target_family = "wasm"))]
    pub fn recover<W>(&mut self, window: &W) -> anyhow::Result<()>
    where
        W: HasWindowHandle + HasDisplayHandle + std::fmt::Debug + Send + Sync + Clone + 'static,
    {
        let gpu_context = self.context.as_ref().expect("recover requires gpu_context");

        // Check if another window already recovered the context
        let needs_new_context = gpu_context
            .borrow()
            .as_ref()
            .is_none_or(|ctx| ctx.device_lost());

        let window_handle = window
            .window_handle()
            .map_err(|e| anyhow::anyhow!("Failed to get window handle: {e}"))?;

        let surface = if needs_new_context {
            log::warn!("GPU device lost, recreating context...");

            // Drop old resources to release Arc<Device>/Arc<Queue> and GPU resources
            self.state = RendererState::Released;
            *gpu_context.borrow_mut() = None;

            // Wait briefly for the GPU driver to stabilize, then try to
            // recreate the context without software renderers. If this fails
            // the caller should request another frame and retry — the real GPU
            // may need more time to come back (e.g. after suspend/resume).
            std::thread::sleep(std::time::Duration::from_millis(350));

            let (new_context, surface) = create_context_and_surface(
                window,
                window_handle.as_raw(),
                self.compositor_gpu,
                true,
            )?;
            *gpu_context.borrow_mut() = Some(new_context);
            surface
        } else {
            let ctx_ref = gpu_context.borrow();
            let instance = &ctx_ref.as_ref().unwrap().instance;
            create_surface(instance, window_handle.as_raw())?
        };

        let config = WgpuSurfaceConfig {
            size: zpui::Size {
                width: zpui::DevicePixels(self.surface_config.width as i32),
                height: zpui::DevicePixels(self.surface_config.height as i32),
            },
            transparent: self.surface_config.alpha_mode != wgpu::CompositeAlphaMode::Opaque,
            preferred_present_mode: Some(self.surface_config.present_mode),
        };
        let gpu_context = Rc::clone(gpu_context);
        let ctx_ref = gpu_context.borrow();
        let context = ctx_ref.as_ref().expect("context should exist");

        self.state = RendererState::Released;
        self.atlas.handle_device_lost(context);

        let is_bgr = self.is_bgr;
        *self = Self::new_internal(
            Some(gpu_context.clone()),
            context,
            surface,
            config,
            self.compositor_gpu,
            self.atlas.clone(),
        )?;
        self.set_subpixel_layout(is_bgr);

        log::info!("GPU recovery complete");
        Ok(())
    }
}

#[cfg(all(
    not(target_family = "wasm"),
    any(test, feature = "bench-support", feature = "test-support")
))]
struct HeadlessRenderTarget {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

#[cfg(all(
    not(target_family = "wasm"),
    any(test, feature = "bench-support", feature = "test-support")
))]
impl HeadlessRenderTarget {
    fn size(&self) -> Size<DevicePixels> {
        Size {
            width: DevicePixels(self.texture.width() as i32),
            height: DevicePixels(self.texture.height() as i32),
        }
    }
}

#[cfg(all(
    not(target_family = "wasm"),
    any(test, feature = "bench-support", feature = "test-support")
))]
pub struct WgpuHeadlessRenderer {
    context: WgpuContext,
    core: WgpuRendererCore,
    render_target: Option<HeadlessRenderTarget>,
    observed_error_generation: u64,
}

#[cfg(all(
    not(target_family = "wasm"),
    any(test, feature = "bench-support", feature = "test-support")
))]
impl WgpuHeadlessRenderer {
    pub fn new() -> anyhow::Result<Self> {
        let (context, target_format) = WgpuContext::new_headless()?;
        let atlas = Arc::new(WgpuAtlas::from_context(&context));
        let core = WgpuRendererCore::new(
            &context,
            atlas,
            target_format,
            wgpu::CompositeAlphaMode::Opaque,
        );

        Ok(Self {
            context,
            core,
            render_target: None,
            observed_error_generation: 0,
        })
    }

    fn ensure_render_target(&mut self, size: Size<DevicePixels>) -> anyhow::Result<()> {
        anyhow::ensure!(
            size.width.0 > 0 && size.height.0 > 0,
            "invalid headless render target size: {size:?}"
        );
        anyhow::ensure!(
            size.width.0 as u32 <= self.core.max_texture_size
                && size.height.0 as u32 <= self.core.max_texture_size,
            "headless render target size {size:?} exceeds maximum texture dimension {}",
            self.core.max_texture_size
        );
        if self
            .render_target
            .as_ref()
            .is_some_and(|target| target.size() == size)
        {
            return Ok(());
        }

        let texture = self
            .core
            .resources
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("headless_render_target"),
                size: wgpu::Extent3d {
                    width: size.width.0 as u32,
                    height: size.height.0 as u32,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.core.target_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.render_target = Some(HeadlessRenderTarget { texture, view });
        Ok(())
    }

    fn check_gpu_errors(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.context.device_lost(),
            "GPU device was lost during headless rendering"
        );
        if let Some(error) = self
            .context
            .errors()
            .observe_error(&mut self.observed_error_generation)
        {
            anyhow::bail!("GPU error during headless rendering: {error}");
        }
        Ok(())
    }

    fn render(&mut self, scene: &Scene, size: Size<DevicePixels>) -> anyhow::Result<()> {
        self.check_gpu_errors()?;
        self.ensure_render_target(size)?;
        let target = self
            .render_target
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Headless render target was not created"))?;
        let (texture, view) = (target.texture.clone(), target.view.clone());
        self.core
            .render_frame(scene, &texture, &view, size, false, wgpu::Color::BLACK)?;
        Ok(())
    }

    /// Copies the current render target back to the CPU. Dimensions come from the
    /// target texture itself, so the copy can never disagree with what was rendered.
    fn read_image(&mut self) -> anyhow::Result<image::RgbaImage> {
        let target = self
            .render_target
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Headless render target was not created"))?;
        let width = target.texture.width();
        let height = target.texture.height();
        let bytes_per_row = width
            .checked_mul(4)
            .ok_or_else(|| anyhow::anyhow!("Headless render target row size overflowed"))?;
        let padded_bytes_per_row = bytes_per_row
            .checked_next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            .ok_or_else(|| anyhow::anyhow!("Headless padded row size overflowed"))?;
        let buffer_size = u64::from(padded_bytes_per_row)
            .checked_mul(u64::from(height))
            .ok_or_else(|| anyhow::anyhow!("Headless readback buffer size overflowed"))?;
        anyhow::ensure!(
            buffer_size <= self.core.resources.device.limits().max_buffer_size,
            "Headless readback buffer size {buffer_size} exceeds maximum buffer size {}",
            self.core.resources.device.limits().max_buffer_size
        );
        let readback_buffer = self
            .core
            .resources
            .device
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("headless_readback_buffer"),
                size: buffer_size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
        let mut encoder =
            self.core
                .resources
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("headless_readback_encoder"),
                });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let submission = self
            .core
            .resources
            .queue
            .submit(std::iter::once(encoder.finish()));
        let (sender, receiver) = std::sync::mpsc::channel();
        readback_buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                if sender.send(result).is_err() {
                    log::error!("Headless readback receiver was dropped before mapping completed");
                }
            });
        self.core
            .resources
            .device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(std::time::Duration::from_secs(30)),
            })
            .map_err(|error| anyhow::anyhow!("Failed to wait for headless rendering: {error}"))?;
        receiver
            .recv_timeout(std::time::Duration::from_secs(30))
            .map_err(|error| anyhow::anyhow!("Failed to receive headless mapping result: {error}"))?
            .map_err(|error| anyhow::anyhow!("Failed to map headless readback buffer: {error}"))?;
        self.check_gpu_errors()?;

        let mapped_data = readback_buffer
            .slice(..)
            .get_mapped_range()
            .map_err(|error| anyhow::anyhow!("Failed to read headless readback buffer: {error}"))?;
        let pixel_capacity = usize::try_from(u64::from(bytes_per_row) * u64::from(height))
            .map_err(|_| anyhow::anyhow!("Headless image size exceeds addressable memory"))?;
        let mut pixels = Vec::with_capacity(pixel_capacity);
        for row in mapped_data
            .chunks_exact(padded_bytes_per_row as usize)
            .take(height as usize)
        {
            pixels.extend_from_slice(&row[..bytes_per_row as usize]);
        }
        drop(mapped_data);
        readback_buffer.unmap();

        if self.core.target_format == wgpu::TextureFormat::Bgra8Unorm {
            for pixel in pixels.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
        }

        image::RgbaImage::from_raw(width, height, pixels)
            .ok_or_else(|| anyhow::anyhow!("Failed to create image from headless pixel data"))
    }
}

#[cfg(all(
    not(target_family = "wasm"),
    any(test, feature = "bench-support", feature = "test-support")
))]
impl zpui::PlatformHeadlessRenderer for WgpuHeadlessRenderer {
    fn render_scene_to_image(
        &mut self,
        scene: &Scene,
        size: Size<DevicePixels>,
    ) -> anyhow::Result<image::RgbaImage> {
        self.render(scene, size)?;
        self.read_image()
    }

    fn render_scene(&mut self, scene: &Scene, size: Size<DevicePixels>) -> anyhow::Result<()> {
        self.render(scene, size)
    }

    fn sprite_atlas(&self) -> Arc<dyn zpui::PlatformAtlas> {
        self.core.atlas.clone()
    }
}

fn begin_pass<'encoder>(
    encoder: &'encoder mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
    label: &str,
) -> wgpu::RenderPass<'encoder> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
            depth_slice: None,
        })],
        depth_stencil_attachment: None,
        ..Default::default()
    })
}

/// The whole device pixels `bounds` covers, clipped to the viewport.
fn shader_layer_region(
    bounds: Bounds<ScaledPixels>,
    viewport_size: Size<DevicePixels>,
) -> Option<Bounds<DevicePixels>> {
    let left = (bounds.origin.x.0.round() as i32).max(0);
    let top = (bounds.origin.y.0.round() as i32).max(0);
    let right = (bounds.bottom_right().x.0.round() as i32).min(viewport_size.width.0);
    let bottom = (bounds.bottom_right().y.0.round() as i32).min(viewport_size.height.0);
    (right > left && bottom > top).then(|| Bounds {
        origin: Point {
            x: DevicePixels(left),
            y: DevicePixels(top),
        },
        size: Size {
            width: DevicePixels(right - left),
            height: DevicePixels(bottom - top),
        },
    })
}

/// Check a custom shader against the interface [`CustomShader`] documents,
/// returning its fragment entry point and the size of its uniform block.
fn shader_layer_interface(module: &naga::Module) -> Result<(String, u64)> {
    let mut layouter = naga::proc::Layouter::default();
    layouter.update(module.to_ctx())?;
    let mut uniform_size = 0;
    for (_, global) in module.global_variables.iter() {
        let Some(binding) = &global.binding else {
            continue;
        };
        let supported = match (
            binding.group,
            binding.binding,
            &module.types[global.ty].inner,
        ) {
            (
                0,
                0,
                naga::TypeInner::Image {
                    dim: naga::ImageDimension::D2,
                    arrayed: false,
                    class:
                        naga::ImageClass::Sampled {
                            kind: naga::ScalarKind::Float,
                            multi: false,
                        },
                },
            ) => true,
            (0, 1, _) if global.space == naga::AddressSpace::Uniform => {
                uniform_size = u64::from(layouter[global.ty].size);
                true
            }
            (0, 2, naga::TypeInner::Sampler { comparison: false }) => true,
            _ => false,
        };
        anyhow::ensure!(
            supported,
            "unsupported binding @group({}) @binding({})",
            binding.group,
            binding.binding
        );
    }

    let entry_point = module
        .entry_points
        .iter()
        .find(|entry_point| entry_point.stage == naga::ShaderStage::Fragment)
        .context("no fragment entry point")?;
    let is_layer_position =
        |binding: &Option<naga::Binding>, ty: naga::Handle<naga::Type>| match binding {
            Some(naga::Binding::BuiltIn(_)) => true,
            Some(naga::Binding::Location { location: 0, .. }) => matches!(
                module.types[ty].inner,
                naga::TypeInner::Vector {
                    size: naga::VectorSize::Bi,
                    scalar: naga::Scalar::F32,
                }
            ),
            _ => false,
        };
    for argument in &entry_point.function.arguments {
        let supported = match (&argument.binding, &module.types[argument.ty].inner) {
            (None, naga::TypeInner::Struct { members, .. }) => members
                .iter()
                .all(|member| is_layer_position(&member.binding, member.ty)),
            (binding, _) => is_layer_position(binding, argument.ty),
        };
        anyhow::ensure!(
            supported,
            "fragment inputs other than @location(0) vec2<f32> are not supplied"
        );
    }
    Ok((entry_point.name.clone(), uniform_size))
}

fn build_shader_layer_pipeline(
    device: &wgpu::Device,
    layers: &ShaderLayerResources,
    format: wgpu::TextureFormat,
    shader: &CustomShader,
) -> Result<ShaderLayerPipeline> {
    let module = naga::front::wgsl::parse_str(shader.wgsl())
        .map_err(|error| anyhow::anyhow!(error.emit_to_string(shader.wgsl())))?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)?;
    let (entry_point, uniform_size) = shader_layer_interface(&module)?;

    #[cfg(not(target_family = "wasm"))]
    let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let fragment_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("custom_shader"),
        source: wgpu::ShaderSource::Wgsl(shader.wgsl().into()),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("shader_layer"),
        layout: Some(&layers.pipeline_layout),
        vertex: wgpu::VertexState {
            module: &layers.vertex_module,
            entry_point: Some("shader_layer_vertex"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &fragment_module,
            entry_point: Some(&entry_point),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });
    #[cfg(not(target_family = "wasm"))]
    if let Some(error) = zpui::block_on(error_scope.pop()) {
        anyhow::bail!("{error}");
    }
    Ok(ShaderLayerPipeline {
        pipeline,
        uniform_size,
    })
}

fn instance_range(range: Range<usize>) -> Range<u32> {
    range.start as u32..range.end as u32
}

#[cfg(not(target_family = "wasm"))]
fn create_context_and_surface<W>(
    window: &W,
    raw_window_handle: raw_window_handle::RawWindowHandle,
    compositor_gpu: Option<CompositorGpuHint>,
    reject_software: bool,
) -> anyhow::Result<(WgpuContext, wgpu::Surface<'static>)>
where
    W: HasDisplayHandle + std::fmt::Debug + Send + Sync + Clone + 'static,
{
    let device_id_override = std::env::var("ZED_DEVICE_ID").ok();
    create_with_backend_fallback(
        device_id_override.as_deref(),
        reject_software,
        |backends, reject_software| {
            let instance =
                WgpuContext::instance_with_backends(Some(Box::new(window.clone())), backends);
            let surface = create_surface(&instance, raw_window_handle)?;
            let context = if reject_software {
                WgpuContext::new_rejecting_software(instance, &surface, compositor_gpu)?
            } else {
                WgpuContext::new(instance, &surface, compositor_gpu)?
            };
            if backends == wgpu::Backends::VULKAN {
                anyhow::ensure!(
                    supports_vulkan_fast_path(context.adapter.get_info().device_type),
                    "Vulkan adapter needs comparison with OpenGL adapters"
                );
            }
            Ok((context, surface))
        },
    )
}

#[cfg(not(target_family = "wasm"))]
fn supports_vulkan_fast_path(device_type: wgpu::DeviceType) -> bool {
    matches!(
        device_type,
        wgpu::DeviceType::DiscreteGpu | wgpu::DeviceType::IntegratedGpu
    )
}

#[cfg(not(target_family = "wasm"))]
fn create_with_backend_fallback<T>(
    device_id_override: Option<&str>,
    reject_software: bool,
    mut create: impl FnMut(wgpu::Backends, bool) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    if !device_id_override.is_some_and(|id| crate::wgpu_context::parse_pci_id(id).is_ok()) {
        match create(wgpu::Backends::VULKAN, true) {
            Ok(context) => return Ok(context),
            Err(error) => {
                log::info!(
                    "Hardware Vulkan initialization failed: {error:#}; trying Vulkan and OpenGL"
                );
            }
        }
    }
    create(wgpu::Backends::VULKAN | wgpu::Backends::GL, reject_software)
}

#[cfg(not(target_family = "wasm"))]
fn create_surface(
    instance: &wgpu::Instance,
    raw_window_handle: raw_window_handle::RawWindowHandle,
) -> anyhow::Result<wgpu::Surface<'static>> {
    unsafe {
        instance
            .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                // Fall back to the display handle already provided via InstanceDescriptor::display.
                raw_display_handle: None,
                raw_window_handle,
            })
            .map_err(|e| anyhow::anyhow!("Failed to create surface: {e}"))
    }
}

struct RenderingParameters {
    path_sample_count: u32,
    gamma_ratios: [f32; 4],
    grayscale_enhanced_contrast: f32,
    subpixel_enhanced_contrast: f32,
}

impl RenderingParameters {
    fn new(adapter: &wgpu::Adapter, surface_format: wgpu::TextureFormat) -> Self {
        use std::env;

        let format_features = adapter.get_texture_format_features(surface_format);
        let path_sample_count = [4, 2, 1]
            .into_iter()
            .find(|&n| format_features.flags.sample_count_supported(n))
            .unwrap_or(1);

        let gamma = env::var("ZED_FONTS_GAMMA")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|value: &f32| value.is_finite())
            .unwrap_or(1.8_f32)
            .clamp(1.0, 2.2);
        let gamma_ratios = get_gamma_correction_ratios(gamma);

        let grayscale_enhanced_contrast = env::var("ZED_FONTS_GRAYSCALE_ENHANCED_CONTRAST")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|value: &f32| value.is_finite())
            .unwrap_or(1.0_f32)
            .max(0.0);

        let subpixel_enhanced_contrast = env::var("ZED_FONTS_SUBPIXEL_ENHANCED_CONTRAST")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|value: &f32| value.is_finite())
            .unwrap_or(0.5_f32)
            .max(0.0);

        Self {
            path_sample_count,
            gamma_ratios,
            grayscale_enhanced_contrast,
            subpixel_enhanced_contrast,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zpui::{
        BorderStyle, ColorSpace, ContentMask, Corners, Edges, Hsla, MonochromeSprite,
        PolychromeSprite, Quad, Shadow, Size, SubpixelSprite, Underline, linear_color_stop,
        linear_gradient,
    };
    #[cfg(target_os = "linux")]
    use zpui::{DevicePixels, PlatformHeadlessRenderer, Scene};

    #[cfg(target_os = "linux")]
    fn device_size(width: i32, height: i32) -> Size<DevicePixels> {
        Size {
            width: DevicePixels(width),
            height: DevicePixels(height),
        }
    }

    #[cfg(target_os = "linux")]
    fn solid_quad(x: f32, y: f32, width: f32, height: f32, color: Hsla) -> Quad {
        let bounds = Bounds {
            origin: Point {
                x: x.into(),
                y: y.into(),
            },
            size: Size {
                width: width.into(),
                height: height.into(),
            },
        };
        Quad {
            order: 0,
            border_style: BorderStyle::Solid,
            bounds,
            content_mask: ContentMask { bounds },
            background: color.into(),
            border_color: color,
            corner_radii: Corners::default(),
            border_widths: Edges::default(),
            corner_smoothing: 2.0,
            pad: [0; 3],
        }
    }

    /// Channels are compared with a small tolerance so the assertions hold across
    /// drivers without pinning exact rasterizer output.
    #[cfg(target_os = "linux")]
    fn assert_pixel(image: &image::RgbaImage, x: u32, y: u32, expected: [u8; 4]) {
        let actual = image.get_pixel(x, y).0;
        assert!(
            actual
                .iter()
                .zip(expected)
                .all(|(actual, expected)| actual.abs_diff(expected) <= 3),
            "pixel ({x}, {y}) was {actual:?}, expected {expected:?}"
        );
    }

    #[cfg(target_os = "linux")]
    const RED: [u8; 4] = [255, 0, 0, 255];
    #[cfg(target_os = "linux")]
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    #[cfg(target_os = "linux")]
    const BLACK: [u8; 4] = [0, 0, 0, 255];

    #[cfg(target_os = "linux")]
    mod wayland {
        use super::*;
        use wayland_client::{
            Connection, Dispatch, Proxy, QueueHandle, delegate_noop,
            globals::{GlobalListContents, registry_queue_init},
            protocol::{wl_compositor, wl_registry, wl_surface},
        };
        use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

        #[derive(Default)]
        struct State {
            configured: bool,
        }

        impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
            fn event(
                _: &mut Self,
                _: &wl_registry::WlRegistry,
                _: wl_registry::Event,
                _: &GlobalListContents,
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
            }
        }

        impl Dispatch<xdg_wm_base::XdgWmBase, ()> for State {
            fn event(
                _: &mut Self,
                proxy: &xdg_wm_base::XdgWmBase,
                event: xdg_wm_base::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
                if let xdg_wm_base::Event::Ping { serial } = event {
                    proxy.pong(serial);
                }
            }
        }

        impl Dispatch<xdg_surface::XdgSurface, ()> for State {
            fn event(
                state: &mut Self,
                proxy: &xdg_surface::XdgSurface,
                event: xdg_surface::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
                if let xdg_surface::Event::Configure { serial } = event {
                    proxy.ack_configure(serial);
                    state.configured = true;
                }
            }
        }

        delegate_noop!(State: ignore wl_compositor::WlCompositor);
        delegate_noop!(State: ignore wl_surface::WlSurface);
        delegate_noop!(State: ignore xdg_toplevel::XdgToplevel);

        #[test]
        #[ignore = "requires a live Wayland compositor and Vulkan adapter"]
        fn failed_frame_releases_acquired_swapchain_images() -> anyhow::Result<()> {
            let connection = Connection::connect_to_env()?;
            let (globals, mut events) = registry_queue_init::<State>(&connection)?;
            let qh = events.handle();
            let compositor = globals.bind::<wl_compositor::WlCompositor, _, _>(&qh, 1..=4, ())?;
            let shell = globals.bind::<xdg_wm_base::XdgWmBase, _, _>(&qh, 1..=1, ())?;
            let surface = compositor.create_surface(&qh, ());
            let shell_surface = shell.get_xdg_surface(&surface, &qh, ());
            let toplevel = shell_surface.get_toplevel(&qh, ());
            toplevel.set_title("GPUI frame recovery regression".into());
            shell_surface.set_window_geometry(0, 0, 128, 128);
            surface.commit();
            let mut state = State::default();
            events.roundtrip(&mut state)?;
            anyhow::ensure!(
                state.configured,
                "compositor did not configure the test window"
            );

            let instance = WgpuContext::instance_with_backends(
                Some(Box::new(connection.backend())),
                wgpu::Backends::VULKAN,
            );
            let window = std::ptr::NonNull::new(surface.id().as_ptr().cast())
                .context("Wayland surface pointer")?;
            let gpu_surface = create_surface(
                &instance,
                raw_window_handle::RawWindowHandle::Wayland(
                    raw_window_handle::WaylandWindowHandle::new(window),
                ),
            )?;
            let context = WgpuContext::new(instance, &gpu_surface, None)?;
            let adapter = context.adapter.get_info();
            assert_eq!(adapter.backend, wgpu::Backend::Vulkan);
            eprintln!("frame recovery test adapter: {adapter:?}");
            let mut renderer = WgpuRenderer::new_internal(
                None,
                &context,
                gpu_surface,
                WgpuSurfaceConfig {
                    size: device_size(128, 128),
                    transparent: false,
                    preferred_present_mode: Some(wgpu::PresentMode::Mailbox),
                },
                None,
                Arc::new(WgpuAtlas::from_context(&context)),
            )?;
            let mut scene = Scene::default();
            scene.insert_primitive(solid_quad(0.0, 0.0, 128.0, 128.0, zpui::red()));
            scene.finish();
            assert!(renderer.draw(&scene), "initial healthy frame failed");
            events.roundtrip(&mut state)?;

            for attempt in 0..8 {
                let core = renderer.core_mut().context("ready renderer")?;
                let capacity = core.instance_data_capacity;
                let maximum = core.max_instance_data_size;
                core.instance_data_capacity = 0;
                core.max_instance_data_size = 0;
                assert!(!renderer.draw(&scene), "capacity error was not reported");
                let core = renderer.core_mut().context("ready renderer")?;
                core.instance_data_capacity = capacity;
                core.max_instance_data_size = maximum;
                assert!(
                    renderer.draw(&scene),
                    "healthy frame after error {attempt} failed"
                );
                events.roundtrip(&mut state)?;
                eprintln!("frame recovery attempt {attempt}: healthy frame presented");
            }
            let mut error_generation = 0;
            assert!(
                context
                    .errors()
                    .observe_error(&mut error_generation)
                    .is_none()
            );
            drop(renderer);
            toplevel.destroy();
            shell_surface.destroy();
            surface.destroy();
            connection.flush()?;
            Ok(())
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn nonfinite_font_environment_uses_defaults_and_draws_glyphs() -> anyhow::Result<()> {
        const CHILD: &str = "GPUI_NONFINITE_FONT_ENV_TEST_CHILD";
        if std::env::var_os(CHILD).is_none() {
            for value in ["NaN", "inf", "-inf"] {
                let output = std::process::Command::new(std::env::current_exe()?)
                    .args([
                        "--exact",
                        "wgpu_renderer::tests::nonfinite_font_environment_uses_defaults_and_draws_glyphs",
                        "--nocapture",
                    ])
                    .env(CHILD, "1")
                    .env("ZED_FONTS_GAMMA", value)
                    .env("ZED_FONTS_GRAYSCALE_ENHANCED_CONTRAST", value)
                    .env("ZED_FONTS_SUBPIXEL_ENHANCED_CONTRAST", value)
                    .output()?;
                assert!(
                    output.status.success(),
                    "font environment {value}:\n{}\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr),
                );
                eprintln!(
                    "font environment {value}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            return Ok(());
        }

        let mut renderer = WgpuHeadlessRenderer::new()?;
        let size = device_size(4, 4);
        eprintln!(
            "font test adapter: {:?}",
            renderer.context.adapter.get_info()
        );
        let tile = renderer
            .sprite_atlas()
            .get_or_insert_with(
                zpui::RenderSvgParams {
                    path: "font_environment_test".into(),
                    size,
                }
                .into(),
                &mut || Ok(Some((size, std::borrow::Cow::Owned(vec![128; 16])))),
            )?
            .context("glyph tile")?;
        let bounds = Bounds {
            origin: Point::default(),
            size: Size {
                width: ScaledPixels(4.0),
                height: ScaledPixels(4.0),
            },
        };
        let mut scene = Scene::default();
        scene.insert_primitive(MonochromeSprite {
            order: 0,
            pad: 0,
            bounds,
            content_mask: ContentMask { bounds },
            color: zpui::white(),
            tile,
            transformation: zpui::TransformationMatrix::unit(),
        });
        scene.finish();
        let image = renderer.render_scene_to_image(&scene, size)?;
        let pixel = image.get_pixel(2, 2).0;
        assert!(pixel[0] > 0 && pixel[0] < 255, "glyph pixel: {pixel:?}");
        assert_eq!(pixel[0], pixel[1]);
        assert_eq!(pixel[1], pixel[2]);
        assert_eq!(pixel[3], 255);
        let parameters = &renderer.core.rendering_params;
        assert_eq!(parameters.gamma_ratios, get_gamma_correction_ratios(1.8));
        assert_eq!(parameters.grayscale_enhanced_contrast, 1.0);
        assert_eq!(parameters.subpixel_enhanced_contrast, 0.5);
        eprintln!("nonfinite font environment renders glyph pixel {pixel:?}");
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn headless_renderer_draws_quads_with_distinct_colors() -> anyhow::Result<()> {
        let mut renderer = WgpuHeadlessRenderer::new()?;
        let mut scene = Scene::default();
        scene.insert_primitive(solid_quad(0.0, 0.0, 32.0, 32.0, zpui::red()));
        scene.insert_primitive(solid_quad(32.0, 0.0, 32.0, 32.0, zpui::blue()));
        scene.finish();

        let image = renderer.render_scene_to_image(&scene, device_size(64, 32))?;
        assert_eq!(image.dimensions(), (64, 32));
        assert_pixel(&image, 8, 16, RED);
        assert_pixel(&image, 24, 16, RED);
        assert_pixel(&image, 40, 16, BLUE);
        assert_pixel(&image, 56, 16, BLUE);
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn headless_renderer_captures_each_requested_size() -> anyhow::Result<()> {
        let mut renderer = WgpuHeadlessRenderer::new()?;
        let mut scene = Scene::default();
        scene.insert_primitive(solid_quad(2.0, 2.0, 4.0, 3.0, zpui::red()));
        scene.finish();

        // 13 px rows are 52 bytes, forcing readback to strip copy-row padding.
        let image = renderer.render_scene_to_image(&scene, device_size(13, 7))?;
        assert_eq!(image.dimensions(), (13, 7));
        assert_pixel(&image, 0, 0, BLACK);
        assert_pixel(&image, 3, 3, RED);
        assert_pixel(&image, 12, 6, BLACK);

        let image = renderer.render_scene_to_image(&scene, device_size(17, 9))?;
        assert_eq!(image.dimensions(), (17, 9));
        assert_pixel(&image, 3, 3, RED);
        assert_pixel(&image, 16, 8, BLACK);

        let image = renderer.render_scene_to_image(&Scene::default(), device_size(13, 7))?;
        assert_eq!(image.dimensions(), (13, 7));
        assert!(image.pixels().all(|pixel| pixel.0 == BLACK));
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn headless_renderer_reuses_target_for_same_size() -> anyhow::Result<()> {
        let mut renderer = WgpuHeadlessRenderer::new()?;
        let target_texture = |renderer: &WgpuHeadlessRenderer| {
            renderer
                .render_target
                .as_ref()
                .map(|target| target.texture.clone())
        };

        renderer.render_scene(&Scene::default(), device_size(16, 16))?;
        let first = target_texture(&renderer);
        assert!(first.is_some());

        renderer.render_scene(&Scene::default(), device_size(16, 16))?;
        assert_eq!(target_texture(&renderer), first);

        renderer.render_scene(&Scene::default(), device_size(16, 17))?;
        assert_ne!(target_texture(&renderer), first);
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn headless_renderer_rejects_invalid_sizes() -> anyhow::Result<()> {
        let mut renderer = WgpuHeadlessRenderer::new()?;
        let too_large = renderer.core.max_texture_size as i32 + 1;

        for size in [
            device_size(0, 8),
            device_size(8, 0),
            device_size(-1, 8),
            device_size(too_large, 8),
        ] {
            assert!(
                renderer.render_scene(&Scene::default(), size).is_err(),
                "{size:?} should be rejected"
            );
            assert!(
                renderer
                    .render_scene_to_image(&Scene::default(), size)
                    .is_err(),
                "{size:?} should be rejected"
            );
        }

        // Rejection must leave the renderer usable.
        let image = renderer.render_scene_to_image(&Scene::default(), device_size(4, 4))?;
        assert_eq!(image.dimensions(), (4, 4));
        Ok(())
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn vulkan_fast_path_preserves_priority_over_gl_device_types() {
        assert!(supports_vulkan_fast_path(wgpu::DeviceType::DiscreteGpu));
        assert!(supports_vulkan_fast_path(wgpu::DeviceType::IntegratedGpu));
        assert!(!supports_vulkan_fast_path(wgpu::DeviceType::Other));
        assert!(!supports_vulkan_fast_path(wgpu::DeviceType::VirtualGpu));
        assert!(!supports_vulkan_fast_path(wgpu::DeviceType::Cpu));
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn valid_device_override_preserves_combined_adapter_selection() {
        for device_id in ["1001", "0x1001", " 0X1001 "] {
            for reject_software in [false, true] {
                let mut attempts = Vec::new();
                create_with_backend_fallback(
                    Some(device_id),
                    reject_software,
                    |backends, reject_software| {
                        attempts.push((backends, reject_software));
                        Ok(())
                    },
                )
                .expect("explicit adapter selection should succeed");

                assert_eq!(
                    attempts,
                    [(wgpu::Backends::VULKAN | wgpu::Backends::GL, reject_software)]
                );
            }
        }
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn invalid_device_override_does_not_disable_vulkan_fast_path() {
        let mut attempts = Vec::new();
        create_with_backend_fallback(Some("invalid"), false, |backends, reject_software| {
            attempts.push((backends, reject_software));
            Ok(())
        })
        .expect("an invalid override should not prevent initialization");

        assert_eq!(attempts, [(wgpu::Backends::VULKAN, true)]);
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn hardware_vulkan_success_does_not_initialize_gl() {
        let mut attempts = Vec::new();
        let selected = create_with_backend_fallback(None, false, |backends, reject_software| {
            attempts.push((backends, reject_software));
            Ok(wgpu::Backend::Vulkan)
        })
        .expect("hardware Vulkan should succeed");

        assert_eq!(selected, wgpu::Backend::Vulkan);
        assert_eq!(attempts, [(wgpu::Backends::VULKAN, true)]);
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn failed_vulkan_surface_uses_combined_backend_fallback() {
        let mut attempts = Vec::new();
        let selected = create_with_backend_fallback(None, false, |backends, reject_software| {
            attempts.push((backends, reject_software));
            if backends == wgpu::Backends::VULKAN {
                anyhow::bail!("Vulkan surface configuration failed");
            }
            Ok(wgpu::Backend::Gl)
        })
        .expect("OpenGL should remain available");

        assert_eq!(selected, wgpu::Backend::Gl);
        assert_eq!(
            attempts,
            [
                (wgpu::Backends::VULKAN, true),
                (wgpu::Backends::VULKAN | wgpu::Backends::GL, false),
            ]
        );
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn software_vulkan_remains_available_at_startup() {
        let mut attempts = Vec::new();
        let selected = create_with_backend_fallback(None, false, |backends, reject_software| {
            attempts.push((backends, reject_software));
            if reject_software {
                anyhow::bail!("Only a CPU adapter is available");
            }
            Ok(wgpu::Backend::Vulkan)
        })
        .expect("startup should retain software fallback");

        assert_eq!(selected, wgpu::Backend::Vulkan);
        assert_eq!(attempts.len(), 2);
        assert!(attempts[1].0.contains(wgpu::Backends::VULKAN));
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn recovery_rejects_software_on_both_attempts() {
        let mut attempts = Vec::new();
        let result = create_with_backend_fallback(None, true, |backends, reject_software| {
            attempts.push((backends, reject_software));
            if reject_software {
                anyhow::bail!("Hardware GPU has not recovered");
            }
            Ok(())
        });

        assert_eq!(
            result
                .expect_err("recovery should wait for hardware")
                .to_string(),
            "Hardware GPU has not recovered"
        );
        assert_eq!(
            attempts,
            [
                (wgpu::Backends::VULKAN, true),
                (wgpu::Backends::VULKAN | wgpu::Backends::GL, true),
            ]
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    mod shader_layers {
        use super::*;
        use zpui::{PlatformHeadlessRenderer, ShaderLayer, hsla};

        const SWAP_OR_TINT: &str = r#"
@group(0) @binding(0) var content: texture_2d<f32>;
@group(0) @binding(1) var<uniform> tint: vec4<f32>;
@group(0) @binding(2) var content_sampler: sampler;

@fragment
fn main(@location(0) position: vec2<f32>) -> @location(0) vec4<f32> {
    let uv = position / vec2<f32>(textureDimensions(content));
    let color = textureSample(content, content_sampler, uv);
    if position.x < 8.0 {
        return tint;
    }
    return color.bgra;
}
"#;

        fn solid_quad(bounds: Bounds<ScaledPixels>, color: Hsla) -> Quad {
            Quad {
                order: 0,
                border_style: BorderStyle::Solid,
                bounds,
                content_mask: ContentMask { bounds },
                background: color.into(),
                border_color: color,
                corner_radii: Corners::default(),
                border_widths: Edges::default(),
                corner_smoothing: 2.0,
                pad: [0; 3],
            }
        }

        fn bounds(x: f32, y: f32, width: f32, height: f32) -> Bounds<ScaledPixels> {
            Bounds {
                origin: Point {
                    x: x.into(),
                    y: y.into(),
                },
                size: Size {
                    width: width.into(),
                    height: height.into(),
                },
            }
        }

        fn red_layer(shader: &str, inner: Option<ShaderLayer>) -> ShaderLayer {
            let layer_bounds = bounds(8.0, 8.0, 16.0, 16.0);
            let mut scene = Scene::default();
            scene.insert_primitive(solid_quad(layer_bounds, hsla(0.0, 1.0, 0.5, 1.0)));
            if let Some(inner) = inner {
                scene.insert_primitive(inner);
            }
            scene.finish();
            let tint: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
            ShaderLayer {
                order: 0,
                bounds: layer_bounds,
                content_mask: ContentMask {
                    bounds: layer_bounds,
                },
                shader: CustomShader::new(shader.to_owned()),
                uniforms: bytemuck::cast_slice(&tint).into(),
                scene: Rc::new(scene),
            }
        }

        fn render(layer: ShaderLayer) -> anyhow::Result<image::RgbaImage> {
            let mut renderer = WgpuHeadlessRenderer::new()?;
            let mut scene = Scene::default();
            scene.insert_primitive(solid_quad(
                bounds(0.0, 0.0, 32.0, 32.0),
                hsla(0.0, 0.0, 0.0, 1.0),
            ));
            scene.insert_primitive(layer);
            scene.finish();
            renderer.render_scene_to_image(
                &scene,
                Size {
                    width: DevicePixels(32),
                    height: DevicePixels(32),
                },
            )
        }

        #[test]
        fn shader_layers_run_custom_shaders_over_their_content() -> anyhow::Result<()> {
            let image = render(red_layer(SWAP_OR_TINT, None))?;
            assert_eq!(image.get_pixel(4, 12).0, [0, 0, 0, 255]);
            assert_eq!(image.get_pixel(10, 12).0, [0, 255, 0, 255]);
            assert_eq!(image.get_pixel(20, 12).0, [0, 0, 255, 255]);
            assert_eq!(image.get_pixel(28, 12).0, [0, 0, 0, 255]);
            Ok(())
        }

        #[test]
        fn nested_shader_layers_feed_the_outer_layer() -> anyhow::Result<()> {
            let inner = red_layer(SWAP_OR_TINT, None);
            let image = render(red_layer(SWAP_OR_TINT, Some(inner)))?;
            assert_eq!(image.get_pixel(10, 12).0, [0, 255, 0, 255]);
            assert_eq!(image.get_pixel(20, 12).0, [255, 0, 0, 255]);
            Ok(())
        }

        #[test]
        fn invalid_custom_shaders_paint_the_layer_unchanged() -> anyhow::Result<()> {
            for shader in [
                "not wgsl",
                "@fragment fn main(@location(1) x: vec4<f32>) -> @location(0) vec4<f32> { return x; }",
            ] {
                let image = render(red_layer(shader, None))?;
                assert_eq!(image.get_pixel(10, 12).0, [255, 0, 0, 255]);
                assert_eq!(image.get_pixel(20, 12).0, [255, 0, 0, 255]);
                assert_eq!(image.get_pixel(28, 12).0, [0, 0, 0, 255]);
            }
            Ok(())
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn glass_lenses_its_backdrop() -> anyhow::Result<()> {
        use zpui::PlatformHeadlessRenderer;
        let mut renderer = WgpuHeadlessRenderer::new()?;
        zpui::check_glass_rendering(|scene| {
            renderer.render_scene_to_image(
                scene,
                Size {
                    width: DevicePixels(64),
                    height: DevicePixels(64),
                },
            )
        })
    }

    #[test]
    fn glass_shader_is_valid_wgsl() {
        validate_wgsl(zpui::GLASS_SHADER, naga::valid::Capabilities::empty());
    }

    #[test]
    fn webgl_shader_is_valid_wgsl_without_storage_buffers() {
        assert!(!WEBGL_SHADERS.contains("var<storage"));
        validate_wgsl(WEBGL_SHADERS, naga::valid::Capabilities::empty());
    }

    #[test]
    fn storage_buffer_shader_is_valid_wgsl() {
        validate_wgsl(STORAGE_BUFFER_SHADERS, naga::valid::Capabilities::empty());
    }

    #[test]
    fn subpixel_shader_is_valid_wgsl() {
        validate_wgsl(
            SUBPIXEL_SHADERS,
            naga::valid::Capabilities::DUAL_SOURCE_BLENDING,
        );
    }

    fn validate_wgsl(source: &str, capabilities: naga::valid::Capabilities) {
        let module = naga::front::wgsl::parse_str(source).expect("shader should parse");
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), capabilities)
            .validate(&module)
            .expect("shader should validate");
    }

    #[test]
    fn webgl_record_sizes_match_shader_word_strides() {
        assert_eq!(std::mem::size_of::<Quad>(), 44 * 4);
        assert_eq!(std::mem::size_of::<Shadow>(), 28 * 4);
        assert_eq!(std::mem::size_of::<PathRasterizationVertex>(), 26 * 4);
        assert_eq!(std::mem::size_of::<PathSprite>(), 4 * 4);
        assert_eq!(std::mem::size_of::<Underline>(), 16 * 4);
        assert_eq!(std::mem::size_of::<MonochromeSprite>(), 28 * 4);
        assert_eq!(std::mem::size_of::<SubpixelSprite>(), 28 * 4);
        assert_eq!(std::mem::size_of::<PolychromeSprite>(), 26 * 4);
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        assert_eq!(std::mem::size_of::<SurfaceParams>(), 14 * 4);
    }

    #[test]
    fn webgl_quad_layout_matches_fixed_decoder() {
        let quad = Quad {
            order: 41,
            border_style: BorderStyle::Dashed,
            bounds: Bounds {
                origin: Point {
                    x: 1.0.into(),
                    y: 2.0.into(),
                },
                size: Size {
                    width: 3.0.into(),
                    height: 4.0.into(),
                },
            },
            content_mask: ContentMask {
                bounds: Bounds {
                    origin: Point {
                        x: 5.0.into(),
                        y: 6.0.into(),
                    },
                    size: Size {
                        width: 7.0.into(),
                        height: 8.0.into(),
                    },
                },
            },
            background: linear_gradient(
                11.0,
                linear_color_stop(
                    Hsla {
                        h: 12.0,
                        s: 13.0,
                        l: 14.0,
                        a: 15.0,
                    },
                    16.0,
                ),
                linear_color_stop(
                    Hsla {
                        h: 17.0,
                        s: 18.0,
                        l: 19.0,
                        a: 20.0,
                    },
                    21.0,
                ),
            )
            .color_space(ColorSpace::Oklab),
            border_color: Hsla {
                h: 22.0,
                s: 23.0,
                l: 24.0,
                a: 25.0,
            },
            corner_radii: Corners {
                top_left: 26.0.into(),
                top_right: 27.0.into(),
                bottom_right: 28.0.into(),
                bottom_left: 29.0.into(),
            },
            border_widths: Edges {
                top: 30.0.into(),
                right: 31.0.into(),
                bottom: 32.0.into(),
                left: 33.0.into(),
            },
            corner_smoothing: 34.0,
            pad: [35, 36, 37],
        };

        let (bytes, _) = instance_part(std::slice::from_ref(&quad));
        let words: &[u32] = bytemuck::cast_slice(bytes);
        assert_eq!(
            words,
            &[
                41,
                1,
                1.0_f32.to_bits(),
                2.0_f32.to_bits(),
                3.0_f32.to_bits(),
                4.0_f32.to_bits(),
                5.0_f32.to_bits(),
                6.0_f32.to_bits(),
                7.0_f32.to_bits(),
                8.0_f32.to_bits(),
                1,
                1,
                0,
                0,
                0,
                0,
                11.0_f32.to_bits(),
                12.0_f32.to_bits(),
                13.0_f32.to_bits(),
                14.0_f32.to_bits(),
                15.0_f32.to_bits(),
                16.0_f32.to_bits(),
                17.0_f32.to_bits(),
                18.0_f32.to_bits(),
                19.0_f32.to_bits(),
                20.0_f32.to_bits(),
                21.0_f32.to_bits(),
                0,
                22.0_f32.to_bits(),
                23.0_f32.to_bits(),
                24.0_f32.to_bits(),
                25.0_f32.to_bits(),
                26.0_f32.to_bits(),
                27.0_f32.to_bits(),
                28.0_f32.to_bits(),
                29.0_f32.to_bits(),
                30.0_f32.to_bits(),
                31.0_f32.to_bits(),
                32.0_f32.to_bits(),
                33.0_f32.to_bits(),
                34.0_f32.to_bits(),
                35,
                36,
                37,
            ]
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    mod instance_upload {
        use super::*;
        use std::borrow::Cow;
        use std::time::{Duration, Instant};
        use zpui::{
            AtlasKey, AtlasTile, DevicePixels, PlatformAtlas, PlatformHeadlessRenderer,
            RenderSvgParams, Scene, TransformationMatrix, hsla,
        };

        const COLUMNS: usize = 45;
        const ROWS: usize = 60;
        const CELL: f32 = 26.0;

        fn rect(x: f32, y: f32, width: f32, height: f32) -> Bounds<ScaledPixels> {
            Bounds {
                origin: Point {
                    x: x.into(),
                    y: y.into(),
                },
                size: Size {
                    width: width.into(),
                    height: height.into(),
                },
            }
        }

        fn page(
            scene: &mut Scene,
            left: f32,
            width: f32,
            height: f32,
            tile: &AtlasTile,
            extra: usize,
        ) {
            let page = rect(left, 0.0, width, height);
            let mask = ContentMask { bounds: page };
            let color = hsla(0.6, 0.2, 0.5, 1.0);
            scene.insert_primitive(Shadow {
                order: 0,
                blur_radius: 96.0.into(),
                bounds: page,
                corner_radii: Corners::all(24.0.into()),
                content_mask: mask,
                color: hsla(0.6, 0.8, 0.6, 0.06),
                element_bounds: page,
                element_corner_radii: Corners::all(24.0.into()),
                inset: 1,
                corner_smoothing: 4.0,
            });
            for row in 0..ROWS + extra {
                let y = row as f32 * CELL * 1.6;
                let line = rect(left, y, width, CELL * 1.6);
                scene.insert_primitive(Quad {
                    order: 0,
                    border_style: BorderStyle::Solid,
                    bounds: line,
                    content_mask: mask,
                    background: hsla(0.0, 0.0, 0.1 + row as f32 * 0.001, 1.0).into(),
                    border_color: color,
                    corner_radii: Corners::default(),
                    border_widths: Edges::default(),
                    corner_smoothing: 2.0,
                    pad: [0; 3],
                });
                if row % 9 == 0 {
                    scene.insert_primitive(Underline {
                        order: 0,
                        pad: 0,
                        bounds: rect(left, y + CELL * 1.4, width / 3.0, 2.0),
                        content_mask: mask,
                        color,
                        thickness: 2.0.into(),
                        wavy: false.into(),
                    });
                }
                for column in 0..COLUMNS {
                    scene.insert_primitive(MonochromeSprite {
                        order: 0,
                        pad: 0,
                        bounds: rect(left + column as f32 * CELL, y, CELL, CELL * 1.4),
                        content_mask: mask,
                        color,
                        tile: *tile,
                        transformation: TransformationMatrix::unit(),
                    });
                }
            }
        }

        /// One frame of a page swipe on a 1179x2556 phone: two terminal pages
        /// of 45x60 glyphs side by side, offset by the swipe.
        fn swipe_frame(frame: usize, tile: &AtlasTile, changing: bool) -> Scene {
            let (width, height) = (1179.0, 2556.0);
            let offset = (frame % 60) as f32 / 60.0 * width;
            let extra = if changing { frame % 3 } else { 0 };
            let mut scene = Scene::default();
            page(&mut scene, -offset, width, height, tile, extra);
            page(&mut scene, width - offset, width, height, tile, 0);
            scene.finish();
            scene
        }

        fn glyph_tile(renderer: &WgpuHeadlessRenderer) -> anyhow::Result<AtlasTile> {
            let size = Size {
                width: DevicePixels(8),
                height: DevicePixels(8),
            };
            renderer
                .core
                .atlas
                .get_or_insert_with(
                    AtlasKey::Svg(RenderSvgParams {
                        path: "glyph".into(),
                        size,
                    }),
                    &mut || Ok(Some((size, Cow::Owned(vec![255; 64])))),
                )?
                .ok_or_else(|| anyhow::anyhow!("no glyph tile"))
        }

        /// Quads come first in the instance buffer, so the underline and the
        /// glyph sit behind a varying number of quads and must be found by
        /// their first instance, including in the buffer a grow replaced.
        #[test]
        fn each_primitive_kind_draws_its_own_instances() -> anyhow::Result<()> {
            let mut renderer = WgpuHeadlessRenderer::new()?;
            let tile = glyph_tile(&renderer)?;
            let red = hsla(0.0, 1.0, 0.5, 1.0);
            let green = hsla(1.0 / 3.0, 1.0, 0.5, 1.0);
            let blue = hsla(2.0 / 3.0, 1.0, 0.5, 1.0);
            for hidden_quads in [0, 7, 3] {
                if hidden_quads == 3 {
                    renderer.core.instance_data_capacity = 16;
                }
                let mut scene = Scene::default();
                let screen = ContentMask {
                    bounds: rect(0.0, 0.0, 48.0, 16.0),
                };
                for ix in 0..=hidden_quads {
                    let bounds = if ix == hidden_quads {
                        rect(0.0, 0.0, 16.0, 16.0)
                    } else {
                        rect(100.0, 0.0, 16.0, 16.0)
                    };
                    scene.insert_primitive(Quad {
                        order: 0,
                        border_style: BorderStyle::Solid,
                        bounds,
                        content_mask: ContentMask { bounds },
                        background: red.into(),
                        border_color: red,
                        corner_radii: Corners::default(),
                        border_widths: Edges::default(),
                        corner_smoothing: 2.0,
                        pad: [0; 3],
                    });
                }
                scene.insert_primitive(Underline {
                    order: 0,
                    pad: 0,
                    bounds: rect(16.0, 0.0, 16.0, 16.0),
                    content_mask: screen,
                    color: green,
                    thickness: 16.0.into(),
                    wavy: false.into(),
                });
                scene.insert_primitive(MonochromeSprite {
                    order: 0,
                    pad: 0,
                    bounds: rect(32.0, 0.0, 16.0, 16.0),
                    content_mask: screen,
                    color: blue,
                    tile,
                    transformation: TransformationMatrix::unit(),
                });
                scene.finish();

                let image = renderer.render_scene_to_image(
                    &scene,
                    Size {
                        width: DevicePixels(48),
                        height: DevicePixels(16),
                    },
                )?;
                for (x, expected) in [
                    (8, [255, 0, 0, 255]),
                    (24, [0, 255, 0, 255]),
                    (40, [0, 0, 255, 255]),
                ] {
                    let actual = image.get_pixel(x, 8).0;
                    assert!(
                        actual
                            .iter()
                            .zip(expected)
                            .all(|(actual, expected)| actual.abs_diff(expected) <= 3),
                        "pixel ({x}, 8) behind {hidden_quads} hidden quads was {actual:?}, \
                         expected {expected:?}"
                    );
                }
            }
            assert!(renderer.core.instance_data_capacity > 16);
            Ok(())
        }

        fn percentile(sorted: &[Duration], fraction: f64) -> Duration {
            sorted[((sorted.len() - 1) as f64 * fraction).round() as usize]
        }

        #[test]
        #[ignore = "benchmark: cargo test -p zpui-wgpu --release bench_swipe_frames -- --ignored --nocapture"]
        fn bench_swipe_frames() -> anyhow::Result<()> {
            let mut renderer = WgpuHeadlessRenderer::new()?;
            let tile = glyph_tile(&renderer)?;
            let size = Size {
                width: DevicePixels(1179),
                height: DevicePixels(2556),
            };
            for changing in [false, true] {
                let mut times = Vec::new();
                for frame in 0..660 {
                    let scene = swipe_frame(frame, &tile, changing);
                    let start = Instant::now();
                    renderer.render(&scene, size)?;
                    let elapsed = start.elapsed();
                    renderer
                        .context
                        .device
                        .poll(wgpu::PollType::wait_indefinitely())?;
                    if frame >= 60 {
                        times.push(elapsed);
                    }
                }
                times.sort_unstable();
                let mean = times.iter().sum::<Duration>() / times.len() as u32;
                println!(
                    "{} counts: {} frames, CPU per frame mean {:.1} us, p50 {:.1} us, p95 {:.1} us",
                    if changing { "changing" } else { "stable" },
                    times.len(),
                    mean.as_secs_f64() * 1e6,
                    percentile(&times, 0.5).as_secs_f64() * 1e6,
                    percentile(&times, 0.95).as_secs_f64() * 1e6,
                );
            }
            Ok(())
        }
    }
}
