use super::*;
use bevy::ecs::system::RunSystemOnce;

#[path = "retention_tests.rs"]
mod retention;
#[path = "reuse_tests.rs"]
mod reuse;

fn terrain(bounds: f32, resolution: u32, height: impl Fn(f32, f32) -> f32) -> WorldTerrain {
    let mut map = WorldTerrain::default().generator.loaded_map().clone();
    let limits = shared::map::MapBounds {
        min: [-bounds; 2],
        max: [bounds; 2],
    };
    let heights = (0..resolution)
        .flat_map(|z| {
            let height = &height;
            (0..resolution).map(move |x| {
                height(
                    -bounds + 2.0 * bounds * x as f32 / (resolution - 1) as f32,
                    -bounds + 2.0 * bounds * z as f32 / (resolution - 1) as f32,
                )
            })
        })
        .collect();
    map.definition.bounds = limits;
    map.definition.generated = None;
    map.heightmap =
        shared::map::HeightmapData::new(limits, resolution, resolution, heights, Some(0.0));
    map.rivers = default();
    map.river_segments_by_chunk.clear();
    map.terrain_deltas_by_chunk.clear();
    WorldTerrain::from_loaded_map(map)
}

fn finish(
    search: &mut WaterSearch,
    terrain: &WorldTerrain,
    cache: &mut WaterRouteCache,
) -> Option<Vec<Vec2>> {
    for _ in 0..10_000 {
        if let WaterPlanResult::Complete(route) = cache.advance(search, terrain) {
            return route;
        }
    }
    panic!("bounded test route never completed");
}

#[test]
fn an_expired_water_deadline_does_no_work_despite_the_larger_unit_ceiling() {
    let terrain = terrain(32., 2, |_, _| -4.);
    let geometry = WaterNavigationGeometry::default();
    let hull = WatercraftClearance::DINGHY;
    let offsets = hull.samples();
    let mut known = HashMap::default();
    let mut reads = TerrainDependencies::default();
    let mut segment = Segment::new(Vec2::new(-20., 0.), Vec2::new(20., 0.));
    let mut budget = SAMPLE_BUDGET;
    assert_eq!(SLICE, Duration::from_micros(500));
    assert_eq!(SAMPLE_BUDGET, 32_768);
    assert_eq!(
        segment.advance(
            &terrain,
            &geometry,
            hull,
            &offsets,
            &mut known,
            &mut reads,
            &mut budget,
            Instant::now() - Duration::from_millis(1),
        ),
        None
    );
    assert_eq!(budget, SAMPLE_BUDGET);
    assert_eq!((segment.next, segment.probe), (0, 0));
    assert!(known.is_empty());
}

#[test]
fn water_work_ceiling_retains_partial_hull_probes_and_cannot_skip_a_shallow_midpoint() {
    let terrain = terrain(32., 65, |x, z| {
        if x.abs() <= 1. && z.abs() <= 1. {
            -0.1
        } else {
            -4.
        }
    });
    let geometry = WaterNavigationGeometry::default();
    let hull = WatercraftClearance::DINGHY;
    let offsets = hull.samples();
    let start = Vec2::new(-20., 0.);
    let end = Vec2::new(20., 0.);
    assert!(geometry.point_clear(&terrain, start, hull));
    assert!(geometry.point_clear(&terrain, end, hull));
    let mut segment = Segment::new(start, end);
    let mut known = HashMap::default();
    let mut reads = TerrainDependencies::default();
    let mut budget = 2;
    assert_eq!(
        segment.advance(
            &terrain,
            &geometry,
            hull,
            &offsets,
            &mut known,
            &mut reads,
            &mut budget,
            Instant::now() + Duration::from_secs(1),
        ),
        None
    );
    assert_eq!(budget, 0);
    assert_eq!((segment.next, segment.probe), (0, 2));
    assert!(known.is_empty(), "partially sampled hull is not certified");
    for _ in 0..5_000 {
        let mut budget = 2;
        if let Some(clear) = segment.advance(
            &terrain,
            &geometry,
            hull,
            &offsets,
            &mut known,
            &mut reads,
            &mut budget,
            Instant::now() + Duration::from_secs(1),
        ) {
            assert!(!clear, "yielding must not skip the shallow middle of a leg");
            assert!(
                segment.next > 0,
                "the clear prefix must have been certified"
            );
            return;
        }
        assert_eq!(budget, 0);
    }
    panic!("retained per-probe work did not finish the bounded fixture");
}

#[test]
fn shoreline_route_retains_exact_start_connector_and_every_segment_is_water() {
    let terrain = terrain(30.0, 61, |x, z| {
        if (3.0..=5.0).contains(&x) && (-2.0..=-1.0).contains(&z) {
            2.0
        } else {
            -2.0
        }
    });
    let start = Vec2::new(0.0, -4.0);
    let goal = Vec2::new(12.0, 0.0);
    assert!(segment_is_water(&terrain, start, Vec2::new(-6., -6.)));
    assert!(
        !segment_is_water(&terrain, start, Vec2::new(6.0, 0.0)),
        "fixture must expose the skipped-connector defect"
    );
    let mut cache = WaterRouteCache::default();
    let mut search = cache.begin(&terrain, start, goal);
    let route = finish(&mut search, &terrain, &mut cache).expect("connected shore needs a route");
    assert_eq!(route.last(), Some(&goal));
    let mut previous = start;
    for next in &route {
        assert!(
            WaterNavigationGeometry::default().segment_clear(
                &terrain,
                previous,
                *next,
                WatercraftClearance::DINGHY
            ),
            "dry first leg or shortcut: {previous:?}->{next:?}"
        );
        previous = *next;
    }
    let mut cached = cache.begin(&terrain, start, goal);
    let reused = finish(&mut cached, &terrain, &mut cache).expect("cached route disappeared");
    assert_eq!(reused, route);
    assert_eq!(cached.expanded, 0, "identical request must not repeat A*");
}

#[test]
fn long_direct_route_yields_and_replacement_releases_its_frontier() {
    let mut world = World::new();
    world.insert_resource(terrain(4096.0, 2, |_, _| -2.0));
    world.init_resource::<VesselNavigationQueue>();
    let vessel = world
        .spawn((Vessel, PlayerPosition(Vec3::new(-3000.0, 0.0, 0.0))))
        .id();
    world
        .resource_mut::<VesselNavigationQueue>()
        .request(vessel, VesselGoal::Sail(Vec2::new(3000.0, 0.0)));
    world.run_system_once(plan).unwrap();
    assert!(
        world.get::<VesselRoute>(vessel).is_none(),
        "long direct validation must yield too"
    );
    assert!(world
        .resource::<VesselNavigationQueue>()
        .active
        .contains_key(&vessel));
    let replacement = VesselGoal::Sail(Vec2::new(-2990.0, 0.0));
    world
        .resource_mut::<VesselNavigationQueue>()
        .request(vessel, replacement);
    assert!(!world
        .resource::<VesselNavigationQueue>()
        .active
        .contains_key(&vessel));
    for _ in 0..1_000 {
        world.run_system_once(plan).unwrap();
        if world.get::<VesselRoute>(vessel).is_some() {
            break;
        }
    }
    assert_eq!(
        world.get::<VesselRoute>(vessel).unwrap().waypoints.last(),
        Some(&Vec2::new(-2990.0, 0.0))
    );
    assert!(!world.resource::<VesselNavigationQueue>().is_pending(vessel));
}

#[test]
fn terrain_change_invalidates_both_success_cache_and_retained_search() {
    let mut terrain = terrain(48.0, 2, |_, _| -2.0);
    let start = Vec2::new(-24.0, 0.0);
    let goal = Vec2::new(24.0, 0.0);
    let mut cache = WaterRouteCache::default();
    let mut search = cache.begin(&terrain, start, goal);
    assert!(finish(&mut search, &terrain, &mut cache).is_some());
    let mut cached = cache.begin(&terrain, start, goal);
    terrain.apply_flatten_rect(Vec3::new(goal.x, 4.0, goal.y), Vec2::splat(4.0), 0.0, 0.0);
    assert!(terrain.get_water_height(goal.x, goal.y).is_none());
    assert!(
        finish(&mut cached, &terrain, &mut cache).is_none(),
        "old cached water endpoint became dry"
    );
    let mut repeated = cache.begin(&terrain, start, goal);
    assert!(finish(&mut repeated, &terrain, &mut cache).is_none());
    assert_eq!(repeated.expanded, 0);
}

#[test]
fn fleet_burst_bounds_frontiers_without_losing_any_destination() {
    let mut world = World::new();
    world.insert_resource(terrain(4096.0, 2, |_, _| -2.0));
    world.init_resource::<VesselNavigationQueue>();
    let destination = Vec2::new(3000.0, 0.0);
    let vessels: Vec<_> = (0..32)
        .map(|_| {
            let vessel = world
                .spawn((Vessel, PlayerPosition(Vec3::new(-3000.0, 0.0, 0.0))))
                .id();
            world
                .resource_mut::<VesselNavigationQueue>()
                .request(vessel, VesselGoal::Sail(destination));
            vessel
        })
        .collect();
    let mut high_water = 0;
    for _ in 0..2_000 {
        world.run_system_once(plan).unwrap();
        let queue = world.resource::<VesselNavigationQueue>();
        high_water = high_water.max(queue.active.len());
        assert!(
            queue.active.len() <= 4,
            "queued hulls must not each retain a full frontier"
        );
        if queue.pending.is_empty() {
            break;
        }
    }
    assert!(high_water > 0, "fixture did not exercise retained planning");
    for vessel in vessels {
        assert_eq!(
            world
                .get::<VesselRoute>(vessel)
                .expect("fleet order was lost")
                .waypoints
                .last(),
            Some(&destination)
        );
    }
    assert!(world.resource::<VesselNavigationQueue>().pending.is_empty());
}

fn assert_full_hull_route(terrain: &WorldTerrain, start: Vec2, goal: Vec2, route: &[Vec2]) {
    assert_eq!(route.last(), Some(&goal));
    let geometry = WaterNavigationGeometry::default();
    let mut previous = start;
    for point in route {
        assert!(
            geometry.segment_clear(terrain, previous, *point, WatercraftClearance::DINGHY),
            "uncertified coast/shortcut {previous:?} -> {point:?}"
        );
        previous = *point;
    }
}

#[test]
fn long_island_detour_uses_coarse_grid_with_full_swept_hull_proof() {
    let terrain = terrain(256., 257, |x, z| {
        if x.abs() <= 24. && z.abs() <= 120. {
            4.
        } else {
            -3.
        }
    });
    let start = Vec2::new(-180., 0.);
    let goal = Vec2::new(180., 0.);
    let mut cache = WaterRouteCache::default();
    let mut search = cache.begin(&terrain, start, goal);
    assert_eq!(search.cell_size, COARSE_CELL);
    let route = finish(&mut search, &terrain, &mut cache).expect("water surrounds the island");
    assert_eq!(
        search.cell_size, COARSE_CELL,
        "open-water detour should not need fine-grid expansion"
    );
    assert!(route.len() > 1);
    assert_full_hull_route(&terrain, start, goal, &route);
}

#[test]
fn long_narrow_channel_falls_back_to_fine_grid_without_relaxing_clearance() {
    let terrain = terrain(240., 481, |x, z| {
        if (6.0..=23.0).contains(&z) && !(x.abs() <= 4. && z <= 14.) {
            -3.
        } else {
            4.
        }
    });
    let start = Vec2::new(-180., 12.);
    let goal = Vec2::new(180., 12.);
    let mut cache = WaterRouteCache::default();
    let mut search = cache.begin(&terrain, start, goal);
    assert_eq!(search.cell_size, COARSE_CELL);
    let route = finish(&mut search, &terrain, &mut cache)
        .expect("fine grid must find the bent navigable channel");
    assert_eq!(search.cell_size, NAV_CELL);
    assert_full_hull_route(&terrain, start, goal, &route);
}

#[test]
fn distant_earthworks_preserve_retained_water_proof_but_touched_edits_restart_it() {
    let mut terrain = terrain(1024., 2, |_, _| -3.);
    let start = Vec2::new(-600., 0.);
    let goal = Vec2::new(600., 0.);
    let mut cache = WaterRouteCache::default();
    let mut search = cache.begin(&terrain, start, goal);
    while search.known.len() < 60 {
        assert!(matches!(
            cache.advance(&mut search, &terrain),
            WaterPlanResult::Pending
        ));
    }
    let known = search.known.len();
    terrain.apply_flatten_rect(Vec3::new(0., 4., 700.), Vec2::splat(8.), 0., 0.);
    assert!(matches!(
        cache.advance(&mut search, &terrain),
        WaterPlanResult::Pending
    ));
    assert!(
        search.known.len() >= known,
        "distant edits discarded already certified samples"
    );
    // This is beside the centreline: only the hull footprint reaches the new
    // dry bank. Every footprint sample, not just the centre, must invalidate.
    terrain.apply_flatten_rect(Vec3::new(start.x, 4., 2.), Vec2::new(6., 2.), 0., 0.);
    assert!(
        finish(&mut search, &terrain, &mut cache).is_none(),
        "a changed bank overlapping the hull must invalidate old proof"
    );
}

#[test]
fn replacing_all_terrain_deltas_rechecks_the_already_certified_start() {
    let mut terrain = terrain(1024., 2, |_, _| -3.);
    let start = Vec2::new(-600., 0.);
    let goal = Vec2::new(600., 0.);
    let mut cache = WaterRouteCache::default();
    let mut search = cache.begin(&terrain, start, goal);
    while search.known.len() < 60 {
        assert!(matches!(
            cache.advance(&mut search, &terrain),
            WaterPlanResult::Pending
        ));
    }
    assert_eq!(
        search.known.get(&[start.x.to_bits(), start.y.to_bits()]),
        Some(&true)
    );
    // Full replacement clears chunk counters. Only the full-rebuild stamp
    // reveals that an already certified endpoint has become dry; proceeding
    // from the retained cursor would never revisit that endpoint.
    let mut edited = self::terrain(1024., 2, |_, _| -3.);
    edited.apply_flatten_rect(Vec3::new(start.x, 4., start.y), Vec2::splat(8.), 0., 0.);
    terrain.replace_delta_chunks(edited.delta_chunks().clone());
    assert!(terrain.get_water_height(start.x, start.y).is_none());
    assert!(
        finish(&mut search, &terrain, &mut cache).is_none(),
        "full replacement must not reuse the now-dry starting connector"
    );
}

#[test]
fn certified_voyage_keeps_sailing_past_distant_earthworks_and_stops_before_local_changes() {
    for warp in [1., 25.] {
        let mut world = World::new();
        world.insert_resource(terrain(1024., 2, |_, _| -3.));
        world.init_resource::<VesselNavigationQueue>();
        world.init_resource::<WaterNavigationGeometry>();
        world.spawn(shared::components::TimeWarp(warp));
        let start = Vec3::new(-300., 0., 0.);
        let goal = Vec2::new(300., 0.);
        let boat = world
            .spawn((
                Vessel,
                VesselNavigation::DINGHY,
                PlayerPosition(start),
                PlayerRotation(0.),
                CharacterMotion::STATIONARY,
                RegionCoord::from_world_pos(start),
            ))
            .id();
        world
            .resource_mut::<VesselNavigationQueue>()
            .request(boat, VesselGoal::Sail(goal));
        for _ in 0..2_000 {
            world.run_system_once(plan).unwrap();
            if world.get::<VesselRoute>(boat).is_some() {
                break;
            }
        }
        assert!(world.get::<VesselRouteCertification>(boat).is_some());
        world.resource_mut::<WorldTerrain>().apply_flatten_rect(
            Vec3::new(0., 4., 500.),
            Vec2::splat(10.),
            0.,
            0.,
        );
        world.run_system_once(step_boats).unwrap();
        let advanced = world.get::<PlayerPosition>(boat).unwrap().0;
        assert!(
            advanced.x > start.x,
            "distant construction stopped a certified voyage at {warp}x"
        );
        assert!(!world.resource::<VesselNavigationQueue>().is_pending(boat));
        assert!(world.get::<VesselRoute>(boat).is_some());
        world.resource_mut::<WorldTerrain>().apply_flatten_rect(
            Vec3::new(0., 4., 0.),
            Vec2::splat(10.),
            0.,
            0.,
        );
        world.run_system_once(step_boats).unwrap();
        assert_eq!(
            world.get::<PlayerPosition>(boat).unwrap().0,
            advanced,
            "local route changes must stop the hull before another movement step at {warp}x"
        );
        assert!(world.get::<VesselRoute>(boat).is_none());
        assert!(world.resource::<VesselNavigationQueue>().is_pending(boat));
    }
}
