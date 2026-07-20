//! A realm run: generation (layout, packs, shrines, altar, boss), the player
//! controller, the per-tick orchestration, and run completion/death flow.

use crate::combat::*;
use crate::content::*;
use crate::enemies::*;
use crate::fx::FxQueue;
use crate::vocab::*;
use goetia::prelude::*;
use std::collections::HashSet;

pub const CELL: f32 = 4.0;

pub struct RunState {
    pub demon: Demon,
    pub layout: RealmLayout,
    pub mods: Vec<RealmModDef>,
    pub boss: Entity,
    pub cleared: bool,
    pub entry: Vec2,
    pub portal_out: Option<Vec2>,
    pub shrines: Vec<(Vec2, bool)>, // (pos, used)
    pub altar: Vec2,
    pub boss_pos: Vec2,
    pub ticks: u64,
    pub cycle_timer: u32,
    pub hostile_shrines: bool,
    pub cycle_rate: f32,
}

// -------------------------------------------------------------- walkability

pub fn pos_walkable(gs: &Gs, p: Vec2) -> bool {
    match &gs.walkable {
        None => true,
        Some(set) => set.contains(&((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32)),
    }
}

pub fn clamp_walkable(gs: &Gs, from: Vec2, to: Vec2) -> Vec2 {
    if pos_walkable(gs, to) {
        return to;
    }
    // Try axis-aligned slides, then give up and stay.
    let tx = Vec2::new(to.x, from.y);
    if pos_walkable(gs, tx) {
        return tx;
    }
    let ty = Vec2::new(from.x, to.y);
    if pos_walkable(gs, ty) {
        return ty;
    }
    from
}

// -------------------------------------------------------------- generation

pub fn generate(eng: &mut Engine, gs: &mut Gs, demon: Demon, tier: u32) -> RunState {
    eng.world = World::new();
    eng.world.insert_resource(SpatialGrid::new(2.5));
    eng.world.insert_resource(FxQueue::default());
    gs.tier = tier;
    gs.run_inv.clear();
    gs.pc = crate::combat::PlayerCtx::new(Entity::DEAD);
    gs.blight_phase = gs.build.has_rule(&Rule::LockBlight);
    gs.boss_reflect.clear();
    gs.last_player_trigger = None;

    // Realm modifiers (tier 2+): risk is a dial the player owns.
    let mut mods: Vec<RealmModDef> = Vec::new();
    if tier >= 2 {
        let pool = gs.db.realm_mods().to_vec();
        let count = (2 + (tier / 4)).min(4) as usize;
        let mut idxs: Vec<usize> = (0..pool.len()).collect();
        eng.streams.get("packs").shuffle(&mut idxs);
        for i in idxs.into_iter().take(count) {
            mods.push(pool[i].clone());
        }
    }
    let hp_mul: f32 = mods.iter().map(|m| m.hp_mul).product();
    let dmg_mul: f32 = mods.iter().map(|m| m.dmg_mul).product();
    let speed_mul: f32 = mods.iter().map(|m| m.speed_mul).product();
    gs.loot_mul = mods.iter().map(|m| m.loot_mul).product::<f32>()
        * (1.0 + (tier as f32 - 1.0).max(0.0) * 0.12); // tiers pay
    gs.reveal_on_drop = mods.iter().any(|m| m.reveal_hidden);
    gs.death_novas = mods.iter().any(|m| m.death_novas);
    let spawn_discord: u32 = mods.iter().map(|m| m.spawn_discord).sum();
    let hostile_shrines = mods.iter().any(|m| m.hostile_shrines);
    let cycle_rate: f32 = mods.iter().map(|m| m.cycle_rate_mul).product();

    // Layout via the engine assembler.
    let realm = gs.db.realm(demon).clone();
    let templates = gs.db.room_templates(demon).to_vec();
    let grammar = gs.db.grammar(demon).clone();
    let layout =
        assemble(&templates, &grammar, eng.streams.get("layout")).expect("realm start template");

    // Walkable set: room cells + door cells.
    let mut walk: HashSet<(i32, i32)> = HashSet::new();
    for r in &layout.rooms {
        let t = &templates[r.template];
        for x in r.x..r.x + t.width as i32 {
            for y in r.y..r.y + t.height as i32 {
                walk.insert((x, y));
            }
        }
    }
    for (_, _, cell) in &layout.connections {
        walk.insert(*cell);
    }
    gs.walkable = Some(walk);

    // Room centers in world units.
    let center = |ri: usize| -> Vec2 {
        let r = &layout.rooms[ri];
        let t = &templates[r.template];
        Vec2::new(
            (r.x as f32 + t.width as f32 * 0.5) * CELL,
            (r.y as f32 + t.height as f32 * 0.5) * CELL,
        )
    };
    let entry = center(0);
    // Boss room: farthest from entry.
    let boss_room = (1..layout.rooms.len())
        .max_by_key(|&i| {
            let c = center(i);
            ((c - entry).length() * 10.0) as i64
        })
        .unwrap_or(layout.rooms.len().saturating_sub(1));
    let boss_pos = center(boss_room);

    // The player.
    let player = eng.world.spawn((
        Pos(entry),
        PrevPos(entry),
        Vel(Vec2::ZERO),
        Health::new(gs.build.sheet.get(K_MAX_HP)),
        StatusBag::new(),
        PlayerTag,
    ));
    gs.pc.entity = player;

    // Packs.
    let mut rng = eng.streams.get("packs").clone();
    let density = 2 + (tier / 2).min(9);
    for (ri, room) in layout.rooms.iter().enumerate() {
        if ri == 0 {
            continue;
        }
        let t = &templates[room.template];
        let area = (t.width * t.height) as f32;
        let is_boss_room = ri == boss_room;
        let packs = if is_boss_room {
            1
        } else {
            1 + (area / 12.0) as u32
        };
        for _ in 0..packs {
            let px = (room.x as f32 + rng.range_f32(0.6, t.width as f32 - 0.6)) * CELL;
            let py = (room.y as f32 + rng.range_f32(0.6, t.height as f32 - 0.6)) * CELL;
            let pack_at = Vec2::new(px, py);
            let size = if is_boss_room {
                2
            } else {
                density / 2 + rng.range_u32(density)
            };
            let weights: Vec<f32> = realm
                .enemies
                .iter()
                .map(|e| gs.db.enemy(e).weight)
                .collect();
            for _ in 0..size {
                let pick = rng.weighted_index(&weights).unwrap_or(0);
                let id = realm.enemies[pick].clone();
                let off = Vec2::new(rng.range_f32(-2.0, 2.0), rng.range_f32(-2.0, 2.0));
                let elite = rng.chance(0.06 + tier as f32 * 0.005);
                let e = spawn_enemy(
                    eng,
                    &gs.db,
                    &id,
                    pack_at + off,
                    tier,
                    elite,
                    hp_mul,
                    dmg_mul,
                    speed_mul,
                );
                if spawn_discord > 0 {
                    apply_status_to(eng, gs, e, "discord", spawn_discord, 3.0);
                }
            }
        }
    }

    // Boss.
    let boss_kind = match demon {
        Demon::Vassago => BossKind::Vassago,
        Demon::Andras => BossKind::Andras,
        Demon::Buer => BossKind::Buer,
    };
    let boss = spawn_enemy(
        eng,
        &gs.db,
        &realm.boss,
        boss_pos,
        tier,
        false,
        hp_mul * (realm.boss_hp / 100.0),
        dmg_mul,
        speed_mul,
    );
    eng.world.insert(
        boss,
        BossC {
            kind: boss_kind,
            timer: 90,
            phase: 0,
        },
    );

    // Shrines (Vassago's realm gets the reveal shrines) + corruption altar.
    let mut shrines = Vec::new();
    if demon == Demon::Vassago && layout.rooms.len() > 3 {
        for i in [1usize, layout.rooms.len() / 2] {
            if i != boss_room {
                shrines.push((center(i) + Vec2::new(1.5, 0.0), false));
            }
        }
    }
    let altar_room = (1..layout.rooms.len())
        .find(|&i| i != boss_room)
        .unwrap_or(0);
    let altar = center(altar_room) + Vec2::new(-1.5, 1.5);

    eng.camera.target = Vec3::new(entry.x, 0.0, entry.y);
    eng.camera.zoom = 16.0;
    eng.audio.play(&gs.sounds.portal, "sfx", 0.6, 1.0);

    RunState {
        demon,
        layout,
        mods,
        boss,
        cleared: false,
        entry,
        portal_out: None,
        shrines,
        altar,
        boss_pos,
        ticks: 0,
        cycle_timer: 0,
        hostile_shrines,
        cycle_rate,
    }
}

// ------------------------------------------------------------- player tick

pub struct InputFrame {
    pub mv: Vec2,
    pub aim: Vec2,
    pub cast: [bool; 6],
    pub channel_held: bool,
    pub dodge: bool,
    pub interact: bool,
}

pub fn read_input(eng: &mut Engine) -> InputFrame {
    let mut mv = Vec2::ZERO;
    // Screen-relative movement mapped to the iso ground plane.
    if eng.input.key_down(KeyCode::KeyW) {
        mv += Vec2::new(-1.0, -1.0);
    }
    if eng.input.key_down(KeyCode::KeyS) {
        mv += Vec2::new(1.0, 1.0);
    }
    if eng.input.key_down(KeyCode::KeyA) {
        mv += Vec2::new(-1.0, 1.0);
    }
    if eng.input.key_down(KeyCode::KeyD) {
        mv += Vec2::new(1.0, -1.0);
    }
    let g = eng.mouse_ground();
    InputFrame {
        mv: mv.normalize_or_zero(),
        aim: Vec2::new(g.x, g.z),
        // Edge OR hold: sub-tick taps must never eat a cast (feel budget).
        cast: [
            eng.input.mouse_down(0) || eng.input.mouse_pressed(0),
            eng.input.mouse_down(1) || eng.input.mouse_pressed(1),
            eng.input.key_down(KeyCode::KeyQ) || eng.input.key_pressed(KeyCode::KeyQ),
            eng.input.key_down(KeyCode::KeyE) || eng.input.key_pressed(KeyCode::KeyE),
            eng.input.key_down(KeyCode::KeyR) || eng.input.key_pressed(KeyCode::KeyR),
            eng.input.key_down(KeyCode::KeyF) || eng.input.key_pressed(KeyCode::KeyF),
        ],
        channel_held: eng.input.mouse_down(0) || eng.input.mouse_down(1),
        dodge: eng.input.key_pressed(KeyCode::Space),
        interact: eng.input.key_pressed(KeyCode::KeyG),
    }
}

pub fn tick_player(eng: &mut Engine, gs: &mut Gs, input: &InputFrame) {
    gs.pc.aim = input.aim;
    for cd in &mut gs.pc.cooldowns {
        *cd = cd.saturating_sub(1);
    }
    gs.pc.dodge_cd = gs.pc.dodge_cd.saturating_sub(1);
    gs.pc.iframes = gs.pc.iframes.saturating_sub(1);
    gs.pc.frenzy = gs.pc.frenzy.saturating_sub(1);
    gs.pc.discorded = gs.pc.discorded.saturating_sub(1);

    // Movement.
    let speed = 6.5
        * (1.0
            + gs.stat(K_SPEED)
            + if gs.pc.frenzy > 0 {
                gs.pc.frenzy_move
            } else {
                0.0
            });
    let moving = input.mv.length_squared() > 0.01;
    if moving {
        gs.pc.still_ticks = 0;
    } else {
        gs.pc.still_ticks = gs.pc.still_ticks.saturating_add(1);
    }
    let (from, to) = {
        let p = eng
            .world
            .get::<Pos>(gs.pc.entity)
            .map(|p| p.0)
            .unwrap_or(Vec2::ZERO);
        (p, p + input.mv * speed * FIXED_DT)
    };
    let to = clamp_walkable(gs, from, to);
    if let Some(p) = eng.world.get_mut::<Pos>(gs.pc.entity) {
        p.0 = to;
    }
    if let Some(pp) = eng.world.get_mut::<PrevPos>(gs.pc.entity) {
        pp.0 = from;
    }

    // Stillness consecrates (Goetic rule): the ground blesses the patient.
    if gs.build.has_rule(&Rule::StillnessConsecrates) && gs.pc.still_ticks == 45 {
        eng.world.spawn((
            Pos(to),
            Zone {
                friendly: true,
                radius: 3.0,
                life: 240,
                tick_interval: 20,
                timer: 1,
                dmg: [0.0, 0.0, 4.0, 0.0],
                apply: vec![],
                consecrate: true,
                inverted: false,
                telegraph_burst: None,
                power: 1.0,
                slot: 0,
            },
        ));
        gs.pc.still_ticks = 0;
    }

    // Dodge: engine-grade escape; BloodDodge removes the cooldown for blood.
    if input.dodge {
        let free_dodge = gs.build.has_rule(&Rule::BloodDodge);
        if gs.pc.dodge_cd == 0 || free_dodge {
            if free_dodge {
                let max = gs.stat(K_MAX_HP);
                if let Some(h) = eng.world.get_mut::<Health>(gs.pc.entity) {
                    h.hp = (h.hp - max * 0.05).max(1.0);
                }
            } else {
                gs.pc.dodge_cd = 60;
            }
            let dir = if moving {
                input.mv
            } else {
                (input.aim - to).normalize_or_zero()
            };
            crate::skills::player_dash(eng, gs, dir, 4.5, &[0.0; 4], &[], 0);
        }
    }

    // Casts (held keys auto-repeat off cooldown — horde game, not typing test).
    for slot in 0..6 {
        if input.cast[slot] {
            crate::skills::cast(eng, gs, slot, 1.0, input.aim, false);
        }
    }
    crate::skills::tick_channel(eng, gs, input.channel_held);

    // Echo/pending casts: bounded drain per tick; the rest carries over.
    let mut budget = 8;
    while budget > 0 {
        let Some((slot, power, aim)) = gs.pc.pending_casts.pop() else {
            break;
        };
        crate::skills::cast(eng, gs, slot, power, aim, true);
        budget -= 1;
    }
    if gs.pc.pending_casts.len() > 64 {
        gs.pc.pending_casts.truncate(64); // even Pillar 1 has a queue limit
    }

    // Regen.
    let regen = gs.stat(K_REGEN);
    if regen > 0.0 {
        heal_player(eng, gs, regen * FIXED_DT);
    }
}

// ------------------------------------------------------------ run orchestra

pub enum RunEvent {
    None,
    PlayerDied,
    ReturnedToCourt,
}

pub fn tick_run(eng: &mut Engine, gs: &mut Gs, rs: &mut RunState) -> RunEvent {
    rs.ticks += 1;
    let input = read_input(eng);

    // Buer's Cycle: realm-wide pulse. Blight = pressure, bloom = fertility.
    if rs.demon == Demon::Buer {
        if gs.build.has_rule(&Rule::LockBlight) {
            gs.blight_phase = true;
        } else {
            rs.cycle_timer += (60.0 * rs.cycle_rate) as u32 / 60;
            let period = 600;
            if rs.cycle_timer >= period {
                rs.cycle_timer = 0;
                gs.blight_phase = !gs.blight_phase;
                let c = if gs.blight_phase {
                    palette::ICHOR
                } else {
                    palette::GOLD
                };
                let pp = eng
                    .world
                    .get::<Pos>(gs.pc.entity)
                    .map(|p| p.0)
                    .unwrap_or(Vec2::ZERO);
                spawn_ring(eng, pp, 12.0, c);
                eng.audio.play(
                    &gs.sounds.ritual,
                    "sfx",
                    0.5,
                    if gs.blight_phase { 0.7 } else { 1.3 },
                );
            }
        }
        // Blight pressure on the player (or nourishment, if pacted).
        if gs.blight_phase && rs.ticks.is_multiple_of(30) {
            if gs.build.has_rule(&Rule::BlightHealsYou) {
                let amt = gs.stat(K_MAX_HP) * 0.01;
                heal_player(eng, gs, amt);
            } else {
                let amt = gs.stat(K_MAX_HP) * 0.005;
                hit_player(eng, gs, amt, DmgType::Void, true);
            }
        }
    } else {
        gs.blight_phase = false;
    }

    tick_player(eng, gs, &input);
    tick_enemies(eng, gs);
    tick_boss(eng, gs);
    crate::skills::tick_projectiles(eng, gs);
    crate::skills::tick_zones(eng, gs);
    crate::skills::tick_minions_totems(eng, gs);
    tick_statuses(eng, gs);
    tick_delayed_hits(eng, gs);
    process_triggers(eng, gs);
    crate::loot::tick_pickup(eng, gs);

    // Corpses age out.
    let mut old = Vec::new();
    eng.world.each::<(&mut CorpseC,)>(|e, (c,)| {
        c.age += 1;
        if c.age >= c.max_age {
            old.push(e);
        }
    });
    for e in old {
        eng.commands.despawn(e);
    }

    // Interactions.
    if input.interact {
        interact(eng, gs, rs);
    }

    // Boss death → clear state, loot shower, exit portal.
    if !rs.cleared && !eng.world.is_alive(rs.boss) {
        rs.cleared = true;
        rs.portal_out = Some(rs.boss_pos);
        crate::loot::boss_loot(eng, gs, rs.boss_pos);
        eng.hitstop(0.25);
        eng.shake(0.9);
        eng.audio.play(&gs.sounds.boss_dead, "sfx", 1.0, 0.8);
        eng.floaters.spawn(
            Vec3::new(rs.boss_pos.x, 2.0, rs.boss_pos.y),
            format!("{} FALLS", rs.demon.name()),
            palette::GOLD.extend(1.0),
            3.0,
        );
    }

    // Camera follows.
    if let Some(p) = eng.world.get::<Pos>(gs.pc.entity) {
        let t = Vec3::new(p.0.x, 0.0, p.0.y);
        eng.camera.target = eng.camera.target.lerp(t, 0.12);
    }

    // Death.
    let dead = eng
        .world
        .get::<Health>(gs.pc.entity)
        .map(|h| h.hp <= 0.0)
        .unwrap_or(false);
    if dead {
        eng.audio.play(&gs.sounds.death, "sfx", 0.9, 1.0);
        eng.shake(1.0);
        return RunEvent::PlayerDied;
    }
    RunEvent::None
}

fn interact(eng: &mut Engine, gs: &mut Gs, rs: &mut RunState) {
    let pp = eng
        .world
        .get::<Pos>(gs.pc.entity)
        .map(|p| p.0)
        .unwrap_or(Vec2::ZERO);

    // Exit portal (after boss) or entry portal: bank and leave.
    let near_exit = rs.portal_out.map(|p| pp.distance(p) < 2.5).unwrap_or(false);
    if near_exit || pp.distance(rs.entry) < 2.0 {
        eng.audio.play(&gs.sounds.portal, "sfx", 0.7, 1.2);
        // Banking happens in game.rs on ReturnedToCourt.
        rs.ticks = u64::MAX; // sentinel consumed by game.rs
        return;
    }

    // Vassago shrines: unveil what is hidden.
    for (spos, used) in &mut rs.shrines {
        if !*used && pp.distance(*spos) < 2.2 {
            *used = true;
            if rs.hostile_shrines {
                // The realm modifier warned you.
                for i in 0..5 {
                    let a = i as f32 * 1.256;
                    let at = *spos + Vec2::new(a.cos(), a.sin()) * 2.0;
                    let realm = gs.db.realm(rs.demon).clone();
                    let id = realm.enemies[i % realm.enemies.len()].clone();
                    spawn_enemy(eng, &gs.db, &id, at, gs.tier, i == 0, 1.0, 1.0, 1.0);
                }
                eng.audio.play(&gs.sounds.curse, "sfx", 0.8, 0.6);
            }
            let mut revealed = 0;
            for item in gs
                .run_inv
                .iter_mut()
                .chain(gs.loadout.equipment.iter_mut().flatten())
            {
                for a in &mut item.affixes {
                    if a.hidden && !a.revealed {
                        a.revealed = true;
                        revealed += 1;
                    }
                }
            }
            gs.recompile();
            spawn_burst(eng, *spos, palette::HEX, 60, 4.0);
            eng.floaters.spawn(
                Vec3::new(spos.x, 1.5, spos.y),
                format!("{revealed} AFFIXES UNVEILED"),
                palette::HEX.extend(1.0),
                2.0,
            );
            eng.audio.play(&gs.sounds.goetic, "sfx", 0.7, 1.3);
            return;
        }
    }

    // Corruption altar: gamble the most recent pickup.
    if pp.distance(rs.altar) < 2.2 {
        if gs.dust < 10 {
            eng.floaters.spawn(
                Vec3::new(rs.altar.x, 1.5, rs.altar.y),
                "NEED 10 DUST",
                palette::ASH.extend(1.0) * 4.0,
                1.6,
            );
            return;
        }
        let Some(mut item) = gs.run_inv.pop() else {
            eng.floaters.spawn(
                Vec3::new(rs.altar.x, 1.5, rs.altar.y),
                "NOTHING TO OFFER",
                palette::ASH.extend(1.0) * 4.0,
                1.6,
            );
            return;
        };
        gs.dust -= 10;
        let mut crng = eng.streams.get("corrupt").clone();
        let mut nrng = eng.streams.get("naming").clone();
        let outcome =
            crate::items::corrupt_item(&gs.db, &mut gs.loot, &mut crng, &mut nrng, &mut item);
        *eng.streams.get("corrupt") = crng;
        *eng.streams.get("naming") = nrng;
        eng.audio.play(&gs.sounds.corrupt, "sfx", 0.8, 1.0);
        let (txt, color) = match outcome {
            crate::items::CorruptOutcome::Bricked => {
                spawn_burst(eng, rs.altar, palette::VOID * 6.0, 40, 5.0);
                ("DEVOURED".to_string(), palette::BLOOD)
            }
            crate::items::CorruptOutcome::Rerolled => {
                gs.run_inv.push(item);
                ("REWRITTEN".to_string(), palette::HEX)
            }
            crate::items::CorruptOutcome::CorruptAffix => {
                gs.run_inv.push(item);
                ("MARKED".to_string(), palette::HEX)
            }
            crate::items::CorruptOutcome::Awakened => {
                let name = item.name.clone();
                spawn_burst(eng, rs.altar, palette::BRIMSTONE, 120, 7.0);
                eng.hitstop(0.12);
                eng.shake(0.5);
                eng.audio.play(&gs.sounds.goetic, "sfx", 1.0, 0.8);
                gs.run_inv.push(item);
                (format!("AWAKENED: {name}"), palette::BRIMSTONE)
            }
        };
        eng.floaters.spawn(
            Vec3::new(rs.altar.x, 1.8, rs.altar.y),
            txt,
            color.extend(1.0),
            2.2,
        );
    }
}
