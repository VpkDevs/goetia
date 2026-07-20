//! Items, loot generation, corruption, and build compilation.
//! An equipped build compiles to: one StatSheet + a flat list of Reactions +
//! a flat list of Rules. Combat only ever consults the compiled form.

use crate::content::*;
use crate::vocab::*;
use goetia::prelude::*;
use serde::{Deserialize, Serialize};

// ------------------------------------------------------------------- items

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AffixRoll {
    pub def_id: String,
    pub value: f32,
    /// Rolled from the hidden pool: veiled until revealed.
    pub hidden: bool,
    pub revealed: bool,
    pub corrupt: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ItemInstance {
    pub uid: u64,
    pub name: String,
    pub slot: Slot,
    pub rarity: Rarity,
    pub ilvl: u32,
    pub affixes: Vec<AffixRoll>,
    /// Hand-designed Goetic id, or None (awakened items carry generated data).
    pub goetic: Option<String>,
    pub lore: Option<String>,
    /// Awakened via the corruption altar (procedurally mutated Goetic).
    pub awakened: bool,
}

impl ItemInstance {
    /// Is this affix currently contributing? (hidden + unrevealed = latent,
    /// unless the Vassago contract flips them all on.)
    pub fn affix_active(&self, a: &AffixRoll, all_hidden_active: bool) -> bool {
        !a.hidden || a.revealed || all_hidden_active
    }
}

// --------------------------------------------------------------- loot state

/// Per-profile loot RNG state (pity lives here so it persists).
#[derive(Serialize, Deserialize)]
pub struct LootTables {
    /// Pity counters as misses since last hit for rare/goetic.
    pub rare_misses: u32,
    pub goetic_misses: u32,
    pub next_uid: u64,
}

impl Default for LootTables {
    fn default() -> Self {
        LootTables {
            rare_misses: 0,
            goetic_misses: 0,
            next_uid: 1,
        }
    }
}

const SLOT_POOL: [Slot; 5] = [
    Slot::Weapon,
    Slot::Armor,
    Slot::Relic,
    Slot::Ring,
    Slot::Ring,
];

pub fn roll_rarity(lt: &mut LootTables, rng: &mut Pcg32, rare_bonus: f32) -> Rarity {
    // Base weights with pity ramps (streak-breakers per the engine tables).
    let w_common = 55.0;
    let w_magic = 34.0;
    let w_rare = (9.0 + lt.rare_misses as f32 * 0.9) * (1.0 + rare_bonus);
    let w_goetic = (0.7 + lt.goetic_misses as f32 * 0.12) * (1.0 + rare_bonus);
    let pick = rng
        .weighted_index(&[w_common, w_magic, w_rare, w_goetic])
        .unwrap_or(0);
    match pick {
        3 => {
            lt.goetic_misses = 0;
            lt.rare_misses = 0;
            Rarity::Goetic
        }
        2 => {
            lt.rare_misses = 0;
            lt.goetic_misses += 1;
            Rarity::Rare
        }
        n => {
            lt.rare_misses += 1;
            lt.goetic_misses += 1;
            if n == 1 {
                Rarity::Magic
            } else {
                Rarity::Common
            }
        }
    }
}

fn affix_pool(db: &ContentDb, slot: Slot, hidden: bool, allow_corrupt: bool) -> Vec<&AffixDef> {
    db.affixes()
        .iter()
        .filter(|a| a.slots.contains(&slot))
        .filter(|a| a.hidden_pool == hidden || (hidden && a.curse))
        .filter(|a| allow_corrupt || !a.corrupt_only)
        .filter(|a| hidden || !a.curse) // curses only arrive veiled
        .collect()
}

fn roll_affix(pool: &[&AffixDef], rng: &mut Pcg32, hidden: bool) -> Option<AffixRoll> {
    if pool.is_empty() {
        return None;
    }
    let weights: Vec<f32> = pool.iter().map(|a| a.weight).collect();
    let i = rng.weighted_index(&weights)?;
    let a = pool[i];
    Some(AffixRoll {
        def_id: a.id.clone(),
        value: rng.range_f32(a.lo, a.hi),
        hidden,
        revealed: false,
        corrupt: a.corrupt_only,
    })
}

fn base_name(db: &ContentDb, slot: Slot, rng: &mut Pcg32) -> String {
    let n = db.naming();
    let noun = match slot {
        Slot::Weapon => ["ATHAME", "SCOURGE", "CENSER", "BRAND"],
        Slot::Armor => ["SHROUD", "CUIRASS", "VESTMENT", "PALL"],
        Slot::Relic => ["IDOL", "LEDGER", "PHYLACTERY", "SEAL"],
        Slot::Ring => ["BAND", "SIGNET", "COIL", "KNOT"],
    };
    let adj = &n.adjectives[rng.range_u32(n.adjectives.len() as u32) as usize];
    format!("{} {}", adj, noun[rng.range_u32(4) as usize])
}

/// Generate one item of the given rarity.
pub fn gen_item(
    db: &ContentDb,
    lt: &mut LootTables,
    rng: &mut Pcg32,
    rarity: Rarity,
    tier: u32,
    reveal_hidden: bool,
) -> ItemInstance {
    let slot = SLOT_POOL[rng.range_u32(5) as usize];
    let uid = lt.next_uid;
    lt.next_uid += 1;

    if rarity == Rarity::Goetic {
        let candidates: Vec<&GoeticDef> = db.goetics().iter().filter(|g| g.slot == slot).collect();
        let all: Vec<&GoeticDef> = if candidates.is_empty() {
            db.goetics().iter().collect()
        } else {
            candidates
        };
        let g = all[rng.range_u32(all.len() as u32) as usize];
        let mut item = ItemInstance {
            uid,
            name: g.name.clone(),
            slot: g.slot,
            rarity,
            ilvl: tier,
            affixes: Vec::new(),
            goetic: Some(g.id.clone()),
            lore: Some(g.lore.clone()),
            awakened: false,
        };
        maybe_hidden(db, rng, &mut item, reveal_hidden);
        return item;
    }

    let n_affixes = match rarity {
        Rarity::Common => 0,
        Rarity::Magic => 1 + rng.range_u32(2),
        Rarity::Rare => 3 + rng.range_u32(3),
        Rarity::Goetic => 0,
    };
    let pool = affix_pool(db, slot, false, false);
    let mut affixes = Vec::new();
    for _ in 0..n_affixes {
        if let Some(a) = roll_affix(&pool, rng, false) {
            // No duplicate stat spam: cap identical defs at 2.
            if affixes
                .iter()
                .filter(|x: &&AffixRoll| x.def_id == a.def_id)
                .count()
                < 2
            {
                affixes.push(a);
            }
        }
    }
    let mut item = ItemInstance {
        uid,
        name: base_name(db, slot, rng),
        slot,
        rarity,
        ilvl: tier,
        affixes,
        goetic: None,
        lore: None,
        awakened: false,
    };
    if rarity == Rarity::Rare {
        maybe_hidden(db, rng, &mut item, reveal_hidden);
    }
    item
}

/// Rares/Goetics may carry one veiled affix (Vassago's whole deal).
fn maybe_hidden(db: &ContentDb, rng: &mut Pcg32, item: &mut ItemInstance, reveal: bool) {
    if rng.chance(0.45) {
        let pool = affix_pool(db, item.slot, true, false);
        if let Some(mut a) = roll_affix(&pool, rng, true) {
            a.revealed = reveal;
            item.affixes.push(a);
        }
    }
}

// -------------------------------------------------------------- corruption

pub enum CorruptOutcome {
    Bricked,
    Rerolled,
    CorruptAffix,
    Awakened,
}

/// Gamble an item at a realm's corruption altar.
pub fn corrupt_item(
    db: &ContentDb,
    lt: &mut LootTables,
    corrupt_rng: &mut Pcg32,
    naming_rng: &mut Pcg32,
    item: &mut ItemInstance,
) -> CorruptOutcome {
    let _ = lt;
    let roll = corrupt_rng.next_f32();
    if roll < 0.22 {
        return CorruptOutcome::Bricked; // caller deletes the item
    }
    if roll < 0.60 {
        // Reroll all non-hidden affixes.
        let n = item.affixes.iter().filter(|a| !a.hidden).count().max(2);
        item.affixes.retain(|a| a.hidden);
        let pool = affix_pool(db, item.slot, false, false);
        for _ in 0..n {
            if let Some(a) = roll_affix(&pool, corrupt_rng, false) {
                item.affixes.push(a);
            }
        }
        if item.rarity < Rarity::Rare {
            item.rarity = Rarity::Rare;
        }
        return CorruptOutcome::Rerolled;
    }
    if roll < 0.85 {
        let pool = affix_pool(db, item.slot, false, true);
        let corrupt_pool: Vec<&AffixDef> = pool.into_iter().filter(|a| a.corrupt_only).collect();
        if let Some(a) = roll_affix(&corrupt_pool, corrupt_rng, false) {
            item.affixes.push(a);
        }
        return CorruptOutcome::CorruptAffix;
    }
    // AWAKEN: procedurally mutated Goetic with generated name + lore.
    item.rarity = Rarity::Goetic;
    item.awakened = true;
    let n = db.naming();
    let pick = |v: &Vec<String>, r: &mut Pcg32| v[r.range_u32(v.len() as u32) as usize].clone();
    item.name = format!(
        "{} {}",
        pick(&n.adjectives, naming_rng),
        pick(&n.nouns, naming_rng)
    );
    item.lore = Some(format!(
        "TAKEN FROM {} WHO {}",
        pick(&n.sources, naming_rng),
        pick(&n.deeds, naming_rng)
    ));
    // Two extra behavioral affixes from the full reaction pool + a power surge.
    let reaction_pool: Vec<&AffixDef> = db
        .affixes()
        .iter()
        .filter(|a| a.reaction.is_some() && a.slots.contains(&item.slot))
        .collect();
    for _ in 0..2 {
        if let Some(a) = roll_affix(&reaction_pool, corrupt_rng, false) {
            item.affixes.push(a);
        }
    }
    for a in &mut item.affixes {
        a.value *= 1.3;
    }
    CorruptOutcome::Awakened
}

// ------------------------------------------------------------ compiled build

/// Player loadout (persisted).
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Loadout {
    pub equipment: Vec<Option<ItemInstance>>, // EQUIP_SLOTS entries
    pub skills: Vec<Option<String>>,          // 6 slots of skill ids
    pub sigils: Vec<Vec<Option<String>>>,     // per skill slot, 3 sockets
    pub contracts: Vec<Option<String>>,       // 3 slots
}

impl Loadout {
    pub fn new() -> Loadout {
        Loadout {
            equipment: vec![None; EQUIP_SLOTS],
            skills: vec![None; 6],
            sigils: vec![vec![None; 3]; 6],
            contracts: vec![None; 3],
        }
    }
}

/// Everything combat consults at runtime. Rebuilt on any loadout change —
/// respec is free (Pillar: experimentation friction is a bug).
pub struct CompiledBuild {
    pub sheet: StatSheet,
    pub reactions: Vec<Reaction>,
    pub rules: Vec<Rule>,
}

impl CompiledBuild {
    pub fn has_rule(&self, r: &Rule) -> bool {
        self.rules.iter().any(|x| x == r)
    }
    pub fn discord_power(&self) -> Option<f32> {
        self.rules.iter().find_map(|r| match r {
            Rule::PlayerDiscordable { power } => Some(*power),
            _ => None,
        })
    }
}

pub fn compile_build(db: &ContentDb, loadout: &Loadout) -> CompiledBuild {
    let mut sheet = StatSheet::new()
        .with(K_MAX_HP, 170.0)
        .with(K_REGEN, 2.5)
        .with(K_SPEED, 0.0)
        .with(K_CAST_SPEED, 0.0)
        .with(K_CRIT, 0.05)
        .with(K_CRIT_MULT, 0.5) // bonus over 1.0
        .with(K_STATUS_CHANCE, 0.0)
        .with(K_ARMOR, 0.0)
        .with(K_RESIST, 0.0);
    let mut reactions = Vec::new();
    let mut rules = Vec::new();

    let apply_mod = |sheet: &mut StatSheet, stat: &str, op: MOp, value: f32| {
        let key = StatKey(goetia::key64(stat));
        match op {
            MOp::Add => sheet.add_modifier(key, ModOp::Add, value),
            MOp::Mul => sheet.add_modifier(key, ModOp::Mul, value),
        };
    };

    // Rules first (AllHiddenActive changes which affixes count).
    for c in loadout.contracts.iter().flatten() {
        let def = db.contract(c);
        rules.extend(def.rules.iter().cloned());
    }
    for item in loadout.equipment.iter().flatten() {
        if let Some(g) = &item.goetic {
            rules.extend(db.goetic(g).rules.iter().cloned());
        }
    }
    let all_hidden = rules.contains(&Rule::AllHiddenActive);

    for item in loadout.equipment.iter().flatten() {
        for roll in &item.affixes {
            if !item.affix_active(roll, all_hidden) {
                continue;
            }
            let def = db.affix(&roll.def_id);
            if let (Some(stat), Some(op)) = (&def.stat, def.op) {
                apply_mod(&mut sheet, stat, op, roll.value);
            }
            if let Some(r) = &def.reaction {
                reactions.push(r.clone());
            }
        }
        if let Some(g) = &item.goetic {
            let def = db.goetic(g);
            for m in &def.mods {
                apply_mod(&mut sheet, &m.stat, m.op, m.value);
            }
            reactions.extend(def.reactions.iter().cloned());
        }
    }
    for c in loadout.contracts.iter().flatten() {
        let def = db.contract(c);
        for m in &def.mods {
            apply_mod(&mut sheet, &m.stat, m.op, m.value);
        }
        reactions.extend(def.reactions.iter().cloned());
    }
    // Sigil React ops are live while the skill is socketed.
    for (si, skill) in loadout.skills.iter().enumerate() {
        if skill.is_none() {
            continue;
        }
        for sig in loadout.sigils[si].iter().flatten() {
            for op in &db.sigil(sig).ops {
                if let SigilOp::React(r) = op {
                    reactions.push(r.clone());
                }
            }
        }
    }

    CompiledBuild {
        sheet,
        reactions,
        rules,
    }
}

// ----------------------------------------------------------------- display

pub fn item_lines(db: &ContentDb, item: &ItemInstance, all_hidden: bool) -> Vec<(String, Vec3)> {
    let mut out = Vec::new();
    out.push((item.name.clone(), item.rarity.color()));
    out.push((
        format!(
            "{} {} T{}",
            item.rarity.name(),
            slot_name(item.slot),
            item.ilvl
        ),
        palette::ASH * 4.0,
    ));
    if let Some(g) = &item.goetic {
        let def = db.goetic(g);
        for m in &def.mods {
            out.push((
                format!("{} {:+.0}%", m.stat.to_uppercase(), m.value * 100.0),
                palette::BONE,
            ));
        }
        for r in &def.reactions {
            out.push((reaction_line(r), palette::BRIMSTONE));
        }
        for r in &def.rules {
            out.push((rule_line(r), palette::BRIMSTONE));
        }
    }
    for roll in &item.affixes {
        let def = db.affix(&roll.def_id);
        if roll.hidden && !roll.revealed && !all_hidden {
            out.push(("??? (VEILED)".into(), palette::HEX));
            continue;
        }
        let pct = (roll.value * 100.0).round();
        let flat = roll.value.round();
        let text = def
            .name
            .replace("#%", &format!("{pct:+.0}%"))
            .replace('#', &format!("{flat:+.0}"));
        let color = if def.curse {
            palette::BLOOD
        } else if roll.corrupt {
            palette::HEX
        } else if def.reaction.is_some() {
            palette::GOLD
        } else if roll.hidden {
            Vec3::new(0.6, 0.5, 0.9)
        } else {
            palette::BONE
        };
        out.push((text, color));
    }
    if let Some(l) = &item.lore {
        out.push((format!("\"{l}\""), palette::ASH * 3.0));
    }
    out
}

pub fn reaction_line(r: &Reaction) -> String {
    let when = r.on.replace('_', " ").to_uppercase();
    let what = match &r.action {
        Action::Nova { pct, dtype, .. } => format!("{} NOVA {:.0}%", dtype.name(), pct * 100.0),
        Action::ApplyStatus { status, stacks, .. } => {
            format!("APPLY {}X {}", stacks, status.to_uppercase())
        }
        Action::Echo { pct } => format!("ECHO CAST {:.0}%", pct * 100.0),
        Action::FreeReset => "RESET COOLDOWN".into(),
        Action::Heal { pct_max } => format!("HEAL {:.0}%", pct_max * 100.0),
        Action::SpreadStatus { status, .. } => format!("SPREAD {}", status.to_uppercase()),
        Action::Detonate { status } => format!("DETONATE {}", status.to_uppercase()),
        Action::Frenzy { .. } => "FRENZY".into(),
        Action::Dust { amount } => format!("+{amount} DUST"),
    };
    if r.chance < 1.0 {
        format!("{}: {:.0}% {}", when, r.chance * 100.0, what)
    } else {
        format!("{when}: {what}")
    }
}

pub fn rule_line(r: &Rule) -> String {
    match r {
        Rule::AllHiddenActive => "ALL VEILED AFFIXES ARE ACTIVE".into(),
        Rule::PlayerDiscordable { power } => {
            format!(
                "YOU CAN BE DISCORDED. WHILE DISCORDED +{:.0}% DAMAGE",
                power * 100.0
            )
        }
        Rule::ProcsTargetSelf => "YOUR PROCS MAY TARGET YOU".into(),
        Rule::LockBlight => "THE CYCLE IS LOCKED TO BLIGHT".into(),
        Rule::BlightHealsYou => "BLIGHT HEALS YOU".into(),
        Rule::AppraiseOnPickup => "ITEMS ARE APPRAISED ON PICKUP".into(),
        Rule::DoubleDamageDelayed => "ALL DAMAGE DEALT TWICE, ONE SECOND APART".into(),
        Rule::AllHellfire => "ALL YOUR DAMAGE IS HELLFIRE".into(),
        Rule::EternalIgnite => "YOUR IGNITES REFUSE TO DIE".into(),
        Rule::BloodDodge => "DODGE HAS NO COOLDOWN. IT DRINKS 5% LIFE".into(),
        Rule::CritsPetrify => "YOUR CRITS PETRIFY".into(),
        Rule::StillnessConsecrates => "STILLNESS CONSECRATES THE GROUND".into(),
        Rule::ServantsInherit => "YOUR SERVANTS INHERIT YOUR REACTIONS".into(),
        Rule::LootGravity => "LOOT CRAWLS TO YOU".into(),
    }
}
