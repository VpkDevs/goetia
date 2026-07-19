//! Engine-native flat-shaded meshes generated from primitives, with a tiny
//! CSG-ish authoring API: `MeshBuilder::column().twisted(0.2)`. No importers;
//! the game's look is primitives + shaders + particles.
//!
//! Vertices are deliberately non-indexed-per-face (positions duplicated) so
//! every triangle carries a true face normal — that IS the flat-shaded look.

use glam::{Mat4, Quat, Vec3};
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct MeshHandle(pub(crate) usize);

pub(crate) struct GpuMesh {
    pub vertices: wgpu::Buffer,
    pub indices: wgpu::Buffer,
    pub index_count: u32,
}

pub(crate) struct MeshStore {
    meshes: Vec<GpuMesh>,
}

impl MeshStore {
    pub fn new() -> Self {
        MeshStore { meshes: Vec::new() }
    }

    pub fn register(&mut self, device: &wgpu::Device, b: MeshBuilder) -> MeshHandle {
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh-verts"),
            contents: bytemuck::cast_slice(&b.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh-idx"),
            contents: bytemuck::cast_slice(&b.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        self.meshes.push(GpuMesh { vertices, indices, index_count: b.indices.len() as u32 });
        MeshHandle(self.meshes.len() - 1)
    }

    pub fn get(&self, h: MeshHandle) -> &GpuMesh {
        &self.meshes[h.0]
    }
}

/// CPU-side mesh under construction.
#[derive(Clone, Default)]
pub struct MeshBuilder {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl MeshBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    // ---------------------------------------------------------- primitives

    /// Unit cube centered at origin (1×1×1). Scale via instance transform or
    /// [`Self::scaled`].
    pub fn cube() -> Self {
        let mut m = MeshBuilder::new();
        m.push_box(Vec3::splat(-0.5), Vec3::splat(0.5));
        m
    }

    /// Axis-aligned box between two corners.
    pub fn boxed(min: Vec3, max: Vec3) -> Self {
        let mut m = MeshBuilder::new();
        m.push_box(min, max);
        m
    }

    /// Tall square column: base 1×1, height `h`, base at y=0.
    pub fn column(h: f32) -> Self {
        Self::boxed(Vec3::new(-0.5, 0.0, -0.5), Vec3::new(0.5, h, 0.5))
    }

    /// N-sided prism (cylinder-ish), radius r, height h, base at y=0.
    pub fn prism(sides: u32, r: f32, h: f32) -> Self {
        let mut m = MeshBuilder::new();
        let n = sides.max(3);
        for i in 0..n {
            let a0 = i as f32 / n as f32 * std::f32::consts::TAU;
            let a1 = (i + 1) as f32 / n as f32 * std::f32::consts::TAU;
            let p0 = Vec3::new(a0.cos() * r, 0.0, a0.sin() * r);
            let p1 = Vec3::new(a1.cos() * r, 0.0, a1.sin() * r);
            let q0 = p0 + Vec3::Y * h;
            let q1 = p1 + Vec3::Y * h;
            m.push_quad(p1, p0, q0, q1); // side (wound for outward normal)
            m.push_tri(Vec3::new(0.0, h, 0.0), q0, q1); // top fan (ccw from above)
            m.push_tri(Vec3::ZERO, p1, p0); // bottom
        }
        m
    }

    /// Icosphere-ish: octahedron subdivided `sub` times then normalized —
    /// low-poly faceted orb (projectiles, gems).
    pub fn orb(sub: u32, r: f32) -> Self {
        let mut tris: Vec<[Vec3; 3]> = Vec::new();
        let (xp, xn) = (Vec3::X, -Vec3::X);
        let (yp, yn) = (Vec3::Y, -Vec3::Y);
        let (zp, zn) = (Vec3::Z, -Vec3::Z);
        for &[a, b, c] in &[
            [yp, zp, xp], [yp, xp, zn], [yp, zn, xn], [yp, xn, zp],
            [yn, xp, zp], [yn, zn, xp], [yn, xn, zn], [yn, zp, xn],
        ] {
            tris.push([a, b, c]);
        }
        for _ in 0..sub {
            let mut next = Vec::new();
            for [a, b, c] in tris {
                let ab = ((a + b) * 0.5).normalize();
                let bc = ((b + c) * 0.5).normalize();
                let ca = ((c + a) * 0.5).normalize();
                next.push([a, ab, ca]);
                next.push([ab, b, bc]);
                next.push([ca, bc, c]);
                next.push([ab, bc, ca]);
            }
            tris = next;
        }
        let mut m = MeshBuilder::new();
        for [a, b, c] in tris {
            m.push_tri(a * r, b * r, c * r);
        }
        m
    }

    /// Flat ground quad (y=0), size w×d centered at origin, normal +Y.
    pub fn ground(w: f32, d: f32) -> Self {
        let mut m = MeshBuilder::new();
        let (hw, hd) = (w * 0.5, d * 0.5);
        m.push_quad(
            Vec3::new(-hw, 0.0, -hd),
            Vec3::new(-hw, 0.0, hd),
            Vec3::new(hw, 0.0, hd),
            Vec3::new(hw, 0.0, -hd),
        );
        m
    }

    /// Four-sided spike: pyramid pointing +Y, base 1×1 at y=0, height h.
    pub fn spike(h: f32) -> Self {
        let mut m = MeshBuilder::new();
        let tip = Vec3::new(0.0, h, 0.0);
        let b = [
            Vec3::new(-0.5, 0.0, -0.5),
            Vec3::new(0.5, 0.0, -0.5),
            Vec3::new(0.5, 0.0, 0.5),
            Vec3::new(-0.5, 0.0, 0.5),
        ];
        for i in 0..4 {
            m.push_tri(tip, b[i], b[(i + 1) % 4]);
        }
        m.push_quad(b[3], b[2], b[1], b[0]); // underside
        m
    }

    // -------------------------------------------------------- CSG-ish ops

    /// Merge another mesh in place (union-by-concatenation; occult-idol
    /// authoring is stacked primitives, not booleans).
    pub fn merged(mut self, other: MeshBuilder) -> Self {
        let base = self.vertices.len() as u32;
        self.vertices.extend(other.vertices);
        self.indices.extend(other.indices.iter().map(|i| i + base));
        self
    }

    pub fn transformed(mut self, m: Mat4) -> Self {
        let nm = m.inverse().transpose();
        for v in &mut self.vertices {
            let p = m.transform_point3(Vec3::from(v.pos));
            let n = nm.transform_vector3(Vec3::from(v.normal)).normalize_or_zero();
            v.pos = p.to_array();
            v.normal = n.to_array();
        }
        self
    }

    pub fn translated(self, t: Vec3) -> Self {
        self.transformed(Mat4::from_translation(t))
    }
    pub fn scaled(self, s: Vec3) -> Self {
        self.transformed(Mat4::from_scale(s))
    }
    pub fn rotated(self, q: Quat) -> Self {
        self.transformed(Mat4::from_quat(q))
    }

    /// Twist around Y: `turns_per_unit` full rotations per world unit height.
    /// Obsidian pillars want ~0.05–0.2.
    pub fn twisted(mut self, turns_per_unit: f32) -> Self {
        for v in &mut self.vertices {
            let a = v.pos[1] * turns_per_unit * std::f32::consts::TAU;
            let (s, c) = a.sin_cos();
            let (x, z) = (v.pos[0], v.pos[2]);
            v.pos[0] = x * c - z * s;
            v.pos[2] = x * s + z * c;
        }
        self.renormal()
    }

    /// Taper along Y from full width at y_min to `top_scale` at y_max.
    pub fn tapered(mut self, top_scale: f32) -> Self {
        let (y0, y1) = self.y_range();
        let span = (y1 - y0).max(1e-5);
        for v in &mut self.vertices {
            let t = (v.pos[1] - y0) / span;
            let s = 1.0 + (top_scale - 1.0) * t;
            v.pos[0] *= s;
            v.pos[2] *= s;
        }
        self.renormal()
    }

    /// Random faceting jitter (deterministic: hash of vertex position).
    /// Makes primitives read as hewn rock.
    pub fn jittered(mut self, amount: f32) -> Self {
        for v in &mut self.vertices {
            let h = |a: f32, b: f32| {
                let x = (a * 127.1 + b * 311.7).sin() * 43758.547;
                x.fract() - 0.5
            };
            let j = Vec3::new(
                h(v.pos[0], v.pos[1] + v.pos[2]),
                h(v.pos[1], v.pos[2] + v.pos[0] + 7.0),
                h(v.pos[2], v.pos[0] + v.pos[1] + 13.0),
            ) * amount;
            v.pos = (Vec3::from(v.pos) + j).to_array();
        }
        self.renormal()
    }

    fn y_range(&self) -> (f32, f32) {
        let mut lo = f32::MAX;
        let mut hi = f32::MIN;
        for v in &self.vertices {
            lo = lo.min(v.pos[1]);
            hi = hi.max(v.pos[1]);
        }
        (lo, hi)
    }

    /// Recompute flat face normals after a deforming op.
    fn renormal(mut self) -> Self {
        for tri in self.indices.chunks_exact(3) {
            let a = Vec3::from(self.vertices[tri[0] as usize].pos);
            let b = Vec3::from(self.vertices[tri[1] as usize].pos);
            let c = Vec3::from(self.vertices[tri[2] as usize].pos);
            let n = (b - a).cross(c - a).normalize_or_zero().to_array();
            for &i in tri {
                self.vertices[i as usize].normal = n;
            }
        }
        self
    }

    // ------------------------------------------------------------ helpers

    fn push_tri(&mut self, a: Vec3, b: Vec3, c: Vec3) {
        let n = (b - a).cross(c - a).normalize_or_zero().to_array();
        let base = self.vertices.len() as u32;
        for p in [a, b, c] {
            self.vertices.push(Vertex { pos: p.to_array(), normal: n });
        }
        self.indices.extend([base, base + 1, base + 2]);
    }

    fn push_quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3) {
        self.push_tri(a, b, c);
        self.push_tri(a, c, d);
    }

    fn push_box(&mut self, min: Vec3, max: Vec3) {
        let (a, b) = (min, max);
        let p = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
        // -X, +X, -Y, +Y, -Z, +Z faces, CCW seen from outside.
        self.push_quad(p(a.x, a.y, a.z), p(a.x, a.y, b.z), p(a.x, b.y, b.z), p(a.x, b.y, a.z));
        self.push_quad(p(b.x, a.y, b.z), p(b.x, a.y, a.z), p(b.x, b.y, a.z), p(b.x, b.y, b.z));
        self.push_quad(p(a.x, a.y, a.z), p(b.x, a.y, a.z), p(b.x, a.y, b.z), p(a.x, a.y, b.z));
        self.push_quad(p(a.x, b.y, b.z), p(b.x, b.y, b.z), p(b.x, b.y, a.z), p(a.x, b.y, a.z));
        self.push_quad(p(b.x, a.y, a.z), p(a.x, a.y, a.z), p(a.x, b.y, a.z), p(b.x, b.y, a.z));
        self.push_quad(p(a.x, a.y, b.z), p(b.x, a.y, b.z), p(b.x, b.y, b.z), p(a.x, b.y, b.z));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_has_36_indices_and_outward_normals() {
        let c = MeshBuilder::cube();
        assert_eq!(c.indices.len(), 36);
        // Every face normal should point away from center.
        for tri in c.indices.chunks_exact(3) {
            let a = Vec3::from(c.vertices[tri[0] as usize].pos);
            let b = Vec3::from(c.vertices[tri[1] as usize].pos);
            let cc = Vec3::from(c.vertices[tri[2] as usize].pos);
            let center = (a + b + cc) / 3.0;
            let n = Vec3::from(c.vertices[tri[0] as usize].normal);
            assert!(n.dot(center) > 0.0, "inward-facing normal {n:?} at {center:?}");
        }
    }

    #[test]
    fn authoring_chain_compiles_and_deforms() {
        let m = MeshBuilder::column(3.0).twisted(0.1).tapered(0.5).jittered(0.02);
        assert!(!m.vertices.is_empty());
        let (lo, hi) = m.y_range();
        assert!(lo > -0.2 && hi > 2.5);
    }
}
