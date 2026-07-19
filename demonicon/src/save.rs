//! Persistence: bank, dust, loadout, loot pity, court progress.
//! Atomic writes via the engine save substrate; saved on bank/death/quit.

use crate::combat::Gs;
use crate::items::{ItemInstance, Loadout, LootTables};
use goetia::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Progress {
    /// Highest tier cleared per demon (Vassago, Andras, Buer).
    pub cleared: [u32; 3],
    pub runs: u64,
    pub deaths: u64,
}

pub fn save_path() -> String {
    format!("{}/saves/slot0.ron", env!("CARGO_MANIFEST_DIR"))
}

pub fn save_all(gs: &Gs, progress: &Progress) {
    let mut sf = SaveFile::new();
    let ok = sf.put("loadout", &gs.loadout).is_ok()
        && sf.put("bank", &gs.bank).is_ok()
        && sf.put("dust", &gs.dust).is_ok()
        && sf.put("loot_tables", &gs.loot).is_ok()
        && sf.put("progress", progress).is_ok();
    if !ok {
        log::warn!("save serialization failed");
        return;
    }
    if let Err(e) = sf.write(save_path()) {
        log::warn!("save write failed: {e}");
    }
}

pub struct Loaded {
    pub loadout: Loadout,
    pub bank: Vec<ItemInstance>,
    pub dust: u64,
    pub loot: LootTables,
    pub progress: Progress,
}

pub fn load_all() -> Option<Loaded> {
    let sf = SaveFile::read(save_path()).ok()?;
    Some(Loaded {
        loadout: sf.take("loadout").ok()?,
        bank: sf.take("bank").unwrap_or_default(),
        dust: sf.take("dust").unwrap_or(0),
        loot: sf.take("loot_tables").unwrap_or_default(),
        progress: sf.take("progress").unwrap_or_default(),
    })
}
