//! GPU particle system: one million particle budget, compute-simulated,
//! spawned via ring-buffer requests. Particles never touch the CPU after
//! spawn; the CPU only expands burst requests into spawn entries.

use glam::{Vec3, Vec4};
use goetia_core::Pcg32;

pub const PARTICLE_CAPACITY: u32 = 1 << 20; // 1,048,576
pub const MAX_SPAWNS_PER_FRAME: u32 = 32_768;
const WORKGROUP: u32 = 256;

/// A burst request from the game (one API call → up to thousands of
/// particles). Expanded CPU-side with a render-local RNG (never the sim's).
#[derive(Clone, Copy, Debug)]
pub struct ParticleSpawn {
    pub pos: Vec3,
    pub count: u32,
    /// Base velocity added to every particle.
    pub vel: Vec3,
    /// Random direction magnitude added on top (sphere).
    pub spread: f32,
    pub color_from: Vec4,
    pub color_to: Vec4,
    pub size: (f32, f32),
    pub life: (f32, f32),
    pub gravity: f32,
    pub drag: f32,
}

impl Default for ParticleSpawn {
    fn default() -> Self {
        ParticleSpawn {
            pos: Vec3::ZERO,
            count: 16,
            vel: Vec3::ZERO,
            spread: 3.0,
            color_from: Vec4::new(1.0, 0.6, 0.2, 1.0),
            color_to: Vec4::new(1.0, 0.2, 0.05, 1.0),
            size: (0.06, 0.14),
            life: (0.3, 0.9),
            gravity: 6.0,
            drag: 1.5,
        }
    }
}

/// GPU-side particle: 64 bytes.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuParticle {
    pos_life: [f32; 4],
    vel_size: [f32; 4],
    color: [f32; 4],
    /// x: gravity, y: drag, z: max_life, w: unused.
    params: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SimParams {
    dt: f32,
    spawn_count: u32,
    cursor: u32,
    capacity: u32,
}

pub struct ParticleSystem {
    #[allow(dead_code)] // keeps the buffer alive alongside its bind groups
    particles: wgpu::Buffer,
    spawns: wgpu::Buffer,
    params: wgpu::Buffer,
    sim_bind: wgpu::BindGroup,
    draw_bind: wgpu::BindGroup,
    spawn_pipeline: wgpu::ComputePipeline,
    sim_pipeline: wgpu::ComputePipeline,
    draw_pipeline: wgpu::RenderPipeline,
    cursor: u32,
    rng: Pcg32,
    staging: Vec<GpuParticle>,
    /// (expiry time, count) ring for the overlay's alive estimate.
    alive_log: std::collections::VecDeque<(f64, u32)>,
    clock: std::time::Instant,
}

impl ParticleSystem {
    pub fn new(device: &wgpu::Device, globals_layout: &wgpu::BindGroupLayout) -> Self {
        let particles = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("particles"),
            size: PARTICLE_CAPACITY as u64 * std::mem::size_of::<GpuParticle>() as u64,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let spawns = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("particle-spawns"),
            size: MAX_SPAWNS_PER_FRAME as u64 * std::mem::size_of::<GpuParticle>() as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("particle-params"),
            size: std::mem::size_of::<SimParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let storage = |read_only| wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        };
        let sim_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("particle-sim"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: storage(false),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: storage(true),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let sim_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("particle-sim"),
            layout: &sim_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: particles.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: spawns.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: params.as_entire_binding() },
            ],
        });

        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("particle-draw"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: storage(true),
                count: None,
            }],
        });
        let draw_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("particle-draw"),
            layout: &draw_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: particles.as_entire_binding() }],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("particles"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/particles.wgsl").into()),
        });

        let sim_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("particle-sim"),
            bind_group_layouts: &[&sim_layout],
            push_constant_ranges: &[],
        });
        let spawn_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("particle-spawn"),
            layout: Some(&sim_pl),
            module: &shader,
            entry_point: "cs_spawn",
            compilation_options: Default::default(),
            cache: None,
        });
        let sim_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("particle-sim"),
            layout: Some(&sim_pl),
            module: &shader,
            entry_point: "cs_sim",
            compilation_options: Default::default(),
            cache: None,
        });

        let draw_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("particle-draw"),
            bind_group_layouts: &[globals_layout, &draw_layout],
            push_constant_ranges: &[],
        });
        let draw_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("particle-draw"),
            layout: Some(&draw_pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_particle",
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_particle",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: crate::HDR_FORMAT,
                    // Additive: emissive sparks over the scene.
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::Zero,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: crate::DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });

        ParticleSystem {
            particles,
            spawns,
            params,
            sim_bind,
            draw_bind,
            spawn_pipeline,
            sim_pipeline,
            draw_pipeline,
            cursor: 0,
            rng: Pcg32::new(0xC0FFEE, 7), // render-side visual RNG, not sim
            staging: Vec::with_capacity(4096),
            alive_log: std::collections::VecDeque::new(),
            clock: std::time::Instant::now(),
        }
    }

    /// Expand bursts, upload, and encode spawn+sim compute passes.
    pub fn dispatch(
        &mut self,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        _globals: &wgpu::BindGroup,
        bursts: &[ParticleSpawn],
        dt: f32,
    ) {
        self.staging.clear();
        let now = self.clock.elapsed().as_secs_f64();
        for b in bursts {
            let n = b.count.min(MAX_SPAWNS_PER_FRAME - self.staging.len() as u32);
            for _ in 0..n {
                // Uniform direction on sphere.
                let z = self.rng.range_f32(-1.0, 1.0);
                let a = self.rng.range_f32(0.0, std::f32::consts::TAU);
                let r = (1.0 - z * z).max(0.0).sqrt();
                let dir = Vec3::new(r * a.cos(), z, r * a.sin());
                let vel = b.vel + dir * self.rng.range_f32(0.0, b.spread);
                let size = self.rng.range_f32(b.size.0, b.size.1);
                let life = self.rng.range_f32(b.life.0, b.life.1);
                let ct = self.rng.next_f32();
                let color = b.color_from.lerp(b.color_to, ct);
                self.staging.push(GpuParticle {
                    pos_life: [b.pos.x, b.pos.y, b.pos.z, life],
                    vel_size: [vel.x, vel.y, vel.z, size],
                    color: color.to_array(),
                    params: [b.gravity, b.drag, life, 0.0],
                });
            }
            if n > 0 {
                self.alive_log.push_back((now + b.life.1 as f64, n));
            }
        }
        while let Some(&(t, _)) = self.alive_log.front() {
            if t < now {
                self.alive_log.pop_front();
            } else {
                break;
            }
        }

        let spawn_count = self.staging.len() as u32;
        if spawn_count > 0 {
            queue.write_buffer(&self.spawns, 0, bytemuck::cast_slice(&self.staging));
        }
        let p = SimParams {
            dt,
            spawn_count,
            cursor: self.cursor,
            capacity: PARTICLE_CAPACITY,
        };
        queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&p));
        self.cursor = (self.cursor + spawn_count) % PARTICLE_CAPACITY;

        let mut cp = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("particles"),
            timestamp_writes: None,
        });
        if spawn_count > 0 {
            cp.set_pipeline(&self.spawn_pipeline);
            cp.set_bind_group(0, &self.sim_bind, &[]);
            cp.dispatch_workgroups(spawn_count.div_ceil(WORKGROUP), 1, 1);
        }
        cp.set_pipeline(&self.sim_pipeline);
        cp.set_bind_group(0, &self.sim_bind, &[]);
        cp.dispatch_workgroups(PARTICLE_CAPACITY / WORKGROUP, 1, 1);
    }

    pub fn draw<'a>(&'a self, rp: &mut wgpu::RenderPass<'a>, globals: &'a wgpu::BindGroup) {
        rp.set_pipeline(&self.draw_pipeline);
        rp.set_bind_group(0, globals, &[]);
        rp.set_bind_group(1, &self.draw_bind, &[]);
        rp.draw(0..6, 0..PARTICLE_CAPACITY);
    }

    /// Rough count of live particles (spawn log, not GPU readback).
    pub fn alive_estimate(&self) -> u32 {
        self.alive_log.iter().map(|(_, n)| n).sum()
    }
}
