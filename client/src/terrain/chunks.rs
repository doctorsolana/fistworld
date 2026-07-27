use bevy::prelude::*;
use std::collections::HashSet;

use shared::terrain::ChunkCoord;

use super::materials::TerrainSplatMaterial;

/// Marker component for terrain chunk entities.
#[derive(Component)]
pub struct TerrainChunk {
    pub coord: ChunkCoord,
    pub weightmap: Handle<Image>,
    pub material: Handle<TerrainSplatMaterial>,
}

/// Tracks whether this chunk is using the lite terrain material.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct TerrainMaterialLod {
    pub use_lite: bool,
}

/// Resource tracking which chunks are currently loaded.
#[derive(Resource, Default)]
pub struct LoadedChunks {
    pub chunks: HashSet<ChunkCoord>,
}

/// Far terrain mesh (static, low-res).
#[derive(Component)]
pub struct FarTerrain;

#[derive(Component)]
pub struct FarTerrainState {
    pub center_cell: IVec2,
    pub view_distance: i32,
    /// True when the hole under the streamed chunks is currently filled (map view).
    /// Part of the change detection: zooming in place must also trigger a recut.
    pub hole_filled: bool,
}

#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone)]
pub struct TerrainUpdateSet;
