//! Engine-level damage-number / world-text batcher: spawn at a world
//! position, numbers rise, pop, and fade; drawn through the UI layer.

use glam::{Vec2, Vec3, Vec4};
use goetia_render::{CameraRig, UiBatch};

struct Floater {
    world: Vec3,
    text: String,
    color: Vec4,
    age: f32,
    life: f32,
    scale: f32,
}

#[derive(Default)]
pub struct DamageNumbers {
    items: Vec<Floater>,
}

impl DamageNumbers {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn spawn(&mut self, world: Vec3, text: impl Into<String>, color: Vec4, scale: f32) {
        self.items.push(Floater {
            world,
            text: text.into(),
            color,
            age: 0.0,
            life: 0.9,
            scale,
        });
    }

    /// Advance with real (render) time — damage numbers ignore hitstop so the
    /// world freezes but the payoff keeps moving.
    pub fn update(&mut self, dt: f32) {
        for f in &mut self.items {
            f.age += dt;
        }
        self.items.retain(|f| f.age < f.life);
    }

    pub fn draw(&self, ui: &mut UiBatch, cam: &CameraRig, viewport: Vec2) {
        let aspect = viewport.x / viewport.y.max(1.0);
        for f in &self.items {
            let t = f.age / f.life;
            // Pop in (overshoot), rise, fade out.
            let pop = if t < 0.15 { 0.6 + t / 0.15 * 0.6 } else { 1.2 - (t - 0.15) * 0.25 };
            let rise = t * 1.6;
            let mut p = cam.world_to_screen(f.world + Vec3::Y * (1.8 + rise), viewport, aspect);
            let alpha = if t > 0.6 { 1.0 - (t - 0.6) / 0.4 } else { 1.0 };
            let scale = f.scale * pop;
            p.x -= UiBatch::text_width(scale, &f.text) * 0.5;
            let c = Vec4::new(f.color.x, f.color.y, f.color.z, f.color.w * alpha);
            ui.text_shadowed(p, scale, c, &f.text);
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}
