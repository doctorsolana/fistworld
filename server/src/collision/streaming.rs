//! Collider streaming systems.

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use shared::building::{build_zones_by_chunk, point_in_any_build_zone_entries, BuildZoneEntry};
use shared::components::PlayerPosition;
use shared::terrain::{ChunkCoord, WorldTerrain};

use crate::collision::building_index::BuildingSpatialIndex;
use crate::collision::library::{BakedColliderLibrary, StaticColliderInstance, StaticColliders};

/// How many chunks around each player we keep static colliders loaded for.
const COLLIDER_VIEW_DISTANCE_CHUNKS: i32 = 3;

/// Limit how many chunks we load per fixed tick.
const MAX_COLLIDER_CHUNKS_TO_LOAD_PER_TICK: usize = 6;

/// Spatial hash cell size in meters.
const COLLIDER_CELL_SIZE: f32 = 16.0;

/// Stateful cache used by static-collider streaming to avoid full recomputation each tick.
#[derive(Resource, Default)]
pub struct ColliderStreamingState {
    last_player_centers: HashSet<ChunkCoord>,
    desired_chunks: HashSet<ChunkCoord>,
    cached_building_version: u64,
    zones_by_chunk: HashMap<ChunkCoord, Vec<BuildZoneEntry>>,
}

fn changed_zone_chunks(
    old: &HashMap<ChunkCoord, Vec<BuildZoneEntry>>,
    new: &HashMap<ChunkCoord, Vec<BuildZoneEntry>>,
) -> Vec<ChunkCoord> {
    let mut changed: HashSet<ChunkCoord> = HashSet::with_capacity(old.len().max(new.len()));

    for (chunk, zones) in new {
        if old.get(chunk) != Some(zones) {
            changed.insert(*chunk);
        }
    }

    for chunk in old.keys() {
        if !new.contains_key(chunk) {
            changed.insert(*chunk);
        }
    }

    let mut changed: Vec<ChunkCoord> = changed.into_iter().collect();
    changed.sort_by_key(|coord| (coord.x, coord.z));
    changed
}

fn cell_key(x: f32, z: f32) -> (i32, i32) {
    (
        (x / COLLIDER_CELL_SIZE).floor() as i32,
        (z / COLLIDER_CELL_SIZE).floor() as i32,
    )
}

/// Stream in/out static colliders based on player positions.
pub fn update_static_collider_streaming(
    terrain: Res<WorldTerrain>,
    library: Option<Res<BakedColliderLibrary>>,
    building_index: Res<BuildingSpatialIndex>,
    players: Query<&PlayerPosition>,
    mut colliders: ResMut<StaticColliders>,
    mut state: ResMut<ColliderStreamingState>,
) {
    let Some(library) = library else { return };
    let mut zone_chunks_to_refresh = Vec::new();

    if state.cached_building_version != building_index.version {
        let buildings: Vec<(Vec3, shared::building::BuildingType, f32)> = building_index
            .snapshot()
            .iter()
            .map(|entry| (entry.position, entry.building_type, entry.rotation))
            .collect();
        let new_zones_by_chunk = build_zones_by_chunk(&buildings);
        zone_chunks_to_refresh = changed_zone_chunks(&state.zones_by_chunk, &new_zones_by_chunk);
        state.zones_by_chunk = new_zones_by_chunk;
        state.cached_building_version = building_index.version;
    }

    // Compute (or reuse) desired chunks as union around active player centers.
    let mut player_centers: Vec<ChunkCoord> = Vec::new();
    let mut player_center_set: HashSet<ChunkCoord> = HashSet::new();
    for pos in players.iter() {
        let center = ChunkCoord::from_world_pos(pos.0);
        player_center_set.insert(center);
        player_centers.push(center);
    }

    if player_center_set != state.last_player_centers {
        state.last_player_centers = player_center_set;
        state.desired_chunks.clear();
        for center in player_centers.iter() {
            state
                .desired_chunks
                .extend(center.chunks_in_radius(COLLIDER_VIEW_DISTANCE_CHUNKS));
        }
        state.desired_chunks.retain(|c| c.in_world_bounds());
    }

    // Unload chunks that are no longer desired.
    let to_unload: Vec<ChunkCoord> = colliders
        .loaded_chunks
        .difference(&state.desired_chunks)
        .copied()
        .collect();
    for chunk in to_unload {
        unload_chunk(&mut colliders, chunk);
    }

    for chunk in zone_chunks_to_refresh {
        if !colliders.loaded_chunks.contains(&chunk) {
            continue;
        }

        unload_chunk(&mut colliders, chunk);
        if state.desired_chunks.contains(&chunk) {
            load_chunk(
                &terrain,
                &library,
                &mut colliders,
                chunk,
                state.zones_by_chunk.get(&chunk).map(Vec::as_slice),
            );
        }
    }

    // Load new chunks.
    let to_load: Vec<ChunkCoord> = state
        .desired_chunks
        .difference(&colliders.loaded_chunks)
        .copied()
        .collect();

    // Bucket by nearest player chunk (Chebyshev distance) to avoid full vector sort.
    let max_bucket = (COLLIDER_VIEW_DISTANCE_CHUNKS + 1).max(1) as usize;
    let mut buckets: Vec<Vec<ChunkCoord>> = vec![Vec::new(); max_bucket];
    for chunk in to_load {
        let dist = player_centers
            .iter()
            .map(|p| (chunk.x - p.x).abs().max((chunk.z - p.z).abs()))
            .min()
            .unwrap_or(COLLIDER_VIEW_DISTANCE_CHUNKS)
            .clamp(0, COLLIDER_VIEW_DISTANCE_CHUNKS) as usize;
        buckets[dist].push(chunk);
    }

    let mut loaded_this_tick = 0usize;
    for bucket in buckets {
        for chunk in bucket {
            if loaded_this_tick >= MAX_COLLIDER_CHUNKS_TO_LOAD_PER_TICK {
                break;
            }
            load_chunk(
                &terrain,
                &library,
                &mut colliders,
                chunk,
                state.zones_by_chunk.get(&chunk).map(Vec::as_slice),
            );
            loaded_this_tick += 1;
        }
        if loaded_this_tick >= MAX_COLLIDER_CHUNKS_TO_LOAD_PER_TICK {
            break;
        }
    }
}

fn unload_chunk(colliders: &mut StaticColliders, chunk: ChunkCoord) {
    let removed = colliders.loaded_chunks.remove(&chunk);

    let Some(ids) = colliders.chunk_instances.remove(&chunk) else {
        if removed {
            colliders.version = colliders.version.wrapping_add(1);
        }
        return;
    };
    for id in ids {
        if let Some(inst) = colliders.instances.remove(&id) {
            colliders.pending_removed.push(id);
            if let Some(cell_list) = colliders.cells.get_mut(&inst.cell) {
                cell_list.retain(|x| *x != id);
                if cell_list.is_empty() {
                    colliders.cells.remove(&inst.cell);
                }
            }
        }
    }
    colliders.version = colliders.version.wrapping_add(1);
}

fn load_chunk(
    terrain: &WorldTerrain,
    library: &BakedColliderLibrary,
    colliders: &mut StaticColliders,
    chunk: ChunkCoord,
    chunk_zones: Option<&[BuildZoneEntry]>,
) {
    let spawns = shared::props::generate_chunk_prop_spawns(&terrain.generator, chunk);

    let mut ids = Vec::new();
    for spawn in spawns {
        let Some(kind) = spawn.kind else {
            continue;
        };
        if let Some(zones) = chunk_zones {
            let point_xz = Vec2::new(spawn.position.x, spawn.position.z);
            if point_in_any_build_zone_entries(point_xz, zones) {
                continue;
            }
        }

        if !library.by_kind.contains_key(&kind) {
            continue;
        }

        let id = colliders.next_id;
        colliders.next_id = colliders.next_id.wrapping_add(1);

        let cell = cell_key(spawn.position.x, spawn.position.z);
        let inst = StaticColliderInstance {
            kind,
            position: spawn.position,
            rotation: spawn.rotation,
            scale: spawn.scale,
            cell,
        };

        colliders.instances.insert(id, inst);
        colliders.pending_added.push_back(id);
        colliders.cells.entry(cell).or_default().push(id);
        ids.push(id);
    }

    colliders.loaded_chunks.insert(chunk);
    colliders.chunk_instances.insert(chunk, ids);
    colliders.version = colliders.version.wrapping_add(1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::building::BuildingType;

    #[test]
    fn changed_zone_chunks_detects_add_remove_and_updates() {
        let mut old = HashMap::new();
        let chunk_a = ChunkCoord::new(0, 0);
        let chunk_b = ChunkCoord::new(1, 0);
        let chunk_c = ChunkCoord::new(2, 0);

        old.insert(
            chunk_a,
            vec![BuildZoneEntry::from_building(
                Vec3::new(4.0, 0.0, 4.0),
                BuildingType::Windmill,
                0.0,
            )],
        );
        old.insert(
            chunk_b,
            vec![BuildZoneEntry::from_building(
                Vec3::new(68.0, 0.0, 4.0),
                BuildingType::Church,
                0.0,
            )],
        );

        let mut new = HashMap::new();
        new.insert(chunk_b, old[&chunk_b].clone());
        new.insert(
            chunk_c,
            vec![BuildZoneEntry::from_building(
                Vec3::new(132.0, 0.0, 4.0),
                BuildingType::House05,
                0.25,
            )],
        );

        let changed = changed_zone_chunks(&old, &new);
        assert_eq!(changed, vec![chunk_a, chunk_c]);
    }
}
