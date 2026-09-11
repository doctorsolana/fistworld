//! Dense ribbon segments indexed once when their road geometry changes.
use super::*;

#[derive(Clone, Copy)]
pub(super) struct RoadSegmentRef<'a> {
    pub road: &'a RoadPaintSnapshot,
    pub dense_index: usize,
}

#[derive(Default)]
pub(super) struct RoadPaintIndex {
    by_chunk: HashMap<ChunkCoord, Vec<(Entity, usize)>>,
    by_road: HashMap<Entity, Vec<ChunkCoord>>,
}

impl RoadPaintIndex {
    fn remove(&mut self, entity: Entity, dirty: &mut HashSet<ChunkCoord>) {
        let Some(chunks) = self.by_road.remove(&entity) else {
            return;
        };
        for coord in chunks {
            dirty.insert(coord);
            if let Some(entries) = self.by_chunk.get_mut(&coord) {
                entries.retain(|(owner, _)| *owner != entity);
                if entries.is_empty() {
                    self.by_chunk.remove(&coord);
                }
            }
        }
    }

    fn insert(
        &mut self,
        entity: Entity,
        road: &RoadPaintSnapshot,
        dirty: &mut HashSet<ChunkCoord>,
    ) {
        if !road.has_surface() {
            return;
        }
        let mut touched = HashSet::new();
        for (index, pair) in road.points.windows(2).enumerate() {
            let op = road_segment_paint_op(road, pair[0], pair[1], index);
            let (min, max) = shared::terrain::terrain_paint_op_bounds(&op);
            // Inclusive samples belong to both chunks at an exact border.
            let a = (min / CHUNK_SIZE).ceil().as_ivec2() - IVec2::ONE;
            let b = (max / CHUNK_SIZE).floor().as_ivec2();
            for x in a.x..=b.x {
                for z in a.y..=b.y {
                    let coord = ChunkCoord::new(x, z);
                    self.by_chunk
                        .entry(coord)
                        .or_default()
                        .push((entity, index));
                    touched.insert(coord);
                }
            }
        }
        dirty.extend(touched.iter().copied());
        self.by_road.insert(entity, touched.into_iter().collect());
    }

    pub(super) fn segments<'a>(
        &self,
        coord: ChunkCoord,
        roads: &'a HashMap<Entity, RoadPaintSnapshot>,
    ) -> Vec<RoadSegmentRef<'a>> {
        self.by_chunk
            .get(&coord)
            .into_iter()
            .flatten()
            .filter_map(|(entity, index)| {
                roads.get(entity).map(|road| RoadSegmentRef {
                    road,
                    dense_index: *index,
                })
            })
            .collect()
    }
}

impl VillageRoadPaintState {
    pub(super) fn remove_road(&mut self, entity: Entity) {
        self.index.remove(entity, &mut self.dirty_chunks);
        self.roads.remove(&entity);
    }

    pub(super) fn replace_road(&mut self, entity: Entity, next: RoadPaintSnapshot) {
        self.index.remove(entity, &mut self.dirty_chunks);
        self.index.insert(entity, &next, &mut self.dirty_chunks);
        self.roads.insert(entity, next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(surface: RoadSurface, points: Vec<Vec2>) -> RoadPaintSnapshot {
        RoadPaintSnapshot::from_road(&VillageRoad {
            settlement: "Index test".into(),
            builder: "Builder".into(),
            built_through: points.len() as u16,
            points,
            width: 2.6,
            reserved_width: 4.,
            class: RoadClass::Lane,
            surface,
            stone_committed: 0,
        })
    }

    fn assert_matches_reference(state: &VillageRoadPaintState, coord: ChunkCoord) {
        let mut images = Assets::<Image>::default();
        let base = (0..64 * 64)
            .map(|i| [(i % 64) as u8 * 4, 0, 255 - (i % 64) as u8 * 4, 0])
            .collect::<Vec<_>>();
        let mut indexed =
            crate::terrain::paint::build_weightmap_from_weights(base.clone(), 64, &mut images);
        let mut reference =
            crate::terrain::paint::build_weightmap_from_weights(base, 64, &mut images);
        let segments = state.index.segments(coord, &state.roads);
        composite_segments_into_chunk(coord, &mut indexed, &segments, &[]);
        let roads = state.roads.iter().map(|(e, r)| (*e, r)).collect::<Vec<_>>();
        composite_roads_into_chunk(coord, &mut reference, &roads, &[]);
        assert_eq!(indexed.resolution, reference.resolution, "{coord:?}");
        assert_eq!(
            indexed.endpoint_samples, reference.endpoint_samples,
            "{coord:?}"
        );
        assert_eq!(indexed.weights, reference.weights, "{coord:?}");
    }

    #[test]
    fn indexed_mixed_roads_match_world_scan_across_inclusive_chunk_borders() {
        let mut state = VillageRoadPaintState::default();
        for (id, surface, points) in [
            (
                1,
                RoadSurface::Dirt,
                vec![Vec2::new(-75., 62.75), Vec2::new(138., 62.75)],
            ),
            (
                2,
                RoadSurface::Stone,
                vec![
                    Vec2::new(-5., -8.),
                    Vec2::new(53., 91.),
                    Vec2::new(86., 121.),
                ],
            ),
            // Paint bounds touch x=0 exactly; the inclusive west endpoint still participates.
            (
                3,
                RoadSurface::Dirt,
                vec![Vec2::new(4., -44.), Vec2::new(4., -12.)],
            ),
        ] {
            state.replace_road(Entity::from_bits(id), snapshot(surface, points));
        }
        for x in -2..=2 {
            for z in -1..=2 {
                assert_matches_reference(&state, ChunkCoord::new(x, z));
            }
        }
    }

    #[test]
    fn remote_roads_do_not_grow_local_work_and_replacements_remove_stale_segments() {
        let mut state = VillageRoadPaintState::default();
        let entity = Entity::from_bits(1);
        state.replace_road(
            entity,
            snapshot(
                RoadSurface::Dirt,
                vec![Vec2::new(8., 32.), Vec2::new(56., 32.)],
            ),
        );
        let coord = ChunkCoord::new(0, 0);
        let local_count = state.index.segments(coord, &state.roads).len();
        assert!(local_count > 0);
        state.dirty_chunks.clear();
        for id in 2..1002 {
            let x = 4096. + (id as f32) * 96.;
            state.replace_road(
                Entity::from_bits(id),
                snapshot(
                    RoadSurface::Stone,
                    vec![Vec2::new(x, 32.), Vec2::new(x + 40., 32.)],
                ),
            );
        }
        assert_eq!(state.index.segments(coord, &state.roads).len(), local_count);
        assert!(
            !state.dirty_chunks.contains(&coord),
            "distant growth dirtied a local map"
        );
        assert_matches_reference(&state, coord);
        state.dirty_chunks.clear();
        state.replace_road(
            entity,
            snapshot(
                RoadSurface::Stone,
                vec![Vec2::new(140., 32.), Vec2::new(176., 32.)],
            ),
        );
        assert!(state.index.segments(coord, &state.roads).is_empty());
        assert!(state.dirty_chunks.contains(&coord));
        let moved = ChunkCoord::new(2, 0);
        assert!(state.dirty_chunks.contains(&moved));
        assert_matches_reference(&state, coord);
        assert_matches_reference(&state, moved);
        state.dirty_chunks.clear();
        state.remove_road(entity);
        assert!(state.index.segments(moved, &state.roads).is_empty());
        assert!(state.dirty_chunks.contains(&moved));
        assert_matches_reference(&state, moved);
    }
}
