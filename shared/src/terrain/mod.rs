//! Authored terrain generation and runtime terrain data.
//!
//! Scale: 1 unit = 1 meter
//! - Player is 1.8m tall (human scale)
//! - Chunks are 64m x 64m

mod generator;
mod paint;
mod serialization;

pub use generator::*;
pub use paint::{TerrainLayer, TerrainPaintOp, TerrainPaintShape};
pub use serialization::{TerrainDeltaChunk, TerrainDeltaData};
