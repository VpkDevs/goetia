//! The eight verbs and everything they spawn. Sigil ops fold over a SkillDef
//! to produce the modified cast — conversion, echoes, orbits, extra counts —
//! then this module runs the resulting entities every tick.

use crate::combat::*;
use crate::content::*;
use crate::vocab::*;
use goetia::prelude::*;

/// A SkillDef after sigils are applied.
pub struct ModSkill {
    pub dmg: DmgVec,
    pub apply: Vec<StatusApply>,
    pub kind: SkillKind,
    pub cd: u32,
    pub echo: f32, // >0: echo cast at pct
    pub orbit: bool,
    pub pierce_add: u32,
    pub count_add: u32,
    pub area_mul: f32,
    pub speed_mul: f32,
    pub duration_mul: f32,
    pub dmg_mul: f32,
}

pub fn modify_skill(
    db: &ContentDb,
    loadout: &crate::items::Loadout,
    slot: usize,
) -> Option<ModSkill> {
    let sid = loadout.skills.get(slot)?.as_ref()?;
    let def = db.skill(sid);
    let mut m = ModSkill {
        dmg: def.dmg_vec(),
        apply: def.apply.clone(),
        kind: def.kind.clone(),
        cd: def.cd_ticks,
        echo: 0.0,
        orbit: false,
        pierce_add: 0,
        count_add: 0,
        area_mul: 1.0,
        speed_mul: 1.0,
        duration_mul: 1.0,
        dmg_mul: 1.0,
    };
    for sig in loadout.sigils[slot].iter().flatten() {
        for op in &db.sigil(sig).ops {
            match op {
                SigilOp::Echo { pct } => m.echo = m.echo.max(*pct),
                SigilOp::Convert { from, to } => {
                    let v = m.dmg[from.index()];
                    m.dmg[from.index()] = 0.0;
                    m.dmg[to.index()] += v;
                }
                SigilOp::Pierce { add } => m.pierce_add += add,
                SigilOp::Orbit => m.orbit = true,
                SigilOp::AreaMul { m: x } => m.area_mul *= x,
                SigilOp::CdMul { m: x } => m.cd = ((m.cd as f32) * x).max(1.0) as u32,
                SigilOp::DmgMul { m: x } => m.dmg_mul *= x,
                SigilOp::SpeedMul { m: x } => m.speed_mul *= x,
                SigilOp::CountAdd { n } => m.count_add += n,
                SigilOp::DurationMul { m: x } => m.duration_mul *= x,
                SigilOp::AddApply(a) => m.apply.push(a.clone()),
                SigilOp::React(_) => {} // handled at build compile
            }
        }
    }
    for d in &mut m.dmg {
        *d *= m.dmg_mul;
    }
    Some(m)
}

/// Cast the skill in `slot` toward `aim`. `power` scales damage (echoes cast
/// at reduced power). `free` skips the cooldown (echo/pending casts).
pub fn cast(eng: &mut Engine, gs: &mut Gs, slot: usize, power: f32, aim: Vec2, free: bool) {
    let Some(m) = modify_skill(&gs.db, &gs.loadout, slot) else {
        return;
    };
    if !free {
        if gs.pc.cooldowns[slot] > 0 {
            return;
        }
        let cast_speed = 1.0
            + gs.stat(K_CAST_SPEED)
            + if gs.pc.frenzy > 0 {
                gs.pc.frenzy_cast
            } else {
                0.0
            };
        gs.pc.cooldowns[slot] = ((m.cd as f32) / cast_speed.max(0.25)) as u16;
    }
    let ppos = eng
        .world
        .get::<Pos>(gs.pc.entity)
        .map(|p| p.0)
        .unwrap_or(Vec2::ZERO);
    let dir = (aim - ppos).normalize_or_zero();
    let dir = if dir.length_squared() < 0.5 {
        Vec2::new(1.0, 0.0)
    } else {
        dir
    };
    let mut dmg = m.dmg;
    for d in &mut dmg {
        *d *= power;
    }

    match &m.kind {
        SkillKind::Projectile {
            speed,
            radius,
            count,
            spread_deg,
            pierce,
            life_ticks,
        } => {
            let n = count + m.count_add;
            let speed = speed * m.speed_mul * (1.0 + gs.stat(K_PROJ_SPEED));
            for i in 0..n {
                let t = if n == 1 {
                    0.0
                } else {
                    i as f32 / (n - 1) as f32 - 0.5
                };
                let ang = t * spread_deg.to_radians();
                let (s, c) = ang.sin_cos();
                let d = Vec2::new(dir.x * c - dir.y * s, dir.x * s + dir.y * c);
                if m.orbit {
                    let angle = i as f32 / n as f32 * std::f32::consts::TAU;
                    eng.world.spawn((
                        Pos(ppos),
                        PrevPos(ppos),
                        Vel(Vec2::ZERO),
                        Proj {
                            friendly: true,
                            dmg,
                            radius: *radius * 1.2,
                            life: ((*life_ticks as f32) * m.duration_mul * 3.0) as u16,
                            pierce: u32::MAX, // orbiters grind
                            apply: m.apply.clone(),
                            slot,
                            last_hit: Entity::DEAD,
                            orbit: Some(angle),
                            glow: true,
                            power,
                        },
                    ));
                } else {
                    eng.world.spawn((
                        Pos(ppos + d * 0.8),
                        PrevPos(ppos + d * 0.8),
                        Vel(d * speed),
                        Proj {
                            friendly: true,
                            dmg,
                            radius: *radius,
                            life: ((*life_ticks as f32) * m.duration_mul) as u16,
                            pierce: pierce + m.pierce_add,
                            apply: m.apply.clone(),
                            slot,
                            last_hit: Entity::DEAD,
                            orbit: None,
                            glow: true,
                            power,
                        },
                    ));
                }
            }
            eng.audio
                .play(&gs.sounds.cast, "sfx", 0.3, 1.0 + slot as f32 * 0.07);
        }
        SkillKind::Nova { radius } => {
            nova_at(eng, gs, ppos, radius * m.area_mul, dmg, &m.apply, false);
            eng.shake(0.12);
        }
        SkillKind::Ground {
            radius,
            duration_ticks,
            tick_interval,
            consecrate,
        } => {
            eng.world.spawn((
                Pos(aim),
                Zone {
                    friendly: true,
                    radius: radius * m.area_mul * (1.0 + gs.stat(K_AOE)),
                    life: ((*duration_ticks as f32) * m.duration_mul) as u16,
                    tick_interval: *tick_interval as u16,
                    timer: 0,
                    dmg,
                    apply: m.apply.clone(),
                    consecrate: *consecrate,
                    inverted: false,
                    telegraph_burst: None,
                    power,
                    slot,
                },
            ));
            eng.audio.play(&gs.sounds.ritual, "sfx", 0.4, 0.9);
        }
        SkillKind::Minion {
            count,
            life_ticks,
            attack_cd,
            speed,
        } => {
            let n = count + m.count_add;
            let minion_bonus = 1.0 + gs.stat(K_MINION);
            let mut md = dmg;
            for d in &mut md {
                *d *= minion_bonus;
            }
            for i in 0..n {
                let a = i as f32 / n as f32 * std::f32::consts::TAU;
                let off = Vec2::new(a.cos(), a.sin()) * 1.5;
                eng.world.spawn((
                    Pos(ppos + off),
                    PrevPos(ppos + off),
                    Vel(Vec2::ZERO),
                    Health::new(40.0 + gs.tier as f32 * 10.0),
                    StatusBag::new(),
                    MinionC {
                        life: ((*life_ticks as f32) * m.duration_mul) as u16,
                        attack_cd: *attack_cd as u16,
                        timer: 0,
                        dmg: md,
                        speed: *speed,
                        slot,
                        power,
                    },
                ));
            }
            eng.audio.play(&gs.sounds.summon, "sfx", 0.45, 0.8);
        }
        SkillKind::Beam { .. } => {
            // Channel start; beam damage runs in tick_channel while held.
            gs.pc.channel = Some(slot);
            gs.pc.channel_tick = 0;
        }
        SkillKind::Dash { dist } => {
            player_dash(eng, gs, dir, *dist, &dmg, &m.apply, slot);
        }
        SkillKind::Curse { radius } => {
            let r = radius * m.area_mul * (1.0 + gs.stat(K_AOE));
            let mut near = Vec::new();
            eng.world
                .resource::<SpatialGrid>()
                .query_radius(aim, r, MASK_ENEMY, &mut near);
            let total = dmg_total(&dmg).max(4.0);
            for (e, _) in near {
                for ap in &m.apply {
                    apply_status_to(eng, gs, e, &ap.status, ap.stacks, total * ap.magnitude);
                }
            }
            spawn_ring(eng, aim, r, palette::HEX);
            eng.audio.play(&gs.sounds.curse, "sfx", 0.4, 1.1);
        }
        SkillKind::Totem {
            duration_ticks,
            fire_cd,
            proj_speed,
        } => {
            eng.world.spawn((
                Pos(aim),
                PrevPos(aim),
                Health::new(60.0 + gs.tier as f32 * 15.0),
                StatusBag::new(),
                TotemC {
                    life: ((*duration_ticks as f32) * m.duration_mul) as u16,
                    fire_cd: *fire_cd as u16,
                    timer: 0,
                    dmg,
                    proj_speed: *proj_speed,
                    slot,
                    power,
                    apply: m.apply.clone(),
                },
            ));
            eng.audio.play(&gs.sounds.summon, "sfx", 0.4, 1.2);
        }
    }

    // Cast accounting: every 5th cast is the vocabulary's "nth cast".
    gs.pc.cast_count += 1;
    gs.pc.last_cast = Some((slot, aim));
    if gs.pc.cast_count.is_multiple_of(5) {
        eng.triggers.emit(
            TR_NTH_CAST,
            gs.pc.entity,
            gs.pc.entity,
            gs.pc.cast_count as f32,
        );
        gs.last_player_trigger = Some(TR_NTH_CAST);
    }
    // Echo sigil: queue a weaker recast (echoes count as casts; loops are
    // the trigger budget's problem, not ours).
    if m.echo > 0.0 && power > 0.15 {
        gs.pc.pending_casts.push((slot, power * m.echo, aim));
    }
}

pub fn player_dash(
    eng: &mut Engine,
    gs: &mut Gs,
    dir: Vec2,
    dist: f32,
    dmg: &DmgVec,
    applies: &[StatusApply],
    _slot: usize,
) {
    let ppos = eng
        .world
        .get::<Pos>(gs.pc.entity)
        .map(|p| p.0)
        .unwrap_or(Vec2::ZERO);
    let to = crate::run::clamp_walkable(gs, ppos, ppos + dir * dist);
    // Rip through: damage along the path.
    if dmg_total(dmg) > 0.0 {
        let mut near = Vec::new();
        let mid = (ppos + to) * 0.5;
        eng.world.resource::<SpatialGrid>().query_radius(
            mid,
            dist * 0.6 + 1.0,
            MASK_ENEMY,
            &mut near,
        );
        for (e, ep) in near {
            // Only those close to the segment.
            let seg = to - ppos;
            let t = ((ep - ppos).dot(seg) / seg.length_squared()).clamp(0.0, 1.0);
            if (ppos + seg * t).distance(ep) < 1.2 {
                hit_enemy(eng, gs, e, ep, *dmg, applies, true, false);
            }
        }
    }
    if let Some(p) = eng.world.get_mut::<Pos>(gs.pc.entity) {
        p.0 = to;
    }
    gs.pc.iframes = 14;
    crate::combat::spawn_burst(eng, ppos, palette::BONE, 14, 2.0);
    eng.audio.play(&gs.sounds.dodge, "sfx", 0.35, 1.3);
    eng.triggers.emit(TR_DODGE, gs.pc.entity, gs.pc.entity, 1.0);
    gs.last_player_trigger = Some(TR_DODGE);
}

// ------------------------------------------------------------ tick systems

pub fn tick_projectiles(eng: &mut Engine, gs: &mut Gs) {
    let ppos = eng
        .world
        .get::<Pos>(gs.pc.entity)
        .map(|p| p.0)
        .unwrap_or(Vec2::ZERO);
    struct Hit {
        proj: Entity,
        target: Entity,
        at: Vec2,
        dmg: DmgVec,
        applies: Vec<StatusApply>,
        spent: bool,
        friendly: bool,
    }
    let mut hits: Vec<Hit> = Vec::new();
    let mut dead: Vec<Entity> = Vec::new();
    {
        let grid = eng.world.remove_resource::<SpatialGrid>().unwrap();
        eng.world
            .each::<(&mut Pos, &mut PrevPos, &mut Vel, &mut Proj)>(|ent, (p, pp, v, pr)| {
                pp.0 = p.0;
                pr.life = pr.life.saturating_sub(1);
                if pr.life == 0 {
                    dead.push(ent);
                    return;
                }
                if let Some(angle) = &mut pr.orbit {
                    *angle += 0.09;
                    let r = 2.6;
                    p.0 = ppos + Vec2::new(angle.cos(), angle.sin()) * r;
                } else {
                    p.0 += v.0 * FIXED_DT;
                }
                let mask = if pr.friendly { MASK_ENEMY } else { MASK_PLAYER };
                if let Some((hit, at, _)) = grid.sweep(pp.0, p.0, pr.radius, mask) {
                    if hit != pr.last_hit {
                        pr.last_hit = hit;
                        let spent = if pr.pierce == 0 {
                            true
                        } else {
                            pr.pierce = pr.pierce.saturating_sub(1);
                            false
                        };
                        hits.push(Hit {
                            proj: ent,
                            target: hit,
                            at,
                            dmg: pr.dmg,
                            applies: pr.apply.clone(),
                            spent,
                            friendly: pr.friendly,
                        });
                    }
                }
            });
        eng.world.insert_resource(grid);
    }
    for h in hits {
        if h.spent {
            eng.commands.despawn(h.proj);
        }
        if h.friendly {
            hit_enemy(eng, gs, h.target, h.at, h.dmg, &h.applies, true, false);
        } else {
            hit_player(eng, gs, dmg_total(&h.dmg), dominant_type(&h.dmg), false);
        }
    }
    for e in dead {
        eng.commands.despawn(e);
    }
    // Walls eat non-orbiting projectiles.
    let mut walled = Vec::new();
    eng.world.each::<(&Pos, &Proj)>(|ent, (p, pr)| {
        if pr.orbit.is_none() && !crate::run::pos_walkable(gs, p.0) {
            walled.push(ent);
        }
    });
    for e in walled {
        eng.commands.despawn(e);
    }
}

pub fn tick_zones(eng: &mut Engine, gs: &mut Gs) {
    struct ZoneTick {
        at: Vec2,
        radius: f32,
        dmg: DmgVec,
        applies: Vec<StatusApply>,
        consecrate: bool,
        inverted: bool,
        friendly: bool,
        burst: Option<DmgVec>,
    }
    let mut ticks: Vec<ZoneTick> = Vec::new();
    let mut dead: Vec<Entity> = Vec::new();
    eng.world.each::<(&Pos, &mut Zone)>(|ent, (p, z)| {
        if z.life == 0 {
            if let Some(b) = z.telegraph_burst {
                ticks.push(ZoneTick {
                    at: p.0,
                    radius: z.radius,
                    dmg: b,
                    applies: vec![],
                    consecrate: false,
                    inverted: false,
                    friendly: z.friendly,
                    burst: Some(b),
                });
            }
            dead.push(ent);
            return;
        }
        z.life -= 1;
        if z.tick_interval > 0 {
            z.timer = z.timer.saturating_sub(1);
            if z.timer == 0 {
                z.timer = z.tick_interval;
                ticks.push(ZoneTick {
                    at: p.0,
                    radius: z.radius,
                    dmg: z.dmg,
                    applies: z.apply.clone(),
                    consecrate: z.consecrate,
                    inverted: z.inverted,
                    friendly: z.friendly,
                    burst: None,
                });
            }
        }
    });
    for t in ticks {
        if let Some(b) = t.burst {
            // Hostile telegraph pop: damages the player if inside.
            let pp = eng
                .world
                .get::<Pos>(gs.pc.entity)
                .map(|p| p.0)
                .unwrap_or(Vec2::ZERO);
            spawn_ring(eng, t.at, t.radius, palette::BLOOD);
            if !t.friendly && pp.distance(t.at) < t.radius {
                hit_player(eng, gs, dmg_total(&b), dominant_type(&b), false);
            }
            continue;
        }
        if t.friendly {
            let mut near = Vec::new();
            eng.world
                .resource::<SpatialGrid>()
                .query_radius(t.at, t.radius, MASK_ENEMY, &mut near);
            for (e, ep) in near {
                hit_enemy(eng, gs, e, ep, t.dmg, &t.applies, false, true);
            }
            // Consecration heals its caster standing inside (unless inverted).
            if t.consecrate {
                let pp = eng
                    .world
                    .get::<Pos>(gs.pc.entity)
                    .map(|p| p.0)
                    .unwrap_or(Vec2::ZERO);
                if pp.distance(t.at) < t.radius {
                    if t.inverted {
                        hit_player(eng, gs, dmg_total(&t.dmg) * 0.4, DmgType::Hex, true);
                    } else {
                        let heal = gs.stat(K_MAX_HP) * 0.015;
                        heal_player(eng, gs, heal);
                    }
                }
            }
        }
    }
    for e in dead {
        eng.commands.despawn(e);
    }
}

pub fn tick_minions_totems(eng: &mut Engine, gs: &mut Gs) {
    let inherit = gs.build.has_rule(&Rule::ServantsInherit);
    // Minions seek nearest enemy and swing.
    struct Swing {
        target: Entity,
        at: Vec2,
        dmg: DmgVec,
    }
    let mut swings: Vec<Swing> = Vec::new();
    let mut dead: Vec<Entity> = Vec::new();
    {
        let grid = eng.world.remove_resource::<SpatialGrid>().unwrap();
        eng.world
            .each::<(&mut Pos, &mut Vel, &mut MinionC, &Health)>(|ent, (p, v, m, h)| {
                m.life = m.life.saturating_sub(1);
                if m.life == 0 || h.hp <= 0.0 {
                    dead.push(ent);
                    return;
                }
                m.timer = m.timer.saturating_sub(1);
                let mut near = Vec::new();
                grid.query_radius(p.0, 10.0, MASK_ENEMY, &mut near);
                if let Some((tgt, tp)) = near
                    .iter()
                    .min_by(|a, b| {
                        a.1.distance_squared(p.0)
                            .partial_cmp(&b.1.distance_squared(p.0))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .copied()
                {
                    let d = tp - p.0;
                    if d.length() > 1.2 {
                        v.0 = d.normalize_or_zero() * m.speed;
                        p.0 += v.0 * FIXED_DT;
                    } else if m.timer == 0 {
                        m.timer = m.attack_cd;
                        swings.push(Swing {
                            target: tgt,
                            at: tp,
                            dmg: m.dmg,
                        });
                    }
                } else {
                    v.0 = Vec2::ZERO;
                }
            });
        eng.world.insert_resource(grid);
    }
    for s in swings {
        let r = hit_enemy(eng, gs, s.target, s.at, s.dmg, &[], true, false);
        // Servants inherit: their kills/crits already emit triggers via the
        // shared pipeline, which is the whole point.
        let _ = (r, inherit);
    }

    // Totems fire projectiles at the nearest enemy.
    struct Shot {
        from: Vec2,
        dir: Vec2,
        dmg: DmgVec,
        speed: f32,
        slot: usize,
        power: f32,
        applies: Vec<StatusApply>,
    }
    let mut shots: Vec<Shot> = Vec::new();
    {
        let grid = eng.world.remove_resource::<SpatialGrid>().unwrap();
        eng.world
            .each::<(&Pos, &mut TotemC, &Health)>(|ent, (p, t, h)| {
                t.life = t.life.saturating_sub(1);
                if t.life == 0 || h.hp <= 0.0 {
                    dead.push(ent);
                    return;
                }
                t.timer = t.timer.saturating_sub(1);
                if t.timer == 0 {
                    let mut near = Vec::new();
                    grid.query_radius(p.0, 14.0, MASK_ENEMY, &mut near);
                    if let Some((_, tp)) = near.first() {
                        t.timer = t.fire_cd;
                        shots.push(Shot {
                            from: p.0,
                            dir: (*tp - p.0).normalize_or_zero(),
                            dmg: t.dmg,
                            speed: t.proj_speed,
                            slot: t.slot,
                            power: t.power,
                            applies: t.apply.clone(),
                        });
                    }
                }
            });
        eng.world.insert_resource(grid);
    }
    for s in shots {
        eng.world.spawn((
            Pos(s.from + s.dir * 0.6),
            PrevPos(s.from),
            Vel(s.dir * s.speed),
            Proj {
                friendly: true,
                dmg: s.dmg,
                radius: 0.25,
                life: 120,
                pierce: 0,
                apply: s.applies,
                slot: s.slot,
                last_hit: Entity::DEAD,
                orbit: None,
                glow: true,
                power: s.power,
            },
        ));
        eng.audio.play(&gs.sounds.cast, "sfx", 0.12, 1.4);
    }
    for e in dead {
        eng.commands.despawn(e);
    }
}

/// Beam channel: damage along the aim line while the key is held.
pub fn tick_channel(eng: &mut Engine, gs: &mut Gs, held: bool) {
    let Some(slot) = gs.pc.channel else { return };
    if !held {
        gs.pc.channel = None;
        return;
    }
    let Some(m) = modify_skill(&gs.db, &gs.loadout, slot) else {
        gs.pc.channel = None;
        return;
    };
    let SkillKind::Beam {
        range,
        width,
        tick_interval,
    } = m.kind
    else {
        gs.pc.channel = None;
        return;
    };
    gs.pc.channel_tick += 1;
    let interval = ((tick_interval as f32) / (1.0 + gs.stat(K_CAST_SPEED))).max(2.0) as u16;
    if !gs.pc.channel_tick.is_multiple_of(interval) {
        return;
    }
    let ppos = eng
        .world
        .get::<Pos>(gs.pc.entity)
        .map(|p| p.0)
        .unwrap_or(Vec2::ZERO);
    let dir = (gs.pc.aim - ppos).normalize_or_zero();
    // Sample along the beam; unique targets only.
    let mut seen: Vec<Entity> = Vec::new();
    let steps = (range / 1.5).ceil() as u32;
    for i in 0..steps {
        let at = ppos + dir * (1.0 + i as f32 * 1.5);
        let mut near = Vec::new();
        eng.world.resource::<SpatialGrid>().query_radius(
            at,
            width * m.area_mul,
            MASK_ENEMY,
            &mut near,
        );
        for (e, ep) in near {
            if !seen.contains(&e) {
                seen.push(e);
                hit_enemy(eng, gs, e, ep, m.dmg, &m.apply, true, false);
            }
        }
    }
    // Beam ticks count as casts every 5th — this is a known engine of loops.
    gs.pc.cast_count += 1;
    gs.pc.last_cast = Some((slot, gs.pc.aim));
    if gs.pc.cast_count.is_multiple_of(5) {
        eng.triggers.emit(
            TR_NTH_CAST,
            gs.pc.entity,
            gs.pc.entity,
            gs.pc.cast_count as f32,
        );
    }
    eng.audio.play(&gs.sounds.beam, "sfx", 0.08, 1.0);
}
