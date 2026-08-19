//! Client-side water rendering (rivers + ocean)

pub mod chunks;
mod edge;
pub mod material;
pub mod mesh;
pub mod overlay;

pub use chunks::WaterChunk;

use chunks::{
    cleanup_water_chunks, spawn_water_chunks, LoadedWaterChunks, WaterDetailCoverage,
    WaterRenderAssets,
};
use edge::{ensure_ocean_edge_extension, update_ocean_edge_extension};
use material::{
    setup_water_assets, sync_water_detail_bounds, sync_water_map_bounds, sync_water_wave_clock,
    update_water_cull_mode, update_water_sun_dir, ToonWaterMaterial,
};
use overlay::{despawn_underwater_overlay, spawn_underwater_overlay, update_underwater_overlay};

use bevy::asset::RenderAssetUsages;
use bevy::color::LinearRgba;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, PrimitiveTopology, RenderPipelineDescriptor, ShaderType,
    SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;
use std::collections::HashMap;

use shared::terrain::{ChunkCoord, WorldTerrain, CHUNK_RESOLUTION, CHUNK_SIZE, VERTEX_SPACING};

use crate::render::systems::{ClientWorldRoot, SunLight};
use crate::states::GameState;
use crate::terrain::TerrainUpdateSet;

/// Linear palette for the detailed animated water surface.
pub(crate) const WATER_SHALLOW_RGBA: [f32; 4] = [0.12, 0.62, 0.92, 0.70];
pub(crate) const WATER_DEEP_RGBA: [f32; 4] = [0.012, 0.09, 0.26, 0.92];

pub struct WaterPlugin;

impl Plugin for WaterPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<ToonWaterMaterial>::default());
        app.init_resource::<LoadedWaterChunks>();
        app.init_resource::<WaterDetailCoverage>();
        app.add_systems(Startup, setup_water_assets);
        app.add_systems(OnEnter(GameState::Playing), spawn_underwater_overlay);
        app.add_systems(OnExit(GameState::Playing), despawn_underwater_overlay);
        app.add_systems(
            Update,
            (
                cleanup_water_chunks,
                spawn_water_chunks,
                ensure_ocean_edge_extension,
                update_ocean_edge_extension,
            )
                .chain()
                .after(TerrainUpdateSet)
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            (
                update_underwater_overlay,
                update_water_cull_mode,
                update_water_sun_dir,
                sync_water_wave_clock,
                sync_water_map_bounds,
                sync_water_detail_bounds.after(spawn_water_chunks),
            )
                .after(TerrainUpdateSet)
                .run_if(in_state(GameState::Playing)),
        );
    }
}
