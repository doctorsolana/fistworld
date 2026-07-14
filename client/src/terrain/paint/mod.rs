pub mod weights;

pub(crate) use weights::{build_weightmap_from_weights, log_weightmap_stats};

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::Extent3d;
use bevy::render::render_resource::{TextureDimension, TextureFormat};
use std::collections::HashMap;

use shared::terrain::ChunkCoord;

use super::materials::weightmap_sampler;

pub(crate) const WEIGHTMAP_RESOLUTION: u32 = shared::terrain::TERRAIN_WEIGHTMAP_RESOLUTION;
const WEIGHTMAP_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;

/// Live weightmap image handles + CPU copies per loaded chunk.
#[derive(Resource, Default)]
pub struct TerrainPaintState {
    pub weightmaps: HashMap<ChunkCoord, WeightMapData>,
}

#[derive(Clone)]
pub struct WeightMapData {
    pub handle: Handle<Image>,
    pub resolution: u32,
    pub weights: Vec<[u8; 4]>,
}
