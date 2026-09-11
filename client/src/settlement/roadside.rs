//! Sparse, walkable shoulder dressing around the authoritative built roads.
//!
//! Road and plot changes update small spatial caches. Only nearby loaded chunks
//! receive geometry, at one chunk per frame, with one opaque material and one
//! mesh/entity per chunk. Decoration never changes roads, land or collision.

mod mesh;
mod placement;

use std::collections::{HashMap, HashSet};

use bevy::camera::visibility::VisibilityRange;
use bevy::ecs::system::SystemParam;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use shared::building::BuildZoneEntry;
use shared::components::{FarmField, HouseholdYard, PlayerPosition, PlayerRotation, VillageRoad};
use shared::terrain::{Biome, ChunkCoord, WorldTerrain};

use crate::props::BuildZoneChunkIndex;
use crate::render::systems::{ClientWorldRoot, GraphicsSettings};
use crate::states::GameState;
use crate::streaming::{
    camera_view_distance, chunk_stream_priority, streaming_anchor, streaming_view_priority,
    AnchorCamera, AnchorPlayer,
};
use crate::terrain::{LoadedChunks, TerrainUpdateSet};

use mesh::RoadsideMesh;
use placement::{
    candidates, meadow_candidates, Candidate, DetailKind, PlotFootprint, PlotShape, RoadSegment,
    MAX_CLUSTERS_PER_CHUNK,
};

const CHUNK_RADIUS: i32 = 5;
const ROAD_INDEX_UPDATES_PER_FRAME: usize = 4;
const MAX_TRIANGLES_PER_CHUNK: usize = 8_000;
const FADE_START: f32 = 420.0;
const FADE_END: f32 = 520.0;

pub(super) struct RoadsidePlugin;

impl Plugin for RoadsidePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RoadsideState>()
            .init_resource::<RoadsideReadiness>()
            .add_systems(
                Update,
                update_roadside
                    .after(TerrainUpdateSet)
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(OnExit(GameState::Playing), clear_roadside);
    }
}

/// Inspection counters describe the combined mesh, not independently simulated props.
#[derive(Component)]
pub(crate) struct RoadsideChunk {
    pub(crate) clusters: usize,
    pub(crate) triangles: usize,
}

/// Semantic capture readiness for the work visible at the current streaming anchor.
#[derive(Resource, Default, Clone, Copy, Debug)]
pub(crate) struct RoadsideReadiness {
    pub(crate) ready: bool,
    pub(crate) pending_road_updates: usize,
    pub(crate) pending_chunks: usize,
}

#[derive(Clone, PartialEq)]
struct RoadSnapshot {
    points: Vec<Vec2>,
    built_through: usize,
    width: f32,
    reserved_width: f32,
}

impl RoadSnapshot {
    fn from_road(road: &VillageRoad) -> Option<Self> {
        (road.width.is_finite()
            && road.width > 0.0
            && road.reservation_width().is_finite()
            && road.points.iter().all(|point| point.is_finite()))
        .then(|| Self {
            points: road.points.clone(),
            built_through: usize::from(road.built_through).min(road.points.len()),
            width: road.width,
            reserved_width: road.reservation_width(),
        })
    }

    fn segments(&self, owner: Entity) -> Vec<RoadSegment> {
        let mut distance_before = 0.0;
        self.points
            .windows(2)
            .enumerate()
            .map(|(index, points)| {
                let segment = RoadSegment {
                    owner,
                    start: points[0],
                    end: points[1],
                    distance_before,
                    width: self.width,
                    reserved_width: self.reserved_width,
                    built: index + 1 < self.built_through,
                };
                distance_before += points[0].distance(points[1]);
                segment
            })
            .collect()
    }
}

struct RoadRecord {
    snapshot: RoadSnapshot,
    chunks: HashSet<ChunkCoord>,
}

struct ChunkRecord {
    entity: Option<Entity>,
    mesh: Option<Handle<Mesh>>,
    ground_version: u64,
}

#[derive(Resource, Default)]
struct RoadsideState {
    roads: HashMap<Entity, RoadRecord>,
    pending_roads: HashMap<Entity, Option<RoadSnapshot>>,
    segments: HashMap<ChunkCoord, Vec<RoadSegment>>,
    plots: HashMap<Entity, PlotFootprint>,
    plots_by_chunk: HashMap<ChunkCoord, Vec<Entity>>,
    building_zones: HashMap<ChunkCoord, Vec<BuildZoneEntry>>,
    chunks: HashMap<ChunkCoord, ChunkRecord>,
    dirty: HashSet<ChunkCoord>,
    material: Option<Handle<StandardMaterial>>,
}

type ChangedField<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static FarmField,
        &'static PlayerPosition,
        &'static PlayerRotation,
    ),
    Or<(
        Changed<FarmField>,
        Changed<PlayerPosition>,
        Changed<PlayerRotation>,
    )>,
>;
type ChangedYard<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static HouseholdYard,
        &'static PlayerPosition,
        &'static PlayerRotation,
    ),
    Or<(
        Changed<HouseholdYard>,
        Changed<PlayerPosition>,
        Changed<PlayerRotation>,
    )>,
>;

#[derive(SystemParam)]
struct RoadsideInputs<'w, 's> {
    roads: Query<'w, 's, (Entity, &'static VillageRoad), Changed<VillageRoad>>,
    fields: ChangedField<'w, 's>,
    yards: ChangedYard<'w, 's>,
    removed_roads: RemovedComponents<'w, 's, VillageRoad>,
    removed_fields: RemovedComponents<'w, 's, FarmField>,
    removed_yards: RemovedComponents<'w, 's, HouseholdYard>,
    zones: Res<'w, BuildZoneChunkIndex>,
    terrain: Res<'w, WorldTerrain>,
    colliders: Option<Res<'w, crate::props::ClientDerivedColliderLibrary>>,
    loaded: Res<'w, LoadedChunks>,
    settings: Res<'w, GraphicsSettings>,
    players: AnchorPlayer<'w, 's>,
    cameras: AnchorCamera<'w, 's>,
    roots: Query<'w, 's, Entity, With<ClientWorldRoot>>,
}

impl RoadsideState {
    fn mark_dirty(&mut self, coord: ChunkCoord) {
        // An unseen/unloaded chunk will build from current caches when loaded.
        // Only resident records need invalidation; retaining the whole world's
        // unseen road cells would create an ever-growing per-frame dirty scan.
        if self.chunks.contains_key(&coord) {
            self.dirty.insert(coord);
        }
    }

    fn replace_plot(&mut self, entity: Entity, next: Option<PlotFootprint>) {
        if self.plots.get(&entity) == next.as_ref() {
            return;
        }
        if let Some(previous) = self.plots.remove(&entity) {
            for coord in previous.chunks() {
                if let Some(entries) = self.plots_by_chunk.get_mut(&coord) {
                    entries.retain(|entry| *entry != entity);
                    if entries.is_empty() {
                        self.plots_by_chunk.remove(&coord);
                    }
                }
                self.mark_dirty(coord);
            }
        }
        if let Some(next) = next {
            for coord in next.chunks() {
                self.plots_by_chunk.entry(coord).or_default().push(entity);
                self.mark_dirty(coord);
            }
            self.plots.insert(entity, next);
        }
    }

    fn apply_road_updates(&mut self) {
        let mut pending = self.pending_roads.keys().copied().collect::<Vec<_>>();
        pending.sort_unstable_by_key(|entity| entity.to_bits());
        pending.truncate(ROAD_INDEX_UPDATES_PER_FRAME);
        for entity in pending {
            let next = self.pending_roads.remove(&entity).flatten();
            if self.roads.get(&entity).map(|record| &record.snapshot) == next.as_ref() {
                continue;
            }
            if let Some(previous) = self.roads.remove(&entity) {
                for coord in previous.chunks {
                    if let Some(entries) = self.segments.get_mut(&coord) {
                        entries.retain(|segment| segment.owner != entity);
                        if entries.is_empty() {
                            self.segments.remove(&coord);
                        }
                    }
                    self.mark_dirty(coord);
                }
            }
            if let Some(snapshot) = next {
                let mut chunks = HashSet::new();
                for segment in snapshot.segments(entity) {
                    for coord in segment.chunks() {
                        self.segments.entry(coord).or_default().push(segment);
                        self.mark_dirty(coord);
                        chunks.insert(coord);
                    }
                }
                self.roads.insert(entity, RoadRecord { snapshot, chunks });
            }
        }
    }

    fn clear_chunk(
        &mut self,
        coord: ChunkCoord,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) {
        if let Some(record) = self.chunks.remove(&coord) {
            if let Some(entity) = record.entity {
                commands.entity(entity).try_despawn();
            }
            if let Some(mesh) = record.mesh {
                meshes.remove(mesh.id());
            }
        }
        self.dirty.remove(&coord);
    }

    fn candidate_is_clear(&self, coord: ChunkCoord, candidate: &Candidate) -> bool {
        if self
            .segments
            .get(&coord)
            .is_some_and(|segments| segments.iter().any(|road| !candidate.clear_of_road(road)))
        {
            return false;
        }
        if self.plots_by_chunk.get(&coord).is_some_and(|plots| {
            plots.iter().any(|entity| {
                self.plots
                    .get(entity)
                    .is_some_and(|plot| plot.contains(candidate.point, candidate.radius))
            })
        }) {
            return false;
        }
        // A candidate can overlap a plot just over a chunk edge. Inflated exact
        // rotated footprints, including civic squares and field reservations,
        // are checked in the neighbouring index cells rather than by a full scan.
        for dx in -1..=1 {
            for dz in -1..=1 {
                let neighbour = ChunkCoord::new(coord.x + dx, coord.z + dz);
                if self.building_zones.get(&neighbour).is_some_and(|zones| {
                    zones.iter().any(|zone| {
                        let mut inflated = *zone;
                        inflated.half_extents += Vec2::splat(candidate.radius + 0.2);
                        inflated.contains_point(candidate.point)
                    })
                }) {
                    return false;
                }
            }
        }
        true
    }
}

fn ground_version(coord: ChunkCoord, terrain: &WorldTerrain) -> u64 {
    // Bounds are chunk-owned, but a tuft at an edge may sample the next height tile.
    let mut version = terrain.full_rebuild_version() as u64;
    for dx in -1..=1 {
        for dz in -1..=1 {
            version = version.wrapping_mul(0x9e3779b97f4a7c15)
                ^ terrain.chunk_modification_version(ChunkCoord::new(coord.x + dx, coord.z + dz))
                    as u64;
        }
    }
    version
}

fn ground_suits(candidate: &Candidate, terrain: &WorldTerrain) -> bool {
    if candidate.kind != DetailKind::Stone
        && !matches!(
            terrain.get_biome(candidate.point.x, candidate.point.y),
            Biome::Grasslands | Biome::Natureland
        )
    {
        return false;
    }
    ground_profile_suits(candidate, |point| {
        terrain
            .get_water_height(point.x, point.y)
            .is_none()
            .then(|| terrain.get_height(point.x, point.y))
    })
}

fn ground_profile_suits(
    candidate: &Candidate,
    mut ground: impl FnMut(Vec2) -> Option<f32>,
) -> bool {
    let mut low = f32::INFINITY;
    let mut high = f32::NEG_INFINITY;
    for offset in [Vec2::ZERO, Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y] {
        let p = candidate.point + offset * candidate.radius;
        let Some(height) = ground(p).filter(|height| height.is_finite()) else {
            return false;
        };
        low = low.min(height);
        high = high.max(height);
    }
    // Stems/clumps and bedded stone vertices follow their own ground heights.
    // Stones also limit grade relative to their diameter, so a small group
    // cannot inherit the full height allowance on an abrupt verge step.
    high - low
        <= if candidate.kind == DetailKind::Stone {
            0.18_f32.min(candidate.radius * 2.0 * 0.18)
        } else {
            0.45
        }
}

fn build_chunk(
    coord: ChunkCoord,
    state: &RoadsideState,
    terrain: &WorldTerrain,
    colliders: Option<&crate::props::ClientDerivedColliderLibrary>,
) -> (RoadsideMesh, usize) {
    let mut mesh = RoadsideMesh::default();
    let Some(segments) = state.segments.get(&coord) else {
        return (mesh, 0);
    };
    let mut accepted = Vec::<Candidate>::new();
    // Sample the immutable prop recipe once for this bounded chunk rebuild.
    // Current streamed entities are deliberately not the source: decoration
    // must not change when a neighbouring tree finishes loading. Use canonical
    // baked trunk/rock radii, leaving foliage free to grow beneath canopies.
    let mut obstacles = Vec::new();
    let chunk_min = coord.world_pos().xz();
    let chunk_max = chunk_min + Vec2::splat(shared::terrain::CHUNK_SIZE);
    for x in coord.x - 1..=coord.x + 1 {
        for z in coord.z - 1..=coord.z + 1 {
            for prop in shared::props::generate_chunk_blocking_props(
                &terrain.generator,
                ChunkCoord::new(x, z),
            ) {
                let radius = colliders
                    .and_then(|library| library.by_kind.get(&prop.kind))
                    .map_or(1.5, |collider| collider.bounding_radius)
                    * prop.scale;
                if prop
                    .position
                    .distance_squared(prop.position.clamp(chunk_min, chunk_max))
                    <= (radius + 1.9).powi(2)
                {
                    obstacles.push((prop.position, radius));
                }
            }
        }
    }
    let world_seed = terrain
        .generator
        .loaded_map()
        .definition
        .generated
        .as_ref()
        .map_or(0, |g| g.seed);
    let mut choices = candidates(coord, segments);
    let remaining = placement::MAX_CANDIDATES_PER_CHUNK.saturating_sub(choices.len());
    choices.extend(
        meadow_candidates(coord, segments, world_seed)
            .into_iter()
            .take(remaining),
    );
    for candidate in choices {
        if accepted.iter().any(|other| other.seed == candidate.seed) {
            continue;
        }
        if !state.candidate_is_clear(coord, &candidate) || !ground_suits(&candidate, terrain) {
            continue;
        }
        if obstacles.iter().any(|(center, radius)| {
            center.distance_squared(candidate.point) < (radius + candidate.radius + 0.15).powi(2)
        }) {
            continue;
        }
        // This list is capped at 64. A tiny bounded comparison avoids allocating
        // a second spatial index for a one-off group assembly.
        if accepted.iter().any(|other| {
            other.point.distance_squared(candidate.point)
                < (other.radius + candidate.radius + 0.20).powi(2)
        }) {
            continue;
        }
        if mesh.triangles() + RoadsideMesh::triangle_bound(candidate.kind) > MAX_TRIANGLES_PER_CHUNK
        {
            continue;
        }
        mesh.append(&candidate, terrain, coord.world_pos());
        accepted.push(candidate);
        if accepted.len() == MAX_CLUSTERS_PER_CHUNK {
            break;
        }
    }
    (mesh, accepted.len())
}

fn update_roadside(
    mut commands: Commands,
    mut inputs: RoadsideInputs,
    mut state: ResMut<RoadsideState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut readiness: ResMut<RoadsideReadiness>,
) {
    // A removed-and-reinserted component in one tick is represented by its
    // current query value; a stale removal event must not erase the live road.
    for entity in inputs.removed_roads.read() {
        state.pending_roads.insert(entity, None);
    }
    for (entity, road) in &inputs.roads {
        state
            .pending_roads
            .insert(entity, RoadSnapshot::from_road(road));
    }
    state.apply_road_updates();
    for entity in inputs
        .removed_fields
        .read()
        .chain(inputs.removed_yards.read())
    {
        state.replace_plot(entity, None);
    }
    for (entity, field, position, rotation) in &inputs.fields {
        state.replace_plot(
            entity,
            Some(PlotFootprint::from_field(field, position.0, rotation.0)),
        );
    }
    for (entity, yard, position, rotation) in &inputs.yards {
        state.replace_plot(
            entity,
            Some(PlotFootprint {
                origin: position.0,
                yaw: rotation.0,
                shape: PlotShape::Yard(yard.clone()),
            }),
        );
    }
    if inputs.zones.is_changed() {
        let changed = state
            .building_zones
            .keys()
            .chain(inputs.zones.by_chunk.keys())
            .copied()
            .filter(|coord| state.building_zones.get(coord) != inputs.zones.by_chunk.get(coord))
            .collect::<HashSet<_>>();
        for coord in changed {
            if let Some(zones) = inputs.zones.by_chunk.get(&coord) {
                state.building_zones.insert(coord, zones.clone());
            } else {
                state.building_zones.remove(&coord);
            }
            for dx in -1..=1 {
                for dz in -1..=1 {
                    state.mark_dirty(ChunkCoord::new(coord.x + dx, coord.z + dz));
                }
            }
        }
    }

    *readiness = RoadsideReadiness {
        pending_road_updates: state.pending_roads.len(),
        ..default()
    };
    if !inputs.settings.props_enabled {
        let chunks = state.chunks.keys().copied().collect::<Vec<_>>();
        for coord in chunks {
            state.clear_chunk(coord, &mut commands, &mut meshes);
        }
        readiness.ready = true;
        return;
    }
    let Some(anchor) = streaming_anchor(&inputs.players, &inputs.cameras) else {
        return;
    };
    let anchor_chunk = ChunkCoord::from_world_pos(anchor);
    let stale = state
        .chunks
        .keys()
        .copied()
        .filter(|coord| {
            !inputs.loaded.chunks.contains(coord)
                || (coord.x - anchor_chunk.x).abs() > CHUNK_RADIUS
                || (coord.z - anchor_chunk.z).abs() > CHUNK_RADIUS
                || state
                    .segments
                    .get(coord)
                    .is_none_or(|segments| !segments.iter().any(|segment| segment.built))
        })
        .collect::<Vec<_>>();
    for coord in stale {
        state.clear_chunk(coord, &mut commands, &mut meshes);
    }
    if camera_view_distance(&inputs.cameras) > FADE_END + 100.0 {
        readiness.ready = true;
        return;
    }
    let Ok(root) = inputs.roots.single() else {
        return;
    };
    let view = streaming_view_priority(&inputs.cameras);
    let pending = inputs
        .loaded
        .chunks
        .iter()
        .copied()
        .filter(|coord| {
            (coord.x - anchor_chunk.x).abs() <= CHUNK_RADIUS
                && (coord.z - anchor_chunk.z).abs() <= CHUNK_RADIUS
                && state
                    .segments
                    .get(coord)
                    .is_some_and(|segments| segments.iter().any(|segment| segment.built))
                && (state.dirty.contains(coord)
                    || state.chunks.get(coord).is_none_or(|record| {
                        record.ground_version != ground_version(*coord, &inputs.terrain)
                    }))
        })
        .collect::<Vec<_>>();
    readiness.pending_chunks = pending.len();
    let next = pending
        .into_iter()
        .min_by_key(|coord| chunk_stream_priority(*coord, anchor, view));
    let Some(coord) = next else {
        readiness.ready = readiness.pending_road_updates == 0;
        return;
    };
    let (built, clusters) =
        build_chunk(coord, &state, &inputs.terrain, inputs.colliders.as_deref());
    let triangles = built.triangles();
    state.clear_chunk(coord, &mut commands, &mut meshes);
    let mut record = ChunkRecord {
        entity: None,
        mesh: None,
        ground_version: ground_version(coord, &inputs.terrain),
    };
    if triangles > 0 {
        let material = state
            .material
            .get_or_insert_with(|| {
                materials.add(StandardMaterial {
                    base_color: Color::WHITE,
                    perceptual_roughness: 0.98,
                    metallic: 0.0,
                    reflectance: 0.12,
                    ..default()
                })
            })
            .clone();
        let mesh = meshes.add(built.finish());
        let entity = commands
            .spawn((
                Name::new(format!("Roadside dressing {},{}", coord.x, coord.z)),
                RoadsideChunk {
                    clusters,
                    triangles,
                },
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material),
                Transform::from_translation(coord.world_pos()),
                Visibility::Inherited,
                VisibilityRange {
                    start_margin: 0.0..0.0,
                    end_margin: FADE_START..FADE_END,
                    use_aabb: true,
                },
                NotShadowCaster,
            ))
            .id();
        commands.entity(root).add_child(entity);
        record.entity = Some(entity);
        record.mesh = Some(mesh);
    }
    state.chunks.insert(coord, record);
    readiness.pending_chunks -= 1;
    readiness.ready = readiness.pending_road_updates == 0 && readiness.pending_chunks == 0;
}

fn clear_roadside(
    mut commands: Commands,
    mut state: ResMut<RoadsideState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut readiness: ResMut<RoadsideReadiness>,
) {
    let chunks = state.chunks.keys().copied().collect::<Vec<_>>();
    for coord in chunks {
        state.clear_chunk(coord, &mut commands, &mut meshes);
    }
    if let Some(material) = state.material.take() {
        materials.remove(material.id());
    }
    *state = RoadsideState::default();
    *readiness = RoadsideReadiness::default();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grounded_plants_accept_rolling_verges_but_reject_steep_or_wet_edges() {
        for kind in [DetailKind::Flowers, DetailKind::Bush] {
            let plant = Candidate {
                point: Vec2::ZERO,
                radius: 1.5,
                seed: 1,
                kind,
            };
            assert!(ground_profile_suits(&plant, |p| Some(10.0 + p.x * 0.14)));
            assert!(!ground_profile_suits(&plant, |p| Some(10.0 + p.x * 0.30)));
            // Dry at the centre is insufficient: the patch must not hang over
            // a water edge or an unavailable/non-finite height sample.
            assert!(!ground_profile_suits(&plant, |p| {
                (p.x < 1.0).then_some(10.0)
            }));
            assert!(!ground_profile_suits(&plant, |p| {
                Some(if p.y > 1.0 { f32::NAN } else { 10.0 })
            }));
        }

        for radius in [0.32, 0.5, 0.57] {
            let stone = Candidate {
                point: Vec2::ZERO,
                radius,
                seed: 1,
                kind: DetailKind::Stone,
            };
            for slope in [-0.14, 0.14] {
                assert!(ground_profile_suits(&stone, |p| Some(10.0 + p.x * slope)));
            }
            assert!(!ground_profile_suits(&stone, |p| Some(10.0 + p.x * 0.20)));
            assert!(!ground_profile_suits(&stone, |p| Some(if p.x > 0.0 {
                10.19
            } else {
                10.0
            })));
            assert!(!ground_profile_suits(&stone, |p| (p.y < radius).then_some(10.0)));
        }
    }

    fn snapshot(z: f32) -> RoadSnapshot {
        RoadSnapshot {
            points: vec![Vec2::new(4.0, z), Vec2::new(25.0, z)],
            built_through: 2,
            width: 2.6,
            reserved_width: 4.0,
        }
    }

    #[test]
    fn road_index_is_budgeted_and_removal_preserves_other_roads() {
        let mut state = RoadsideState::default();
        for i in 1..=5 {
            state
                .pending_roads
                .insert(Entity::from_bits(i), Some(snapshot(i as f32 * 3.0)));
        }
        state.apply_road_updates();
        assert_eq!(state.roads.len(), ROAD_INDEX_UPDATES_PER_FRAME);
        assert_eq!(state.pending_roads.len(), 1);
        state.apply_road_updates();
        assert_eq!(state.roads.len(), 5);
        assert!(
            state.dirty.is_empty(),
            "unseen road chunks need no retained dirty markers"
        );
        let removed = Entity::from_bits(2);
        state.pending_roads.insert(removed, None);
        state.apply_road_updates();
        assert_eq!(state.roads.len(), 4);
        assert!(state
            .segments
            .values()
            .flatten()
            .all(|segment| segment.owner != removed));
        assert!(state
            .segments
            .values()
            .flatten()
            .any(|segment| { segment.owner == Entity::from_bits(3) && segment.built }));
        for owner in state.roads.keys().copied().collect::<Vec<_>>() {
            state.pending_roads.insert(owner, None);
        }
        state.apply_road_updates();
        assert!(state.roads.is_empty());
        assert!(state.segments.is_empty());
    }

    #[test]
    fn new_planned_segments_do_not_reseed_the_built_prefix() {
        let original = snapshot(12.0);
        let mut extended = original.clone();
        extended.points.push(Vec2::new(100.0, 50.0));
        let owner = Entity::from_bits(1);
        assert_eq!(
            candidates(ChunkCoord::new(0, 0), &original.segments(owner)),
            candidates(ChunkCoord::new(0, 0), &extended.segments(owner))
        );
        assert!(!extended.segments(owner)[1].built);
    }

    #[test]
    fn geometry_changes_invalidate_only_resident_chunks() {
        let mut state = RoadsideState::default();
        let owner = Entity::from_bits(1);
        state.pending_roads.insert(owner, Some(snapshot(12.0)));
        state.apply_road_updates();
        let resident = ChunkCoord::new(0, 0);
        state.chunks.insert(
            resident,
            ChunkRecord {
                entity: None,
                mesh: None,
                ground_version: 0,
            },
        );
        let mut widened = snapshot(12.0);
        widened.width = 3.2;
        state.pending_roads.insert(owner, Some(widened));
        state.apply_road_updates();
        assert_eq!(state.dirty, HashSet::from([resident]));
    }

    #[test]
    fn cleanup_releases_owned_assets_and_preserves_the_world_root() {
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<RoadsideState>()
            .init_resource::<RoadsideReadiness>()
            .add_systems(Update, clear_roadside);
        let root = app.world_mut().spawn(ClientWorldRoot).id();
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Mesh::from(Cuboid::new(1.0, 1.0, 1.0)));
        let material = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        let mesh_id = mesh.id();
        let material_id = material.id();
        let child = app
            .world_mut()
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                ChildOf(root),
            ))
            .id();
        {
            let mut state = app.world_mut().resource_mut::<RoadsideState>();
            state.material = Some(material);
            state.chunks.insert(
                ChunkCoord::new(0, 0),
                ChunkRecord {
                    entity: Some(child),
                    mesh: Some(mesh),
                    ground_version: 0,
                },
            );
            state.dirty.insert(ChunkCoord::new(0, 0));
        }
        app.world_mut().resource_mut::<RoadsideReadiness>().ready = true;
        app.update();
        assert!(app.world().get_entity(child).is_err());
        assert!(app.world().get_entity(root).is_ok());
        assert!(app
            .world()
            .resource::<Assets<Mesh>>()
            .get(mesh_id)
            .is_none());
        assert!(app
            .world()
            .resource::<Assets<StandardMaterial>>()
            .get(material_id)
            .is_none());
        assert!(app.world().resource::<RoadsideState>().chunks.is_empty());
        assert!(app.world().resource::<RoadsideState>().dirty.is_empty());
        assert!(!app.world().resource::<RoadsideReadiness>().ready);
    }
}
