//! goetia_core — archetype ECS, work-stealing job system, deterministic time & RNG.
//!
//! Everything in the fixed-tick simulation flows through this crate. Determinism
//! contract: same seed + same inputs => bit-identical world state. All sim
//! randomness must come from [`pcg::PcgStreams`]; wall-clock time never touches
//! sim state.

pub mod ecs;
pub mod events;
pub mod hash;
pub mod jobs;
pub mod pcg;
pub mod schedule;
pub mod time;

pub use ecs::{CommandBuffer, Entity, Query, World};
pub use events::Events;
pub use hash::StateHasher;
pub use jobs::JobPool;
pub use pcg::{Pcg32, PcgStreams};
pub use schedule::{Schedule, SystemDef};
pub use time::GameClock;
