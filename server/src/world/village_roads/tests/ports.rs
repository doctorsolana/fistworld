use super::*;
use crate::collision::building_index::{BuildingSpatialIndex, sync_building_spatial_index};
use crate::world::navgrid::{ObstacleGridState, sync_obstacle_grid};
use shared::components::{PortGeometry, SettlementId, SettlementPort, ShipKind};

fn geometry(yaw: f32) -> PortGeometry {
    let shore = Vec3::new(1700.37, 80., 0.61);
    let sea = Quat::from_rotation_y(yaw) * Vec3::NEG_Z;
    PortGeometry {
        shore,
        pier_end: shore + sea * 34.,
        berth: shore + sea * 40. - Vec3::Y,
        departure: shore + sea * 40. + Quat::from_rotation_y(yaw) * Vec3::X * 15. - Vec3::Y,
        yaw: yaw - std::f32::consts::FRAC_PI_2,
        maximum_ship: ShipKind::Cog,
    }
}

#[test]
fn port_route_cache_matches_live_solids_and_reports_completion_and_removal() {
    let mut app = App::new();
    app.init_resource::<BuildingSpatialIndex>()
        .init_resource::<SpatialObstacleGrid>()
        .init_resource::<ObstacleGridState>()
        .add_systems(
            Update,
            (sync_building_spatial_index, sync_obstacle_grid).chain(),
        );
    let entity = app.world_mut().spawn_empty().id();
    let mut cache = NavigationBuildingCache::default();
    for step in 0..8 {
        let geometry = geometry(step as f32 * std::f32::consts::TAU / 8.);
        assert!(geometry.valid());
        for built in [false, true] {
            let port = SettlementPort {
                settlement: SettlementId(1),
                geometry,
                built,
            };
            app.world_mut().entity_mut(entity).insert(port);
            app.update();
            let changed = cache.rebuild_with_fields(
                std::iter::empty(),
                std::iter::empty(),
                std::iter::empty(),
                std::iter::empty(),
                std::iter::once(&port),
            );
            assert_eq!(cache.blockers.len(), if built { 19 } else { 0 });
            assert!(
                cache.buildings.is_empty(),
                "ports must not invent a house doorway"
            );
            if built {
                assert_eq!(changed.len(), 19);
            }
            let live = app.world().resource::<SpatialObstacleGrid>();
            for x in -36..=36 {
                for z in -148..=28 {
                    let point = geometry
                        .project_asset_point(Vec3::new(x as f32 * 0.25, 1., z as f32 * 0.25))
                        .xz();
                    assert_eq!(
                        cache.spatial.point_blocked(point),
                        live.point_blocked(point)
                    );
                }
            }
            let aisle_start = geometry.project_asset_point(Vec3::new(0., 1., 5.4)).xz();
            let aisle_end = geometry.project_asset_point(Vec3::new(0., 1., -20.)).xz();
            assert!(!cache.spatial.segment_blocked(aisle_start, aisle_end));
            assert!(
                cache
                    .rebuild_with_fields(
                        std::iter::empty(),
                        std::iter::empty(),
                        std::iter::empty(),
                        std::iter::empty(),
                        std::iter::once(&port),
                    )
                    .is_empty(),
                "unchanged ports must not invalidate cached routes"
            );
        }
        app.world_mut()
            .entity_mut(entity)
            .remove::<SettlementPort>();
        app.update();
        assert_eq!(
            cache
                .rebuild_with_fields(
                    std::iter::empty(),
                    std::iter::empty(),
                    std::iter::empty(),
                    std::iter::empty(),
                    std::iter::empty(),
                )
                .len(),
            19,
            "removal must publish every former solid for local route invalidation"
        );
        assert!(cache.spatial.is_empty());
        assert!(app.world().resource::<SpatialObstacleGrid>().is_empty());
    }
}

fn await_route(app: &mut App, mover: Entity) -> Vec<Vec2> {
    for _ in 0..256 {
        app.update();
        assert!(
            app.world().get::<NavigationRouteFailed>(mover).is_none(),
            "a clear detour must be found"
        );
        if let Some(route) = app.world().get::<TravelRoute>(mover) {
            let mut points = vec![app.world().get::<PlayerPosition>(mover).unwrap().0.xz()];
            points.extend(
                route
                    .waypoints
                    .iter()
                    .map(|waypoint| waypoint.position.xz()),
            );
            return points;
        }
    }
    panic!("small local port detour never completed its bounded planner slices");
}

#[test]
fn real_route_planner_invalidates_a_new_port_crossing_detours_and_reopens_removed_solids() {
    let geometry = geometry(0.);
    let mut terrain = WorldTerrain::default();
    terrain.apply_flatten_rect(geometry.shore, Vec2::splat(80.), 0., 4.);
    let mut live_props = StaticColliders::default();
    // This fixture isolates port solids on an explicitly cleared dry forecourt.
    // Loaded empty chunks agree between the real planner and live certification.
    for chunk in agent_route_prop_chunks(geometry.shore.xz(), geometry.shore.xz(), 150.) {
        live_props.loaded_chunks.insert(chunk);
    }
    let mut app = road_test_app();
    app.insert_resource(terrain)
        .insert_resource(live_props)
        .init_resource::<Time>()
        .insert_resource(DerivedColliderLibrary { by_kind: default() })
        .init_resource::<BuildingSpatialIndex>()
        .init_resource::<SpatialObstacleGrid>()
        .init_resource::<ObstacleGridState>()
        .init_resource::<VillageRoadGraph>()
        .init_resource::<PathfindingBudgetSettings>()
        .add_systems(
            Update,
            (
                sync_building_spatial_index,
                sync_obstacle_grid,
                retry_failed_routes_after_obstacle_change,
                queue_villager_travel_routes,
                plan_villager_travel_routes,
            )
                .chain(),
        );
    let port = app
        .world_mut()
        .spawn(SettlementPort {
            settlement: SettlementId(1),
            geometry,
            built: false,
        })
        .id();
    let start = geometry.project_asset_point(Vec3::new(-10., 1., 2.));
    let goal = geometry.project_asset_point(Vec3::new(-0.7, 1., 2.));
    let mover = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            PlayerPosition(start),
            MoveTarget(goal),
        ))
        .id();
    let before = await_route(&mut app, mover);
    assert!((polyline_length(&before) - start.xz().distance(goal.xz())).abs() < 0.01);
    assert!(
        app.world_mut()
            .resource_mut::<VillageRoadGraph>()
            .tactical_route(start.xz(), goal.xz())
            .is_some()
    );

    app.world_mut()
        .get_mut::<SettlementPort>(port)
        .unwrap()
        .built = true;
    app.world_mut()
        .entity_mut(mover)
        .remove::<TravelRoute>()
        .insert(NavigationRoutePending::new(goal));
    let detour = await_route(&mut app, mover);
    let live = app.world().resource::<SpatialObstacleGrid>();
    assert!(live.segment_blocked(start.xz(), goal.xz()));
    assert!(polyline_length(&detour) > start.xz().distance(goal.xz()) + 1.);
    assert!(
        detour
            .windows(2)
            .all(|leg| !live.segment_blocked(leg[0], leg[1]))
    );

    // A goal inside the office really is inaccessible, then becomes legal when
    // the port is removed. Exercise the normal bounded retry without changing
    // the body's position or target to erase the previous failure.
    let office = geometry.project_asset_point(Vec3::new(-5.25, 1., 2.));
    app.world_mut()
        .entity_mut(mover)
        .remove::<TravelRoute>()
        .insert(MoveTarget(office));
    for _ in 0..256 {
        app.update();
        if app.world().get::<NavigationRouteFailed>(mover).is_some() {
            break;
        }
    }
    assert!(app.world().get::<NavigationRouteFailed>(mover).is_some());
    app.world_mut().entity_mut(port).remove::<SettlementPort>();
    // Geometry changes deliberately do not wake every failed actor worldwide.
    // Let this actor's real-time backoff expire before the ordinary retry.
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs(30));
    // The first tick updates the movement grid and expires this due failure.
    app.update();
    let reopened = await_route(&mut app, mover);
    assert_eq!(reopened.last().copied(), Some(office.xz()));
    assert!((polyline_length(&reopened) - start.xz().distance(office.xz())).abs() < 0.01);
    assert!(app.world().resource::<SpatialObstacleGrid>().is_empty());
}
