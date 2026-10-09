use gpui::{
    Bounds, GLASS_MAX_BLUR_LEVELS, GLASS_SHADER, Glass, GlassBlurPlan, GlassBlurUniform,
    GlassUniform, Size, glass_level_rect,
};
use smallvec::SmallVec;
use std::num::NonZeroU64;

/// Slots a glass draw can take in the uniform buffer: its own block, and one
/// per blur pass down and back up the chain.
const SLOTS_PER_GLASS: u64 = 1 + 2 * GLASS_MAX_BLUR_LEVELS as u64;

/// Pipelines and scratch textures for [`Glass`], created when a scene first
/// paints glass.
pub(crate) struct GlassResources {
    glass_layout: wgpu::BindGroupLayout,
    blur_layout: wgpu::BindGroupLayout,
    glass_pipeline: wgpu::RenderPipeline,
    down_pipeline: wgpu::RenderPipeline,
    up_pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    format: wgpu::TextureFormat,
    uniform_alignment: u64,
    uniforms: Option<wgpu::Buffer>,
    uniform_bytes: Vec<u8>,
    levels: Vec<Level>,
    glass_bind_group: Option<wgpu::BindGroup>,
    drawn: bool,
}

struct Level {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    /// Reads this level as a blur pass's source.
    bind_group: Option<wgpu::BindGroup>,
}

/// A glass draw with its backdrop region and blur worked out.
struct Planned<'a> {
    glass: &'a Glass,
    region: Bounds<i32>,
    scissor: Bounds<i32>,
    blur: GlassBlurPlan,
}

impl GlassResources {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let uniform_entry = |binding, size: usize| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: true,
                min_binding_size: NonZeroU64::new(size as u64),
            },
            count: None,
        };
        let sampler_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let glass_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glass_layout"),
            entries: &[
                uniform_entry(0, size_of::<GlassUniform>()),
                sampler_entry(1),
                texture_entry(2),
                texture_entry(3),
            ],
        });
        let blur_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glass_blur_layout"),
            entries: &[
                uniform_entry(4, size_of::<GlassBlurUniform>()),
                sampler_entry(5),
                texture_entry(6),
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glass"),
            source: wgpu::ShaderSource::Wgsl(GLASS_SHADER.into()),
        });
        let pipeline = |label: &str,
                        layout: &wgpu::BindGroupLayout,
                        vertex: &str,
                        fragment: &str,
                        topology: wgpu::PrimitiveTopology,
                        blend: Option<wgpu::BlendState>| {
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[Some(layout)],
                immediate_size: 0,
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(vertex),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fragment),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let glass_pipeline = pipeline(
            "glass",
            &glass_layout,
            "vs_glass",
            "fs_glass",
            wgpu::PrimitiveTopology::TriangleStrip,
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        );
        let down_pipeline = pipeline(
            "glass_down",
            &blur_layout,
            "vs_glass_blur",
            "fs_glass_down",
            wgpu::PrimitiveTopology::TriangleList,
            None,
        );
        let up_pipeline = pipeline(
            "glass_up",
            &blur_layout,
            "vs_glass_blur",
            "fs_glass_up",
            wgpu::PrimitiveTopology::TriangleList,
            None,
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("glass_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            glass_layout,
            blur_layout,
            glass_pipeline,
            down_pipeline,
            up_pipeline,
            sampler,
            format,
            uniform_alignment: device.limits().min_uniform_buffer_offset_alignment as u64,
            uniforms: None,
            uniform_bytes: Vec::new(),
            levels: Vec::new(),
            glass_bind_group: None,
            drawn: false,
        }
    }

    /// Starts a frame that draws at most `glass_count` glasses.
    pub(crate) fn begin_frame(&mut self, device: &wgpu::Device, glass_count: usize) {
        self.drawn = false;
        self.uniform_bytes.clear();
        let needed = glass_count as u64 * SLOTS_PER_GLASS * self.slot_size(512);
        if self
            .uniforms
            .as_ref()
            .is_some_and(|buffer| buffer.size() >= needed)
        {
            return;
        }
        let size = needed.next_power_of_two().max(16 * 1024);
        self.uniforms = Some(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("glass_uniforms"),
            size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        self.glass_bind_group = None;
        for level in &mut self.levels {
            level.bind_group = None;
        }
    }

    /// Uploads the frame's uniform blocks; call before submitting.
    pub(crate) fn finish_frame(&mut self, queue: &wgpu::Queue) {
        if !self.drawn {
            self.levels.clear();
            self.glass_bind_group = None;
        }
        if let Some(buffer) = &self.uniforms
            && !self.uniform_bytes.is_empty()
        {
            queue.write_buffer(buffer, 0, &self.uniform_bytes);
        }
    }

    pub(crate) fn release_textures(&mut self) {
        self.levels.clear();
        self.glass_bind_group = None;
    }

    fn slot_size(&self, size: u64) -> u64 {
        size.next_multiple_of(self.uniform_alignment)
    }

    fn push_uniform(&mut self, bytes: &[u8]) -> u32 {
        let offset = self.uniform_bytes.len();
        let slot = self.slot_size(bytes.len() as u64) as usize;
        self.uniform_bytes.resize(offset + slot, 0);
        self.uniform_bytes[offset..offset + bytes.len()].copy_from_slice(bytes);
        offset as u32
    }

    fn ensure_levels(&mut self, device: &wgpu::Device, size: Size<i32>, depth: u32) {
        let fits = self.levels.first().is_some_and(|level| {
            level.texture.width() == size.width as u32
                && level.texture.height() == size.height as u32
        });
        if !fits {
            self.levels.clear();
            self.glass_bind_group = None;
        }
        while self.levels.len() <= depth.max(1) as usize {
            let level = self.levels.len() as u32;
            let width = (size.width as u32).div_ceil(1 << level).max(1);
            let height = (size.height as u32).div_ceil(1 << level).max(1);
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("glass_level"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.levels.push(Level {
                texture,
                view,
                bind_group: None,
            });
        }
        let Some(buffer) = self.uniforms.clone() else {
            return;
        };
        for level in &mut self.levels {
            if level.bind_group.is_none() {
                level.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("glass_blur_source"),
                    layout: &self.blur_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: &buffer,
                                offset: 0,
                                size: NonZeroU64::new(size_of::<GlassBlurUniform>() as u64),
                            }),
                        },
                        wgpu::BindGroupEntry {
                            binding: 5,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 6,
                            resource: wgpu::BindingResource::TextureView(&level.view),
                        },
                    ],
                }));
            }
        }
        if self.glass_bind_group.is_none() {
            self.glass_bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("glass_backdrop"),
                layout: &self.glass_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &buffer,
                            offset: 0,
                            size: NonZeroU64::new(size_of::<GlassUniform>() as u64),
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&self.levels[0].view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&self.levels[1].view),
                    },
                ],
            }));
        }
    }

    /// Draws a batch of glass over `target`, which must be readable by copy.
    /// The caller ends its render pass first and begins a new one after.
    pub(crate) fn draw(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        glasses: &[Glass],
        target: &wgpu::Texture,
        target_view: &wgpu::TextureView,
    ) {
        let viewport = Size {
            width: target.width() as i32,
            height: target.height() as i32,
        };
        let max_levels = (viewport.width.min(viewport.height).max(1) as u32)
            .ilog2()
            .saturating_sub(2)
            .min(GLASS_MAX_BLUR_LEVELS);
        let planned: SmallVec<[Planned; 8]> = glasses
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
                    encoder,
                    &planned[start..end],
                    target,
                    target_view,
                    viewport,
                );
                start = end;
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_run(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        run: &[Planned],
        target: &wgpu::Texture,
        target_view: &wgpu::TextureView,
        viewport: Size<i32>,
    ) {
        let depth = run.iter().map(|p| p.blur.levels).max().unwrap_or(0);
        self.ensure_levels(device, viewport, depth);

        for planned in run {
            let region = planned.region;
            let origin = wgpu::Origin3d {
                x: region.origin.x as u32,
                y: region.origin.y as u32,
                z: 0,
            };
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: target,
                    mip_level: 0,
                    origin,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &self.levels[0].texture,
                    mip_level: 0,
                    origin,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: region.size.width as u32,
                    height: region.size.height as u32,
                    depth_or_array_layers: 1,
                },
            );
        }

        for level in 0..depth {
            let slots: SmallVec<[(u32, Bounds<i32>); 8]> = run
                .iter()
                .filter(|planned| planned.blur.levels > level)
                .map(|planned| {
                    let uniform = GlassBlurUniform::new(
                        glass_level_rect(planned.region, level),
                        planned.blur.offset,
                    );
                    (
                        self.push_uniform(uniform.as_bytes()),
                        glass_level_rect(planned.region, level + 1),
                    )
                })
                .collect();
            self.blur_pass(
                encoder,
                level,
                level + 1,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                &slots,
                true,
            );
        }
        for level in (1..depth).rev() {
            let slots: SmallVec<[(u32, Bounds<i32>); 8]> = run
                .iter()
                .filter(|planned| planned.blur.levels > level)
                .map(|planned| {
                    let uniform = GlassBlurUniform::new(
                        glass_level_rect(planned.region, level + 1),
                        planned.blur.offset,
                    );
                    (
                        self.push_uniform(uniform.as_bytes()),
                        glass_level_rect(planned.region, level),
                    )
                })
                .collect();
            self.blur_pass(encoder, level + 1, level, wgpu::LoadOp::Load, &slots, false);
        }

        let draws: SmallVec<[(u32, Bounds<i32>); 8]> = run
            .iter()
            .map(|planned| {
                let blurred_level = if planned.blur.levels > 0 { 1 } else { 0 };
                let uniform = planned
                    .glass
                    .uniform(planned.region, blurred_level, viewport);
                (self.push_uniform(uniform.as_bytes()), planned.scissor)
            })
            .collect();
        let Some(bind_group) = &self.glass_bind_group else {
            return;
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("glass_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.glass_pipeline);
        for (offset, scissor) in draws {
            pass.set_bind_group(0, bind_group, &[offset]);
            pass.set_scissor_rect(
                scissor.origin.x as u32,
                scissor.origin.y as u32,
                scissor.size.width as u32,
                scissor.size.height as u32,
            );
            pass.draw(0..4, 0..1);
        }
    }

    fn blur_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        source: u32,
        destination: u32,
        load: wgpu::LoadOp<wgpu::Color>,
        slots: &[(u32, Bounds<i32>)],
        down: bool,
    ) {
        if slots.is_empty() {
            return;
        }
        let Some(bind_group) = &self.levels[source as usize].bind_group else {
            return;
        };
        let target = &self.levels[destination as usize];
        let width = target.texture.width() as i32;
        let height = target.texture.height() as i32;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(if down { "glass_down" } else { "glass_up" }),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            ..Default::default()
        });
        pass.set_pipeline(if down {
            &self.down_pipeline
        } else {
            &self.up_pipeline
        });
        for (offset, rect) in slots {
            let left = rect.origin.x.clamp(0, width);
            let top = rect.origin.y.clamp(0, height);
            let right = (rect.origin.x + rect.size.width).clamp(0, width);
            let bottom = (rect.origin.y + rect.size.height).clamp(0, height);
            if right <= left || bottom <= top {
                continue;
            }
            pass.set_bind_group(0, bind_group, &[*offset]);
            pass.set_scissor_rect(
                left as u32,
                top as u32,
                (right - left) as u32,
                (bottom - top) as u32,
            );
            pass.draw(0..3, 0..1);
        }
    }
}

fn intersects(a: Bounds<i32>, b: Bounds<i32>) -> bool {
    a.origin.x < b.origin.x + b.size.width
        && b.origin.x < a.origin.x + a.size.width
        && a.origin.y < b.origin.y + b.size.height
        && b.origin.y < a.origin.y + a.size.height
}

/// How many glasses a scene paints, its shader layers' included.
pub(crate) fn glass_count(scene: &gpui::Scene) -> usize {
    scene.glasses.len()
        + scene
            .shader_layers
            .iter()
            .map(|layer| glass_count(&layer.scene))
            .sum::<usize>()
}
