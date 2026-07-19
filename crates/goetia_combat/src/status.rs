//! Status effect machinery: apply / stack / tick / expire / detonate, with
//! event emission. Mechanics-agnostic — "Ignite" is game data (a StatusDef
//! loaded from RON); this module only runs the lifecycle.

use goetia_core::Entity;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct StatusId(pub u64);

impl StatusId {
    pub const fn of(name: &str) -> Self {
        StatusId(crate::key64(name))
    }
}

/// Definition of a status effect (game data, typically RON-loaded).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StatusDef {
    pub name: String,
    /// 0 = unstackable (re-apply refreshes only).
    pub max_stacks: u32,
    /// Lifetime in sim ticks (60/s). 0 = permanent until removed/detonated.
    pub duration_ticks: u32,
    /// Emit `Ticked` every N ticks. 0 = never ticks.
    pub tick_interval: u32,
    /// Re-applying resets the remaining duration.
    pub refresh_on_apply: bool,
}

impl StatusDef {
    pub fn id(&self) -> StatusId {
        StatusId(crate::key64(&self.name))
    }
}

/// Registry of definitions; world resource.
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct StatusRegistry {
    defs: BTreeMap<StatusId, StatusDef>,
}

impl StatusRegistry {
    pub fn register(&mut self, def: StatusDef) -> StatusId {
        let id = def.id();
        self.defs.insert(id, def);
        id
    }
    pub fn get(&self, id: StatusId) -> Option<&StatusDef> {
        self.defs.get(&id)
    }
    pub fn len(&self) -> usize {
        self.defs.len()
    }
    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ActiveStatus {
    pub id: StatusId,
    pub stacks: u32,
    pub remaining: u32,
    tick_timer: u32,
    /// Game-defined payload carried with the status (e.g. damage per tick).
    pub magnitude: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StatusEvent {
    Applied { entity: Entity, id: StatusId, stacks: u32 },
    Ticked { entity: Entity, id: StatusId, stacks: u32, magnitude: f32 },
    Expired { entity: Entity, id: StatusId, stacks: u32 },
    Detonated { entity: Entity, id: StatusId, stacks: u32, magnitude: f32 },
}

/// Per-entity component holding active statuses. Small vec; typical entity
/// carries < 8 statuses.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct StatusBag {
    pub active: Vec<ActiveStatus>,
}

impl StatusBag {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, id: StatusId) -> Option<&ActiveStatus> {
        self.active.iter().find(|s| s.id == id)
    }
    pub fn has(&self, id: StatusId) -> bool {
        self.get(id).is_some()
    }
    pub fn stacks(&self, id: StatusId) -> u32 {
        self.get(id).map(|s| s.stacks).unwrap_or(0)
    }

    /// Apply `stacks` of a status. Emits `Applied` with the resulting count.
    pub fn apply(
        &mut self,
        entity: Entity,
        def: &StatusDef,
        stacks: u32,
        magnitude: f32,
        events: &mut Vec<StatusEvent>,
    ) {
        let id = def.id();
        if let Some(s) = self.active.iter_mut().find(|s| s.id == id) {
            let cap = if def.max_stacks == 0 { 1 } else { def.max_stacks };
            s.stacks = (s.stacks + stacks).min(cap);
            s.magnitude = s.magnitude.max(magnitude);
            if def.refresh_on_apply {
                s.remaining = def.duration_ticks;
            }
            events.push(StatusEvent::Applied { entity, id, stacks: s.stacks });
        } else {
            self.active.push(ActiveStatus {
                id,
                stacks: stacks.max(1).min(if def.max_stacks == 0 { 1 } else { def.max_stacks }),
                remaining: def.duration_ticks,
                tick_timer: def.tick_interval,
                magnitude,
            });
            events.push(StatusEvent::Applied { entity, id, stacks: stacks.max(1) });
        }
    }

    /// Advance one sim tick: emits `Ticked` on interval, `Expired` on timeout.
    pub fn tick(
        &mut self,
        entity: Entity,
        registry: &StatusRegistry,
        events: &mut Vec<StatusEvent>,
    ) {
        let mut i = 0;
        while i < self.active.len() {
            let s = &mut self.active[i];
            let def = registry.get(s.id);
            let interval = def.map(|d| d.tick_interval).unwrap_or(0);
            if interval > 0 {
                s.tick_timer = s.tick_timer.saturating_sub(1);
                if s.tick_timer == 0 {
                    s.tick_timer = interval;
                    events.push(StatusEvent::Ticked {
                        entity,
                        id: s.id,
                        stacks: s.stacks,
                        magnitude: s.magnitude,
                    });
                }
            }
            let permanent = def.map(|d| d.duration_ticks == 0).unwrap_or(false);
            if !permanent {
                if s.remaining <= 1 {
                    let ev = StatusEvent::Expired { entity, id: s.id, stacks: s.stacks };
                    self.active.swap_remove(i);
                    events.push(ev);
                    continue; // don't advance i — swapped element takes slot i
                }
                s.remaining -= 1;
            }
            i += 1;
        }
    }

    /// Remove a status, emitting `Detonated` (e.g. consume Ignite for a blast).
    /// Returns (stacks, magnitude) if it was present.
    pub fn detonate(
        &mut self,
        entity: Entity,
        id: StatusId,
        events: &mut Vec<StatusEvent>,
    ) -> Option<(u32, f32)> {
        let idx = self.active.iter().position(|s| s.id == id)?;
        let s = self.active.swap_remove(idx);
        events.push(StatusEvent::Detonated {
            entity,
            id,
            stacks: s.stacks,
            magnitude: s.magnitude,
        });
        Some((s.stacks, s.magnitude))
    }

    /// Silent removal (dispel).
    pub fn remove(&mut self, id: StatusId) -> bool {
        if let Some(i) = self.active.iter().position(|s| s.id == id) {
            self.active.swap_remove(i);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(name: &str, stacks: u32, dur: u32, interval: u32) -> StatusDef {
        StatusDef {
            name: name.into(),
            max_stacks: stacks,
            duration_ticks: dur,
            tick_interval: interval,
            refresh_on_apply: true,
        }
    }

    #[test]
    fn lifecycle() {
        let e = Entity { index: 1, gen: 0 };
        let mut reg = StatusRegistry::default();
        let ignite = def("ignite", 5, 3, 1);
        let id = reg.register(ignite.clone());
        let mut bag = StatusBag::new();
        let mut evs = Vec::new();

        bag.apply(e, &ignite, 2, 7.0, &mut evs);
        assert_eq!(bag.stacks(id), 2);
        bag.apply(e, &ignite, 9, 3.0, &mut evs);
        assert_eq!(bag.stacks(id), 5); // capped

        evs.clear();
        bag.tick(e, &reg, &mut evs); // tick 1: Ticked
        bag.tick(e, &reg, &mut evs); // tick 2: Ticked
        bag.tick(e, &reg, &mut evs); // tick 3: Ticked + Expired
        let ticked = evs.iter().filter(|e| matches!(e, StatusEvent::Ticked { .. })).count();
        let expired = evs.iter().filter(|e| matches!(e, StatusEvent::Expired { .. })).count();
        assert_eq!(ticked, 3);
        assert_eq!(expired, 1);
        assert!(!bag.has(id));
    }

    #[test]
    fn detonate_consumes() {
        let e = Entity { index: 1, gen: 0 };
        let d = def("brand", 3, 100, 0);
        let mut bag = StatusBag::new();
        let mut evs = Vec::new();
        bag.apply(e, &d, 3, 12.0, &mut evs);
        let got = bag.detonate(e, d.id(), &mut evs);
        assert_eq!(got, Some((3, 12.0)));
        assert!(!bag.has(d.id()));
        assert!(matches!(evs.last(), Some(StatusEvent::Detonated { stacks: 3, .. })));
    }
}
