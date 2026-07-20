//! goetia — the engine facade. Shot 2 builds against this crate and only
//! this crate; see API_CONTRACT.md at the workspace root for the frozen
//! surface.

pub mod app;
pub mod floaters;
pub mod input;
pub mod overlay;

pub use app::{App, AppConfig, Engine, Game};
pub use floaters::DamageNumbers;
pub use input::Input;

// Subsystem re-exports (the whole public API flows through here).
pub use goetia_audio::{AudioEngine, LoopHandle, Sound};
pub use goetia_combat::{
    key64,
    spatial::SpatialGrid,
    stats::{ModOp, ModifierHandle, StatKey, StatSheet},
    status::{ActiveStatus, StatusBag, StatusDef, StatusEvent, StatusId, StatusRegistry},
    triggers::{
        TriggerBus, TriggerConfig, TriggerEmitter, TriggerEvent, TriggerKind, TriggerStats,
    },
};
pub use goetia_core::{
    ecs::Bundle, hash::fnv1a64, time::FIXED_DT, time::TICK_RATE, CommandBuffer, Entity, Events,
    GameClock, JobPool, Pcg32, PcgStreams, Schedule, StateHasher, SystemDef, World,
};
pub use goetia_data::{DataHandle, DataRegistry, SaveFile, SaveResult};
pub use goetia_procgen::{
    rooms::{assemble, Door, PlacedRoom, RealmGrammar, RealmLayout, RoomTemplate, Side},
    WeightedTable,
};
pub use goetia_render::{
    palette, CameraRig, FrameSubmit, InstanceRaw, Light, MeshBuilder, MeshHandle, ParticleSpawn,
    Renderer, UiBatch,
};

/// Commonly used external types, re-exported so games depend only on goetia.
pub mod prelude {
    pub use super::*;
    pub use glam::{Mat4, Quat, Vec2, Vec3, Vec4};
    pub use winit::event::{ElementState, MouseButton, WindowEvent};
    pub use winit::keyboard::KeyCode;
}
