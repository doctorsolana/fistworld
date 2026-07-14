//! Authored terrain generation and runtime terrain data.
//!
//! Scale: 1 unit = 1 meter
//! - Player is 1.8m tall (human scale)
//! - Chunks are 64m x 64m

mod generator;
mod paint;
mod serialization;

pub use generator::*;
pub use paint::{
    apply_terrain_paint_op_to_weights, build_terrain_weightmap_weights, terrain_paint_op_bounds,
    terrain_paint_op_chunk_coords, terrain_paint_op_intersects_chunk, TerrainLayer, TerrainPaintOp,
    TerrainPaintShape, TERRAIN_WEIGHTMAP_RESOLUTION,
};
pub use serialization::{TerrainDeltaChunk, TerrainDeltaData};
