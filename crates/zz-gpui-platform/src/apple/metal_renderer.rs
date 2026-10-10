use crate::apple::metal_atlas::MetalAtlas;
use crate::apple::metal_glass::{MetalGlass, scene_has_glass};
use anyhow::{Context as _, Result};
use block2::RcBlock;
use cocoa::{
    base::{NO, YES},
    foundation::{NSSize, NSUInteger},
    quartzcore::AutoresizingMask,
};
#[cfg(any(test, feature = "bench-support", feature = "test-support"))]
use image::RgbaImage;
use objc2::runtime::AnyObject;
use zz_gpui::{
    AtlasTextureId, Background, Bounds, ContentMask, CustomShader, DevicePixels, PaintSurface,
    Path, Point, PrimitiveBatch, ScaledPixels, Scene, ShaderLayer, Size, point, size,
};

use core_foundation::base::TCFType;
use core_video::{
    metal_texture::CVMetalTextureGetTexture,
    metal_texture_cache::CVMetalTextureCache,
    pixel_buffer::{kCVPixelFormatType_32BGRA, kCVPixelFormatType_420YpCbCr8BiPlanarFullRange},
};
use foreign_types::{ForeignType, ForeignTypeRef};
use metal::{
    CAMetalLayer, CommandQueue, MTLGPUFamily, MTLPixelFormat, MTLResourceOptions, NSRange,
};
use objc::{self, msg_send, sel, sel_impl};
use parking_lot::Mutex;

use std::{
    cell::Cell, collections::HashMap, ffi::c_void, mem, mem::MaybeUninit, ops::Range, ptr, slice,
    sync::Arc,
};

// Exported to metal
pub(crate) type PointF = zz_gpui::Point<f32>;

#[cfg(not(feature = "runtime_shaders"))]
const SHADERS_METALLIB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/shaders.metallib"));
#[cfg(feature = "runtime_shaders")]
const SHADERS_SOURCE_FILE: &str = include_str!(concat!(env!("OUT_DIR"), "/stitched_shaders.metal"));
// Use 4x MSAA, all devices support it.
// https://developer.apple.com/documentation/metal/mtldevice/1433355-supportstexturesamplecount
const PATH_SAMPLE_COUNT: u32 = 4;
/// Metal requires the offset a buffer is bound at to be 256-byte aligned.
const INSTANCE_BUFFER_ALIGNMENT: usize = 256;
const MAX_INSTANCE_BUFFER_SIZE: usize = 256 * 1024 * 1024;

pub type Context = Arc<Mutex<InstanceBufferPool>>;
pub type Renderer = MetalRenderer;

pub unsafe fn new_renderer(
    context: self::Context,
    _native_window: *mut c_void,
    _native_view: *mut c_void,
    _bounds: zz_gpui::Size<f32>,
    transparent: bool,
) -> Renderer {
    MetalRenderer::new(context, transparent)
}

pub struct InstanceBufferPool {
    buffer_size: usize,
    buffers: Vec<metal::Buffer>,
}

impl Default for InstanceBufferPool {
    fn default() -> Self {
        Self {
            buffer_size: 2 * 1024 * 1024,
            buffers: Vec::new(),
        }
    }
}

pub(crate) struct InstanceBuffer {
    metal_buffer: metal::Buffer,
    size: usize,
}

impl InstanceBufferPool {
    pub(crate) fn reset(&mut self, buffer_size: usize) {
        self.buffer_size = buffer_size;
        self.buffers.clear();
    }

    pub(crate) fn acquire(
        &mut self,
        device: &metal::Device,
        unified_memory: bool,
    ) -> InstanceBuffer {
        let buffer = self.buffers.pop().unwrap_or_else(|| {
            let options = if unified_memory {
                MTLResourceOptions::StorageModeShared
                    // Buffers are write only which can benefit from the combined cache
                    // https://developer.apple.com/documentation/metal/mtlresourceoptions/cpucachemodewritecombined
                    | MTLResourceOptions::CPUCacheModeWriteCombined
            } else {
                MTLResourceOptions::StorageModeManaged
            };

            device.new_buffer(self.buffer_size as u64, options)
        });
        InstanceBuffer {
            metal_buffer: buffer,
            size: self.buffer_size,
        }
    }

    pub(crate) fn release(&mut self, buffer: InstanceBuffer) {
        if buffer.size == self.buffer_size {
            self.buffers.push(buffer.metal_buffer)
        }
    }
}

pub struct MetalRenderer {
    device: metal::Device,
    layer: Option<metal::MetalLayer>,
    is_apple_gpu: bool,
    is_unified_memory: bool,
    presents_with_transaction: bool,
    underlay_active: bool,
    /// For headless rendering, tracks whether output should be opaque
    opaque: bool,
    command_queue: CommandQueue,
    paths_rasterization_pipeline_state: metal::RenderPipelineState,
    path_sprites_pipeline_state: metal::RenderPipelineState,
    shadows_pipeline_state: metal::RenderPipelineState,
    quads_pipeline_state: metal::RenderPipelineState,
    underlines_pipeline_state: metal::RenderPipelineState,
    monochrome_sprites_pipeline_state: metal::RenderPipelineState,
    polychrome_sprites_pipeline_state: metal::RenderPipelineState,
    surfaces_pipeline_state: metal::RenderPipelineState,
    bgra_surfaces_pipeline_state: metal::RenderPipelineState,
    hole_surfaces_pipeline_state: metal::RenderPipelineState,
    shader_layer_vertex: metal::Function,
    shader_layer_sampler: metal::SamplerState,
    shader_layer_pipelines: HashMap<u64, Option<ShaderLayerPipeline>>,
    shader_layer_targets: Vec<metal::Texture>,
    shader_layer_inputs: Vec<metal::Texture>,
    shader_layers_drawn: bool,
    /// Built when a scene first paints glass; `Err` once building failed.
    glass: Option<Result<MetalGlass, ()>>,
    unit_vertices: metal::Buffer,
    #[allow(clippy::arc_with_non_send_sync)]
    instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>,
    sprite_atlas: Arc<MetalAtlas>,
    core_video_texture_cache: core_video::metal_texture_cache::CVMetalTextureCache,
    path_intermediate_texture: Option<metal::Texture>,
    path_intermediate_msaa_texture: Option<metal::Texture>,
    path_sample_count: u32,
    /// Offscreen render target reused across `render_scene` calls when
    /// rendering headlessly without reading pixels back.
    #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
    headless_render_target: Option<metal::Texture>,
}

#[repr(C)]
pub struct PathRasterizationVertex {
    pub xy_position: Point<ScaledPixels>,
    pub st_position: Point<f32>,
    pub color: Background,
    pub bounds: Bounds<ScaledPixels>,
}

impl MetalRenderer {
    /// Creates a new MetalRenderer with a CAMetalLayer for window-based rendering.
    pub fn new(instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>, transparent: bool) -> Self {
        let device = Self::create_device();

        let layer = metal::MetalLayer::new();
        layer.set_device(&device);
        layer.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        // Support direct-to-display rendering if the window is not transparent
        // https://developer.apple.com/documentation/metal/managing-your-game-window-for-metal-in-macos
        layer.set_opaque(!transparent);
        layer.set_maximum_drawable_count(3);
        // Allow texture reading for visual tests (captures screenshots without ScreenCaptureKit)
        #[cfg(any(test, feature = "test-support"))]
        layer.set_framebuffer_only(false);
        unsafe {
            let _: () = msg_send![&*layer, setAllowsNextDrawableTimeout: NO];
            let _: () = msg_send![&*layer, setNeedsDisplayOnBoundsChange: YES];
            let _: () = msg_send![
                &*layer,
                setAutoresizingMask: AutoresizingMask::WIDTH_SIZABLE
                    | AutoresizingMask::HEIGHT_SIZABLE
            ];
        }

        Self::new_internal(device, Some(layer), !transparent, instance_buffer_pool)
    }

    /// Creates a new headless MetalRenderer for offscreen rendering without a window.
    ///
    /// This renderer can render scenes to images without requiring a CAMetalLayer,
    /// window, or AppKit. Use `render_scene_to_image()` to render scenes.
    #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
    pub fn new_headless(instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>) -> Self {
        let device = Self::create_device();
        Self::new_internal(device, None, true, instance_buffer_pool)
    }

    fn create_device() -> metal::Device {
        // Prefer low‐power integrated GPUs on Intel Mac. On Apple
        // Silicon, there is only ever one GPU, so this is equivalent to
        // `metal::Device::system_default()`.
        if let Some(d) = metal::Device::all()
            .into_iter()
            .min_by_key(|d| (d.is_removable(), !d.is_low_power()))
        {
            d
        } else {
            // For some reason `all()` can return an empty list, see https://github.com/zed-industries/zed/issues/37689
            // In that case, we fall back to the system default device.
            log::error!(
                "Unable to enumerate Metal devices; attempting to use system default device"
            );
            metal::Device::system_default().unwrap_or_else(|| {
                log::error!("unable to access a compatible graphics device");
                std::process::exit(1);
            })
        }
    }

    fn new_internal(
        device: metal::Device,
        layer: Option<metal::MetalLayer>,
        opaque: bool,
        instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>,
    ) -> Self {
        #[cfg(feature = "runtime_shaders")]
        let library = device
            .new_library_with_source(&SHADERS_SOURCE_FILE, &metal::CompileOptions::new())
            .expect("error building metal library");
        #[cfg(not(feature = "runtime_shaders"))]
        let library = device
            .new_library_with_data(SHADERS_METALLIB)
            .expect("error building metal library");

        fn to_float2_bits(point: PointF) -> u64 {
            let mut output = point.y.to_bits() as u64;
            output <<= 32;
            output |= point.x.to_bits() as u64;
            output
        }

        // Shared memory can be used only if CPU and GPU share the same memory space.
        // https://developer.apple.com/documentation/metal/setting-resource-storage-modes
        let is_unified_memory = device.has_unified_memory();
        // Apple GPU families support memoryless textures, which can significantly reduce
        // memory usage by keeping render targets in on-chip tile memory instead of
        // allocating backing store in system memory.
        // https://developer.apple.com/documentation/metal/mtlgpufamily
        let is_apple_gpu = device.supports_family(MTLGPUFamily::Apple1);

        let unit_vertices = [
            to_float2_bits(point(0., 0.)),
            to_float2_bits(point(1., 0.)),
            to_float2_bits(point(0., 1.)),
            to_float2_bits(point(0., 1.)),
            to_float2_bits(point(1., 0.)),
            to_float2_bits(point(1., 1.)),
        ];
        let unit_vertices = device.new_buffer_with_data(
            unit_vertices.as_ptr() as *const c_void,
            mem::size_of_val(&unit_vertices) as u64,
            if is_unified_memory {
                MTLResourceOptions::StorageModeShared
                    | MTLResourceOptions::CPUCacheModeWriteCombined
            } else {
                MTLResourceOptions::StorageModeManaged
            },
        );

        let paths_rasterization_pipeline_state = build_path_rasterization_pipeline_state(
            &device,
            &library,
            "paths_rasterization",
            "path_rasterization_vertex",
            "path_rasterization_fragment",
            MTLPixelFormat::BGRA8Unorm,
            PATH_SAMPLE_COUNT,
        );
        let path_sprites_pipeline_state = build_premultiplied_pipeline_state(
            &device,
            &library,
            "path_sprites",
            "path_sprite_vertex",
            "path_sprite_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );
        let shadows_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "shadows",
            "shadow_vertex",
            "shadow_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );
        let quads_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "quads",
            "quad_vertex",
            "quad_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );
        let underlines_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "underlines",
            "underline_vertex",
            "underline_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );
        let monochrome_sprites_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "monochrome_sprites",
            "monochrome_sprite_vertex",
            "monochrome_sprite_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );
        let polychrome_sprites_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "polychrome_sprites",
            "polychrome_sprite_vertex",
            "polychrome_sprite_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );
        let surfaces_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "surfaces",
            "surface_vertex",
            "surface_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );
        let bgra_surfaces_pipeline_state = build_premultiplied_pipeline_state(
            &device,
            &library,
            "bgra_surfaces",
            "surface_vertex",
            "surface_bgra_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );
        let hole_surfaces_pipeline_state = build_hole_pipeline_state(
            &device,
            &library,
            "hole_surfaces",
            "surface_vertex",
            "surface_hole_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );

        let shader_layer_vertex = library
            .get_function("shader_layer_vertex", None)
            .expect("error locating vertex function");
        let sampler_descriptor = metal::SamplerDescriptor::new();
        sampler_descriptor.set_min_filter(metal::MTLSamplerMinMagFilter::Linear);
        sampler_descriptor.set_mag_filter(metal::MTLSamplerMinMagFilter::Linear);
        sampler_descriptor.set_address_mode_s(metal::MTLSamplerAddressMode::ClampToEdge);
        sampler_descriptor.set_address_mode_t(metal::MTLSamplerAddressMode::ClampToEdge);
        let shader_layer_sampler = device.new_sampler(&sampler_descriptor);

        let command_queue = device.new_command_queue();
        let sprite_atlas = Arc::new(MetalAtlas::new(device.clone(), is_apple_gpu));
        let core_video_texture_cache =
            CVMetalTextureCache::new(None, device.clone(), None).unwrap();

        Self {
            device,
            layer,
            presents_with_transaction: false,
            underlay_active: false,
            is_apple_gpu,
            is_unified_memory,
            opaque,
            command_queue,
            paths_rasterization_pipeline_state,
            path_sprites_pipeline_state,
            shadows_pipeline_state,
            quads_pipeline_state,
            underlines_pipeline_state,
            monochrome_sprites_pipeline_state,
            polychrome_sprites_pipeline_state,
            surfaces_pipeline_state,
            bgra_surfaces_pipeline_state,
            hole_surfaces_pipeline_state,
            shader_layer_vertex,
            shader_layer_sampler,
            shader_layer_pipelines: HashMap::default(),
            shader_layer_targets: Vec::new(),
            shader_layer_inputs: Vec::new(),
            shader_layers_drawn: false,
            glass: None,
            unit_vertices,
            instance_buffer_pool,
            sprite_atlas,
            core_video_texture_cache,
            path_intermediate_texture: None,
            path_intermediate_msaa_texture: None,
            path_sample_count: PATH_SAMPLE_COUNT,
            #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
            headless_render_target: None,
        }
    }

    pub fn layer(&self) -> Option<&metal::MetalLayerRef> {
        self.layer.as_ref().map(|l| l.as_ref())
    }

    pub fn layer_ptr(&self) -> *mut CAMetalLayer {
        self.layer
            .as_ref()
            .map(|l| l.as_ptr())
            .unwrap_or(ptr::null_mut())
    }

    pub fn sprite_atlas(&self) -> &Arc<MetalAtlas> {
        &self.sprite_atlas
    }

    pub fn set_presents_with_transaction(&mut self, presents_with_transaction: bool) {
        self.presents_with_transaction = presents_with_transaction;
        if let Some(layer) = &self.layer {
            layer.set_presents_with_transaction(self.transactional_present());
        }
    }

    /// See `Window::set_underlay_active`.
    pub fn set_underlay_active(&mut self, active: bool) {
        if self.underlay_active == active {
            return;
        }
        self.underlay_active = active;
        if let Some(layer) = &self.layer {
            layer.set_opaque(self.layer_opaque());
            layer.set_presents_with_transaction(self.transactional_present());
        }
    }

    fn transactional_present(&self) -> bool {
        self.presents_with_transaction || self.underlay_active
    }

    /// Whether this renderer can draw glass, building its pipelines the
    /// first time it is asked. Windows paint a plain fill in its place when
    /// it cannot.
    pub fn supports_glass(&mut self) -> bool {
        let glass = self.glass.get_or_insert_with(|| {
            MetalGlass::new(&self.device)
                .inspect_err(|error| log::error!("glass is unavailable: {error:#}"))
                .map_err(drop)
        });
        glass.is_ok()
    }

    fn layer_opaque(&self) -> bool {
        self.opaque && !self.underlay_active
    }

    pub fn update_drawable_size(&mut self, size: Size<DevicePixels>) {
        if let Some(layer) = &self.layer {
            let ns_size = NSSize {
                width: size.width.0 as f64,
                height: size.height.0 as f64,
            };
            unsafe {
                let _: () = msg_send![
                    layer.as_ref(),
                    setDrawableSize: ns_size
                ];
            }
        }
        self.update_path_intermediate_textures(size);
    }

    fn update_path_intermediate_textures(&mut self, size: Size<DevicePixels>) {
        // We are uncertain when this happens, but sometimes size can be 0 here. Most likely before
        // the layout pass on window creation. Zero-sized texture creation causes SIGABRT.
        // https://github.com/zed-industries/zed/issues/36229
        if size.width.0 <= 0 || size.height.0 <= 0 {
            self.path_intermediate_texture = None;
            self.path_intermediate_msaa_texture = None;
            return;
        }

        let texture_descriptor = metal::TextureDescriptor::new();
        texture_descriptor.set_width(size.width.0 as u64);
        texture_descriptor.set_height(size.height.0 as u64);
        texture_descriptor.set_pixel_format(metal::MTLPixelFormat::BGRA8Unorm);
        texture_descriptor.set_storage_mode(metal::MTLStorageMode::Private);
        texture_descriptor
            .set_usage(metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead);
        self.path_intermediate_texture = Some(self.device.new_texture(&texture_descriptor));

        if self.path_sample_count > 1 {
            // https://developer.apple.com/documentation/metal/choosing-a-resource-storage-mode-for-apple-gpus
            // Rendering MSAA textures are done in a single pass, so we can use memory-less storage on Apple Silicon
            let storage_mode = if self.is_apple_gpu {
                metal::MTLStorageMode::Memoryless
            } else {
                metal::MTLStorageMode::Private
            };

            let msaa_descriptor = texture_descriptor;
            msaa_descriptor.set_texture_type(metal::MTLTextureType::D2Multisample);
            msaa_descriptor.set_storage_mode(storage_mode);
            msaa_descriptor.set_sample_count(self.path_sample_count as _);
            self.path_intermediate_msaa_texture = Some(self.device.new_texture(&msaa_descriptor));
        } else {
            self.path_intermediate_msaa_texture = None;
        }
    }

    pub fn update_transparency(&mut self, transparent: bool) {
        self.opaque = !transparent;
        if let Some(layer) = &self.layer {
            layer.set_opaque(self.layer_opaque());
        }
    }

    pub fn destroy(&self) {
        // nothing to do
    }

    pub fn draw(&mut self, scene: &Scene) {
        let layer = match &self.layer {
            Some(l) => l.clone(),
            None => {
                log::error!(
                    "draw() called on headless renderer - use render_scene_to_image() instead"
                );
                return;
            }
        };
        let viewport_size = layer.drawable_size();
        let viewport_size: Size<DevicePixels> = size(
            (viewport_size.width.ceil() as i32).into(),
            (viewport_size.height.ceil() as i32).into(),
        );
        let drawable = if let Some(drawable) = layer.next_drawable() {
            drawable
        } else {
            log::error!(
                "failed to retrieve next drawable, drawable size: {:?}",
                viewport_size
            );
            return;
        };

        let command_buffer = match self.render_frame(scene, drawable.texture(), viewport_size) {
            Ok(command_buffer) => command_buffer,
            Err(error) => {
                log::error!("failed to render: {error:#}");
                return;
            }
        };

        if self.transactional_present() {
            command_buffer.commit();
            command_buffer.wait_until_scheduled();
            drawable.present();
        } else {
            command_buffer.present_drawable(drawable);
            command_buffer.commit();
        }
    }

    fn render_frame(
        &mut self,
        scene: &Scene,
        texture: &metal::TextureRef,
        viewport_size: Size<DevicePixels>,
    ) -> Result<metal::CommandBuffer> {
        let mut writer = InstanceBufferWriter::new(
            &self.device,
            &self.instance_buffer_pool,
            self.is_unified_memory,
        );
        let instance_bindings = write_instances(scene, &mut writer).with_context(|| {
            format!(
                "scene too large: {} paths, {} shadows, {} quads, {} underlines, {} mono, {} poly, {} surfaces",
                scene.paths.len(),
                scene.shadows.len(),
                scene.quads.len(),
                scene.underlines.len(),
                scene.monochrome_sprites.len(),
                scene.polychrome_sprites.len(),
                scene.surfaces.len(),
            )
        })?;
        let atlas_frame = self.sprite_atlas.begin_frame();
        self.shader_layers_drawn = false;
        if scene_has_glass(scene) {
            self.supports_glass();
        }
        if let Some(Ok(glass)) = &mut self.glass {
            glass.begin_frame();
        }
        let command_buffer = self.draw_primitives_to_texture(
            scene,
            &instance_bindings,
            &mut writer,
            texture,
            viewport_size,
        )?;
        if !self.shader_layers_drawn {
            self.shader_layer_targets.clear();
            self.shader_layer_inputs.clear();
        }
        if let Some(Ok(glass)) = &mut self.glass {
            glass.end_frame();
        }

        let instance_buffer_pool = self.instance_buffer_pool.clone();
        let instance_buffer = Cell::new(Some(writer.finish()));
        let block = RcBlock::new(move |_: ptr::NonNull<AnyObject>| {
            if let Some(instance_buffer) = instance_buffer.take() {
                instance_buffer_pool.lock().release(instance_buffer);
            }
            atlas_frame.complete();
        });
        // SAFETY: Both pointee types are opaque views of the same Objective-C block pointer ABI.
        unsafe {
            command_buffer.add_completed_handler(&*RcBlock::as_ptr(&block).cast());
        }

        Ok(command_buffer)
    }

    /// Renders the scene to a texture and returns the pixel data as an RGBA image.
    /// This does not present the frame to screen - useful for visual testing
    /// where we want to capture what would be rendered without displaying it.
    ///
    /// Note: This requires a layer-backed renderer. For headless rendering,
    /// use `render_scene_to_image()` instead.
    #[cfg(any(test, feature = "test-support"))]
    pub fn render_to_image(&mut self, scene: &Scene) -> Result<RgbaImage> {
        let layer = self
            .layer
            .clone()
            .ok_or_else(|| anyhow::anyhow!("render_to_image requires a layer-backed renderer"))?;
        let viewport_size = layer.drawable_size();
        let viewport_size: Size<DevicePixels> = size(
            (viewport_size.width.ceil() as i32).into(),
            (viewport_size.height.ceil() as i32).into(),
        );
        let drawable = layer
            .next_drawable()
            .ok_or_else(|| anyhow::anyhow!("Failed to get drawable for render_to_image"))?;

        let command_buffer = self.render_frame(scene, drawable.texture(), viewport_size)?;

        // Commit and wait for completion without presenting
        command_buffer.commit();
        command_buffer.wait_until_completed();

        read_texture_to_image(drawable.texture())
    }

    /// Renders a scene to an image without requiring a window or CAMetalLayer.
    ///
    /// This is the primary method for headless rendering. It creates an offscreen
    /// texture, renders the scene to it, and returns the pixel data as an RGBA image.
    #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
    pub fn render_scene_to_image(
        &mut self,
        scene: &Scene,
        size: Size<DevicePixels>,
    ) -> Result<RgbaImage> {
        if size.width.0 <= 0 || size.height.0 <= 0 {
            anyhow::bail!("Invalid size for render_scene_to_image: {:?}", size);
        }

        // Headless callers do not have a Cocoa event-loop pool to release
        // autoreleased command buffers and render-pass descriptors.
        objc2::rc::autoreleasepool(|_| {
            // Update path intermediate textures for this size
            self.update_path_intermediate_textures(size);

            // Create an offscreen texture as render target
            let texture_descriptor = metal::TextureDescriptor::new();
            texture_descriptor.set_width(size.width.0 as u64);
            texture_descriptor.set_height(size.height.0 as u64);
            texture_descriptor.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
            texture_descriptor.set_usage(
                metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead,
            );
            texture_descriptor.set_storage_mode(metal::MTLStorageMode::Managed);
            let target_texture = self.device.new_texture(&texture_descriptor);

            let command_buffer = self.render_frame(scene, &target_texture, size)?;

            // On discrete GPUs (non-unified memory), Managed textures require an
            // explicit blit synchronize before the CPU can read back the rendered
            // data. Without this, get_bytes returns stale zeros.
            if !self.is_unified_memory {
                let blit = command_buffer.new_blit_command_encoder();
                blit.synchronize_resource(&target_texture);
                blit.end_encoding();
            }

            // Commit and wait for completion
            command_buffer.commit();
            command_buffer.wait_until_completed();

            read_texture_to_image(&target_texture)
        })
    }

    /// Renders a scene to a reused offscreen texture without reading pixels
    /// back or blocking on GPU completion.
    ///
    /// This mirrors the CPU cost of presenting a frame to a window (scene
    /// encoding, instance buffer writes, command submission) and is used by
    /// headless benchmark rendering, where the produced pixels are never
    /// inspected.
    #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
    pub fn render_scene(&mut self, scene: &Scene, size: Size<DevicePixels>) -> Result<()> {
        if size.width.0 <= 0 || size.height.0 <= 0 {
            anyhow::bail!("Invalid size for render_scene: {:?}", size);
        }

        objc2::rc::autoreleasepool(|_| {
            self.update_path_intermediate_textures(size);

            let needs_new_target = self.headless_render_target.as_ref().is_none_or(|texture| {
                texture.width() != size.width.0 as u64 || texture.height() != size.height.0 as u64
            });
            if needs_new_target {
                let texture_descriptor = metal::TextureDescriptor::new();
                texture_descriptor.set_width(size.width.0 as u64);
                texture_descriptor.set_height(size.height.0 as u64);
                texture_descriptor.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
                texture_descriptor.set_usage(
                    metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead,
                );
                texture_descriptor.set_storage_mode(metal::MTLStorageMode::Private);
                self.headless_render_target = Some(self.device.new_texture(&texture_descriptor));
            }
            let target_texture = self
                .headless_render_target
                .clone()
                .expect("just ensured the render target exists");

            let command_buffer = self.render_frame(scene, &target_texture, size)?;

            // Commit without waiting, mirroring presentation to a real window where
            // the CPU doesn't block on the GPU.
            command_buffer.commit();
            Ok(())
        })
    }

    fn draw_primitives_to_texture(
        &mut self,
        scene: &Scene,
        instance_bindings: &InstanceBindings,
        writer: &mut InstanceBufferWriter,
        texture: &metal::TextureRef,
        viewport_size: Size<DevicePixels>,
    ) -> Result<metal::CommandBuffer> {
        let command_queue = self.command_queue.clone();
        let command_buffer = command_queue.new_command_buffer();
        let alpha = if self.layer_opaque() { 1. } else { 0. };
        self.encode_scene(
            command_buffer,
            scene,
            instance_bindings,
            writer,
            texture,
            viewport_size,
            Some(metal::MTLClearColor::new(0., 0., 0., alpha)),
            0,
        )?;
        Ok(command_buffer.to_owned())
    }

    fn encode_scene(
        &mut self,
        command_buffer: &metal::CommandBufferRef,
        scene: &Scene,
        instance_bindings: &InstanceBindings,
        writer: &mut InstanceBufferWriter,
        texture: &metal::TextureRef,
        viewport_size: Size<DevicePixels>,
        clear_color: Option<metal::MTLClearColor>,
        depth: usize,
    ) -> Result<()> {
        let mut command_encoder =
            new_command_encoder_for_texture(command_buffer, texture, viewport_size, clear_color);

        for batch in scene.batches() {
            match batch {
                PrimitiveBatch::Shadows(range) => {
                    self.draw_shadows(range, instance_bindings, viewport_size, command_encoder)
                }
                PrimitiveBatch::Quads(range) => {
                    self.draw_quads(range, instance_bindings, viewport_size, command_encoder)
                }
                PrimitiveBatch::Paths(range) => {
                    let paths = &scene.paths[range];
                    command_encoder.end_encoding();

                    let did_draw = self.draw_paths_to_intermediate(
                        paths,
                        writer,
                        viewport_size,
                        command_buffer,
                    )?;

                    command_encoder = new_command_encoder_for_texture(
                        command_buffer,
                        texture,
                        viewport_size,
                        None,
                    );

                    if did_draw {
                        if let Err(error) = self.draw_paths_from_intermediate(
                            paths,
                            writer,
                            viewport_size,
                            command_encoder,
                        ) {
                            command_encoder.end_encoding();
                            return Err(error);
                        }
                    }
                }
                PrimitiveBatch::Underlines(range) => {
                    self.draw_underlines(range, instance_bindings, viewport_size, command_encoder)
                }
                PrimitiveBatch::MonochromeSprites { texture_id, range } => self
                    .draw_monochrome_sprites(
                        texture_id,
                        range,
                        instance_bindings,
                        viewport_size,
                        command_encoder,
                    ),
                PrimitiveBatch::PolychromeSprites { texture_id, range } => self
                    .draw_polychrome_sprites(
                        texture_id,
                        range,
                        instance_bindings,
                        viewport_size,
                        command_encoder,
                    ),
                PrimitiveBatch::Surfaces(range) => self.draw_surfaces(
                    &scene.surfaces[range.clone()],
                    range.start,
                    instance_bindings,
                    viewport_size,
                    command_encoder,
                ),
                PrimitiveBatch::ShaderLayers(range) => {
                    command_encoder.end_encoding();
                    for layer in &scene.shader_layers[range] {
                        self.draw_shader_layer(
                            layer,
                            command_buffer,
                            writer,
                            texture,
                            viewport_size,
                            depth,
                        )?;
                    }
                    command_encoder = new_command_encoder_for_texture(
                        command_buffer,
                        texture,
                        viewport_size,
                        None,
                    );
                }
                PrimitiveBatch::Glass(range) => {
                    let Some(Ok(glass)) = &mut self.glass else {
                        continue;
                    };
                    command_encoder.end_encoding();
                    let pending = glass.draw(
                        &self.device,
                        command_buffer,
                        &scene.glasses[range],
                        texture,
                        scene.window_corner_mask,
                    );
                    command_encoder = new_command_encoder_for_texture(
                        command_buffer,
                        texture,
                        viewport_size,
                        None,
                    );
                    glass.draw_pending(
                        command_encoder,
                        &pending,
                        Size {
                            width: viewport_size.width.0,
                            height: viewport_size.height.0,
                        },
                    );
                }
                PrimitiveBatch::SubpixelSprites { .. } => unreachable!(),
            }
        }

        command_encoder.end_encoding();

        Ok(())
    }

    fn draw_shader_layer(
        &mut self,
        layer: &ShaderLayer,
        command_buffer: &metal::CommandBufferRef,
        writer: &mut InstanceBufferWriter,
        texture: &metal::TextureRef,
        viewport_size: Size<DevicePixels>,
        depth: usize,
    ) -> Result<()> {
        let Some(region) = shader_layer_region(layer.bounds, viewport_size) else {
            return Ok(());
        };
        let instance_bindings = write_instances(&layer.scene, writer)?;
        let Some(pipeline) = self.shader_layer_pipeline(&layer.shader) else {
            return self.encode_scene(
                command_buffer,
                &layer.scene,
                &instance_bindings,
                writer,
                texture,
                viewport_size,
                None,
                depth + 1,
            );
        };
        self.shader_layers_drawn = true;

        let target = self.shader_layer_target(depth, viewport_size);
        self.encode_scene(
            command_buffer,
            &layer.scene,
            &instance_bindings,
            writer,
            &target,
            viewport_size,
            Some(metal::MTLClearColor::new(0., 0., 0., 0.)),
            depth + 1,
        )?;

        let input = self.shader_layer_input(region.size);
        let blit = command_buffer.new_blit_command_encoder();
        blit.copy_from_texture(
            &target,
            0,
            0,
            metal::MTLOrigin {
                x: region.origin.x.0 as u64,
                y: region.origin.y.0 as u64,
                z: 0,
            },
            metal::MTLSize {
                width: region.size.width.0 as u64,
                height: region.size.height.0 as u64,
                depth: 1,
            },
            &input,
            0,
            0,
            metal::MTLOrigin { x: 0, y: 0, z: 0 },
        );
        blit.end_encoding();

        let uniforms = if pipeline.uniform_size > 0 {
            let mut bytes = vec![0u8; pipeline.uniform_size];
            let len = layer.uniforms.len().min(bytes.len());
            bytes[..len].copy_from_slice(&layer.uniforms[..len]);
            Some(writer.write(&bytes)?)
        } else {
            None
        };

        let bounds = ShaderLayerBounds {
            bounds: Bounds {
                origin: point(
                    ScaledPixels(region.origin.x.0 as f32),
                    ScaledPixels(region.origin.y.0 as f32),
                ),
                size: size(
                    ScaledPixels(region.size.width.0 as f32),
                    ScaledPixels(region.size.height.0 as f32),
                ),
            },
            content_mask: layer.content_mask,
        };
        let command_encoder =
            new_command_encoder_for_texture(command_buffer, texture, viewport_size, None);
        command_encoder.set_render_pipeline_state(&pipeline.state);
        command_encoder.set_vertex_buffer(
            ShaderLayerInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_bytes(
            ShaderLayerInputIndex::Bounds as u64,
            mem::size_of_val(&bounds) as u64,
            &bounds as *const ShaderLayerBounds as *const _,
        );
        command_encoder.set_vertex_bytes(
            ShaderLayerInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_fragment_texture(0, Some(&input));
        command_encoder.set_fragment_sampler_state(0, Some(&self.shader_layer_sampler));
        if let Some(uniforms) = &uniforms {
            command_encoder.set_fragment_buffer(0, Some(&uniforms.buffer), uniforms.offset as u64);
        }
        command_encoder.draw_primitives(metal::MTLPrimitiveType::Triangle, 0, 6);
        command_encoder.end_encoding();
        Ok(())
    }

    fn shader_layer_pipeline(&mut self, shader: &CustomShader) -> Option<ShaderLayerPipeline> {
        if let Some(pipeline) = self.shader_layer_pipelines.get(&shader.id()) {
            return pipeline.clone();
        }
        let pipeline = build_shader_layer_pipeline(&self.device, &self.shader_layer_vertex, shader)
            .inspect_err(|error| log::error!("custom shader {shader:?} failed: {error:#}"))
            .ok();
        self.shader_layer_pipelines
            .insert(shader.id(), pipeline.clone());
        pipeline
    }

    fn shader_layer_target(&mut self, depth: usize, size: Size<DevicePixels>) -> metal::Texture {
        let stale = self
            .shader_layer_targets
            .first()
            .is_some_and(|texture| !texture_has_size(texture, size));
        if stale {
            self.shader_layer_targets.clear();
        }
        while self.shader_layer_targets.len() <= depth {
            let texture = self.new_shader_layer_texture(size, true);
            self.shader_layer_targets.push(texture);
        }
        self.shader_layer_targets[depth].clone()
    }

    fn shader_layer_input(&mut self, size: Size<DevicePixels>) -> metal::Texture {
        if let Some(texture) = self
            .shader_layer_inputs
            .iter()
            .find(|texture| texture_has_size(texture, size))
        {
            return texture.clone();
        }
        if self.shader_layer_inputs.len() >= MAX_SHADER_LAYER_INPUTS {
            self.shader_layer_inputs.clear();
        }
        let texture = self.new_shader_layer_texture(size, false);
        self.shader_layer_inputs.push(texture.clone());
        texture
    }

    fn new_shader_layer_texture(
        &self,
        size: Size<DevicePixels>,
        render_target: bool,
    ) -> metal::Texture {
        let descriptor = metal::TextureDescriptor::new();
        descriptor.set_width(size.width.0 as u64);
        descriptor.set_height(size.height.0 as u64);
        descriptor.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        descriptor.set_storage_mode(metal::MTLStorageMode::Private);
        descriptor.set_usage(if render_target {
            metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead
        } else {
            metal::MTLTextureUsage::ShaderRead
        });
        self.device.new_texture(&descriptor)
    }

    fn draw_paths_to_intermediate(
        &self,
        paths: &[Path<ScaledPixels>],
        writer: &mut InstanceBufferWriter,
        viewport_size: Size<DevicePixels>,
        command_buffer: &metal::CommandBufferRef,
    ) -> Result<bool> {
        if paths.is_empty() {
            return Ok(false);
        }
        let intermediate_texture = self
            .path_intermediate_texture
            .as_ref()
            .context("missing path intermediate texture")?;

        let mut vertices = Vec::new();
        for path in paths {
            vertices.extend(path.vertices.iter().map(|v| PathRasterizationVertex {
                xy_position: v.xy_position,
                st_position: v.st_position,
                color: path.color,
                bounds: path.bounds.intersect(&path.content_mask.bounds),
            }));
        }
        let vertex_instance_bindings = writer.write(&vertices)?;

        let render_pass_descriptor = metal::RenderPassDescriptor::new();
        let color_attachment = render_pass_descriptor
            .color_attachments()
            .object_at(0)
            .unwrap();
        color_attachment.set_load_action(metal::MTLLoadAction::Clear);
        color_attachment.set_clear_color(metal::MTLClearColor::new(0., 0., 0., 0.));

        if let Some(msaa_texture) = &self.path_intermediate_msaa_texture {
            color_attachment.set_texture(Some(msaa_texture));
            color_attachment.set_resolve_texture(Some(intermediate_texture));
            color_attachment.set_store_action(metal::MTLStoreAction::MultisampleResolve);
        } else {
            color_attachment.set_texture(Some(intermediate_texture));
            color_attachment.set_store_action(metal::MTLStoreAction::Store);
        }

        let command_encoder = command_buffer.new_render_command_encoder(render_pass_descriptor);
        command_encoder.set_render_pipeline_state(&self.paths_rasterization_pipeline_state);
        command_encoder.set_vertex_buffer(
            PathRasterizationInputIndex::Vertices as u64,
            Some(&vertex_instance_bindings.buffer),
            vertex_instance_bindings.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            PathRasterizationInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_fragment_buffer(
            PathRasterizationInputIndex::Vertices as u64,
            Some(&vertex_instance_bindings.buffer),
            vertex_instance_bindings.offset as u64,
        );
        command_encoder.draw_primitives(
            metal::MTLPrimitiveType::Triangle,
            0,
            vertices.len() as u64,
        );

        command_encoder.end_encoding();
        Ok(true)
    }

    fn draw_shadows(
        &self,
        shadows: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if shadows.is_empty() {
            return;
        }

        command_encoder.set_render_pipeline_state(&self.shadows_pipeline_state);
        command_encoder.set_vertex_buffer(
            ShadowInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            ShadowInputIndex::Shadows as u64,
            Some(&instance_bindings.shadows.buffer),
            instance_bindings.shadows.offset as u64,
        );
        command_encoder.set_fragment_buffer(
            ShadowInputIndex::Shadows as u64,
            Some(&instance_bindings.shadows.buffer),
            instance_bindings.shadows.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            ShadowInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        command_encoder.draw_primitives_instanced_base_instance(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            shadows.len() as u64,
            shadows.start as u64,
        );
    }

    fn draw_quads(
        &self,
        quads: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if quads.is_empty() {
            return;
        }

        command_encoder.set_render_pipeline_state(&self.quads_pipeline_state);
        command_encoder.set_vertex_buffer(
            QuadInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            QuadInputIndex::Quads as u64,
            Some(&instance_bindings.quads.buffer),
            instance_bindings.quads.offset as u64,
        );
        command_encoder.set_fragment_buffer(
            QuadInputIndex::Quads as u64,
            Some(&instance_bindings.quads.buffer),
            instance_bindings.quads.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            QuadInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        command_encoder.draw_primitives_instanced_base_instance(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            quads.len() as u64,
            quads.start as u64,
        );
    }

    fn draw_paths_from_intermediate(
        &self,
        paths: &[Path<ScaledPixels>],
        writer: &mut InstanceBufferWriter,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) -> Result<()> {
        let Some(first_path) = paths.first() else {
            return Ok(());
        };
        let intermediate_texture = self
            .path_intermediate_texture
            .as_ref()
            .context("missing path intermediate texture")?;

        command_encoder.set_render_pipeline_state(&self.path_sprites_pipeline_state);
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        command_encoder.set_fragment_texture(
            SpriteInputIndex::AtlasTexture as u64,
            Some(intermediate_texture),
        );

        // When copying paths from the intermediate texture to the drawable,
        // each pixel must only be copied once, in case of transparent paths.
        //
        // If all paths have the same draw order, then their bounds are all
        // disjoint, so we can copy each path's bounds individually. If this
        // batch combines different draw orders, we perform a single copy
        // for a minimal spanning rect.
        let sprites;
        if paths.last().unwrap().order == first_path.order {
            sprites = paths
                .iter()
                .map(|path| PathSprite {
                    bounds: path.clipped_bounds(),
                })
                .collect();
        } else {
            let mut bounds = first_path.clipped_bounds();
            for path in paths.iter().skip(1) {
                bounds = bounds.union(&path.clipped_bounds());
            }
            sprites = vec![PathSprite { bounds }];
        }

        let sprite_instance_bindings = writer.write(&sprites)?;
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&sprite_instance_bindings.buffer),
            sprite_instance_bindings.offset as u64,
        );

        command_encoder.draw_primitives_instanced(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            sprites.len() as u64,
        );
        Ok(())
    }

    fn draw_underlines(
        &self,
        underlines: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if underlines.is_empty() {
            return;
        }

        command_encoder.set_render_pipeline_state(&self.underlines_pipeline_state);
        command_encoder.set_vertex_buffer(
            UnderlineInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            UnderlineInputIndex::Underlines as u64,
            Some(&instance_bindings.underlines.buffer),
            instance_bindings.underlines.offset as u64,
        );
        command_encoder.set_fragment_buffer(
            UnderlineInputIndex::Underlines as u64,
            Some(&instance_bindings.underlines.buffer),
            instance_bindings.underlines.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            UnderlineInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        command_encoder.draw_primitives_instanced_base_instance(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            underlines.len() as u64,
            underlines.start as u64,
        );
    }

    fn draw_monochrome_sprites(
        &self,
        texture_id: AtlasTextureId,
        sprites: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if sprites.is_empty() {
            return;
        }

        let Some(texture) = self.sprite_atlas.metal_texture(texture_id) else {
            return;
        };
        let texture_size = size(
            DevicePixels(texture.width() as i32),
            DevicePixels(texture.height() as i32),
        );
        command_encoder.set_render_pipeline_state(&self.monochrome_sprites_pipeline_state);
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_bindings.monochrome_sprites.buffer),
            instance_bindings.monochrome_sprites.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::AtlasTextureSize as u64,
            mem::size_of_val(&texture_size) as u64,
            &texture_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_fragment_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_bindings.monochrome_sprites.buffer),
            instance_bindings.monochrome_sprites.offset as u64,
        );
        command_encoder.set_fragment_texture(SpriteInputIndex::AtlasTexture as u64, Some(&texture));

        command_encoder.draw_primitives_instanced_base_instance(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            sprites.len() as u64,
            sprites.start as u64,
        );
    }

    fn draw_polychrome_sprites(
        &self,
        texture_id: AtlasTextureId,
        sprites: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if sprites.is_empty() {
            return;
        }

        let Some(texture) = self.sprite_atlas.metal_texture(texture_id) else {
            return;
        };
        let texture_size = size(
            DevicePixels(texture.width() as i32),
            DevicePixels(texture.height() as i32),
        );
        command_encoder.set_render_pipeline_state(&self.polychrome_sprites_pipeline_state);
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_bindings.polychrome_sprites.buffer),
            instance_bindings.polychrome_sprites.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::AtlasTextureSize as u64,
            mem::size_of_val(&texture_size) as u64,
            &texture_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_fragment_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_bindings.polychrome_sprites.buffer),
            instance_bindings.polychrome_sprites.offset as u64,
        );
        command_encoder.set_fragment_texture(SpriteInputIndex::AtlasTexture as u64, Some(&texture));

        command_encoder.draw_primitives_instanced_base_instance(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            sprites.len() as u64,
            sprites.start as u64,
        );
    }

    fn draw_surfaces(
        &mut self,
        surfaces: &[PaintSurface],
        first_surface: usize,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if surfaces.is_empty() {
            return;
        }

        command_encoder.set_vertex_buffer(
            SurfaceInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            SurfaceInputIndex::Surfaces as u64,
            Some(&instance_bindings.surfaces.buffer),
            instance_bindings.surfaces.offset as u64,
        );
        command_encoder.set_fragment_buffer(
            SurfaceInputIndex::Surfaces as u64,
            Some(&instance_bindings.surfaces.buffer),
            instance_bindings.surfaces.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            SurfaceInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        for (index, surface) in surfaces.iter().enumerate() {
            let Some(image_buffer) = &surface.image_buffer else {
                command_encoder.set_render_pipeline_state(&self.hole_surfaces_pipeline_state);
                let texture_size = size(DevicePixels::from(0), DevicePixels::from(0));
                command_encoder.set_vertex_bytes(
                    SurfaceInputIndex::TextureSize as u64,
                    mem::size_of_val(&texture_size) as u64,
                    &texture_size as *const Size<DevicePixels> as *const _,
                );
                command_encoder.draw_primitives_instanced_base_instance(
                    metal::MTLPrimitiveType::Triangle,
                    0,
                    6,
                    1,
                    (first_surface + index) as u64,
                );
                continue;
            };
            let texture_size = size(
                DevicePixels::from(image_buffer.get_width() as i32),
                DevicePixels::from(image_buffer.get_height() as i32),
            );

            let mut y_texture = None;
            let mut cb_cr_texture = None;
            let mut bgra_texture = None;
            let pixel_format = image_buffer.get_pixel_format();
            if pixel_format == kCVPixelFormatType_420YpCbCr8BiPlanarFullRange {
                command_encoder.set_render_pipeline_state(&self.surfaces_pipeline_state);
                y_texture = Some(
                    self.core_video_texture_cache
                        .create_texture_from_image(
                            image_buffer.as_concrete_TypeRef(),
                            None,
                            MTLPixelFormat::R8Unorm,
                            image_buffer.get_width_of_plane(0),
                            image_buffer.get_height_of_plane(0),
                            0,
                        )
                        .unwrap(),
                );
                cb_cr_texture = Some(
                    self.core_video_texture_cache
                        .create_texture_from_image(
                            image_buffer.as_concrete_TypeRef(),
                            None,
                            MTLPixelFormat::RG8Unorm,
                            image_buffer.get_width_of_plane(1),
                            image_buffer.get_height_of_plane(1),
                            1,
                        )
                        .unwrap(),
                );
            } else if pixel_format == kCVPixelFormatType_32BGRA {
                command_encoder.set_render_pipeline_state(&self.bgra_surfaces_pipeline_state);
                bgra_texture = Some(
                    self.core_video_texture_cache
                        .create_texture_from_image(
                            image_buffer.as_concrete_TypeRef(),
                            None,
                            MTLPixelFormat::BGRA8Unorm,
                            image_buffer.get_width(),
                            image_buffer.get_height(),
                            0,
                        )
                        .unwrap(),
                );
            } else {
                panic!("unsupported CoreVideo surface format {pixel_format:#x}");
            }

            command_encoder.set_vertex_bytes(
                SurfaceInputIndex::TextureSize as u64,
                mem::size_of_val(&texture_size) as u64,
                &texture_size as *const Size<DevicePixels> as *const _,
            );
            command_encoder.set_fragment_texture(
                SurfaceInputIndex::YTexture as u64,
                y_texture
                    .as_ref()
                    .or(bgra_texture.as_ref())
                    .map(|texture| unsafe {
                        let texture = CVMetalTextureGetTexture(texture.as_concrete_TypeRef());
                        metal::TextureRef::from_ptr(texture as *mut _)
                    }),
            );
            command_encoder.set_fragment_texture(
                SurfaceInputIndex::CbCrTexture as u64,
                cb_cr_texture.as_ref().map(|texture| unsafe {
                    let texture = CVMetalTextureGetTexture(texture.as_concrete_TypeRef());
                    metal::TextureRef::from_ptr(texture as *mut _)
                }),
            );

            command_encoder.draw_primitives_instanced_base_instance(
                metal::MTLPrimitiveType::Triangle,
                0,
                6,
                1,
                (first_surface + index) as u64,
            );
        }
    }
}

fn new_command_encoder_for_texture<'a>(
    command_buffer: &'a metal::CommandBufferRef,
    texture: &'a metal::TextureRef,
    viewport_size: Size<DevicePixels>,
    clear_color: Option<metal::MTLClearColor>,
) -> &'a metal::RenderCommandEncoderRef {
    let render_pass_descriptor = metal::RenderPassDescriptor::new();
    let color_attachment = render_pass_descriptor
        .color_attachments()
        .object_at(0)
        .unwrap();
    color_attachment.set_texture(Some(texture));
    color_attachment.set_store_action(metal::MTLStoreAction::Store);
    if let Some(clear_color) = clear_color {
        color_attachment.set_load_action(metal::MTLLoadAction::Clear);
        color_attachment.set_clear_color(clear_color);
    } else {
        color_attachment.set_load_action(metal::MTLLoadAction::Load);
    }

    let command_encoder = command_buffer.new_render_command_encoder(render_pass_descriptor);
    command_encoder.set_viewport(metal::MTLViewport {
        originX: 0.0,
        originY: 0.0,
        width: i32::from(viewport_size.width) as f64,
        height: i32::from(viewport_size.height) as f64,
        znear: 0.0,
        zfar: 1.0,
    });
    command_encoder
}

#[cfg(any(test, feature = "bench-support", feature = "test-support"))]
fn read_texture_to_image(texture: &metal::TextureRef) -> Result<RgbaImage> {
    let width = texture.width() as u32;
    let height = texture.height() as u32;
    let bytes_per_row = width as usize * 4;
    let mut pixels = vec![0u8; height as usize * bytes_per_row];

    let region = metal::MTLRegion {
        origin: metal::MTLOrigin { x: 0, y: 0, z: 0 },
        size: metal::MTLSize {
            width: width as u64,
            height: height as u64,
            depth: 1,
        },
    };
    texture.get_bytes(
        pixels.as_mut_ptr() as *mut std::ffi::c_void,
        bytes_per_row as u64,
        region,
        0,
    );

    // Convert BGRA to RGBA (swap B and R channels)
    for chunk in pixels.chunks_exact_mut(4) {
        chunk.swap(0, 2);
    }

    RgbaImage::from_raw(width, height, pixels).context("failed to create RgbaImage from pixel data")
}

fn build_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: metal::MTLPixelFormat,
) -> metal::RenderPipelineState {
    let vertex_fn = library
        .get_function(vertex_fn_name, None)
        .expect("error locating vertex function");
    let fragment_fn = library
        .get_function(fragment_fn_name, None)
        .expect("error locating fragment function");

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(true);
    color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_source_rgb_blend_factor(metal::MTLBlendFactor::SourceAlpha);
    color_attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
    color_attachment.set_destination_alpha_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);

    device
        .new_render_pipeline_state(&descriptor)
        .expect("could not create render pipeline state")
}

fn build_premultiplied_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: metal::MTLPixelFormat,
) -> metal::RenderPipelineState {
    let vertex_fn = library
        .get_function(vertex_fn_name, None)
        .expect("error locating vertex function");
    let fragment_fn = library
        .get_function(fragment_fn_name, None)
        .expect("error locating fragment function");

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(true);
    color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_source_rgb_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
    color_attachment.set_destination_alpha_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);

    device
        .new_render_pipeline_state(&descriptor)
        .expect("could not create render pipeline state")
}

fn build_hole_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: metal::MTLPixelFormat,
) -> metal::RenderPipelineState {
    let vertex_fn = library
        .get_function(vertex_fn_name, None)
        .expect("error locating vertex function");
    let fragment_fn = library
        .get_function(fragment_fn_name, None)
        .expect("error locating fragment function");

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(true);
    color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_source_rgb_blend_factor(metal::MTLBlendFactor::Zero);
    color_attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::Zero);
    color_attachment.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
    color_attachment.set_destination_alpha_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);

    device
        .new_render_pipeline_state(&descriptor)
        .expect("could not create render pipeline state")
}

fn build_path_rasterization_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: metal::MTLPixelFormat,
    path_sample_count: u32,
) -> metal::RenderPipelineState {
    let vertex_fn = library
        .get_function(vertex_fn_name, None)
        .expect("error locating vertex function");
    let fragment_fn = library
        .get_function(fragment_fn_name, None)
        .expect("error locating fragment function");

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    if path_sample_count > 1 {
        descriptor.set_raster_sample_count(path_sample_count as _);
        descriptor.set_alpha_to_coverage_enabled(false);
    }
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(true);
    color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_source_rgb_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
    color_attachment.set_destination_alpha_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);

    device
        .new_render_pipeline_state(&descriptor)
        .expect("could not create render pipeline state")
}

#[derive(Clone)]
struct InstanceBinding {
    buffer: metal::Buffer,
    offset: usize,
}

struct InstanceBindings {
    quads: InstanceBinding,
    shadows: InstanceBinding,
    underlines: InstanceBinding,
    monochrome_sprites: InstanceBinding,
    polychrome_sprites: InstanceBinding,
    surfaces: InstanceBinding,
}

fn write_instances(scene: &Scene, writer: &mut InstanceBufferWriter) -> Result<InstanceBindings> {
    Ok(InstanceBindings {
        quads: writer.write(&scene.quads)?,
        shadows: writer.write(&scene.shadows)?,
        underlines: writer.write(&scene.underlines)?,
        monochrome_sprites: writer.write(&scene.monochrome_sprites)?,
        polychrome_sprites: writer.write(&scene.polychrome_sprites)?,
        surfaces: writer.write_iter(scene.surfaces.iter().map(|surface| SurfaceBounds {
            bounds: surface.bounds,
            content_mask: surface.content_mask,
            corner_radii: surface.corner_radii,
            corner_smoothing: surface.corner_smoothing,
        }))?,
    })
}

struct InstanceBufferWriter {
    device: metal::Device,
    pool: Arc<Mutex<InstanceBufferPool>>,
    unified_memory: bool,
    filled: Vec<(InstanceBuffer, usize)>,
    current: InstanceBuffer,
    offset: usize,
}

impl InstanceBufferWriter {
    fn new(
        device: &metal::Device,
        pool: &Arc<Mutex<InstanceBufferPool>>,
        unified_memory: bool,
    ) -> Self {
        let current = pool.lock().acquire(device, unified_memory);
        Self {
            device: device.clone(),
            pool: pool.clone(),
            unified_memory,
            filled: Vec::new(),
            current,
            offset: 0,
        }
    }

    fn allocate<T>(&mut self, count: usize) -> Result<(InstanceBinding, &mut [MaybeUninit<T>])> {
        let size = mem::size_of::<T>() * count;
        let mut offset = self.offset.next_multiple_of(INSTANCE_BUFFER_ALIGNMENT);
        if offset + size > self.current.size {
            self.grow(size)?;
            offset = 0;
        }
        self.offset = offset + size;

        let binding = InstanceBinding {
            buffer: self.current.metal_buffer.clone(),
            offset,
        };
        // Safety: the reservation lies within a buffer this frame owns
        // exclusively, and never overlaps one handed out earlier.
        let values = unsafe {
            let start = (self.current.metal_buffer.contents() as *mut u8).add(offset);
            slice::from_raw_parts_mut(start.cast::<MaybeUninit<T>>(), count)
        };
        Ok((binding, values))
    }

    fn write<T>(&mut self, values: &[T]) -> Result<InstanceBinding> {
        let (binding, destination) = self.allocate::<T>(values.len())?;
        unsafe {
            ptr::copy_nonoverlapping(
                values.as_ptr(),
                destination.as_mut_ptr().cast::<T>(),
                values.len(),
            );
        }
        Ok(binding)
    }

    fn write_iter<T>(
        &mut self,
        values: impl ExactSizeIterator<Item = T>,
    ) -> Result<InstanceBinding> {
        let (binding, destination) = self.allocate::<T>(values.len())?;
        for (slot, value) in destination.iter_mut().zip(values) {
            slot.write(value);
        }
        Ok(binding)
    }

    fn grow(&mut self, required: usize) -> Result<()> {
        let mut pool = self.pool.lock();
        let buffer_size = (pool.buffer_size * 2)
            .max(required.next_power_of_two())
            .min(MAX_INSTANCE_BUFFER_SIZE);
        anyhow::ensure!(
            buffer_size >= required,
            "instance buffer needs {required} bytes, above the maximum of {MAX_INSTANCE_BUFFER_SIZE}"
        );
        anyhow::ensure!(
            buffer_size > self.current.size,
            "frame instance data exceeds the {MAX_INSTANCE_BUFFER_SIZE}-byte maximum"
        );
        if buffer_size != pool.buffer_size {
            log::info!("increased instance buffer size to {buffer_size}");
            pool.reset(buffer_size);
        }
        let buffer = pool.acquire(&self.device, self.unified_memory);
        drop(pool);

        let filled = mem::replace(&mut self.current, buffer);
        self.filled.push((filled, self.offset));
        self.offset = 0;
        Ok(())
    }

    fn finish(self) -> InstanceBuffer {
        let Self {
            unified_memory,
            filled,
            current,
            offset,
            ..
        } = self;

        if !unified_memory {
            for (buffer, written) in &filled {
                if *written == 0 {
                    continue;
                }
                buffer.metal_buffer.did_modify_range(NSRange {
                    location: 0,
                    length: *written as NSUInteger,
                });
            }
            if offset > 0 {
                current.metal_buffer.did_modify_range(NSRange {
                    location: 0,
                    length: offset as NSUInteger,
                });
            }
        }

        // Metal retains encoded resources until the command buffer completes.
        // Only the final, largest buffer is worth keeping in the pool.
        drop(filled);
        current
    }
}

#[repr(C)]
enum ShadowInputIndex {
    Vertices = 0,
    Shadows = 1,
    ViewportSize = 2,
}

#[repr(C)]
enum QuadInputIndex {
    Vertices = 0,
    Quads = 1,
    ViewportSize = 2,
}

#[repr(C)]
enum UnderlineInputIndex {
    Vertices = 0,
    Underlines = 1,
    ViewportSize = 2,
}

#[repr(C)]
enum SpriteInputIndex {
    Vertices = 0,
    Sprites = 1,
    ViewportSize = 2,
    AtlasTextureSize = 3,
    AtlasTexture = 4,
}

#[repr(C)]
enum SurfaceInputIndex {
    Vertices = 0,
    Surfaces = 1,
    ViewportSize = 2,
    TextureSize = 3,
    YTexture = 4,
    CbCrTexture = 5,
}

#[repr(C)]
enum ShaderLayerInputIndex {
    Vertices = 0,
    Bounds = 1,
    ViewportSize = 2,
}

#[repr(C)]
enum PathRasterizationInputIndex {
    Vertices = 0,
    ViewportSize = 1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct PathSprite {
    pub bounds: Bounds<ScaledPixels>,
}

#[derive(Clone, Debug, PartialEq)]
#[repr(C)]
pub struct ShaderLayerBounds {
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
}

#[derive(Clone)]
struct ShaderLayerPipeline {
    state: metal::RenderPipelineState,
    uniform_size: usize,
}

const MAX_SHADER_LAYER_INPUTS: usize = 8;

fn texture_has_size(texture: &metal::TextureRef, size: Size<DevicePixels>) -> bool {
    texture.width() == size.width.0 as u64 && texture.height() == size.height.0 as u64
}

/// The whole device pixels a layer covers, clipped to the viewport.
fn shader_layer_region(
    bounds: Bounds<ScaledPixels>,
    viewport_size: Size<DevicePixels>,
) -> Option<Bounds<DevicePixels>> {
    let left = (bounds.origin.x.0.round() as i32).max(0);
    let top = (bounds.origin.y.0.round() as i32).max(0);
    let right = (bounds.bottom_right().x.0.round() as i32).min(viewport_size.width.0);
    let bottom = (bounds.bottom_right().y.0.round() as i32).min(viewport_size.height.0);
    (right > left && bottom > top).then(|| Bounds {
        origin: point(DevicePixels(left), DevicePixels(top)),
        size: size(DevicePixels(right - left), DevicePixels(bottom - top)),
    })
}

fn build_shader_layer_pipeline(
    device: &metal::DeviceRef,
    vertex_fn: &metal::FunctionRef,
    shader: &CustomShader,
) -> Result<ShaderLayerPipeline> {
    use naga::back::msl;

    let module = naga::front::wgsl::parse_str(shader.wgsl())
        .map_err(|error| anyhow::anyhow!(error.emit_to_string(shader.wgsl())))?;
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)?;
    let entry_point = module
        .entry_points
        .iter()
        .find(|entry_point| entry_point.stage == naga::ShaderStage::Fragment)
        .context("no fragment entry point")?;

    let mut uniform_size = 0;
    let mut resources = msl::BindingMap::default();
    let mut layouter = naga::proc::Layouter::default();
    layouter.update(module.to_ctx())?;
    for (_, global) in module.global_variables.iter() {
        let Some(binding) = &global.binding else {
            continue;
        };
        let target = match (binding.group, binding.binding, global.space) {
            (0, 0, naga::AddressSpace::Handle) => msl::BindTarget {
                texture: Some(0),
                ..Default::default()
            },
            (0, 1, naga::AddressSpace::Uniform) => {
                uniform_size = layouter[global.ty].size as usize;
                msl::BindTarget {
                    buffer: Some(0),
                    ..Default::default()
                }
            }
            (0, 2, naga::AddressSpace::Handle) => msl::BindTarget {
                sampler: Some(msl::BindSamplerTarget::Resource(0)),
                ..Default::default()
            },
            _ => anyhow::bail!(
                "unsupported binding @group({}) @binding({})",
                binding.group,
                binding.binding
            ),
        };
        resources.insert(*binding, target);
    }
    let mut per_entry_point_map = msl::EntryPointResourceMap::default();
    per_entry_point_map.insert(
        entry_point.name.clone(),
        msl::EntryPointResources {
            resources,
            ..Default::default()
        },
    );
    let options = msl::Options {
        lang_version: (2, 4),
        per_entry_point_map,
        fake_missing_bindings: false,
        ..Default::default()
    };
    let pipeline_options = msl::PipelineOptions {
        entry_point: Some((naga::ShaderStage::Fragment, entry_point.name.clone())),
        ..Default::default()
    };
    let (source, translation) = msl::write_string(&module, &info, &options, &pipeline_options)?;
    let fragment_name = translation
        .entry_point_names
        .into_iter()
        .next()
        .context("no translated entry point")?
        .map_err(|error| anyhow::anyhow!("{error}"))?;

    let library = device
        .new_library_with_source(&source, &metal::CompileOptions::new())
        .map_err(|error| anyhow::anyhow!(error))?;
    let fragment_fn = library
        .get_function(&fragment_name, None)
        .map_err(|error| anyhow::anyhow!(error))?;

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label("shader_layer");
    descriptor.set_vertex_function(Some(vertex_fn));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
    color_attachment.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
    color_attachment.set_blending_enabled(true);
    color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_source_rgb_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
    color_attachment.set_destination_alpha_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
    let state = device
        .new_render_pipeline_state(&descriptor)
        .map_err(|error| anyhow::anyhow!(error))?;
    Ok(ShaderLayerPipeline {
        state,
        uniform_size,
    })
}

#[derive(Clone, Debug, PartialEq)]
#[repr(C)]
pub struct SurfaceBounds {
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub corner_radii: zz_gpui::Corners<ScaledPixels>,
    pub corner_smoothing: f32,
}

#[cfg(any(test, feature = "bench-support", feature = "test-support"))]
pub struct MetalHeadlessRenderer {
    renderer: MetalRenderer,
}

#[cfg(any(test, feature = "bench-support", feature = "test-support"))]
impl MetalHeadlessRenderer {
    pub fn new() -> Self {
        let instance_buffer_pool = Arc::new(Mutex::new(InstanceBufferPool::default()));
        let renderer = MetalRenderer::new_headless(instance_buffer_pool);
        Self { renderer }
    }
}

#[cfg(any(test, feature = "bench-support", feature = "test-support"))]
impl zz_gpui::PlatformHeadlessRenderer for MetalHeadlessRenderer {
    fn render_scene_to_image(
        &mut self,
        scene: &Scene,
        size: Size<DevicePixels>,
    ) -> anyhow::Result<image::RgbaImage> {
        self.renderer.render_scene_to_image(scene, size)
    }

    fn render_scene(&mut self, scene: &Scene, size: Size<DevicePixels>) -> anyhow::Result<()> {
        self.renderer.render_scene(scene, size)
    }

    fn sprite_atlas(&self) -> Arc<dyn zz_gpui::PlatformAtlas> {
        self.renderer.sprite_atlas().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zz_gpui::{Quad, Shadow, hsla, px};

    fn small_target(renderer: &MetalRenderer) -> metal::Texture {
        let descriptor = metal::TextureDescriptor::new();
        descriptor.set_width(16);
        descriptor.set_height(16);
        descriptor.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        descriptor.set_usage(metal::MTLTextureUsage::RenderTarget);
        descriptor.set_storage_mode(metal::MTLStorageMode::Private);
        renderer.device.new_texture(&descriptor)
    }

    #[test]
    fn abandoned_render_frame_releases_its_atlas_guard() -> Result<()> {
        let mut renderer =
            MetalRenderer::new_headless(Arc::new(Mutex::new(InstanceBufferPool::default())));
        let atlas = renderer.sprite_atlas.clone();
        assert_eq!(Arc::strong_count(&atlas), 2);
        objc::rc::autoreleasepool(|| -> Result<()> {
            let target = small_target(&renderer);
            let commands =
                renderer.render_frame(&Scene::default(), &target, size(16.into(), 16.into()))?;
            assert_eq!(Arc::strong_count(&atlas), 3);
            drop(commands);
            Ok(())
        })?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while Arc::strong_count(&atlas) != 2 {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(Arc::strong_count(&atlas), 2);
        Ok(())
    }

    #[test]
    fn failed_encoding_and_invalid_targets_leave_no_atlas_guard() -> Result<()> {
        let mut renderer =
            MetalRenderer::new_headless(Arc::new(Mutex::new(InstanceBufferPool::default())));
        let atlas = renderer.sprite_atlas.clone();
        let mut path = Path::new(point(px(0.0), px(0.0)));
        path.line_to(point(px(16.0), px(0.0)));
        path.line_to(point(px(0.0), px(16.0)));
        path.content_mask.bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(16.0), px(16.0)));
        let mut scene = Scene::default();
        scene.insert_primitive(path.scale(1.0));
        scene.finish();
        objc::rc::autoreleasepool(|| {
            let target = small_target(&renderer);
            let error = renderer
                .render_frame(&scene, &target, size(16.into(), 16.into()))
                .err()
                .expect("headless path target was not initialized");
            assert!(
                error
                    .to_string()
                    .contains("missing path intermediate texture")
            );
            assert_eq!(Arc::strong_count(&atlas), 2);
        });
        assert!(
            renderer
                .render_scene(&scene, size(0.into(), 16.into()))
                .is_err()
        );
        assert!(
            renderer
                .render_scene_to_image(&scene, size(16.into(), 0.into()))
                .is_err()
        );
        assert!(renderer.render_to_image(&scene).is_err());
        renderer.draw(&scene);
        assert_eq!(Arc::strong_count(&atlas), 2);
        Ok(())
    }

    #[test]
    fn pending_render_frame_outlives_renderer_without_retaining_it() -> Result<()> {
        let mut renderer =
            MetalRenderer::new_headless(Arc::new(Mutex::new(InstanceBufferPool::default())));
        let atlas = Arc::downgrade(&renderer.sprite_atlas);
        objc::rc::autoreleasepool(|| -> Result<()> {
            let target = small_target(&renderer);
            let commands =
                renderer.render_frame(&Scene::default(), &target, size(16.into(), 16.into()))?;
            drop(renderer);
            assert!(atlas.upgrade().is_some());
            commands.commit();
            commands.wait_until_completed();
            assert_eq!(commands.status(), metal::MTLCommandBufferStatus::Completed);
            Ok(())
        })?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while atlas.upgrade().is_some() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(atlas.upgrade().is_none());
        Ok(())
    }

    #[test]
    fn glass_lenses_its_backdrop() -> Result<()> {
        let mut renderer =
            MetalRenderer::new_headless(Arc::new(Mutex::new(InstanceBufferPool::default())));
        zz_gpui::check_glass_rendering(|scene| {
            renderer.render_scene_to_image(scene, size(64.into(), 64.into()))
        })
    }

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

    fn solid_quad(bounds: Bounds<ScaledPixels>, color: zz_gpui::Hsla) -> Quad {
        let mut quad = Quad::default();
        quad.bounds = bounds;
        quad.content_mask.bounds = bounds;
        quad.background = color.into();
        quad
    }

    fn red_layer(
        bounds: Bounds<ScaledPixels>,
        shader: &str,
        inner: Option<ShaderLayer>,
    ) -> ShaderLayer {
        let mut scene = Scene::default();
        scene.insert_primitive(solid_quad(bounds, hsla(0.0, 1.0, 0.5, 1.0)));
        if let Some(inner) = inner {
            scene.insert_primitive(inner);
        }
        scene.finish();
        let tint: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
        ShaderLayer {
            order: 0,
            bounds,
            content_mask: ContentMask { bounds },
            shader: CustomShader::new(shader.to_owned()),
            uniforms: tint.iter().flat_map(|value| value.to_ne_bytes()).collect(),
            scene: std::rc::Rc::new(scene),
        }
    }

    fn render_layer(layer: ShaderLayer) -> Result<RgbaImage> {
        let mut renderer =
            MetalRenderer::new_headless(Arc::new(Mutex::new(InstanceBufferPool::default())));
        let target = Bounds::new(point(px(0.0), px(0.0)), size(px(32.0), px(32.0))).scale(1.0);
        let mut scene = Scene::default();
        scene.insert_primitive(solid_quad(target, hsla(0.0, 0.0, 0.0, 1.0)));
        scene.insert_primitive(layer);
        scene.finish();
        renderer.render_scene_to_image(&scene, size(32.into(), 32.into()))
    }

    fn layer_bounds() -> Bounds<ScaledPixels> {
        Bounds::new(point(px(8.0), px(8.0)), size(px(16.0), px(16.0))).scale(1.0)
    }

    #[test]
    fn shader_layers_run_custom_shaders_over_their_content() -> Result<()> {
        let image = render_layer(red_layer(layer_bounds(), SWAP_OR_TINT, None))?;
        assert_eq!(image.get_pixel(4, 12).0, [0, 0, 0, 255]);
        assert_eq!(image.get_pixel(10, 12).0, [0, 255, 0, 255]);
        assert_eq!(image.get_pixel(20, 12).0, [0, 0, 255, 255]);
        assert_eq!(image.get_pixel(28, 12).0, [0, 0, 0, 255]);
        Ok(())
    }

    #[test]
    fn nested_shader_layers_feed_the_outer_layer() -> Result<()> {
        let inner = red_layer(layer_bounds(), SWAP_OR_TINT, None);
        let image = render_layer(red_layer(layer_bounds(), SWAP_OR_TINT, Some(inner)))?;
        assert_eq!(image.get_pixel(10, 12).0, [0, 255, 0, 255]);
        assert_eq!(image.get_pixel(20, 12).0, [255, 0, 0, 255]);
        Ok(())
    }

    #[test]
    fn invalid_custom_shaders_paint_the_layer_unchanged() -> Result<()> {
        let image = render_layer(red_layer(layer_bounds(), "not wgsl", None))?;
        assert_eq!(image.get_pixel(10, 12).0, [255, 0, 0, 255]);
        assert_eq!(image.get_pixel(20, 12).0, [255, 0, 0, 255]);
        assert_eq!(image.get_pixel(28, 12).0, [0, 0, 0, 255]);
        Ok(())
    }

    #[test]
    fn translucent_layers_preserve_source_over_alpha() -> Result<()> {
        let mut renderer =
            MetalRenderer::new_headless(Arc::new(Mutex::new(InstanceBufferPool::default())));
        renderer.opaque = false;
        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(16.0), px(16.0))).scale(1.0);
        for (alphas, expected) in [
            ([0.0, 0.5, 0.0], 128_u8),
            ([0.93, 0.5, 0.0], 246),
            ([0.93, 0.5, 0.5], 251),
            ([0.93, 0.5, 1.0], 255),
        ] {
            let mut scene = Scene::default();
            for alpha in alphas {
                let mut quad = Quad::default();
                quad.bounds = bounds;
                quad.content_mask.bounds = bounds;
                quad.background = hsla(0.0, 0.0, 0.15, alpha).into();
                scene.insert_primitive(quad);
            }
            scene.finish();
            let image = renderer.render_scene_to_image(&scene, size(16.into(), 16.into()))?;
            assert!(image.get_pixel(8, 8)[3].abs_diff(expected) <= 1);
        }
        Ok(())
    }

    #[test]
    fn blurred_shadows_follow_smoothed_corners() -> Result<()> {
        let mut renderer =
            MetalRenderer::new_headless(Arc::new(Mutex::new(InstanceBufferPool::default())));
        renderer.opaque = false;
        let bounds = Bounds::new(point(px(20.0), px(20.0)), size(px(96.0), px(96.0))).scale(1.0);
        for (smoothing, radius, diagonal) in [(2.0, 32.0, 29), (4.0, 32.0, 25), (4.0, 48.0, 34)] {
            let mut scene = Scene::default();
            scene.insert_primitive(Shadow {
                order: Default::default(),
                blur_radius: px(2.0).scale(1.0),
                bounds,
                corner_radii: zz_gpui::Corners::all(px(radius).scale(1.0)),
                content_mask: zz_gpui::ContentMask {
                    bounds: Bounds::new(point(px(0.0), px(0.0)), size(px(136.0), px(136.0)))
                        .scale(1.0),
                },
                color: hsla(0.0, 0.0, 1.0, 1.0),
                element_bounds: bounds,
                element_corner_radii: zz_gpui::Corners::all(px(radius).scale(1.0)),
                inset: 0,
                corner_smoothing: smoothing,
            });
            scene.finish();
            let image = renderer.render_scene_to_image(&scene, size(136.into(), 136.into()))?;
            let edge_alpha = image.get_pixel(20, 68)[3];
            let far = 135 - diagonal;
            for (x, y) in [
                (diagonal, diagonal),
                (far, diagonal),
                (diagonal, far),
                (far, far),
            ] {
                let corner_alpha = image.get_pixel(x, y)[3];
                assert!(
                    corner_alpha.abs_diff(edge_alpha) <= 32,
                    "detached shadow at ({x}, {y}), smoothing={smoothing}, radius={radius}: corner={corner_alpha}, edge={edge_alpha}"
                );
            }
        }
        Ok(())
    }

    fn pane_glow_scene(width: f32, height: f32, glow: bool) -> Scene {
        let mut scene = Scene::default();
        let full = Bounds::new(point(px(0.0), px(0.0)), size(px(width), px(height))).scale(1.0);
        let mut background = Quad::default();
        background.bounds = full;
        background.content_mask.bounds = full;
        background.background = hsla(0.0, 0.0, 0.1, 1.0).into();
        scene.insert_primitive(background);
        if glow {
            let element = Bounds::new(
                point(px(12.0), px(12.0)),
                size(px(width - 24.0), px(height - 24.0)),
            );
            let hole = (element + point(px(32.0), px(48.0))).dilate(px(16.0));
            scene.insert_primitive(Shadow {
                order: Default::default(),
                blur_radius: px(192.0).scale(1.0),
                bounds: hole.scale(1.0),
                corner_radii: zz_gpui::Corners::all(px(43.0).scale(1.0)),
                content_mask: background.content_mask,
                color: hsla(0.6, 0.8, 0.6, 0.06),
                element_bounds: element.scale(1.0),
                element_corner_radii: zz_gpui::Corners::all(px(27.0).scale(1.0)),
                inset: 1,
                corner_smoothing: 4.0,
            });
        }
        scene.finish();
        scene
    }

    #[test]
    #[ignore = "benchmark: cargo test -p zz_gpui-apple --release bench_pane_glow -- --ignored --nocapture"]
    fn bench_pane_glow() -> Result<()> {
        let mut renderer =
            MetalRenderer::new_headless(Arc::new(Mutex::new(InstanceBufferPool::default())));
        let (width, height) = (5344.0, 2964.0);
        let target = size((width as i32).into(), (height as i32).into());
        let mut time = |glow: bool| -> Result<f64> {
            let scene = pane_glow_scene(width, height, glow);
            renderer.render_scene_to_image(&scene, target)?;
            let frames = 120;
            let start = std::time::Instant::now();
            for _ in 0..frames {
                renderer.render_scene(&scene, target)?;
            }
            renderer.render_scene_to_image(&scene, target)?;
            Ok(start.elapsed().as_secs_f64() * 1000.0 / f64::from(frames + 1))
        };
        let plain = time(false)?;
        let glow = time(true)?;
        println!(
            "pane glow {width}x{height}: background {plain:.3} ms, with glow {glow:.3} ms, glow {:.3} ms/frame",
            glow - plain
        );
        Ok(())
    }

    #[test]
    #[ignore = "benchmark: cargo test -p zz-gpui-platform --release bench_glass -- --ignored --nocapture"]
    fn bench_glass() -> Result<()> {
        let mut renderer =
            MetalRenderer::new_headless(Arc::new(Mutex::new(InstanceBufferPool::default())));
        let (width, height) = (5344.0, 2964.0);
        let target = size((width as i32).into(), (height as i32).into());
        zz_gpui::bench_glass_rendering(width, height, |scene| {
            renderer.render_scene_to_image(scene, target)?;
            let frames = 120;
            let start = std::time::Instant::now();
            for _ in 0..frames {
                renderer.render_scene(scene, target)?;
            }
            renderer.render_scene_to_image(scene, target)?;
            Ok(start.elapsed().as_secs_f64() * 1000.0 / f64::from(frames + 1))
        })
    }

    #[test]
    fn faint_inset_shadows_dither_dark_composites() -> Result<()> {
        let mut renderer =
            MetalRenderer::new_headless(Arc::new(Mutex::new(InstanceBufferPool::default())));
        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(256.0), px(1024.0))).scale(1.0);

        for opacity in [0.0, 0.032] {
            let mut scene = Scene::default();
            let mut background = Quad::default();
            background.bounds = bounds;
            background.content_mask.bounds = bounds;
            background.background = hsla(0.0, 0.0, 0.15, 1.0).into();
            scene.insert_primitive(background);
            scene.insert_primitive(Shadow {
                order: Default::default(),
                blur_radius: px(96.0).scale(1.0),
                bounds,
                corner_radii: Default::default(),
                content_mask: background.content_mask,
                color: hsla(0.0, 0.0, 1.0, opacity),
                element_bounds: bounds,
                element_corner_radii: Default::default(),
                inset: 1,
                corner_smoothing: 2.0,
            });
            scene.finish();
            let image = renderer.render_scene_to_image(&scene, size(256.into(), 1024.into()))?;
            let values: Vec<_> = (400..600).map(|y| image.get_pixel(16, y)[0]).collect();
            let min = *values.iter().min().unwrap();
            let max = *values.iter().max().unwrap();
            assert!(image.pixels().all(|pixel| pixel[3] == 255));
            if opacity == 0.0 {
                assert!(image.pixels().all(|pixel| pixel[0] == 38));
            } else {
                assert!(min < max, "flat band: min={min}, max={max}");
                assert!(max - min <= 2, "visible noise: min={min}, max={max}");
                let mean =
                    values.iter().map(|value| f32::from(*value)).sum::<f32>() / values.len() as f32;
                assert!(
                    (mean - 41.0).abs() <= 0.5,
                    "brightness changed: mean={mean}"
                );
            }
        }
        Ok(())
    }
}
