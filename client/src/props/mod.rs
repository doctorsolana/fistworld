//! Environmental props - rocks, trees, grass, etc.
//!
//! Spawns authored map objects by chunk.

mod assets;
mod debug;
pub(crate) mod foliage;
mod ground_cover_chunked;
mod ground_cover_instancing;
mod kinds;
mod lod;
mod plugin;
mod simple_mesh;
mod spawn;
mod types;
mod wind;

pub use debug::PropLodDebugMode;
pub use plugin::PropsPlugin;
pub use types::*;

pub(crate) use kinds::{is_tree_kind, uses_swap_mesh_lod};
pub(crate) use simple_mesh::{try_spawn_simple_prop_mesh, SimplePropMeshCache};
