use anyhow::{Context as _, Result};
use gpui::{
    Bounds, GLASS_SHADER, Glass, GlassBlurPass, GlassUniform, Size, WindowCornerMask,
    glass_level_size, plan_glass,
};
use metal::MTLPixelFormat;
use std::ffi::c_void;

/// Pipelines and scratch textures for [`Glass`], built from the same WGSL the
/// wgpu renderer runs, translated to MSL by naga when a scene first paints
/// glass.
pub(crate) struct MetalGlass {
    glass_pipeline: metal::RenderPipelineState,
    down_pipeline: metal::RenderPipelineState,
    up_pipeline: metal::RenderPipelineState,
    sampler: metal::SamplerState,
    levels: Vec<metal::Texture>,
    /// The largest chain any run needed this frame, in level 0 texels.
    peak: Size<i32>,
}

/// The chain grows in steps of this many level 0 texels, so a glass that
/// moves or springs does not reallocate it every frame.
const CHAIN_STEP: u32 = 256;

impl MetalGlass {
    pub(crate) fn new(device: &metal::DeviceRef) -> Result<Self> {
        use naga::back::msl;

        let module = naga::front::wgsl::parse_str(GLASS_SHADER)
            .map_err(|error| anyhow::anyhow!(error.emit_to_string(GLASS_SHADER)))?;
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)?;

        let buffer = |index| msl::BindTarget {
            buffer: Some(index),
            ..Default::default()
        };
        let texture = |index| msl::BindTarget {
            texture: Some(index),
            ..Default::default()
        };
        let sampler = |index| msl::BindTarget {
            sampler: Some(msl::BindSamplerTarget::Resource(index)),
            ..Default::default()
        };
        let binding = |binding| naga::ResourceBinding { group: 0, binding };
        let glass_resources = msl::BindingMap::from_iter([
            (binding(0), buffer(0)),
            (binding(1), sampler(0)),
            (binding(2), texture(0)),
            (binding(3), texture(1)),
        ]);
        let blur_resources = msl::BindingMap::from_iter([
            (binding(4), buffer(0)),
            (binding(5), sampler(0)),
            (binding(6), texture(0)),
        ]);
        let mut per_entry_point_map = msl::EntryPointResourceMap::default();
        for (name, resources) in [
            ("vs_glass", &glass_resources),
            ("fs_glass", &glass_resources),
            ("vs_glass_blur", &blur_resources),
            ("fs_glass_down", &blur_resources),
            ("fs_glass_up", &blur_resources),
        ] {
            per_entry_point_map.insert(
                name.to_string(),
                msl::EntryPointResources {
                    resources: resources.clone(),
                    ..Default::default()
                },
            );
        }
        let options = msl::Options {
            lang_version: (2, 4),
            per_entry_point_map,
            fake_missing_bindings: false,
            ..Default::default()
        };
        let (source, translation) =
            msl::write_string(&module, &info, &options, &msl::PipelineOptions::default())?;
        let library = device
            .new_library_with_source(&source, &metal::CompileOptions::new())
            .map_err(|error| anyhow::anyhow!(error))?;
        let function = |name: &str| -> Result<metal::Function> {
            let index = module
                .entry_points
                .iter()
                .position(|entry_point| entry_point.name == name)
                .with_context(|| format!("no entry point {name}"))?;
            let translated = translation
                .entry_point_names
                .get(index)
                .with_context(|| format!("{name} was not translated"))?
                .as_ref()
                .map_err(|error| anyhow::anyhow!("{error}"))?;
            library
                .get_function(translated, None)
                .map_err(|error| anyhow::anyhow!(error))
        };

        let pipeline = |label: &str,
                        vertex: &str,
                        fragment: &str,
                        blend: bool|
         -> Result<metal::RenderPipelineState> {
            let descriptor = metal::RenderPipelineDescriptor::new();
            descriptor.set_label(label);
            let vertex = function(vertex)?;
            let fragment = function(fragment)?;
            descriptor.set_vertex_function(Some(vertex.as_ref()));
            descriptor.set_fragment_function(Some(fragment.as_ref()));
            let attachment = descriptor
                .color_attachments()
                .object_at(0)
                .context("no color attachment")?;
            attachment.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
            if blend {
                attachment.set_blending_enabled(true);
                attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
                attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
                attachment.set_source_rgb_blend_factor(metal::MTLBlendFactor::One);
                attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
                attachment
                    .set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
                attachment
                    .set_destination_alpha_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
            }
            device
                .new_render_pipeline_state(&descriptor)
                .map_err(|error| anyhow::anyhow!(error))
        };

        let sampler_descriptor = metal::SamplerDescriptor::new();
        sampler_descriptor.set_min_filter(metal::MTLSamplerMinMagFilter::Linear);
        sampler_descriptor.set_mag_filter(metal::MTLSamplerMinMagFilter::Linear);
        sampler_descriptor.set_address_mode_s(metal::MTLSamplerAddressMode::ClampToEdge);
        sampler_descriptor.set_address_mode_t(metal::MTLSamplerAddressMode::ClampToEdge);

        Ok(Self {
            glass_pipeline: pipeline("glass", "vs_glass", "fs_glass", true)?,
            down_pipeline: pipeline("glass_down", "vs_glass_blur", "fs_glass_down", false)?,
            up_pipeline: pipeline("glass_up", "vs_glass_blur", "fs_glass_up", false)?,
            sampler: device.new_sampler(&sampler_descriptor),
            levels: Vec::new(),
            peak: Size::default(),
        })
    }

    pub(crate) fn begin_frame(&mut self) {
        self.peak = Size::default();
    }

    /// Frees a chain the frame did not use, or used little of.
    pub(crate) fn end_frame(&mut self) {
        if let Some(level) = self.levels.first() {
            let area = level.width() as i64 * level.height() as i64;
            let peak = self.peak.width as i64 * self.peak.height as i64;
            if area > peak * 4 {
                self.levels.clear();
            }
        }
    }

    /// Makes the chain span `extent` level 0 texels and reach `depth` levels.
    fn ensure_chain(&mut self, device: &metal::DeviceRef, extent: Size<i32>, depth: u32) {
        self.peak = Size {
            width: self.peak.width.max(extent.width),
            height: self.peak.height.max(extent.height),
        };
        let fits = self.levels.first().is_some_and(|level| {
            level.width() as i32 >= extent.width && level.height() as i32 >= extent.height
        });
        if !fits {
            self.levels.clear();
        }
        let capacity = match self.levels.first() {
            Some(level) => Size {
                width: level.width() as i32,
                height: level.height() as i32,
            },
            None => Size {
                width: (extent.width.max(1) as u32).next_multiple_of(CHAIN_STEP) as i32,
                height: (extent.height.max(1) as u32).next_multiple_of(CHAIN_STEP) as i32,
            },
        };
        while self.levels.len() <= depth.max(1) as usize {
            let size = glass_level_size(capacity, self.levels.len() as u32);
            let descriptor = metal::TextureDescriptor::new();
            descriptor.set_width(size.width as u64);
            descriptor.set_height(size.height as u64);
            descriptor.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
            descriptor.set_storage_mode(metal::MTLStorageMode::Private);
            descriptor.set_usage(
                metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead,
            );
            self.levels.push(device.new_texture(&descriptor));
        }
    }

    /// Draws a batch of glass over `target`. The caller ends its encoder
    /// first. Every run but the last draws in an encoder of its own; the last
    /// run's draws come back, for the caller to issue with
    /// [`Self::draw_pending`] in the encoder that resumes the scene, which
    /// saves the tiled GPU a load and store of the whole target.
    pub(crate) fn draw(
        &mut self,
        device: &metal::DeviceRef,
        command_buffer: &metal::CommandBufferRef,
        glasses: &[Glass],
        target: &metal::TextureRef,
        window_mask: Option<WindowCornerMask>,
    ) -> Vec<(GlassUniform, Bounds<i32>)> {
        let viewport = Size {
            width: target.width() as i32,
            height: target.height() as i32,
        };
        let mut pending = Vec::new();
        for run in plan_glass(glasses, viewport, window_mask) {
            if !pending.is_empty() {
                let encoder = render_encoder(command_buffer, target, metal::MTLLoadAction::Load);
                self.draw_pending(encoder, &pending, viewport);
                encoder.end_encoding();
            }
            self.ensure_chain(device, run.extent, run.depth);

            let blit = command_buffer.new_blit_command_encoder();
            for (region, destination) in &run.copies {
                blit.copy_from_texture(
                    target,
                    0,
                    0,
                    metal::MTLOrigin {
                        x: region.origin.x as u64,
                        y: region.origin.y as u64,
                        z: 0,
                    },
                    metal::MTLSize {
                        width: region.size.width as u64,
                        height: region.size.height as u64,
                        depth: 1,
                    },
                    &self.levels[0],
                    0,
                    0,
                    metal::MTLOrigin {
                        x: destination.x as u64,
                        y: destination.y as u64,
                        z: 0,
                    },
                );
            }
            blit.end_encoding();

            for pass in &run.passes {
                self.blur_pass(command_buffer, pass);
            }

            pending = run.draws;
        }
        pending
    }

    /// Issues a run's glass draws into `encoder`, an encoder over the
    /// target, and gives it back its full scissor.
    pub(crate) fn draw_pending(
        &self,
        encoder: &metal::RenderCommandEncoderRef,
        draws: &[(GlassUniform, Bounds<i32>)],
        viewport: Size<i32>,
    ) {
        if draws.is_empty() {
            return;
        }
        encoder.set_render_pipeline_state(&self.glass_pipeline);
        encoder.set_fragment_sampler_state(0, Some(&self.sampler));
        encoder.set_fragment_texture(0, Some(&self.levels[0]));
        encoder.set_fragment_texture(1, Some(&self.levels[1]));
        for (uniform, scissor) in draws {
            let bytes = uniform.as_bytes();
            encoder.set_vertex_bytes(0, bytes.len() as u64, bytes.as_ptr() as *const c_void);
            encoder.set_fragment_bytes(0, bytes.len() as u64, bytes.as_ptr() as *const c_void);
            if set_scissor(encoder, *scissor, viewport) {
                encoder.draw_primitives(metal::MTLPrimitiveType::TriangleStrip, 0, 4);
            }
        }
        encoder.set_scissor_rect(metal::MTLScissorRect {
            x: 0,
            y: 0,
            width: viewport.width as u64,
            height: viewport.height as u64,
        });
    }

    fn blur_pass(&self, command_buffer: &metal::CommandBufferRef, pass: &GlassBlurPass) {
        if pass.draws.is_empty() {
            return;
        }
        let target = &self.levels[pass.destination as usize];
        let size = Size {
            width: target.width() as i32,
            height: target.height() as i32,
        };
        // A down pass writes every texel a later pass reads, so on a tiled GPU
        // its level never needs loading.
        let load = if pass.down {
            metal::MTLLoadAction::DontCare
        } else {
            metal::MTLLoadAction::Load
        };
        let encoder = render_encoder(command_buffer, target, load);
        encoder.set_render_pipeline_state(if pass.down {
            &self.down_pipeline
        } else {
            &self.up_pipeline
        });
        encoder.set_fragment_sampler_state(0, Some(&self.sampler));
        encoder.set_fragment_texture(0, Some(&self.levels[pass.source as usize]));
        for (uniform, rect) in &pass.draws {
            let bytes = uniform.as_bytes();
            encoder.set_fragment_bytes(0, bytes.len() as u64, bytes.as_ptr() as *const c_void);
            if set_scissor(encoder, *rect, size) {
                encoder.draw_primitives(metal::MTLPrimitiveType::Triangle, 0, 3);
            }
        }
        encoder.end_encoding();
    }
}

fn render_encoder<'a>(
    command_buffer: &'a metal::CommandBufferRef,
    texture: &metal::TextureRef,
    load: metal::MTLLoadAction,
) -> &'a metal::RenderCommandEncoderRef {
    let descriptor = metal::RenderPassDescriptor::new();
    let attachment = descriptor.color_attachments().object_at(0).unwrap();
    attachment.set_texture(Some(texture));
    attachment.set_load_action(load);
    attachment.set_store_action(metal::MTLStoreAction::Store);
    command_buffer.new_render_command_encoder(descriptor)
}

/// Scissors to `rect` within a target of `size`; false when nothing is left.
fn set_scissor(
    encoder: &metal::RenderCommandEncoderRef,
    rect: Bounds<i32>,
    size: Size<i32>,
) -> bool {
    let left = rect.origin.x.clamp(0, size.width);
    let top = rect.origin.y.clamp(0, size.height);
    let right = (rect.origin.x + rect.size.width).clamp(0, size.width);
    let bottom = (rect.origin.y + rect.size.height).clamp(0, size.height);
    if right <= left || bottom <= top {
        return false;
    }
    encoder.set_scissor_rect(metal::MTLScissorRect {
        x: left as u64,
        y: top as u64,
        width: (right - left) as u64,
        height: (bottom - top) as u64,
    });
    true
}

/// Whether a scene paints glass, its shader layers' included.
pub(crate) fn scene_has_glass(scene: &gpui::Scene) -> bool {
    !scene.glasses.is_empty()
        || scene
            .shader_layers
            .iter()
            .any(|layer| scene_has_glass(&layer.scene))
}
