use gpui::{
    Bounds, GLASS_MAX_BLUR_LEVELS, GLASS_SHADER, Glass, GlassBlurPass, GlassBlurUniform,
    GlassUniform, Size, WindowCornerMask, glass_level_size, plan_glass,
};
use std::num::NonZeroU64;

/// Slots a glass draw can take in the uniform buffer: its own block, and one
/// per blur pass down and back up the chain.
const SLOTS_PER_GLASS: u64 = 1 + 2 * GLASS_MAX_BLUR_LEVELS as u64;

/// The chain grows in steps of this many level 0 texels, so a glass that
/// moves or springs does not reallocate it every frame.
const CHAIN_STEP: u32 = 256;

/// Pipelines and the blur chain for [`Glass`], created when a scene first
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
    /// The largest chain any run needed this frame, in level 0 texels.
    peak: Size<i32>,
}

/// Glass draws waiting for a pass over the target.
pub(crate) struct PendingGlass {
    bind_group: wgpu::BindGroup,
    draws: Vec<(u32, Bounds<i32>)>,
}

struct Level {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    /// Reads this level as a blur pass's source.
    bind_group: wgpu::BindGroup,
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
            peak: Size::default(),
        }
    }

    /// Starts a frame that draws at most `glass_count` glasses.
    pub(crate) fn begin_frame(&mut self, device: &wgpu::Device, glass_count: usize) {
        self.peak = Size::default();
        self.uniform_bytes.clear();
        let slot = (size_of::<GlassUniform>() as u64).next_multiple_of(self.uniform_alignment);
        let needed = glass_count as u64 * SLOTS_PER_GLASS * slot;
        if self
            .uniforms
            .as_ref()
            .is_some_and(|buffer| buffer.size() >= needed)
        {
            return;
        }
        self.uniforms = Some(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("glass_uniforms"),
            size: needed.next_power_of_two().max(16 * 1024),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        // Bind groups name the buffer they read.
        self.release_textures();
    }

    /// Uploads the frame's uniform blocks, and frees a chain the frame did
    /// not use, or used little of. Call before submitting.
    pub(crate) fn finish_frame(&mut self, queue: &wgpu::Queue) {
        if let Some(level) = self.levels.first() {
            let area = level.texture.width() as i64 * level.texture.height() as i64;
            let peak = self.peak.width as i64 * self.peak.height as i64;
            if area > peak * 4 {
                self.release_textures();
            }
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

    fn push_uniform(&mut self, bytes: &[u8]) -> u32 {
        let offset = self.uniform_bytes.len();
        let slot = (bytes.len() as u64).next_multiple_of(self.uniform_alignment) as usize;
        self.uniform_bytes.resize(offset + slot, 0);
        self.uniform_bytes[offset..offset + bytes.len()].copy_from_slice(bytes);
        offset as u32
    }

    /// Makes the chain span `extent` level 0 texels and reach `depth` levels.
    fn ensure_chain(&mut self, device: &wgpu::Device, extent: Size<i32>, depth: u32) {
        self.peak = Size {
            width: self.peak.width.max(extent.width),
            height: self.peak.height.max(extent.height),
        };
        let fits = self.levels.first().is_some_and(|level| {
            level.texture.width() as i32 >= extent.width
                && level.texture.height() as i32 >= extent.height
        });
        if !fits {
            self.release_textures();
        }
        let Some(buffer) = self.uniforms.clone() else {
            return;
        };
        let capacity = match self.levels.first() {
            Some(level) => Size {
                width: level.texture.width() as i32,
                height: level.texture.height() as i32,
            },
            None => Size {
                width: (extent.width.max(1) as u32).next_multiple_of(CHAIN_STEP) as i32,
                height: (extent.height.max(1) as u32).next_multiple_of(CHAIN_STEP) as i32,
            },
        };
        while self.levels.len() <= depth.max(1) as usize {
            let size = glass_level_size(capacity, self.levels.len() as u32);
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("glass_level"),
                size: wgpu::Extent3d {
                    width: size.width as u32,
                    height: size.height as u32,
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
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
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
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                ],
            });
            self.levels.push(Level {
                texture,
                view,
                bind_group,
            });
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
    /// The caller ends its render pass first. Every run but the last draws
    /// in a pass of its own; the last run's draws come back, for the caller
    /// to issue with [`Self::draw_pending`] at the start of the pass that
    /// resumes the scene, which saves a tiled GPU a load and store of the
    /// whole target.
    pub(crate) fn draw(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        glasses: &[Glass],
        target: &wgpu::Texture,
        target_view: &wgpu::TextureView,
        window_mask: Option<WindowCornerMask>,
    ) -> Option<PendingGlass> {
        let viewport = Size {
            width: target.width() as i32,
            height: target.height() as i32,
        };
        let mut pending = None;
        for run in plan_glass(glasses, viewport, window_mask) {
            if let Some(pending) = pending.take() {
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
                self.draw_pending(&mut pass, pending, viewport);
            }
            self.ensure_chain(device, run.extent, run.depth);
            let Some(bind_group) = self.glass_bind_group.clone() else {
                return None;
            };

            for (region, destination) in &run.copies {
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: target,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: region.origin.x as u32,
                            y: region.origin.y as u32,
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture: &self.levels[0].texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: destination.x as u32,
                            y: destination.y as u32,
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::Extent3d {
                        width: region.size.width as u32,
                        height: region.size.height as u32,
                        depth_or_array_layers: 1,
                    },
                );
            }

            for pass in &run.passes {
                self.blur_pass(encoder, pass);
            }

            let draws = run
                .draws
                .iter()
                .map(|(uniform, scissor)| (self.push_uniform(uniform.as_bytes()), *scissor))
                .collect();
            pending = Some(PendingGlass { bind_group, draws });
        }
        pending
    }

    /// Issues a run's glass draws into `pass`, a pass over the target, and
    /// gives the pass back its full scissor.
    pub(crate) fn draw_pending(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        pending: PendingGlass,
        viewport: Size<i32>,
    ) {
        pass.set_pipeline(&self.glass_pipeline);
        for (offset, scissor) in pending.draws {
            pass.set_bind_group(0, &pending.bind_group, &[offset]);
            pass.set_scissor_rect(
                scissor.origin.x as u32,
                scissor.origin.y as u32,
                scissor.size.width as u32,
                scissor.size.height as u32,
            );
            pass.draw(0..4, 0..1);
        }
        pass.set_scissor_rect(0, 0, viewport.width as u32, viewport.height as u32);
    }

    fn blur_pass(&mut self, encoder: &mut wgpu::CommandEncoder, pass: &GlassBlurPass) {
        if pass.draws.is_empty() {
            return;
        }
        let draws: Vec<(u32, Bounds<i32>)> = pass
            .draws
            .iter()
            .map(|(uniform, rect)| (self.push_uniform(uniform.as_bytes()), *rect))
            .collect();
        let source = &self.levels[pass.source as usize];
        let target = &self.levels[pass.destination as usize];
        let width = target.texture.width() as i32;
        let height = target.texture.height() as i32;
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(if pass.down { "glass_down" } else { "glass_up" }),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: if pass.down {
                        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                    } else {
                        wgpu::LoadOp::Load
                    },
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            ..Default::default()
        });
        render_pass.set_pipeline(if pass.down {
            &self.down_pipeline
        } else {
            &self.up_pipeline
        });
        for (offset, rect) in draws {
            let left = rect.origin.x.clamp(0, width);
            let top = rect.origin.y.clamp(0, height);
            let right = (rect.origin.x + rect.size.width).clamp(0, width);
            let bottom = (rect.origin.y + rect.size.height).clamp(0, height);
            if right <= left || bottom <= top {
                continue;
            }
            render_pass.set_bind_group(0, &source.bind_group, &[offset]);
            render_pass.set_scissor_rect(
                left as u32,
                top as u32,
                (right - left) as u32,
                (bottom - top) as u32,
            );
            render_pass.draw(0..3, 0..1);
        }
    }
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
