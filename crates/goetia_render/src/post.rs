//! Post stack: HDR targets, dual-filter bloom chain, tonemap/grade composite.

pub struct HdrTargets {
    pub color: wgpu::Texture,
    pub color_view: wgpu::TextureView,
    pub depth_view: wgpu::TextureView,
    pub width: u32,
    pub height: u32,
}

impl HdrTargets {
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("hdr-color"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: crate::HDR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: crate::DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let color_view = color.create_view(&Default::default());
        let depth_view = depth.create_view(&Default::default());
        HdrTargets { color, color_view, depth_view, width, height }
    }
}

const BLOOM_LEVELS: usize = 5;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PostParams {
    v: [f32; 4], // threshold, knee, strength, unused
}

struct BloomLevel {
    view: wgpu::TextureView,
    /// Bind group sampling *this* level as source.
    bind: wgpu::BindGroup,
}

pub struct PostStack {
    sampler: wgpu::Sampler,
    params: wgpu::Buffer,
    layout_src: wgpu::BindGroupLayout,
    layout_tex: wgpu::BindGroupLayout,
    prefilter: wgpu::RenderPipeline,
    down: wgpu::RenderPipeline,
    up: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    hdr_bind: wgpu::BindGroup,
    levels: Vec<BloomLevel>,
    bloom_top_bind: wgpu::BindGroup,
}

impl PostStack {
    pub fn new(device: &wgpu::Device, hdr: &HdrTargets, out_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("post"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/post.wgsl").into()),
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("post"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("post-params"),
            size: std::mem::size_of::<PostParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let layout_src = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post-src"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let layout_tex = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post-tex"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });

        let make = |label: &str,
                    entry: &str,
                    layouts: &[&wgpu::BindGroupLayout],
                    format: wgpu::TextureFormat,
                    blend: Option<wgpu::BlendState>| {
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: layouts,
                push_constant_ranges: &[],
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pl),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: "vs_fullscreen",
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: entry,
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };

        let additive = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::OVER,
        };

        let prefilter = make("bloom-prefilter", "fs_prefilter", &[&layout_src], crate::HDR_FORMAT, None);
        let down = make("bloom-down", "fs_down", &[&layout_src], crate::HDR_FORMAT, None);
        let up = make("bloom-up", "fs_up", &[&layout_src], crate::HDR_FORMAT, Some(additive));
        let composite = make("composite", "fs_composite", &[&layout_src, &layout_tex], out_format, None);

        let (hdr_bind, levels, bloom_top_bind) =
            build_binds(device, &layout_src, &layout_tex, &sampler, &params, hdr);

        PostStack {
            sampler,
            params,
            layout_src,
            layout_tex,
            prefilter,
            down,
            up,
            composite,
            hdr_bind,
            levels,
            bloom_top_bind,
        }
    }

    /// (Re)create all size-dependent resources (call after resize).
    pub fn rebind(&mut self, device: &wgpu::Device, hdr: &HdrTargets) {
        let (hdr_bind, levels, bloom_top_bind) = build_binds(
            device,
            &self.layout_src,
            &self.layout_tex,
            &self.sampler,
            &self.params,
            hdr,
        );
        self.hdr_bind = hdr_bind;
        self.levels = levels;
        self.bloom_top_bind = bloom_top_bind;
    }

    pub fn run(
        &self,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        swap_view: &wgpu::TextureView,
        strength: f32,
    ) {
        queue.write_buffer(
            &self.params,
            0,
            bytemuck::bytes_of(&PostParams { v: [1.0, 0.5, 0.35 * strength, 0.0] }),
        );

        let pass = |target: &wgpu::TextureView,
                        pipeline: &wgpu::RenderPipeline,
                        src: &wgpu::BindGroup,
                        extra: Option<&wgpu::BindGroup>,
                        load: wgpu::LoadOp<wgpu::Color>,
                        enc: &mut wgpu::CommandEncoder| {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("post"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            rp.set_pipeline(pipeline);
            rp.set_bind_group(0, src, &[]);
            if let Some(e) = extra {
                rp.set_bind_group(1, e, &[]);
            }
            rp.draw(0..3, 0..1);
        };

        let clear = wgpu::LoadOp::Clear(wgpu::Color::BLACK);
        // Prefilter HDR -> level 0.
        pass(&self.levels[0].view, &self.prefilter, &self.hdr_bind, None, clear, enc);
        // Downsample chain.
        for i in 0..self.levels.len() - 1 {
            pass(&self.levels[i + 1].view, &self.down, &self.levels[i].bind, None, clear, enc);
        }
        // Upsample additively back to level 0.
        for i in (0..self.levels.len() - 1).rev() {
            pass(&self.levels[i].view, &self.up, &self.levels[i + 1].bind, None, wgpu::LoadOp::Load, enc);
        }
        // Composite to swapchain.
        pass(swap_view, &self.composite, &self.hdr_bind, Some(&self.bloom_top_bind), clear, enc);
    }
}

/// Build every size-dependent bind group: HDR source bind, bloom chain
/// levels, and the composite's bloom-top texture bind.
fn build_binds(
    device: &wgpu::Device,
    layout_src: &wgpu::BindGroupLayout,
    layout_tex: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    params: &wgpu::Buffer,
    hdr: &HdrTargets,
) -> (wgpu::BindGroup, Vec<BloomLevel>, wgpu::BindGroup) {
    let src_bind = |view: &wgpu::TextureView| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("post-src"),
            layout: layout_src,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry { binding: 2, resource: params.as_entire_binding() },
            ],
        })
    };

    let hdr_bind = src_bind(&hdr.color_view);
    let mut w = (hdr.width / 2).max(1);
    let mut h = (hdr.height / 2).max(1);
    let mut levels = Vec::new();
    for _ in 0..BLOOM_LEVELS {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bloom"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: crate::HDR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        let bind = src_bind(&view);
        levels.push(BloomLevel { view, bind });
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }
    let bloom_top_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("bloom-top"),
        layout: layout_tex,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&levels[0].view),
        }],
    });
    (hdr_bind, levels, bloom_top_bind)
}
