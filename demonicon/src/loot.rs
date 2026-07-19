//! Loot: drops, ground items, pickup, fanfare. Cadence targets: a drop per
//! pack, something worth reading every few minutes, and the occasional
//! stop-and-think item — pity ramps live in LootTables.

use crate::combat::*;
use crate::items::*;
use crate::vocab::*;
use goetia::prelude::*;

pub fn drop_loot(eng: &mut Engine, gs: &mut Gs, at: Vec2, elite: bool) {
    let quant = 1.0 + gs.stat(K_LOOT_QUANT);
    let rare_bonus = gs.stat(K_LOOT_RARE) + if elite { 0.6 } else { 0.0 };
    let bloom_bonus = if !gs.blight_phase { 1.25 } else { 1.0 }; // bloom = fertility
    let mut chance = 0.16 * quant * gs.loot_mul * bloom_bonus * if elite { 6.0 } else { 1.0 };
    let mut rng = eng.streams.get("loot").clone();
    while chance > 0.0 {
        if rng.next_f32() < chance.min(1.0) {
            let rarity = roll_rarity(&mut gs.loot, &mut rng, rare_bonus);
            if rarity == Rarity::Common {
                // Commons auto-shatter: dust on the ground, zero inventory sludge.
                eng.world.spawn((
                    Pos(at + Vec2::new(rng.range_f32(-0.8, 0.8), rng.range_f32(-0.8, 0.8))),
                    LootDrop { item: None, dust: 1 + rng.range_u32(3), age: 0 },
                ));
            } else {
                let item = gen_item(&gs.db, &mut gs.loot, &mut rng, rarity, gs.tier, gs.reveal_on_drop);
                let auto_appraise = gs.build.has_rule(&crate::content::Rule::AppraiseOnPickup);
                let mut item = item;
                if auto_appraise {
                    for a in &mut item.affixes {
                        a.revealed = true;
                    }
                }
                eng.world.spawn((
                    Pos(at + Vec2::new(rng.range_f32(-1.0, 1.0), rng.range_f32(-1.0, 1.0))),
                    LootDrop { item: Some(item), dust: 0, age: 0 },
                ));
            }
        }
        chance -= 1.0;
    }
    *eng.streams.get("loot") = rng;
}

/// Boss chest: a shower, guaranteed rare+, goetic pity heavily nudged.
pub fn boss_loot(eng: &mut Engine, gs: &mut Gs, at: Vec2) {
    let mut rng = eng.streams.get("loot").clone();
    let n = 5 + (gs.stat(K_LOOT_QUANT) * 3.0) as u32;
    for i in 0..n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        let p = at + Vec2::new(a.cos(), a.sin()) * rng.range_f32(1.0, 3.0);
        gs.loot.rare_misses += 6; // the kill earns pity
        let rarity = roll_rarity(&mut gs.loot, &mut rng, 1.2).max(Rarity::Magic);
        let item = gen_item(&gs.db, &mut gs.loot, &mut rng, rarity, gs.tier, gs.reveal_on_drop);
        eng.world.spawn((Pos(p), LootDrop { item: Some(item), dust: 0, age: 0 }));
    }
    // And a dust cascade.
    for _ in 0..6 {
        let p = at + Vec2::new(rng.range_f32(-3.0, 3.0), rng.range_f32(-3.0, 3.0));
        eng.world.spawn((Pos(p), LootDrop { item: None, dust: 4 + rng.range_u32(6), age: 0 }));
    }
    *eng.streams.get("loot") = rng;
}

/// Walk-over pickup + loot gravity + the fanfare ladder.
pub fn tick_pickup(eng: &mut Engine, gs: &mut Gs) {
    let ppos = eng.world.get::<Pos>(gs.pc.entity).map(|p| p.0).unwrap_or(Vec2::ZERO);
    let gravity = gs.build.has_rule(&crate::content::Rule::LootGravity);
    let mut picked: Vec<(Entity, Option<ItemInstance>, u32, Vec2)> = Vec::new();
    eng.world.each::<(&mut Pos, &mut LootDrop)>(|e, (p, l)| {
        l.age = l.age.saturating_add(1);
        let d = ppos.distance(p.0);
        if gravity && d < 8.0 && d > 1.0 {
            p.0 += (ppos - p.0).normalize_or_zero() * 6.0 * FIXED_DT;
        }
        if d < 1.5 {
            picked.push((e, l.item.take(), l.dust, p.0));
        }
    });
    for (e, item, dust, at) in picked {
        eng.commands.despawn(e);
        if dust > 0 {
            gs.dust += dust as u64;
            eng.audio.play(&gs.sounds.dust, "sfx", 0.15, 1.5);
        }
        if let Some(item) = item {
            let (count, pitch, vol, shake) = match item.rarity {
                Rarity::Common => (6, 1.2, 0.2, 0.0),
                Rarity::Magic => (14, 1.0, 0.3, 0.0),
                Rarity::Rare => (30, 0.9, 0.5, 0.08),
                Rarity::Goetic => (80, 0.7, 0.8, 0.25),
            };
            spawn_burst(eng, at, item.rarity.color(), count, 3.0);
            eng.audio.play(&gs.sounds.loot, "sfx", vol, pitch);
            if shake > 0.0 {
                eng.shake(shake);
                eng.hitstop(0.03);
            }
            if item.rarity == Rarity::Goetic {
                eng.floaters.spawn(
                    Vec3::new(at.x, 1.0, at.y),
                    item.name.clone(),
                    item.rarity.color().extend(1.0),
                    2.2,
                );
                eng.audio.play(&gs.sounds.goetic, "sfx", 0.9, 1.0);
            }
            eng.triggers.emit(TR_LOOT, gs.pc.entity, gs.pc.entity, 1.0);
            gs.last_player_trigger = Some(TR_LOOT);
            gs.run_inv.push(item);
        }
    }
}
