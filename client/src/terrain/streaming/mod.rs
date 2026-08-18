pub mod far_terrain;
pub mod ingest;
pub mod regenerate;
pub mod spawn;

pub(crate) use far_terrain::{ensure_far_terrain_mesh, update_far_terrain_hole};
pub(crate) use ingest::ingest_delta_chunks_from_server;
pub(crate) use regenerate::{process_chunk_tasks, regenerate_dirty_chunks};
pub(crate) use spawn::{
    spawn_terrain_chunks, update_terrain_chunks, update_terrain_material_lod,
    update_terrain_render_distance,
};

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::tasks::{block_on, poll_once, AsyncComputeTaskPool, Task};
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Instant;

use shared::terrain::{
    ChunkCoord, TerrainDeltaChunk, TerrainDeltaData, TerrainGenerator, WorldTerrain,
    CHUNK_RESOLUTION, CHUNK_SIZE,
};

use crate::render::systems::{ClientWorldRoot, GraphicsSettings};
use crate::streaming::{streaming_anchor, AnchorCamera, AnchorPlayer};
use crate::ui::DebugPerfSettings;

use super::chunks::{FarTerrain, FarTerrainState, LoadedChunks, TerrainChunk, TerrainMaterialLod};
use super::debug::{PerfHitchStats, TerrainDebugSettings};
use super::materials::{
    water_params_for_generator, TerrainRenderAssets, TerrainSplatExtension, TerrainSplatMaterial,
};
use super::mesh::{build_far_terrain_mesh, build_terrain_mesh, compute_chunk_tangents};
use super::paint::{
    build_weightmap_from_weights, log_weightmap_stats, TerrainPaintState, WEIGHTMAP_RESOLUTION,
};

/// Tracks which chunk the player is currently in and the desired chunk ordering.
#[derive(Resource)]
pub struct TerrainStreamingState {
    pub center: Option<ChunkCoord>,
    /// Desired chunks sorted from nearest -> farthest (spawn near chunks first).
    pub desired_order: Vec<ChunkCoord>,
    /// Render distance in chunks (may be lower than load distance).
    pub render_distance: i32,
    /// Last player chunk we applied terrain material LOD against.
    pub material_lod_center: Option<ChunkCoord>,
    /// Last normal-strength radius applied to loaded terrain materials.
    pub material_lod_radius: i32,
    /// A close-view hole handoff retained stale chunks that should be checked
    /// again after the far-terrain cutout moves to the new streaming centre.
    pub unload_pending: bool,
}

impl Default for TerrainStreamingState {
    fn default() -> Self {
        Self {
            center: None,
            desired_order: Vec::new(),
            render_distance: -1,
            material_lod_center: None,
            material_lod_radius: -1,
            unload_pending: false,
        }
    }
}

#[derive(Resource, Default)]
pub struct TerrainChunkTasks {
    pub tasks: HashMap<ChunkCoord, Task<ChunkBuildResult>>,
    pub ordered_coords: Vec<ChunkCoord>,
    pub order_center: Option<ChunkCoord>,
    pub order_view_distance: i32,
    pub order_dirty: bool,
}

impl TerrainChunkTasks {
    pub fn contains_key(&self, coord: &ChunkCoord) -> bool {
        self.tasks.contains_key(coord)
    }

    pub fn insert(&mut self, coord: ChunkCoord, task: Task<ChunkBuildResult>) {
        self.tasks.insert(coord, task);
        self.order_dirty = true;
    }

    pub fn remove(&mut self, coord: &ChunkCoord) {
        if self.tasks.remove(coord).is_some() {
            self.order_dirty = true;
        }
    }
}

#[derive(Resource, Default)]
pub struct TerrainTaskScratch {
    pub completed: Vec<ChunkBuildResult>,
    pub to_remove: Vec<ChunkCoord>,
}

pub struct ChunkBuildResult {
    pub coord: ChunkCoord,
    pub generator: TerrainGenerator,
    pub mesh_data: shared::terrain::ChunkMeshData,
    pub tangents: Vec<[f32; 4]>,
    pub weights: Vec<[u8; 4]>,
    pub resolution: u32,
}

/// Tracks the last seen delta chunk versions for change detection.
#[derive(Resource, Default)]
pub struct TerrainDeltaState {
    /// Last seen version per chunk coord.
    pub chunk_versions: HashMap<ChunkCoord, u32>,
    /// Chunks queued for mesh regeneration (dedup set).
    pub dirty_chunks: HashSet<ChunkCoord>,
    /// FIFO queue for dirty chunk regeneration (budgeted per frame).
    pub dirty_queue: VecDeque<ChunkCoord>,
}

// =============================================================================
// FAR TERRAIN SETTINGS (static low-res mesh)
// =============================================================================

// 513 puts a vertex every 16m on an 8km map. At 257 (32m) the baked
// coastline was a visibly square staircase from mid zoom; 16m keeps the
// one-time build and the async hole rebuilds cheap while halving the step.
const FAR_TERRAIN_RESOLUTION: usize = 513;
const FAR_TERRAIN_INNER_BUFFER: f32 = 0.0;
const FAR_TERRAIN_Y_OFFSET: f32 = -0.05;

const SPLAT_NORMAL_RATIO: f32 = 0.35;
const DIRTY_REGEN_MAX_PER_FRAME: usize = 8;
const TASK_FINALIZE_MAX_PER_FRAME: usize = 6;
const TASK_FINALIZE_MAX_BOOTSTRAP_PER_FRAME: usize = 24;
const EDGE_DELTA_EPSILON: f32 = 0.0001;
const EDGE_WEST: u8 = 1 << 0;
const EDGE_EAST: u8 = 1 << 1;
const EDGE_SOUTH: u8 = 1 << 2;
const EDGE_NORTH: u8 = 1 << 3;

pub(super) fn splat_normal_radius(render_distance: i32) -> i32 {
    ((render_distance as f32) * SPLAT_NORMAL_RATIO)
        .round()
        .max(1.0) as i32
}

pub(super) fn desired_terrain_normal_strength(
    coord: ChunkCoord,
    player_chunk: ChunkCoord,
    render_distance: i32,
) -> f32 {
    let radius = splat_normal_radius(render_distance);
    let dx = (coord.x - player_chunk.x).abs();
    let dz = (coord.z - player_chunk.z).abs();
    if dx.max(dz) <= radius {
        1.0
    } else {
        0.0
    }
}

// =============================================================================
// FAR TERRAIN (STATIC LOW-RES MESH)
// =============================================================================

// height-based render distance removed (always use view_distance)
