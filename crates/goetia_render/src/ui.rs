//! 2D UI layer: batched quads + atlas font text, immediate-mode-flavored.
//! The font is a hand-authored 5×7 bitmap (no asset files, crisp at integer
//! scales) — the sanctioned MSDF fallback.

use glam::{Vec2, Vec4};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct UiVertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

#[derive(Clone, Copy, Debug)]
pub struct UiRect {
    pub pos: Vec2,
    pub size: Vec2,
    pub color: Vec4,
}

pub const GLYPH_W: f32 = 5.0;
pub const GLYPH_H: f32 = 7.0;
pub const GLYPH_ADVANCE: f32 = 6.0;

const ATLAS_COLS: u32 = 32;
const ATLAS_W: u32 = 256; // 32 cells × 8 px — bytes_per_row aligned to 256
const ATLAS_H: u32 = 24; // 3 rows × 8 px
const CELL: u32 = 8;
/// Atlas cell for the solid-white block (used by rects); maps from char 127.
const WHITE_INDEX: u32 = 95;

/// CPU-side draw list. Build one per frame (or reuse with `clear`).
#[derive(Default)]
pub struct UiBatch {
    verts: Vec<UiVertex>,
}

impl UiBatch {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.verts.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.verts.is_empty()
    }

    fn cell_uv(index: u32) -> ([f32; 2], [f32; 2]) {
        let col = index % ATLAS_COLS;
        let row = index / ATLAS_COLS;
        let u0 = (col * CELL) as f32 / ATLAS_W as f32;
        let v0 = (row * CELL) as f32 / ATLAS_H as f32;
        let u1 = (col * CELL + GLYPH_W as u32) as f32 / ATLAS_W as f32;
        let v1 = (row * CELL + GLYPH_H as u32) as f32 / ATLAS_H as f32;
        ([u0, v0], [u1, v1])
    }

    fn quad(&mut self, pos: Vec2, size: Vec2, uv0: [f32; 2], uv1: [f32; 2], color: Vec4) {
        let c = color.to_array();
        let (x0, y0) = (pos.x, pos.y);
        let (x1, y1) = (pos.x + size.x, pos.y + size.y);
        let v = |x: f32, y: f32, u: f32, w: f32| UiVertex { pos: [x, y], uv: [u, w], color: c };
        self.verts.extend([
            v(x0, y0, uv0[0], uv0[1]),
            v(x1, y0, uv1[0], uv0[1]),
            v(x1, y1, uv1[0], uv1[1]),
            v(x0, y0, uv0[0], uv0[1]),
            v(x1, y1, uv1[0], uv1[1]),
            v(x0, y1, uv0[0], uv1[1]),
        ]);
    }

    /// Solid rectangle in pixel coordinates.
    pub fn rect(&mut self, pos: Vec2, size: Vec2, color: Vec4) {
        let (uv0, uv1) = Self::cell_uv(WHITE_INDEX);
        // Sample the middle of the white cell to dodge bleed.
        let mid = [(uv0[0] + uv1[0]) * 0.5, (uv0[1] + uv1[1]) * 0.5];
        self.quad(pos, size, mid, mid, color);
    }

    /// Draw text; returns pixel width. `scale` 2.0 = 10×14 px glyphs.
    pub fn text(&mut self, pos: Vec2, scale: f32, color: Vec4, s: &str) -> f32 {
        let mut x = pos.x;
        for ch in s.chars() {
            let ch = ch.to_ascii_uppercase();
            let idx = glyph_index(ch);
            if ch != ' ' {
                let (uv0, uv1) = Self::cell_uv(idx);
                self.quad(
                    Vec2::new(x, pos.y),
                    Vec2::new(GLYPH_W * scale, GLYPH_H * scale),
                    uv0,
                    uv1,
                    color,
                );
            }
            x += GLYPH_ADVANCE * scale;
        }
        x - pos.x
    }

    /// Text with a 1px drop shadow (readability over bright scenes).
    pub fn text_shadowed(&mut self, pos: Vec2, scale: f32, color: Vec4, s: &str) -> f32 {
        let sh = Vec4::new(0.0, 0.0, 0.0, color.w * 0.8);
        self.text(pos + Vec2::splat(scale.max(1.0)), scale, sh, s);
        self.text(pos, scale, color, s)
    }

    pub fn text_width(scale: f32, s: &str) -> f32 {
        s.chars().count() as f32 * GLYPH_ADVANCE * scale
    }
}

fn glyph_index(ch: char) -> u32 {
    let c = ch as u32;
    if (32..127).contains(&c) {
        c - 32
    } else {
        (b'?' - 32) as u32
    }
}

pub struct UiRenderer {
    pipeline: wgpu::RenderPipeline,
    bind: wgpu::BindGroup,
    screen_buf: wgpu::Buffer,
    vbuf: wgpu::Buffer,
    vcap: usize,
}

impl UiRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let atlas_data = build_atlas();
        let atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("font-atlas"),
            size: wgpu::Extent3d { width: ATLAS_W, height: ATLAS_H, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &atlas,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &atlas_data,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(ATLAS_W),
                rows_per_image: Some(ATLAS_H),
            },
            wgpu::Extent3d { width: ATLAS_W, height: ATLAS_H, depth_or_array_layers: 1 },
        );
        let atlas_view = atlas.create_view(&Default::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ui"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let screen_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ui-screen"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ui"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
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
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ui"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: screen_buf.as_entire_binding() },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ui"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/ui.wgsl").into()),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ui"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ui"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<UiVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });

        let vbuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ui-verts"),
            size: (4096 * std::mem::size_of::<UiVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        UiRenderer { pipeline, bind, screen_buf, vbuf, vcap: 4096 }
    }

    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: (u32, u32),
        batch: &UiBatch,
    ) {
        if batch.verts.is_empty() {
            return;
        }
        queue.write_buffer(
            &self.screen_buf,
            0,
            bytemuck::bytes_of(&[size.0 as f32, size.1 as f32, 0.0f32, 0.0f32]),
        );
        if batch.verts.len() > self.vcap {
            self.vcap = batch.verts.len().next_power_of_two();
            self.vbuf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("ui-verts"),
                size: (self.vcap * std::mem::size_of::<UiVertex>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        queue.write_buffer(&self.vbuf, 0, bytemuck::cast_slice(&batch.verts));
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ui"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        rp.set_pipeline(&self.pipeline);
        rp.set_bind_group(0, &self.bind, &[]);
        rp.set_vertex_buffer(0, self.vbuf.slice(..));
        rp.draw(0..batch.verts.len() as u32, 0..1);
    }
}

// -------------------------------------------------------------------- font

/// 5×7 glyphs, one string per row, '#' = on. Chars not listed render as '?'.
fn glyph_rows(ch: char) -> [&'static str; 7] {
    match ch {
        'A' => ["01110", "10001", "10001", "11111", "10001", "10001", "10001"],
        'B' => ["11110", "10001", "10001", "11110", "10001", "10001", "11110"],
        'C' => ["01110", "10001", "10000", "10000", "10000", "10001", "01110"],
        'D' => ["11110", "10001", "10001", "10001", "10001", "10001", "11110"],
        'E' => ["11111", "10000", "10000", "11110", "10000", "10000", "11111"],
        'F' => ["11111", "10000", "10000", "11110", "10000", "10000", "10000"],
        'G' => ["01110", "10001", "10000", "10111", "10001", "10001", "01111"],
        'H' => ["10001", "10001", "10001", "11111", "10001", "10001", "10001"],
        'I' => ["11111", "00100", "00100", "00100", "00100", "00100", "11111"],
        'J' => ["00111", "00010", "00010", "00010", "00010", "10010", "01100"],
        'K' => ["10001", "10010", "10100", "11000", "10100", "10010", "10001"],
        'L' => ["10000", "10000", "10000", "10000", "10000", "10000", "11111"],
        'M' => ["10001", "11011", "10101", "10101", "10001", "10001", "10001"],
        'N' => ["10001", "11001", "10101", "10011", "10001", "10001", "10001"],
        'O' => ["01110", "10001", "10001", "10001", "10001", "10001", "01110"],
        'P' => ["11110", "10001", "10001", "11110", "10000", "10000", "10000"],
        'Q' => ["01110", "10001", "10001", "10001", "10101", "10010", "01101"],
        'R' => ["11110", "10001", "10001", "11110", "10100", "10010", "10001"],
        'S' => ["01111", "10000", "10000", "01110", "00001", "00001", "11110"],
        'T' => ["11111", "00100", "00100", "00100", "00100", "00100", "00100"],
        'U' => ["10001", "10001", "10001", "10001", "10001", "10001", "01110"],
        'V' => ["10001", "10001", "10001", "10001", "10001", "01010", "00100"],
        'W' => ["10001", "10001", "10001", "10101", "10101", "10101", "01010"],
        'X' => ["10001", "10001", "01010", "00100", "01010", "10001", "10001"],
        'Y' => ["10001", "10001", "01010", "00100", "00100", "00100", "00100"],
        'Z' => ["11111", "00001", "00010", "00100", "01000", "10000", "11111"],
        '0' => ["01110", "10001", "10011", "10101", "11001", "10001", "01110"],
        '1' => ["00100", "01100", "00100", "00100", "00100", "00100", "01110"],
        '2' => ["01110", "10001", "00001", "00010", "00100", "01000", "11111"],
        '3' => ["11110", "00001", "00001", "01110", "00001", "00001", "11110"],
        '4' => ["00010", "00110", "01010", "10010", "11111", "00010", "00010"],
        '5' => ["11111", "10000", "11110", "00001", "00001", "10001", "01110"],
        '6' => ["00110", "01000", "10000", "11110", "10001", "10001", "01110"],
        '7' => ["11111", "00001", "00010", "00100", "01000", "01000", "01000"],
        '8' => ["01110", "10001", "10001", "01110", "10001", "10001", "01110"],
        '9' => ["01110", "10001", "10001", "01111", "00001", "00010", "01100"],
        '!' => ["00100", "00100", "00100", "00100", "00100", "00000", "00100"],
        '"' => ["01010", "01010", "00000", "00000", "00000", "00000", "00000"],
        '#' => ["01010", "11111", "01010", "01010", "01010", "11111", "01010"],
        '%' => ["11001", "11010", "00010", "00100", "01000", "01011", "10011"],
        '\'' => ["00100", "00100", "00000", "00000", "00000", "00000", "00000"],
        '(' => ["00010", "00100", "01000", "01000", "01000", "00100", "00010"],
        ')' => ["01000", "00100", "00010", "00010", "00010", "00100", "01000"],
        '*' => ["00000", "10101", "01110", "11111", "01110", "10101", "00000"],
        '+' => ["00000", "00100", "00100", "11111", "00100", "00100", "00000"],
        ',' => ["00000", "00000", "00000", "00000", "00000", "00100", "01000"],
        '-' => ["00000", "00000", "00000", "11111", "00000", "00000", "00000"],
        '.' => ["00000", "00000", "00000", "00000", "00000", "00110", "00110"],
        '/' => ["00001", "00010", "00010", "00100", "01000", "01000", "10000"],
        ':' => ["00000", "00110", "00110", "00000", "00110", "00110", "00000"],
        ';' => ["00000", "00110", "00110", "00000", "00110", "00100", "01000"],
        '<' => ["00010", "00100", "01000", "10000", "01000", "00100", "00010"],
        '=' => ["00000", "00000", "11111", "00000", "11111", "00000", "00000"],
        '>' => ["01000", "00100", "00010", "00001", "00010", "00100", "01000"],
        '?' => ["01110", "10001", "00001", "00010", "00100", "00000", "00100"],
        '[' => ["01110", "01000", "01000", "01000", "01000", "01000", "01110"],
        ']' => ["01110", "00010", "00010", "00010", "00010", "00010", "01110"],
        '_' => ["00000", "00000", "00000", "00000", "00000", "00000", "11111"],
        '|' => ["00100", "00100", "00100", "00100", "00100", "00100", "00100"],
        _ => ["00000", "00000", "00000", "00000", "00000", "00000", "00000"],
    }
}

fn build_atlas() -> Vec<u8> {
    let mut data = vec![0u8; (ATLAS_W * ATLAS_H) as usize];
    for idx in 0..95u32 {
        let ch = char::from_u32(32 + idx).unwrap();
        let rows = glyph_rows(ch);
        let cx = (idx % ATLAS_COLS) * CELL;
        let cy = (idx / ATLAS_COLS) * CELL;
        for (ry, row) in rows.iter().enumerate() {
            for (rx, bit) in row.bytes().enumerate() {
                if bit == b'1' || bit == b'#' {
                    let x = cx + rx as u32;
                    let y = cy + ry as u32;
                    data[(y * ATLAS_W + x) as usize] = 255;
                }
            }
        }
    }
    // Solid white cell (rect rendering).
    let cx = (WHITE_INDEX % ATLAS_COLS) * CELL;
    let cy = (WHITE_INDEX / ATLAS_COLS) * CELL;
    for y in cy..cy + CELL {
        for x in cx..cx + CELL {
            data[(y * ATLAS_W + x) as usize] = 255;
        }
    }
    data
}
