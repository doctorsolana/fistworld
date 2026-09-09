//! Terrain-splat village roads.
//!
//! A road is simulation data, not a raised render mesh. The server replicates
//! its compact built polyline; this module composites that prefix into the
//! already-loaded terrain weightmaps. Changes only dirty touched 64 m chunks,
//! and every repaint starts from the generated/authored base weights so road
//! removal, streaming and dirt-to-stone upgrades remain reversible.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use bevy::asset::AssetId;
use bevy::prelude::*;

use shared::components::{
    BuildingOf, MarketLevel, PlayerPosition, PlayerRotation, RoadSurface, Settlement,
    SettlementBuilding, SettlementBuildingKind, SettlementCivicSquare, SettlementId,
    SettlementTier, VillageRoad,
};
use shared::terrain::{
    apply_terrain_paint_op_to_weights, terrain_paint_op_chunk_coords,
    terrain_paint_op_intersects_chunk, ChunkCoord, TerrainLayer, TerrainPaintOp, TerrainPaintShape,
    CHUNK_SIZE,
};

use crate::terrain::paint::{TerrainPaintState, WeightMapData};
use crate::terrain::PerfHitchStats;

/// A few roads can replicate progress together at 100x. Capping uploads keeps
/// that burst invisible to frame time; the dirty set coalesces repeat changes.
const MAX_ROAD_PAINT_CHUNKS_PER_FRAME: usize = 4;
const DIRT_STRENGTH: f32 = 0.88;
const DIRT_FALLOFF_METERS: f32 = 1.35;
const STONE_STRENGTH: f32 = 0.97;
const STONE_FALLOFF_METERS: f32 = 0.65;
const STONE_DIRT_SHOULDER_EXTRA_WIDTH_METERS: f32 = 1.2;
const STONE_DIRT_SHOULDER_STRENGTH: f32 = 0.62;
const STONE_DIRT_SHOULDER_FALLOFF_METERS: f32 = 1.0;

#[derive(Clone, Debug, PartialEq)]
struct RoadPaintSnapshot {
    points: Vec<Vec2>,
    width: f32,
    surface: RoadSurface,
}

impl RoadPaintSnapshot {
    fn from_road(road: &VillageRoad) -> Self {
        Self {
            points: road.built_points().to_vec(),
            width: road.width,
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

    /// The dirt under everything: the whole floor of an earthen square, or the compacted bed a
    /// paved square's cobbles sit in, reaching past them by the roads' shoulder width.
    fn bed_op(&self) -> TerrainPaintOp {
        let (strength, falloff, extra) = match self.surface {
            RoadSurface::Dirt => (DIRT_STRENGTH, DIRT_FALLOFF_METERS, 0.0),
            RoadSurface::Stone => (
                STONE_DIRT_SHOULDER_STRENGTH,
                STONE_DIRT_SHOULDER_FALLOFF_METERS,
                STONE_DIRT_SHOULDER_EXTRA_WIDTH_METERS * 0.5,
            ),
        };
        TerrainPaintOp {
            id: 1,
            layer: TerrainLayer::Dirt,
            strength,
            falloff,
            shape: TerrainPaintShape::Rect {
                center: self.center,
                half_extents: self.half_extents + Vec2::splat(extra),
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
    squares: HashMap<Entity, SquarePaintSnapshot>,
    weightmaps: HashMap<ChunkCoord, AssetId<Image>>,
    dirty_chunks: HashSet<ChunkCoord>,
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
) {
    let started = Instant::now();

    for entity in removed_roads.read() {
        if let Some(previous) = state.roads.remove(&entity) {
            mark_snapshot_chunks_dirty(&previous, &mut state.dirty_chunks);
        }
    }

    for (entity, road) in roads.iter() {
        if state.roads.contains_key(&entity) && !road.is_changed() {
            continue;
        }
        let next = RoadPaintSnapshot::from_road(&road);
        if state.roads.get(&entity) == Some(&next) {
            continue;
        }
        if let Some(previous) = state.roads.insert(entity, next.clone()) {
            mark_snapshot_chunks_dirty(&previous, &mut state.dirty_chunks);
        }
        mark_snapshot_chunks_dirty(&next, &mut state.dirty_chunks);
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
    let loaded_weightmaps = terrain_paint
        .weightmaps
        .iter()
        .map(|(coord, data)| (*coord, data.handle.id()))
        .collect::<Vec<_>>();
    let loaded_coords = loaded_weightmaps
        .iter()
        .map(|(coord, _)| *coord)
        .collect::<HashSet<_>>();
    state
        .weightmaps
        .retain(|coord, _| loaded_coords.contains(coord));
    state
        .dirty_chunks
        .retain(|coord| loaded_coords.contains(coord));
    for (coord, handle) in loaded_weightmaps {
        if state.weightmaps.insert(coord, handle) != Some(handle) {
            state.dirty_chunks.insert(coord);
        }
    }

    let mut queued = state.dirty_chunks.iter().copied().collect::<Vec<_>>();
    queued.sort_unstable_by_key(|coord| (coord.x, coord.z));
    queued.truncate(MAX_ROAD_PAINT_CHUNKS_PER_FRAME);

    // Stable ordering ensures dirt is laid first and stone wins cleanly at an
    // intersection. Entity bits only break ties between equal surfaces.
    let mut snapshots = state
        .roads
        .iter()
        .filter(|(_, road)| road.has_surface())
        .map(|(entity, road)| (*entity, road))
        .collect::<Vec<_>>();
    snapshots
        .sort_unstable_by_key(|(entity, road)| (surface_order(road.surface), entity.to_bits()));

    let squares = state.squares.values().cloned().collect::<Vec<_>>();

    let mut completed = Vec::with_capacity(queued.len());
    for coord in queued {
        let Some(weightmap) = terrain_paint.weightmaps.get_mut(&coord) else {
            continue;
        };
        composite_roads_into_chunk(coord, weightmap, &snapshots, &squares);
        if upload_weightmap(weightmap, &mut images) {
            completed.push(coord);
            perf.paint_chunks_updated += 1;
        }
    }
    for coord in completed {
        state.dirty_chunks.remove(&coord);
    }

    perf.terrain_paint_ms += started.elapsed().as_secs_f32() * 1000.0;
}

fn surface_order(surface: RoadSurface) -> u8 {
    match surface {
        RoadSurface::Dirt => 0,
        RoadSurface::Stone => 1,
    }
}

fn mark_snapshot_chunks_dirty(snapshot: &RoadPaintSnapshot, dirty: &mut HashSet<ChunkCoord>) {
    for (segment, pair) in snapshot.points.windows(2).enumerate() {
        let op = road_segment_paint_op(snapshot, pair[0], pair[1], segment);
        dirty.extend(terrain_paint_op_chunk_coords(&op));
        if snapshot.surface == RoadSurface::Stone {
            let shoulder = stone_dirt_shoulder_op(snapshot, pair[0], pair[1], segment);
            dirty.extend(terrain_paint_op_chunk_coords(&shoulder));
        }
    }
}

fn mark_square_chunks_dirty(snapshot: &SquarePaintSnapshot, dirty: &mut HashSet<ChunkCoord>) {
    // The bed is the larger of the two rectangles, so it covers every chunk the top touches.
    dirty.extend(terrain_paint_op_chunk_coords(&snapshot.bed_op()));
}

fn composite_roads_into_chunk(
    coord: ChunkCoord,
    weightmap: &mut WeightMapData,
    roads: &[(Entity, &RoadPaintSnapshot)],
    squares: &[SquarePaintSnapshot],
) {
    weightmap.weights.clone_from(&weightmap.base_weights);
    let chunk_min = Vec2::new(coord.world_pos().x, coord.world_pos().z);
    let chunk_max = chunk_min + Vec2::splat(CHUNK_SIZE);

    // Square floors and beds go down first, under every road, for the same reason the stone
    // roads' beds do: nothing laid later may cover cobble with a shoulder.
    for square in squares {
        let bed = square.bed_op();
        if terrain_paint_op_intersects_chunk(&bed, chunk_min, chunk_max) {
            apply_terrain_paint_op_to_weights(
                &bed,
                chunk_min,
                &mut weightmap.weights,
                weightmap.resolution,
            );
        }
    }

    for (_, road) in roads {
        // Paving is laid over a slightly wider compacted-earth bed. Paint the
        // ENTIRE bed before any stone: interleaving bed/stone per segment lets
        // the next segment's wider shoulder cover the previous segment's
        // paving at bends.
        if road.surface == RoadSurface::Stone {
            for (segment, pair) in road.points.windows(2).enumerate() {
                let shoulder = stone_dirt_shoulder_op(road, pair[0], pair[1], segment);
                if terrain_paint_op_intersects_chunk(&shoulder, chunk_min, chunk_max) {
                    apply_terrain_paint_op_to_weights(
                        &shoulder,
                        chunk_min,
                        &mut weightmap.weights,
                        weightmap.resolution,
                    );
                }
            }
        }
        // All paved segments now land as one top coat over the complete dirt
        // bed, so no shoulder can overpaint cobble at a joint.
        for (segment, pair) in road.points.windows(2).enumerate() {
            let op = road_segment_paint_op(road, pair[0], pair[1], segment);
            if terrain_paint_op_intersects_chunk(&op, chunk_min, chunk_max) {
                apply_terrain_paint_op_to_weights(
                    &op,
                    chunk_min,
                    &mut weightmap.weights,
                    weightmap.resolution,
                );
            }
        }
    }

    // Paving last: a stone road's dirt shoulder crosses the square's edge, and the cobbles must
    // win there so the road reads as running INTO the square.
    for square in squares {
        let Some(top) = square.top_op() else {
            continue;
        };
        if terrain_paint_op_intersects_chunk(&top, chunk_min, chunk_max) {
            apply_terrain_paint_op_to_weights(
                &top,
                chunk_min,
                &mut weightmap.weights,
                weightmap.resolution,
            );
        }
    }
}

fn stone_dirt_shoulder_op(
    road: &RoadPaintSnapshot,
    start: Vec2,
    end: Vec2,
    segment: usize,
) -> TerrainPaintOp {
    TerrainPaintOp {
        id: segment as u64 + 1,
        layer: TerrainLayer::Dirt,
        strength: STONE_DIRT_SHOULDER_STRENGTH,
        falloff: STONE_DIRT_SHOULDER_FALLOFF_METERS,
        shape: TerrainPaintShape::Line {
            start,
            end,
            width: road.width * segment_width_variation(start, end, segment)
                + STONE_DIRT_SHOULDER_EXTRA_WIDTH_METERS,
        },
    }
}

fn road_segment_paint_op(
    road: &RoadPaintSnapshot,
    start: Vec2,
    end: Vec2,
    segment: usize,
) -> TerrainPaintOp {
    let (layer, strength, falloff) = match road.surface {
        RoadSurface::Dirt => (TerrainLayer::Dirt, DIRT_STRENGTH, DIRT_FALLOFF_METERS),
        RoadSurface::Stone => (
            TerrainLayer::Cobblestone,
            STONE_STRENGTH,
            STONE_FALLOFF_METERS,
        ),
    };
    TerrainPaintOp {
        // Local compositing does not persist this operation; a stable non-zero
        // id still keeps it valid and useful in diagnostics/tests.
        id: segment as u64 + 1,
        layer,
        strength,
        falloff,
        shape: TerrainPaintShape::Line {
            start,
            end,
            // Small deterministic changes prevent a surveyed path from reading
            // as a perfect extrusion while remaining stable across frames.
            width: road.width * segment_width_variation(start, end, segment),
        },
    }
}

fn segment_width_variation(start: Vec2, end: Vec2, segment: usize) -> f32 {
    let midpoint = (start + end) * 0.5;
    let hash = midpoint.x.to_bits().rotate_left(7)
        ^ midpoint.y.to_bits().rotate_left(19)
        ^ (segment as u32).wrapping_mul(2_654_435_761);
    0.94 + (hash & 255) as f32 / 255.0 * 0.12
}

fn upload_weightmap(weightmap: &WeightMapData, images: &mut Assets<Image>) -> bool {
    let Some(mut image) = images.get_mut(&weightmap.handle) else {
        return false;
    };
    let mut pixels = Vec::with_capacity(weightmap.weights.len() * 4);
    for weights in &weightmap.weights {
        pixels.extend_from_slice(weights);
    }
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
        weights.weights[(z * weights.resolution + x) as usize]
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
        let verge = pixel(&weightmap, 31, 34);
        let meadow = pixel(&weightmap, 31, 39);
        assert!(paving[3] > 240, "the paved core should stay cobblestone");
        assert!(
            verge[0] > 0 && verge[1] > verge[3],
            "expected a grass/dirt-dominant verge, got {verge:?}"
        );
        assert_eq!(meadow, [255, 0, 0, 0]);
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
    fn a_paved_square_is_cobble_with_a_dirt_verge_and_a_road_runs_into_it() {
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
            &[square],
        );

        assert!(
            pixel(&weightmap, 32, 32)[3] > 240,
            "the square's centre is cobblestone"
        );
        assert!(
            pixel(&weightmap, 32, 26)[3] > 240,
            "the road's end and the square's edge are one surface"
        );
        let verge = pixel(&weightmap, 32, 38);
        assert!(
            verge[1] > verge[3] && verge[1] > 0,
            "past the kerb the bed shows as dirt, got {verge:?}"
        );
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
}
