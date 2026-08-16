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

use shared::components::{RoadSurface, VillageRoad};
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

#[derive(Resource, Default)]
pub(super) struct VillageRoadPaintState {
    roads: HashMap<Entity, RoadPaintSnapshot>,
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

    let mut completed = Vec::with_capacity(queued.len());
    for coord in queued {
        let Some(weightmap) = terrain_paint.weightmaps.get_mut(&coord) else {
            continue;
        };
        composite_roads_into_chunk(coord, weightmap, &snapshots);
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

fn composite_roads_into_chunk(
    coord: ChunkCoord,
    weightmap: &mut WeightMapData,
    roads: &[(Entity, &RoadPaintSnapshot)],
) {
    weightmap.weights.clone_from(&weightmap.base_weights);
    let chunk_min = Vec2::new(coord.world_pos().x, coord.world_pos().z);
    let chunk_max = chunk_min + Vec2::splat(CHUNK_SIZE);

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
        );

        assert!(
            pixel(&weightmap, 31, 31)[3] > 240,
            "the last top coat must leave the bend predominantly cobblestone"
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
