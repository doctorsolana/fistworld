//! Owner-keyed, transient dirt paths derived from accepted household land.
//! They share the terrain compositor; no road entity or extra ground mesh exists.

use super::*;
use shared::components::{HouseholdYard, YARD_APPROACH_HALF_WIDTH};
use shared::rotation::local_to_world_xz;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct YardPathPaintSnapshot {
    boundary: Vec<Vec2>,
    entry: Vec2,
    center: Vec2,
    approach: Option<Vec2>,
    seed: u64,
}

impl YardPathPaintSnapshot {
    pub(super) fn from_yard(yard: &HouseholdYard, origin: Vec3, yaw: f32) -> Option<Self> {
        let (entry, center) = yard.entry_path()?;
        if !origin.is_finite() || !yaw.is_finite() || !entry.is_finite() || !center.is_finite() {
            return None;
        }
        let world = |p| origin.xz() + local_to_world_xz(p, yaw);
        let boundary: Vec<_> = yard.boundary_points().into_iter().map(world).collect();
        if boundary.len() < 3 || !boundary.iter().all(|p| p.is_finite()) {
            return None;
        }
        Some(Self {
            boundary,
            entry: world(entry),
            center: world(center),
            approach: yard.approach_path().map(|(_, p)| world(p)),
            seed: yard.seed,
        })
    }

    fn bounds(&self) -> (Vec2, Vec2) {
        let end = self.approach.unwrap_or(self.entry);
        let radius = Vec2::splat(YARD_APPROACH_HALF_WIDTH);
        (
            self.entry.min(self.center).min(end) - radius,
            self.entry.max(self.center).max(end) + radius,
        )
    }

    fn contains(&self, point: Vec2, inside: bool) -> bool {
        if inside {
            shared::components::distance_squared_to_segment(point, self.entry, self.center)
                <= YARD_APPROACH_HALF_WIDTH.powi(2)
                && self
                    .boundary
                    .iter()
                    .copied()
                    .zip(self.boundary.iter().copied().cycle().skip(1))
                    .take(self.boundary.len())
                    .all(|(a, b)| (b - a).perp_dot(point - a) >= -0.00001)
        } else {
            self.approach.is_some_and(|end| {
                shared::components::distance_squared_to_segment(point, self.entry, end)
                    <= YARD_APPROACH_HALF_WIDTH.powi(2)
            })
        }
    }

    /// Coverage is unioned with road dirt before stone is applied. Pixel work
    /// visits only each short span's bounds, never the whole chunk per house.
    pub(super) fn rasterize(&self, chunk_min: Vec2, map: &WeightMapData, dirt: &mut [f32]) {
        for (end, inside) in [(Some(self.center), true), (self.approach, false)] {
            let Some(end) = end else {
                continue;
            };
            let delta = end - self.entry;
            let length = delta.length();
            if length < 0.1 {
                continue;
            }
            let side = Vec2::new(-delta.y, delta.x) / length;
            let count = (length / 0.6).ceil() as usize;
            let node = |i: usize| {
                let t = i as f32 / count as f32;
                let bow = if self.seed & 1 == 0 { 0.13 } else { -0.13 };
                self.entry.lerp(end, t) + side * bow * (std::f32::consts::PI * t).sin()
            };
            for i in 0..count {
                let a = node(i);
                let b = node(i + 1);
                let d = b - a;
                let padding = Vec2::splat(YARD_APPROACH_HALF_WIDTH);
                let lo = a.min(b) - padding;
                let hi = a.max(b) + padding;
                if hi.x < chunk_min.x
                    || hi.y < chunk_min.y
                    || lo.x > chunk_min.x + CHUNK_SIZE
                    || lo.y > chunk_min.y + CHUNK_SIZE
                {
                    continue;
                }
                let lo = ((lo - chunk_min) / map.sample_step())
                    .floor()
                    .max(Vec2::ZERO)
                    .as_uvec2();
                let hi = ((hi - chunk_min) / map.sample_step())
                    .ceil()
                    .min(Vec2::splat(map.resolution as f32 - 1.))
                    .as_uvec2();
                for z in lo.y..=hi.y {
                    for x in lo.x..=hi.x {
                        let p = map.sample_position(chunk_min, x, z);
                        if !self.contains(p, inside) {
                            continue;
                        }
                        let u = ((p - a).dot(d) / d.length_squared()).clamp(0., 1.);
                        let t = (i as f32 + u) / count as f32;
                        let nearest = a + d * u;
                        let radius = 0.45 + road_wear(nearest * 0.63) * 0.07;
                        let signed = p.distance(nearest) - radius;
                        let edge = 1. - raster::smooth(-0.20, 0.12, signed);
                        let finish = if inside {
                            1. - raster::smooth(0.78, 1., t)
                        } else {
                            1.
                        };
                        let wear = 0.76 + road_wear(nearest * 0.44 + Vec2::new(4.1, -9.3)) * 0.12;
                        let coverage = edge * finish * wear;
                        let index = (z * map.resolution + x) as usize;
                        dirt[index] = dirt[index].max(coverage);
                    }
                }
            }
        }
    }
}

#[derive(Default)]
pub(super) struct YardPathPaintIndex {
    sources: HashMap<Entity, YardPathPaintSnapshot>,
    by_chunk: HashMap<ChunkCoord, Vec<Entity>>,
    by_owner: HashMap<Entity, Vec<ChunkCoord>>,
}

impl YardPathPaintIndex {
    pub(super) fn replace(
        &mut self,
        owner: Entity,
        next: Option<YardPathPaintSnapshot>,
        dirty: &mut HashSet<ChunkCoord>,
    ) {
        if self.sources.get(&owner) == next.as_ref() {
            return;
        }
        if let Some(old) = self.by_owner.remove(&owner) {
            for coord in old {
                dirty.insert(coord);
                if let Some(entries) = self.by_chunk.get_mut(&coord) {
                    entries.retain(|e| *e != owner);
                    if entries.is_empty() {
                        self.by_chunk.remove(&coord);
                    }
                }
            }
        }
        self.sources.remove(&owner);
        let Some(next) = next else {
            return;
        };
        let (lo, hi) = next.bounds();
        // Match the road index's inclusive endpoint convention at chunk seams.
        let a = (lo / CHUNK_SIZE).ceil().as_ivec2() - IVec2::ONE;
        let b = (hi / CHUNK_SIZE).floor().as_ivec2();
        let mut chunks = Vec::new();
        for x in a.x..=b.x {
            for z in a.y..=b.y {
                let coord = ChunkCoord::new(x, z);
                self.by_chunk.entry(coord).or_default().push(owner);
                chunks.push(coord);
                dirty.insert(coord);
            }
        }
        self.by_owner.insert(owner, chunks);
        self.sources.insert(owner, next);
    }

    pub(super) fn paths(&self, coord: ChunkCoord) -> Vec<&YardPathPaintSnapshot> {
        self.by_chunk
            .get(&coord)
            .into_iter()
            .flatten()
            .filter_map(|owner| self.sources.get(owner))
            .collect()
    }
}

pub(in crate::settlement) fn sync_yard_paths(
    mut state: ResMut<VillageRoadPaintState>,
    changed: Query<
        (Entity, &HouseholdYard, &PlayerPosition, &PlayerRotation),
        Or<(
            Changed<HouseholdYard>,
            Changed<PlayerPosition>,
            Changed<PlayerRotation>,
        )>,
    >,
    mut removed_yards: RemovedComponents<HouseholdYard>,
    mut removed_positions: RemovedComponents<PlayerPosition>,
    mut removed_rotations: RemovedComponents<PlayerRotation>,
) {
    let VillageRoadPaintState {
        yard_paths,
        dirty_chunks,
        ..
    } = &mut *state;
    for entity in removed_yards
        .read()
        .chain(removed_positions.read())
        .chain(removed_rotations.read())
    {
        yard_paths.replace(entity, None, dirty_chunks);
    }
    for (entity, yard, position, rotation) in &changed {
        yard_paths.replace(
            entity,
            YardPathPaintSnapshot::from_yard(yard, position.0, rotation.0),
            dirty_chunks,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::components::{YardSide, YardUse};

    fn yard() -> HouseholdYard {
        HouseholdYard {
            minimum: Vec2::new(-2., 0.),
            maximum: Vec2::new(2., 6.),
            side: YardSide::Right,
            use_kind: YardUse::Flowers,
            seed: 31,
            boundary: vec![
                Vec2::new(-2., 0.),
                Vec2::new(2., 0.),
                Vec2::new(1., 6.),
                Vec2::new(-2., 6.),
            ],
            entry: Some(Vec2::ZERO),
            approach: Some(Vec2::new(0., -6.)),
            house: None,
        }
    }

    fn map(images: &mut Assets<Image>) -> WeightMapData {
        crate::terrain::paint::build_weightmap_from_weights(
            vec![[255, 0, 0, 0]; 64 * 64],
            64,
            images,
        )
    }

    fn snapshot(origin: Vec3, yaw: f32) -> YardPathPaintSnapshot {
        YardPathPaintSnapshot::from_yard(&yard(), origin, yaw).unwrap()
    }

    #[test]
    fn path_coverage_is_clipped_to_its_owner_and_matches_inclusive_chunk_borders() {
        let mut images = Assets::<Image>::default();
        let path = snapshot(Vec3::new(64., 0., 64.), 0.37);
        let mut maps = HashMap::new();
        let mut painted = 0;
        for x in 0..=1 {
            for z in 0..=1 {
                let coord = ChunkCoord::new(x, z);
                let mut m = map(&mut images);
                composite_ground_into_chunk(coord, &mut m, &[], &[], &[&path]);
                for z in 0..m.resolution {
                    for x in 0..m.resolution {
                        if m.weights[(z * m.resolution + x) as usize][1] == 0 {
                            continue;
                        }
                        painted += 1;
                        let p = m.sample_position(coord.world_pos().xz(), x, z);
                        assert!(
                            path.contains(p, true) || path.contains(p, false),
                            "paint escaped accepted owner at {p:?}"
                        );
                    }
                }
                maps.insert(coord, m);
            }
        }
        assert!(painted > 12);
        for z in 0..=1 {
            let west = &maps[&ChunkCoord::new(0, z)];
            let east = &maps[&ChunkCoord::new(1, z)];
            for row in 0..128 {
                assert_eq!(west.weights[row * 128 + 127], east.weights[row * 128]);
            }
        }
        for x in 0..=1 {
            let north = &maps[&ChunkCoord::new(x, 0)];
            let south = &maps[&ChunkCoord::new(x, 1)];
            for column in 0..128 {
                assert_eq!(north.weights[127 * 128 + column], south.weights[column]);
            }
        }
    }

    #[test]
    fn yard_path_union_is_order_independent_and_never_covers_paved_roads() {
        let mut images = Assets::<Image>::default();
        let path = snapshot(Vec3::new(32., 0., 32.), 0.);
        let other = snapshot(Vec3::new(32.3, 0., 32.), 0.08);
        let road = RoadPaintSnapshot::from_road(&VillageRoad {
            settlement: "test".into(),
            builder: String::new(),
            points: vec![Vec2::new(20., 32.), Vec2::new(44., 32.)],
            built_through: 2,
            width: 2.6,
            reserved_width: 4.,
            surface: RoadSurface::Stone,
            class: RoadClass::Main,
            stone_committed: 0,
        });
        let segments: Vec<_> = (0..road.source_segments.len())
            .map(|dense_index| RoadSegmentRef {
                road: &road,
                dense_index,
            })
            .collect();
        let mut a = map(&mut images);
        let mut b = map(&mut images);
        composite_ground_into_chunk(
            ChunkCoord::new(0, 0),
            &mut a,
            &segments,
            &[],
            &[&path, &other],
        );
        composite_ground_into_chunk(
            ChunkCoord::new(0, 0),
            &mut b,
            &segments,
            &[],
            &[&other, &path, &path],
        );
        assert_eq!(
            a.weights, b.weights,
            "overlap and repeat sources use coverage max, not opaque paint layering"
        );
        assert!(
            a.weights[64 * 128 + 64][3] > 240,
            "stone still wins over yard dirt at the junction"
        );
    }

    #[test]
    fn owner_index_releases_old_chunks_and_remote_yards_do_not_increase_local_work() {
        let mut index = YardPathPaintIndex::default();
        let mut dirty = HashSet::new();
        let owner = Entity::from_bits(1);
        let first = snapshot(Vec3::new(32., 0., 32.), 0.);
        index.replace(owner, Some(first.clone()), &mut dirty);
        let coord = ChunkCoord::new(0, 0);
        assert_eq!(index.paths(coord).len(), 1);
        dirty.clear();
        index.replace(owner, Some(first), &mut dirty);
        assert!(dirty.is_empty(), "same source is a no-op");
        for id in 2..202 {
            index.replace(
                Entity::from_bits(id),
                Some(snapshot(Vec3::new(1024. + id as f32 * 64., 0., 32.), 0.)),
                &mut dirty,
            );
        }
        assert_eq!(index.paths(coord).len(), 1);
        assert!(!dirty.contains(&coord));
        dirty.clear();
        index.replace(
            owner,
            Some(snapshot(Vec3::new(160., 0., 32.), 0.7)),
            &mut dirty,
        );
        assert!(dirty.contains(&coord) && dirty.contains(&ChunkCoord::new(2, 0)));
        assert!(index.paths(coord).is_empty());
        index.replace(owner, None, &mut dirty);
        assert!(index.paths(ChunkCoord::new(2, 0)).is_empty());
        assert!(!index.by_owner.contains_key(&owner));
    }

    #[test]
    fn live_paths_repaint_moves_rebuilt_images_and_removals_with_bounded_readiness() {
        let mut app = App::new();
        app.init_resource::<Assets<Image>>();
        app.init_resource::<TerrainPaintState>();
        app.init_resource::<VillageRoadPaintState>();
        app.init_resource::<GroundPaintReadiness>();
        app.init_resource::<PerfHitchStats>();
        app.add_systems(
            Update,
            (sync_yard_paths, paint_village_roads_into_terrain).chain(),
        );
        for x in 0..6 {
            let m = map(&mut app.world_mut().resource_mut::<Assets<Image>>());
            app.world_mut()
                .resource_mut::<TerrainPaintState>()
                .weightmaps
                .insert(ChunkCoord::new(x, 0), m);
        }
        let owner = app
            .world_mut()
            .spawn((
                yard(),
                PlayerPosition(Vec3::new(32., 0., 32.)),
                PlayerRotation(0.),
            ))
            .id();
        assert!(!app.world().resource::<GroundPaintReadiness>().ready);
        app.update();
        assert_eq!(
            app.world()
                .resource::<GroundPaintReadiness>()
                .pending_chunks,
            2,
            "only four image uploads per update"
        );
        assert!(!app.world().resource::<GroundPaintReadiness>().ready);
        app.update();
        assert!(app.world().resource::<GroundPaintReadiness>().ready);
        let has_path = |app: &App, x| {
            app.world().resource::<TerrainPaintState>().weightmaps[&ChunkCoord::new(x, 0)]
                .weights
                .iter()
                .any(|w| w[1] > 0)
        };
        assert!(has_path(&app, 0));
        let uploads = app
            .world()
            .resource::<PerfHitchStats>()
            .paint_chunks_updated;
        app.world_mut()
            .get_mut::<HouseholdYard>(owner)
            .unwrap()
            .use_kind = YardUse::Laundry;
        app.update();
        assert_eq!(
            app.world()
                .resource::<PerfHitchStats>()
                .paint_chunks_updated,
            uploads,
            "decorative use change does not repaint path geometry"
        );
        app.world_mut()
            .entity_mut(owner)
            .insert(PlayerPosition(Vec3::new(96., 0., 32.)));
        app.update();
        assert!(!has_path(&app, 0) && has_path(&app, 1));
        assert_eq!(
            app.world().resource::<TerrainPaintState>().weightmaps[&ChunkCoord::new(0, 0)]
                .resolution,
            64
        );
        let replacement = map(&mut app.world_mut().resource_mut::<Assets<Image>>());
        app.world_mut()
            .resource_mut::<TerrainPaintState>()
            .weightmaps
            .insert(ChunkCoord::new(1, 0), replacement);
        app.update();
        assert!(
            has_path(&app, 1),
            "terrain image rebuild reuses current owner sources"
        );
        app.world_mut().entity_mut(owner).remove::<HouseholdYard>();
        app.update();
        let paint = app.world().resource::<TerrainPaintState>();
        let cleared = &paint.weightmaps[&ChunkCoord::new(1, 0)];
        assert_eq!(cleared.weights, cleared.base_weights);
        assert!(!cleared.endpoint_samples);
        assert!(app.world().resource::<GroundPaintReadiness>().ready);
    }

    #[test]
    fn leaving_the_world_discards_cached_sources_and_cold_readiness() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        let mut state = VillageRoadPaintState::default();
        state.yard_paths.replace(
            Entity::from_bits(1),
            Some(snapshot(Vec3::ZERO, 0.)),
            &mut state.dirty_chunks,
        );
        world.insert_resource(state);
        world.insert_resource(GroundPaintReadiness {
            ready: true,
            pending_chunks: 0,
        });
        world.run_system_once(clear_ground_paint).unwrap();
        let state = world.resource::<VillageRoadPaintState>();
        assert!(state.yard_paths.sources.is_empty());
        assert!(state.dirty_chunks.is_empty() && state.weightmaps.is_empty());
        assert!(!world.resource::<GroundPaintReadiness>().ready);
    }
}
