//! Terrain-splat village roads.
//!
//! A road is simulation data, not a raised render mesh. The server replicates
//! its compact built polyline; this module composites that prefix into the
//! already-loaded terrain weightmaps. Changes only dirty touched 64 m chunks,
//! and every repaint starts from the generated/authored base weights so road
//! removal, streaming and dirt-to-stone upgrades remain reversible.

mod geometry;
mod index;
mod raster;
mod yard_paths;

pub(super) use geometry::road_wear;
use geometry::{width_variation, worn_road_points};
use index::{RoadPaintIndex, RoadSegmentRef};
pub(super) use yard_paths::sync_yard_paths;
use yard_paths::{YardPathPaintIndex, YardPathPaintSnapshot};

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use bevy::asset::AssetId;
use bevy::prelude::*;

use shared::components::{
    BuildingOf, MarketLevel, PlayerPosition, PlayerRotation, RoadClass, RoadSurface, Settlement,
    SettlementBuilding, SettlementBuildingKind, SettlementCivicSquare, SettlementId,
    SettlementTier, VillageRoad,
};
use shared::terrain::{
    terrain_paint_op_chunk_coords, terrain_paint_op_intersects_chunk, ChunkCoord, TerrainLayer,
    TerrainPaintOp, TerrainPaintShape, CHUNK_SIZE,
};

use crate::terrain::paint::{TerrainPaintState, WeightMapData};
use crate::terrain::PerfHitchStats;

/// A few roads can replicate progress together at 100x. Capping uploads keeps
/// that burst invisible to frame time; the dirty set coalesces repeat changes.
const MAX_ROAD_PAINT_CHUNKS_PER_FRAME: usize = 4;
/// Half-metre shoulders resolve the small bends in a lane. Only chunks with
/// built roads/squares use this detail; the generated base stays at 64².
const ROAD_WEIGHTMAP_RESOLUTION: u32 = 128;
const DIRT_STRENGTH: f32 = 0.94;
const DIRT_FALLOFF_METERS: f32 = 0.48;
const STONE_STRENGTH: f32 = 0.97;
const STONE_FALLOFF_METERS: f32 = 0.32;
const STONE_DIRT_SHOULDER_STRENGTH: f32 = 0.62;
const STONE_DIRT_SHOULDER_FALLOFF_METERS: f32 = 1.0;

#[derive(Clone, Debug, PartialEq)]
struct RoadPaintSnapshot {
    points: Vec<Vec2>,
    surveyed_points: Vec<Vec2>,
    source_segments: Vec<usize>,
    width: f32,
    reserved_width: f32,
    class: RoadClass,
    surface: RoadSurface,
}

impl RoadPaintSnapshot {
    fn from_road(road: &VillageRoad) -> Self {
        let (points, source_segments) = worn_road_points(road);
        Self {
            points,
            source_segments,
            surveyed_points: road.built_points().to_vec(),
            width: road.width,
            reserved_width: road.reservation_width(),
            class: road.class,
            surface: road.surface,
        }
    }

    fn has_surface(&self) -> bool {
        self.points.len() >= 2 && self.width > 0.0
    }
}

/// A market square's ground, painted with the SAME layers, strengths and falloffs as the roads
/// that lead to it. The market glb ships no ground slab: an earthen market is a rectangle of the
/// Dirt layer over its plot, a paved one is Cobblestone over a dirt bed. A road ends at the
/// square's edge, so with one material on both the road flows into the square instead of stopping
/// at a kerb of another colour, and paving the square is the same in-place upgrade as paving a road.
#[derive(Clone, Debug, PartialEq)]
struct SquarePaintSnapshot {
    center: Vec2,
    half_extents: Vec2,
    /// Paint-space rotation. `TerrainPaintShape::Rect` maps world to local with the SAME matrix
    /// `local_to_world_xz` uses for local to world, so the building yaw enters negated.
    rotation: f32,
    surface: RoadSurface,
}

impl SquarePaintSnapshot {
    fn from_civic(square: &SettlementCivicSquare, level: MarketLevel) -> Self {
        Self {
            center: square.center.xz(),
            half_extents: square.half_extents,
            rotation: -square.rotation,
            surface: match level {
                MarketLevel::Earthen => RoadSurface::Dirt,
                MarketLevel::Paved => RoadSurface::Stone,
            },
        }
    }

    fn from_market(position: Vec3, rotation_y: f32, level: MarketLevel) -> Self {
        // Both market levels share one 12 x 12 footprint (BAKERY_WINDMILL_HANDOVER.md, 1a), so the
        // earthen definition describes the paved square's ground as well.
        let def = SettlementBuildingKind::Market.art().definition();
        Self {
            center: def.world_footprint_center(position, rotation_y),
            half_extents: def.footprint * 0.5,
            rotation: -rotation_y,
            surface: match level {
                MarketLevel::Earthen => RoadSurface::Dirt,
                MarketLevel::Paved => RoadSurface::Stone,
            },
        }
    }

    /// The whole earthen floor, or the compacted bed beneath a paved square.
    /// Both use the accepted bounds; the raster exposes the bed by wearing
    /// cobbles inward rather than spreading a shoulder onto unclaimed land.
    fn bed_op(&self) -> TerrainPaintOp {
        let (strength, falloff) = match self.surface {
            RoadSurface::Dirt => (DIRT_STRENGTH, DIRT_FALLOFF_METERS),
            RoadSurface::Stone => (
                STONE_DIRT_SHOULDER_STRENGTH,
                STONE_DIRT_SHOULDER_FALLOFF_METERS,
            ),
        };
        TerrainPaintOp {
            id: 1,
            layer: TerrainLayer::Dirt,
            strength,
            falloff,
            shape: TerrainPaintShape::Rect {
                center: self.center,
                half_extents: self.half_extents,
                rotation: self.rotation,
            },
        }
    }

    /// The cobbles of a paved square. Painted AFTER every road so the paving wins over a road's
    /// dirt shoulder where the road meets the square.
    fn top_op(&self) -> Option<TerrainPaintOp> {
        (self.surface == RoadSurface::Stone).then(|| TerrainPaintOp {
            id: 2,
            layer: TerrainLayer::Cobblestone,
            strength: STONE_STRENGTH,
            falloff: STONE_FALLOFF_METERS,
            shape: TerrainPaintShape::Rect {
                center: self.center,
                half_extents: self.half_extents,
                rotation: self.rotation,
            },
        })
    }
}

#[derive(Resource, Default)]
pub(super) struct VillageRoadPaintState {
    roads: HashMap<Entity, RoadPaintSnapshot>,
    index: RoadPaintIndex,
    squares: HashMap<Entity, SquarePaintSnapshot>,
    weightmaps: HashMap<ChunkCoord, AssetId<Image>>,
    dirty_chunks: HashSet<ChunkCoord>,
    yard_paths: YardPathPaintIndex,
}

/// All currently loaded terrain images include their latest roads and yard
/// paths. Unloaded chunks receive current sources when their image arrives.
#[derive(Resource, Default)]
pub(crate) struct GroundPaintReadiness {
    pub ready: bool,
    pub pending_chunks: usize,
}

pub(super) fn clear_ground_paint(
    mut state: ResMut<VillageRoadPaintState>,
    mut readiness: ResMut<GroundPaintReadiness>,
) {
    *state = VillageRoadPaintState::default();
    *readiness = GroundPaintReadiness::default();
}

/// Convert replicated road changes into localized terrain texture updates.
///
/// Querying the small road table each frame is cheap; unchanged polylines are
/// neither cloned nor repainted. Newly streamed/rebuilt weightmaps are detected
/// by handle identity and receive all current roads automatically.
pub(super) fn paint_village_roads_into_terrain(
    roads: Query<(Entity, Ref<VillageRoad>)>,
    mut removed_roads: RemovedComponents<VillageRoad>,
    markets: Query<(
        Entity,
        &SettlementBuilding,
        &PlayerPosition,
        &PlayerRotation,
        Option<&MarketLevel>,
        Option<&BuildingOf>,
    )>,
    civic_squares: Query<(Entity, &SettlementId, &Settlement, &SettlementCivicSquare)>,
    mut removed_civic_squares: RemovedComponents<SettlementCivicSquare>,
    mut removed_settlements: RemovedComponents<Settlement>,
    mut removed_buildings: RemovedComponents<SettlementBuilding>,
    mut state: ResMut<VillageRoadPaintState>,
    mut terrain_paint: ResMut<TerrainPaintState>,
    mut images: ResMut<Assets<Image>>,
    mut perf: ResMut<PerfHitchStats>,
    terrain_chunks: Query<&crate::terrain::TerrainChunk>,
    mut terrain_materials: Option<ResMut<Assets<shared::terrain::TerrainSplatMaterial>>>,
    mut readiness: Option<ResMut<GroundPaintReadiness>>,
) {
    let started = Instant::now();

    for entity in removed_roads.read() {
        state.remove_road(entity);
    }

    for (entity, road) in roads.iter() {
        if state.roads.contains_key(&entity) && !road.is_changed() {
            continue;
        }
        let next = RoadPaintSnapshot::from_road(&road);
        if state.roads.get(&entity) == Some(&next) {
            continue;
        }
        state.replace_road(entity, next);
    }

    // Market squares. The table is a handful of entities, and a snapshot compares four numbers,
    // so a plain equality check each frame is cheaper than change detection over three components.
    for entity in removed_buildings
        .read()
        .chain(removed_civic_squares.read())
        .chain(removed_settlements.read())
    {
        if let Some(previous) = state.squares.remove(&entity) {
            mark_square_chunks_dirty(&previous, &mut state.dirty_chunks);
        }
    }
    let mut civic_finishes = HashMap::new();
    for (entity, building, position, rotation, level, owner) in markets.iter() {
        if building.kind != SettlementBuildingKind::Market {
            continue;
        }
        if let Some(owner) = owner {
            civic_finishes.insert(owner.0, level.copied().unwrap_or_default());
        }
        let next = SquarePaintSnapshot::from_market(
            position.0,
            rotation.0,
            level.copied().unwrap_or_default(),
        );
        if state.squares.get(&entity) == Some(&next) {
            continue;
        }
        debug!(
            "market square paint: {:?} centre {:?} half {:?} rot {:.2}",
            next.surface, next.center, next.half_extents, next.rotation
        );
        if let Some(previous) = state.squares.insert(entity, next.clone()) {
            mark_square_chunks_dirty(&previous, &mut state.dirty_chunks);
        }
        mark_square_chunks_dirty(&next, &mut state.dirty_chunks);
    }

    // The protected common remains grass while it is only a land reservation.
    // Its ground joins the actual Market's finish; the reserved pedestrian space
    // uses the road compositor, so no raised slab flickers over roads or terrain.
    for (entity, id, settlement, square) in &civic_squares {
        let finish = civic_finishes
            .get(id)
            .copied()
            .or_else(|| (settlement.tier >= SettlementTier::Town).then_some(MarketLevel::Earthen));
        let Some(finish) = finish else {
            if let Some(previous) = state.squares.remove(&entity) {
                mark_square_chunks_dirty(&previous, &mut state.dirty_chunks);
            }
            continue;
        };
        let next = SquarePaintSnapshot::from_civic(square, finish);
        if state.squares.get(&entity) == Some(&next) {
            continue;
        }
        if let Some(previous) = state.squares.insert(entity, next.clone()) {
            mark_square_chunks_dirty(&previous, &mut state.dirty_chunks);
        }
        mark_square_chunks_dirty(&next, &mut state.dirty_chunks);
    }

    // A terrain edit or stream reload replaces the image even when the chunk
    // coordinate stays the same. Handle identity makes that replacement dirty.
    state
        .weightmaps
        .retain(|coord, _| terrain_paint.weightmaps.contains_key(coord));
    state
        .dirty_chunks
        .retain(|coord| terrain_paint.weightmaps.contains_key(coord));
    for (coord, map) in &terrain_paint.weightmaps {
        let handle = map.handle.id();
        if state.weightmaps.insert(*coord, handle) != Some(handle) {
            state.dirty_chunks.insert(*coord);
        }
    }

    let mut queued = state.dirty_chunks.iter().copied().collect::<Vec<_>>();
    if queued.is_empty() {
        if let Some(materials) = terrain_materials.as_deref_mut() {
            sync_sampling_layout(&terrain_paint, &terrain_chunks, materials, &[]);
        }
        perf.terrain_paint_ms += started.elapsed().as_secs_f32() * 1000.0;
        if let Some(readiness) = readiness.as_deref_mut() {
            readiness.pending_chunks = 0;
            readiness.ready = true;
        }
        return;
    }
    queued.sort_unstable_by_key(|coord| (coord.x, coord.z));
    queued.truncate(MAX_ROAD_PAINT_CHUNKS_PER_FRAME);

    // Dirt/stone coverage unions commute, so chunk-local segments need no
    // entity sorting and no scan of the rest of the world's dense ribbons.
    let squares = state.squares.values().cloned().collect::<Vec<_>>();

    let mut completed = Vec::with_capacity(queued.len());
    for coord in queued {
        let Some(weightmap) = terrain_paint.weightmaps.get_mut(&coord) else {
            continue;
        };
        let segments = state.index.segments(coord, &state.roads);
        let yard_paths = state.yard_paths.paths(coord);
        composite_ground_into_chunk(coord, weightmap, &segments, &squares, &yard_paths);
        if upload_weightmap(weightmap, &mut images) {
            completed.push(coord);
            perf.paint_chunks_updated += 1;
        }
    }
    for coord in &completed {
        state.dirty_chunks.remove(coord);
    }

    if let Some(materials) = terrain_materials.as_deref_mut() {
        sync_sampling_layout(&terrain_paint, &terrain_chunks, materials, &completed);
    }

    if let Some(readiness) = readiness.as_deref_mut() {
        readiness.pending_chunks = state.dirty_chunks.len();
        readiness.ready = state.dirty_chunks.is_empty();
    }

    perf.terrain_paint_ms += started.elapsed().as_secs_f32() * 1000.0;
}

/// A newly spawned chunk can become queryable one frame after its image is
/// painted. Check layout even on an otherwise idle compositor frame, but only
/// mutate a material when the image's sampling contract actually changes.
fn sync_sampling_layout(
    terrain_paint: &TerrainPaintState,
    chunks: &Query<&crate::terrain::TerrainChunk>,
    materials: &mut Assets<shared::terrain::TerrainSplatMaterial>,
    repainted: &[ChunkCoord],
) {
    for chunk in chunks {
        let Some(map) = terrain_paint.weightmaps.get(&chunk.coord) else {
            continue;
        };
        if map.handle.id() != chunk.weightmap.id() {
            continue;
        }
        let layout = if map.endpoint_samples { 1.0 } else { 0.0 };
        if materials.get(&chunk.material).is_some_and(|m| {
            m.extension.params.weightmap_endpoint_samples != layout
                || repainted.contains(&chunk.coord)
        }) {
            if let Some(mut material) = materials.get_mut(&chunk.material) {
                material.extension.params.weightmap_endpoint_samples = layout;
            }
        }
    }
}

fn mark_square_chunks_dirty(snapshot: &SquarePaintSnapshot, dirty: &mut HashSet<ChunkCoord>) {
    // The bed is the larger of the two rectangles, so it covers every chunk the top touches.
    dirty.extend(terrain_paint_op_chunk_coords(&snapshot.bed_op()));
}

fn composite_ground_into_chunk(
    coord: ChunkCoord,
    weightmap: &mut WeightMapData,
    segments: &[RoadSegmentRef],
    squares: &[SquarePaintSnapshot],
    yard_paths: &[&YardPathPaintSnapshot],
) {
    let chunk_min = Vec2::new(coord.world_pos().x, coord.world_pos().z);
    let chunk_max = chunk_min + Vec2::splat(CHUNK_SIZE);
    let has_surface = squares
        .iter()
        .any(|square| terrain_paint_op_intersects_chunk(&square.bed_op(), chunk_min, chunk_max))
        || !segments.is_empty()
        || !yard_paths.is_empty();
    weightmap.resolution = if has_surface {
        weightmap.base_resolution.max(ROAD_WEIGHTMAP_RESOLUTION)
    } else {
        weightmap.base_resolution
    };
    weightmap.endpoint_samples = has_surface;
    reset_composite_base(weightmap);

    // Square floors and beds go down first, under every road, for the same reason the stone
    // roads' beds do: nothing laid later may cover cobble with a shoulder.
    for square in squares {
        let bed = square.bed_op();
        if terrain_paint_op_intersects_chunk(&bed, chunk_min, chunk_max) {
            raster::paint_square(&bed, chunk_min, weightmap);
        }
    }

    raster::paint_road_and_yard_segments(chunk_min, weightmap, segments, yard_paths);

    // Paving last: a stone road's dirt shoulder crosses the square's edge, and the cobbles must
    // win there so the road reads as running INTO the square.
    for square in squares {
        let Some(top) = square.top_op() else {
            continue;
        };
        if terrain_paint_op_intersects_chunk(&top, chunk_min, chunk_max) {
            raster::paint_square(&top, chunk_min, weightmap);
        }
    }
}

#[cfg(test)]
fn composite_segments_into_chunk(
    coord: ChunkCoord,
    weightmap: &mut WeightMapData,
    segments: &[RoadSegmentRef],
    squares: &[SquarePaintSnapshot],
) {
    composite_ground_into_chunk(coord, weightmap, segments, squares, &[]);
}

/// Brute-force reference selection used only by equivalence tests. Production
/// uses the retained segment/chunk index and does not scan world roads here.
#[cfg(test)]
fn composite_roads_into_chunk(
    coord: ChunkCoord,
    map: &mut WeightMapData,
    roads: &[(Entity, &RoadPaintSnapshot)],
    squares: &[SquarePaintSnapshot],
) {
    let min = coord.world_pos().xz();
    let max = min + Vec2::splat(CHUNK_SIZE);
    let segments: Vec<_> = roads
        .iter()
        .flat_map(|(_, road)| {
            road.points
                .windows(2)
                .enumerate()
                .filter_map(move |(dense_index, pair)| {
                    let op = road_segment_paint_op(road, pair[0], pair[1], dense_index);
                    terrain_paint_op_intersects_chunk(&op, min, max)
                        .then_some(RoadSegmentRef { road, dense_index })
                })
        })
        .collect();
    composite_segments_into_chunk(coord, map, &segments, squares);
}

/// Bilinear reconstruction of the same authored surface, followed by vector
/// road painting at the finer resolution. Never resample a previous composite:
/// edits/removal must not accumulate blur or leave an old road behind.
fn reset_composite_base(weightmap: &mut WeightMapData) {
    if !weightmap.endpoint_samples && weightmap.resolution == weightmap.base_resolution {
        weightmap.weights.clone_from(&weightmap.base_weights);
        return;
    }
    let source = weightmap.base_resolution;
    let target = weightmap.resolution;
    weightmap.weights.resize((target * target) as usize, [0; 4]);
    let endpoint_samples = weightmap.endpoint_samples;
    let source_coordinate = |index: u32| {
        let uv = if endpoint_samples {
            index as f32 / target.saturating_sub(1).max(1) as f32
        } else {
            (index as f32 + 0.5) / target as f32
        };
        (uv * source as f32 - 0.5).clamp(0.0, (source - 1) as f32)
    };
    for z in 0..target {
        let source_z = source_coordinate(z);
        let z0 = source_z.floor() as u32;
        let z1 = (z0 + 1).min(source - 1);
        for x in 0..target {
            let source_x = source_coordinate(x);
            let x0 = source_x.floor() as u32;
            let x1 = (x0 + 1).min(source - 1);
            let sample = |sx, sz, channel| {
                weightmap.base_weights[(sz * source + sx) as usize][channel] as f32
            };
            let mut weight = [0; 4];
            for (channel, value) in weight.iter_mut().enumerate() {
                *value = sample(x0, z0, channel)
                    .lerp(sample(x1, z0, channel), source_x - x0 as f32)
                    .lerp(
                        sample(x0, z1, channel).lerp(sample(x1, z1, channel), source_x - x0 as f32),
                        source_z - z0 as f32,
                    )
                    .round() as u8;
            }
            weightmap.weights[(z * target + x) as usize] = weight;
        }
    }
}

/// Conservative upload/dirty bounds for the custom coverage rasterizer.
/// This operation is only a bound; it does not paint the visible surface.
fn road_segment_paint_op(
    road: &RoadPaintSnapshot,
    start: Vec2,
    end: Vec2,
    segment: usize,
) -> TerrainPaintOp {
    TerrainPaintOp {
        id: segment as u64 + 1,
        layer: TerrainLayer::Dirt,
        strength: 1.0,
        falloff: 0.0,
        shape: TerrainPaintShape::Line {
            start,
            end,
            width: road.reserved_width + 4.0,
        },
    }
}

fn upload_weightmap(weightmap: &WeightMapData, images: &mut Assets<Image>) -> bool {
    let Some(mut image) = images.get_mut(&weightmap.handle) else {
        return false;
    };
    let mut pixels = Vec::with_capacity(weightmap.weights.len() * 4);
    for weights in &weightmap.weights {
        pixels.extend_from_slice(weights);
    }
    image.texture_descriptor.size.width = weightmap.resolution;
    image.texture_descriptor.size.height = weightmap.resolution;
    image.data = Some(pixels);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::paint::build_weightmap_from_weights;

    fn road(surface: RoadSurface, start: Vec2, end: Vec2) -> VillageRoad {
        VillageRoad {
            settlement: "Oakmead".into(),
            builder: "Mara".into(),
            points: vec![start, end],
            built_through: 2,
            width: 2.6,
            reserved_width: 4.0,
            surface,
            class: default(),
            stone_committed: 0,
        }
    }

    fn pixel(weights: &WeightMapData, x: u32, z: u32) -> [u8; 4] {
        // Test points are metre-cell centres, independent of render detail.
        let at = |i: u32| {
            if weights.endpoint_samples {
                ((i as f32 + 0.5) * (weights.resolution - 1) as f32 / CHUNK_SIZE).round() as u32
            } else {
                ((i as f32 + 0.5) * weights.resolution as f32 / CHUNK_SIZE) as u32
            }
        };
        let x = at(x);
        let z = at(z);
        weights.weights[(z * weights.resolution + x) as usize]
    }

    #[test]
    fn road_detail_preserves_authored_base_and_restores_it_exactly_on_removal() {
        let mut images = Assets::<Image>::default();
        let base: Vec<_> = (0..64 * 64)
            .map(|i| {
                let grass = (i % 64) as u8 * 4;
                [grass, 0, 255 - grass, 0]
            })
            .collect();
        let mut map = build_weightmap_from_weights(base.clone(), 64, &mut images);
        let handle = map.handle.id();
        let lane = RoadPaintSnapshot::from_road(&road(
            RoadSurface::Dirt,
            Vec2::new(8.0, 32.0),
            Vec2::new(56.0, 32.0),
        ));
        let lanes = [(Entity::from_bits(1), &lane)];
        composite_roads_into_chunk(ChunkCoord::new(0, 0), &mut map, &lanes, &[]);
        assert_eq!(map.resolution, ROAD_WEIGHTMAP_RESOLUTION);
        assert!(map.endpoint_samples);
        assert_eq!(map.base_resolution, 64);
        assert_eq!(map.base_weights, base);
        let first_composite = map.weights.clone();
        assert!(pixel(&map, 32, 31)[1] > 200);
        assert!(upload_weightmap(&map, &mut images));
        assert_eq!(map.handle.id(), handle);
        let image = images.get(&map.handle).unwrap();
        assert_eq!(image.width(), ROAD_WEIGHTMAP_RESOLUTION);
        assert_eq!(image.data.as_ref().unwrap().len(), 128 * 128 * 4);
        composite_roads_into_chunk(ChunkCoord::new(0, 0), &mut map, &lanes, &[]);
        assert_eq!(
            map.weights, first_composite,
            "repainting must not accumulate interpolation"
        );

        composite_roads_into_chunk(ChunkCoord::new(0, 0), &mut map, &[], &[]);
        assert_eq!(map.resolution, 64);
        assert!(!map.endpoint_samples);
        assert_eq!(map.weights, base);
        assert!(upload_weightmap(&map, &mut images));
        assert_eq!(images.get(&map.handle).unwrap().width(), 64);
        composite_roads_into_chunk(ChunkCoord::new(4, 4), &mut map, &lanes, &[]);
        assert_eq!(
            map.resolution, 64,
            "distant roads must not expand wilderness maps"
        );
        assert_eq!(map.weights, base);
    }

    #[test]
    fn dirt_is_blended_into_the_ground_with_a_soft_shoulder() {
        let resolution = 64;
        let mut images = Assets::<Image>::default();
        let mut weightmap = build_weightmap_from_weights(
            vec![[255, 0, 0, 0]; (resolution * resolution) as usize],
            resolution,
            &mut images,
        );
        let snapshot = RoadPaintSnapshot::from_road(&road(
            RoadSurface::Dirt,
            Vec2::new(8.0, 32.0),
            Vec2::new(56.0, 32.0),
        ));
        let entity = Entity::from_bits(1);
        composite_roads_into_chunk(
            ChunkCoord::new(0, 0),
            &mut weightmap,
            &[(entity, &snapshot)],
            &[],
        );

        let centre = pixel(&weightmap, 32, 31);
        let shoulder = pixel(&weightmap, 32, 33);
        let meadow = pixel(&weightmap, 32, 38);
        assert!(centre[1] > 200, "road centre should be predominantly dirt");
        assert!(shoulder[0] > 0 && shoulder[1] > 0, "edge should be blended");
        assert_eq!(meadow, [255, 0, 0, 0]);
    }

    #[test]
    fn stone_wins_over_dirt_at_a_crossing() {
        let resolution = 64;
        let mut images = Assets::<Image>::default();
        let mut weightmap = build_weightmap_from_weights(
            vec![[255, 0, 0, 0]; (resolution * resolution) as usize],
            resolution,
            &mut images,
        );
        let dirt = RoadPaintSnapshot::from_road(&road(
            RoadSurface::Dirt,
            Vec2::new(8.0, 32.0),
            Vec2::new(56.0, 32.0),
        ));
        let stone = RoadPaintSnapshot::from_road(&road(
            RoadSurface::Stone,
            Vec2::new(32.0, 8.0),
            Vec2::new(32.0, 56.0),
        ));
        composite_roads_into_chunk(
            ChunkCoord::new(0, 0),
            &mut weightmap,
            &[
                (Entity::from_bits(1), &dirt),
                (Entity::from_bits(2), &stone),
            ],
            &[],
        );

        assert!(pixel(&weightmap, 31, 31)[3] > 240);
    }

    #[test]
    fn stone_road_has_a_dirt_verge_before_the_meadow() {
        let resolution = 64;
        let mut images = Assets::<Image>::default();
        let mut weightmap = build_weightmap_from_weights(
            vec![[255, 0, 0, 0]; (resolution * resolution) as usize],
            resolution,
            &mut images,
        );
        let stone = RoadPaintSnapshot::from_road(&road(
            RoadSurface::Stone,
            Vec2::new(8.0, 32.0),
            Vec2::new(56.0, 32.0),
        ));
        composite_roads_into_chunk(
            ChunkCoord::new(0, 0),
            &mut weightmap,
            &[(Entity::from_bits(1), &stone)],
            &[],
        );

        let paving = pixel(&weightmap, 31, 31);
        assert!(paving[3] > 240, "the paved core should stay cobblestone");
        // The worn paving varies in width: one fixed metre-cell centre can be
        // paving, especially after inclusive endpoint sampling. Require a real
        // contiguous shoulder inside the surveyed corridor instead of assuming
        // that the widest part has a dirt texel at an arbitrary coordinate.
        let mut run = 0;
        let mut longest = 0;
        for x in 0..weightmap.resolution {
            let world_x = weightmap.sample_position(Vec2::ZERO, x, 0).x;
            if !(24.0..40.0).contains(&world_x) {
                continue;
            }
            let has_verge = (0..weightmap.resolution).any(|z| {
                let point = weightmap.sample_position(Vec2::ZERO, x, z);
                let pixel = weightmap.weights[(z * weightmap.resolution + x) as usize];
                (32.0..34.0).contains(&point.y)
                    && pixel[0] > 20
                    && pixel[1] > 40
                    && pixel[1] > pixel[3]
            });
            run = if has_verge { run + 1 } else { 0 };
            longest = longest.max(run);
        }
        assert!(
            longest >= 4,
            "the road needs a visible, contiguous grass/dirt shoulder"
        );
        assert_eq!(pixel(&weightmap, 31, 39), [255, 0, 0, 0]);
    }

    #[test]
    fn stone_shoulder_never_overpaints_the_paved_joint() {
        let resolution = 64;
        let mut images = Assets::<Image>::default();
        let mut weightmap = build_weightmap_from_weights(
            vec![[255, 0, 0, 0]; (resolution * resolution) as usize],
            resolution,
            &mut images,
        );
        let mut bent = road(
            RoadSurface::Stone,
            Vec2::new(8.0, 30.0),
            Vec2::new(56.0, 30.0),
        );
        bent.points = vec![
            Vec2::new(8.0, 30.0),
            Vec2::new(32.0, 32.0),
            Vec2::new(56.0, 30.0),
        ];
        bent.built_through = bent.points.len() as u16;
        let snapshot = RoadPaintSnapshot::from_road(&bent);
        composite_roads_into_chunk(
            ChunkCoord::new(0, 0),
            &mut weightmap,
            &[(Entity::from_bits(1), &snapshot)],
            &[],
        );

        assert!(
            pixel(&weightmap, 31, 31)[3] > 240,
            "the last top coat must leave the bend predominantly cobblestone"
        );
    }

    #[test]
    fn a_paved_square_has_an_inner_dirt_verge_and_a_road_runs_into_it() {
        let resolution = 64;
        let mut images = Assets::<Image>::default();
        let mut weightmap = build_weightmap_from_weights(
            vec![[255, 0, 0, 0]; (resolution * resolution) as usize],
            resolution,
            &mut images,
        );
        let square =
            SquarePaintSnapshot::from_market(Vec3::new(32.0, 0.0, 32.0), 0.0, MarketLevel::Paved);
        // A stone road ending at the square's front edge (door_offset is -6.5, the edge -6.0).
        let road = RoadPaintSnapshot::from_road(&road(
            RoadSurface::Stone,
            Vec2::new(32.0, 4.0),
            Vec2::new(32.0, 25.5),
        ));
        composite_roads_into_chunk(
            ChunkCoord::new(0, 0),
            &mut weightmap,
            &[(Entity::from_bits(1), &road)],
            &[square.clone()],
        );

        assert!(
            pixel(&weightmap, 32, 32)[3] > 240,
            "the square's centre is cobblestone"
        );
        assert!(
            pixel(&weightmap, 32, 26)[3] > 240,
            "the road's end and the square's edge are one surface"
        );
        // Find the actual dusty edge on the quiet rear half, rather than an
        // old point outside the accepted square. Adjacent samples must show
        // an edge run, not an isolated chip or unrelated road shoulder.
        let mut inner_verge = false;
        for z in 0..weightmap.resolution {
            for x in 0..weightmap.resolution - 1 {
                let p = weightmap.sample_position(Vec2::ZERO, x, z);
                let q = weightmap.sample_position(Vec2::ZERO, x + 1, z);
                let near_rear = |p: Vec2| {
                    let local = p - square.center;
                    local.x.abs() < square.half_extents.x - 1.5
                        && local.y > square.half_extents.y - 1.5
                        && local.y < square.half_extents.y
                };
                let dusty = |w: [u8; 4]| w[1] > 30 && w[1] > w[3];
                if near_rear(p)
                    && near_rear(q)
                    && dusty(weightmap.weights[(z * weightmap.resolution + x) as usize])
                    && dusty(weightmap.weights[(z * weightmap.resolution + x + 1) as usize])
                {
                    inner_verge = true;
                }
            }
        }
        assert!(
            inner_verge,
            "the worn paving must expose a contiguous inner dirt transition"
        );
        assert_eq!(pixel(&weightmap, 32, 38), [255, 0, 0, 0]);
        assert_eq!(
            pixel(&weightmap, 32, 44),
            [255, 0, 0, 0],
            "meadow beyond the bed"
        );
    }

    #[test]
    fn an_earthen_square_is_dirt_and_a_turned_square_follows_its_building() {
        let resolution = 64;
        let mut images = Assets::<Image>::default();
        let mut weightmap = build_weightmap_from_weights(
            vec![[255, 0, 0, 0]; (resolution * resolution) as usize],
            resolution,
            &mut images,
        );
        // Turned a quarter, so its local +Z edge lies along world X: the same map
        // `local_to_world_xz` uses for the building's own transform.
        let square = SquarePaintSnapshot::from_market(
            Vec3::new(32.0, 0.0, 32.0),
            std::f32::consts::FRAC_PI_2,
            MarketLevel::Earthen,
        );
        let expected_edge =
            shared::rotation::local_to_world_xz(Vec2::new(0.0, 5.5), std::f32::consts::FRAC_PI_2);
        composite_roads_into_chunk(ChunkCoord::new(0, 0), &mut weightmap, &[], &[square]);

        assert!(pixel(&weightmap, 32, 32)[1] > 200, "the floor is dirt");
        assert_eq!(
            pixel(&weightmap, 32, 32)[3],
            0,
            "an earthen square has no cobble"
        );
        let inside = pixel(
            &weightmap,
            (32.0 + expected_edge.x) as u32,
            (32.0 + expected_edge.y) as u32,
        );
        assert!(
            inside[1] > 200,
            "a point 5.5 m along the building's own +Z is still floor, got {inside:?}"
        );
    }

    #[test]
    fn a_market_entity_paints_its_square_through_the_system() {
        let resolution = 64;
        let base = vec![[255, 0, 0, 0]; (resolution * resolution) as usize];
        let mut app = App::new();
        app.init_resource::<Assets<Image>>();
        app.init_resource::<TerrainPaintState>();
        app.init_resource::<VillageRoadPaintState>();
        app.init_resource::<PerfHitchStats>();
        app.add_systems(Update, paint_village_roads_into_terrain);
        let weightmap = {
            let mut images = app.world_mut().resource_mut::<Assets<Image>>();
            build_weightmap_from_weights(base.clone(), resolution, &mut images)
        };
        app.world_mut()
            .resource_mut::<TerrainPaintState>()
            .weightmaps
            .insert(ChunkCoord::new(0, 0), weightmap);
        let market = app
            .world_mut()
            .spawn((
                SettlementBuilding {
                    kind: SettlementBuildingKind::Market,
                    settlement: "Oakmead".into(),
                    owner: None,
                    quality: 1.0,
                    workers: Vec::new(),
                },
                PlayerPosition(Vec3::new(32.0, 0.0, 32.0)),
                PlayerRotation(0.0),
                MarketLevel::Paved,
            ))
            .id();

        app.update();
        let centre = {
            let map =
                &app.world().resource::<TerrainPaintState>().weightmaps[&ChunkCoord::new(0, 0)];
            pixel(map, 32, 32)
        };
        assert!(
            centre[3] > 240,
            "the square's centre should be cobble, got {centre:?}"
        );

        app.world_mut()
            .entity_mut(market)
            .insert(MarketLevel::Earthen);
        app.update();
        let centre = {
            let map =
                &app.world().resource::<TerrainPaintState>().weightmaps[&ChunkCoord::new(0, 0)];
            pixel(map, 32, 32)
        };
        assert!(
            centre[1] > 200 && centre[3] == 0,
            "downgraded to earth, got {centre:?}"
        );

        app.world_mut().despawn(market);
        app.update();
        assert_eq!(
            app.world().resource::<TerrainPaintState>().weightmaps[&ChunkCoord::new(0, 0)].weights,
            base
        );
    }

    #[test]
    fn civic_apron_tracks_market_finish_and_removal_without_touching_other_ground() {
        let mut app = App::new();
        app.init_resource::<Assets<Image>>();
        app.init_resource::<TerrainPaintState>();
        app.init_resource::<VillageRoadPaintState>();
        app.init_resource::<PerfHitchStats>();
        app.add_systems(Update, paint_village_roads_into_terrain);
        let map = build_weightmap_from_weights(
            vec![[255, 0, 0, 0]; 64 * 64],
            64,
            &mut app.world_mut().resource_mut::<Assets<Image>>(),
        );
        app.world_mut()
            .resource_mut::<TerrainPaintState>()
            .weightmaps
            .insert(ChunkCoord::new(0, 0), map);
        let square = SettlementCivicSquare {
            center: Vec3::new(32.0, 0.0, 32.0),
            half_extents: Vec2::splat(14.0),
            rotation: 0.0,
            market_position: Vec3::new(32.0, 0.0, 26.0),
            market_rotation: std::f32::consts::PI,
        };
        let hall = app
            .world_mut()
            .spawn((
                SettlementId(1),
                Settlement {
                    name: "Square".into(),
                    tier: SettlementTier::Hamlet,
                    residents: 8,
                    treasury: 0,
                },
                square.clone(),
            ))
            .id();
        let apron = |world: &World| {
            pixel(
                &world.resource::<TerrainPaintState>().weightmaps[&ChunkCoord::new(0, 0)],
                32,
                42,
            )
        };
        app.update();
        assert_eq!(
            apron(app.world()),
            [255, 0, 0, 0],
            "a reservation alone is not paved"
        );
        let market = app
            .world_mut()
            .spawn((
                BuildingOf(SettlementId(1)),
                SettlementBuilding {
                    kind: SettlementBuildingKind::Market,
                    settlement: "Square".into(),
                    owner: None,
                    quality: 1.0,
                    workers: Vec::new(),
                },
                PlayerPosition(square.market_position),
                PlayerRotation(square.market_rotation),
                MarketLevel::Earthen,
            ))
            .id();
        app.update();
        assert!(
            apron(app.world())[1] > 200,
            "the pedestrian apron joins the earthen market"
        );
        app.world_mut()
            .entity_mut(market)
            .insert(MarketLevel::Paved);
        app.update();
        assert!(
            apron(app.world())[3] > 240,
            "the funded market finish extends into its apron"
        );
        app.world_mut()
            .entity_mut(hall)
            .remove::<SettlementCivicSquare>();
        app.update();
        assert_eq!(apron(app.world()), [255, 0, 0, 0]);
        assert!(
            pixel(
                &app.world().resource::<TerrainPaintState>().weightmaps[&ChunkCoord::new(0, 0)],
                32,
                26
            )[3] > 240,
            "removing the civic reservation does not erase the actual market"
        );
    }

    #[test]
    fn removing_a_road_restores_the_original_weightmap() {
        let resolution = 64;
        let base = vec![[255, 0, 0, 0]; (resolution * resolution) as usize];
        let mut app = App::new();
        app.init_resource::<Assets<Image>>();
        app.init_resource::<TerrainPaintState>();
        app.init_resource::<VillageRoadPaintState>();
        app.init_resource::<PerfHitchStats>();
        app.add_systems(Update, paint_village_roads_into_terrain);
        let weightmap = {
            let mut images = app.world_mut().resource_mut::<Assets<Image>>();
            build_weightmap_from_weights(base.clone(), resolution, &mut images)
        };
        app.world_mut()
            .resource_mut::<TerrainPaintState>()
            .weightmaps
            .insert(ChunkCoord::new(0, 0), weightmap);
        let road_entity = app
            .world_mut()
            .spawn(road(
                RoadSurface::Dirt,
                Vec2::new(8.0, 32.0),
                Vec2::new(56.0, 32.0),
            ))
            .id();

        app.update();
        assert!(
            app.world().resource::<TerrainPaintState>().weightmaps[&ChunkCoord::new(0, 0)]
                .weights
                .iter()
                .any(|weights| weights[1] > 0)
        );

        app.world_mut().despawn(road_entity);
        app.update();
        assert_eq!(
            app.world().resource::<TerrainPaintState>().weightmaps[&ChunkCoord::new(0, 0)].weights,
            base
        );
    }

    #[test]
    fn unchanged_roads_repaint_replaced_and_reloaded_chunk_images() {
        let mut app = App::new();
        app.init_resource::<Assets<Image>>();
        app.init_resource::<TerrainPaintState>();
        app.init_resource::<VillageRoadPaintState>();
        app.init_resource::<PerfHitchStats>();
        app.add_systems(Update, paint_village_roads_into_terrain);
        app.world_mut().spawn(road(
            RoadSurface::Stone,
            Vec2::new(8., 32.),
            Vec2::new(56., 32.),
        ));
        let coord = ChunkCoord::new(0, 0);
        let mut prior = None;
        for pass in 0..3 {
            if pass == 2 {
                app.world_mut()
                    .resource_mut::<TerrainPaintState>()
                    .weightmaps
                    .remove(&coord);
                app.update();
                assert!(!app
                    .world()
                    .resource::<VillageRoadPaintState>()
                    .weightmaps
                    .contains_key(&coord));
            }
            let map = build_weightmap_from_weights(
                vec![[255, 0, 0, 0]; 64 * 64],
                64,
                &mut app.world_mut().resource_mut::<Assets<Image>>(),
            );
            let handle = map.handle.id();
            assert_ne!(prior, Some(handle));
            prior = Some(handle);
            app.world_mut()
                .resource_mut::<TerrainPaintState>()
                .weightmaps
                .insert(coord, map);
            app.update();
            let map = &app.world().resource::<TerrainPaintState>().weightmaps[&coord];
            assert!(map.endpoint_samples);
            assert!(
                pixel(map, 31, 31)[3] > 240,
                "pass {pass} lost an unchanged road"
            );
        }
    }

    #[test]
    fn late_chunk_material_gets_endpoint_sampling_and_removal_restores_original_pixels() {
        let mut app = App::new();
        app.init_resource::<Assets<Image>>();
        app.init_resource::<Assets<shared::terrain::TerrainSplatMaterial>>();
        app.init_resource::<TerrainPaintState>();
        app.init_resource::<VillageRoadPaintState>();
        app.init_resource::<PerfHitchStats>();
        app.add_systems(Update, paint_village_roads_into_terrain);
        let base: Vec<_> = (0..64 * 64)
            .map(|i| {
                let grass = (i % 64) as u8 * 4;
                [grass, 0, 255 - grass, 0]
            })
            .collect();
        let map = build_weightmap_from_weights(
            base.clone(),
            64,
            &mut app.world_mut().resource_mut::<Assets<Image>>(),
        );
        let handle = map.handle.clone();
        app.world_mut()
            .resource_mut::<TerrainPaintState>()
            .weightmaps
            .insert(ChunkCoord::new(0, 0), map);
        let entity = app
            .world_mut()
            .spawn(road(
                RoadSurface::Dirt,
                Vec2::new(8., 32.),
                Vec2::new(56., 32.),
            ))
            .id();
        app.update();
        // Deferred terrain spawns can make the material appear a frame after
        // the image is painted. An empty dirty queue must still synchronize it.
        let material = app
            .world_mut()
            .resource_mut::<Assets<shared::terrain::TerrainSplatMaterial>>()
            .add(shared::terrain::TerrainSplatMaterial {
                base: default(),
                extension: shared::terrain::TerrainSplatExtension {
                    weight_map: handle.clone(),
                    albedo_array: default(),
                    normal_array: default(),
                    cloud_field: default(),
                    params: shared::terrain::TerrainSplatParams {
                        layer_tiling: Vec4::ONE,
                        water_params: Vec4::ZERO,
                        debug_mode: 0,
                        normal_strength: 0.,
                        weightmap_endpoint_samples: 0.,
                        _pad: 0.,
                    },
                    palette: shared::terrain::stylized_palette(),
                },
            });
        app.world_mut().spawn(crate::terrain::TerrainChunk {
            coord: ChunkCoord::new(0, 0),
            weightmap: handle.clone(),
            material: material.clone(),
        });
        app.update();
        assert_eq!(
            app.world()
                .resource::<Assets<shared::terrain::TerrainSplatMaterial>>()
                .get(&material)
                .unwrap()
                .extension
                .params
                .weightmap_endpoint_samples,
            1.
        );
        app.world_mut().despawn(entity);
        app.update();
        let map = &app.world().resource::<TerrainPaintState>().weightmaps[&ChunkCoord::new(0, 0)];
        assert!(!map.endpoint_samples);
        assert_eq!(map.resolution, 64);
        assert_eq!(map.weights, base);
        assert_eq!(map.base_weights, base);
        let image = app
            .world()
            .resource::<Assets<Image>>()
            .get(&handle)
            .unwrap();
        assert_eq!(
            image.data.as_ref().unwrap(),
            &base
                .iter()
                .flat_map(|p| p.iter().copied())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            app.world()
                .resource::<Assets<shared::terrain::TerrainSplatMaterial>>()
                .get(&material)
                .unwrap()
                .extension
                .params
                .weightmap_endpoint_samples,
            0.
        );
    }
}
