//! Collider streaming systems.

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use shared::building::{
    BuildZoneEntry, FarmFieldClaimChanges, FarmFieldClaimIndex, point_in_any_build_zone_entries,
};
use shared::components::PlayerPosition;
use shared::terrain::{ChunkCoord, WorldTerrain};

use crate::collision::building_index::BuildingSpatialIndex;
use crate::collision::library::{DerivedColliderLibrary, StaticColliderInstance, StaticColliders};

/// Static-collider reach around authoritative actors and buildings.
const COLLIDER_VIEW_DISTANCE_CHUNKS: i32 = 3;

/// Limit how many chunks we load per fixed tick.
const MAX_COLLIDER_CHUNKS_TO_LOAD_PER_TICK: usize = 6;

/// Spatial hash cell size in meters.
use crate::collision::library::COLLIDER_CELL_SIZE;

/// Stateful cache used by static-collider streaming to avoid full recomputation each tick.
#[derive(Resource, Default)]
pub struct ColliderStreamingState {
    map_bounds: Option<shared::map::MapBounds>,
    last_player_centers: HashMap<ChunkCoord, i32>,
    center_scratch: HashMap<ChunkCoord, i32>,
    desired_chunks: HashSet<ChunkCoord>,
    cached_building_version: u64,
    field_claims: FarmFieldClaimIndex,
    zones_by_chunk: HashMap<ChunkCoord, Vec<BuildZoneEntry>>,
}

impl ColliderStreamingState {
    fn update_centers(
        &mut self,
        centers: impl Iterator<Item = (ChunkCoord, i32)>,
        bounds: shared::map::MapBounds,
    ) {
        self.center_scratch.clear();
        // Do not reserve for the iterator's entity count: a crowded settlement
        // can contribute thousands of positions but only a handful of centers.
        for (center, radius) in centers {
            self.center_scratch
                .entry(center)
                .and_modify(|current| *current = (*current).max(radius))
                .or_insert(radius);
        }
        if self.center_scratch == self.last_player_centers
            && self
                .map_bounds
                .is_some_and(|old| old.min == bounds.min && old.max == bounds.max)
        {
            return;
        }
        self.map_bounds = Some(bounds);
        std::mem::swap(&mut self.last_player_centers, &mut self.center_scratch);
        self.desired_chunks.clear();
        // PlayerPosition also belongs to world actors and buildings. Many share
        // a chunk; expand each distinct center once, retaining the same coverage.
        for (center, radius) in &self.last_player_centers {
            for dx in -*radius..=*radius {
                for dz in -*radius..=*radius {
                    let chunk = ChunkCoord::new(center.x + dx, center.z + dz);
                    if chunk.in_map_bounds(bounds) {
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

/// Stream colliders around authoritative bodies, never around a camera.
/// Wild horses need one neighboring chunk for their five-meter grazing range;
/// workers, buildings and ridden mounts retain the wider navigation footprint.
pub fn update_static_collider_streaming(
    terrain: Res<WorldTerrain>,
    library: Option<Res<DerivedColliderLibrary>>,
    building_index: Res<BuildingSpatialIndex>,
    players: Query<
        (&PlayerPosition, Option<&shared::components::Horse>),
        Without<shared::components::Player>,
    >,
    roads: Query<&shared::components::VillageRoad>,
    mut colliders: ResMut<StaticColliders>,
    mut state: ResMut<ColliderStreamingState>,
    mut field_changes: FarmFieldClaimChanges,
) {
    let Some(library) = library else { return };
    let field_version = state.field_claims.version();
    let mut zone_chunks_to_refresh = field_changes.sync(&mut state.field_claims);

    if state.cached_building_version != building_index.version
        || field_version != state.field_claims.version()
    {
        let buildings: Vec<(Vec3, shared::building::BuildingType, f32)> = building_index
            .snapshot()
            .iter()
            .map(|entry| (entry.position, entry.building_type, entry.rotation))
            .collect();
        let new_zones_by_chunk = state.field_claims.building_zones_by_chunk(&buildings);
        zone_chunks_to_refresh.extend(changed_zone_chunks(
            &state.zones_by_chunk,
            &new_zones_by_chunk,
        ));
        state.zones_by_chunk = new_zones_by_chunk;
        state.cached_building_version = building_index.version;
    }
    zone_chunks_to_refresh.sort_unstable_by_key(|c| (c.x, c.z));
    zone_chunks_to_refresh.dedup();

    state.update_centers(
        players.iter().map(|(position, horse)| {
            let radius = if horse.is_some_and(|horse| horse.rider.is_none()) {
                1
            } else {
                COLLIDER_VIEW_DISTANCE_CHUNKS
            };
            (ChunkCoord::from_world_pos(position.0), radius)
        }),
        terrain.generator.active_map_bounds(),
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
        // Re-enter through the ordinary six-chunk budget below. A batch of
        // accepted farm edits must not regenerate every nearby prop recipe in
        // one fixed tick. Cleared trees disappear from collision immediately.
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
            .keys()
            .map(|p| (chunk.x - p.x).abs().max((chunk.z - p.z).abs()))
            .min()
            .unwrap_or(COLLIDER_VIEW_DISTANCE_CHUNKS)
            .clamp(0, COLLIDER_VIEW_DISTANCE_CHUNKS) as usize;
        buckets[dist].push(chunk);
    }

    let mut loaded_this_tick = 0usize;
    for mut bucket in buckets {
        // Stable budget order prevents hash iteration from choosing which body
        // can move first during startup or a footprint expansion.
        bucket.sort_unstable_by_key(|chunk| (chunk.x, chunk.z));
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
                Some(&state.field_claims),
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
    fields: Option<&FarmFieldClaimIndex>,
    roads: impl Iterator<Item = &'a shared::components::VillageRoad> + Clone,
) {
    let spawns = shared::props::generate_chunk_prop_spawns(&terrain.generator, chunk);

    let mut ids = Vec::new();
    for spawn in spawns {
        let Some(kind) = spawn.kind else {
            continue;
        };
        if fields.is_some_and(|fields| fields.contains_point(spawn.position.xz())) {
            continue;
        }
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
        if chunk.in_map_bounds(terrain.generator.active_map_bounds()) {
            load_chunk(
                terrain,
                library,
                &mut colliders,
                chunk,
                None,
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
        let bounds = shared::map::MapBounds {
            min: [-128.0; 2],
            max: [128.0; 2],
        };
        let centers = [
            ChunkCoord::new(0, 0),
            ChunkCoord::new(2, -1),
            ChunkCoord::new(64, 64),
        ];
        let positions: Vec<_> = centers.into_iter().cycle().take(5_000).collect();
        let expected: HashSet<_> = positions
            .iter()
            .flat_map(|center| center.chunks_in_radius(COLLIDER_VIEW_DISTANCE_CHUNKS))
            .filter(|chunk| chunk.in_map_bounds(bounds))
            .collect();
        let mut state = ColliderStreamingState::default();
        state.update_centers(
            positions
                .iter()
                .copied()
                .map(|c| (c, COLLIDER_VIEW_DISTANCE_CHUNKS)),
            bounds,
        );
        assert_eq!(state.last_player_centers.len(), centers.len());
        assert_eq!(state.desired_chunks, expected);

        // Population and ordering can change without altering spatial coverage.
        state.update_centers(
            centers
                .into_iter()
                .rev()
                .map(|c| (c, COLLIDER_VIEW_DISTANCE_CHUNKS)),
            bounds,
        );
        assert_eq!(state.desired_chunks, expected);
        state.update_centers(std::iter::empty(), bounds);
        assert!(state.desired_chunks.is_empty());
    }

    #[test]
    fn stationary_centers_reclip_when_the_owned_map_bounds_change() {
        let mut state = ColliderStreamingState::default();
        let center = ChunkCoord::new(0, 0);
        let wide = shared::map::MapBounds {
            min: [-256.0; 2],
            max: [256.0; 2],
        };
        let tight = shared::map::MapBounds {
            min: [0.0; 2],
            max: [shared::terrain::CHUNK_SIZE; 2],
        };
        state.update_centers(
            std::iter::once((center, COLLIDER_VIEW_DISTANCE_CHUNKS)),
            wide,
        );
        assert_eq!(state.desired_chunks.len(), 49);
        state.update_centers(
            std::iter::once((center, COLLIDER_VIEW_DISTANCE_CHUNKS)),
            tight,
        );
        assert_eq!(state.desired_chunks, HashSet::from([center]));
        state.update_centers(
            std::iter::once((center, COLLIDER_VIEW_DISTANCE_CHUNKS)),
            wide,
        );
        assert_eq!(state.desired_chunks.len(), 49);
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

        // Crop geometry has its own invalidation source: no building marker
        // changes when a parcel is accepted, clipped, moved or removed.
        let field_at = Vec3::new(24.0, 0.0, 24.0);
        let field = app
            .world_mut()
            .spawn((
                shared::components::FarmField {
                    settlement: "Test".into(),
                    farmstead: field_at,
                    plot_index: 0,
                    quality: 1.0,
                    layout_version: 0,
                    shape: None,
                },
                PlayerPosition(field_at),
                shared::components::PlayerRotation(0.0),
            ))
            .id();
        app.update();
        let field_revision = app.world().resource::<StaticColliders>().chunk_versions[&origin];
        assert!(
            app.world()
                .resource::<ColliderStreamingState>()
                .field_claims
                .contains_point(field_at.xz())
        );
        app.world_mut()
            .entity_mut(field)
            .get_mut::<shared::components::FarmField>()
            .unwrap()
            .quality = 0.4;
        app.update();
        assert_eq!(
            app.world().resource::<StaticColliders>().chunk_versions[&origin],
            field_revision
        );
        app.world_mut()
            .entity_mut(field)
            .get_mut::<shared::components::FarmField>()
            .unwrap()
            .shape = Some(shared::components::FarmFieldShape::default());
        app.update();
        assert!(app.world().resource::<StaticColliders>().chunk_versions[&origin] > field_revision);
        assert!(
            !app.world()
                .resource::<ColliderStreamingState>()
                .field_claims
                .contains_point(field_at.xz())
        );
        app.world_mut().despawn(field);
        app.update();
        assert!(
            !app.world()
                .resource::<ColliderStreamingState>()
                .field_claims
                .has_farm(field_at)
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
