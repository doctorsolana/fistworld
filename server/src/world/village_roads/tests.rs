use super::*;
use crate::collision::library::{DerivedCollider, StaticColliderInstance};
use shared::components::{SettlementTier, TimeWarp, WorkStatus};
use shared::props::PropKind;
use shared::region::RegionCoord;

fn road_test_app() -> App {
    let mut app = App::new();
    app.init_resource::<crate::world::identity::WorldIdAllocator>()
        .init_resource::<crate::world::identity::WorldIdentityIndex>()
        .add_systems(
            PreUpdate,
            (
                crate::world::identity::assign_stable_world_ids,
                crate::world::identity::rebuild_world_identity_index,
                crate::world::identity::reconcile_stable_world_relationships,
                crate::world::identity::reconcile_stable_adjunct_relationships,
                crate::world::identity::reconcile_stable_road_relationships,
                crate::world::identity::reconcile_stable_civic_employment,
            )
                .chain(),
        );
    app
}

#[test]
fn permit_access_reuse_preserves_network_and_hall_endpoints() {
    let door = Vec2::new(10.0, 10.0);
    let approach = Vec2::new(10.0, 7.75);
    let network = Vec2::new(16.0, 4.0);
    assert_eq!(
        construction::planned_access_survey_points(&[door, approach, network], door, network, None,),
        Some(vec![approach, network]),
    );

    let hall_approach = Vec2::new(22.0, 1.0);
    let hall_door = Vec2::new(22.0, 3.0);
    assert_eq!(
        construction::planned_access_survey_points(
            &[door, approach, hall_approach, hall_door],
            door,
            hall_approach,
            Some(hall_door),
        ),
        Some(vec![approach, hall_approach]),
    );
    assert!(construction::planned_access_survey_points(
        &[door, approach, network],
        door,
        network + Vec2::X,
        None,
    )
    .is_none());
}

fn one_static_prop(
    kind: PropKind,
    position: Vec3,
    horizontal_radius: f32,
) -> (StaticColliders, DerivedColliderLibrary) {
    let cell = (
        (position.x / 16.0).floor() as i32,
        (position.z / 16.0).floor() as i32,
    );
    let mut colliders = StaticColliders::default();
    colliders.instances.insert(
        1,
        StaticColliderInstance {
            kind,
            position,
            scale: 1.0,
            cell,
        },
    );
    colliders.cells.insert(cell, vec![1]);
    let derived = DerivedColliderLibrary {
        by_kind: std::collections::HashMap::from([(kind, DerivedCollider { horizontal_radius })]),
    };
    (colliders, derived)
}

#[test]
fn farmstead_permit_allows_a_clearable_tree_but_rejects_a_rock_in_its_door_apron() {
    let kind = SettlementBuildingKind::Farmstead;
    let position = Vec3::ZERO;
    let rotation = 0.0;
    let (door, approach) = doorway_approach(kind, position, rotation);
    let tree = door.lerp(approach, 0.55);
    let (colliders, derived) = one_static_prop(
        PropKind::BroadleafLargeA,
        Vec3::new(tree.x, 0.0, tree.y),
        0.9,
    );

    assert!(doorway_road_apron_is_clear_of_props(
        kind, position, rotation, &colliders, &derived,
    ));
    let (rock_colliders, rock_derived) =
        one_static_prop(PropKind::BoulderA, Vec3::new(tree.x, 0.0, tree.y), 0.9);
    assert!(!doorway_road_apron_is_clear_of_props(
        kind,
        position,
        rotation,
        &rock_colliders,
        &rock_derived,
    ));
    assert!(doorway_road_apron_is_clear_of_props(
        kind,
        position + Vec3::X * 20.0,
        rotation,
        &colliders,
        &derived,
    ));
}

#[test]
fn mature_fixture_roads_reject_rocks_and_durably_clear_trees() {
    let points = [Vec2::new(-4.0, 0.0), Vec2::new(4.0, 0.0)];
    let (mut trees, tree_shapes) = one_static_prop(PropKind::BroadleafLargeA, Vec3::ZERO, 0.9);
    assert!(road_access_is_clear_of_permanent_props(
        &points,
        RoadClass::Lane.initial_reserved_width(),
        &trees,
        &tree_shapes,
    ));
    assert_eq!(
        clear_completed_road_trees(&points, 2.5, &mut trees, &tree_shapes),
        1,
    );
    assert!(trees.instances.is_empty());
    assert!(trees.road_tree_was_cleared(Vec2::ZERO));

    let (rocks, rock_shapes) = one_static_prop(PropKind::BoulderA, Vec3::ZERO, 0.9);
    assert!(!road_access_is_clear_of_permanent_props(
        &points,
        RoadClass::Lane.initial_reserved_width(),
        &rocks,
        &rock_shapes,
    ));
}

#[test]
fn farm_field_reservation_rejects_props_inside_its_rotated_rows() {
    let rotation = 0.63;
    let center = Vec2::new(20.0, -10.0);
    let local_tree = Vec2::new(3.7, 4.9);
    let tree = center + shared::rotation::local_to_world_xz(local_tree, rotation);
    let (colliders, derived) = one_static_prop(
        PropKind::BroadleafSpreadingA,
        Vec3::new(tree.x, 0.0, tree.y),
        0.7,
    );

    assert!(!rotated_rect_is_clear_of_props(
        center,
        Vec2::new(4.0, 5.5),
        rotation,
        shared::components::FARM_FIELD_EDGE_CLEARANCE,
        &colliders,
        &derived,
    ));
    assert!(rotated_rect_is_clear_of_permanent_props(
        center,
        Vec2::new(4.0, 5.5),
        rotation,
        shared::components::FARM_FIELD_TERRACE_MARGIN,
        &colliders,
        &derived,
    ));
    let (rocks, rock_shapes) =
        one_static_prop(PropKind::BoulderA, Vec3::new(tree.x, 0.0, tree.y), 0.7);
    assert!(!rotated_rect_is_clear_of_permanent_props(
        center,
        Vec2::new(4.0, 5.5),
        rotation,
        shared::components::FARM_FIELD_TERRACE_MARGIN,
        &rocks,
        &rock_shapes,
    ));
    assert!(rotated_rect_is_clear_of_props(
        center + Vec2::X * 20.0,
        Vec2::new(4.0, 5.5),
        rotation,
        shared::components::FARM_FIELD_EDGE_CLEARANCE,
        &colliders,
        &derived,
    ));
}

#[test]
fn route_certification_uses_the_same_tree_collision_as_movement() {
    let (colliders, derived) = one_static_prop(PropKind::BroadleafLargeA, Vec3::ZERO, 1.0);
    let blocked = [Vec2::new(-4.0, 0.0), Vec2::new(4.0, 0.0)];
    let detour = [
        Vec2::new(-4.0, 0.0),
        Vec2::new(-4.0, 4.0),
        Vec2::new(4.0, 4.0),
        Vec2::new(4.0, 0.0),
    ];

    assert!(!polyline_clear_live_world(
        &blocked,
        None,
        Some(&colliders),
        Some(&derived),
    ));
    assert!(polyline_clear_live_world(
        &detour,
        None,
        Some(&colliders),
        Some(&derived),
    ));
}

#[test]
fn failed_road_surveys_use_bounded_real_time_backoff() {
    let first = RoadSurveyBackoff::after_failure(None, 10.0);
    assert_eq!(first.failures, 1);
    assert_eq!(first.retry_after, 10.5);
    assert!(first.should_warn());

    let second = RoadSurveyBackoff::after_failure(Some(first), first.retry_after);
    assert_eq!(second.failures, 2);
    assert_eq!(second.retry_after, 11.5);
    assert!(second.should_warn());

    let mut state = second;
    let mut now = state.retry_after;
    let mut last_delay = 0.0;
    for _ in 0..12 {
        state = RoadSurveyBackoff::after_failure(Some(state), now);
        last_delay = state.retry_after - now;
        assert!(last_delay <= ROAD_SURVEY_RETRY_MAX_SECONDS);
        now = state.retry_after;
    }
    assert_eq!(last_delay, ROAD_SURVEY_RETRY_MAX_SECONDS);
}

#[test]
fn a_burst_of_reasserted_failed_routes_sleeps_until_real_time_backoff() {
    let mut app = road_test_app();
    app.init_resource::<Time>();
    app.init_resource::<VillageRoadGraph>();
    app.add_systems(
        Update,
        (
            queue_villager_travel_routes,
            retry_failed_routes_after_obstacle_change,
        )
            .chain(),
    );
    let goal = Vec3::new(40.0, 0.0, 20.0);
    let mut movers = Vec::new();
    for _ in 0..40 {
        let mover = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(Vec3::ZERO),
                MoveTarget(goal),
            ))
            .id();
        let backoff = NavigationRouteBackoff::after_failure(None, goal, 7, 0, 0.0, mover);
        app.world_mut().entity_mut(mover).insert(backoff);
        movers.push(mover);
    }

    // Reasserting the same target marks MoveTarget as changed, matching the
    // work-routine bug from the live 123-villager trace. None may bypass the
    // retained negative result before its real-time retry.
    for mover in movers.iter().copied() {
        app.world_mut().entity_mut(mover).insert(MoveTarget(goal));
    }
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_millis(500));
    app.update();
    assert_eq!(
        app.world_mut()
            .query::<&NavigationRoutePending>()
            .iter(app.world())
            .count(),
        0
    );

    // A genuinely different destination bypasses the old failure immediately.
    for mover in movers.iter().copied() {
        app.world_mut()
            .entity_mut(mover)
            .insert(MoveTarget(goal + Vec3::X));
    }
    app.update();
    assert_eq!(
        app.world_mut()
            .query::<&NavigationRoutePending>()
            .iter(app.world())
            .count(),
        40
    );
    assert!(movers
        .iter()
        .all(|mover| app.world().get::<NavigationRouteBackoff>(*mover).is_none()));
}

#[test]
fn an_orphaned_unchanged_move_target_reenters_the_route_queue() {
    let mut app = road_test_app();
    app.init_resource::<Time>();
    app.add_systems(Update, queue_villager_travel_routes);
    let goal = Vec3::new(24.0, 0.0, -12.0);
    let mover = app
        .world_mut()
        .spawn((CharacterKind::Villager, MoveTarget(goal)))
        .id();
    // Reproduce a route handoff which removed planner state but accidentally
    // left the authoritative target unchanged. Added/Changed must not be the
    // only way this order can enter the queue.
    app.world_mut().clear_trackers();

    app.update();

    assert_eq!(
        app.world()
            .get::<NavigationRoutePending>(mover)
            .map(|pending| pending.goal),
        Some(goal)
    );
}

#[test]
fn stale_moot_transit_cannot_suppress_a_later_route_request() {
    let mut app = road_test_app();
    app.init_resource::<Time>();
    app.add_systems(Update, queue_villager_travel_routes);
    let goal = Vec3::new(24.0, 0.0, -12.0);
    let mover = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            MoveTarget(goal),
            crate::world::village::MootQueueTransit,
        ))
        .id();
    app.world_mut().clear_trackers();

    app.update();

    assert_eq!(
        app.world()
            .get::<NavigationRoutePending>(mover)
            .map(|pending| pending.goal),
        Some(goal),
        "a consumed Moot ticket must release movement to the next routine"
    );
}

#[test]
fn unchanged_geometry_rejects_a_cached_failure_without_another_survey() {
    let mut app = road_test_app();
    app.init_resource::<Time>();
    app.insert_resource(WorldTerrain::default());
    app.init_resource::<VillageRoadGraph>();
    app.insert_resource(PathfindingBudgetSettings {
        max_requests_per_tick: 16,
        ..default()
    });
    app.add_systems(Update, plan_villager_travel_routes);

    let goal = Vec3::new(1720.0, 0.0, 20.0);
    let mover = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            PlayerPosition(Vec3::new(1700.0, 0.0, 0.0)),
            MoveTarget(goal),
            NavigationRoutePending::new(goal),
        ))
        .id();
    let obstacle_version = 0x9E37_79B9_7F4A_7C15;
    let backoff = NavigationRouteBackoff {
        goal,
        failures: 1,
        retry_after: 0.0,
        geometry_version: obstacle_version,
        road_opportunity_version: 0,
    };
    app.world_mut().entity_mut(mover).insert(backoff);

    app.update();

    let mover = app.world().entity(mover);
    assert!(mover.contains::<NavigationRouteFailed>());
    assert!(!mover.contains::<NavigationRoutePending>());
    assert_eq!(mover.get::<NavigationRouteBackoff>().unwrap().failures, 2);
}
use std::time::{Duration, Instant};

/// Diagnostic for the full generated map near a reported live settlement.
/// Kept ignored because it measures wall time rather than correctness.
#[test]
#[ignore = "diagnostic: run explicitly with --ignored --nocapture"]
fn real_world_village_route_profile() {
    let mut app = road_test_app();
    app.insert_resource(WorldTerrain::default());
    app.init_resource::<VillageRoadGraph>();
    app.init_resource::<SpatialObstacleGrid>();
    app.insert_resource(PathfindingBudgetSettings {
        max_requests_per_tick: 1,
        ..default()
    });
    app.add_systems(Update, plan_villager_travel_routes);

    let village_centre = Vec2::new(-346.0, 306.0);
    for (ring, count) in [(15.0_f32, 8_usize), (26.0, 12)] {
        for index in 0..count {
            let angle = index as f32 / count as f32 * std::f32::consts::TAU;
            let point = village_centre + Vec2::from_angle(angle) * ring;
            let position = Vec3::new(
                point.x,
                app.world()
                    .resource::<WorldTerrain>()
                    .get_height(point.x, point.y),
                point.y,
            );
            app.world_mut().spawn((
                PlacedBuilding {
                    building_type: BuildingType::LogCabin,
                    rotation: angle + std::f32::consts::PI,
                },
                BuildingPosition(position),
            ));
        }
    }

    let goals = [
        Vec2::new(-371.4, 311.4),
        Vec2::new(-343.1, 301.4),
        Vec2::new(-331.9, 278.2),
        Vec2::new(-343.7, 285.0),
        Vec2::new(-374.1, 310.7),
        Vec2::new(-346.2, 282.1),
        Vec2::new(-356.9, 281.1),
        Vec2::new(-350.2, 337.6),
        Vec2::new(-317.3, 299.5),
        Vec2::new(-374.7, 330.1),
        Vec2::new(-321.6, 302.9),
        Vec2::new(-361.1, 278.7),
    ];
    for (index, goal) in goals.into_iter().enumerate() {
        let start = Vec2::new(-345.0 + (index % 4) as f32 * 2.0, 309.0);
        let start = Vec3::new(
            start.x,
            app.world()
                .resource::<WorldTerrain>()
                .get_height(start.x, start.y),
            start.y,
        );
        let goal = Vec3::new(
            goal.x,
            app.world()
                .resource::<WorldTerrain>()
                .get_height(goal.x, goal.y),
            goal.y,
        );
        app.world_mut().spawn((
            CharacterKind::Villager,
            PlayerPosition(start),
            MoveTarget(goal),
            NavigationRoutePending::new(goal),
        ));
    }

    for tick in 0..6 {
        let started = Instant::now();
        app.update();
        let elapsed = started.elapsed();
        let pending = app
            .world_mut()
            .query::<&NavigationRoutePending>()
            .iter(app.world())
            .count();
        println!("REAL ROUTES tick={tick} elapsed={elapsed:?} pending={pending}");
    }

    let terrain = app.world().resource::<WorldTerrain>();
    let hall = Vec3::new(
        village_centre.x,
        terrain.get_height(village_centre.x, village_centre.y),
        village_centre.y,
    );
    let mut occupied = vec![(hall, SettlementBuildingKind::Hall.clearance())];
    let mut radial_roads = Vec::new();
    let mut houses = 0usize;
    while let Some((position, rotation)) = crate::world::village::find_site(
        terrain,
        hall,
        SettlementBuildingKind::House,
        &occupied,
        &radial_roads.iter().collect::<Vec<_>>(),
    ) {
        occupied.push((position, SettlementBuildingKind::House.clearance()));
        let door = SettlementBuildingKind::House.entrance_position(position, rotation);
        let hall_door = SettlementBuildingKind::Hall.entrance_position(hall, 0.0);
        radial_roads.push(VillageRoad {
            settlement: "Profile".into(),
            builder: format!("Builder {houses}"),
            points: vec![
                Vec2::new(door.x, door.z),
                Vec2::new(hall_door.x, hall_door.z),
            ],
            built_through: 2,
            width: VILLAGE_ROAD_WIDTH,
            reserved_width: RoadClass::Lane.initial_reserved_width(),
            surface: RoadSurface::Dirt,
            class: RoadClass::Lane,
            stone_committed: 0,
        });
        houses += 1;
        if houses >= 40 {
            break;
        }
    }
    println!("REAL SITING houses_with_radial_roads={houses}");
}

#[test]
fn a_failed_route_retries_only_after_its_destination_changes() {
    let mut app = road_test_app();
    app.init_resource::<VillageRoadGraph>();
    app.add_systems(Update, retry_failed_routes_after_obstacle_change);
    let goal = Vec3::new(20.0, 0.0, 12.0);
    let mover = app
        .world_mut()
        .spawn((
            PlayerPosition(Vec3::ZERO),
            MoveTarget(goal),
            NavigationRouteFailed { goal },
        ))
        .id();

    app.update();
    assert!(app.world().get::<NavigationRouteFailed>(mover).is_some());
    assert!(app.world().get::<NavigationRoutePending>(mover).is_none());

    app.world_mut()
        .entity_mut(mover)
        .insert(MoveTarget(goal + Vec3::X));
    app.update();
    assert!(app.world().get::<NavigationRouteFailed>(mover).is_none());
    assert!(app.world().get::<NavigationRoutePending>(mover).is_some());
}

#[test]
fn an_out_of_bounds_agent_goal_is_reported_before_route_survey() {
    let mut app = road_test_app();
    app.insert_resource(WorldTerrain::default());
    app.init_resource::<VillageRoadGraph>();
    app.insert_resource(PathfindingBudgetSettings {
        max_requests_per_tick: 16,
        ..default()
    });
    app.add_systems(Update, plan_villager_travel_routes);
    let invalid = Vec3::new(1.0e9, 0.0, 1.0e9);
    let mover = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            PlayerPosition(Vec3::ZERO),
            MoveTarget(invalid),
            NavigationRoutePending::new(invalid),
        ))
        .id();

    app.update();

    let mover = app.world().entity(mover);
    assert!(!mover.contains::<MoveTarget>());
    assert!(!mover.contains::<NavigationRoutePending>());
    assert!(!mover.contains::<TravelRoute>());
    assert_eq!(
        mover.get::<NavigationRouteFailed>().unwrap().goal,
        invalid,
        "the owning task needs a failure signal so it can choose a valid destination"
    );
}

#[test]
fn survey_wraps_around_a_building_instead_of_crossing_it() {
    let terrain = WorldTerrain::default();
    let mut scratch = SurveyScratch::default();
    let start = Vec2::new(1700.0, 0.0);
    let goal = Vec2::new(1740.0, 0.0);
    let blocker = BuildingBlocker {
        center: Vec2::new(1720.0, 0.0),
        half: Vec2::new(5.0, 7.0),
        rotation: 0.0,
    };
    let path = survey_village_road(
        &terrain,
        start,
        goal,
        &[blocker],
        &PropBlockers::default(),
        7,
        &mut scratch,
    );
    assert!(path.len() > 2, "the path needs a bend: {path:?}");
    assert!(path.iter().all(|point| !blocker.contains(*point)));
    assert!(path.windows(2).all(|pair| {
        let steps = (pair[0].distance(pair[1]) / 0.25).ceil() as usize;
        (0..=steps)
            .all(|step| !blocker.contains(pair[0].lerp(pair[1], step as f32 / steps.max(1) as f32)))
    }));
}

#[test]
fn farmer_can_walk_from_front_door_around_farmstead_to_rear_field() {
    let terrain = WorldTerrain::default();
    // Preserve the exact relative geometry from the lab regression while
    // translating it onto the generated world's stable dry test plateau.
    let center = Vec2::new(1700.0, 0.0);
    let start = center + Vec2::new(3.66172, -0.18534);
    let goal = center + Vec2::new(-8.83977, 3.40013);
    let blocker = BuildingBlocker {
        center,
        half: Vec2::new(2.985, 3.59),
        rotation: -1.520343,
    };
    let mut live = SpatialObstacleGrid::new();
    live.insert(shared::spatial::ObstacleEntry {
        center: blocker.center,
        half_extents: blocker.half,
        rotation: blocker.rotation,
        obstacle_type: 0,
    });

    let building = NavigationBuilding {
        blocker,
        kind: SettlementBuildingKind::Farmstead,
        position: Vec3::new(blocker.center.x, 0.0, blocker.center.y),
        rotation: blocker.rotation,
    };
    let start_endpoint = navigation_endpoint(start, &[building]);
    let goal_endpoint = navigation_endpoint(goal, &[building]);
    let mut scratch = SurveyScratch::default();
    let start_apron = survey_agent_route(
        &terrain,
        start_endpoint.actual,
        start_endpoint.survey,
        &[blocker],
        Some(&live),
        &PropBlockers::default(),
        &mut scratch,
        EXTENDED_LOCAL_SURVEY_MAX_NODES,
    );
    let middle = survey_agent_route(
        &terrain,
        start_endpoint.survey,
        goal_endpoint.survey,
        &[blocker],
        Some(&live),
        &PropBlockers::default(),
        &mut scratch,
        EXTENDED_LOCAL_SURVEY_MAX_NODES,
    );
    let goal_apron = survey_agent_route(
        &terrain,
        goal_endpoint.survey,
        goal_endpoint.actual,
        &[blocker],
        Some(&live),
        &PropBlockers::default(),
        &mut scratch,
        EXTENDED_LOCAL_SURVEY_MAX_NODES,
    );
    let route = complete_agent_route(
        start_endpoint,
        goal_endpoint,
        &start_apron,
        middle,
        &goal_apron,
    );
    assert!(
        route.len() >= 2,
        "a farmer released outside the front door must route around the Farmstead to field 2"
    );
    assert!(route
        .windows(2)
        .all(|edge| !live.segment_blocked(edge[0], edge[1])));
}

#[test]
fn farm_work_stand_is_certified_from_the_authored_front_door() {
    let terrain = WorldTerrain::default();
    let x = 1700.0;
    let z = 0.0;
    let farm = Vec3::new(x, terrain.get_height(x, z), z);
    let rotation = 0.0;
    let field = SettlementBuildingKind::Farmstead
        .field_position_at(farm, rotation, 0)
        .unwrap();
    let definition = SettlementBuildingKind::Farmstead.art().definition();
    let mut obstacles = SpatialObstacleGrid::new();
    obstacles.insert(shared::spatial::ObstacleEntry {
        center: Vec2::new(farm.x, farm.z),
        half_extents: definition.footprint * 0.5 + Vec2::splat(CHARACTER_NAV_RADIUS),
        rotation,
        obstacle_type: 0,
    });

    let stand = reachable_farm_work_stand(
        &terrain,
        farm,
        rotation,
        field,
        0,
        Some(&obstacles),
        None,
        None,
    );
    assert!(
        stand.is_some(),
        "the normal authored Farmstead must retain a certified side corridor"
    );
}

#[test]
fn route_endpoint_marks_a_near_door_actor_for_bounded_building_escape() {
    let center = Vec2::new(100.0, -300.0);
    let rotation = -0.7;
    let kind = SettlementBuildingKind::Tavern;
    let position = Vec3::new(center.x, 0.0, center.y);
    let blocker = BuildingBlocker {
        center,
        half: Vec2::new(4.28, 3.78),
        rotation,
    };
    let building = NavigationBuilding {
        blocker,
        kind,
        position,
        rotation,
    };
    let door = kind.entrance_position(position, rotation);
    let inward = (center - Vec2::new(door.x, door.z)).normalize();
    let trapped = Vec2::new(door.x, door.z) + inward * 1.7;
    assert!(blocker.contains(trapped));

    let endpoint = navigation_endpoint(trapped, &[building]);

    assert!(endpoint.escaping_building);
    assert_ne!(endpoint.actual, endpoint.survey);
    assert!(!blocker.contains(endpoint.survey));
}

#[test]
fn route_endpoint_recovers_an_actor_deep_inside_an_authored_building() {
    let center = Vec2::new(100.0, -300.0);
    let rotation = -0.7;
    let kind = SettlementBuildingKind::FishermansHut;
    let position = Vec3::new(center.x, 0.0, center.y);
    let blocker = BuildingBlocker {
        center,
        half: Vec2::splat(4.0),
        rotation,
    };
    let building = NavigationBuilding {
        blocker,
        kind,
        position,
        rotation,
    };

    let endpoint = navigation_endpoint(center, &[building]);

    assert!(endpoint.escaping_building);
    assert_eq!(endpoint.actual, center);
    assert!(!blocker.contains(endpoint.survey));
}

#[test]
fn surveyed_road_keeps_its_full_ribbon_out_of_a_wheat_field() {
    let terrain = WorldTerrain::default();
    let mut scratch = SurveyScratch::default();
    let start = Vec2::new(1700.0, 0.0);
    let goal = Vec2::new(1740.0, 0.0);
    let field_center = Vec2::new(1720.0, 0.0);
    let field_rotation = 0.37;
    let field_half = SettlementBuildingKind::Farmstead
        .field_half_extents()
        .unwrap();
    let blocker = BuildingBlocker {
        center: field_center,
        half: field_half
            + Vec2::splat(
                RoadClass::Lane.initial_reserved_width() * 0.5
                    + shared::components::FARM_FIELD_EDGE_CLEARANCE
                    + ROAD_SURVEY_FIELD_EPSILON,
            ),
        rotation: field_rotation,
    };
    let path = survey_village_road(
        &terrain,
        start,
        goal,
        &[blocker],
        &PropBlockers::default(),
        17,
        &mut scratch,
    );
    assert!(path.len() > 2, "the crop needs a visible detour: {path:?}");

    let road = VillageRoad {
        settlement: "Fieldford".into(),
        builder: "Mara".into(),
        built_through: path.len() as u16,
        points: path,
        width: VILLAGE_ROAD_WIDTH,
        reserved_width: RoadClass::Lane.initial_reserved_width(),
        surface: RoadSurface::Dirt,
        class: RoadClass::Lane,
        stone_committed: 0,
    };
    assert!(
        !road.intersects_rotated_rect(
            field_center,
            field_half,
            field_rotation,
            shared::components::FARM_FIELD_EDGE_CLEARANCE,
        ),
        "the surveyed road ribbon clipped the authored 8x11 metre crop: {:?}",
        road.points
    );
}

#[test]
fn a_farmstead_retains_its_road_request_until_both_fields_exist() {
    let mut app = road_test_app();
    app.insert_resource(WorldTerrain::default());
    app.add_systems(Update, plan_requested_roads);
    let settlement = app
        .world_mut()
        .spawn((
            shared::components::SettlementId(1),
            Settlement {
                name: "Fieldford".into(),
                tier: SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            },
            PlayerPosition(Vec3::new(1_700.0, 0.0, 0.0)),
            PlayerRotation(0.0),
        ))
        .id();
    let position = Vec3::new(1_740.0, 0.0, 0.0);
    let builder = app
        .world_mut()
        .spawn((
            CharacterName("Mara".into()),
            CharacterKind::Villager,
            CharacterActivity::Idle,
            PlayerPosition(position),
            VillagerIntent::Building {
                settlement,
                site: Entity::PLACEHOLDER,
            },
        ))
        .id();
    let farm = app
        .world_mut()
        .spawn((
            SettlementBuilding {
                kind: SettlementBuildingKind::Farmstead,
                settlement: "Fieldford".into(),
                owner: Some("Mara".into()),
                quality: 0.8,
                workers: Vec::new(),
            },
            PlayerPosition(position),
            PlayerRotation(0.0),
            RoadRequest {
                builder,
                settlement,
                completed_site: Entity::PLACEHOLDER,
                attempt: 0,
            },
        ))
        .id();

    app.update();

    assert!(app.world().get::<RoadRequest>(farm).is_some());
    assert!(app.world().get::<RoadBuilderRoutine>(builder).is_none());
    assert_eq!(
        app.world_mut()
            .query::<&VillageRoad>()
            .iter(app.world())
            .count(),
        0
    );

    let first_field = SettlementBuildingKind::Farmstead
        .field_position_at(position, 0.0, 0)
        .expect("Farmstead has its first field");
    app.world_mut().spawn((
        FarmField {
            settlement: "Fieldford".into(),
            farmstead: position,
            plot_index: 0,
            quality: 0.8,
        },
        PlayerPosition(first_field),
        PlayerRotation(0.0),
    ));
    app.update();

    assert!(
        app.world().get::<RoadRequest>(farm).is_some(),
        "one field is still an incomplete Farmstead layout"
    );
    assert!(app.world().get::<RoadBuilderRoutine>(builder).is_none());
}

#[test]
fn inland_river_above_sea_level_is_not_dry_road_ground() {
    let terrain = WorldTerrain::default();
    let ocean = terrain.water_level().expect("generated world has water");
    let river_point = terrain
        .rivers()
        .iter()
        .flatten()
        .find(|point| {
            terrain
                .water_surface_height(point.x, point.z)
                .is_some_and(|surface| {
                    surface > ocean + 0.2 && terrain.get_height(point.x, point.z) < surface
                })
        })
        .expect("generated world has an inland river");

    assert!(!road_sample_is_dry(
        &terrain,
        Vec2::new(river_point.x, river_point.z),
    ));
}

#[test]
fn coarse_site_ranking_still_detects_a_generated_river_crossing() {
    let terrain = WorldTerrain::default();
    let ocean = terrain.water_level().expect("generated world has water");
    let (midpoint, direction) = terrain
        .rivers()
        .iter()
        .flat_map(|river| river.windows(2))
        .find_map(|pair| {
            let a = Vec2::new(pair[0].x, pair[0].z);
            let b = Vec2::new(pair[1].x, pair[1].z);
            let midpoint = a.lerp(b, 0.5);
            terrain
                .water_surface_height(midpoint.x, midpoint.y)
                .is_some_and(|surface| {
                    surface > ocean + 0.2 && terrain.get_height(midpoint.x, midpoint.y) < surface
                })
                .then_some((midpoint, (b - a).normalize_or_zero()))
        })
        .expect("generated world has an inland river segment");
    let across = Vec2::new(-direction.y, direction.x);

    assert!(across.length_squared() > 0.9);
    assert!(!road_segment_is_coarsely_dry(
        &terrain,
        midpoint - across * 12.0,
        midpoint + across * 12.0,
    ));
}

#[test]
fn agent_survey_uses_the_live_obstacle_grid_as_a_hard_constraint() {
    let terrain = WorldTerrain::default();
    let mut scratch = SurveyScratch::default();
    let mut grid = SpatialObstacleGrid::default();
    grid.insert(shared::spatial::ObstacleEntry {
        center: Vec2::new(1720.0, 0.0),
        half_extents: Vec2::new(2.985, 3.59),
        rotation: -std::f32::consts::FRAC_PI_4,
        obstacle_type: 0,
    });
    let route = survey_agent_route(
        &terrain,
        Vec2::new(1724.5452, -1.7452),
        Vec2::new(1712.5754, 5.3033),
        &[],
        Some(&grid),
        &PropBlockers::default(),
        &mut scratch,
        AGENT_SURVEY_MAX_NODES,
    );

    assert!(!route.is_empty());
    assert!(route.windows(2).all(|segment| {
        let steps = (segment[0].distance(segment[1]) / NAVIGATION_SAMPLE_STEP)
            .ceil()
            .max(1.0) as usize;
        (0..=steps).all(|step| {
            !grid.point_blocked(segment[0].lerp(segment[1], step as f32 / steps as f32))
        })
    }));
}

#[test]
fn graph_reuses_one_cached_route_for_both_directions() {
    let mut graph = VillageRoadGraph::default();
    graph.nodes = vec![
        RoadGraphNode {
            point: Vec2::ZERO,
            edges: vec![(1, 5.0)],
        },
        RoadGraphNode {
            point: Vec2::X * 5.0,
            edges: vec![(0, 5.0), (2, 5.0)],
        },
        RoadGraphNode {
            point: Vec2::X * 10.0,
            edges: vec![(1, 5.0)],
        },
    ];
    assert_eq!(graph.shortest_path(0, 2), Some(vec![0, 1, 2]));
    assert_eq!(graph.routes.len(), 2);
    assert_eq!(graph.shortest_path(2, 0), Some(vec![2, 1, 0]));
    assert_eq!(graph.routes.len(), 2, "reverse lookup should hit the cache");
}

#[test]
fn many_origins_share_one_destination_road_tree() {
    let mut graph = VillageRoadGraph::default();
    graph.nodes = vec![
        RoadGraphNode {
            point: Vec2::ZERO,
            edges: vec![(1, 5.0)],
        },
        RoadGraphNode {
            point: Vec2::X * 5.0,
            edges: vec![(0, 5.0), (2, 5.0)],
        },
        RoadGraphNode {
            point: Vec2::X * 10.0,
            edges: vec![(1, 5.0)],
        },
    ];

    assert_eq!(graph.shortest_path(0, 2), Some(vec![0, 1, 2]));
    assert_eq!(graph.shortest_path(1, 2), Some(vec![1, 2]));
    assert_eq!(
        graph.destination_trees.len(),
        1,
        "a second commuter to the same door rebuilt Dijkstra"
    );
}

#[test]
fn embodied_route_rejection_discards_stale_tactical_answers() {
    let mut graph = VillageRoadGraph::default();
    let start = Vec2::new(2.0, 4.0);
    let goal = Vec2::new(18.0, -6.0);
    let route = [(start, false), (goal, false)];
    graph.cache_tactical_route(start, goal, &route, true);
    let unrelated_start = Vec2::new(200.0, 200.0);
    let unrelated_goal = Vec2::new(220.0, 200.0);
    graph.cache_tactical_route(
        unrelated_start,
        unrelated_goal,
        &[(unrelated_start, false), (unrelated_goal, false)],
        true,
    );
    assert!(graph.tactical_route(start, goal).is_some());
    assert!(graph.tactical_route(goal, start).is_some());

    graph.invalidate_tactical_routes_after_embodied_rejection(start, goal);

    assert!(graph.tactical_route(start, goal).is_none());
    assert!(graph.tactical_route(goal, start).is_none());
    assert!(graph
        .tactical_route(unrelated_start, unrelated_goal)
        .is_some());
}

#[test]
fn road_growth_preserves_safe_routes_and_only_wakes_nearby_failures() {
    let mut graph = VillageRoadGraph::default();
    let start = Vec2::new(10.0, 10.0);
    let goal = Vec2::new(90.0, 10.0);
    let route = vec![(start, false), (goal, false)];
    graph.cache_tactical_route(start, goal, &route, true);
    let original_opportunity = graph.route_opportunity_version(start, goal);

    graph.note_road_opportunity(Vec2::new(900.0, 900.0));
    assert_eq!(
        graph.route_opportunity_version(start, goal),
        original_opportunity,
        "a road on the other side of the world must not wake this route"
    );
    assert!(
        graph.tactical_route(start, goal).is_some(),
        "adding walkable road geometry cannot invalidate a certified route"
    );

    graph.note_road_opportunity(goal + Vec2::X * 5.0);
    assert!(graph.route_opportunity_version(start, goal) > original_opportunity);
    assert!(graph.tactical_route(start, goal).is_some());
}

#[test]
fn extended_agent_search_yields_and_resumes_instead_of_monopolising_a_tick() {
    let terrain = WorldTerrain::default();
    let start = Vec2::new(1600.0, 0.0);
    let goal = Vec2::new(1900.0, 0.0);
    let props = PropBlockers::default();
    let survey = RoadSurvey {
        terrain: &terrain,
        buildings: &[],
        live_buildings: None,
        props: &props,
        start,
        goal,
        min: start.min(goal) - Vec2::splat(SURVEY_PADDING),
        max: start.max(goal) + Vec2::splat(SURVEY_PADDING),
        max_nodes: EXTENDED_LOCAL_SURVEY_MAX_NODES,
        cell_size: SURVEY_CELL,
        coarse_stride: 1,
        fine_endpoint_radius: 0.0,
    };
    let mut scratch = SurveyScratch::default();
    let mut state = SurveySearchState::default();
    assert!(matches!(
        resume_survey_a_star(&survey, &mut scratch, &mut state, Some(Instant::now())),
        SurveySearchResult::Pending
    ));
    assert_eq!(
        state.expanded, MIN_INCREMENTAL_ROUTE_CELLS_PER_SLICE,
        "an expired slice makes its bounded latency-guaranteeing progress rather than running the whole A*"
    );
    assert!(matches!(
        resume_survey_a_star(&survey, &mut scratch, &mut state, None),
        SurveySearchResult::Found(_) | SurveySearchResult::Failed
    ));
    assert!(state.expanded > 1);
}

#[test]
fn tactical_cache_reuses_reverse_commutes_until_geometry_changes() {
    let mut graph = VillageRoadGraph::default();
    let home = Vec2::new(10.0, -3.0);
    let work = Vec2::new(24.0, 8.0);
    let route = vec![(home, false), (Vec2::new(16.0, 1.0), true), (work, false)];

    graph.cache_tactical_route(home, work, &route, true);
    assert_eq!(graph.tactical_route(home, work), Some(route.as_slice()));

    let mut reverse = route.clone();
    reverse.reverse();
    assert_eq!(graph.tactical_route(work, home), Some(reverse.as_slice()));

    let far_blocker = BuildingBlocker {
        center: Vec2::splat(500.0),
        half: Vec2::splat(2.0),
        rotation: 0.0,
    };
    graph.invalidate_tactical_routes_near_buildings(&[far_blocker]);
    assert!(
        graph.tactical_route(home, work).is_some(),
        "an unrelated building change must preserve the certified commute"
    );
    let intersecting_blocker = BuildingBlocker {
        center: Vec2::new(16.0, 1.0),
        half: Vec2::splat(1.0),
        rotation: 0.0,
    };
    graph.invalidate_tactical_routes_near_buildings(&[intersecting_blocker]);
    assert!(
        graph.tactical_route(home, work).is_none(),
        "a building intersecting the polyline must invalidate that commute"
    );
}

#[test]
fn tactical_cache_does_not_reverse_an_asymmetric_prop_exemption() {
    let home = Vec2::ZERO;
    let work = Vec2::X * 4.0;
    let route = vec![(home, false), (work, false)];
    let mut props = PropBlockers::default();
    props.insert_radius(Vec2::X * 3.0, 0.45);
    assert!(!reverse_route_clears_goal_prop_exemption(&route, &props));

    let mut graph = VillageRoadGraph::default();
    graph.cache_tactical_route(home, work, &route, false);
    assert!(graph.tactical_route(home, work).is_some());
    assert!(
        graph.tactical_route(work, home).is_none(),
        "the reverse trip needs its own survey when the forward goal used a prop exemption"
    );
}

#[test]
fn survey_memoizes_repeated_geometry_checks_within_one_search() {
    let terrain = WorldTerrain::default();
    let mut scratch = SurveyScratch::default();
    let props = PropBlockers::default();
    let survey = RoadSurvey {
        terrain: &terrain,
        buildings: &[],
        live_buildings: None,
        props: &props,
        start: Vec2::new(1700.0, 0.0),
        goal: Vec2::new(1710.0, 0.0),
        min: Vec2::new(1680.0, -20.0),
        max: Vec2::new(1730.0, 20.0),
        max_nodes: AGENT_SURVEY_MAX_NODES,
        cell_size: SURVEY_CELL,
        coarse_stride: 1,
        fine_endpoint_radius: 0.0,
    };
    scratch.begin_search();
    let first = survey.line_clear(survey.start, survey.goal, &mut scratch);
    let hits_before = scratch.metrics.line_cache_hits;
    let second = survey.line_clear(survey.start, survey.goal, &mut scratch);

    assert_eq!(first, second);
    assert_eq!(scratch.metrics.line_cache_hits, hits_before + 1);
}

#[test]
fn villager_route_uses_the_road_and_never_crosses_a_building() {
    let mut app = road_test_app();
    app.insert_resource(WorldTerrain::default());
    app.insert_resource(PathfindingBudgetSettings {
        max_requests_per_tick: 8,
        ..default()
    });

    let road_points = [
        Vec2::new(1700.0, 0.0),
        Vec2::new(1710.0, 10.0),
        Vec2::new(1730.0, 10.0),
        Vec2::new(1740.0, 0.0),
    ];
    let mut graph = VillageRoadGraph::default();
    for point in road_points {
        graph.nodes.push(RoadGraphNode {
            point,
            edges: Vec::new(),
        });
    }
    for index in 0..graph.nodes.len() - 1 {
        let distance = graph.nodes[index]
            .point
            .distance(graph.nodes[index + 1].point);
        graph.nodes[index].edges.push((index + 1, distance));
        graph.nodes[index + 1].edges.push((index, distance));
    }
    graph.initialized = true;
    app.insert_resource(graph);
    app.add_systems(
        Update,
        (queue_villager_travel_routes, plan_villager_travel_routes).chain(),
    );

    let building_type = BuildingType::LogCabin;
    let building_position = Vec3::new(1720.0, 0.0, 0.0);
    app.world_mut().spawn((
        PlacedBuilding {
            building_type,
            rotation: 0.0,
        },
        BuildingPosition(building_position),
    ));
    let start = Vec3::new(1700.0, 0.0, 0.0);
    let goal = Vec3::new(1740.0, 0.0, 0.0);
    let villager = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            PlayerPosition(start),
            MoveTarget(goal),
        ))
        .id();

    app.update();

    let route = app.world().get::<TravelRoute>(villager).unwrap();
    assert!(
        route.waypoints.iter().any(|waypoint| waypoint.on_road),
        "the safe precomputed road should beat a fresh direct detour"
    );
    assert!(!app
        .world()
        .entity(villager)
        .contains::<NavigationRoutePending>());

    let footprint = building_type.definition().footprint;
    let blocker = BuildingBlocker {
        center: Vec2::new(building_position.x, building_position.z),
        half: footprint * 0.5 + Vec2::splat(shared::physics::CHARACTER_NAV_RADIUS),
        rotation: 0.0,
    };
    let mut points = vec![Vec2::new(start.x, start.z)];
    points.extend(
        route
            .waypoints
            .iter()
            .map(|waypoint| Vec2::new(waypoint.position.x, waypoint.position.z)),
    );
    assert!(points.windows(2).all(|pair| {
        let steps = (pair[0].distance(pair[1]) / 0.2).ceil().max(1.0) as usize;
        (0..=steps).all(|step| !blocker.contains(pair[0].lerp(pair[1], step as f32 / steps as f32)))
    }));
}

#[test]
fn bounded_route_planning_serves_every_pending_villager_fairly() {
    let mut app = road_test_app();
    app.insert_resource(WorldTerrain::default());
    app.insert_resource(PathfindingBudgetSettings {
        max_requests_per_tick: 1,
        ..default()
    });
    app.init_resource::<VillageRoadGraph>();
    app.add_systems(
        Update,
        (queue_villager_travel_routes, plan_villager_travel_routes).chain(),
    );

    let villagers: Vec<_> = (0..3)
        .map(|index| {
            let start = Vec3::new(1700.0, 0.0, index as f32 * 4.0);
            app.world_mut()
                .spawn((
                    CharacterKind::Villager,
                    PlayerPosition(start),
                    MoveTarget(start + Vec3::X * 12.0),
                ))
                .id()
        })
        .collect();

    for _ in 0..villagers.len() {
        app.update();
    }

    assert!(villagers.iter().all(|entity| {
        app.world().get::<TravelRoute>(*entity).is_some()
            && app.world().get::<NavigationRoutePending>(*entity).is_none()
    }));
}

#[test]
fn a_retained_long_route_cannot_pin_the_queue_ahead_of_a_short_worker_route() {
    let mut app = road_test_app();
    app.insert_resource(WorldTerrain::default());
    app.insert_resource(PathfindingBudgetSettings {
        max_requests_per_tick: 1,
        ..default()
    });
    app.init_resource::<VillageRoadGraph>();
    app.add_systems(
        Update,
        (queue_villager_travel_routes, plan_villager_travel_routes).chain(),
    );

    let long_start = Vec3::new(1_700.0, 0.0, 0.0);
    let long = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            PlayerPosition(long_start),
            MoveTarget(long_start + Vec3::X * 300.0),
        ))
        .id();
    let short_start = Vec3::new(1_700.0, 0.0, 8.0);
    let short = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            PlayerPosition(short_start),
            MoveTarget(short_start + Vec3::X * 12.0),
        ))
        .id();

    app.update();
    assert!(app.world().get::<NavigationRoutePending>(long).is_some());
    app.update();

    assert!(app.world().get::<TravelRoute>(short).is_some());
    assert!(app.world().get::<NavigationRoutePending>(short).is_none());
}

#[test]
fn the_building_builder_owns_and_finishes_its_road_at_100x() {
    let mut app = road_test_app();
    app.init_resource::<Time>();
    app.insert_resource(WorldTerrain::default());
    app.add_systems(
        Update,
        (
            crate::world::village::claim_settlement_hall_obstacles,
            plan_requested_roads,
            build_village_roads,
            crate::player::hero::step_units,
        )
            .chain(),
    );

    let hall_position = Vec3::new(1700.0, 0.0, 0.0);
    let settlement = app
        .world_mut()
        .spawn((
            shared::components::SettlementId(1),
            Settlement {
                name: "Oakmead".into(),
                tier: SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            },
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
        ))
        .id();
    app.world_mut().spawn(TimeWarp(100.0));

    let house_position = Vec3::new(1740.0, 0.0, 0.0);
    let house_door = SettlementBuildingKind::House.entrance_position(house_position, 0.0);
    let builder = app
        .world_mut()
        .spawn((
            CharacterName("Mara".into()),
            CharacterKind::Villager,
            CharacterActivity::Idle,
            PlayerPosition(house_door),
            PlayerRotation(0.0),
            RegionCoord::from_world_pos(house_door),
            VillagerIntent::Building {
                settlement,
                site: Entity::PLACEHOLDER,
            },
        ))
        .id();
    let house = app
        .world_mut()
        .spawn((
            shared::components::BuildingId(78),
            shared::components::BuildingOf(shared::components::SettlementId(1)),
            SettlementBuilding {
                kind: SettlementBuildingKind::House,
                settlement: "Oakmead".into(),
                owner: Some("Mara".into()),
                quality: 0.7,
                workers: Vec::new(),
            },
            PlayerPosition(house_position),
            PlayerRotation(0.0),
            RoadRequest {
                builder,
                settlement,
                completed_site: Entity::PLACEHOLDER,
                attempt: 0,
            },
            PlannedRoadAccess {
                settlement_id: shared::components::SettlementId(1),
                points: vec![
                    Vec2::new(house_door.x, house_door.z),
                    Vec2::new(hall_position.x, hall_position.z),
                ],
                half_width: RoadClass::Lane.initial_reserved_width() * 0.5,
            },
        ))
        .id();
    let farmstead_position = Vec3::new(1720.0, 0.0, 4.5);
    app.world_mut().spawn((
        shared::components::BuildingId(77),
        shared::components::BuildingOf(shared::components::SettlementId(1)),
        SettlementBuilding {
            kind: SettlementBuildingKind::Farmstead,
            settlement: "Oakmead".into(),
            owner: Some("Farmer".into()),
            quality: 0.7,
            workers: Vec::new(),
        },
        PlayerPosition(farmstead_position),
        PlayerRotation(0.0),
    ));
    let field_position = Vec3::new(1720.0, 0.0, -4.5);
    app.world_mut().spawn((
        FarmField {
            settlement: "Oakmead".into(),
            farmstead: Vec3::new(1720.0, 0.0, 4.5),
            plot_index: 0,
            quality: 0.7,
        },
        shared::components::AttachedTo(shared::components::BuildingId(77)),
        PlayerPosition(field_position),
        PlayerRotation(0.0),
    ));

    let mut saw_building_animation = false;
    for _ in 0..180 {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs_f32(
                1.0 / shared::protocol::FIXED_TIMESTEP_HZ as f32,
            ));
        app.update();
        saw_building_animation |= app
            .world()
            .get::<CharacterActivity>(builder)
            .is_some_and(|activity| *activity == CharacterActivity::Building);
        if app.world().get::<RoadBuilderRoutine>(builder).is_none()
            && app
                .world()
                .iter_entities()
                .any(|entity| entity.contains::<VillageRoad>())
        {
            break;
        }
    }

    let mut roads = app.world_mut().query::<&VillageRoad>();
    let road = roads.single(app.world()).unwrap();
    let hall_door = SettlementBuildingKind::Hall.entrance_position(hall_position, 0.0);
    assert_eq!(road.builder, "Mara");
    assert!(
        road.is_complete(),
        "road stopped at {}/{}",
        road.built_through,
        road.points.len()
    );
    assert!(road.points[0].distance(Vec2::new(house_door.x, house_door.z)) < 0.01);
    assert!(
        road.points.len() > 4
            && road
                .points
                .windows(2)
                .all(|segment| segment[0].distance(segment[1]) <= 3.0),
        "a connector regressed to a giant straight construction segment: {:?}",
        road.points
    );
    assert!(
        road.points
            .last()
            .unwrap()
            .distance(Vec2::new(hall_door.x, hall_door.z))
            < 0.01
    );
    let hall_blocker = BuildingBlocker {
        center: shared::components::CivicHallLevel::reserved_world_center(hall_position, 0.0),
        half: shared::components::CivicHallLevel::reserved_half_extents(),
        rotation: 0.0,
    };
    assert!(
        road.points.windows(2).all(|pair| {
            let steps = (pair[0].distance(pair[1]) / 0.2).ceil().max(1.0) as usize;
            (0..=steps).all(|step| {
                !hall_blocker.contains(pair[0].lerp(pair[1], step as f32 / steps as f32))
            })
        }),
        "the road crossed the hall footprint: {:?}",
        road.points
    );
    assert!(
        !road.intersects_rotated_rect(
            Vec2::new(field_position.x, field_position.z),
            SettlementBuildingKind::Farmstead
                .field_half_extents()
                .unwrap(),
            0.0,
            shared::components::FARM_FIELD_EDGE_CLEARANCE,
        ),
        "the road crossed the planted wheat field: {:?}",
        road.points
    );
    assert!(
        saw_building_animation,
        "road work never exposed its build animation"
    );
    assert!(matches!(
        app.world().get::<VillagerIntent>(builder),
        Some(VillagerIntent::Resident { settlement: home }) if *home == settlement
    ));
    assert!(
        app.world().get::<PlannedRoadAccess>(house).is_none(),
        "the access reservation outlived its completed physical connector"
    );
}

#[test]
fn road_builder_chops_an_obstructing_tree_before_laying_the_ribbon() {
    let mut app = road_test_app();
    app.init_resource::<Time>();
    app.insert_resource(WorldTerrain::default());
    let tree = Vec2::new(1_702.0, 0.0);
    let (colliders, derived) = one_static_prop(
        PropKind::BroadleafLargeA,
        Vec3::new(tree.x, 0.0, tree.y),
        0.5,
    );
    app.insert_resource(colliders);
    app.insert_resource(derived);
    app.add_systems(
        Update,
        (build_village_roads, crate::player::hero::step_units).chain(),
    );
    app.world_mut().spawn(TimeWarp(100.0));

    let settlement = app.world_mut().spawn_empty().id();
    let start = Vec2::new(1_700.0, 0.0);
    let end = Vec2::new(1_704.0, 0.0);
    let road = app
        .world_mut()
        .spawn((
            VillageRoad {
                settlement: "Timbercross".into(),
                builder: "Mara".into(),
                points: vec![start, end],
                built_through: 1,
                width: VILLAGE_ROAD_WIDTH,
                reserved_width: RoadClass::Lane.initial_reserved_width(),
                surface: RoadSurface::Dirt,
                class: RoadClass::Lane,
                stone_committed: 0,
            },
            shared::components::RoadOf(shared::components::SettlementId(1)),
            RoadTreeClearancePlan {
                trees: vec![RoadTreeObstruction {
                    point: tree,
                    radius: 0.5,
                }],
            },
        ))
        .id();
    let builder_position = Vec3::new(start.x, 0.0, start.y);
    let builder = app
        .world_mut()
        .spawn((
            CharacterName("Mara".into()),
            CharacterKind::Villager,
            CharacterActivity::Idle,
            PlayerPosition(builder_position),
            PlayerRotation(0.0),
            RegionCoord::from_world_pos(builder_position),
            VillagerIntent::RoadBuilding { settlement, road },
            RoadBuilderRoutine {
                road,
                settlement,
                attempt: 0,
                phase: RoadBuildPhase::GoingTo { point: 1 },
            },
        ))
        .id();

    let mut saw_chopping = false;
    for _ in 0..180 {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs_f32(
                1.0 / shared::protocol::FIXED_TIMESTEP_HZ as f32,
            ));
        app.update();
        saw_chopping |= app
            .world()
            .get::<CharacterActivity>(builder)
            .is_some_and(|activity| *activity == CharacterActivity::Chopping);
        if app.world().get::<RoadBuilderRoutine>(builder).is_none() {
            break;
        }
    }

    assert!(saw_chopping, "the worker never played the chopping job");
    assert!(
        app.world()
            .resource::<StaticColliders>()
            .instances
            .is_empty(),
        "the felled tree remained in authoritative collision"
    );
    assert!(
        app.world()
            .resource::<StaticColliders>()
            .road_tree_was_cleared(tree),
        "collider streaming could resurrect the felled tree before the ribbon settled"
    );
    assert!(app.world().get::<VillageRoad>(road).unwrap().is_complete());
    assert!(
        app.world()
            .get::<RoadTreeClearancePlan>(road)
            .unwrap()
            .trees
            .is_empty(),
        "the completed tree task remained queued"
    );
}

#[test]
fn a_building_beside_the_network_still_gets_its_own_short_road() {
    let mut app = road_test_app();
    app.insert_resource(WorldTerrain::default());
    app.add_systems(Update, plan_requested_roads);

    let hall_position = Vec3::new(1700.0, 0.0, 0.0);
    let settlement = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Oakmead".into(),
                tier: SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            },
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
        ))
        .id();

    let house_position = Vec3::new(1740.0, 0.0, 0.0);
    let house_door = SettlementBuildingKind::House.entrance_position(house_position, 0.0);
    let nearby_network = Vec2::new(house_door.x, house_door.z - 1.6);
    let hall_door3 = SettlementBuildingKind::Hall.entrance_position(hall_position, 0.0);
    let hall_door = Vec2::new(hall_door3.x, hall_door3.z);
    assert!(nearby_network.distance(Vec2::new(house_door.x, house_door.z)) < 2.0);
    app.world_mut().spawn(VillageRoad {
        settlement: "Oakmead".into(),
        builder: "EarlierBuilder".into(),
        points: vec![nearby_network, hall_door],
        built_through: 2,
        width: VILLAGE_ROAD_WIDTH,
        reserved_width: RoadClass::Lane.initial_reserved_width(),
        surface: RoadSurface::Dirt,
        class: RoadClass::Lane,
        stone_committed: 0,
    });

    let builder = app
        .world_mut()
        .spawn((
            CharacterName("NearBuilder".into()),
            CharacterKind::Villager,
            CharacterActivity::Idle,
            PlayerPosition(house_door),
            VillagerIntent::Building {
                settlement,
                site: Entity::PLACEHOLDER,
            },
        ))
        .id();
    let house = app
        .world_mut()
        .spawn((
            SettlementBuilding {
                kind: SettlementBuildingKind::House,
                settlement: "Oakmead".into(),
                owner: Some("NearBuilder".into()),
                quality: 0.7,
                workers: Vec::new(),
            },
            PlayerPosition(house_position),
            PlayerRotation(0.0),
            PlacedBuilding {
                building_type: BuildingType::LogCabin,
                rotation: 0.0,
            },
            BuildingPosition(house_position),
            RoadRequest {
                builder,
                settlement,
                completed_site: Entity::PLACEHOLDER,
                attempt: 0,
            },
        ))
        .id();

    for _ in 0..40 {
        app.update();
        if app.world().get::<RoadRequest>(house).is_none() {
            break;
        }
    }

    let connector = app
        .world_mut()
        .query::<&VillageRoad>()
        .iter(app.world())
        .find(|road| road.builder == "NearBuilder")
        .expect("the nearby house must receive its own connector road");
    assert!(connector.points.len() >= 2);
    assert!(connector.points[0].distance(Vec2::new(house_door.x, house_door.z)) < 0.01);
    assert!(connector.points.last().unwrap().distance(nearby_network) < 0.01);
    assert!(app.world().get::<RoadRequest>(house).is_none());
    assert!(app.world().get::<RoadBuilderRoutine>(builder).is_some());
}

#[test]
fn detached_and_unfinished_roads_are_not_public_network_anchors() {
    let hall_door = Vec2::new(1_700.0, -3.0);
    let connected = VillageRoad {
        settlement: "Oakmead".into(),
        builder: "First".into(),
        points: vec![hall_door, hall_door + Vec2::X * 8.0],
        built_through: 2,
        width: VILLAGE_ROAD_WIDTH,
        reserved_width: RoadClass::Lane.initial_reserved_width(),
        surface: RoadSurface::Dirt,
        class: RoadClass::Lane,
        stone_committed: 0,
    };
    let detached = VillageRoad {
        settlement: "Oakmead".into(),
        builder: "Second".into(),
        points: vec![Vec2::new(1_740.0, 0.0), Vec2::new(1_745.0, 0.0)],
        built_through: 2,
        width: VILLAGE_ROAD_WIDTH,
        reserved_width: RoadClass::Lane.initial_reserved_width(),
        surface: RoadSurface::Dirt,
        class: RoadClass::Lane,
        stone_committed: 0,
    };
    let unfinished = VillageRoad {
        settlement: "Oakmead".into(),
        builder: "Third".into(),
        points: vec![Vec2::new(1_730.0, 0.0), hall_door],
        built_through: 1,
        width: VILLAGE_ROAD_WIDTH,
        reserved_width: RoadClass::Lane.initial_reserved_width(),
        surface: RoadSurface::Dirt,
        class: RoadClass::Lane,
        stone_committed: 0,
    };
    let roads = [&connected, &detached, &unfinished];

    let network = hall_road_network(hall_door, &roads);

    assert_eq!(network.disconnected_components, 1);
    assert!(network.connected_keys.contains(&graph_key(hall_door)));
    assert!(network
        .connected_keys
        .contains(&graph_key(hall_door + Vec2::X * 8.0)));
    assert!(!network
        .connected_keys
        .contains(&graph_key(detached.points[0])));
    assert!(!network
        .connected_keys
        .contains(&graph_key(unfinished.points[0])));
}

#[test]
fn moot_steward_audits_repairs_and_is_paid_from_the_treasury() {
    let mut app = road_test_app();
    app.add_systems(
        Update,
        (
            crate::world::village::ensure_civic_accounts,
            ensure_moot_administrations,
            staff_and_pay_moot_stewards,
            crate::world::village::run_civic_payroll,
            audit_village_roads,
        )
            .chain(),
    );
    let clock = app.world_mut().spawn(WorldTime::new_default()).id();
    let hall_position = Vec3::new(1_700.0, 0.0, 0.0);
    let settlement = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Stewardham".into(),
                tier: SettlementTier::Hamlet,
                residents: 1,
                treasury: 1_000,
            },
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
        ))
        .id();
    let steward = app
        .world_mut()
        .spawn((
            CharacterName("Alda".into()),
            VillagerIntent::Resident { settlement },
            Occupation(None),
            Wallet::new(1_000),
        ))
        .id();
    let house = app
        .world_mut()
        .spawn((
            SettlementBuilding {
                kind: SettlementBuildingKind::House,
                settlement: "Stewardham".into(),
                owner: Some("Alda".into()),
                quality: 0.7,
                workers: Vec::new(),
            },
            PlayerPosition(Vec3::new(1_730.0, 0.0, 0.0)),
            PlayerRotation(0.0),
        ))
        .id();

    app.update();

    let administration = app.world().get::<MootAdministration>(settlement).unwrap();
    assert_eq!(administration.lead_steward.as_deref(), Some("Alda"));
    assert_eq!(administration.roadless_buildings, 1);
    assert_eq!(administration.disconnected_buildings, 0);
    let request = app.world().get::<RoadRequest>(house).unwrap();
    assert_eq!(request.builder, steward);
    assert_eq!(request.settlement, settlement);
    assert_eq!(
        app.world().get::<Occupation>(steward).unwrap().0.as_deref(),
        Some("Moot Steward")
    );

    app.world_mut()
        .entity_mut(steward)
        .get_mut::<CharacterName>()
        .unwrap()
        .0 = "Brina".into();
    app.update();
    assert_eq!(
        app.world()
            .get::<MootAdministration>(settlement)
            .unwrap()
            .lead_steward
            .as_deref(),
        Some("Brina")
    );

    app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 1;
    app.update();

    assert_eq!(
        app.world().get::<Settlement>(settlement).unwrap().treasury,
        900
    );
    assert_eq!(app.world().get::<Wallet>(steward).unwrap().balance(), 1_100);
    assert_eq!(
        app.world()
            .get::<MootAdministration>(settlement)
            .unwrap()
            .wage_arrears,
        0
    );
}

#[test]
fn a_second_moot_steward_waits_for_a_real_collection_backlog() {
    let mut app = road_test_app();
    app.add_systems(
        Update,
        (
            crate::world::village::ensure_civic_accounts,
            ensure_moot_administrations,
            staff_moot_stewards,
            staff_public_positions,
        )
            .chain(),
    );
    let hall = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Twinsteward".into(),
                tier: SettlementTier::Hamlet,
                residents: 16,
                treasury: 3_000,
            },
            PlayerPosition(Vec3::ZERO),
            PlayerRotation(0.0),
        ))
        .id();
    for name in ["Alda", "Brina", "Cora", "Dena"] {
        app.world_mut().spawn((
            CharacterName(name.into()),
            VillagerIntent::Resident { settlement: hall },
            Occupation(None),
            WorkStatus::LookingForWork,
            Wallet::new(1_000),
        ));
    }

    app.update();

    let first_staffed = {
        let world = app.world_mut();
        let mut stewards =
            world.query::<(&shared::components::CivicEmployment, Option<&MootSteward>)>();
        stewards
            .iter(world)
            .filter(|(job, steward)| {
                job.role == shared::components::CivicRole::MootSteward && steward.is_some()
            })
            .count()
    };
    assert_eq!(
        first_staffed, 1,
        "a small Hamlet should retain private labour"
    );

    let settlement_id = *app
        .world()
        .get::<shared::components::SettlementId>(hall)
        .unwrap();
    let mut backlog =
        shared::economy::GoodsInventory::new(shared::economy::capacity::LUMBERJACK_HUT);
    assert_eq!(backlog.add(shared::economy::Good::Wood, 60), 60);
    app.world_mut().spawn((
        shared::components::BuildingOf(settlement_id),
        SettlementBuilding {
            kind: SettlementBuildingKind::LumberjackHut,
            settlement: "Twinsteward".into(),
            owner: Some("Cora".into()),
            quality: 0.8,
            workers: Vec::new(),
        },
        backlog,
        shared::economy::BusinessSalePolicy {
            company_reserve_units: 0,
            ..default()
        },
        shared::economy::BusinessCondition::default(),
    ));
    app.update();

    let mut stewards = app
        .world_mut()
        .query::<(&shared::components::CivicEmployment, Option<&MootSteward>)>();
    let staffed = stewards
        .iter(app.world())
        .filter(|(job, steward)| {
            job.role == shared::components::CivicRole::MootSteward && steward.is_some()
        })
        .count();
    assert_eq!(staffed, 2);
    let administration = app.world().get::<MootAdministration>(hall).unwrap();
    assert_eq!(administration.city_workers.len(), 2);
    assert_eq!(SettlementTier::Hamlet.public_worker_positions(), 2);
}

#[test]
fn strategic_moot_steward_audits_offscreen_and_wakes_for_repairs() {
    let mut app = road_test_app();
    app.add_systems(
        Update,
        (
            ensure_moot_administrations,
            staff_and_pay_moot_stewards,
            audit_village_roads,
        )
            .chain(),
    );
    app.world_mut().spawn(WorldTime::new_default());
    let settlement_id = shared::components::SettlementId(610);
    let settlement = app
        .world_mut()
        .spawn((
            settlement_id,
            Settlement {
                name: "Far Stewardham".into(),
                tier: SettlementTier::Hamlet,
                residents: 1,
                treasury: 1_000,
            },
            PlayerPosition(Vec3::new(1_700.0, 0.0, 0.0)),
            PlayerRotation(0.0),
        ))
        .id();
    let steward = app
        .world_mut()
        .spawn((
            shared::components::PersonId(611),
            CharacterName("Alda".into()),
            CharacterKind::Villager,
            PlayerPosition(Vec3::new(1_700.0, 0.0, 2.0)),
            VillagerIntent::Resident { settlement },
            Occupation(None),
            Wallet::new(1_000),
            crate::world::village::strategic::StrategicPerson,
            crate::world::village::strategic::StrategicTravel::for_test(
                Vec3::new(1_704.0, 0.0, 2.0),
                0.0,
            ),
        ))
        .id();
    let house = app
        .world_mut()
        .spawn((
            shared::components::BuildingId(612),
            shared::components::BuildingOf(settlement_id),
            SettlementBuilding {
                kind: SettlementBuildingKind::House,
                settlement: "Far Stewardham".into(),
                owner: Some("Alda".into()),
                quality: 0.7,
                workers: Vec::new(),
            },
            PlayerPosition(Vec3::new(1_730.0, 0.0, 0.0)),
            PlayerRotation(0.0),
        ))
        .id();

    app.update();

    let request = app.world().get::<RoadRequest>(house).unwrap();
    assert_eq!(request.builder, steward);
    assert!(
        app.world()
            .get::<crate::world::village::strategic::StrategicPerson>(steward)
            .is_none(),
        "an off-screen steward must wake before adopting physical road work"
    );
    assert!(
        app.world()
            .get::<crate::world::village::strategic::StrategicTravel>(steward)
            .is_none(),
        "the repair commitment must replace abstract leisure travel"
    );
    assert!(app
        .world()
        .get::<crate::world::village::strategic::PendingStrategicDemotion>(steward)
        .is_some());
}

#[test]
fn steward_does_not_mistake_a_neighbours_road_for_the_buildings_connector() {
    let mut app = road_test_app();
    app.add_systems(
        Update,
        (
            ensure_moot_administrations,
            staff_and_pay_moot_stewards,
            audit_village_roads,
        )
            .chain(),
    );
    app.world_mut().spawn(WorldTime::new_default());
    let settlement_id = shared::components::SettlementId(620);
    let hall_position = Vec3::new(1_700.0, 0.0, 0.0);
    let settlement = app
        .world_mut()
        .spawn((
            settlement_id,
            Settlement {
                name: "Closeham".into(),
                tier: SettlementTier::Hamlet,
                residents: 1,
                treasury: 1_000,
            },
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
        ))
        .id();
    let steward = app
        .world_mut()
        .spawn((
            shared::components::PersonId(621),
            CharacterName("Alda".into()),
            CharacterKind::Villager,
            PlayerPosition(hall_position + Vec3::Z * 2.0),
            VillagerIntent::Resident { settlement },
            Occupation(None),
            Wallet::new(1_000),
        ))
        .id();
    let house_position = Vec3::new(1_730.0, 0.0, 0.0);
    let house = app
        .world_mut()
        .spawn((
            shared::components::BuildingId(622),
            shared::components::BuildingOf(settlement_id),
            SettlementBuilding {
                kind: SettlementBuildingKind::House,
                settlement: "Closeham".into(),
                owner: Some("Alda".into()),
                quality: 0.7,
                workers: Vec::new(),
            },
            PlayerPosition(house_position),
            PlayerRotation(0.0),
        ))
        .id();
    let neighbour = app.world_mut().spawn_empty().id();
    let house_door3 = SettlementBuildingKind::House.entrance_position(house_position, 0.0);
    let house_door = Vec2::new(house_door3.x, house_door3.z);
    let hall_door3 = SettlementBuildingKind::Hall.entrance_position(hall_position, 0.0);
    app.world_mut().spawn((
        VillageRoad {
            settlement: "Closeham".into(),
            builder: "Neighbour".into(),
            points: vec![house_door, Vec2::new(hall_door3.x, hall_door3.z)],
            built_through: 2,
            width: VILLAGE_ROAD_WIDTH,
            reserved_width: RoadClass::Lane.initial_reserved_width(),
            surface: RoadSurface::Dirt,
            class: RoadClass::Lane,
            stone_committed: 0,
        },
        shared::components::RoadOf(settlement_id),
        RoadConnectorFor {
            building: neighbour,
        },
    ));

    app.update();

    let request = app.world().get::<RoadRequest>(house).unwrap();
    assert_eq!(request.builder, steward);
    assert_eq!(
        app.world()
            .get::<MootAdministration>(settlement)
            .unwrap()
            .roadless_buildings,
        1
    );
}

#[test]
fn moot_steward_reclaims_an_abandoned_unfinished_connector() {
    let mut app = road_test_app();
    app.add_systems(
        Update,
        (
            ensure_moot_administrations,
            staff_and_pay_moot_stewards,
            audit_village_roads,
        )
            .chain(),
    );
    app.world_mut().spawn(WorldTime::new_default());
    let hall_position = Vec3::new(1_700.0, 0.0, 0.0);
    let settlement = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Repairwick".into(),
                tier: SettlementTier::Hamlet,
                residents: 1,
                treasury: 1_000,
            },
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
        ))
        .id();
    let steward = app
        .world_mut()
        .spawn((
            CharacterName("Alda".into()),
            VillagerIntent::Resident { settlement },
            Occupation(None),
            Wallet::new(1_000),
        ))
        .id();
    let house_position = Vec3::new(1_730.0, 0.0, 0.0);
    let house = app
        .world_mut()
        .spawn((
            SettlementBuilding {
                kind: SettlementBuildingKind::House,
                settlement: "Repairwick".into(),
                owner: Some("Alda".into()),
                quality: 0.7,
                workers: Vec::new(),
            },
            PlayerPosition(house_position),
            PlayerRotation(0.0),
        ))
        .id();
    let door3 = SettlementBuildingKind::House.entrance_position(house_position, 0.0);
    let door = Vec2::new(door3.x, door3.z);
    let abandoned = app
        .world_mut()
        .spawn(VillageRoad {
            settlement: "Repairwick".into(),
            builder: "Missing Builder".into(),
            points: vec![door, door + Vec2::X * 8.0],
            built_through: 1,
            width: VILLAGE_ROAD_WIDTH,
            reserved_width: RoadClass::Lane.initial_reserved_width(),
            surface: RoadSurface::Dirt,
            class: RoadClass::Lane,
            stone_committed: 0,
        })
        .id();

    app.update();

    assert!(app.world().get_entity(abandoned).is_err());
    let request = app.world().get::<RoadRequest>(house).unwrap();
    assert_eq!(request.builder, steward);
    assert_eq!(request.attempt, 0);
    let administration = app.world().get::<MootAdministration>(settlement).unwrap();
    assert_eq!(administration.roadless_buildings, 1);
    assert_eq!(administration.disconnected_buildings, 0);
    assert_eq!(administration.pending_road_buildings, 0);
}

#[test]
fn moot_steward_keeps_one_oldest_repair_request_while_off_duty() {
    let mut app = road_test_app();
    app.add_systems(
        Update,
        (
            ensure_moot_administrations,
            staff_and_pay_moot_stewards,
            audit_village_roads,
        )
            .chain(),
    );
    let clock = app.world_mut().spawn(WorldTime::new_default()).id();
    let settlement_id = shared::components::SettlementId(500);
    let settlement = app
        .world_mut()
        .spawn((
            settlement_id,
            Settlement {
                name: "Queueford".into(),
                tier: SettlementTier::Hamlet,
                residents: 1,
                treasury: 1_000,
            },
            PlayerPosition(Vec3::new(1_700.0, 0.0, 0.0)),
            PlayerRotation(0.0),
        ))
        .id();
    let steward = app
        .world_mut()
        .spawn((
            CharacterName("Alda".into()),
            CharacterKind::Villager,
            PlayerPosition(Vec3::new(1_700.0, 0.0, 2.0)),
            VillagerIntent::Resident { settlement },
            Occupation(None),
            Wallet::new(1_000),
        ))
        .id();
    for (id, x) in [(501, 1_730.0), (502, 1_745.0)] {
        app.world_mut().spawn((
            shared::components::BuildingId(id),
            shared::components::BuildingOf(settlement_id),
            SettlementBuilding {
                kind: SettlementBuildingKind::House,
                settlement: "Queueford".into(),
                owner: Some("Resident".into()),
                quality: 0.7,
                workers: Vec::new(),
            },
            PlayerPosition(Vec3::new(x, 0.0, 0.0)),
            PlayerRotation(0.0),
        ));
    }

    app.update();
    let first_request = app
        .world_mut()
        .query::<(Entity, &RoadRequest)>()
        .iter(app.world())
        .map(|(building, request)| (building, *request))
        .collect::<Vec<_>>();
    assert_eq!(first_request.len(), 1);
    assert_eq!(first_request[0].1.builder, steward);
    assert_eq!(
        *app.world()
            .get::<shared::components::BuildingId>(first_request[0].0)
            .unwrap(),
        shared::components::BuildingId(501),
        "the stable oldest building should receive the public worker first"
    );

    app.world_mut()
        .get_mut::<WorldTime>(clock)
        .unwrap()
        .seconds_in_cycle += (ROAD_AUDIT_INTERVAL_SECONDS + 1.0) as f32;
    app.update();

    let requests = app
        .world_mut()
        .query::<&RoadRequest>()
        .iter(app.world())
        .count();
    assert_eq!(
        requests, 1,
        "repeated audits must not enqueue the same steward on several buildings"
    );
    let backlog: Vec<_> = app
        .world_mut()
        .query::<(Entity, &RoadRepairBacklog)>()
        .iter(app.world())
        .map(|(building, _)| building)
        .collect();
    assert_eq!(backlog.len(), 1);
    assert_eq!(
        *app.world()
            .get::<shared::components::BuildingId>(backlog[0])
            .unwrap(),
        shared::components::BuildingId(502),
        "the second roadless building must remain explicitly queued"
    );
    let administration = app.world().get::<MootAdministration>(settlement).unwrap();
    assert_eq!(administration.pending_road_buildings, 1);
    assert_eq!(administration.roadless_buildings, 1);
}

#[test]
fn inactive_private_road_request_becomes_backlog_without_dual_ownership() {
    let mut app = road_test_app();
    app.add_systems(
        Update,
        (
            ensure_moot_administrations,
            staff_and_pay_moot_stewards,
            audit_village_roads,
        )
            .chain(),
    );
    let clock = app.world_mut().spawn(WorldTime::new_default()).id();
    let settlement_id = shared::components::SettlementId(800);
    let settlement = app
        .world_mut()
        .spawn((
            settlement_id,
            Settlement {
                name: "Backlogford".into(),
                tier: SettlementTier::Hamlet,
                residents: 2,
                treasury: 1_000,
            },
            PlayerPosition(Vec3::new(1_700.0, 0.0, 0.0)),
            PlayerRotation(0.0),
        ))
        .id();
    let steward = app
        .world_mut()
        .spawn((
            CharacterName("Alda".into()),
            CharacterKind::Villager,
            PlayerPosition(Vec3::new(1_700.0, 0.0, 2.0)),
            VillagerIntent::Resident { settlement },
            Occupation(None),
            Wallet::new(1_000),
        ))
        .id();
    app.update();

    let steward_job = app
        .world()
        .get::<shared::components::CivicEmployment>(steward)
        .copied();
    assert!(
        steward_job.is_some(),
        "the fixture needs an accountable steward"
    );
    let steward_task = app.world_mut().spawn_empty().id();
    app.world_mut()
        .entity_mut(steward)
        .insert(VillagerIntent::Building {
            settlement,
            site: steward_task,
        });

    let completed_site = app.world_mut().spawn_empty().id();
    let stale_builder = app
        .world_mut()
        .spawn((
            CharacterName("Mara".into()),
            CharacterKind::Villager,
            PlayerPosition(Vec3::new(1_730.0, 0.0, 0.0)),
            VillagerIntent::Building {
                settlement,
                site: completed_site,
            },
            shared::components::EmployedAt(shared::components::BuildingId(999)),
        ))
        .id();
    let building = app
        .world_mut()
        .spawn((
            shared::components::BuildingId(801),
            shared::components::BuildingOf(settlement_id),
            SettlementBuilding {
                kind: SettlementBuildingKind::House,
                settlement: "Backlogford".into(),
                owner: Some("Mara".into()),
                quality: 0.5,
                workers: Vec::new(),
            },
            PlayerPosition(Vec3::new(1_730.0, 0.0, 0.0)),
            PlayerRotation(0.0),
            RoadRequest {
                builder: stale_builder,
                settlement,
                completed_site,
                attempt: 0,
            },
        ))
        .id();
    app.world_mut()
        .get_mut::<WorldTime>(clock)
        .unwrap()
        .seconds_in_cycle += (ROAD_AUDIT_INTERVAL_SECONDS + 1.0) as f32;

    app.update();

    assert!(app.world().get::<RoadRequest>(building).is_none());
    assert!(app.world().get::<RoadRepairBacklog>(building).is_some());
    assert!(matches!(
        app.world().get::<VillagerIntent>(stale_builder),
        Some(VillagerIntent::Resident { settlement: home }) if *home == settlement
    ));
}

#[test]
fn moot_steward_reclaims_a_live_connector_that_makes_no_daylight_progress() {
    let mut app = road_test_app();
    app.add_systems(
        Update,
        (
            ensure_moot_administrations,
            staff_and_pay_moot_stewards,
            audit_village_roads,
        )
            .chain(),
    );
    let clock = app.world_mut().spawn(WorldTime::new_default()).id();
    let settlement = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Stallford".into(),
                tier: SettlementTier::Hamlet,
                residents: 2,
                treasury: 1_000,
            },
            PlayerPosition(Vec3::new(1_700.0, 0.0, 0.0)),
            PlayerRotation(0.0),
        ))
        .id();
    let steward = app
        .world_mut()
        .spawn((
            CharacterName("Alda".into()),
            VillagerIntent::Resident { settlement },
            Occupation(None),
            Wallet::new(1_000),
        ))
        .id();
    let house_position = Vec3::new(1_730.0, 0.0, 0.0);
    let house = app
        .world_mut()
        .spawn((
            SettlementBuilding {
                kind: SettlementBuildingKind::House,
                settlement: "Stallford".into(),
                owner: Some("Bryn".into()),
                quality: 0.7,
                workers: Vec::new(),
            },
            PlayerPosition(house_position),
            PlayerRotation(0.0),
        ))
        .id();
    let door3 = SettlementBuildingKind::House.entrance_position(house_position, 0.0);
    let door = Vec2::new(door3.x, door3.z);
    let road = app
        .world_mut()
        .spawn((
            VillageRoad {
                settlement: "Stallford".into(),
                builder: "Bryn".into(),
                points: vec![door, door + Vec2::X * 8.0],
                built_through: 1,
                width: VILLAGE_ROAD_WIDTH,
                reserved_width: RoadClass::Lane.initial_reserved_width(),
                surface: RoadSurface::Dirt,
                class: RoadClass::Lane,
                stone_committed: 0,
            },
            RoadConnectorFor { building: house },
        ))
        .id();
    let builder = app
        .world_mut()
        .spawn((
            CharacterName("Bryn".into()),
            CharacterKind::Villager,
            PlayerPosition(door3),
            VillagerIntent::RoadBuilding { settlement, road },
            Occupation(None),
            Wallet::new(1_000),
            RoadBuilderRoutine {
                road,
                settlement,
                attempt: 0,
                phase: RoadBuildPhase::GoingTo { point: 1 },
            },
        ))
        .id();

    app.update();
    assert!(app.world().get::<RoadRequest>(house).is_none());

    app.world_mut()
        .get_mut::<WorldTime>(clock)
        .unwrap()
        .seconds_in_cycle += (ROAD_BUILDER_STALL_SECONDS + ROAD_AUDIT_INTERVAL_SECONDS) as f32;
    app.update();

    assert!(app.world().get_entity(road).is_err());
    assert!(app.world().get::<RoadBuilderRoutine>(builder).is_none());
    assert!(matches!(
        app.world().get::<VillagerIntent>(builder),
        Some(VillagerIntent::Resident { settlement: owner }) if *owner == settlement
    ));
    let request = app.world().get::<RoadRequest>(house).unwrap();
    assert_eq!(request.builder, steward);
    let administration = app.world().get::<MootAdministration>(settlement).unwrap();
    assert_eq!(administration.roadless_buildings, 1);
    assert_eq!(administration.pending_road_buildings, 0);
}

#[test]
fn active_road_builder_is_not_hired_for_a_production_job() {
    let mut app = road_test_app();
    app.add_systems(Update, crate::world::village::fill_vacancies);
    let settlement = app
        .world_mut()
        .spawn(Settlement {
            name: "OneJob".into(),
            tier: SettlementTier::Hamlet,
            residents: 1,
            treasury: 0,
        })
        .id();
    let road = app.world_mut().spawn_empty().id();
    let worker = app
        .world_mut()
        .spawn((
            CharacterName("Bryn".into()),
            VillagerIntent::RoadBuilding { settlement, road },
            PlayerPosition(Vec3::ZERO),
            Occupation(None),
            WorkStatus::LookingForWork,
            RoadBuilderRoutine {
                road,
                settlement,
                attempt: 0,
                phase: RoadBuildPhase::GoingTo { point: 0 },
            },
        ))
        .id();
    let farm = app
        .world_mut()
        .spawn((
            SettlementBuilding {
                kind: SettlementBuildingKind::Farmstead,
                settlement: "OneJob".into(),
                owner: Some("Someone Else".into()),
                quality: 1.0,
                workers: Vec::new(),
            },
            PlayerPosition(Vec3::X * 10.0),
        ))
        .id();

    app.update();

    assert!(app
        .world()
        .get::<SettlementBuilding>(farm)
        .unwrap()
        .workers
        .is_empty());
    assert_eq!(
        *app.world().get::<WorkStatus>(worker).unwrap(),
        WorkStatus::LookingForWork
    );
    assert!(app.world().get::<Occupation>(worker).unwrap().0.is_none());
}

#[test]
fn moot_steward_cannot_also_be_hired_as_a_farmer() {
    let mut app = road_test_app();
    app.add_systems(Update, crate::world::village::fill_vacancies);
    let settlement = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Singletrade".into(),
                tier: SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            },
            MootAdministration {
                lead_steward: Some("Alda".into()),
                ..default()
            },
        ))
        .id();
    let steward = app
        .world_mut()
        .spawn((
            CharacterName("Alda".into()),
            VillagerIntent::Resident { settlement },
            PlayerPosition(Vec3::ZERO),
            Occupation(Some("Moot Steward".into())),
            WorkStatus::Employed,
            MootSteward { settlement },
        ))
        .id();
    let farm = app
        .world_mut()
        .spawn((
            SettlementBuilding {
                kind: SettlementBuildingKind::Farmstead,
                settlement: "Singletrade".into(),
                owner: Some("Someone Else".into()),
                quality: 1.0,
                // Reproduce a save written by the old recruiter, which
                // could leave the Road Steward in a farm roster too.
                workers: vec!["Alda".into()],
            },
            PlayerPosition(Vec3::X * 10.0),
        ))
        .id();

    app.update();

    assert!(app
        .world()
        .get::<SettlementBuilding>(farm)
        .unwrap()
        .workers
        .is_empty());
    assert_eq!(
        app.world().get::<Occupation>(steward).unwrap().0.as_deref(),
        Some("Moot Steward")
    );
    assert_eq!(
        *app.world().get::<WorkStatus>(steward).unwrap(),
        WorkStatus::Employed
    );
}

#[test]
fn moot_steward_reclaims_a_stale_request_from_a_builder_with_a_new_permit() {
    let mut app = road_test_app();
    app.add_systems(
        Update,
        (
            ensure_moot_administrations,
            staff_and_pay_moot_stewards,
            audit_village_roads,
        )
            .chain(),
    );
    app.world_mut().spawn(WorldTime::new_default());
    let settlement = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Permitford".into(),
                tier: SettlementTier::Hamlet,
                residents: 2,
                treasury: 1_000,
            },
            PlayerPosition(Vec3::new(1_700.0, 0.0, 0.0)),
            PlayerRotation(0.0),
        ))
        .id();
    let steward = app
        .world_mut()
        .spawn((
            CharacterName("Alda".into()),
            VillagerIntent::Resident { settlement },
            Occupation(None),
            Wallet::new(1_000),
        ))
        .id();
    let newer_site = app.world_mut().spawn_empty().id();
    let original_builder = app
        .world_mut()
        .spawn((
            CharacterName("Bryn".into()),
            VillagerIntent::Building {
                settlement,
                site: newer_site,
            },
            Occupation(None),
            Wallet::new(1_000),
        ))
        .id();
    let house = app
        .world_mut()
        .spawn((
            SettlementBuilding {
                kind: SettlementBuildingKind::House,
                settlement: "Permitford".into(),
                owner: Some("Bryn".into()),
                quality: 0.7,
                workers: Vec::new(),
            },
            PlayerPosition(Vec3::new(1_730.0, 0.0, 0.0)),
            PlayerRotation(0.0),
        ))
        .id();
    app.world_mut().entity_mut(house).insert(RoadRequest {
        builder: original_builder,
        settlement,
        completed_site: house,
        attempt: 0,
    });

    app.update();

    let request = app.world().get::<RoadRequest>(house).unwrap();
    assert_eq!(request.builder, steward);
    let administration = app.world().get::<MootAdministration>(settlement).unwrap();
    assert_eq!(administration.roadless_buildings, 1);
    assert_eq!(administration.pending_road_buildings, 0);
}

#[test]
fn moot_steward_reclaims_a_connector_from_a_privately_employed_owner() {
    let mut app = road_test_app();
    app.add_systems(
        Update,
        (
            ensure_moot_administrations,
            staff_and_pay_moot_stewards,
            audit_village_roads,
        )
            .chain(),
    );
    app.world_mut().spawn(WorldTime::new_default());
    let settlement = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Jobford".into(),
                tier: SettlementTier::Hamlet,
                residents: 2,
                treasury: 1_000,
            },
            PlayerPosition(Vec3::new(1_700.0, 0.0, 0.0)),
            PlayerRotation(0.0),
        ))
        .id();
    let steward = app
        .world_mut()
        .spawn((
            CharacterName("Alda".into()),
            VillagerIntent::Resident { settlement },
            Occupation(None),
            Wallet::new(1_000),
        ))
        .id();
    let employed_owner = app
        .world_mut()
        .spawn((
            CharacterName("Bryn".into()),
            VillagerIntent::Building {
                settlement,
                site: Entity::PLACEHOLDER,
            },
            PlayerPosition(Vec3::new(1_730.0, 0.0, 0.0)),
            Occupation(Some("FARMSTEAD".into())),
            Wallet::new(1_000),
            shared::components::EmployedAt(shared::components::BuildingId(99)),
        ))
        .id();
    let house = app
        .world_mut()
        .spawn((
            SettlementBuilding {
                kind: SettlementBuildingKind::House,
                settlement: "Jobford".into(),
                owner: Some("Bryn".into()),
                quality: 0.7,
                workers: Vec::new(),
            },
            PlayerPosition(Vec3::new(1_730.0, 0.0, 0.0)),
            PlayerRotation(0.0),
            RoadRequest {
                builder: employed_owner,
                settlement,
                completed_site: Entity::PLACEHOLDER,
                attempt: 0,
            },
        ))
        .id();

    app.update();

    let request = app.world().get::<RoadRequest>(house).unwrap();
    assert_eq!(request.builder, steward);
    assert!(matches!(
        app.world().get::<VillagerIntent>(employed_owner),
        Some(VillagerIntent::Resident { settlement: home }) if *home == settlement
    ));
    let administration = app.world().get::<MootAdministration>(settlement).unwrap();
    assert_eq!(administration.roadless_buildings, 1);
    assert_eq!(administration.pending_road_buildings, 0);
}

#[test]
fn road_builder_skips_a_failed_already_built_door_anchor() {
    let mut app = road_test_app();
    app.init_resource::<Time>();
    app.insert_resource(WorldTerrain::default());
    app.add_systems(Update, build_village_roads);
    app.world_mut()
        .spawn(shared::components::TimeWarp::clamped(100.0));
    let settlement = app
        .world_mut()
        .spawn(shared::components::SettlementId(1))
        .id();
    let terrain = app.world().resource::<WorldTerrain>();
    let first = Vec2::new(1_700.0, 0.0);
    let second = first + Vec2::X * 5.0;
    let first3 = Vec3::new(first.x, terrain.get_height(first.x, first.y), first.y);
    let road = app
        .world_mut()
        .spawn((
            VillageRoad {
                settlement: "Anchorham".into(),
                builder: "Aud".into(),
                points: vec![first, second],
                built_through: 1,
                width: VILLAGE_ROAD_WIDTH,
                reserved_width: RoadClass::Lane.initial_reserved_width(),
                surface: RoadSurface::Dirt,
                class: RoadClass::Lane,
                stone_committed: 0,
            },
            shared::components::RoadOf(shared::components::SettlementId(1)),
        ))
        .id();
    let builder = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            PlayerPosition(first3 + Vec3::X * 12.0),
            PlayerRotation(0.0),
            CharacterActivity::Idle,
            VillagerIntent::RoadBuilding { settlement, road },
            MoveTarget(first3),
            NavigationRouteFailed { goal: first3 },
            RoadBuilderRoutine {
                road,
                settlement,
                attempt: 0,
                phase: RoadBuildPhase::GoingTo { point: 0 },
            },
        ))
        .id();

    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs_f32(1.0 / 60.0));
    app.update();

    let routine = app.world().get::<RoadBuilderRoutine>(builder).unwrap();
    assert!(matches!(
        routine.phase,
        RoadBuildPhase::GoingTo { point: 1 }
    ));
    assert!(app.world().get::<NavigationRouteFailed>(builder).is_none());
    let target = app.world().get::<MoveTarget>(builder).unwrap().0;
    assert!(Vec2::new(target.x, target.z).distance(second) < 0.01);
    assert!(app.world().get_entity(road).is_ok());
}

#[test]
fn road_builder_discards_a_failed_route_from_an_older_destination() {
    let mut app = road_test_app();
    app.init_resource::<Time>();
    app.insert_resource(WorldTerrain::default());
    app.add_systems(Update, build_village_roads);
    let settlement = app
        .world_mut()
        .spawn(shared::components::SettlementId(1))
        .id();
    let terrain = app.world().resource::<WorldTerrain>();
    let first = Vec2::new(1_700.0, 0.0);
    let second = first + Vec2::X * 5.0;
    let first3 = Vec3::new(first.x, terrain.get_height(first.x, first.y), first.y);
    let second3 = Vec3::new(second.x, terrain.get_height(second.x, second.y), second.y);
    let road = app
        .world_mut()
        .spawn((
            VillageRoad {
                settlement: "Freshroad".into(),
                builder: "Bryn".into(),
                points: vec![first, second],
                built_through: 1,
                width: VILLAGE_ROAD_WIDTH,
                reserved_width: RoadClass::Lane.initial_reserved_width(),
                surface: RoadSurface::Dirt,
                class: RoadClass::Lane,
                stone_committed: 0,
            },
            shared::components::RoadOf(shared::components::SettlementId(1)),
        ))
        .id();
    let builder = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            PlayerPosition(first3),
            PlayerRotation(0.0),
            CharacterActivity::Idle,
            VillagerIntent::RoadBuilding { settlement, road },
            MoveTarget(second3),
            NavigationRouteFailed {
                goal: first3 + Vec3::Z * 20.0,
            },
            RoadBuilderRoutine {
                road,
                settlement,
                attempt: 0,
                phase: RoadBuildPhase::GoingTo { point: 1 },
            },
        ))
        .id();

    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs_f32(1.0 / 60.0));
    app.update();

    assert!(app.world().get::<NavigationRouteFailed>(builder).is_none());
    assert_eq!(app.world().get::<MoveTarget>(builder).unwrap().0, second3);
    assert!(app.world().get::<RoadBuilderRoutine>(builder).is_some());
    assert!(app.world().get_entity(road).is_ok());
}

#[test]
fn road_builder_works_a_nearby_waypoint_instead_of_resurveying_forever() {
    let mut app = road_test_app();
    app.init_resource::<Time>();
    app.insert_resource(WorldTerrain::default());
    app.add_systems(Update, build_village_roads);
    let settlement_id = shared::components::SettlementId(1);
    let settlement = app.world_mut().spawn(settlement_id).id();
    let terrain = app.world().resource::<WorldTerrain>();
    let first = Vec2::new(1_700.0, 0.0);
    let second = first + Vec2::X * 2.0;
    let nearby = second + Vec2::Y * 2.4;
    let nearby3 = Vec3::new(nearby.x, terrain.get_height(nearby.x, nearby.y), nearby.y);
    let target3 = Vec3::new(second.x, terrain.get_height(second.x, second.y), second.y);
    let building = app.world_mut().spawn_empty().id();
    let road = app
        .world_mut()
        .spawn((
            VillageRoad {
                settlement: "Apronham".into(),
                builder: "Bryn".into(),
                points: vec![first, second],
                built_through: 1,
                width: VILLAGE_ROAD_WIDTH,
                reserved_width: RoadClass::Lane.initial_reserved_width(),
                surface: RoadSurface::Dirt,
                class: RoadClass::Lane,
                stone_committed: 0,
            },
            shared::components::RoadOf(settlement_id),
            RoadConnectorFor { building },
        ))
        .id();
    let builder = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            PlayerPosition(nearby3),
            PlayerRotation(0.0),
            CharacterActivity::Idle,
            VillagerIntent::RoadBuilding { settlement, road },
            MoveTarget(target3),
            NavigationRouteFailed { goal: target3 },
            RoadBuilderRoutine {
                road,
                settlement,
                attempt: 0,
                phase: RoadBuildPhase::GoingTo { point: 1 },
            },
        ))
        .id();

    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs_f32(1.0 / 60.0));
    app.update();

    assert!(app.world().get_entity(road).is_ok());
    assert!(app.world().get::<RoadRequest>(building).is_none());
    assert!(app.world().get::<NavigationRouteFailed>(builder).is_none());
    assert!(matches!(
        app.world()
            .get::<RoadBuilderRoutine>(builder)
            .unwrap()
            .phase,
        RoadBuildPhase::Working { point: 1, .. }
    ));
    assert_eq!(
        app.world().get::<CharacterActivity>(builder),
        Some(&CharacterActivity::Building)
    );
}

#[test]
fn failed_embodied_road_keeps_the_buildings_access_reserved_for_resurvey() {
    let mut app = road_test_app();
    app.init_resource::<Time>();
    app.insert_resource(WorldTerrain::default());
    app.add_systems(Update, build_village_roads);
    let settlement_id = shared::components::SettlementId(1);
    let settlement = app.world_mut().spawn(settlement_id).id();
    let terrain = app.world().resource::<WorldTerrain>();
    let first = Vec2::new(1_700.0, 0.0);
    let second = first + Vec2::X * 5.0;
    let first3 = Vec3::new(first.x, terrain.get_height(first.x, first.y), first.y);
    let second3 = Vec3::new(second.x, terrain.get_height(second.x, second.y), second.y);
    let building = app
        .world_mut()
        .spawn(PlannedRoadAccess {
            settlement_id,
            points: vec![first, second],
            half_width: RoadClass::Lane.initial_reserved_width() * 0.5,
        })
        .id();
    let road = app
        .world_mut()
        .spawn((
            VillageRoad {
                settlement: "Retryham".into(),
                builder: "Bryn".into(),
                points: vec![first, second],
                built_through: 1,
                width: VILLAGE_ROAD_WIDTH,
                reserved_width: RoadClass::Lane.initial_reserved_width(),
                surface: RoadSurface::Dirt,
                class: RoadClass::Lane,
                stone_committed: 0,
            },
            shared::components::RoadOf(settlement_id),
            RoadConnectorFor { building },
        ))
        .id();
    let builder = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            PlayerPosition(first3),
            PlayerRotation(0.0),
            CharacterActivity::Idle,
            VillagerIntent::RoadBuilding { settlement, road },
            MoveTarget(second3),
            NavigationRouteFailed { goal: second3 },
            RoadBuilderRoutine {
                road,
                settlement,
                attempt: 0,
                phase: RoadBuildPhase::GoingTo { point: 1 },
            },
        ))
        .id();

    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs_f32(1.0 / 60.0));
    app.update();

    assert!(app.world().get_entity(road).is_err());
    assert!(app.world().get::<RoadBuilderRoutine>(builder).is_none());
    assert!(app.world().get::<PlannedRoadAccess>(building).is_some());
    let request = app.world().get::<RoadRequest>(building).unwrap();
    assert_eq!(request.builder, builder);
    assert_eq!(request.completed_site, building);
    assert_eq!(request.attempt, 1);
}

#[test]
fn exhausted_embodied_road_immediately_enters_the_steward_backlog() {
    let mut app = road_test_app();
    app.init_resource::<Time>();
    app.insert_resource(WorldTerrain::default());
    app.add_systems(Update, build_village_roads);
    let settlement_id = shared::components::SettlementId(1);
    let settlement = app.world_mut().spawn(settlement_id).id();
    let terrain = app.world().resource::<WorldTerrain>();
    let first = Vec2::new(1_700.0, 0.0);
    let second = first + Vec2::X * 5.0;
    let first3 = Vec3::new(first.x, terrain.get_height(first.x, first.y), first.y);
    let second3 = Vec3::new(second.x, terrain.get_height(second.x, second.y), second.y);
    let building = app
        .world_mut()
        .spawn(PlannedRoadAccess {
            settlement_id,
            points: vec![first, second],
            half_width: RoadClass::Lane.initial_reserved_width() * 0.5,
        })
        .id();
    let road = app
        .world_mut()
        .spawn((
            VillageRoad {
                settlement: "Retryham".into(),
                builder: "Bryn".into(),
                points: vec![first, second],
                built_through: 1,
                width: VILLAGE_ROAD_WIDTH,
                reserved_width: RoadClass::Lane.initial_reserved_width(),
                surface: RoadSurface::Dirt,
                class: RoadClass::Lane,
                stone_committed: 0,
            },
            shared::components::RoadOf(settlement_id),
            RoadConnectorFor { building },
        ))
        .id();
    let builder = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            PlayerPosition(first3),
            PlayerRotation(0.0),
            CharacterActivity::Idle,
            VillagerIntent::RoadBuilding { settlement, road },
            MoveTarget(second3),
            NavigationRouteFailed { goal: second3 },
            RoadBuilderRoutine {
                road,
                settlement,
                attempt: MAX_ROAD_SURVEY_ATTEMPTS - 1,
                phase: RoadBuildPhase::GoingTo { point: 1 },
            },
        ))
        .id();

    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs_f32(1.0 / 60.0));
    app.update();

    assert!(app.world().get_entity(road).is_err());
    assert!(app.world().get::<RoadBuilderRoutine>(builder).is_none());
    assert!(app.world().get::<RoadRequest>(building).is_none());
    assert!(app.world().get::<RoadRepairBacklog>(building).is_some());
    assert!(app.world().get::<PlannedRoadAccess>(building).is_some());
}

#[test]
fn a_retained_road_request_cannot_steal_a_builder_from_a_new_site() {
    let mut app = road_test_app();
    app.insert_resource(WorldTerrain::default());
    app.add_systems(Update, plan_requested_roads);

    let settlement = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Twojobs".into(),
                tier: SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            },
            PlayerPosition(Vec3::new(1700.0, 0.0, 0.0)),
            PlayerRotation(0.0),
        ))
        .id();
    let completed_site = app.world_mut().spawn_empty().id();
    let newer_site = app.world_mut().spawn_empty().id();
    let builder = app
        .world_mut()
        .spawn((
            CharacterName("Mara".into()),
            CharacterKind::Villager,
            CharacterActivity::Idle,
            PlayerPosition(Vec3::new(1740.0, 0.0, 0.0)),
            VillagerIntent::Building {
                settlement,
                site: newer_site,
            },
        ))
        .id();
    let building = app
        .world_mut()
        .spawn((
            SettlementBuilding {
                kind: SettlementBuildingKind::House,
                settlement: "Twojobs".into(),
                owner: Some("Mara".into()),
                quality: 0.5,
                workers: Vec::new(),
            },
            PlayerPosition(Vec3::new(1740.0, 0.0, 0.0)),
            PlayerRotation(0.0),
            RoadRequest {
                builder,
                settlement,
                completed_site,
                attempt: 0,
            },
        ))
        .id();

    app.update();

    assert!(matches!(
        app.world().get::<VillagerIntent>(builder),
        Some(VillagerIntent::Building { settlement: home, site })
            if *home == settlement && *site == newer_site
    ));
    assert!(app.world().get::<RoadBuilderRoutine>(builder).is_none());
    assert!(
        app.world().get::<RoadRequest>(building).is_some(),
        "the old road should wait until its builder is free"
    );
}

#[test]
fn open_market_squares_are_not_published_as_solid_navigation_blocks() {
    let markets = [
        (
            PlacedBuilding {
                building_type: BuildingType::Market,
                rotation: 0.0,
            },
            BuildingPosition(Vec3::ZERO),
        ),
        (
            PlacedBuilding {
                building_type: BuildingType::MarketPaved,
                rotation: 0.4,
            },
            BuildingPosition(Vec3::new(20.0, 0.0, 0.0)),
        ),
    ];
    let mut cache = NavigationBuildingCache::default();
    cache.rebuild(
        markets
            .iter()
            .map(|(building, position)| (building, position)),
    );

    assert!(cache.blockers.is_empty());
    assert!(cache.buildings.is_empty());
    assert!(!cache.spatial.point_blocked(Vec2::ZERO));
    assert!(!cache.spatial.point_blocked(Vec2::new(20.0, 0.0)));

    let cabin = [(
        PlacedBuilding {
            building_type: BuildingType::LogCabin,
            rotation: 0.0,
        },
        BuildingPosition(Vec3::ZERO),
    )];
    cache.rebuild(
        cabin
            .iter()
            .map(|(building, position)| (building, position)),
    );
    assert_eq!(cache.blockers.len(), 1);
    assert_eq!(cache.buildings.len(), 1);
    assert!(cache.spatial.point_blocked(Vec2::ZERO));
}
