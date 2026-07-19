//! goetia_procgen — seeded realm assembly. Generic mechanism only: room
//! templates + adjacency grammar in (RON-authored by the game), connected
//! layout out. Also weighted tables with pity for loot math.
//!
//! All randomness comes through a caller-supplied [`goetia_core::Pcg32`]
//! (typically the `layout` stream), so loot rolls can never perturb layout.

pub mod rooms;
pub mod table;

pub use rooms::{Door, PlacedRoom, RealmGrammar, RealmLayout, RoomTemplate, Side};
pub use table::WeightedTable;
