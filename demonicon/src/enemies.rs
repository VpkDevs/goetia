//! Enemy AI, spawning, and the three bosses. Discord is handled here: a
//! Discorded enemy retargets the nearest other enemy — any source may apply
//! it (Pillar 2), so realm mods, skills, items and contracts all get riots.

use crate::combat::*;
use crate::content::*;
use crate::vocab::*;
use goetia::prelude::*;

// Args mirror the realm-modifier pipeline (hp/dmg/speed multipliers arrive
// independently); bundling them into a struct would only move the noise.
#[allow(clippy::too_many_arguments)]
pub fn spawn_enemy(
    eng: &mut Engine,
    db: &ContentDb,
    def_id: &str,
    at: Vec2,
    tier: u32,
    elite: bool,
    hp_mul: f32,
    dmg_mul: f32,
    speed_mul: f32,
) -> Entity {
    let def = db.enemy(def_id);
    let tier_hp = 1.22f32.powi(tier as i32);
    let tier_dmg = 1.13f32.powi(tier as i32);
    let hp = def.hp * tier_hp * hp_mul * if elite { 4.0 } else { 1.0 };
    let phase = (at.x * 13.7 + at.y * 7.3).fract().abs() * 6.0;
    eng.world.spawn((
        Pos(at),
        PrevPos(at),
        Vel(Vec2::ZERO),
        Health::new(hp),
        StatusBag::new(),
        EnemyC {
            def_id: def_id.to_string(),
            state: AiState::Seek,
            timer: 30 + (phase * 17.0) as u16 % 60,
            phase,
            elite,
            hp_mul,
            dmg_mul: dmg_mul * tier_dmg * if elite { 1.5 } else { 1.0 },
            speed_mul,
            attack_cd: 0,
            orbit: phase,
        },
    ))
}

/// One AI pass. Also rebuilds the spatial grid (enemies + player + minions
/// + totems all insert themselves — one grid, two masks).
pub fn tick_enemies(eng: &mut Engine, gs: &mut Gs) {
    // ---- grid rebuild
    {
        let mut grid = eng.world.remove_resource::<SpatialGrid>().unwrap();
        grid.clear();
        let mut ins: Vec<(Entity, Vec2, f32, u32)> = Vec::new();
        eng.world.each::<(&Pos, &EnemyC)>(|e, (p, ec)| {
            let r = gs.db.enemy(&ec.def_id).radius * if ec.elite { 1.5 } else { 1.0 };
            ins.push((e, p.0, r, MASK_ENEMY));
        });
        eng.world.each::<(&Pos, &MinionC)>(|e, (p, _)| {
            ins.push((e, p.0, 0.5, MASK_PLAYER));
        });
        eng.world.each::<(&Pos, &TotemC)>(|e, (p, _)| {
            ins.push((e, p.0, 0.6, MASK_PLAYER));
        });
        if let Some(pp) = eng.world.get::<Pos>(gs.pc.entity) {
            ins.push((gs.pc.entity, pp.0, 0.5, MASK_PLAYER));
        }
        for (e, p, r, m) in ins {
            grid.insert(e, p, r, m);
        }
        eng.world.insert_resource(grid);
    }

    let ppos = eng
        .world
        .get::<Pos>(gs.pc.entity)
        .map(|p| p.0)
        .unwrap_or(Vec2::ZERO);
    let mut rng = eng.streams.get("ai").clone();

    struct Attack {
        source: Entity,
        target_enemy: Option<Entity>, // Discord: enemy-on-enemy violence
        dmg: f32,
        dtype: DmgType,
        at: Vec2,
        inflict: Option<StatusApply>,
    }
    struct EShot {
        from: Vec2,
        dir: Vec2,
        dmg: f32,
        dtype: DmgType,
        speed: f32,
        glow: bool,
    }
    struct HealPulse {
        at: Vec2,
        amount: f32,
    }
    let mut attacks: Vec<Attack> = Vec::new();
    let mut shots: Vec<EShot> = Vec::new();
    let mut heals: Vec<HealPulse> = Vec::new();

    {
        let grid = eng.world.remove_resource::<SpatialGrid>().unwrap();
        let db = &gs.db;
        let blight = gs.blight_phase;
        eng.world
            .each::<(&mut Pos, &mut Vel, &mut EnemyC, &StatusBag)>(|ent, (p, v, ec, bag)| {
                let def = db.enemy(&ec.def_id);
                let petrified = bag.has(ST_PETRIFY);
                if petrified {
                    v.0 = Vec2::ZERO;
                    return;
                }
                let discorded = bag.has(ST_DISCORD);
                ec.timer = ec.timer.saturating_sub(1);
                ec.attack_cd = ec.attack_cd.saturating_sub(1);

                // Target selection: player (plus their servants), unless
                // Discorded — then the nearest fellow enemy gets it.
                let (tgt_pos, tgt_enemy): (Vec2, Option<Entity>) = if discorded {
                    let mut near = Vec::new();
                    grid.query_radius(p.0, 12.0, MASK_ENEMY, &mut near);
                    near.retain(|(e, _)| *e != ent);
                    if let Some((e, ep)) = near.first() {
                        (*ep, Some(*e))
                    } else {
                        (ppos, None)
                    }
                } else {
                    (ppos, None)
                };

                let to = tgt_pos - p.0;
                let dist = to.length().max(0.01);
                let dir = to / dist;
                let speed = def.speed * ec.speed_mul * if blight { 1.08 } else { 1.0 };
                let melee_range = def.radius + 0.9;

                match &def.ai {
                    EnemyAiKind::Melee => {
                        if dist > melee_range {
                            v.0 = dir * speed;
                        } else {
                            v.0 = Vec2::ZERO;
                            if ec.attack_cd == 0 {
                                ec.attack_cd = 62;
                                attacks.push(Attack {
                                    source: ent,
                                    target_enemy: tgt_enemy,
                                    dmg: def.dmg * ec.dmg_mul * apart_mult(def, &grid, p.0),
                                    dtype: def.dmg_type,
                                    at: tgt_pos,
                                    inflict: def.inflict.clone(),
                                });
                            }
                        }
                    }
                    EnemyAiKind::Charger => match ec.state {
                        AiState::Telegraph => {
                            v.0 = Vec2::ZERO;
                            if ec.timer == 0 {
                                ec.state = AiState::Lunge;
                                ec.timer = 22;
                                v.0 = dir * speed * 4.0;
                            }
                        }
                        AiState::Lunge => {
                            p.0 += v.0 * FIXED_DT;
                            if ec.timer == 0 {
                                ec.state = AiState::Seek;
                                ec.timer = 40 + rng.range_u32(50) as u16;
                            }
                            if dist < melee_range && ec.attack_cd == 0 {
                                ec.attack_cd = 48;
                                attacks.push(Attack {
                                    source: ent,
                                    target_enemy: tgt_enemy,
                                    dmg: def.dmg * ec.dmg_mul * 1.5,
                                    dtype: def.dmg_type,
                                    at: tgt_pos,
                                    inflict: def.inflict.clone(),
                                });
                            }
                        }
                        _ => {
                            v.0 = dir * speed;
                            if dist < 7.0 && ec.timer == 0 {
                                ec.state = AiState::Telegraph;
                                ec.timer = 28; // the tell (Pillar 3)
                            }
                        }
                    },
                    EnemyAiKind::Ranged { range, proj_speed } => {
                        if dist > *range {
                            v.0 = dir * speed;
                        } else if dist < range * 0.55 {
                            v.0 = -dir * speed * 0.8;
                        } else {
                            v.0 = Vec2::new(-dir.y, dir.x) * speed * 0.4;
                        }
                        if dist < range * 1.1 && ec.attack_cd == 0 {
                            ec.attack_cd = 70 + rng.range_u32(40) as u16;
                            shots.push(EShot {
                                from: p.0,
                                dir,
                                dmg: def.dmg * ec.dmg_mul,
                                dtype: def.dmg_type,
                                speed: *proj_speed,
                                glow: true,
                            });
                        }
                    }
                    EnemyAiKind::Support { heal } => {
                        // Drift near allies, pulse heals.
                        if dist < 9.0 {
                            v.0 = -dir * speed * 0.7;
                        } else {
                            v.0 = Vec2::new(-dir.y, dir.x) * speed * 0.5;
                        }
                        if ec.attack_cd == 0 {
                            ec.attack_cd = 90;
                            heals.push(HealPulse {
                                at: p.0,
                                amount: *heal * (1.0 + gs.tier as f32 * 0.2),
                            });
                        }
                    }
                    EnemyAiKind::Wheel { orbit_radius } => {
                        ec.orbit += 0.014 * speed.max(1.0) / orbit_radius.max(1.0) * 4.0;
                        let center = Vec2::ZERO; // arena-local wheels orbit room center set at spawn offset
                        let _ = center;
                        // Wheels roll toward the player in a wide arc.
                        let arc = Vec2::new(dir.y, -dir.x);
                        v.0 = (dir * 0.7 + arc * 0.7).normalize_or_zero() * speed;
                        if dist < melee_range + 0.4 && ec.attack_cd == 0 {
                            ec.attack_cd = 35;
                            attacks.push(Attack {
                                source: ent,
                                target_enemy: tgt_enemy,
                                dmg: def.dmg * ec.dmg_mul,
                                dtype: def.dmg_type,
                                at: tgt_pos,
                                inflict: def.inflict.clone(),
                            });
                        }
                    }
                }
                p.0 += v.0 * FIXED_DT;
            });
        eng.world.insert_resource(grid);
    }
    *eng.streams.get("ai") = rng;

    // Keep enemies on walkable ground.
    let mut clamp: Vec<(Entity, Vec2, Vec2)> = Vec::new();
    eng.world
        .each::<(&Pos, &PrevPos, &EnemyC)>(|e, (p, pp, _)| {
            if !crate::run::pos_walkable(gs, p.0) {
                clamp.push((e, pp.0, p.0));
            }
        });
    for (e, prev, _) in clamp {
        if let Some(p) = eng.world.get_mut::<Pos>(e) {
            p.0 = prev;
        }
    }

    // Resolve attacks.
    for a in attacks {
        match a.target_enemy {
            Some(victim) => {
                // Discord: enemies strike each other through the same pipeline
                // the player uses — no special-case damage math.
                let at = eng.world.get::<Pos>(victim).map(|p| p.0).unwrap_or(a.at);
                let mut dmg = [0.0; 4];
                dmg[a.dtype.index()] = a.dmg * 1.5;
                if let Some(h) = eng.world.get_mut::<Health>(victim) {
                    h.hp -= dmg_total(&dmg);
                    h.flash = 1.0;
                    let dead = h.hp <= 0.0;
                    eng.floaters.spawn(
                        Vec3::new(at.x, 0.6, at.y),
                        format!("{:.0}", dmg_total(&dmg)),
                        palette::BLOOD.extend(0.9),
                        1.2,
                    );
                    if dead {
                        // Discord kills still pay out — riots are a loot strategy.
                        kill_enemy(eng, gs, victim, at, 0.0);
                    }
                }
            }
            None => {
                // Might hit a minion/totem instead of the player if closer.
                let hit_servant = nearest_servant(eng, gs, a.at, 1.4);
                if let Some(srv) = hit_servant {
                    if let Some(h) = eng.world.get_mut::<Health>(srv) {
                        h.hp -= a.dmg;
                        h.flash = 1.0;
                    }
                } else {
                    let pp = eng
                        .world
                        .get::<Pos>(gs.pc.entity)
                        .map(|p| p.0)
                        .unwrap_or(Vec2::ZERO);
                    if pp.distance(a.at) < 1.6 {
                        hit_player(eng, gs, a.dmg, a.dtype, false);
                        if let Some(inf) = &a.inflict {
                            let roll = eng.streams.get("ai").next_f32();
                            if roll < inf.chance && can_status_player(gs, &inf.status) {
                                apply_status_to(
                                    eng,
                                    gs,
                                    gs.pc.entity,
                                    &inf.status,
                                    inf.stacks,
                                    a.dmg * inf.magnitude,
                                );
                                if status_by_name(&inf.status) == ST_DISCORD {
                                    gs.pc.discorded = 300;
                                }
                            }
                        }
                    }
                }
                let _ = a.source;
            }
        }
    }
    for s in shots {
        let mut dmg = [0.0; 4];
        dmg[s.dtype.index()] = s.dmg;
        eng.world.spawn((
            Pos(s.from + s.dir * 0.8),
            PrevPos(s.from),
            Vel(s.dir * s.speed),
            Proj {
                friendly: false,
                dmg,
                radius: 0.3,
                life: 200,
                pierce: 0,
                apply: vec![],
                slot: 0,
                last_hit: Entity::DEAD,
                orbit: None,
                glow: s.glow,
                power: 1.0,
            },
        ));
    }
    for hp in heals {
        let mut near = Vec::new();
        eng.world
            .resource::<SpatialGrid>()
            .query_radius(hp.at, 7.0, MASK_ENEMY, &mut near);
        for (e, _) in near {
            if let Some(h) = eng.world.get_mut::<Health>(e) {
                h.hp = (h.hp + hp.amount).min(h.max);
            }
        }
        spawn_ring(eng, hp.at, 7.0, palette::ICHOR);
    }

    // Regenerators (blight clergy) + bloom-phase realm regen.
    let bloom_regen = if !gs.blight_phase { 1.0 } else { 0.0 };
    let mut regens: Vec<(Entity, f32)> = Vec::new();
    eng.world.each::<(&EnemyC, &Health)>(|e, (ec, h)| {
        let r = gs.db.enemy(&ec.def_id).regen + bloom_regen;
        if r > 0.0 && h.hp < h.max {
            regens.push((e, r * FIXED_DT));
        }
    });
    for (e, r) in regens {
        if let Some(h) = eng.world.get_mut::<Health>(e) {
            h.hp = (h.hp + r).min(h.max);
        }
    }

    // Enemies killed by non-pipeline means (discord chip, zones) — sweep.
    let mut dead = Vec::new();
    eng.world.each::<(&Pos, &Health, &EnemyC)>(|e, (p, h, _)| {
        if h.hp <= 0.0 {
            dead.push((e, p.0));
        }
    });
    for (e, at) in dead {
        kill_enemy(eng, gs, e, at, 0.0);
    }
}

fn can_status_player(gs: &Gs, status: &str) -> bool {
    if status_by_name(status) == ST_DISCORD {
        return gs.build.discord_power().is_some();
    }
    true
}

/// Schism knights: stronger apart. Pillar 2 says the *mechanic* is data.
fn apart_mult(def: &EnemyDef, grid: &SpatialGrid, at: Vec2) -> f32 {
    if def.apart_bonus <= 1.0 {
        return 1.0;
    }
    let mut near = Vec::new();
    grid.query_radius(at, 6.0, MASK_ENEMY, &mut near);
    if near.len() <= 1 {
        def.apart_bonus
    } else {
        1.0
    }
}

fn nearest_servant(eng: &mut Engine, gs: &Gs, at: Vec2, r: f32) -> Option<Entity> {
    let mut near = Vec::new();
    eng.world
        .resource::<SpatialGrid>()
        .query_radius(at, r, MASK_PLAYER, &mut near);
    near.retain(|(e, _)| *e != gs.pc.entity);
    near.first().map(|(e, _)| *e)
}

// ------------------------------------------------------------------ bosses

pub fn tick_boss(eng: &mut Engine, gs: &mut Gs) {
    let ppos = eng
        .world
        .get::<Pos>(gs.pc.entity)
        .map(|p| p.0)
        .unwrap_or(Vec2::ZERO);
    let mut actions: Vec<(Entity, BossKind, u8, Vec2)> = Vec::new();
    eng.world
        .each::<(&Pos, &mut BossC, &Health)>(|e, (p, b, h)| {
            b.timer = b.timer.saturating_sub(1);
            let frac = h.hp / h.max;
            let phase = if frac < 0.33 {
                2
            } else if frac < 0.66 {
                1
            } else {
                0
            };
            if phase != b.phase as usize {
                b.phase = phase as u8;
                b.timer = 0; // phase transition acts immediately
            }
            if b.timer == 0 {
                b.timer = match b.kind {
                    BossKind::Vassago => 140,
                    BossKind::Andras => 120,
                    BossKind::Buer => 150,
                } - (phase as u16) * 25;
                actions.push((e, b.kind, phase as u8, p.0));
            }
        });

    for (boss, kind, phase, bpos) in actions {
        match kind {
            BossKind::Vassago => {
                // Light-orb spreads: his attacks are the only lamps in the dark.
                let n = 8 + phase as u32 * 4;
                for i in 0..n {
                    let a =
                        i as f32 / n as f32 * std::f32::consts::TAU + eng.clock.tick as f32 * 0.01;
                    let dir = Vec2::new(a.cos(), a.sin());
                    let mut dmg = [0.0; 4];
                    dmg[DmgType::Hex.index()] = 9.0 + gs.tier as f32 * 2.5;
                    eng.world.spawn((
                        Pos(bpos + dir),
                        PrevPos(bpos),
                        Vel(dir * 8.0),
                        Proj {
                            friendly: false,
                            dmg,
                            radius: 0.35,
                            life: 260,
                            pierce: 0,
                            apply: vec![],
                            slot: 0,
                            last_hit: Entity::DEAD,
                            orbit: None,
                            glow: true, // the point: he illuminates himself
                            power: 1.0,
                        },
                    ));
                }
                if phase >= 1 {
                    // Adds from the stacks.
                    for i in 0..2 {
                        let a = i as f32 * 3.1 + eng.clock.tick as f32 * 0.1;
                        let at = bpos + Vec2::new(a.cos(), a.sin()) * 6.0;
                        spawn_enemy(
                            eng,
                            &gs.db,
                            "index_wraith",
                            at,
                            gs.tier,
                            false,
                            1.0,
                            1.0,
                            1.0,
                        );
                    }
                }
            }
            BossKind::Andras => {
                // Reflection: your own last trigger, thrown back at your feet.
                if let Some(k) = gs.last_player_trigger {
                    gs.boss_reflect.push((k, ppos));
                }
                if phase >= 1 {
                    // Discord wave: the arena itself riots.
                    let mut near = Vec::new();
                    eng.world
                        .resource::<SpatialGrid>()
                        .query_radius(bpos, 18.0, MASK_ENEMY, &mut near);
                    for (e, _) in near.into_iter().take(6) {
                        if e != boss {
                            apply_status_to(eng, gs, e, "discord", 2, 5.0);
                        }
                    }
                    // And you, if you signed his pact.
                    if gs.build.discord_power().is_some() {
                        apply_status_to(eng, gs, gs.pc.entity, "discord", 1, 0.0);
                        gs.pc.discorded = 300;
                    }
                }
            }
            BossKind::Buer => {
                // Spore volley toward the player; wheel movement is in AI.
                let dir = (ppos - bpos).normalize_or_zero();
                for i in 0..(3 + phase as u32) {
                    let spread = (i as f32 - 1.0) * 0.35;
                    let (s, c) = spread.sin_cos();
                    let d = Vec2::new(dir.x * c - dir.y * s, dir.x * s + dir.y * c);
                    let mut dmg = [0.0; 4];
                    dmg[DmgType::Void.index()] = 8.0 + gs.tier as f32 * 2.0;
                    eng.world.spawn((
                        Pos(bpos + d * 1.5),
                        PrevPos(bpos),
                        Vel(d * 7.0),
                        Proj {
                            friendly: false,
                            dmg,
                            radius: 0.4,
                            life: 240,
                            pierce: 0,
                            apply: vec![],
                            slot: 0,
                            last_hit: Entity::DEAD,
                            orbit: None,
                            glow: true,
                            power: 1.0,
                        },
                    ));
                }
                if phase >= 1 && !gs.blight_phase {
                    // Bloom phase: the garden heals its clergy — and him.
                    let heal = 40.0 + gs.tier as f32 * 10.0;
                    if let Some(h) = eng.world.get_mut::<Health>(boss) {
                        h.hp = (h.hp + heal).min(h.max);
                    }
                    spawn_ring(eng, bpos, 5.0, palette::ICHOR);
                }
            }
        }
    }
}

/// BUER phase immunity: callers zero damage against him during bloom.
pub fn buer_immune(eng: &mut Engine, gs: &Gs, target: Entity) -> bool {
    if gs.blight_phase {
        return false;
    }
    eng.world
        .get::<BossC>(target)
        .map(|b| b.kind == BossKind::Buer)
        .unwrap_or(false)
}
