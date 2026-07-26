//! Authoritative terrain colliders (chunked heightfields).

use bevy::prelude::*;
use bevy_rapier3d::prelude::{Collider, RigidBody};
use std::collections::{HashMap, HashSet, VecDeque};

use shared::components::{Npc, NpcPosition, Player, PlayerPosition};
use shared::terrain::{ChunkCoord, WorldTerrain, CHUNK_SIZE};

use crate::physics::layers;

const DEFAULT_RADIUS_CHUNKS: i32 = 6;
const DEFAULT_MAX_LOAD_PER_TICK: usize = 8;
const DEFAULT_HEIGHTFIELD_RESOLUTION: usize = 65;

#[derive(Component, Clone, Copy, Debug)]
pub struct TerrainColliderChunk {
    pub coord: ChunkCoord,
}

#[derive(Resource, Clone, Debug)]
pub struct TerrainColliderSettings {
    pub radius_chunks: i32,
    pub max_load_per_tick: usize,
    pub heightfield_resolution: usize,
}

impl Default for TerrainColliderSettings {
    fn default() -> Self {
        let radius_chunks = std::env::var("CITYSIM_TERRAIN_COLLIDER_RADIUS_CHUNKS")
            .ok()
            .and_then(|raw| raw.parse::<i32>().ok())
            .unwrap_or(DEFAULT_RADIUS_CHUNKS)
            .clamp(2, 24);
        let max_load_per_tick = std::env::var("CITYSIM_TERRAIN_COLLIDER_MAX_LOAD_PER_TICK")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .unwrap_or(DEFAULT_MAX_LOAD_PER_TICK)
            .clamp(1, 64);
        let heightfield_resolution = std::env::var("CITYSIM_TERRAIN_COLLIDER_RESOLUTION")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .unwrap_or(DEFAULT_HEIGHTFIELD_RESOLUTION)
            .clamp(17, 257);
        Self {
            radius_chunks,
            max_load_per_tick,
            heightfield_resolution,
        }
    }
}

#[derive(Resource, Default)]
pub struct TerrainColliderRegistry {
    pub loaded: HashMap<ChunkCoord, Entity>,
    pub loaded_versions: HashMap<ChunkCoord, u32>,
    pub terrain_version: u32,
    pub full_rebuild_version: u32,
    pub radius_chunks: i32,
    pub heightfield_resolution: usize,
    pub centers: Vec<ChunkCoord>,
    pub desired_chunks: HashSet<ChunkCoord>,
    pub pending_load: VecDeque<ChunkCoord>,
}

fn chunk_heights(terrain: &WorldTerrain, coord: ChunkCoord, resolution: usize) -> Vec<f32> {
    let origin = coord.world_pos();
    let mut heights = Vec::with_capacity(resolution * resolution);
    let step = CHUNK_SIZE / (resolution.saturating_sub(1) as f32);

    // Rapier/nalgebra height matrices are column-major.
    // For matrix indexing heights[(row=z, col=x)], we must iterate x first, then z.
    for x in 0..resolution {
        for z in 0..resolution {
            let wx = origin.x + x as f32 * step;
            let wz = origin.z + z as f32 * step;
            heights.push(terrain.get_height(wx, wz));
        }
    }

    heights
}

fn spawn_terrain_chunk_collider(
    commands: &mut Commands,
    terrain: &WorldTerrain,
    coord: ChunkCoord,
    resolution: usize,
) -> Entity {
    let heights = chunk_heights(terrain, coord, resolution);
    let center = coord.world_pos() + Vec3::new(CHUNK_SIZE * 0.5, 0.0, CHUNK_SIZE * 0.5);
    let collider = Collider::heightfield(
        heights,
        resolution,
        resolution,
        Vec3::new(CHUNK_SIZE, 1.0, CHUNK_SIZE),
    );
    let transform = Transform::from_translation(center);

    commands
        .spawn((
            TerrainColliderChunk { coord },
            RigidBody::Fixed,
            collider,
            layers::terrain_groups(),
            transform,
            GlobalTransform::from(transform),
        ))
        .id()
}

fn desired_chunks_for_centers(centers: &[ChunkCoord], radius_chunks: i32) -> HashSet<ChunkCoord> {
    let mut desired = HashSet::new();
    for center in centers {
        desired.extend(center.chunks_in_radius(radius_chunks));
    }
    desired.retain(ChunkCoord::in_world_bounds);
    desired
}

fn gather_centers(
    players: &Query<&PlayerPosition, With<Player>>,
    npcs: &Query<&NpcPosition, With<Npc>>,
) -> Vec<ChunkCoord> {
    // Anchor chain: players -> NPCs -> origin. If both go away the collider set
    // silently collapses to chunk (0,0) and long-range raycasts pass through hills
    // with no error, so keep at least one live anchor.
    let mut centers = Vec::new();

    for pos in players.iter() {
        centers.push(ChunkCoord::from_world_pos(pos.0));
    }

    if centers.is_empty() {
        for pos in npcs.iter().take(8) {
            centers.push(ChunkCoord::from_world_pos(pos.0));
        }
    }

    if centers.is_empty() {
        centers.push(ChunkCoord::new(0, 0));
    }

    centers.sort_by_key(|coord| (coord.x, coord.z));
    centers.dedup();
    centers
}

pub fn sync_terrain_colliders(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    settings: Res<TerrainColliderSettings>,
    mut registry: ResMut<TerrainColliderRegistry>,
    players: Query<&PlayerPosition, With<Player>>,
    npcs: Query<&NpcPosition, With<Npc>>,
) {
    let terrain_version = terrain.modification_version();
    let full_rebuild_version = terrain.full_rebuild_version();
    let centers = gather_centers(&players, &npcs);
    let terrain_changed = registry.terrain_version != terrain_version;
    let full_rebuild_changed = registry.full_rebuild_version != full_rebuild_version;
    let resolution_changed = registry.heightfield_resolution != settings.heightfield_resolution;
    let desired_changed = full_rebuild_changed
        || resolution_changed
        || registry.radius_chunks != settings.radius_chunks
        || registry.centers != centers;

    if full_rebuild_changed || resolution_changed {
        let stale: Vec<Entity> = registry.loaded.values().copied().collect();
        for entity in stale {
            commands.entity(entity).despawn();
        }
        registry.loaded.clear();
        registry.loaded_versions.clear();
        registry.full_rebuild_version = full_rebuild_version;
        registry.heightfield_resolution = settings.heightfield_resolution;
    }

    if desired_changed {
        registry.centers = centers;
        registry.radius_chunks = settings.radius_chunks;
        registry.desired_chunks =
            desired_chunks_for_centers(&registry.centers, settings.radius_chunks);
        registry.pending_load.clear();

        let stale_chunks: Vec<ChunkCoord> = registry
            .loaded
            .keys()
            .copied()
            .filter(|coord| !registry.desired_chunks.contains(coord))
            .collect();
        for coord in stale_chunks {
            if let Some(entity) = registry.loaded.remove(&coord) {
                commands.entity(entity).despawn();
            }
            registry.loaded_versions.remove(&coord);
        }

        let mut missing: Vec<ChunkCoord> = registry
            .desired_chunks
            .iter()
            .copied()
            .filter(|coord| !registry.loaded.contains_key(coord))
            .collect();

        let center = registry
            .centers
            .first()
            .copied()
            .unwrap_or(ChunkCoord::new(0, 0));
        missing.sort_by_key(|coord| (coord.x - center.x).abs().max((coord.z - center.z).abs()));
        registry.pending_load.extend(missing);
    }

    if terrain_changed && !full_rebuild_changed && !resolution_changed {
        let dirty_loaded: Vec<ChunkCoord> = registry
            .loaded
            .keys()
            .copied()
            .filter(|coord| {
                registry.loaded_versions.get(coord).copied().unwrap_or(0)
                    != terrain.chunk_modification_version(*coord)
            })
            .collect();

        for coord in dirty_loaded {
            if let Some(entity) = registry.loaded.remove(&coord) {
                commands.entity(entity).despawn();
            }
            registry.loaded_versions.remove(&coord);
            if registry.desired_chunks.contains(&coord) && !registry.pending_load.contains(&coord) {
                // A loaded collider that just became stale is more urgent than
                // filling the outer edge of the streaming radius.
                registry.pending_load.push_front(coord);
            }
        }
    }

    registry.terrain_version = terrain_version;

    for _ in 0..settings.max_load_per_tick {
        let Some(coord) = registry.pending_load.pop_front() else {
            break;
        };
        if registry.loaded.contains_key(&coord) || !registry.desired_chunks.contains(&coord) {
            continue;
        }
        let entity = spawn_terrain_chunk_collider(
            &mut commands,
            &terrain,
            coord,
            settings.heightfield_resolution,
        );
        registry.loaded.insert(coord, entity);
        registry
            .loaded_versions
            .insert(coord, terrain.chunk_modification_version(coord));
    }
}
