//! Authored terrain generation and chunk mesh helpers.
//!
//! World surface is loaded from a hand-authored heightmap in `maps/<map_id>/map.ron`.

mod constants;
mod map_access;
mod mesh;
mod sampling;
mod types;
mod world;

pub use constants::*;
pub use map_access::world_pos_in_bounds;
pub use types::*;
pub use world::{TerrainGenerator, WorldTerrain};
