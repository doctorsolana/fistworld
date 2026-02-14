//! Environmental props - rocks, trees, grass, etc.
//!
//! Spawns authored map objects by chunk.

mod assets;
mod debug;
mod foliage;
mod kinds;
mod lod;
mod plugin;
mod spawn;
mod types;

pub use debug::PropLodDebugMode;
pub use plugin::PropsPlugin;
pub use types::*;

pub(crate) use kinds::is_tree_kind;
