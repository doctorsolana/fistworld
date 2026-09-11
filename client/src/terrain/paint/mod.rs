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
    /// Transient road composites sample both inclusive chunk endpoints.
    /// Authored/generated base maps retain their original cell-centred layout.
    pub endpoint_samples: bool,
    /// The generated + authored surface before transient village roads are
    /// composited. Runtime overlays always restart here, so removing or
    /// upgrading a road cannot leave stale paint behind.
    pub base_weights: Vec<[u8; 4]>,
    /// Resolution of the immutable authored/generated base. Town-road chunks
    /// can render a finer composite without regenerating the world recipe.
    pub base_resolution: u32,
    pub weights: Vec<[u8; 4]>,
}

impl WeightMapData {
    pub(crate) fn sample_step(&self) -> f32 {
        shared::terrain::CHUNK_SIZE
            / if self.endpoint_samples {
                self.resolution.saturating_sub(1).max(1) as f32
            } else {
                self.resolution.max(1) as f32
            }
    }

    pub(crate) fn sample_position(&self, chunk_min: Vec2, x: u32, z: u32) -> Vec2 {
        let offset = if self.endpoint_samples { 0.0 } else { 0.5 };
        chunk_min + Vec2::new(x as f32 + offset, z as f32 + offset) * self.sample_step()
    }
}
