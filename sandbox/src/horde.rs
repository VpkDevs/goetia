//! The horde stress sim: 500 AI enemies, ~3000 live projectiles, corpses,
//! impact FX. Pure fixed-tick logic — the windowed scene renders it, the
//! determinism test hashes it.

use crate::*;
use glam::Vec2;
use goetia::prelude::*;

pub const ENEMY_COUNT: usize = 500;
pub const PROJECTILE_LIFE: u16 = 240; // 4s
pub const FIRE_PER_TICK: usize = 13; // ≈3120 live at steady state
const ENEMY_MASK: u32 = 0b1;

/// Impact FX handoff from sim to renderer (drained by render_extract; capped
/// so headless runs don't grow unbounded).
#[derive(Default)]
pub struct FxQueue {
    pub impacts: Vec<(Vec2, f32)>, // pos, strength (1 = hit, 3 = death)
}

pub fn reset(eng: &mut Engine) {
    eng.world = World::new();
    eng.schedule = Schedule::new();
    eng.clock.tick = 0;

    eng.world.insert_resource(Arena { half: 26.0 });
    eng.world.insert_resource(FxQueue::default());
    eng.world.insert_resource(SpatialGrid::new(2.0));

    // Movement systems run through the scheduler: prev-pos snapshot, then
    // integrate + corpse aging in parallel (disjoint writes).
    eng.schedule.add(
        SystemDef::new("prev_pos", |w| {
            w.each::<(&Pos, &mut PrevPos)>(|_, (p, pp)| pp.0 = p.0);
        })
        .reads::<Pos>()
        .writes::<PrevPos>(),
    );
    eng.schedule.add(
        SystemDef::new("integrate", |w| {
            w.each::<(&mut Pos, &Vel)>(|_, (p, v)| p.0 += v.0 * FIXED_DT);
        })
        .writes::<Pos>()
        .reads::<Vel>(),
    );
    eng.schedule.add(
        SystemDef::new("corpse_age", |w| {
            w.each::<(&mut Corpse,)>(|_, (c,)| c.age = c.age.saturating_add(1));
        })
        .writes::<Corpse>(),
    );

    let rng = eng.streams.get("packs");
    for i in 0..ENEMY_COUNT {
        let a = rng.range_f32(0.0, std::f32::consts::TAU);
        let r = rng.range_f32(10.0, 24.0);
        let pos = Vec2::new(a.cos(), a.sin()) * r;
        spawn_enemy(&mut eng.world, pos, i as f32 * 0.618);
    }
}

fn spawn_enemy(world: &mut World, pos: Vec2, phase: f32) {
    world.spawn((
        Pos(pos),
        PrevPos(pos),
        Vel(Vec2::ZERO),
        Hp(30.0),
        Enemy {
            state: AiState::Seek,
            timer: 60,
            radius: 0.45,
            spin: if (phase * 10.0) as u32 % 2 == 0 { 1.0 } else { -1.0 },
            phase,
        },
    ));
}

pub fn tick(eng: &mut Engine) {
    let tick = eng.clock.tick;
    let arena_half = eng.world.resource::<Arena>().half;

    // --- scheduled systems (prev-pos, integrate, corpse aging)
    eng.run_schedule();

    // --- AI state machine (sim RNG: "ai" stream)
    {
        let mut rng = eng.streams.get("ai").clone();
        eng.world.each::<(&Pos, &mut Vel, &mut Enemy)>(|_, (p, v, e)| {
            e.timer = e.timer.saturating_sub(1);
            let to_center = -p.0;
            let dist = to_center.length().max(0.001);
            let dir = to_center / dist;
            match e.state {
                AiState::Seek => {
                    let want = dist - 6.0; // hold a ring around the turret
                    v.0 = dir * want.clamp(-1.0, 1.0) * 3.0;
                    if e.timer == 0 {
                        e.state = AiState::Strafe;
                        e.timer = 90 + rng.range_u32(120) as u16;
                    }
                }
                AiState::Strafe => {
                    let tangent = Vec2::new(-dir.y, dir.x) * e.spin;
                    v.0 = tangent * 2.5 + dir * (dist - 8.0).clamp(-1.0, 1.0);
                    if e.timer == 0 {
                        e.state = AiState::Telegraph;
                        e.timer = 24;
                    }
                }
                AiState::Telegraph => {
                    v.0 = Vec2::ZERO;
                    if e.timer == 0 {
                        e.state = AiState::Lunge;
                        e.timer = 18;
                    }
                }
                AiState::Lunge => {
                    v.0 = dir * 12.0;
                    if e.timer == 0 {
                        e.state = AiState::Seek;
                        e.timer = 30 + rng.range_u32(90) as u16;
                    }
                }
            }
        });
        *eng.streams.get("ai") = rng;
    }

    // --- turret fire: deterministic spiral
    {
        let golden = 2.399963;
        for i in 0..FIRE_PER_TICK {
            let a = tick as f32 * golden + i as f32 / FIRE_PER_TICK as f32 * std::f32::consts::TAU;
            let dir = Vec2::new(a.cos(), a.sin());
            let glow = (tick as usize * FIRE_PER_TICK + i) % 60 == 0;
            eng.world.spawn((
                Pos(dir * 0.8),
                PrevPos(dir * 0.8),
                Vel(dir * 14.0),
                Projectile { life: PROJECTILE_LIFE, radius: 0.15, glow, ..Default::default() },
            ));
        }
    }

    // --- spatial grid rebuild (enemies only)
    {
        let mut grid = eng.world.remove_resource::<SpatialGrid>().unwrap();
        grid.clear();
        eng.world.each::<(&Pos, &Enemy)>(|ent, (p, e)| {
            grid.insert(ent, p.0, e.radius, ENEMY_MASK);
        });
        eng.world.insert_resource(grid);
    }

    // --- projectiles: bounce, sweep-hit, expire
    {
        let mut hits: Vec<(Entity, Entity, Vec2, bool)> = Vec::new(); // (proj, enemy, at, spent)
        let mut expired: Vec<Entity> = Vec::new();
        {
            let world = &mut eng.world;
            // Split-borrow dance: grid lives outside the ECS during the sweep.
            let grid = world.remove_resource::<SpatialGrid>().unwrap();
            world.each::<(&mut Pos, &PrevPos, &mut Vel, &mut Projectile)>(
                |ent, (p, pp, v, pr)| {
                    pr.life = pr.life.saturating_sub(1);
                    if pr.life == 0 {
                        expired.push(ent);
                        return;
                    }
                    // Arena bounce.
                    if p.0.x.abs() > arena_half {
                        p.0.x = p.0.x.clamp(-arena_half, arena_half);
                        v.0.x = -v.0.x;
                    }
                    if p.0.y.abs() > arena_half {
                        p.0.y = p.0.y.clamp(-arena_half, arena_half);
                        v.0.y = -v.0.y;
                    }
                    if let Some((hit_ent, at, _t)) = grid.sweep(pp.0, p.0, pr.radius, ENEMY_MASK) {
                        if hit_ent != pr.last_hit {
                            pr.last_hit = hit_ent;
                            pr.pierce = pr.pierce.saturating_sub(1);
                            hits.push((ent, hit_ent, at, pr.pierce == 0));
                        }
                    }
                },
            );
            world.insert_resource(grid);
        }

        let mut rng = eng.streams.get("packs").clone();
        let mut deaths: Vec<(Entity, Vec2, f32)> = Vec::new();
        for (proj, enemy, at, spent) in hits {
            if spent {
                eng.commands.despawn(proj);
            }
            let mut dead = None;
            if let Some(hp) = eng.world.get_mut::<Hp>(enemy) {
                let was_alive = hp.0 > 0.0;
                hp.0 -= 5.0;
                // Only the killing blow registers a death — several projectiles
                // can strike the same enemy in one tick.
                if was_alive && hp.0 <= 0.0 {
                    dead = Some(enemy);
                }
            }
            let strength = if dead.is_some() { 3.0 } else { 1.0 };
            eng.world.resource_mut::<FxQueue>().impacts.push((at, strength));
            if let Some(e) = dead {
                let phase = eng.world.get::<Enemy>(e).map(|en| en.phase).unwrap_or(0.0);
                if let Some(p) = eng.world.get::<Pos>(e) {
                    deaths.push((e, p.0, phase));
                }
            }
        }
        for (e, pos, phase) in deaths {
            if !eng.world.despawn(e) {
                continue; // guard: 1 death = exactly 1 respawn
            }
            eng.world.spawn((Pos(pos), PrevPos(pos), Corpse { age: 0, max_age: 300 }));
            // Respawn at the rim to hold the population at 500.
            let a = rng.range_f32(0.0, std::f32::consts::TAU);
            let p = Vec2::new(a.cos(), a.sin()) * (arena_half - 1.5);
            spawn_enemy(&mut eng.world, p, phase + 1.7);
        }
        *eng.streams.get("packs") = rng;

        for e in expired {
            eng.commands.despawn(e);
        }
    }

    // --- corpse cleanup
    {
        let mut gone = Vec::new();
        eng.world.each::<(&Corpse,)>(|ent, (c,)| {
            if c.age >= c.max_age {
                gone.push(ent);
            }
        });
        for e in gone {
            eng.commands.despawn(e);
        }
    }

    // Cap FX queue in headless runs (nobody drains it).
    let fx = eng.world.resource_mut::<FxQueue>();
    if fx.impacts.len() > 8192 {
        fx.impacts.clear();
    }
}

/// Bit-exact fingerprint of sim state + RNG streams.
pub fn world_hash(eng: &mut Engine) -> u64 {
    let mut h = StateHasher::new();
    h.write_u64(eng.clock.tick);
    eng.world.each::<(&Pos, &Vel, &Hp, &Enemy)>(|ent, (p, v, hp, e)| {
        h.write_u64(ent.to_bits());
        h.write_vec2(p.0.x, p.0.y);
        h.write_vec2(v.0.x, v.0.y);
        h.write_f32(hp.0);
        h.write_u32(e.state as u32);
        h.write_u32(e.timer as u32);
    });
    eng.world.each::<(&Pos, &Projectile)>(|ent, (p, pr)| {
        h.write_u64(ent.to_bits());
        h.write_vec2(p.0.x, p.0.y);
        h.write_u32(pr.life as u32);
    });
    eng.world.each::<(&Corpse,)>(|ent, (c,)| {
        h.write_u64(ent.to_bits());
        h.write_u32(c.age as u32);
    });
    // RNG stream state matters: same world + different stream = divergence next tick.
    let s1 = eng.streams.get("ai").clone();
    let s2 = eng.streams.get("packs").clone();
    let txt = format!("{s1:?}|{s2:?}");
    h.write_bytes(txt.as_bytes());
    h.finish()
}
