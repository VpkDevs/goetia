//! Weighted tables with pity / streak-breaker support. Every miss ramps an
//! entry's effective weight by `pity_ramp`; a hit resets it. Loot math wants
//! exactly this shape.

use goetia_core::Pcg32;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TableEntry<T> {
    pub item: T,
    pub weight: f32,
    /// Effective weight multiplier grows by this per miss: w * (1 + ramp * misses).
    /// 0.0 = plain weighted roll.
    #[serde(default)]
    pub pity_ramp: f32,
    #[serde(skip)]
    misses: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WeightedTable<T> {
    entries: Vec<TableEntry<T>>,
}

impl<T> WeightedTable<T> {
    pub fn new() -> Self {
        WeightedTable { entries: Vec::new() }
    }

    pub fn push(&mut self, item: T, weight: f32, pity_ramp: f32) -> &mut Self {
        self.entries.push(TableEntry { item, weight, pity_ramp, misses: 0 });
        self
    }

    pub fn from_entries(entries: Vec<TableEntry<T>>) -> Self {
        WeightedTable { entries }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Roll once; updates pity counters. None on empty table.
    pub fn roll(&mut self, rng: &mut Pcg32) -> Option<&T> {
        if self.entries.is_empty() {
            return None;
        }
        let weights: Vec<f32> = self
            .entries
            .iter()
            .map(|e| e.weight * (1.0 + e.pity_ramp * e.misses as f32))
            .collect();
        let idx = rng.weighted_index(&weights)?;
        for (i, e) in self.entries.iter_mut().enumerate() {
            if i == idx {
                e.misses = 0;
            } else {
                e.misses += 1;
            }
        }
        Some(&self.entries[idx].item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pity_forces_rare_drop() {
        // Rare item at 1% base weight with aggressive pity must show up well
        // before 1000 rolls, and pity resets after a hit.
        let mut t = WeightedTable::new();
        t.push("common", 99.0, 0.0);
        t.push("rare", 1.0, 5.0);
        let mut rng = Pcg32::new(1, 1);
        let mut first_rare = None;
        for i in 0..500 {
            if *t.roll(&mut rng).unwrap() == "rare" {
                first_rare = Some(i);
                break;
            }
        }
        let hit = first_rare.expect("pity should force the rare");
        assert!(hit < 200, "rare took {hit} rolls");
    }

    #[test]
    fn deterministic() {
        let mk = || {
            let mut t = WeightedTable::new();
            t.push(1, 1.0, 0.5);
            t.push(2, 2.0, 0.0);
            t.push(3, 3.0, 1.0);
            t
        };
        let mut a = mk();
        let mut b = mk();
        let mut ra = Pcg32::new(5, 2);
        let mut rb = Pcg32::new(5, 2);
        let sa: Vec<i32> = (0..50).map(|_| *a.roll(&mut ra).unwrap()).collect();
        let sb: Vec<i32> = (0..50).map(|_| *b.roll(&mut rb).unwrap()).collect();
        assert_eq!(sa, sb);
    }
}
