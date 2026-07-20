//! Uniform-grid spatial index for the isometric combat plane (XZ).
//! Rebuilt every tick (cheap at horde scale), then queried by radius, cone,
//! and swept-circle. Combat "physics" is circle-vs-circle; nothing more.

use glam::Vec2;
use goetia_core::hash::FnvHashMap;
use goetia_core::Entity;

#[derive(Clone, Copy)]
struct Body {
    entity: Entity,
    pos: Vec2,
    radius: f32,
    /// Game-defined filter bits (faction, layer, …).
    mask: u32,
}

pub struct SpatialGrid {
    cell: f32,
    inv_cell: f32,
    map: FnvHashMap<(i32, i32), Vec<Body>>,
    count: usize,
}

impl SpatialGrid {
    /// `cell` should be ~2× the typical query radius.
    pub fn new(cell: f32) -> Self {
        SpatialGrid {
            cell,
            inv_cell: 1.0 / cell,
            map: FnvHashMap::default(),
            count: 0,
        }
    }

    #[inline]
    fn cell_of(&self, p: Vec2) -> (i32, i32) {
        (
            (p.x * self.inv_cell).floor() as i32,
            (p.y * self.inv_cell).floor() as i32,
        )
    }

    pub fn clear(&mut self) {
        for v in self.map.values_mut() {
            v.clear();
        }
        self.count = 0;
    }

    pub fn insert(&mut self, entity: Entity, pos: Vec2, radius: f32, mask: u32) {
        let c = self.cell_of(pos);
        self.map.entry(c).or_default().push(Body {
            entity,
            pos,
            radius,
            mask,
        });
        self.count += 1;
    }

    pub fn len(&self) -> usize {
        self.count
    }
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    fn cells_in_aabb(&self, min: Vec2, max: Vec2) -> impl Iterator<Item = (i32, i32)> {
        let (x0, y0) = self.cell_of(min);
        let (x1, y1) = self.cell_of(max);
        (x0..=x1).flat_map(move |x| (y0..=y1).map(move |y| (x, y)))
    }

    /// All bodies whose circle overlaps the query circle. `mask_filter` is
    /// ANDed against each body's mask; pass `u32::MAX` for everything.
    pub fn query_radius(
        &self,
        pos: Vec2,
        radius: f32,
        mask_filter: u32,
        out: &mut Vec<(Entity, Vec2)>,
    ) {
        let r = Vec2::splat(radius);
        for c in self.cells_in_aabb(pos - r, pos + r) {
            if let Some(bodies) = self.map.get(&c) {
                for b in bodies {
                    if b.mask & mask_filter == 0 {
                        continue;
                    }
                    let rr = radius + b.radius;
                    if pos.distance_squared(b.pos) <= rr * rr {
                        out.push((b.entity, b.pos));
                    }
                }
            }
        }
    }

    /// Bodies inside a cone: within `range` of `pos` and within `half_angle`
    /// radians of `dir` (normalized).
    pub fn query_cone(
        &self,
        pos: Vec2,
        dir: Vec2,
        half_angle: f32,
        range: f32,
        mask_filter: u32,
        out: &mut Vec<(Entity, Vec2)>,
    ) {
        let cos_half = half_angle.cos();
        let r = Vec2::splat(range);
        for c in self.cells_in_aabb(pos - r, pos + r) {
            if let Some(bodies) = self.map.get(&c) {
                for b in bodies {
                    if b.mask & mask_filter == 0 {
                        continue;
                    }
                    let to = b.pos - pos;
                    let d = to.length();
                    if d > range + b.radius {
                        continue;
                    }
                    if d <= b.radius || to.normalize().dot(dir) >= cos_half {
                        out.push((b.entity, b.pos));
                    }
                }
            }
        }
    }

    /// Swept circle (projectile step): first body hit moving a circle of
    /// `radius` from `from` to `to`. Returns (entity, hit position, t in [0,1]).
    pub fn sweep(
        &self,
        from: Vec2,
        to: Vec2,
        radius: f32,
        mask_filter: u32,
    ) -> Option<(Entity, Vec2, f32)> {
        let d = to - from;
        let len = d.length();
        let pad = Vec2::splat(radius + self.cell);
        let min = from.min(to) - pad;
        let max = from.max(to) + pad;
        let mut best: Option<(Entity, Vec2, f32)> = None;
        for c in self.cells_in_aabb(min, max) {
            if let Some(bodies) = self.map.get(&c) {
                for b in bodies {
                    if b.mask & mask_filter == 0 {
                        continue;
                    }
                    let rr = radius + b.radius;
                    // Segment-circle: solve |from + t*d - b.pos|^2 = rr^2
                    let m = from - b.pos;
                    if len < 1e-6 {
                        if m.length_squared() <= rr * rr {
                            return Some((b.entity, b.pos, 0.0));
                        }
                        continue;
                    }
                    let a = d.dot(d);
                    let bq = 2.0 * m.dot(d);
                    let cq = m.dot(m) - rr * rr;
                    let disc = bq * bq - 4.0 * a * cq;
                    if disc < 0.0 {
                        continue;
                    }
                    let t = (-bq - disc.sqrt()) / (2.0 * a);
                    let t = if cq <= 0.0 { 0.0 } else { t }; // already overlapping
                    if !(0.0..=1.0).contains(&t) {
                        continue;
                    }
                    if best.map(|(_, _, bt)| t < bt).unwrap_or(true) {
                        best = Some((b.entity, from + d * t, t));
                    }
                }
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(i: u32) -> Entity {
        Entity { index: i, gen: 0 }
    }

    #[test]
    fn radius_and_masks() {
        let mut g = SpatialGrid::new(4.0);
        g.insert(e(1), Vec2::new(0.0, 0.0), 0.5, 0b01);
        g.insert(e(2), Vec2::new(3.0, 0.0), 0.5, 0b10);
        g.insert(e(3), Vec2::new(50.0, 50.0), 0.5, 0b01);
        let mut out = Vec::new();
        g.query_radius(Vec2::ZERO, 4.0, u32::MAX, &mut out);
        assert_eq!(out.len(), 2);
        out.clear();
        g.query_radius(Vec2::ZERO, 4.0, 0b10, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, e(2));
    }

    #[test]
    fn sweep_hits_nearest() {
        let mut g = SpatialGrid::new(4.0);
        g.insert(e(1), Vec2::new(10.0, 0.0), 1.0, u32::MAX);
        g.insert(e(2), Vec2::new(5.0, 0.0), 1.0, u32::MAX);
        let hit = g.sweep(Vec2::new(0.0, 0.0), Vec2::new(20.0, 0.0), 0.25, u32::MAX);
        let (ent, _, t) = hit.expect("should hit");
        assert_eq!(ent, e(2));
        assert!(t < 0.3);
        // miss: offset lane
        assert!(g
            .sweep(Vec2::new(0.0, 5.0), Vec2::new(20.0, 5.0), 0.25, u32::MAX)
            .is_none());
    }

    #[test]
    fn cone() {
        let mut g = SpatialGrid::new(4.0);
        g.insert(e(1), Vec2::new(5.0, 0.5), 0.5, u32::MAX);
        g.insert(e(2), Vec2::new(-5.0, 0.0), 0.5, u32::MAX);
        let mut out = Vec::new();
        g.query_cone(Vec2::ZERO, Vec2::X, 0.5, 10.0, u32::MAX, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, e(1));
    }
}
