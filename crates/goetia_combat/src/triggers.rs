//! The trigger bus — the game's proc engine. Event → reaction chains with
//! loop damping: a global per-tick budget plus per-chain depth cap and
//! magnitude falloff. Deliberately survives infinite loops: when the budget
//! is hit, remaining events are dropped and counted, and the sim holds rate.

use goetia_core::Entity;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct TriggerKind(pub u64);

impl TriggerKind {
    pub const fn of(name: &str) -> Self {
        TriggerKind(crate::key64(name))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TriggerEvent {
    pub kind: TriggerKind,
    pub source: Entity,
    pub target: Entity,
    /// Game-defined payload (damage amount, stack count, …). Scaled by
    /// `chain_falloff` on every reaction hop.
    pub magnitude: f32,
    pub chain_depth: u32,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct TriggerStats {
    pub emitted: u64,
    pub processed: u64,
    pub dropped_budget: u64,
    pub dropped_depth: u64,
    pub dropped_floor: u64,
    pub max_depth_seen: u32,
    /// Ticks on which the budget ran out (cumulative).
    pub budget_hit_ticks: u64,
    /// Events processed on the most recent tick.
    pub last_tick_processed: u32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct TriggerConfig {
    /// Hard cap on events processed per tick.
    pub budget_per_tick: u32,
    /// Chains deeper than this are cut.
    pub max_chain_depth: u32,
    /// Magnitude multiplier per chain hop (e.g. 0.7 = each echo is 30% weaker).
    pub chain_falloff: f32,
    /// Events whose magnitude decays below this are dropped.
    pub magnitude_floor: f32,
}

impl Default for TriggerConfig {
    fn default() -> Self {
        TriggerConfig {
            budget_per_tick: 2048,
            max_chain_depth: 16,
            chain_falloff: 0.7,
            magnitude_floor: 0.05,
        }
    }
}

/// Handed to reaction handlers so they can extend the chain. Child events
/// inherit depth+1 and falloff-scaled magnitude, and are dropped (counted)
/// past the depth cap or magnitude floor.
pub struct TriggerEmitter<'a> {
    queue: &'a mut VecDeque<TriggerEvent>,
    stats: &'a mut TriggerStats,
    config: &'a TriggerConfig,
    parent_depth: u32,
    parent_magnitude_scale: f32,
}

impl<'a> TriggerEmitter<'a> {
    pub fn emit(&mut self, kind: TriggerKind, source: Entity, target: Entity, magnitude: f32) {
        let depth = self.parent_depth + 1;
        self.stats.emitted += 1;
        if depth > self.config.max_chain_depth {
            self.stats.dropped_depth += 1;
            return;
        }
        let m = magnitude * self.parent_magnitude_scale;
        if m.abs() < self.config.magnitude_floor {
            self.stats.dropped_floor += 1;
            return;
        }
        self.stats.max_depth_seen = self.stats.max_depth_seen.max(depth);
        self.queue.push_back(TriggerEvent {
            kind,
            source,
            target,
            magnitude: m,
            chain_depth: depth,
        });
    }
}

#[derive(Default)]
pub struct TriggerBus {
    queue: VecDeque<TriggerEvent>,
    pub config: TriggerConfig,
    pub stats: TriggerStats,
}

impl TriggerBus {
    pub fn new(config: TriggerConfig) -> Self {
        TriggerBus {
            queue: VecDeque::new(),
            config,
            stats: TriggerStats::default(),
        }
    }

    /// Emit a root event (chain depth 0, no falloff).
    pub fn emit(&mut self, kind: TriggerKind, source: Entity, target: Entity, magnitude: f32) {
        self.stats.emitted += 1;
        self.queue.push_back(TriggerEvent {
            kind,
            source,
            target,
            magnitude,
            chain_depth: 0,
        });
    }

    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    /// Process up to `budget_per_tick` events. The handler receives each event
    /// plus an emitter for chaining reactions. Call once per sim tick.
    /// Unprocessed events at budget exhaustion are DROPPED — a contained
    /// explosion beats a frozen sim.
    pub fn process(&mut self, mut handler: impl FnMut(&TriggerEvent, &mut TriggerEmitter)) {
        let mut spent = 0u32;
        while let Some(ev) = self.queue.pop_front() {
            if spent >= self.config.budget_per_tick {
                // Put it back to count precisely, then drop everything.
                self.queue.push_front(ev);
                let dropped = self.queue.len() as u64;
                self.stats.dropped_budget += dropped;
                self.queue.clear();
                self.stats.budget_hit_ticks += 1;
                break;
            }
            spent += 1;
            self.stats.processed += 1;
            // Falloff compounds per hop: a reaction at depth d emits children
            // scaled by falloff^(d+1), so runaway chains decay geometrically
            // no matter what magnitude the handler passes.
            let scale = self.config.chain_falloff.powi(ev.chain_depth as i32 + 1);
            let mut emitter = TriggerEmitter {
                queue: &mut self.queue,
                stats: &mut self.stats,
                config: &self.config,
                parent_depth: ev.chain_depth,
                parent_magnitude_scale: scale,
            };
            handler(&ev, &mut emitter);
        }
        self.stats.last_tick_processed = spent;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: TriggerKind = TriggerKind::of("on_hit");
    const B: TriggerKind = TriggerKind::of("on_echo");
    const E: Entity = Entity { index: 0, gen: 0 };

    #[test]
    fn infinite_loop_is_contained() {
        let mut bus = TriggerBus::new(TriggerConfig {
            budget_per_tick: 100,
            max_chain_depth: 1000, // depth cap effectively off
            chain_falloff: 1.0,    // no decay
            magnitude_floor: 0.0,  // no floor
        });
        bus.emit(A, E, E, 1.0);
        // A emits two B, each B emits two A: exponential explosion.
        bus.process(|ev, em| {
            let k = if ev.kind == A { B } else { A };
            em.emit(k, E, E, 1.0);
            em.emit(k, E, E, 1.0);
        });
        assert_eq!(bus.stats.processed, 100); // budget held
        assert!(bus.stats.dropped_budget > 0);
        assert_eq!(bus.pending(), 0); // nothing leaks to next tick
        assert_eq!(bus.stats.budget_hit_ticks, 1);
    }

    #[test]
    fn falloff_and_depth_damp_chains() {
        let mut bus = TriggerBus::new(TriggerConfig {
            budget_per_tick: 1_000_000,
            max_chain_depth: 64,
            chain_falloff: 0.5,
            magnitude_floor: 0.1,
        });
        bus.emit(A, E, E, 1.0);
        bus.process(|_, em| em.emit(A, E, E, 1.0));
        // magnitudes: 1.0 (d0) -> 0.5 -> 0.25 -> 0.125 -> dropped (0.0625 < 0.1)
        assert_eq!(bus.stats.processed, 4);
        assert_eq!(bus.stats.dropped_floor, 1);
        assert_eq!(bus.stats.dropped_budget, 0);
    }
}
