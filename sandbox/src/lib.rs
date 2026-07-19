//! Sandbox sim library — shared between the windowed stress scenes and the
//! headless determinism test. Everything in here is deterministic: sim RNG
//! only, fixed tick, no wall clock.

pub mod horde;

use glam::Vec2;
use goetia::{App, Engine, FrameSubmit, Game, Renderer};

/// Headless horde run → world hash. Shared by the in-app determinism scene
/// and the CI test.
pub fn run_headless_horde(seed: u64, ticks: u64) -> u64 {
    struct HeadlessHorde;
    impl Game for HeadlessHorde {
        fn init(&mut self, eng: &mut Engine, _gfx: Option<&mut Renderer>) {
            horde::reset(eng);
        }
        fn fixed_update(&mut self, eng: &mut Engine) {
            horde::tick(eng);
        }
        fn render_extract(&mut self, _e: &mut Engine, _f: &mut FrameSubmit, _a: f32) {}
    }
    let mut eng = App::run_headless(HeadlessHorde, seed, ticks);
    horde::world_hash(&mut eng)
}

// ------------------------------------------------------------- components

#[derive(Clone, Copy)]
pub struct Pos(pub Vec2);
#[derive(Clone, Copy)]
pub struct PrevPos(pub Vec2);
#[derive(Clone, Copy)]
pub struct Vel(pub Vec2);
#[derive(Clone, Copy)]
pub struct Hp(pub f32);

#[derive(Clone, Copy, PartialEq)]
pub enum AiState {
    Seek,
    Strafe,
    Telegraph,
    Lunge,
}

#[derive(Clone, Copy)]
pub struct Enemy {
    pub state: AiState,
    pub timer: u16,
    pub radius: f32,
    /// -1 or +1 strafe direction.
    pub spin: f32,
    /// Visual phase seed (also deterministic).
    pub phase: f32,
}

#[derive(Clone, Copy)]
pub struct Projectile {
    pub life: u16,
    pub radius: f32,
    /// Glowing projectiles also submit a light.
    pub glow: bool,
    /// Enemies pierced before the projectile dies.
    pub pierce: u8,
    /// Last entity hit (skip to avoid re-hitting every tick while inside).
    pub last_hit: goetia::Entity,
}

impl Default for Projectile {
    fn default() -> Self {
        Projectile {
            life: 240,
            radius: 0.15,
            glow: false,
            pierce: 4,
            last_hit: goetia::Entity::DEAD,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Corpse {
    pub age: u16,
    pub max_age: u16,
}

/// Arena bounds resource.
#[derive(Clone, Copy)]
pub struct Arena {
    pub half: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use goetia::prelude::*;

    /// CI determinism gate: 10k ticks from the same seed, twice, bit-equal.
    #[test]
    fn determinism_10k_ticks() {
        const SEED: u64 = 0xDEAD_BEEF_CAFE_F00D;
        const TICKS: u64 = 10_000;

        struct H;
        impl Game for H {
            fn init(&mut self, eng: &mut Engine, _: Option<&mut Renderer>) {
                horde::reset(eng);
            }
            fn fixed_update(&mut self, eng: &mut Engine) {
                horde::tick(eng);
            }
            fn render_extract(&mut self, _: &mut Engine, _: &mut FrameSubmit, _: f32) {}
        }

        let mut a = App::run_headless(H, SEED, TICKS);
        let ha = horde::world_hash(&mut a);
        let mut b = App::run_headless(H, SEED, TICKS);
        let hb = horde::world_hash(&mut b);
        assert_eq!(ha, hb, "world hashes diverged: {ha:#x} vs {hb:#x}");
    }

    #[test]
    fn realm_seed_stable() {
        let templates = vec![
            RoomTemplate {
                name: "hub".into(),
                width: 3,
                height: 3,
                doors: vec![
                    Door { side: Side::North, offset: 1 },
                    Door { side: Side::South, offset: 1 },
                    Door { side: Side::East, offset: 1 },
                    Door { side: Side::West, offset: 1 },
                ],
                tags: vec![],
                weight: 1.0,
            },
            RoomTemplate {
                name: "hall".into(),
                width: 2,
                height: 4,
                doors: vec![
                    Door { side: Side::North, offset: 0 },
                    Door { side: Side::South, offset: 1 },
                ],
                tags: vec![],
                weight: 1.2,
            },
            RoomTemplate {
                name: "cell".into(),
                width: 2,
                height: 2,
                doors: vec![Door { side: Side::West, offset: 0 }],
                tags: vec![],
                weight: 1.0,
            },
            RoomTemplate {
                name: "wide".into(),
                width: 4,
                height: 2,
                doors: vec![
                    Door { side: Side::East, offset: 0 },
                    Door { side: Side::West, offset: 1 },
                ],
                tags: vec![],
                weight: 1.0,
            },
            RoomTemplate {
                name: "square".into(),
                width: 3,
                height: 3,
                doors: vec![
                    Door { side: Side::North, offset: 1 },
                    Door { side: Side::West, offset: 1 },
                ],
                tags: vec![],
                weight: 0.9,
            },
            RoomTemplate {
                name: "nook".into(),
                width: 2,
                height: 2,
                doors: vec![Door { side: Side::South, offset: 0 }],
                tags: vec![],
                weight: 0.8,
            },
        ];
        let g = RealmGrammar {
            start: "hub".into(),
            target_rooms: 15,
            allow: vec![],
        };
        let a = assemble(&templates, &g, &mut Pcg32::new(42, 1)).unwrap();
        let b = assemble(&templates, &g, &mut Pcg32::new(42, 1)).unwrap();
        assert_eq!(a.hash(), b.hash());
        assert!(
            a.rooms.len() >= 10,
            "expected ~15 rooms, got {}",
            a.rooms.len()
        );
    }
}
