//! Client-side terrain rendering
//!
//! Updated for Bevy 0.18

mod chunks;
mod debug;
pub mod map_view;
mod materials;
mod mesh;
mod paint;
mod streaming;

use bevy::pbr::MaterialPlugin;
use bevy::prelude::*;

use crate::states::GameState;
use shared::terrain::WorldTerrain;

pub use chunks::{LoadedChunks, TerrainChunk, TerrainUpdateSet};
pub use debug::PerfHitchStats;
// Cloud-shadow sync writes the palette's cloud fields into chunk materials.
pub use materials::TerrainSplatMaterial;
pub(crate) use streaming::TerrainStreamingState;

/// Plugin for terrain rendering.
pub struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<materials::TerrainSplatMaterial>::default());
        app.init_resource::<chunks::LoadedChunks>();
        app.init_resource::<WorldTerrain>();
        app.init_resource::<streaming::TerrainStreamingState>();
        app.init_resource::<streaming::TerrainDeltaState>();
        app.init_resource::<paint::TerrainPaintState>();
        app.init_resource::<streaming::TerrainChunkTasks>();
        app.init_resource::<streaming::TerrainTaskScratch>();
        app.init_resource::<streaming::far_terrain::FarTerrainHoleTask>();
        app.init_resource::<debug::TerrainDebugSettings>();
        app.init_resource::<debug::PerfHitchStats>();
        app.init_resource::<debug::TerrainPerfLogConfig>();
        app.init_resource::<debug::TerrainWarmupState>();
        app.add_systems(Startup, materials::setup_terrain_render_assets);
        app.add_systems(
            Update,
            materials::build_terrain_texture_arrays.run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            PreUpdate,
            debug::reset_perf_hitch_stats.run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            (
                streaming::ingest_delta_chunks_from_server,
                streaming::regenerate_dirty_chunks,
                streaming::update_terrain_render_distance,
                streaming::update_terrain_chunks,
                streaming::spawn_terrain_chunks,
                streaming::process_chunk_tasks,
                streaming::update_terrain_material_lod,
                streaming::ensure_far_terrain_mesh,
                streaming::update_far_terrain_hole,
            )
                .chain()
                .in_set(chunks::TerrainUpdateSet)
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            materials::sync_terrain_water_clock
                .after(chunks::TerrainUpdateSet)
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            (
                debug::track_asset_activity,
                debug::handle_terrain_debug_input,
                debug::sync_terrain_debug_materials,
                debug::warmup_terrain_pipeline,
                debug::cleanup_warmup_terrain,
            )
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            PostUpdate,
            debug::log_perf_hitch_stats.run_if(in_state(GameState::Playing)),
        );
    }
}
