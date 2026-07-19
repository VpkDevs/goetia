//! The Goetic Court: hub scene. Walk the ring, pick a seat, sign your pacts,
//! descend. Free respec happens here (see ui.rs); this module is the scene.

use crate::combat::*;
use crate::fx::FxQueue;
use crate::vocab::*;
use goetia::prelude::*;

pub struct CourtState {
    pub sel_demon: usize,
    pub tier: u32,
}

impl Default for CourtState {
    fn default() -> Self {
        CourtState { sel_demon: 0, tier: 1 }
    }
}

pub fn enter_court(eng: &mut Engine, gs: &mut Gs) {
    eng.world = World::new();
    eng.world.insert_resource(SpatialGrid::new(2.5));
    eng.world.insert_resource(FxQueue::default());
    gs.walkable = None;
    gs.blight_phase = false;
    gs.pc = PlayerCtx::new(Entity::DEAD);
    let max = gs.build.sheet.get(K_MAX_HP);
    let player = eng.world.spawn((
        Pos(Vec2::new(0.0, 6.0)),
        PrevPos(Vec2::new(0.0, 6.0)),
        Vel(Vec2::ZERO),
        Health::new(max),
        StatusBag::new(),
        PlayerTag,
    ));
    gs.pc.entity = player;
    eng.camera.target = Vec3::ZERO;
    eng.camera.zoom = 24.0;
}

/// Returns Some((demon, tier)) when the player descends.
pub fn tick_court(eng: &mut Engine, gs: &mut Gs, cs: &mut CourtState) -> Option<(Demon, u32)> {
    // Walk the ring (movement only; the Court does not bleed).
    let input = crate::run::read_input(eng);
    let speed = 8.0;
    let (from, to) = {
        let p = eng.world.get::<Pos>(gs.pc.entity).map(|p| p.0).unwrap_or(Vec2::ZERO);
        (p, p + input.mv * speed * FIXED_DT)
    };
    let to = if to.length() < 32.0 { to } else { from };
    if let Some(p) = eng.world.get_mut::<Pos>(gs.pc.entity) {
        p.0 = to;
    }
    if let Some(pp) = eng.world.get_mut::<PrevPos>(gs.pc.entity) {
        pp.0 = from;
    }
    // Heal to full at court.
    if let Some(h) = eng.world.get_mut::<Health>(gs.pc.entity) {
        h.hp = h.max;
    }
    // Camera drifts with you but keeps the ring in frame.
    let t = Vec3::new(to.x * 0.5, 0.0, to.y * 0.5);
    eng.camera.target = eng.camera.target.lerp(t, 0.08);

    // Selection.
    for (k, i) in [(KeyCode::Digit1, 0usize), (KeyCode::Digit2, 1), (KeyCode::Digit3, 2)] {
        if eng.input.key_pressed(k) {
            cs.sel_demon = i;
            eng.audio.play(&gs.sounds.ui, "sfx", 0.4, 1.0 + i as f32 * 0.15);
        }
    }
    if eng.input.key_pressed(KeyCode::Equal) || eng.input.key_pressed(KeyCode::NumpadAdd) {
        cs.tier += 1; // infinite tiers: the ceiling is a lie
        eng.audio.play(&gs.sounds.ui, "sfx", 0.3, 1.3);
    }
    if (eng.input.key_pressed(KeyCode::Minus) || eng.input.key_pressed(KeyCode::NumpadSubtract))
        && cs.tier > 1
    {
        cs.tier -= 1;
        eng.audio.play(&gs.sounds.ui, "sfx", 0.3, 0.8);
    }
    // Standing near a gate also selects it (walkable diegesis).
    for d in DEMONS {
        let a = d.index() as f32 / 3.0 * std::f32::consts::TAU;
        let gate = Vec2::new(a.cos() * 14.0, a.sin() * 14.0);
        if to.distance(gate) < 2.5 {
            cs.sel_demon = d.index();
        }
    }
    if eng.input.key_pressed(KeyCode::Enter) {
        return Some((DEMONS[cs.sel_demon], cs.tier));
    }
    None
}
