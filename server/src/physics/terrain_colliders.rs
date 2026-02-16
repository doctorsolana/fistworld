//! Authoritative terrain colliders (chunked heightfields).

use bevy::prelude::*;
use bevy_rapier3d::prelude::{Collider, CollisionGroups, RigidBody};
use std::collections::{HashMap, HashSet};

use shared::components::{Npc, NpcPosition, Player, PlayerPosition};
use shared::terrain::{ChunkCoord, WorldTerrain, CHUNK_SIZE};
use shared::vehicle::{Vehicle, VehicleState};

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
    pub terrain_version: u32,
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
            CollisionGroups::new(layers::GROUP_TERRAIN, layers::dynamic_actor_mask()),
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
    vehicles: &Query<&VehicleState, With<Vehicle>>,
    npcs: &Query<&NpcPosition, With<Npc>>,
) -> Vec<ChunkCoord> {
    let mut centers = Vec::new();

    for pos in players.iter() {
        centers.push(ChunkCoord::from_world_pos(pos.0));
    }

    if centers.is_empty() {
        for vehicle in vehicles.iter() {
            centers.push(ChunkCoord::from_world_pos(vehicle.position));
        }
    }

    if centers.is_empty() {
        for pos in npcs.iter().take(8) {
            centers.push(ChunkCoord::from_world_pos(pos.0));
        }
    }

    if centers.is_empty() {
        centers.push(ChunkCoord::new(0, 0));
    }

    centers
}

pub fn sync_terrain_colliders(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    settings: Res<TerrainColliderSettings>,
    mut registry: ResMut<TerrainColliderRegistry>,
    players: Query<&PlayerPosition, With<Player>>,
    vehicles: Query<&VehicleState, With<Vehicle>>,
    npcs: Query<&NpcPosition, With<Npc>>,
) {
    let terrain_version = terrain.modification_version();
    if registry.terrain_version != terrain_version {
        let stale: Vec<Entity> = registry.loaded.values().copied().collect();
        for entity in stale {
            commands.entity(entity).despawn();
        }
        registry.loaded.clear();
        registry.terrain_version = terrain_version;
    }

    let centers = gather_centers(&players, &vehicles, &npcs);
    let desired = desired_chunks_for_centers(&centers, settings.radius_chunks);

    let stale_chunks: Vec<ChunkCoord> = registry
        .loaded
        .keys()
        .copied()
        .filter(|coord| !desired.contains(coord))
        .collect();
    for coord in stale_chunks {
        if let Some(entity) = registry.loaded.remove(&coord) {
            commands.entity(entity).despawn();
        }
    }

    let mut missing: Vec<ChunkCoord> = desired
        .iter()
        .copied()
        .filter(|coord| !registry.loaded.contains_key(coord))
        .collect();

    let center = centers.first().copied().unwrap_or(ChunkCoord::new(0, 0));
    missing.sort_by_key(|coord| (coord.x - center.x).abs().max((coord.z - center.z).abs()));

    for coord in missing.into_iter().take(settings.max_load_per_tick) {
        let entity = spawn_terrain_chunk_collider(
            &mut commands,
            &terrain,
            coord,
            settings.heightfield_resolution,
        );
        registry.loaded.insert(coord, entity);
    }
}
