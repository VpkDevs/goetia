//! Content schemas + the ContentDb. Everything gameplay-defining lives in RON
//! under `demonicon/data/` and deserializes into these types. Combinatorics
//! over content volume: skills, sigils, affixes, contracts and Goetics all
//! share one Reaction vocabulary handled by one trigger processor.

use crate::vocab::*;
use goetia::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

fn one() -> f32 {
    1.0
}
fn zero() -> f32 {
    0.0
}

// ---------------------------------------------------------------- reactions

/// The shared behavioral unit: when trigger `on` fires (for the player),
/// with `chance`, do `action`. Sigils, affixes, contracts and Goetics all
/// carry these — that is Pillar 2 in one struct.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reaction {
    pub on: String,
    #[serde(default = "one")]
    pub chance: f32,
    pub action: Action,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Action {
    /// Explosion at the trigger target, pct of player weapon damage.
    Nova { pct: f32, dtype: DmgType, radius: f32 },
    /// Apply a status at the trigger target (or around it).
    ApplyStatus { status: String, stacks: u32, magnitude: f32, radius: f32 },
    /// Recast the player's last skill at pct power (echoes count as casts —
    /// yes, that loops; the trigger budget is the only referee).
    Echo { pct: f32 },
    /// Reset the last-cast skill's cooldown.
    FreeReset,
    Heal { pct_max: f32 },
    /// Copy the named status from the trigger target to enemies in radius.
    SpreadStatus { status: String, radius: f32 },
    /// Detonate the named status on the trigger target.
    Detonate { status: String },
    /// Temporary frenzy buff.
    Frenzy { ticks: u32, cast_speed: f32, move_speed: f32 },
    /// Extra crafting dust.
    Dust { amount: u32 },
}

// ------------------------------------------------------------------ skills

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StatusApply {
    pub status: String,
    pub chance: f32,
    pub stacks: u32,
    /// Magnitude as a fraction of the hit's total damage.
    pub magnitude: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SkillKind {
    Projectile { speed: f32, radius: f32, count: u32, spread_deg: f32, pierce: u32, life_ticks: u32 },
    Nova { radius: f32 },
    Ground { radius: f32, duration_ticks: u32, tick_interval: u32, consecrate: bool },
    Minion { count: u32, life_ticks: u32, attack_cd: u32, speed: f32 },
    Beam { range: f32, width: f32, tick_interval: u32 },
    Dash { dist: f32 },
    Curse { radius: f32 },
    Totem { duration_ticks: u32, fire_cd: u32, proj_speed: f32 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkillDef {
    pub id: String,
    pub name: String,
    pub desc: String,
    pub cd_ticks: u32,
    /// Base damage per type.
    pub dmg: Vec<(DmgType, f32)>,
    #[serde(default)]
    pub apply: Vec<StatusApply>,
    pub kind: SkillKind,
}

impl SkillDef {
    pub fn dmg_vec(&self) -> DmgVec {
        let mut v = [0.0; 4];
        for (t, a) in &self.dmg {
            v[t.index()] += a;
        }
        v
    }
}

// ------------------------------------------------------------------ sigils

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SigilOp {
    Echo { pct: f32 },
    Convert { from: DmgType, to: DmgType },
    Pierce { add: u32 },
    Orbit,
    AreaMul { m: f32 },
    CdMul { m: f32 },
    DmgMul { m: f32 },
    SpeedMul { m: f32 },
    CountAdd { n: u32 },
    DurationMul { m: f32 },
    AddApply(StatusApply),
    React(Reaction),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SigilDef {
    pub id: String,
    pub name: String,
    pub desc: String,
    pub ops: Vec<SigilOp>,
}

// ------------------------------------------------------------------ affixes

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub enum MOp {
    Add,
    Mul,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AffixDef {
    pub id: String,
    /// Display template; `#` is replaced by the rolled value.
    pub name: String,
    pub slots: Vec<Slot>,
    #[serde(default)]
    pub stat: Option<String>,
    #[serde(default)]
    pub op: Option<MOp>,
    #[serde(default = "zero")]
    pub lo: f32,
    #[serde(default = "zero")]
    pub hi: f32,
    #[serde(default = "one")]
    pub weight: f32,
    /// Curses live in the hidden pool; Vassago's contract activates them too.
    #[serde(default)]
    pub curse: bool,
    #[serde(default)]
    pub corrupt_only: bool,
    /// Eligible to roll as a hidden (veiled) affix on Rares/Goetics.
    #[serde(default)]
    pub hidden_pool: bool,
    #[serde(default)]
    pub reaction: Option<Reaction>,
}

// ----------------------------------------------------------------- rules

/// Rule-changers. Read by systems all over the game; contracts and Goetics
/// both use them.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Rule {
    /// Vassago: every hidden affix on carried items is active while veiled.
    AllHiddenActive,
    /// Andras: you can be Discorded; while Discorded, +power but procs may
    /// pick YOU as a target.
    PlayerDiscordable { power: f32 },
    ProcsTargetSelf,
    /// Buer: the Cycle is locked to blight.
    LockBlight,
    BlightHealsYou,
    /// Vassago: hidden affixes reveal on pickup.
    AppraiseOnPickup,
    /// Goetic: all damage dealt twice, second hit one second later.
    DoubleDamageDelayed,
    /// Goetic: all damage becomes Hellfire.
    AllHellfire,
    /// Goetic: your Ignites never expire (duration ×10).
    EternalIgnite,
    /// Goetic: dodge has no cooldown but costs 5% life.
    BloodDodge,
    /// Goetic: crits petrify.
    CritsPetrify,
    /// Goetic: standing still consecrates the ground under you.
    StillnessConsecrates,
    /// Goetic: your totems/minions inherit your reactions.
    ServantsInherit,
    /// Goetic: loot beams pull items to you.
    LootGravity,
}

// --------------------------------------------------------------- contracts

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StatModDef {
    pub stat: String,
    pub op: MOp,
    pub value: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContractDef {
    pub id: String,
    pub demon: Demon,
    pub name: String,
    pub text: String,
    #[serde(default)]
    pub mods: Vec<StatModDef>,
    #[serde(default)]
    pub reactions: Vec<Reaction>,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

// ----------------------------------------------------------------- goetics

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GoeticDef {
    pub id: String,
    pub name: String,
    pub lore: String,
    pub slot: Slot,
    #[serde(default)]
    pub mods: Vec<StatModDef>,
    #[serde(default)]
    pub reactions: Vec<Reaction>,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

// ----------------------------------------------------------------- enemies

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum EnemyAiKind {
    Melee,
    Ranged { range: f32, proj_speed: f32 },
    Charger,
    /// Heals nearby allies per second (Buer clergy, bloom tenders).
    Support { heal: f32 },
    /// Buer spore wheel / BUER himself: orbits the arena.
    Wheel { orbit_radius: f32 },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub enum EnemyShape {
    Spike,   // wraiths, hounds
    Golem,   // slabs and ledgers
    Orb,     // floating indexes
    Wheel,   // spore wheels
    Column,  // clergy
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EnemyDef {
    pub id: String,
    pub name: String,
    pub hp: f32,
    pub dmg: f32,
    pub dmg_type: DmgType,
    pub speed: f32,
    pub radius: f32,
    pub scale: f32,
    pub shape: EnemyShape,
    pub ai: EnemyAiKind,
    #[serde(default = "one")]
    pub weight: f32,
    /// Schism knights: damage multiplier when no ally within 6m.
    #[serde(default = "one")]
    pub apart_bonus: f32,
    /// HP per second regen (blight clergy).
    #[serde(default = "zero")]
    pub regen: f32,
    /// Status applied on hitting the player.
    #[serde(default)]
    pub inflict: Option<StatusApply>,
}

// ------------------------------------------------------------------ realms

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RealmDef {
    pub demon: Demon,
    pub name: String,
    pub rooms_file: String,
    pub grammar_file: String,
    pub enemies: Vec<String>,
    pub boss: String,
    pub boss_hp: f32,
    /// Ambient darkness multiplier (Vassago boss arena goes darker still).
    #[serde(default = "one")]
    pub ambient: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RealmModDef {
    pub id: String,
    pub text: String,
    #[serde(default = "one")]
    pub hp_mul: f32,
    #[serde(default = "one")]
    pub dmg_mul: f32,
    #[serde(default = "one")]
    pub speed_mul: f32,
    #[serde(default = "one")]
    pub loot_mul: f32,
    #[serde(default)]
    pub spawn_discord: u32,
    #[serde(default)]
    pub reveal_hidden: bool,
    #[serde(default)]
    pub hostile_shrines: bool,
    #[serde(default = "one")]
    pub cycle_rate_mul: f32,
    #[serde(default)]
    pub death_novas: bool,
}

// ------------------------------------------------------------------ naming

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NamingDef {
    pub adjectives: Vec<String>,
    pub nouns: Vec<String>,
    pub sources: Vec<String>,
    pub deeds: Vec<String>,
}

// -------------------------------------------------------------- content db

pub struct ContentDb {
    pub skills: DataRegistry<Vec<SkillDef>>,
    pub sigils: DataRegistry<Vec<SigilDef>>,
    pub affixes: DataRegistry<Vec<AffixDef>>,
    pub contracts: DataRegistry<Vec<ContractDef>>,
    pub goetics: DataRegistry<Vec<GoeticDef>>,
    pub enemies: DataRegistry<Vec<EnemyDef>>,
    pub realms: DataRegistry<Vec<RealmDef>>,
    pub realm_mods: DataRegistry<Vec<RealmModDef>>,
    pub statuses: DataRegistry<Vec<StatusDef>>,
    pub naming: DataRegistry<NamingDef>,
    pub rooms: [DataRegistry<Vec<RoomTemplate>>; 3],
    pub grammars: [DataRegistry<RealmGrammar>; 3],
    handles: Handles,
    pub version: u64,
}

struct Handles {
    skills: DataHandle,
    sigils: DataHandle,
    affixes: DataHandle,
    contracts: DataHandle,
    goetics: DataHandle,
    enemies: DataHandle,
    realms: DataHandle,
    realm_mods: DataHandle,
    statuses: DataHandle,
    naming: DataHandle,
    rooms: [DataHandle; 3],
    grammars: [DataHandle; 3],
}

pub fn data_path(rel: &str) -> String {
    // Run from anywhere: prefer CARGO_MANIFEST_DIR layout.
    format!("{}/data/{}", env!("CARGO_MANIFEST_DIR"), rel)
}

fn load<T: serde::de::DeserializeOwned>(reg: &mut DataRegistry<T>, rel: &str) -> DataHandle {
    reg.load(data_path(rel)).unwrap_or_else(|e| panic!("content load failed: {e}"))
}

impl ContentDb {
    pub fn load_all() -> ContentDb {
        let mut skills = DataRegistry::new();
        let mut sigils = DataRegistry::new();
        let mut affixes = DataRegistry::new();
        let mut contracts = DataRegistry::new();
        let mut goetics = DataRegistry::new();
        let mut enemies = DataRegistry::new();
        let mut realms = DataRegistry::new();
        let mut realm_mods = DataRegistry::new();
        let mut statuses = DataRegistry::new();
        let mut naming = DataRegistry::new();
        let mut rooms = [DataRegistry::new(), DataRegistry::new(), DataRegistry::new()];
        let mut grammars = [DataRegistry::new(), DataRegistry::new(), DataRegistry::new()];

        let handles = Handles {
            skills: load(&mut skills, "skills.ron"),
            sigils: load(&mut sigils, "sigils.ron"),
            affixes: load(&mut affixes, "affixes.ron"),
            contracts: load(&mut contracts, "contracts.ron"),
            goetics: load(&mut goetics, "goetics.ron"),
            enemies: load(&mut enemies, "enemies.ron"),
            realms: load(&mut realms, "realms.ron"),
            realm_mods: load(&mut realm_mods, "realm_mods.ron"),
            statuses: load(&mut statuses, "statuses.ron"),
            naming: load(&mut naming, "naming.ron"),
            rooms: [
                load(&mut rooms[0], "realms/vassago_rooms.ron"),
                load(&mut rooms[1], "realms/andras_rooms.ron"),
                load(&mut rooms[2], "realms/buer_rooms.ron"),
            ],
            grammars: [
                load(&mut grammars[0], "realms/vassago_grammar.ron"),
                load(&mut grammars[1], "realms/andras_grammar.ron"),
                load(&mut grammars[2], "realms/buer_grammar.ron"),
            ],
        };

        ContentDb {
            skills,
            sigils,
            affixes,
            contracts,
            goetics,
            enemies,
            realms,
            realm_mods,
            statuses,
            naming,
            rooms,
            grammars,
            handles,
            version: 0,
        }
    }

    /// Register statuses into an engine StatusRegistry.
    pub fn build_status_registry(&self) -> StatusRegistry {
        let mut reg = StatusRegistry::default();
        for def in self.statuses.get(&self.handles.statuses) {
            reg.register(def.clone());
        }
        reg
    }

    /// Debug hot reload; returns true if anything changed.
    pub fn poll(&mut self) -> bool {
        let mut n = 0;
        n += self.skills.poll_reload();
        n += self.sigils.poll_reload();
        n += self.affixes.poll_reload();
        n += self.contracts.poll_reload();
        n += self.goetics.poll_reload();
        n += self.enemies.poll_reload();
        n += self.realms.poll_reload();
        n += self.realm_mods.poll_reload();
        n += self.statuses.poll_reload();
        n += self.naming.poll_reload();
        for r in &mut self.rooms {
            n += r.poll_reload();
        }
        for g in &mut self.grammars {
            n += g.poll_reload();
        }
        if n > 0 {
            self.version += 1;
            true
        } else {
            false
        }
    }

    // Accessors -----------------------------------------------------------

    pub fn skills(&self) -> &[SkillDef] {
        self.skills.get(&self.handles.skills)
    }
    pub fn sigils(&self) -> &[SigilDef] {
        self.sigils.get(&self.handles.sigils)
    }
    pub fn affixes(&self) -> &[AffixDef] {
        self.affixes.get(&self.handles.affixes)
    }
    pub fn contracts(&self) -> &[ContractDef] {
        self.contracts.get(&self.handles.contracts)
    }
    pub fn goetics(&self) -> &[GoeticDef] {
        self.goetics.get(&self.handles.goetics)
    }
    pub fn enemies(&self) -> &[EnemyDef] {
        self.enemies.get(&self.handles.enemies)
    }
    pub fn realms(&self) -> &[RealmDef] {
        self.realms.get(&self.handles.realms)
    }
    pub fn realm_mods(&self) -> &[RealmModDef] {
        self.realm_mods.get(&self.handles.realm_mods)
    }
    pub fn naming(&self) -> &NamingDef {
        self.naming.get(&self.handles.naming)
    }
    pub fn room_templates(&self, demon: Demon) -> &[RoomTemplate] {
        self.rooms[demon.index()].get(&self.handles.rooms[demon.index()])
    }
    pub fn grammar(&self, demon: Demon) -> &RealmGrammar {
        self.grammars[demon.index()].get(&self.handles.grammars[demon.index()])
    }

    pub fn skill(&self, id: &str) -> &SkillDef {
        self.skills().iter().find(|s| s.id == id).unwrap_or_else(|| panic!("skill {id}"))
    }
    pub fn sigil(&self, id: &str) -> &SigilDef {
        self.sigils().iter().find(|s| s.id == id).unwrap_or_else(|| panic!("sigil {id}"))
    }
    pub fn affix(&self, id: &str) -> &AffixDef {
        self.affixes().iter().find(|s| s.id == id).unwrap_or_else(|| panic!("affix {id}"))
    }
    pub fn contract(&self, id: &str) -> &ContractDef {
        self.contracts().iter().find(|s| s.id == id).unwrap_or_else(|| panic!("contract {id}"))
    }
    pub fn goetic(&self, id: &str) -> &GoeticDef {
        self.goetics().iter().find(|s| s.id == id).unwrap_or_else(|| panic!("goetic {id}"))
    }
    pub fn enemy(&self, id: &str) -> &EnemyDef {
        self.enemies().iter().find(|s| s.id == id).unwrap_or_else(|| panic!("enemy {id}"))
    }
    pub fn realm(&self, demon: Demon) -> &RealmDef {
        self.realms().iter().find(|r| r.demon == demon).expect("realm def")
    }
}

/// Free function used by tests to sanity-check content integrity.
pub fn validate(db: &ContentDb) -> Vec<String> {
    let mut errs = Vec::new();
    let mut seen: HashMap<&str, ()> = HashMap::new();
    for s in db.skills() {
        if seen.insert(s.id.as_str(), ()).is_some() {
            errs.push(format!("duplicate skill id {}", s.id));
        }
    }
    for r in db.realms() {
        for e in &r.enemies {
            if !db.enemies().iter().any(|d| &d.id == e) {
                errs.push(format!("realm {} references missing enemy {e}", r.name));
            }
        }
        if !db.enemies().iter().any(|d| d.id == r.boss) {
            errs.push(format!("realm {} missing boss {}", r.name, r.boss));
        }
    }
    errs
}
