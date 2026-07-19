//! goetia_combat — engine-level combat mechanisms. The game defines *content*
//! (what "Ignite" does); this crate defines *mechanism* (how statuses stack,
//! how proc chains are budgeted, how radius queries work).

pub mod spatial;
pub mod stats;
pub mod status;
pub mod triggers;

pub use spatial::SpatialGrid;
pub use stats::{ModOp, ModifierHandle, StatKey, StatSheet};
pub use status::{ActiveStatus, StatusBag, StatusDef, StatusEvent, StatusId, StatusRegistry};
pub use triggers::{TriggerBus, TriggerEmitter, TriggerEvent, TriggerKind, TriggerStats};

/// Interned key: FNV-1a of a name, computable at compile time.
/// Collisions across a game-sized vocabulary are practically impossible (64-bit).
pub const fn key64(name: &str) -> u64 {
    let bytes = name.as_bytes();
    let mut h = 0xcbf29ce484222325u64;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}
