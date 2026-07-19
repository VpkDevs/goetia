//! Combat core: components, the damage pipeline, status semantics, and the
//! reaction processor that turns trigger-bus events into gameplay.
//!
//! Everything speaks the global vocabulary. There are deliberately NO
//! "X can't proc Y" exceptions anywhere in this file — loops are damped by
//! the engine's trigger budget, not forbidden (Pillar 1).

use crate::content::*;
use crate::fx::Sounds;
use crate::items::*;
use crate::vocab::*;
use goetia::prelude::*;

// -------------------------------------------------------------- components

#[derive(Clone, Copy)]
pub struct Pos(pub Vec2);
#[derive(Clone, Copy)]
pub struct PrevPos(pub Vec2);
#[derive(Clone, Copy)]
pub struct Vel(pub Vec2);

#[derive(Clone, Copy)]
pub struct Health {
    pub hp: f32,
    pub max: f32,
    /// Kill/hit flash 0..1, decays render-side; sim writes 1.0 on hit.
    pub flash: f32,
}

impl Health {
    pub fn new(max: f32) -> Health {
        Health { hp: max, max, flash: 0.0 }
    }
}

pub struct PlayerTag;

#[derive(Clone, Copy, PartialEq)]
pub enum AiState {
    Idle,
    Seek,
    Strafe,
    Telegraph,
    Lunge,
    Cast,
}

#[derive(Clone)]
pub struct EnemyC {
    pub def_id: String,
    pub state: AiState,
    pub timer: u16,
    pub phase: f32,
    pub elite: bool,
    pub hp_mul: f32,
    pub dmg_mul: f32,
    pub speed_mul: f32,
    pub attack_cd: u16,
    /// Wheel AI orbit angle.
    pub orbit: f32,
}

#[derive(Clone, Copy, PartialEq)]
pub enum BossKind {
    Vassago,
    Andras,
    Buer,
}

#[derive(Clone, Copy)]
pub struct BossC {
    pub kind: BossKind,
    pub timer: u16,
    pub phase: u8,
}

/// A projectile (player or enemy).
#[derive(Clone)]
pub struct Proj {
    pub friendly: bool,
    pub dmg: DmgVec,
    pub radius: f32,
    pub life: u16,
    pub pierce: u32,
    pub apply: Vec<StatusApply>,
    /// Casting skill slot (player) — echoes and resets need it.
    pub slot: usize,
    pub last_hit: Entity,
    /// Orbiting projectiles circle the player instead of flying.
    pub orbit: Option<f32>, // angle
    pub glow: bool,
    pub power: f32,
}

/// Ground zone (ritual circles, consecration, boss telegraphs).
#[derive(Clone)]
pub struct Zone {
    pub friendly: bool,
    pub radius: f32,
    pub life: u16,
    pub tick_interval: u16,
    pub timer: u16,
    pub dmg: DmgVec,
    pub apply: Vec<StatusApply>,
    /// Consecrate: heals the aligned side instead of damaging it.
    pub consecrate: bool,
    /// Inverted consecration flips who it loves.
    pub inverted: bool,
    /// Telegraph zones deal one burst when `life` hits 0 instead of ticking.
    pub telegraph_burst: Option<DmgVec>,
    pub power: f32,
    pub slot: usize,
}

#[derive(Clone)]
pub struct MinionC {
    pub life: u16,
    pub attack_cd: u16,
    pub timer: u16,
    pub dmg: DmgVec,
    pub speed: f32,
    pub slot: usize,
    pub power: f32,
}

#[derive(Clone)]
pub struct TotemC {
    pub life: u16,
    pub fire_cd: u16,
    pub timer: u16,
    pub dmg: DmgVec,
    pub proj_speed: f32,
    pub slot: usize,
    pub power: f32,
    pub apply: Vec<StatusApply>,
}

#[derive(Clone, Copy)]
pub struct CorpseC {
    pub age: u16,
    pub max_age: u16,
    pub scale: f32,
    pub tint: Vec3,
}

/// An item (or dust pile) on the ground.
pub struct LootDrop {
    pub item: Option<ItemInstance>,
    pub dust: u32,
    pub age: u16,
}

/// Delayed damage packets (the "all damage twice, one second apart" Goetic).
pub struct DelayedHit {
    pub target: Entity,
    pub dmg: DmgVec,
    pub delay: u16,
    pub at: Vec2,
}

// ----------------------------------------------------------------- context

/// Player-adjacent state threaded through combat systems.
pub struct PlayerCtx {
    pub entity: Entity,
    pub aim: Vec2,           // ground cursor
    pub cast_count: u64,
    pub last_cast: Option<(usize, Vec2)>,
    /// Deferred casts queued by Echo/FreeReset reactions.
    pub pending_casts: Vec<(usize, f32, Vec2)>,
    pub cooldowns: [u16; 6],
    pub dodge_cd: u16,
    pub iframes: u16,
    pub frenzy: u16,
    pub frenzy_cast: f32,
    pub frenzy_move: f32,
    pub low_life_armed: bool,
    pub channel: Option<usize>, // beam slot being held
    pub channel_tick: u16,
    pub still_ticks: u16,
    pub discorded: u16, // ticks remaining (Andras contract)
    pub kills_this_run: u64,
}

impl PlayerCtx {
    pub fn new(entity: Entity) -> PlayerCtx {
        PlayerCtx {
            entity,
            aim: Vec2::ZERO,
            cast_count: 0,
            last_cast: None,
            pending_casts: Vec::new(),
            cooldowns: [0; 6],
            dodge_cd: 0,
            iframes: 0,
            frenzy: 0,
            frenzy_cast: 0.0,
            frenzy_move: 0.0,
            low_life_armed: true,
            channel: None,
            channel_tick: 0,
            still_ticks: 0,
            discorded: 0,
            kills_this_run: 0,
        }
    }
}

/// Everything combat needs besides the engine. One bag, passed explicitly.
pub struct Gs {
    pub db: ContentDb,
    pub status_reg: StatusRegistry,
    pub loadout: Loadout,
    pub build: CompiledBuild,
    pub bank: Vec<ItemInstance>,
    pub run_inv: Vec<ItemInstance>,
    pub dust: u64,
    pub loot: LootTables,
    pub sounds: Sounds,
    pub pc: PlayerCtx,
    /// Realm-mod loot multiplier for the current run.
    pub loot_mul: f32,
    pub reveal_on_drop: bool,
    pub death_novas: bool,
    /// Andras boss: reflected trigger echo queue (kind, at).
    pub boss_reflect: Vec<(TriggerKind, Vec2)>,
    pub last_player_trigger: Option<TriggerKind>,
    /// Buer cycle: true = blight phase.
    pub blight_phase: bool,
    pub tier: u32,
    /// Walkable cell set for the current realm (None = open ground/court).
    pub walkable: Option<std::collections::HashSet<(i32, i32)>>,
}

impl Gs {
    pub fn recompile(&mut self) {
        self.build = compile_build(&self.db, &self.loadout);
    }

    pub fn stat(&mut self, k: StatKey) -> f32 {
        self.build.sheet.get(k)
    }

    pub fn dmg_mult(&mut self, t: DmgType) -> f32 {
        (1.0 + self.stat(K_DMG) + self.stat(dmg_key(t))).max(0.0)
    }
}

// ------------------------------------------------------------ damage: out

pub struct HitResult {
    pub killed: bool,
    pub crit: bool,
    pub total: f32,
}

/// The one path by which the player damages an enemy. Every proc, minion,
/// echo and altar-born horror goes through here.
#[allow(clippy::too_many_arguments)]
pub fn hit_enemy(
    eng: &mut Engine,
    gs: &mut Gs,
    target: Entity,
    at: Vec2,
    base: DmgVec,
    applies: &[StatusApply],
    can_crit: bool,
    from_delayed: bool,
) -> HitResult {
    let mut out = HitResult { killed: false, crit: false, total: 0.0 };
    if !eng.world.is_alive(target) {
        return out;
    }
    // BUER is a wheel: only the blight phase can cut him (kill the wheel in
    // the correct phase — realm mechanics beat raw damage).
    if crate::enemies::buer_immune(eng, gs, target) {
        if eng.clock.tick % 30 == 0 {
            eng.floaters.spawn(Vec3::new(at.x, 1.5, at.y), "IMMUNE", palette::ICHOR.extend(0.9), 1.4);
        }
        return out;
    }

    // Scale by build multipliers per type; rules may convert first.
    let mut dmg = base;
    if gs.build.has_rule(&Rule::AllHellfire) {
        let total: f32 = dmg.iter().sum();
        dmg = [0.0, total, 0.0, 0.0];
    }
    for t in DMG_TYPES {
        dmg[t.index()] *= gs.dmg_mult(t);
    }
    // Andras contract: discorded players hit harder.
    if gs.pc.discorded > 0 {
        if let Some(p) = gs.build.discord_power() {
            for d in &mut dmg {
                *d *= 1.0 + p;
            }
        }
    }

    // Crit
    let crit_chance = gs.stat(K_CRIT);
    let crit = can_crit && {
        let r = eng.streams.get("combat").next_f32();
        r < crit_chance
    };
    if crit {
        let mult = 1.0 + 1.0 + gs.stat(K_CRIT_MULT); // base 2x + bonus
        for d in &mut dmg {
            *d *= mult;
        }
    }

    // Hex-mark: consumed by the next non-physical hit, amplifying it.
    let nonphys: f32 = dmg[1] + dmg[2] + dmg[3];
    if nonphys > 0.0 {
        if let Some(bag) = eng.world.get_mut::<StatusBag>(target) {
            let mut evs = Vec::new();
            if let Some((stacks, mag)) = bag.detonate(target, ST_HEXMARK, &mut evs) {
                let amp = 1.5 + 0.25 * stacks as f32 + mag * 0.01;
                dmg[1] *= amp;
                dmg[2] *= amp;
                dmg[3] *= amp;
                eng.triggers.emit(TR_STATUS_DETONATE, gs.pc.entity, target, stacks as f32);
                gs.last_player_trigger = Some(TR_STATUS_DETONATE);
            }
        }
    }

    // Petrify-crack: physical hits shatter the stone for bonus physical.
    if dmg[0] > 0.0 {
        if let Some(bag) = eng.world.get_mut::<StatusBag>(target) {
            let mut evs = Vec::new();
            if let Some((stacks, _)) = bag.detonate(target, ST_PETRIFY, &mut evs) {
                dmg[0] *= 1.6 + 0.2 * stacks as f32;
                eng.triggers.emit(TR_STATUS_DETONATE, gs.pc.entity, target, stacks as f32);
                spawn_burst(eng, at, palette::ASH * 3.0, 20, 3.0);
            }
        }
    }

    let total = dmg_total(&dmg);
    out.total = total;
    out.crit = crit;

    // Status application from the hit.
    let status_bonus = gs.stat(K_STATUS_CHANCE);
    for ap in applies {
        let chance = (ap.chance * (1.0 + status_bonus)).min(1.0);
        let roll = eng.streams.get("combat").next_f32();
        if roll < chance {
            apply_status_to(eng, gs, target, &ap.status, ap.stacks, total * ap.magnitude);
        }
    }
    if crit && gs.build.has_rule(&Rule::CritsPetrify) {
        apply_status_to(eng, gs, target, "petrify", 1, 0.0);
    }

    // The wound itself.
    let mut killed = false;
    if let Some(h) = eng.world.get_mut::<Health>(target) {
        h.hp -= total;
        h.flash = 1.0;
        killed = h.hp <= 0.0;
    }
    // Buer boss is phase-immune — checked before we got here; belt-and-braces
    // handled by caller zeroing dmg.

    let dt = dominant_type(&dmg);
    let scale = if crit { 2.2 } else { 1.5 };
    let txt = if crit { format!("{:.0}!", total) } else { format!("{:.0}", total) };
    eng.floaters.spawn(Vec3::new(at.x, 0.6, at.y), txt, dt.color().extend(1.0), scale);
    if crit {
        eng.triggers.emit(TR_CRIT, gs.pc.entity, target, total);
        gs.last_player_trigger = Some(TR_CRIT);
        eng.shake(0.08);
        eng.audio.play(&gs.sounds.crit, "sfx", 0.35, 1.2);
    }

    // The delayed-double Goetic. Delayed hits don't re-schedule (that would be
    // 2^n, and even Pillar 1 has a memory budget) — but everything else stacks.
    if !from_delayed && gs.build.has_rule(&Rule::DoubleDamageDelayed) {
        eng.world.spawn((DelayedHit { target, dmg: base, delay: 60, at },));
    }

    if killed {
        kill_enemy(eng, gs, target, at, total);
        out.killed = true;
    } else {
        eng.audio.play(&gs.sounds.hit, "sfx", 0.12, 0.9 + (total % 7.0) * 0.03);
    }
    out
}

/// Death: triggers, ignite spread, loot, corpse, juice. In that order.
pub fn kill_enemy(eng: &mut Engine, gs: &mut Gs, target: Entity, at: Vec2, overkill: f32) {
    gs.pc.kills_this_run += 1;

    // Ignite spreads on death — that IS ignite's identity.
    let (had_ignite, ig_stacks, ig_mag) = eng
        .world
        .get::<StatusBag>(target)
        .and_then(|b| b.get(ST_IGNITE).map(|s| (true, s.stacks, s.magnitude)))
        .unwrap_or((false, 0, 0.0));
    if had_ignite {
        let spread_r = 4.0;
        let mut near = Vec::new();
        eng.world.resource::<SpatialGrid>().query_radius(at, spread_r, MASK_ENEMY, &mut near);
        for (e, _) in near.into_iter().take(6) {
            if e != target {
                apply_status_to(eng, gs, e, "ignite", ig_stacks, ig_mag);
            }
        }
        spawn_burst(eng, at, palette::BRIMSTONE, 30, 5.0);
    }

    // Elite/normal juice scale.
    let elite = eng.world.get::<EnemyC>(target).map(|e| e.elite).unwrap_or(false);
    let (scale, tint) = eng
        .world
        .get::<EnemyC>(target)
        .map(|e| (if e.elite { 1.6 } else { 1.0 }, palette::BLOOD))
        .unwrap_or((1.0, palette::BLOOD));

    // Corpse + gibs.
    if let Some(p) = eng.world.get::<Pos>(target) {
        let p = *p;
        eng.world.spawn((
            Pos(p.0),
            CorpseC { age: 0, max_age: 240, scale, tint },
        ));
    }
    spawn_burst(eng, at, palette::BLOOD, if elite { 80 } else { 26 }, if elite { 7.0 } else { 4.5 });
    eng.hitstop(if elite { 0.05 } else { 0.018 });
    eng.shake(if elite { 0.25 } else { 0.06 });
    eng.audio.play(&gs.sounds.kill, "sfx", if elite { 0.7 } else { 0.3 }, if elite { 0.7 } else { 1.0 });
    let _ = overkill;

    // Volatile dead (realm modifier): corpses detonate against YOU.
    if gs.death_novas {
        let pp = eng.world.get::<Pos>(gs.pc.entity).map(|p| p.0).unwrap_or(Vec2::ZERO);
        spawn_ring(eng, at, 2.6, palette::BRIMSTONE);
        if pp.distance(at) < 2.6 {
            hit_player(eng, gs, 6.0 + gs.tier as f32 * 2.0, DmgType::Hellfire, false);
        }
    }

    // Loot.
    crate::loot::drop_loot(eng, gs, at, elite);

    // Triggers last — the kill is real before reactions cascade off it.
    eng.triggers.emit(TR_KILL, gs.pc.entity, target, 1.0);
    gs.last_player_trigger = Some(TR_KILL);

    eng.commands.despawn(target);
}

/// Apply a named status to any entity, emitting the on-status-apply trigger.
pub fn apply_status_to(
    eng: &mut Engine,
    gs: &mut Gs,
    target: Entity,
    status: &str,
    stacks: u32,
    magnitude: f32,
) {
    let id = status_by_name(status);
    let Some(def) = gs.status_reg.get(id).cloned() else { return };
    let mut dur_mult = 1u32;
    if id == ST_IGNITE && gs.build.has_rule(&Rule::EternalIgnite) {
        dur_mult = 10;
    }
    let mut evs = Vec::new();
    if let Some(bag) = eng.world.get_mut::<StatusBag>(target) {
        let mut d = def.clone();
        d.duration_ticks *= dur_mult;
        bag.apply(target, &d, stacks, magnitude, &mut evs);
        eng.triggers.emit(TR_STATUS_APPLY, gs.pc.entity, target, stacks as f32);
        gs.last_player_trigger = Some(TR_STATUS_APPLY);
        // Blight detonates at cap: instant burst of accumulated rot.
        if id == ST_BLIGHT {
            let at_cap = eng
                .world
                .get::<StatusBag>(target)
                .map(|b| b.stacks(ST_BLIGHT) >= def.max_stacks)
                .unwrap_or(false);
            if at_cap {
                let mut evs2 = Vec::new();
                if let Some(bag) = eng.world.get_mut::<StatusBag>(target) {
                    if let Some((s, m)) = bag.detonate(target, ST_BLIGHT, &mut evs2) {
                        let burst = 8.0 + m * 1.5 * s as f32;
                        let pos = eng.world.get::<Pos>(target).map(|p| p.0).unwrap_or(Vec2::ZERO);
                        eng.triggers.emit(TR_STATUS_DETONATE, gs.pc.entity, target, s as f32);
                        spawn_burst(eng, pos, palette::ICHOR, 40, 5.0);
                        hit_enemy(
                            eng,
                            gs,
                            target,
                            pos,
                            [0.0, 0.0, 0.0, burst],
                            &[],
                            false,
                            false,
                        );
                    }
                }
            }
        }
    }
}

// ------------------------------------------------------------- damage: in

/// The one path by which anything damages the player.
pub fn hit_player(eng: &mut Engine, gs: &mut Gs, raw: f32, dtype: DmgType, from_self_proc: bool) {
    if gs.pc.iframes > 0 && !from_self_proc {
        return;
    }
    let reduction = if dtype == DmgType::Physical { gs.stat(K_ARMOR) } else { gs.stat(K_RESIST) };
    let mut amount = raw * (1.0 - reduction.clamp(0.0, 0.75));

    // Consecrated ground: harm flips to healing on aligned ground.
    if player_on_consecrate(eng, gs) {
        let heal = amount * 0.5;
        heal_player(eng, gs, heal);
        return;
    }
    let max = gs.stat(K_MAX_HP);
    if amount <= 0.0 {
        return;
    }
    if let Some(h) = eng.world.get_mut::<Health>(gs.pc.entity) {
        h.hp -= amount;
        h.flash = 1.0;
        amount = amount.min(h.max);
        // Post-hit grace: half a second where the crowd can't stunlock you.
        if !from_self_proc {
            gs.pc.iframes = gs.pc.iframes.max(28);
        }
        let frac = h.hp / h.max;
        eng.shake((amount / max).clamp(0.06, 0.4));
        eng.audio.play(&gs.sounds.hurt, "sfx", 0.5, 1.0);
        if frac < 0.35 && gs.pc.low_life_armed {
            gs.pc.low_life_armed = false;
            eng.triggers.emit(TR_LOW_LIFE, gs.pc.entity, gs.pc.entity, frac);
            gs.last_player_trigger = Some(TR_LOW_LIFE);
        }
        if frac > 0.5 {
            gs.pc.low_life_armed = true;
        }
    }
}

pub fn heal_player(eng: &mut Engine, gs: &mut Gs, amount: f32) {
    let blight_tax = if gs.blight_phase && !gs.build.has_rule(&Rule::BlightHealsYou) { 0.5 } else { 1.0 };
    if let Some(h) = eng.world.get_mut::<Health>(gs.pc.entity) {
        h.hp = (h.hp + amount * blight_tax).min(h.max);
    }
}

fn player_on_consecrate(eng: &mut Engine, gs: &Gs) -> bool {
    let Some(pp) = eng.world.get::<Pos>(gs.pc.entity).copied() else { return false };
    let mut found = false;
    eng.world.each::<(&Pos, &Zone)>(|_, (zp, z)| {
        if z.consecrate && !z.inverted && zp.0.distance(pp.0) < z.radius {
            found = true;
        }
    });
    found
}

// -------------------------------------------------------- status tick pass

/// Once per tick: run bag lifecycles and translate events into gameplay.
pub fn tick_statuses(eng: &mut Engine, gs: &mut Gs) {
    let reg = gs.status_reg.clone();
    let mut events: Vec<StatusEvent> = Vec::new();
    eng.world.each::<(&mut StatusBag,)>(|ent, (bag,)| {
        bag.tick(ent, &reg, &mut events);
    });

    for ev in events {
        match ev {
            StatusEvent::Ticked { entity, id, stacks, magnitude } => {
                if id == ST_IGNITE {
                    let at = eng.world.get::<Pos>(entity).map(|p| p.0).unwrap_or(Vec2::ZERO);
                    let is_player = entity == gs.pc.entity;
                    if is_player {
                        hit_player(eng, gs, magnitude.max(1.0), DmgType::Hellfire, true);
                    } else {
                        // DoT damage does not re-crit; it can still kill/spread.
                        hit_enemy(
                            eng,
                            gs,
                            entity,
                            at,
                            [0.0, (magnitude * (1.0 + stacks as f32 * 0.5)).max(1.0), 0.0, 0.0],
                            &[],
                            false,
                            true, // suppress delayed-double on DoTs
                        );
                    }
                } else if id == ST_BLIGHT {
                    let at = eng.world.get::<Pos>(entity).map(|p| p.0).unwrap_or(Vec2::ZERO);
                    if entity != gs.pc.entity {
                        hit_enemy(
                            eng,
                            gs,
                            entity,
                            at,
                            [0.0, 0.0, 0.0, (magnitude * 0.6 + stacks as f32).max(1.0)],
                            &[],
                            false,
                            true,
                        );
                    } else if gs.build.has_rule(&Rule::BlightHealsYou) {
                        heal_player(eng, gs, magnitude.max(1.0));
                    } else {
                        hit_player(eng, gs, magnitude.max(1.0), DmgType::Void, true);
                    }
                }
            }
            StatusEvent::Expired { .. } | StatusEvent::Applied { .. } => {}
            StatusEvent::Detonated { .. } => {}
        }
    }
}

// ------------------------------------------------------------- trigger pass

/// Once per tick: drain the trigger bus through the player's reaction table.
/// This is where builds become engines.
pub fn process_triggers(eng: &mut Engine, gs: &mut Gs) {
    // Snapshot data the handler needs (the closure can't borrow gs mutably
    // while the bus is also borrowed from eng — take the bus out instead).
    let mut bus = std::mem::take(&mut eng.triggers);
    let reactions = gs.build.reactions.clone();
    let procs_self = gs.build.has_rule(&Rule::ProcsTargetSelf) && gs.pc.discorded > 0;
    let player = gs.pc.entity;

    // Actions are collected and executed after processing: they need
    // &mut Engine which the process() closure holds.
    struct Queued {
        action: Action,
        target: Entity,
        magnitude: f32,
    }
    let mut queued: Vec<Queued> = Vec::new();
    let mut rng = eng.streams.get("proc").clone();

    bus.process(|ev, em| {
        for r in &reactions {
            if trigger_by_name(&r.on) != ev.kind {
                continue;
            }
            if r.chance < 1.0 && rng.next_f32() >= r.chance {
                continue;
            }
            // Note: reaction *consequences* (status applies, kills, crits)
            // re-enter the bus as fresh emissions from the damage pipeline;
            // the per-tick budget is the only damper. No exceptions (Pillar 1).
            let _ = &em;
            let target = if procs_self && rng.chance(0.25) { player } else { ev.target };
            queued.push(Queued { action: r.action.clone(), target, magnitude: ev.magnitude });
        }
    });
    *eng.streams.get("proc") = rng;
    eng.triggers = bus;

    // Execute queued actions.
    for q in queued {
        let at = eng
            .world
            .get::<Pos>(q.target)
            .map(|p| p.0)
            .or_else(|| eng.world.get::<Pos>(player).map(|p| p.0))
            .unwrap_or(Vec2::ZERO);
        match q.action {
            Action::Nova { pct, dtype, radius } => {
                let base = weapon_power(gs) * pct;
                let mut dmg = [0.0; 4];
                dmg[dtype.index()] = base;
                nova_at(eng, gs, at, radius, dmg, &[], q.target == player);
            }
            Action::ApplyStatus { status, stacks, magnitude, radius } => {
                if q.target == player {
                    // Self-targeted proc: statuses land on YOU (Andras pact).
                    apply_status_to(eng, gs, player, &status, stacks, magnitude * 10.0);
                } else if radius > 0.5 {
                    let mut near = Vec::new();
                    eng.world
                        .resource::<SpatialGrid>()
                        .query_radius(at, radius, MASK_ENEMY, &mut near);
                    for (e, _) in near {
                        apply_status_to(eng, gs, e, &status, stacks, magnitude * 10.0);
                    }
                } else {
                    apply_status_to(eng, gs, q.target, &status, stacks, magnitude * 10.0);
                }
            }
            Action::Echo { pct } => {
                if let Some((slot, aim)) = gs.pc.last_cast {
                    gs.pc.pending_casts.push((slot, pct, aim));
                }
            }
            Action::FreeReset => {
                if let Some((slot, _)) = gs.pc.last_cast {
                    gs.pc.cooldowns[slot] = 0;
                }
            }
            Action::Heal { pct_max } => {
                let amt = gs.stat(K_MAX_HP) * pct_max;
                heal_player(eng, gs, amt);
            }
            Action::SpreadStatus { status, radius } => {
                let id = status_by_name(&status);
                let stacks = eng
                    .world
                    .get::<StatusBag>(q.target)
                    .map(|b| b.stacks(id))
                    .unwrap_or(0);
                if stacks > 0 {
                    let mut near = Vec::new();
                    eng.world
                        .resource::<SpatialGrid>()
                        .query_radius(at, radius, MASK_ENEMY, &mut near);
                    for (e, _) in near.into_iter().take(8) {
                        apply_status_to(eng, gs, e, &status, stacks, q.magnitude);
                    }
                }
            }
            Action::Detonate { status } => {
                let id = status_by_name(&status);
                let mut evs = Vec::new();
                let popped = eng
                    .world
                    .get_mut::<StatusBag>(q.target)
                    .and_then(|b| b.detonate(q.target, id, &mut evs));
                if let Some((stacks, mag)) = popped {
                    eng.triggers.emit(TR_STATUS_DETONATE, player, q.target, stacks as f32);
                    let burst = weapon_power(gs) * 0.6 + mag * stacks as f32;
                    let mut dmg = [0.0; 4];
                    dmg[DmgType::Hex.index()] = burst;
                    nova_at(eng, gs, at, 3.5, dmg, &[], false);
                }
            }
            Action::Frenzy { ticks, cast_speed, move_speed } => {
                gs.pc.frenzy = gs.pc.frenzy.max(ticks as u16);
                gs.pc.frenzy_cast = cast_speed;
                gs.pc.frenzy_move = move_speed;
            }
            Action::Dust { amount } => {
                gs.dust += amount as u64;
            }
        }
    }

    // Andras boss reflection: replay the player's last trigger as danger.
    for (kind, at) in std::mem::take(&mut gs.boss_reflect) {
        let _ = kind;
        // Telegraphed hostile burst where the player was standing.
        eng.world.spawn((
            Pos(at),
            Zone {
                friendly: false,
                radius: 2.6,
                life: 40,
                tick_interval: 0,
                timer: 0,
                dmg: [0.0; 4],
                apply: vec![],
                consecrate: false,
                inverted: false,
                telegraph_burst: Some([0.0, 0.0, 14.0 + gs.tier as f32 * 3.0, 0.0]),
                power: 1.0,
                slot: 0,
            },
        ));
    }
}

/// Baseline "weapon power" for procs: scales with tier so reactions stay
/// relevant, plus global damage stats applied in hit_enemy.
pub fn weapon_power(gs: &mut Gs) -> f32 {
    10.0 + gs.tier as f32 * 4.0
}

/// Radial damage helper used by novas, detonations, death-novas.
pub fn nova_at(
    eng: &mut Engine,
    gs: &mut Gs,
    at: Vec2,
    radius: f32,
    dmg: DmgVec,
    applies: &[StatusApply],
    hits_player: bool,
) {
    let r = radius * (1.0 + gs.stat(K_AOE));
    let mut near = Vec::new();
    eng.world.resource::<SpatialGrid>().query_radius(at, r, MASK_ENEMY, &mut near);
    for (e, ep) in near {
        hit_enemy(eng, gs, e, ep, dmg, applies, true, false);
    }
    if hits_player {
        let pp = eng.world.get::<Pos>(gs.pc.entity).map(|p| p.0).unwrap_or(Vec2::ZERO);
        if pp.distance(at) < r {
            hit_player(eng, gs, dmg_total(&dmg) * 0.5, dominant_type(&dmg), true);
        }
    }
    let c = dominant_type(&dmg).color();
    spawn_ring(eng, at, r, c);
    eng.audio.play(&gs.sounds.nova, "sfx", 0.3, 1.0);
}

// ------------------------------------------------------------- fx helpers

pub fn spawn_burst(eng: &mut Engine, at: Vec2, color: Vec3, count: u32, spread: f32) {
    eng.world.resource_mut::<crate::fx::FxQueue>().bursts.push((at, color, count, spread));
}

pub fn spawn_ring(eng: &mut Engine, at: Vec2, radius: f32, color: Vec3) {
    eng.world.resource_mut::<crate::fx::FxQueue>().rings.push((at, radius, color));
}

// ------------------------------------------------------- delayed hit system

pub fn tick_delayed_hits(eng: &mut Engine, gs: &mut Gs) {
    let mut due: Vec<(Entity, Entity, DmgVec, Vec2)> = Vec::new();
    eng.world.each::<(&mut DelayedHit,)>(|ent, (d,)| {
        if d.delay == 0 {
            due.push((ent, d.target, d.dmg, d.at));
        } else {
            d.delay -= 1;
        }
    });
    for (ent, target, dmg, at) in due {
        eng.commands.despawn(ent);
        if eng.world.is_alive(target) {
            let at2 = eng.world.get::<Pos>(target).map(|p| p.0).unwrap_or(at);
            hit_enemy(eng, gs, target, at2, dmg, &[], true, true);
        }
    }
}
