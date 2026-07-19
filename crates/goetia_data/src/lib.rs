//! goetia_data — RON data registry with hot reload, plus the save substrate.

pub mod registry;
pub mod save;

pub use registry::{DataHandle, DataRegistry};
pub use save::{SaveFile, SaveResult};
