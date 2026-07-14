//! Client-side water rendering (rivers + ocean)

pub mod chunks;
pub mod material;
pub mod mesh;
pub mod overlay;

pub use chunks::WaterChunk;

use chunks::{cleanup_water_chunks, spawn_water_chunks, LoadedWaterChunks, WaterRenderAssets};
use material::{
    emit_water_ripples, setup_water_assets, update_water_cull_mode, update_water_sun_dir,
    ToonWaterMaterial,
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

use shared::components::{LocalPlayer, PlayerPosition, PlayerWaterState};
use shared::terrain::{ChunkCoord, WorldTerrain, CHUNK_RESOLUTION, CHUNK_SIZE, VERTEX_SPACING};

use crate::render::systems::{ClientWorldRoot, SunLight};
use crate::states::GameState;
use crate::terrain::{LoadedChunks, TerrainUpdateSet};

pub struct WaterPlugin;

impl Plugin for WaterPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<ToonWaterMaterial>::default());
        app.init_resource::<LoadedWaterChunks>();
        app.add_systems(Startup, setup_water_assets);
        app.add_systems(OnEnter(GameState::Playing), spawn_underwater_overlay);
        app.add_systems(OnExit(GameState::Playing), despawn_underwater_overlay);
        app.add_systems(
            Update,
            (cleanup_water_chunks, spawn_water_chunks)
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
                emit_water_ripples,
            )
                .run_if(in_state(GameState::Playing)),
        );
    }
}
