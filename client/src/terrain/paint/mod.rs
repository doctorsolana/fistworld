pub mod masks;
pub mod ops;
pub mod spatial_index;
pub mod weights;

use masks::paint_mask;
pub(crate) use ops::ingest_paint_ops;
pub(crate) use ops::{apply_paint_op_to_weights, paint_op_intersects_chunk};
use spatial_index::{op_chunk_coords, paint_op_bounds};
use weights::{apply_layer_weight, update_weightmap_image};
pub(crate) use weights::{
    build_weightmap_from_weights, build_weightmap_weights, log_weightmap_stats,
};

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::Extent3d;
use bevy::render::render_resource::{TextureDimension, TextureFormat};
use std::collections::HashMap;
use std::time::Instant;

use shared::terrain::{ChunkCoord, TerrainGenerator, TerrainLayer, TerrainPaintOp, CHUNK_SIZE};

use super::chunks::LoadedChunks;
use super::debug::PerfHitchStats;
use super::materials::weightmap_sampler;
use crate::ui::DebugPerfSettings;

pub(crate) const WEIGHTMAP_RESOLUTION: u32 = 64;
const WEIGHTMAP_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;

#[derive(Resource, Default)]
pub struct TerrainPaintState {
    pub ops: HashMap<u64, TerrainPaintOp>,
    pub weightmaps: HashMap<ChunkCoord, WeightMapData>,
}

#[derive(Resource, Default)]
pub struct TerrainPaintSpatialIndex {
    pub ops_by_chunk: HashMap<ChunkCoord, Vec<u64>>,
}

#[derive(Clone)]
pub struct WeightMapData {
    pub handle: Handle<Image>,
    pub resolution: u32,
    pub weights: Vec<[u8; 4]>,
}
