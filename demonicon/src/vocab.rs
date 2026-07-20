//! The global vocabulary (Pillar 2). Damage types, statuses, triggers, stat
//! keys. Nothing in here belongs to any one skill or item — every mechanic
//! speaks these words and only these words.

use goetia::prelude::*;
use serde::{Deserialize, Serialize};

// ------------------------------------------------------------ damage types

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum DmgType {
    Physical,
    Hellfire,
    Hex,
    Void,
}

pub const DMG_TYPES: [DmgType; 4] = [
    DmgType::Physical,
    DmgType::Hellfire,
    DmgType::Hex,
    DmgType::Void,
];

impl DmgType {
    pub fn index(self) -> usize {
        match self {
            DmgType::Physical => 0,
            DmgType::Hellfire => 1,
            DmgType::Hex => 2,
            DmgType::Void => 3,
        }
    }
    /// Palette color for numbers/particles — legibility is law (Pillar 3).
    pub fn color(self) -> Vec3 {
        match self {
            DmgType::Physical => palette::BONE,
            DmgType::Hellfire => palette::BRIMSTONE,
            DmgType::Hex => palette::HEX,
            DmgType::Void => Vec3::new(0.35, 0.75, 0.95), // spectral cyan
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            DmgType::Physical => "PHYSICAL",
            DmgType::Hellfire => "HELLFIRE",
            DmgType::Hex => "HEX",
            DmgType::Void => "VOID",
        }
    }
}

/// A packet of damage in all four types at once.
pub type DmgVec = [f32; 4];

pub fn dmg_total(d: &DmgVec) -> f32 {
    d.iter().sum()
}

pub fn dominant_type(d: &DmgVec) -> DmgType {
    let mut best = 0;
    for i in 1..4 {
        if d[i] > d[best] {
            best = i;
        }
    }
    DMG_TYPES[best]
}

// ---------------------------------------------------------------- statuses

pub const ST_IGNITE: StatusId = StatusId::of("ignite");
pub const ST_HEXMARK: StatusId = StatusId::of("hexmark");
pub const ST_DISCORD: StatusId = StatusId::of("discord");
pub const ST_BLIGHT: StatusId = StatusId::of("blight");
pub const ST_PETRIFY: StatusId = StatusId::of("petrify");
pub const ST_CONSECRATE: StatusId = StatusId::of("consecrate");

pub fn status_color(id: StatusId) -> Vec3 {
    if id == ST_IGNITE {
        palette::BRIMSTONE
    } else if id == ST_HEXMARK {
        palette::HEX
    } else if id == ST_DISCORD {
        palette::BLOOD
    } else if id == ST_BLIGHT {
        palette::ICHOR
    } else if id == ST_PETRIFY {
        palette::ASH * 2.0
    } else {
        palette::GOLD
    }
}

pub fn status_by_name(name: &str) -> StatusId {
    StatusId(goetia::key64(name))
}

// ---------------------------------------------------------------- triggers

pub const TR_KILL: TriggerKind = TriggerKind::of("on_kill");
pub const TR_CRIT: TriggerKind = TriggerKind::of("on_crit");
pub const TR_STATUS_APPLY: TriggerKind = TriggerKind::of("on_status_apply");
pub const TR_STATUS_DETONATE: TriggerKind = TriggerKind::of("on_status_detonate");
pub const TR_NTH_CAST: TriggerKind = TriggerKind::of("nth_cast");
pub const TR_DODGE: TriggerKind = TriggerKind::of("on_dodge");
pub const TR_LOOT: TriggerKind = TriggerKind::of("on_loot_pickup");
pub const TR_LOW_LIFE: TriggerKind = TriggerKind::of("on_low_life");

pub fn trigger_by_name(name: &str) -> TriggerKind {
    TriggerKind(goetia::key64(name))
}

// --------------------------------------------------------------- stat keys

pub const K_MAX_HP: StatKey = StatKey::of("max_hp");
pub const K_REGEN: StatKey = StatKey::of("hp_regen");
pub const K_SPEED: StatKey = StatKey::of("move_speed");
pub const K_CAST_SPEED: StatKey = StatKey::of("cast_speed");
pub const K_CRIT: StatKey = StatKey::of("crit_chance");
pub const K_CRIT_MULT: StatKey = StatKey::of("crit_mult");
pub const K_DMG: StatKey = StatKey::of("dmg_global");
pub const K_DMG_PHYS: StatKey = StatKey::of("dmg_phys");
pub const K_DMG_FIRE: StatKey = StatKey::of("dmg_hellfire");
pub const K_DMG_HEX: StatKey = StatKey::of("dmg_hex");
pub const K_DMG_VOID: StatKey = StatKey::of("dmg_void");
pub const K_STATUS_CHANCE: StatKey = StatKey::of("status_chance");
pub const K_AOE: StatKey = StatKey::of("aoe");
pub const K_PROJ_SPEED: StatKey = StatKey::of("proj_speed");
pub const K_ARMOR: StatKey = StatKey::of("armor");
pub const K_RESIST: StatKey = StatKey::of("resist");
pub const K_LOOT_QUANT: StatKey = StatKey::of("loot_quant");
pub const K_LOOT_RARE: StatKey = StatKey::of("loot_rare");
pub const K_MINION: StatKey = StatKey::of("minion_dmg");

pub fn dmg_key(t: DmgType) -> StatKey {
    match t {
        DmgType::Physical => K_DMG_PHYS,
        DmgType::Hellfire => K_DMG_FIRE,
        DmgType::Hex => K_DMG_HEX,
        DmgType::Void => K_DMG_VOID,
    }
}

// ---------------------------------------------------------------- factions

pub const MASK_ENEMY: u32 = 0b01;
pub const MASK_PLAYER: u32 = 0b10;

// ------------------------------------------------------------------ realms

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Demon {
    Vassago,
    Andras,
    Buer,
}

impl Demon {
    pub fn signature(self) -> Vec3 {
        match self {
            Demon::Vassago => Vec3::new(0.85, 0.15, 0.12), // crimson
            Demon::Andras => Vec3::new(0.9, 0.72, 0.2),    // sickly gold
            Demon::Buer => Vec3::new(0.3, 0.85, 0.9),      // spectral cyan
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Demon::Vassago => "VASSAGO",
            Demon::Andras => "ANDRAS",
            Demon::Buer => "BUER",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Demon::Vassago => "FINDER OF HIDDEN THINGS",
            Demon::Andras => "SOWER OF DISCORD",
            Demon::Buer => "THE HEALER-WHEEL",
        }
    }
    pub fn index(self) -> usize {
        match self {
            Demon::Vassago => 0,
            Demon::Andras => 1,
            Demon::Buer => 2,
        }
    }
}

pub const DEMONS: [Demon; 3] = [Demon::Vassago, Demon::Andras, Demon::Buer];

// -------------------------------------------------------------- item slots

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Slot {
    Weapon,
    Armor,
    Relic,
    Ring,
}

pub const EQUIP_SLOTS: usize = 5; // weapon, armor, relic, ring, ring

pub fn equip_slot_kind(i: usize) -> Slot {
    match i {
        0 => Slot::Weapon,
        1 => Slot::Armor,
        2 => Slot::Relic,
        _ => Slot::Ring,
    }
}

pub fn slot_name(s: Slot) -> &'static str {
    match s {
        Slot::Weapon => "WEAPON",
        Slot::Armor => "ARMOR",
        Slot::Relic => "RELIC",
        Slot::Ring => "RING",
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub enum Rarity {
    Common,
    Magic,
    Rare,
    Goetic,
}

impl Rarity {
    pub fn color(self) -> Vec3 {
        match self {
            Rarity::Common => palette::ASH * 3.5,
            Rarity::Magic => Vec3::new(0.35, 0.75, 0.95),
            Rarity::Rare => palette::GOLD,
            Rarity::Goetic => palette::BRIMSTONE,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Rarity::Common => "COMMON",
            Rarity::Magic => "MAGIC",
            Rarity::Rare => "RARE",
            Rarity::Goetic => "GOETIC",
        }
    }
}
