use anyhow::{Context as _, Result};
use gpui::{
    Bounds, GLASS_MAX_BLUR_LEVELS, GLASS_SHADER, Glass, GlassBlurPlan, GlassBlurUniform, Size,
    glass_level_rect,
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
    drawn: bool,
}

/// A glass draw with its backdrop region and blur worked out.
struct Planned<'a> {
    glass: &'a Glass,
    region: Bounds<i32>,
    scissor: Bounds<i32>,
    blur: GlassBlurPlan,
}

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
            drawn: false,
        })
    }

    pub(crate) fn begin_frame(&mut self) {
        self.drawn = false;
    }

    /// Frees the blur chain after a frame without glass.
    pub(crate) fn end_frame(&mut self) {
        if !self.drawn {
            self.levels.clear();
        }
    }

    fn ensure_levels(&mut self, device: &metal::DeviceRef, size: Size<i32>, depth: u32) {
        let fits = self.levels.first().is_some_and(|level| {
            level.width() == size.width as u64 && level.height() == size.height as u64
        });
        if !fits {
            self.levels.clear();
        }
        while self.levels.len() <= depth.max(1) as usize {
            let level = self.levels.len() as u32;
            let descriptor = metal::TextureDescriptor::new();
            descriptor.set_width((size.width as u64).div_ceil(1 << level).max(1));
            descriptor.set_height((size.height as u64).div_ceil(1 << level).max(1));
            descriptor.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
            descriptor.set_storage_mode(metal::MTLStorageMode::Private);
            descriptor.set_usage(
                metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead,
            );
            self.levels.push(device.new_texture(&descriptor));
        }
    }

    /// Draws a batch of glass over `target`. The caller ends its encoder
    /// first and opens a new one after.
    pub(crate) fn draw(
        &mut self,
        device: &metal::DeviceRef,
        command_buffer: &metal::CommandBufferRef,
        glasses: &[Glass],
        target: &metal::TextureRef,
    ) {
        let viewport = Size {
            width: target.width() as i32,
            height: target.height() as i32,
        };
        let max_levels = (viewport.width.min(viewport.height).max(1) as u32)
            .ilog2()
            .saturating_sub(2)
            .min(GLASS_MAX_BLUR_LEVELS);
        let planned: Vec<Planned> = glasses
            .iter()
            .filter_map(|glass| {
                Some(Planned {
                    glass,
                    region: glass.backdrop_region(viewport)?,
                    scissor: glass.scissor(viewport)?,
                    blur: glass.blur_plan(max_levels),
                })
            })
            .collect();
        if planned.is_empty() {
            return;
        }
        self.drawn = true;

        // Glass whose backdrop overlaps another's must see it drawn first, and
        // the chain keeps one region's data per texel, so overlapping regions
        // go in separate runs.
        let mut start = 0;
        for end in 1..=planned.len() {
            let overlaps = end < planned.len()
                && planned[start..end]
                    .iter()
                    .any(|other| intersects(other.region, planned[end].region));
            if end == planned.len() || overlaps {
                self.draw_run(
                    device,
                    command_buffer,
                    &planned[start..end],
                    target,
                    viewport,
                );
                start = end;
            }
        }
    }

    fn draw_run(
        &mut self,
        device: &metal::DeviceRef,
        command_buffer: &metal::CommandBufferRef,
        run: &[Planned],
        target: &metal::TextureRef,
        viewport: Size<i32>,
    ) {
        let depth = run.iter().map(|p| p.blur.levels).max().unwrap_or(0);
        self.ensure_levels(device, viewport, depth);

        let blit = command_buffer.new_blit_command_encoder();
        for planned in run {
            let region = planned.region;
            let origin = metal::MTLOrigin {
                x: region.origin.x as u64,
                y: region.origin.y as u64,
                z: 0,
            };
            blit.copy_from_texture(
                target,
                0,
                0,
                origin,
                metal::MTLSize {
                    width: region.size.width as u64,
                    height: region.size.height as u64,
                    depth: 1,
                },
                &self.levels[0],
                0,
                0,
                origin,
            );
        }
        blit.end_encoding();

        for level in 0..depth {
            let passes: Vec<(GlassBlurUniform, Bounds<i32>)> = run
                .iter()
                .filter(|planned| planned.blur.levels > level)
                .map(|planned| {
                    (
                        GlassBlurUniform::new(
                            glass_level_rect(planned.region, level),
                            planned.blur.offset,
                        ),
                        glass_level_rect(planned.region, level + 1),
                    )
                })
                .collect();
            // Every texel a later pass reads is written here first, so the
            // level's old contents never need loading.
            self.blur_pass(
                command_buffer,
                level,
                level + 1,
                metal::MTLLoadAction::DontCare,
                &passes,
                &self.down_pipeline,
            );
        }
        for level in (1..depth).rev() {
            let passes: Vec<(GlassBlurUniform, Bounds<i32>)> = run
                .iter()
                .filter(|planned| planned.blur.levels > level)
                .map(|planned| {
                    (
                        GlassBlurUniform::new(
                            glass_level_rect(planned.region, level + 1),
                            planned.blur.offset,
                        ),
                        glass_level_rect(planned.region, level),
                    )
                })
                .collect();
            self.blur_pass(
                command_buffer,
                level + 1,
                level,
                metal::MTLLoadAction::Load,
                &passes,
                &self.up_pipeline,
            );
        }

        let encoder = render_encoder(command_buffer, target, metal::MTLLoadAction::Load);
        encoder.set_render_pipeline_state(&self.glass_pipeline);
        encoder.set_fragment_sampler_state(0, Some(&self.sampler));
        encoder.set_fragment_texture(0, Some(&self.levels[0]));
        encoder.set_fragment_texture(1, Some(&self.levels[1]));
        for planned in run {
            let blurred_level = if planned.blur.levels > 0 { 1 } else { 0 };
            let uniform = planned
                .glass
                .uniform(planned.region, blurred_level, viewport);
            let bytes = uniform.as_bytes();
            encoder.set_vertex_bytes(0, bytes.len() as u64, bytes.as_ptr() as *const c_void);
            encoder.set_fragment_bytes(0, bytes.len() as u64, bytes.as_ptr() as *const c_void);
            set_scissor(encoder, planned.scissor, viewport);
            encoder.draw_primitives(metal::MTLPrimitiveType::TriangleStrip, 0, 4);
        }
        encoder.end_encoding();
    }

    fn blur_pass(
        &self,
        command_buffer: &metal::CommandBufferRef,
        source: u32,
        destination: u32,
        load: metal::MTLLoadAction,
        passes: &[(GlassBlurUniform, Bounds<i32>)],
        pipeline: &metal::RenderPipelineStateRef,
    ) {
        if passes.is_empty() {
            return;
        }
        let target = &self.levels[destination as usize];
        let size = Size {
            width: target.width() as i32,
            height: target.height() as i32,
        };
        let encoder = render_encoder(command_buffer, target, load);
        encoder.set_render_pipeline_state(pipeline);
        encoder.set_fragment_sampler_state(0, Some(&self.sampler));
        encoder.set_fragment_texture(0, Some(&self.levels[source as usize]));
        for (uniform, rect) in passes {
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

fn intersects(a: Bounds<i32>, b: Bounds<i32>) -> bool {
    a.origin.x < b.origin.x + b.size.width
        && b.origin.x < a.origin.x + a.size.width
        && a.origin.y < b.origin.y + b.size.height
        && b.origin.y < a.origin.y + a.size.height
}

/// Whether a scene paints glass, its shader layers' included.
pub(crate) fn scene_has_glass(scene: &gpui::Scene) -> bool {
    !scene.glasses.is_empty()
        || scene
            .shader_layers
            .iter()
            .any(|layer| scene_has_glass(&layer.scene))
}
