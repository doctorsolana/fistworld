use super::*;
use crate::player::boat::{
    berth::{port_geometry_valid, survey_port_berth},
    dry_at,
    navigation::{WaterPlanResult, WaterRouteCache},
};
use shared::{
    map::{HeightmapData, MapBounds},
    terrain::TerrainGenerator,
};

fn terrain(height: impl Fn(f32, f32) -> f32) -> WorldTerrain {
    let mut terrain = WorldTerrain::default();
    let mut map = terrain.generator.loaded_map().clone();
    let bounds = MapBounds {
        min: [-64.; 2],
        max: [64.; 2],
    };
    let mut heights = Vec::new();
    for z in 0..129 {
        for x in 0..129 {
            heights.push(height(x as f32 - 64., z as f32 - 64.));
        }
    }
    map.definition.bounds = bounds;
    map.definition.generated = None;
    map.heightmap = HeightmapData::new(bounds, 129, 129, heights, Some(0.));
    map.rivers = default();
    map.river_segments_by_chunk.clear();
    map.terrain_deltas_by_chunk.clear();
    // Resource-owned bounds; parallel worlds must never resize this fixture.
    terrain.generator = TerrainGenerator::from_loaded_map(map);
    terrain
}
fn sail(
    cache: &mut WaterRouteCache,
    terrain: &WorldTerrain,
    start: Vec2,
    end: Vec2,
    kind: ShipKind,
) -> Option<Vec<Vec2>> {
    let mut search = cache.begin_for(terrain, start, end, WatercraftClearance::for_ship(kind));
    for _ in 0..20_000 {
        if let WaterPlanResult::Complete(route) = cache.advance(&mut search, terrain) {
            return route;
        }
    }
    panic!("small bounded water fixture never completed");
}
fn bridge() -> RoadBridge {
    RoadBridge {
        start: Vec3::new(-32., 2., 0.),
        end: Vec3::new(32., 2., 0.),
        deck_height: 6.5,
        ramp_length: 14.,
        width: 3.6,
        built: true,
    }
}

/// Preserve the former two-query expression as an independent equivalence oracle.
fn legacy_depth_clear(terrain: &WorldTerrain, point: Vec2, draft: f32) -> bool {
    water_at(terrain, point)
        .is_some_and(|water| water - terrain.get_height(point.x, point.y) >= draft)
}

fn assert_depth_equivalence(terrain: &WorldTerrain, points: &[Vec2]) {
    for hull in [
        WatercraftClearance::DINGHY,
        WatercraftClearance::for_ship(ShipKind::Coaster),
        WatercraftClearance::for_ship(ShipKind::Cog),
    ] {
        for &point in points {
            assert_eq!(
                depth_clear(terrain, point, hull.draft),
                legacy_depth_clear(terrain, point, hull.draft),
                "depth semantics changed for {hull:?} at {point:?}"
            );
        }
    }
}

#[test]
fn one_ground_sample_preserves_shore_depth_bounds_and_absent_water() {
    let terrain = terrain(|x, _| x * 0.125);
    let mut points: Vec<_> = (-256..=256)
        .flat_map(|x| [-7.5, 0., 15.125].map(|z| Vec2::new(x as f32 * 0.25, z)))
        .collect();
    points.extend([
        Vec2::new(-64.001, 0.),
        Vec2::new(64.001, 0.),
        Vec2::new(0., -64.001),
        Vec2::new(0., 64.001),
        Vec2::new(f32::NAN, 0.),
        Vec2::new(f32::INFINITY, 0.),
    ]);
    assert_depth_equivalence(&terrain, &points);
    for hull in [
        WatercraftClearance::DINGHY,
        WatercraftClearance::for_ship(ShipKind::Coaster),
        WatercraftClearance::for_ship(ShipKind::Cog),
    ] {
        assert!(depth_clear(&terrain, Vec2::new(-32., 0.), hull.draft));
        assert!(!depth_clear(&terrain, Vec2::new(32., 0.), hull.draft));
        // Test the exact draft boundary and both sides, including fractional
        // coordinates; the comparison must not acquire a clearance epsilon.
        let boundary = -hull.draft * 8.;
        for offset in [-0.001, 0., 0.001] {
            let point = Vec2::new(boundary + offset, 0.);
            assert_eq!(
                depth_clear(&terrain, point, hull.draft),
                legacy_depth_clear(&terrain, point, hull.draft)
            );
        }
    }
    // Although runtime hulls have positive draft, wetness remains strict even
    // if a caller asks about zero or negative draft exactly on the sea plane.
    for draft in [0., -1., f32::NAN] {
        assert_eq!(
            depth_clear(&terrain, Vec2::ZERO, draft),
            legacy_depth_clear(&terrain, Vec2::ZERO, draft)
        );
    }
    let mut dry_world = terrain;
    let mut map = dry_world.generator.loaded_map().clone();
    map.heightmap.water_level = None;
    dry_world.generator = TerrainGenerator::from_loaded_map(map);
    assert_depth_equivalence(&dry_world, &points);
    assert!(!depth_clear(&dry_world, Vec2::new(-32., 0.), 0.2));
}

#[test]
fn one_ground_sample_preserves_sloping_river_depth_after_earthworks() {
    let mut terrain = WorldTerrain::default();
    let ocean = terrain.water_level().unwrap();
    let point = terrain
        .rivers()
        .iter()
        .flatten()
        .find(|p| {
            terrain
                .water_surface_height(p.x, p.z)
                .is_some_and(|water| water > ocean + 1. && terrain.get_height(p.x, p.z) < water)
        })
        .copied()
        .expect("default generated world must have a wet inland river above the sea");
    let water = terrain.water_surface_height(point.x, point.z).unwrap();
    let points: Vec<_> = (-16..=16)
        .flat_map(|x| {
            (-16..=16).map(move |z| point.xz() + Vec2::new(x as f32 * 0.75, z as f32 * 0.75))
        })
        .collect();
    assert_depth_equivalence(&terrain, &points);
    let initial_revision = terrain.modification_version();
    for (target, expected) in [(water + 1., false), (water - 2., true)] {
        terrain.apply_flatten_rect(Vec3::new(point.x, target, point.z), Vec2::splat(8.), 0., 2.);
        assert!(terrain.modification_version() > initial_revision);
        assert_eq!(terrain.water_surface_height(point.x, point.z), Some(water));
        assert_depth_equivalence(&terrain, &points);
        for hull in [
            WatercraftClearance::DINGHY,
            WatercraftClearance::for_ship(ShipKind::Coaster),
            WatercraftClearance::for_ship(ShipKind::Cog),
        ] {
            assert_eq!(depth_clear(&terrain, point.xz(), hull.draft), expected);
        }
    }
}

#[test]
#[ignore = "cold real seed-91 water-route timing; run alone because map loading sets legacy global bounds"]
fn cold_seed_91_full_hull_route_reports_slices_and_certifies_every_segment() {
    use crate::world::start_config::WorldStartConfig;
    let config =
        WorldStartConfig::from_ron(include_str!("../../../../config/worlds/small-frontier.ron"))
            .unwrap();
    let terrain =
        WorldTerrain::from_loaded_map(shared::map::load_session_map(&config.recipe(91)).unwrap());
    // Exact second-entry endpoints from the normal 30-day run. Each iteration
    // owns a fresh cache: these timings cannot accidentally measure a warm hit.
    for repeat in 0..3 {
        let start = Vec2::new(223.786865, -1564.651245);
        let goal = Vec2::new(-1183.029053, 546.786865);
        let mut cache = WaterRouteCache::default();
        let mut search = cache.begin(&terrain, start, goal);
        let began = std::time::Instant::now();
        let mut slices = 0;
        let route = loop {
            slices += 1;
            match cache.advance(&mut search, &terrain) {
                WaterPlanResult::Pending => assert!(slices < 40_000),
                WaterPlanResult::Complete(route) => break route.expect("real ocean corridor"),
            }
        };
        let planning_ms = began.elapsed().as_secs_f64() * 1000.;
        let mut previous = start;
        for &point in &route {
            assert!(cache.geometry.segment_clear(
                &terrain,
                previous,
                point,
                WatercraftClearance::DINGHY
            ));
            previous = point;
        }
        assert_eq!(previous, goal);
        eprintln!(
            "COLD_WATER repeat={repeat} planning_ms={planning_ms:.3} slices={slices} isolated_60hz_seconds={:.3} points={} progress={:?}",
            slices as f64 / 60.,
            route.len(),
            search.progress()
        );
    }
}

#[test]
#[ignore = "cold real seed-91 late-route diagnostic; run alone"]
fn late_seed_91_ocean_detour_finishes_on_the_coarse_grid() {
    use crate::world::start_config::WorldStartConfig;
    let config =
        WorldStartConfig::from_ron(include_str!("../../../../config/worlds/small-frontier.ron"))
            .unwrap();
    let terrain =
        WorldTerrain::from_loaded_map(shared::map::load_session_map(&config.recipe(91)).unwrap());
    let start = Vec2::new(190.811035, -1743.786865);
    let goal = Vec2::new(-1082.786865, 572.43335);
    let coast_started = std::time::Instant::now();
    let coasts = super::super::coastal_voyages(&terrain, 0);
    eprintln!(
        "LATE_COAST count={} planning_ms={:.3} bad_goal_retained={}",
        coasts.len(),
        coast_started.elapsed().as_secs_f64() * 1000.,
        coasts.iter().any(|v| v.mooring.distance(goal) < 1.)
    );
    let mut cache = WaterRouteCache::default();
    let mut search = cache.begin(&terrain, start, goal);
    let began = std::time::Instant::now();
    let mut slices = 0;
    let route = loop {
        slices += 1;
        match cache.advance(&mut search, &terrain) {
            WaterPlanResult::Pending => assert!(slices < 400_000),
            WaterPlanResult::Complete(route) => break route,
        }
    };
    let planning_ms = began.elapsed().as_secs_f64() * 1000.;
    let route = route.expect("the long ocean detour is reachable on the coarse grid");
    assert_eq!(search.grid_metres(), 24.);
    assert!(
        search.progress().1 > 3_750,
        "exercise the former premature coarse cutoff"
    );
    {
        let mut previous = start;
        for &point in &route {
            assert!(cache.geometry.segment_clear(
                &terrain,
                previous,
                point,
                WatercraftClearance::DINGHY
            ));
            previous = point;
        }
        assert_eq!(previous, goal);
    }
    eprintln!("LATE_WATER planning_ms={planning_ms:.3} slices={slices} isolated_60hz_seconds={:.3} route={route:?} progress={:?}", slices as f64 / 60., search.progress());
}

#[test]
fn coastal_approaches_reject_center_clear_but_hull_blocked_inlets() {
    let make_coast = |neck: Option<f32>| {
        let mut terrain = WorldTerrain::default();
        let mut map = terrain.generator.loaded_map().clone();
        let bounds = MapBounds {
            min: [-256.; 2],
            max: [256.; 2],
        };
        let heights = (0..257)
            .flat_map(|z| {
                (0..257).map(move |x| {
                    let x = -256. + x as f32 * 2.;
                    let z = -256. + z as f32 * 2.;
                    if x >= 0. {
                        4.
                    } else if neck.is_some_and(|neck| (x - neck).abs() <= 4.) && z.abs() > 1. {
                        -0.05
                    } else {
                        -4.
                    }
                })
            })
            .collect();
        map.definition.bounds = bounds;
        map.definition.generated = None;
        map.heightmap = HeightmapData::new(bounds, 257, 257, heights, Some(0.));
        map.rivers = default();
        map.river_segments_by_chunk.clear();
        map.terrain_deltas_by_chunk.clear();
        terrain.generator = TerrainGenerator::from_loaded_map(map);
        terrain
    };
    let open = make_coast(None);
    let voyage = super::super::edge_candidate(&open, 0, 0.5).expect("open coast");
    assert!(WaterNavigationGeometry::default().segment_clear(
        &open,
        voyage.start.xz(),
        voyage.mooring,
        WatercraftClearance::DINGHY
    ));
    for neck in [-100., -28.] {
        let blocked = make_coast(Some(neck));
        assert!(super::super::segment_is_water(
            &blocked,
            Vec2::new(-192., 0.),
            voyage.mooring
        ));
        assert!(WaterNavigationGeometry::default().point_clear(
            &blocked,
            voyage.start.xz(),
            WatercraftClearance::DINGHY
        ));
        assert!(WaterNavigationGeometry::default().point_clear(
            &blocked,
            voyage.mooring,
            WatercraftClearance::DINGHY
        ));
        assert!(
            super::super::edge_candidate(&blocked, 0, 0.5).is_none(),
            "neck at {neck}: real hull cannot enter along a center-only certificate"
        );
    }
}

#[test]
fn beam_and_draft_classes_cannot_borrow_the_other_hulls_cached_route() {
    let terrain = terrain(|x, _| if x.abs() > 4.5 { 2. } else { -0.8 });
    let mut cache = WaterRouteCache::default();
    let a = Vec2::new(0., -30.);
    let b = Vec2::new(0., 30.);
    assert!(sail(&mut cache, &terrain, a, b, ShipKind::Coaster).is_some());
    assert!(sail(&mut cache, &terrain, a, b, ShipKind::Cog).is_none());
    let geometry = WaterNavigationGeometry::default();
    let coaster = WatercraftClearance::for_ship(ShipKind::Coaster);
    let cog = WatercraftClearance::for_ship(ShipKind::Cog);
    // Separate width and depth checks: the wider hull still fails if draft is reduced.
    assert!(!geometry.point_clear(
        &terrain,
        Vec2::ZERO,
        WatercraftClearance { draft: 0.65, ..cog }
    ));
    assert!(!geometry.point_clear(
        &terrain,
        Vec2::ZERO,
        WatercraftClearance {
            draft: 1.0,
            ..coaster
        }
    ));
}
#[test]
fn completed_bridge_invalidates_a_cached_route_and_respects_mast_clearance() {
    let terrain = terrain(|x, _| if x.abs() >= 29. { 2. } else { -3. });
    let mut cache = WaterRouteCache::default();
    let a = Vec2::new(0., -30.);
    let b = Vec2::new(0., 30.);
    assert!(sail(&mut cache, &terrain, a, b, ShipKind::Cog).is_some());
    cache.geometry.replace(vec![Obstacle::Bridge(bridge())]);
    assert!(sail(&mut cache, &terrain, a, b, ShipKind::Coaster).is_some());
    assert!(
        sail(&mut cache, &terrain, a, b, ShipKind::Cog).is_none(),
        "mast must not pass through the completed trusses"
    );
    cache.geometry.replace(Vec::new());
    assert!(
        sail(&mut cache, &terrain, a, b, ShipKind::Cog).is_some(),
        "removed obstruction invalidates cached rejection too"
    );
}
#[test]
fn berth_is_dry_connected_to_clear_water_and_the_pier_is_a_real_obstacle() {
    let terrain = terrain(|x, _| {
        if x <= -4. {
            1.
        } else {
            (-0.5 * (x + 2.)).max(-5.)
        }
    });
    let mut geometry = WaterNavigationGeometry::default();
    let port = survey_port_berth(
        &terrain,
        &geometry,
        Vec2::new(-6., 0.),
        Vec2::X,
        ShipKind::Cog,
    )
    .expect("usable coast");
    assert!(port_geometry_valid(&terrain, &geometry, &port));
    assert!(dry_at(&terrain, port.shore.xz()));
    geometry.replace(vec![Obstacle::Pier(port)]);
    assert!(
        port_geometry_valid(&terrain, &geometry, &port),
        "own pier must leave alongside berth and departure clear"
    );
    let hull = WatercraftClearance::for_ship(ShipKind::Coaster);
    assert!(
        !geometry.point_clear(&terrain, port.pier_end.xz(), hull),
        "a ship cannot occupy the pier"
    );
    assert!(geometry.segment_clear(
        &terrain,
        port.berth.xz(),
        port.departure.xz(),
        WatercraftClearance::for_ship(ShipKind::Cog)
    ));
    let head_corner = port.pier_end.xz() - port.seaward() * 2. + port.right() * 7.;
    assert!(
        !geometry.point_clear(&terrain, head_corner, WatercraftClearance::DINGHY),
        "the broad T-head must block hulls outside the old narrow pier strip"
    );
    assert!(port.yaw.cos().abs() < 0.01 || port.yaw.sin().abs() < 0.01);
    assert!((port.departure - port.berth).xz().dot(port.seaward()).abs() < 0.01);
    let head_rock = self::terrain(|x, z| {
        if (x - 12.).abs() < 1. && (z - 7.).abs() < 1. {
            3.
        } else if x <= -4. {
            1.
        } else {
            (-0.5 * (x + 2.)).max(-5.)
        }
    });
    assert!(
        !port_geometry_valid(&head_rock, &WaterNavigationGeometry::default(), &port),
        "a dry rock beneath an outer head corner is not a valid T-head even when centreline is clear"
    );
    assert!(
        survey_port_berth(
            &terrain,
            &WaterNavigationGeometry::default(),
            Vec2::new(-4., 0.),
            Vec2::X,
            ShipKind::Cog
        )
        .is_none(),
        "the whole shore landing, not only its work point, must stay dry"
    );

    let dry = self::terrain(|_, _| 2.);
    assert!(survey_port_berth(&dry, &geometry, Vec2::ZERO, Vec2::X, ShipKind::Coaster).is_none());
    assert!(!port_geometry_valid(&dry, &geometry, &port));
}
#[test]
fn landing_plane_clears_the_whole_slope_and_rejects_a_buried_or_high_step_platform() {
    let terrain = terrain(|x, z| {
        if x <= -4. {
            1. + z * 0.012
        } else {
            (-0.5 * (x + 2.)).max(-5.)
        }
    });
    let geometry = WaterNavigationGeometry::default();
    let port = survey_port_berth(
        &terrain,
        &geometry,
        Vec2::new(-6., 0.),
        Vec2::X,
        ShipKind::Coaster,
    )
    .expect("a low stone foundation fits the gentle dry slope");
    assert!(port.shore.y > terrain.get_height(-6., 0.) + 0.08);
    assert!(port.shore.y - terrain.get_height(-6., 0.) <= 0.30);
    let rect = port.footprints()[0];
    for x in 0..=30 {
        for z in 0..=16 {
            let p = rect.center
                + (Quat::from_rotation_y(rect.yaw)
                    * Vec3::new(
                        -rect.half_extents.x + 2. * rect.half_extents.x * x as f32 / 30.,
                        0.,
                        -rect.half_extents.y + 2. * rect.half_extents.y * z as f32 / 16.,
                    ))
                .xz();
            assert!(port.shore.y >= terrain.get_height(p.x, p.y) + 0.025);
        }
    }
    let mut buried = port;
    buried.shore.y = terrain.get_height(-6., 0.);
    assert!(!port_geometry_valid(&terrain, &geometry, &buried));
    let mut high_step = port;
    high_step.shore.y += 0.4;
    assert!(!port_geometry_valid(&terrain, &geometry, &high_step));
    assert!(port_geometry_valid(&terrain, &geometry, &port));
}

#[test]
fn obstacle_index_changes_only_for_built_structures_and_real_geometry_changes() {
    let mut app = App::new();
    app.init_resource::<WaterNavigationGeometry>()
        .add_systems(Update, rebuild_water_navigation_geometry);
    let mut b = bridge();
    b.built = false;
    let e = app.world_mut().spawn(b).id();
    app.update();
    assert_eq!(app.world().resource::<WaterNavigationGeometry>().version, 0);
    app.world_mut().get_mut::<RoadBridge>(e).unwrap().built = true;
    app.update();
    let version = app.world().resource::<WaterNavigationGeometry>().version;
    assert!(version > 0);
    app.update();
    assert_eq!(
        app.world().resource::<WaterNavigationGeometry>().version,
        version
    );
    app.world_mut().despawn(e);
    app.update();
    assert!(app.world().resource::<WaterNavigationGeometry>().version > version);
}

#[test]
fn newly_completed_low_bridge_stops_a_previously_certified_cog_before_any_warp_step() {
    use crate::player::boat::{
        plan_vessel_routes, step_boats, VesselGoal, VesselNavigation, VesselNavigationQueue,
        VesselRoute, VesselRouteFailed,
    };
    use bevy::ecs::system::RunSystemOnce;
    use shared::{
        components::{CharacterMotion, PlayerPosition, PlayerRotation, TimeWarp, Vessel},
        region::RegionCoord,
    };
    let mut world = World::new();
    world.insert_resource(terrain(|x, _| if x.abs() >= 29. { 2. } else { -3. }));
    world.init_resource::<VesselNavigationQueue>();
    world.init_resource::<WaterNavigationGeometry>();
    world.spawn(TimeWarp(500.));
    let start = Vec3::new(0., 0., -30.);
    let goal = Vec2::new(0., 30.);
    let ship = world
        .spawn((
            Vessel,
            VesselNavigation::for_ship(ShipKind::Cog),
            PlayerPosition(start),
            PlayerRotation(0.),
            CharacterMotion::STATIONARY,
            RegionCoord::from_world_pos(start),
        ))
        .id();
    world
        .resource_mut::<VesselNavigationQueue>()
        .request(ship, VesselGoal::Sail(goal));
    for _ in 0..2_000 {
        world.run_system_once(plan_vessel_routes).unwrap();
        if world.get::<VesselRoute>(ship).is_some() {
            break;
        }
    }
    assert!(
        world.get::<VesselRoute>(ship).is_some(),
        "fixture must first certify a real crossing"
    );
    world
        .resource_mut::<WaterNavigationGeometry>()
        .replace(vec![Obstacle::Bridge(bridge())]);
    world.run_system_once(step_boats).unwrap();
    assert_eq!(
        world.get::<PlayerPosition>(ship).unwrap().0,
        start,
        "stale route must not jump through the bridge at500x"
    );
    assert!(world.resource::<VesselNavigationQueue>().is_pending(ship));
    for _ in 0..20_000 {
        world.run_system_once(plan_vessel_routes).unwrap();
        if !world.resource::<VesselNavigationQueue>().is_pending(ship) {
            break;
        }
    }
    assert_eq!(
        world
            .get::<VesselRouteFailed>(ship)
            .map(|failure| failure.goal),
        Some(goal)
    );
    assert!(world.get::<VesselRoute>(ship).is_none());
    assert_eq!(world.get::<PlayerPosition>(ship).unwrap().0, start);
}
