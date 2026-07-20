//! Fixed isometric camera with a zoom band, plus trauma-based screenshake.
//! The locked angle is a feature: transparency sorts trivially, culling is a
//! rectangle, and the game reads as a diorama.

use glam::{Mat4, Vec2, Vec3};

/// Classic dimetric-ish angle: 45° yaw, ~35.264° pitch (atan(1/√2)).
const YAW: f32 = std::f32::consts::FRAC_PI_4;
const PITCH: f32 = 0.6154797; // atan(1/sqrt(2))
const EYE_DIST: f32 = 200.0;

pub struct CameraRig {
    /// World point the camera looks at (ground plane).
    pub target: Vec3,
    /// Half-height of the ortho volume, world units. Clamped to the zoom band.
    pub zoom: f32,
    pub zoom_min: f32,
    pub zoom_max: f32,
    /// 0..1; shake amplitude is trauma², decays linearly.
    trauma: f32,
    time: f32,
    shake_offset: Vec2,
    shake_roll: f32,
}

impl Default for CameraRig {
    fn default() -> Self {
        Self::new()
    }
}

impl CameraRig {
    pub fn new() -> Self {
        CameraRig {
            target: Vec3::ZERO,
            zoom: 18.0,
            zoom_min: 10.0,
            zoom_max: 40.0,
            trauma: 0.0,
            time: 0.0,
            shake_offset: Vec2::ZERO,
            shake_roll: 0.0,
        }
    }

    /// Add screenshake trauma (0..1 scale; e.g. small hit 0.2, explosion 0.5).
    pub fn add_trauma(&mut self, amount: f32) {
        self.trauma = (self.trauma + amount).clamp(0.0, 1.0);
    }

    pub fn trauma(&self) -> f32 {
        self.trauma
    }

    /// Advance shake simulation with real (unscaled) time.
    pub fn update(&mut self, dt: f32) {
        self.time += dt;
        self.trauma = (self.trauma - dt * 1.2).max(0.0);
        let a = self.trauma * self.trauma;
        // Cheap deterministic-enough noise: incommensurate sines.
        let t = self.time * 28.0;
        self.shake_offset = Vec2::new(
            (t * 1.0).sin() + (t * 2.7 + 1.3).sin() * 0.5,
            (t * 1.3 + 4.1).sin() + (t * 3.1 + 2.2).sin() * 0.5,
        ) * a
            * self.zoom
            * 0.02;
        self.shake_roll = ((t * 0.9 + 0.7).sin()) * a * 0.02;
    }

    pub fn zoom_by(&mut self, factor: f32) {
        self.zoom = (self.zoom * factor).clamp(self.zoom_min, self.zoom_max);
    }

    fn view_dir() -> Vec3 {
        // From eye toward target.
        let x = YAW.cos() * PITCH.cos();
        let z = YAW.sin() * PITCH.cos();
        let y = PITCH.sin();
        -Vec3::new(x, y, z)
    }

    pub fn eye(&self) -> Vec3 {
        self.target - Self::view_dir() * EYE_DIST
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let h = self.zoom;
        let w = h * aspect;
        let eye = self.eye();
        let view = Mat4::look_at_rh(eye, self.target, Vec3::Y);
        let roll = Mat4::from_rotation_z(self.shake_roll);
        let shake =
            Mat4::from_translation(Vec3::new(self.shake_offset.x, self.shake_offset.y, 0.0));
        let proj = Mat4::orthographic_rh(-w, w, -h, h, 1.0, EYE_DIST * 2.5);
        proj * shake * roll * view
    }

    /// Unproject a screen position (pixels) to the y=0 ground plane.
    pub fn screen_to_ground(&self, screen: Vec2, viewport: Vec2, aspect: f32) -> Vec3 {
        let ndc = Vec2::new(
            screen.x / viewport.x * 2.0 - 1.0,
            1.0 - screen.y / viewport.y * 2.0,
        );
        let inv = self.view_proj(aspect).inverse();
        let near = inv.project_point3(Vec3::new(ndc.x, ndc.y, 0.0));
        let far = inv.project_point3(Vec3::new(ndc.x, ndc.y, 1.0));
        let dir = (far - near).normalize();
        if dir.y.abs() < 1e-6 {
            return near;
        }
        let t = -near.y / dir.y;
        near + dir * t
    }

    /// Project a world point to screen pixels (for damage numbers, markers).
    pub fn world_to_screen(&self, world: Vec3, viewport: Vec2, aspect: f32) -> Vec2 {
        let clip = self.view_proj(aspect).project_point3(world);
        Vec2::new(
            (clip.x * 0.5 + 0.5) * viewport.x,
            (0.5 - clip.y * 0.5) * viewport.y,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ground_roundtrip() {
        let cam = CameraRig::new();
        let vp = Vec2::new(1600.0, 900.0);
        let aspect = vp.x / vp.y;
        let world = Vec3::new(5.0, 0.0, -3.0);
        let s = cam.world_to_screen(world, vp, aspect);
        let back = cam.screen_to_ground(s, vp, aspect);
        assert!((back - world).length() < 0.01, "{back:?} vs {world:?}");
    }
}
