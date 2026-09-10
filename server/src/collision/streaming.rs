//! Collider streaming systems.

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use shared::building::{build_zones_by_chunk, point_in_any_build_zone_entries, BuildZoneEntry};
use shared::components::PlayerPosition;
use shared::terrain::{ChunkCoord, WorldTerrain};

use crate::collision::building_index::BuildingSpatialIndex;
use crate::collision::library::{DerivedColliderLibrary, StaticColliderInstance, StaticColliders};

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
    center_scratch: HashSet<ChunkCoord>,
    desired_chunks: HashSet<ChunkCoord>,
    cached_building_version: u64,
    zones_by_chunk: HashMap<ChunkCoord, Vec<BuildZoneEntry>>,
}

impl ColliderStreamingState {
    fn update_centers(&mut self, centers: impl Iterator<Item = ChunkCoord>) {
        self.center_scratch.clear();
        // Do not reserve for the iterator's entity count: a crowded settlement
        // can contribute thousands of positions but only a handful of centers.
        for center in centers {
            self.center_scratch.insert(center);
        }
        if self.center_scratch == self.last_player_centers {
            return;
        }
        std::mem::swap(&mut self.last_player_centers, &mut self.center_scratch);
        self.desired_chunks.clear();
        // PlayerPosition also belongs to world actors and buildings. Many share
        // a chunk; expand each distinct center once, retaining the same coverage.
        for center in &self.last_player_centers {
            for dx in -COLLIDER_VIEW_DISTANCE_CHUNKS..=COLLIDER_VIEW_DISTANCE_CHUNKS {
                for dz in -COLLIDER_VIEW_DISTANCE_CHUNKS..=COLLIDER_VIEW_DISTANCE_CHUNKS {
                    let chunk = ChunkCoord::new(center.x + dx, center.z + dz);
                    if chunk.in_world_bounds() {
                        self.desired_chunks.insert(chunk);
                    }
                }
            }
        }
    }
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

fn bump_chunk_version(colliders: &mut StaticColliders, chunk: ChunkCoord) {
    colliders.next_chunk_version = colliders.next_chunk_version.wrapping_add(1).max(1);
    colliders
        .chunk_versions
        .insert(chunk, colliders.next_chunk_version);
}

/// Stream in/out static colliders based on player positions.
pub fn update_static_collider_streaming(
    terrain: Res<WorldTerrain>,
    library: Option<Res<DerivedColliderLibrary>>,
    building_index: Res<BuildingSpatialIndex>,
    players: Query<(
        &PlayerPosition,
        Option<&shared::components::Horse>,
        Has<crate::world::wildlife::ActiveWildHorse>,
    )>,
    roads: Query<&shared::components::VillageRoad>,
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

    // Distant wildlife is a cheap record, not a request to load terrain/prop
    // colliders across the whole map. Ridden horses remain physical actors.
    state.update_centers(
        players
            .iter()
            .filter(|(_, horse, active)| horse.is_none_or(|h| h.rider.is_some() || *active))
            .map(|(pos, _, _)| ChunkCoord::from_world_pos(pos.0)),
    );

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
                roads.iter(),
            );
        }
    }

    // Unloading above leaves loaded_chunks a subset of desired_chunks. Building
    // refreshes must run first, even when streaming has otherwise settled.
    if colliders.loaded_chunks.len() == state.desired_chunks.len() {
        return;
    }

    // Bucket by nearest player chunk (Chebyshev distance) to avoid full vector sort.
    let mut buckets: [Vec<ChunkCoord>; COLLIDER_VIEW_DISTANCE_CHUNKS as usize + 1] =
        std::array::from_fn(|_| Vec::new());
    for &chunk in state.desired_chunks.difference(&colliders.loaded_chunks) {
        let dist = state
            .last_player_centers
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
                roads.iter(),
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
            bump_chunk_version(colliders, chunk);
        }
        return;
    };
    for id in ids {
        if let Some(inst) = colliders.instances.remove(&id) {
            if let Some(cell_list) = colliders.cells.get_mut(&inst.cell) {
                cell_list.retain(|x| *x != id);
                if cell_list.is_empty() {
                    colliders.cells.remove(&inst.cell);
                }
            }
        }
    }
    colliders.version = colliders.version.wrapping_add(1);
    bump_chunk_version(colliders, chunk);
}

fn load_chunk<'a>(
    terrain: &WorldTerrain,
    library: &DerivedColliderLibrary,
    colliders: &mut StaticColliders,
    chunk: ChunkCoord,
    chunk_zones: Option<&[BuildZoneEntry]>,
    roads: impl Iterator<Item = &'a shared::components::VillageRoad> + Clone,
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
        if kind.is_road_clearable() {
            let point = Vec2::new(spawn.position.x, spawn.position.z);
            if colliders.road_tree_was_cleared(point)
                || roads.clone().any(|road| {
                    road.contains_built_point(point, shared::components::ROAD_CLEARED_TREE_PADDING)
                })
            {
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
            scale: spawn.scale,
            rotation: spawn.rotation,
            cell,
        };

        colliders.instances.insert(id, inst);
        colliders.cells.entry(cell).or_default().push(id);
        ids.push(id);
    }

    colliders.loaded_chunks.insert(chunk);
    colliders.chunk_instances.insert(chunk, ids);
    colliders.version = colliders.version.wrapping_add(1);
    bump_chunk_version(colliders, chunk);
}

/// One-time founding survey, using the same prop instances and collision
/// radii as runtime streaming. It is independent of whether a player happens
/// to observe the candidate site during world creation.
pub(crate) fn survey_settlement_props(
    terrain: &WorldTerrain,
    library: &DerivedColliderLibrary,
    center: Vec3,
) -> StaticColliders {
    let mut colliders = StaticColliders::default();
    for chunk in ChunkCoord::from_world_pos(center).chunks_in_radius(6) {
        if chunk.in_world_bounds() {
            load_chunk(
                terrain,
                library,
                &mut colliders,
                chunk,
                None,
                std::iter::empty(),
            );
        }
    }
    colliders
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::building::BuildingType;

    #[test]
    fn duplicate_centers_preserve_coverage_and_world_bounds() {
        let centers = [
            ChunkCoord::new(0, 0),
            ChunkCoord::new(2, -1),
            ChunkCoord::new(64, 64),
        ];
        let positions: Vec<_> = centers.into_iter().cycle().take(5_000).collect();
        let expected: HashSet<_> = positions
            .iter()
            .flat_map(|center| center.chunks_in_radius(COLLIDER_VIEW_DISTANCE_CHUNKS))
            .filter(ChunkCoord::in_world_bounds)
            .collect();
        let mut state = ColliderStreamingState::default();
        state.update_centers(positions.iter().copied());
        assert_eq!(state.last_player_centers.len(), centers.len());
        assert_eq!(state.desired_chunks, expected);

        // Population and ordering can change without altering spatial coverage.
        state.update_centers(centers.into_iter().rev());
        assert_eq!(state.desired_chunks, expected);
        state.update_centers(std::iter::empty());
        assert!(state.desired_chunks.is_empty());
    }

    #[test]
    fn streaming_preserves_budget_and_refreshes_build_zones_after_settling() {
        use crate::collision::building_index::sync_building_spatial_index;
        use shared::building::{BuildingPosition, PlacedBuilding};

        let mut app = App::new();
        app.init_resource::<WorldTerrain>()
            .init_resource::<BuildingSpatialIndex>()
            .init_resource::<StaticColliders>()
            .init_resource::<ColliderStreamingState>()
            .insert_resource(DerivedColliderLibrary {
                by_kind: HashMap::new(),
            })
            .add_systems(
                Update,
                (
                    sync_building_spatial_index,
                    update_static_collider_streaming,
                )
                    .chain(),
            );
        let anchor = app.world_mut().spawn(PlayerPosition(Vec3::ZERO)).id();
        let origin = ChunkCoord::new(0, 0);
        app.update();
        let colliders = app.world().resource::<StaticColliders>();
        assert_eq!(
            colliders.loaded_chunks.len(),
            MAX_COLLIDER_CHUNKS_TO_LOAD_PER_TICK
        );
        assert!(
            colliders.loaded_chunks.contains(&origin),
            "nearest chunk loads first"
        );
        for _ in 0..8 {
            let before = app
                .world()
                .resource::<StaticColliders>()
                .loaded_chunks
                .len();
            app.update();
            let after = app
                .world()
                .resource::<StaticColliders>()
                .loaded_chunks
                .len();
            assert!(after - before <= MAX_COLLIDER_CHUNKS_TO_LOAD_PER_TICK);
        }
        assert_eq!(
            app.world()
                .resource::<StaticColliders>()
                .loaded_chunks
                .len(),
            49
        );
        let settled_version = app.world().resource::<StaticColliders>().version;
        app.update();
        assert_eq!(
            app.world().resource::<StaticColliders>().version,
            settled_version
        );

        let before = app.world().resource::<StaticColliders>().chunk_versions[&origin];
        let building = app
            .world_mut()
            .spawn((
                PlacedBuilding {
                    building_type: BuildingType::MootHall,
                    rotation: 0.0,
                },
                BuildingPosition(Vec3::new(16.0, 0.0, 16.0)),
            ))
            .id();
        app.update();
        let after = app.world().resource::<StaticColliders>().chunk_versions[&origin];
        assert!(
            after > before,
            "settled streaming must still refresh new build zones"
        );
        app.world_mut().despawn(building);
        app.update();
        assert!(app.world().resource::<StaticColliders>().chunk_versions[&origin] > after);

        let destination = ChunkCoord::new(10, 0);
        app.world_mut()
            .entity_mut(anchor)
            .insert(PlayerPosition(destination.world_pos()));
        app.update();
        let colliders = app.world().resource::<StaticColliders>();
        assert_eq!(
            colliders.loaded_chunks.len(),
            MAX_COLLIDER_CHUNKS_TO_LOAD_PER_TICK
        );
        assert!(colliders.loaded_chunks.contains(&destination));
        assert!(!colliders.loaded_chunks.contains(&origin));
        app.world_mut().despawn(anchor);
        app.update();
        let colliders = app.world().resource::<StaticColliders>();
        assert!(colliders.loaded_chunks.is_empty());
        assert!(colliders.chunk_instances.is_empty());
    }

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
                BuildingType::MootHall,
                0.0,
            )],
        );
        old.insert(
            chunk_b,
            vec![BuildZoneEntry::from_building(
                Vec3::new(68.0, 0.0, 4.0),
                BuildingType::Farmstead,
                0.0,
            )],
        );

        let mut new = HashMap::new();
        new.insert(chunk_b, old[&chunk_b].clone());
        new.insert(
            chunk_c,
            vec![BuildZoneEntry::from_building(
                Vec3::new(132.0, 0.0, 4.0),
                BuildingType::LogCabin,
                0.25,
            )],
        );

        let changed = changed_zone_chunks(&old, &new);
        assert_eq!(changed, vec![chunk_a, chunk_c]);
    }
}
