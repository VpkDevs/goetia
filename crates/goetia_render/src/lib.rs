//! goetia_render — GPU-driven horde renderer on wgpu.
//!
//! Design center: the pathological frame (hundreds of enemies, thousands of
//! projectiles, 200k+ particles, dozens of lights) is the *normal* frame.
//! Everything draws through per-mesh instance buffers and a single
//! compute-simulated particle system; the fixed isometric camera is exploited
//! for trivially correct sorting and culling.
//!
//! Frame flow: `FrameSubmit` in → instance upload → particle spawn+sim
//! (compute) → HDR forward pass (meshes, lights, emissive) → particle draw
//! (additive) → bloom chain → tonemap/grade composite → UI pass.

pub mod camera;
pub mod mesh;
pub mod particles;
pub mod post;
pub mod ui;

pub use camera::CameraRig;
pub use mesh::{MeshBuilder, MeshHandle};
pub use particles::ParticleSpawn;
pub use ui::{UiBatch, UiRect};

use glam::{Vec3, Vec4};
use std::sync::Arc;

pub const MAX_LIGHTS: usize = 64;
pub const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

// ------------------------------------------------------------------ types

/// One dynamic light. Every projectile can be one; the renderer brute-forces
/// up to [`MAX_LIGHTS`] per frame (brightest-nearest win when over budget).
#[derive(Clone, Copy, Debug)]
pub struct Light {
    pub pos: Vec3,
    pub color: Vec3,
    pub radius: f32,
    pub intensity: f32,
}

/// Per-instance GPU data.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct InstanceRaw {
    pub model: [[f32; 4]; 4],
    /// Base color (linear RGB) + alpha.
    pub color: [f32; 4],
    /// Emissive color * strength in rgb; w = per-instance phase seed.
    pub emissive: [f32; 4],
    /// x: wobble amplitude, y: wobble speed, z: dissolve (0..1), w: unused.
    /// Wobble is the vertex-shader procedural motion that replaces skeletal
    /// animation (bob/flail); dissolve eats the mesh bottom-up for deaths.
    pub anim: [f32; 4],
}

impl InstanceRaw {
    pub fn new(model: glam::Mat4, color: Vec4) -> Self {
        InstanceRaw {
            model: model.to_cols_array_2d(),
            color: color.to_array(),
            emissive: [0.0; 4],
            anim: [0.0; 4],
        }
    }
    pub fn emissive(mut self, color: Vec3, strength: f32) -> Self {
        let phase = self.emissive[3];
        self.emissive = (color * strength).extend(phase).to_array();
        self
    }
    pub fn phase(mut self, seed: f32) -> Self {
        self.emissive[3] = seed;
        self
    }
    pub fn wobble(mut self, amplitude: f32, speed: f32) -> Self {
        self.anim[0] = amplitude;
        self.anim[1] = speed;
        self
    }
    pub fn dissolve(mut self, t: f32) -> Self {
        self.anim[2] = t;
        self
    }
}

/// The DEMONICON palette, shipped in-engine per spec.
pub mod palette {
    use glam::Vec3;
    pub const VOID: Vec3 = Vec3::new(0.024, 0.016, 0.043); // near-black violet
    pub const ASH: Vec3 = Vec3::new(0.16, 0.14, 0.18); // cold grey-violet
    pub const BONE: Vec3 = Vec3::new(0.85, 0.79, 0.65); // pale bone
    pub const BRIMSTONE: Vec3 = Vec3::new(1.0, 0.35, 0.06); // fire orange
    pub const HEX: Vec3 = Vec3::new(0.55, 0.15, 0.95); // occult violet
    pub const ICHOR: Vec3 = Vec3::new(0.12, 0.95, 0.45); // poison green
    pub const BLOOD: Vec3 = Vec3::new(0.75, 0.05, 0.12); // deep red
    pub const GOLD: Vec3 = Vec3::new(1.0, 0.8, 0.25); // loot gold
}

/// Everything the game submits for one rendered frame.
pub struct FrameSubmit {
    /// (mesh, instances) — group instances by mesh for one draw call each.
    pub meshes: Vec<(MeshHandle, Vec<InstanceRaw>)>,
    pub lights: Vec<Light>,
    pub particle_spawns: Vec<ParticleSpawn>,
    pub ui: UiBatch,
    pub ambient: Vec3,
    /// Fog color (xyz) + density (w).
    pub fog: Vec4,
    /// Bloom strength multiplier (1.0 = default look).
    pub bloom: f32,
    /// Seconds to advance the GPU particle sim this frame (already
    /// hitstop/timescale-adjusted so juice freezes particles too).
    pub particle_dt: f32,
}

impl Default for FrameSubmit {
    fn default() -> Self {
        FrameSubmit {
            meshes: Vec::new(),
            lights: Vec::new(),
            particle_spawns: Vec::new(),
            ui: UiBatch::new(),
            ambient: Vec3::new(0.10, 0.09, 0.13),
            fog: Vec4::new(0.03, 0.02, 0.05, 0.018),
            bloom: 1.0,
            particle_dt: 1.0 / 60.0,
        }
    }
}

// ------------------------------------------------------------- gpu globals

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    camera_pos: [f32; 4],
    ambient: [f32; 4],
    fog: [f32; 4],
    /// x: light count, y: time (s), z/w: unused.
    params: [f32; 4],
    lights_pos_radius: [[f32; 4]; MAX_LIGHTS],
    lights_color_intensity: [[f32; 4]; MAX_LIGHTS],
}

#[derive(Default, Clone, Copy)]
pub struct RenderStats {
    pub draw_calls: u32,
    pub instances: u32,
    pub lights: u32,
    pub particles_alive_estimate: u32,
}

struct InstanceBuffer {
    buf: wgpu::Buffer,
    capacity: usize,
}

pub struct Renderer {
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    pub size: (u32, u32),

    globals_buf: wgpu::Buffer,
    globals_bind: wgpu::BindGroup,

    meshes: mesh::MeshStore,
    mesh_pipeline: wgpu::RenderPipeline,
    instance_bufs: Vec<InstanceBuffer>,

    hdr: post::HdrTargets,
    post: post::PostStack,
    pub particles: particles::ParticleSystem,
    ui: ui::UiRenderer,

    start: std::time::Instant,
    pub stats: RenderStats,
}

impl Renderer {
    pub fn new(window: Arc<winit::window::Window>, vsync: bool) -> Renderer {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        let surface = instance.create_surface(window.clone()).expect("create surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("no suitable GPU adapter");
        log::info!("adapter: {}", adapter.get_info().name);
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("goetia"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        ))
        .expect("request device");
        let device = Arc::new(device);
        let queue = Arc::new(queue);

        let caps = surface.get_capabilities(&adapter);
        let format =
            caps.formats.iter().copied().find(|f| f.is_srgb()).unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: if vsync {
                wgpu::PresentMode::AutoVsync
            } else {
                wgpu::PresentMode::AutoNoVsync
            },
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT | wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let globals_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buf.as_entire_binding(),
            }],
        });

        let meshes = mesh::MeshStore::new();
        let mesh_pipeline = Self::build_mesh_pipeline(&device, &globals_layout);
        let hdr = post::HdrTargets::new(&device, config.width, config.height);
        let post = post::PostStack::new(&device, &hdr, format);
        let particles = particles::ParticleSystem::new(&device, &globals_layout);
        let ui = ui::UiRenderer::new(&device, &queue, format);

        Renderer {
            device,
            queue,
            surface,
            config,
            size: (size.width.max(1), size.height.max(1)),
            globals_buf,
            globals_bind,
            meshes,
            mesh_pipeline,
            instance_bufs: Vec::new(),
            hdr,
            post,
            particles,
            ui,
            start: std::time::Instant::now(),
            stats: RenderStats::default(),
        }
    }

    fn build_mesh_pipeline(
        device: &wgpu::Device,
        globals_layout: &wgpu::BindGroupLayout,
    ) -> wgpu::RenderPipeline {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mesh"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/mesh.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh"),
            bind_group_layouts: &[globals_layout],
            push_constant_ranges: &[],
        });
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<mesh::Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3],
        };
        let instance_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<InstanceRaw>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![
                2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4,
                6 => Float32x4,
                7 => Float32x4,
                8 => Float32x4
            ],
        };
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mesh"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                compilation_options: Default::default(),
                buffers: &[vertex_layout, instance_layout],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        })
    }

    pub fn register_mesh(&mut self, builder: MeshBuilder) -> MeshHandle {
        self.meshes.register(&self.device, builder)
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        self.size = (w, h);
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
        self.hdr = post::HdrTargets::new(&self.device, w, h);
        self.post.rebind(&self.device, &self.hdr);
    }

    pub fn aspect(&self) -> f32 {
        self.size.0 as f32 / self.size.1 as f32
    }

    /// Render one frame with the (already interpolated) camera rig.
    pub fn render(&mut self, cam: &CameraRig, frame: &mut FrameSubmit) {
        let surface_tex = match self.surface.get_current_texture() {
            Ok(t) => t,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            Err(e) => {
                log::warn!("surface error: {e:?}");
                return;
            }
        };
        let view = surface_tex.texture.create_view(&Default::default());

        // ---- globals
        let aspect = self.aspect();
        let vp = cam.view_proj(aspect);
        let mut g = Globals {
            view_proj: vp.to_cols_array_2d(),
            camera_pos: cam.eye().extend(1.0).to_array(),
            ambient: frame.ambient.extend(0.0).to_array(),
            fog: frame.fog.to_array(),
            params: [
                frame.lights.len().min(MAX_LIGHTS) as f32,
                self.start.elapsed().as_secs_f32(),
                0.0,
                0.0,
            ],
            lights_pos_radius: [[0.0; 4]; MAX_LIGHTS],
            lights_color_intensity: [[0.0; 4]; MAX_LIGHTS],
        };
        if frame.lights.len() > MAX_LIGHTS {
            let target = cam.target;
            frame.lights.sort_by(|a, b| {
                let ka = a.intensity / (1.0 + a.pos.distance_squared(target));
                let kb = b.intensity / (1.0 + b.pos.distance_squared(target));
                kb.partial_cmp(&ka).unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        for (i, l) in frame.lights.iter().take(MAX_LIGHTS).enumerate() {
            g.lights_pos_radius[i] = [l.pos.x, l.pos.y, l.pos.z, l.radius];
            g.lights_color_intensity[i] = [l.color.x, l.color.y, l.color.z, l.intensity];
        }
        self.queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&g));

        // ---- instance buffers (grow-only, one per mesh slot)
        while self.instance_bufs.len() < frame.meshes.len() {
            self.instance_bufs.push(InstanceBuffer {
                buf: self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("instances"),
                    size: 1024 * std::mem::size_of::<InstanceRaw>() as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                capacity: 1024,
            });
        }
        let mut total_instances = 0u32;
        for (i, (_, insts)) in frame.meshes.iter().enumerate() {
            total_instances += insts.len() as u32;
            let need = insts.len();
            let ib = &mut self.instance_bufs[i];
            if need > ib.capacity {
                let cap = need.next_power_of_two();
                ib.buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("instances"),
                    size: (cap * std::mem::size_of::<InstanceRaw>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                ib.capacity = cap;
            }
            if !insts.is_empty() {
                self.queue.write_buffer(&ib.buf, 0, bytemuck::cast_slice(insts));
            }
        }

        let mut enc = self.device.create_command_encoder(&Default::default());

        // ---- particles: spawn + sim (compute)
        self.particles.dispatch(
            &self.queue,
            &mut enc,
            &self.globals_bind,
            &frame.particle_spawns,
            frame.particle_dt,
        );

        // ---- main HDR pass
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hdr"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.hdr.color_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: frame.fog.x as f64,
                            g: frame.fog.y as f64,
                            b: frame.fog.z as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.hdr.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            rp.set_pipeline(&self.mesh_pipeline);
            rp.set_bind_group(0, &self.globals_bind, &[]);
            let mut draws = 0;
            for (i, (mh, insts)) in frame.meshes.iter().enumerate() {
                if insts.is_empty() {
                    continue;
                }
                let m = self.meshes.get(*mh);
                rp.set_vertex_buffer(0, m.vertices.slice(..));
                rp.set_vertex_buffer(1, self.instance_bufs[i].buf.slice(..));
                rp.set_index_buffer(m.indices.slice(..), wgpu::IndexFormat::Uint32);
                rp.draw_indexed(0..m.index_count, 0, 0..insts.len() as u32);
                draws += 1;
            }
            self.particles.draw(&mut rp, &self.globals_bind);
            self.stats.draw_calls = draws + 1;
        }

        // ---- bloom + composite to swapchain
        self.post.run(&self.queue, &mut enc, &view, frame.bloom);

        // ---- UI on top
        self.ui.render(&self.device, &self.queue, &mut enc, &view, self.size, &frame.ui);

        self.queue.submit([enc.finish()]);
        surface_tex.present();

        self.stats.instances = total_instances;
        self.stats.lights = frame.lights.len() as u32;
        self.stats.particles_alive_estimate = self.particles.alive_estimate();
    }
}
