//! Layered stat sheets: base value + additive + multiplicative + override,
//! recomputed lazily per-key (dirty tracking), deterministic iteration order.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Interned stat key. Build with [`StatKey::of`] (const-friendly via
/// `goetia_combat::key64`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct StatKey(pub u64);

impl StatKey {
    pub const fn of(name: &str) -> Self {
        StatKey(crate::key64(name))
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum ModOp {
    /// Added to base before multipliers.
    Add,
    /// Summed, then applied as `* (1 + sum)` — "increased by 20%" stacks additively.
    Mul,
    /// Highest-priority override wins outright (ties: most recent).
    Override,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct ModifierHandle(u64);

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Modifier {
    key: StatKey,
    op: ModOp,
    value: f32,
    priority: i32,
}

/// A keyed stat container. Attach as a component; one per entity.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct StatSheet {
    base: BTreeMap<StatKey, f32>,
    mods: BTreeMap<ModifierHandle, Modifier>,
    #[serde(skip)]
    cache: BTreeMap<StatKey, (f32, bool)>, // (value, valid)
    next_handle: u64,
}

impl StatSheet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, key: StatKey, base: f32) -> Self {
        self.set_base(key, base);
        self
    }

    pub fn set_base(&mut self, key: StatKey, value: f32) {
        self.base.insert(key, value);
        self.invalidate(key);
    }

    pub fn base(&self, key: StatKey) -> f32 {
        self.base.get(&key).copied().unwrap_or(0.0)
    }

    pub fn add_modifier(&mut self, key: StatKey, op: ModOp, value: f32) -> ModifierHandle {
        self.add_modifier_prio(key, op, value, 0)
    }

    pub fn add_modifier_prio(
        &mut self,
        key: StatKey,
        op: ModOp,
        value: f32,
        priority: i32,
    ) -> ModifierHandle {
        let h = ModifierHandle(self.next_handle);
        self.next_handle += 1;
        self.mods.insert(
            h,
            Modifier {
                key,
                op,
                value,
                priority,
            },
        );
        self.invalidate(key);
        h
    }

    pub fn remove_modifier(&mut self, h: ModifierHandle) -> bool {
        if let Some(m) = self.mods.remove(&h) {
            self.invalidate(m.key);
            true
        } else {
            false
        }
    }

    fn invalidate(&mut self, key: StatKey) {
        if let Some(c) = self.cache.get_mut(&key) {
            c.1 = false;
        }
    }

    /// Final value: `(base + Σadd) * (1 + Σmul)`, unless an Override modifier
    /// exists (highest priority, then latest handle, wins).
    pub fn get(&mut self, key: StatKey) -> f32 {
        if let Some(&(v, true)) = self.cache.get(&key) {
            return v;
        }
        let v = self.compute(key);
        self.cache.insert(key, (v, true));
        v
    }

    /// Non-caching read (for `&self` contexts like parallel systems).
    pub fn peek(&self, key: StatKey) -> f32 {
        if let Some(&(v, true)) = self.cache.get(&key) {
            return v;
        }
        self.compute(key)
    }

    fn compute(&self, key: StatKey) -> f32 {
        let mut add = 0.0f32;
        let mut mul = 0.0f32;
        let mut over: Option<(i32, ModifierHandle, f32)> = None;
        for (h, m) in &self.mods {
            if m.key != key {
                continue;
            }
            match m.op {
                ModOp::Add => add += m.value,
                ModOp::Mul => mul += m.value,
                ModOp::Override => {
                    let cand = (m.priority, *h, m.value);
                    over = Some(match over {
                        Some(cur) if (cur.0, cur.1) > (cand.0, cand.1) => cur,
                        _ => cand,
                    });
                }
            }
        }
        if let Some((_, _, v)) = over {
            return v;
        }
        (self.base(key) + add) * (1.0 + mul)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HP: StatKey = StatKey::of("hp");
    const DMG: StatKey = StatKey::of("dmg");

    #[test]
    fn layering() {
        let mut s = StatSheet::new().with(HP, 100.0).with(DMG, 10.0);
        assert_eq!(s.get(HP), 100.0);
        let a = s.add_modifier(HP, ModOp::Add, 50.0);
        let _b = s.add_modifier(HP, ModOp::Mul, 0.2);
        let close = |a: f32, b: f32| (a - b).abs() < 1e-3;
        assert!(close(s.get(HP), 180.0)); // (100+50)*1.2
        let o = s.add_modifier(HP, ModOp::Override, 1.0);
        assert_eq!(s.get(HP), 1.0);
        s.remove_modifier(o);
        assert!(close(s.get(HP), 180.0));
        s.remove_modifier(a);
        assert!(close(s.get(HP), 120.0));
        assert_eq!(s.get(DMG), 10.0); // untouched key unaffected
    }

    #[test]
    fn cache_invalidation_is_per_key() {
        let mut s = StatSheet::new().with(HP, 10.0).with(DMG, 5.0);
        let _ = s.get(HP);
        let _ = s.get(DMG);
        s.add_modifier(DMG, ModOp::Add, 5.0);
        assert_eq!(s.get(HP), 10.0);
        assert_eq!(s.get(DMG), 10.0);
    }
}
