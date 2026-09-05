//! Village immigration regression fixtures and invariants.

use super::*;

#[test]
fn settlement_seek_targets_the_authored_hall_entrance() {
    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.init_resource::<VillageClock>();
    app.add_systems(Update, seek_settlement);

    let hall_position = Vec3::new(120.0, 8.0, -40.0);
    let hall_rotation = 0.73;
    app.world_mut().spawn((
        Settlement {
            name: "Doorstead".to_string(),
            tier: shared::components::SettlementTier::Hamlet,
            residents: 0,
            treasury: 0,
        },
        PlayerPosition(hall_position),
        PlayerRotation(hall_rotation),
    ));
    let villager = app
        .world_mut()
        .spawn((
            PlayerPosition(hall_position + Vec3::X * 40.0),
            VillagerIntent::Idle,
        ))
        .id();

    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f32(SEEK_INTERVAL + 0.1));
    app.update();

    let target = app.world().get::<MoveTarget>(villager).unwrap().0;
    let expected = SettlementBuildingKind::Hall.entrance_position(hall_position, hall_rotation);
    assert!(
        target.distance(expected) < 0.01,
        "migration must target the reachable authored door, not the solid hall centre: {target:?}"
    );
    assert!(target.distance(hall_position) > 1.0);
}

#[test]
fn migration_admission_is_bounded_by_real_time_even_at_high_warp() {
    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.init_resource::<VillageClock>();
    app.add_systems(Update, seek_settlement);

    let hall_position = Vec3::new(120.0, 8.0, -40.0);
    app.world_mut().spawn((
        Settlement {
            name: "Burstford".to_string(),
            tier: shared::components::SettlementTier::Hamlet,
            residents: 0,
            treasury: 0,
        },
        PlayerPosition(hall_position),
        PlayerRotation(0.0),
        shared::components::TimeWarp::clamped(100.0),
    ));
    for index in 0..40 {
        app.world_mut().spawn((
            PlayerPosition(hall_position + Vec3::new(40.0, 0.0, index as f32 * 0.2)),
            VillagerIntent::Idle,
        ));
    }

    let count_travelling = |app: &mut App| {
        let world = app.world_mut();
        world
            .query::<&VillagerIntent>()
            .iter(world)
            .filter(|intent| matches!(intent, VillagerIntent::Travelling { .. }))
            .count()
    };
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f32(SEEK_INTERVAL + 0.01));
    app.update();
    assert_eq!(
        count_travelling(&mut app),
        MAX_MIGRATION_ADMISSIONS_PER_PASS
    );

    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f32(SEEK_INTERVAL + 0.01));
    app.update();
    assert_eq!(
        count_travelling(&mut app),
        MAX_MIGRATION_ADMISSIONS_PER_PASS * 2,
        "100x must not admit the entire paused spawn burst in one tick"
    );
}

#[test]
fn a_later_successful_cohort_route_wakes_failed_immigrants_early() {
    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.init_resource::<VillageClock>();
    app.init_resource::<crate::world::village_roads::VillageRoadGraph>();
    app.add_systems(Update, seek_settlement);

    let hall_position = Vec3::new(120.0, 8.0, -40.0);
    let hall = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Wakeford".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 0,
                treasury: 0,
            },
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
        ))
        .id();
    let start3 = hall_position + Vec3::new(40.0, 0.0, 3.0);
    let entrance3 = SettlementBuildingKind::Hall.entrance_position(hall_position, 0.0);
    let villager = app
        .world_mut()
        .spawn((
            PlayerPosition(start3),
            VillagerIntent::Idle,
            MigrationCooldown::after_failure(None, hall, 0.0, 0),
        ))
        .id();

    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f32(SEEK_INTERVAL + 0.01));
    app.update();
    assert!(matches!(
        app.world().get::<VillagerIntent>(villager),
        Some(VillagerIntent::Idle)
    ));
    assert!(app.world().get::<MoveTarget>(villager).is_none());

    // This represents one of the later, closer immigrants successfully
    // proving a route to the same hall while the first cohort is cooling down.
    let start = Vec2::new(start3.x, start3.z);
    let entrance = Vec2::new(entrance3.x, entrance3.z);
    app.world_mut()
        .resource_mut::<crate::world::village_roads::VillageRoadGraph>()
        .cache_tactical_route(
            start + Vec2::new(1.0, 0.0),
            entrance,
            &[(start + Vec2::new(1.0, 0.0), false), (entrance, false)],
            false,
        );

    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f32(SEEK_INTERVAL + 0.01));
    app.update();
    assert!(matches!(
        app.world().get::<VillagerIntent>(villager),
        Some(VillagerIntent::Travelling { settlement }) if *settlement == hall
    ));
    assert_eq!(
        app.world().get::<MoveTarget>(villager).unwrap().0,
        entrance3
    );
}

#[test]
fn migration_repairs_an_obsolete_hall_centre_target() {
    let mut app = village_test_app();
    app.add_systems(Update, arrive_at_settlement);

    let hall_position = Vec3::new(120.0, 8.0, -40.0);
    let hall_rotation = 0.73;
    let settlement = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Doorstead".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 0,
                treasury: 0,
            },
            PlayerPosition(hall_position),
            PlayerRotation(hall_rotation),
        ))
        .id();
    let villager = app
        .world_mut()
        .spawn((
            PlayerPosition(hall_position + Vec3::X * 40.0),
            VillagerIntent::Travelling { settlement },
            MoveTarget(hall_position),
        ))
        .id();

    app.update();

    let target = app.world().get::<MoveTarget>(villager).unwrap().0;
    let expected = SettlementBuildingKind::Hall.entrance_position(hall_position, hall_rotation);
    assert!(
        target.distance(expected) < 0.01,
        "a live villager stranded by the old centre target must be woken and redirected"
    );
}

#[test]
fn a_failed_migrant_on_the_front_forecourt_joins_the_line_instead_of_cooling_down() {
    let mut app = village_test_app();
    app.init_resource::<MootQueueClock>();
    app.add_systems(Update, arrive_at_settlement);

    let hall_position = Vec3::new(120.0, 18.0, -40.0);
    let settlement = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Forecourt".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 0,
                treasury: 0,
            },
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
        ))
        .id();
    let position = hall_position + Vec3::new(8.0, -4.0, -5.3);
    let goal = SettlementBuildingKind::Hall.entrance_position(hall_position, 0.0);
    let villager = app
        .world_mut()
        .spawn((
            PlayerPosition(position),
            VillagerIntent::Travelling { settlement },
            MoveTarget(goal),
            NavigationRouteFailed { goal },
        ))
        .id();

    app.update();

    let villager = app.world().entity(villager);
    assert!(matches!(
        villager.get::<VillagerIntent>(),
        Some(VillagerIntent::Travelling { settlement: target }) if *target == settlement
    ));
    assert!(villager.contains::<MootQueueTicket>());
    assert!(!villager.contains::<MigrationCooldown>());
    assert!(!villager.contains::<NavigationRouteFailed>());
}

#[test]
fn rear_hall_upgrade_reservation_cannot_steal_a_migrants_door_route() {
    let mut app = village_test_app();
    app.init_resource::<MootQueueClock>();
    app.add_systems(Update, arrive_at_settlement);

    let hall_position = Vec3::new(120.0, 18.0, -40.0);
    let hall_rotation = 0.73;
    let settlement = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Front Door".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 0,
                treasury: 0,
            },
            PlayerPosition(hall_position),
            PlayerRotation(hall_rotation),
        ))
        .id();
    // Local +Z is behind the Hall. Eight metres is inside the future Town
    // Hall reservation, outside the current Moot shell, and inside the old
    // radial arrival threshold that prematurely cancelled this route.
    let rear = shared::rotation::local_to_world_xz(Vec2::new(0.0, 8.0), hall_rotation);
    let position = hall_position + Vec3::new(rear.x, 0.0, rear.y);
    let entrance = SettlementBuildingKind::Hall.entrance_position(hall_position, hall_rotation);
    let villager = app
        .world_mut()
        .spawn((
            PlayerPosition(position),
            VillagerIntent::Travelling { settlement },
            MoveTarget(entrance),
        ))
        .id();

    app.update();

    let villager = app.world().entity(villager);
    assert!(villager.get::<MootQueueTicket>().is_none());
    assert!(villager
        .get::<MoveTarget>()
        .is_some_and(|target| target.0.distance_squared(entrance) < 0.01));
    assert!(matches!(
        villager.get::<VillagerIntent>(),
        Some(VillagerIntent::Travelling { settlement: target }) if *target == settlement
    ));
}

#[test]
fn rear_reservation_migrant_walks_around_the_moot_and_clears_the_line_at_one_and_ten_x() {
    use crate::collision::building_index::{sync_building_spatial_index, BuildingSpatialIndex};
    use crate::player::hero::step_units;
    use crate::world::navgrid::{sync_obstacle_grid, ObstacleGridState};
    use crate::world::pathfinding::PathfindingBudgetSettings;
    use shared::components::TimeWarp;
    use shared::region::RegionCoord;

    for warp in [1.0, 10.0] {
        let mut app = village_test_app();
        app.init_resource::<Time>();
        app.init_resource::<MootQueueClock>();
        app.init_resource::<BuildingSpatialIndex>();
        app.init_resource::<ObstacleGridState>();
        app.init_resource::<SpatialObstacleGrid>();
        app.init_resource::<VillageRoadGraph>();
        app.insert_resource(PathfindingBudgetSettings {
            max_requests_per_tick: 8,
            max_milliseconds_per_tick: 50.0,
        });
        app.insert_resource(WorldTerrain::default());
        app.add_systems(
            Update,
            (
                claim_settlement_hall_obstacles,
                sync_building_spatial_index,
                sync_obstacle_grid,
                arrive_at_settlement,
                advance_immigration_departures,
                advance_moot_service_queues,
                recount_residents,
                crate::world::village_roads::rebuild_village_road_graph,
                crate::world::village_roads::queue_villager_travel_routes,
                crate::world::village_roads::plan_villager_travel_routes,
                step_units,
            )
                .chain(),
        );

        let hall_xz = Vec2::new(1_700.0, 0.0);
        let hall_y = app
            .world()
            .resource::<WorldTerrain>()
            .get_height(hall_xz.x, hall_xz.y);
        let hall_position = Vec3::new(hall_xz.x, hall_y, hall_xz.y);
        let hall_rotation = 0.73;
        let settlement = app
            .world_mut()
            .spawn((
                Settlement {
                    name: format!("Rear Approach {warp}x"),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 0,
                    treasury: 0,
                },
                PlayerPosition(hall_position),
                PlayerRotation(hall_rotation),
            ))
            .id();
        app.world_mut().spawn(TimeWarp::clamped(warp));

        let rear = shared::rotation::local_to_world_xz(Vec2::new(0.0, 8.0), hall_rotation);
        let rear_xz = hall_xz + rear;
        let position = Vec3::new(
            rear_xz.x,
            app.world()
                .resource::<WorldTerrain>()
                .get_height(rear_xz.x, rear_xz.y),
            rear_xz.y,
        );
        let entrance = SettlementBuildingKind::Hall.entrance_position(hall_position, hall_rotation);
        let villager = app
            .world_mut()
            .spawn((
                CharacterName(format!("RearMigrant{warp}")),
                CharacterKind::Villager,
                CharacterActivity::Idle,
                PlayerPosition(position),
                PlayerRotation(0.0),
                RegionCoord::from_world_pos(position),
                VillagerIntent::Travelling { settlement },
                MoveTarget(entrance),
            ))
            .id();

        let tick = std::time::Duration::from_secs_f32(1.0 / 60.0);
        // Includes the route around the reserved Hall shell, FIFO service,
        // and the protected post-registration walk out of the forecourt.
        for _ in 0..1_800 {
            app.world_mut().resource_mut::<Time>().advance_by(tick);
            app.update();
            if matches!(
                app.world().get::<VillagerIntent>(villager),
                Some(VillagerIntent::Resident { settlement: home }) if *home == settlement
            ) {
                break;
            }
        }

        assert!(matches!(
            app.world().get::<VillagerIntent>(villager),
            Some(VillagerIntent::Resident { settlement: home }) if *home == settlement
        ), "the {warp}x migrant never reached the front queue from the rear reservation; position={:?} target={:?} pending={} failed={} ticket={:?}",
            app.world().get::<PlayerPosition>(villager),
            app.world().get::<MoveTarget>(villager),
            app.world().get::<NavigationRoutePending>(villager).is_some(),
            app.world().get::<NavigationRouteFailed>(villager).is_some(),
            app.world().get::<MootQueueTicket>(villager),
        );
        assert_eq!(
            app.world().get::<Settlement>(settlement).unwrap().residents,
            1
        );
        assert!(app.world().get::<MootQueueTicket>(villager).is_none());
        assert!(app
            .world()
            .get::<NavigationRoutePending>(villager)
            .is_none());
        assert!(app.world().get::<NavigationRouteFailed>(villager).is_none());
    }
}

#[test]
fn thirty_then_thirty_immigrants_recover_across_day_two_and_warp_changes() {
    use shared::components::TimeWarp;

    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.init_resource::<VillageClock>();
    app.add_systems(
        Update,
        (seek_settlement, arrive_at_settlement, recount_residents).chain(),
    );

    let first_hall_position = Vec3::ZERO;
    let second_hall_position = Vec3::new(100.0, 0.0, 0.0);
    let first_hall = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Nearford".into(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 0,
                treasury: 0,
            },
            PlayerPosition(first_hall_position),
            PlayerRotation(0.0),
        ))
        .id();
    let second_hall = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Farford".into(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 0,
                treasury: 0,
            },
            PlayerPosition(second_hall_position),
            PlayerRotation(0.0),
        ))
        .id();
    let clock = app
        .world_mut()
        .spawn((WorldTime::new_default(), TimeWarp::clamped(100.0)))
        .id();
    let failed_goal = SettlementBuildingKind::Hall.entrance_position(first_hall_position, 0.0);

    let mut first_wave = Vec::new();
    for index in 0..30 {
        let position = Vec3::new(20.0, 0.0, index as f32 * 0.05);
        first_wave.push(
            app.world_mut()
                .spawn((
                    PlayerPosition(position),
                    VillagerIntent::Travelling {
                        settlement: first_hall,
                    },
                    MoveTarget(failed_goal),
                    NavigationRouteFailed { goal: failed_goal },
                ))
                .id(),
        );
    }

    // The failed cohort must return to decision-making, not remain counted
    // as embodied-but-not-resident travellers forever.
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f32(3.1));
    app.update();
    assert!(first_wave.iter().all(|entity| matches!(
        app.world().get::<VillagerIntent>(*entity),
        Some(VillagerIntent::Idle)
    )));

    // At 10x the next bounded seek passes exclude Nearford only for these
    // people, so they choose the other viable town. Thirty migrants require
    // four real-time admission batches; warp changes never alter that CPU
    // pacing or the state transition.
    app.world_mut().get_mut::<TimeWarp>(clock).unwrap().0 = 10.0;
    for _ in 0..4 {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(0.26));
        app.update();
    }
    assert!(first_wave.iter().all(|entity| matches!(
        app.world().get::<VillagerIntent>(*entity),
        Some(VillagerIntent::Travelling { settlement }) if *settlement == second_hall
    )));
    for entity in &first_wave {
        app.world_mut()
            .get_mut::<PlayerPosition>(*entity)
            .unwrap()
            .0 = second_hall_position;
    }
    app.update();

    // Day two receives a fresh cohort. Their choices are independent of
    // the first cohort's cooldown, so they can join the nearer town.
    app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 2;
    app.world_mut().get_mut::<TimeWarp>(clock).unwrap().0 = 100.0;
    for index in 0..30 {
        let position = first_hall_position + Vec3::new(0.0, 0.0, index as f32 * 0.01);
        app.world_mut()
            .spawn((PlayerPosition(position), VillagerIntent::Idle));
    }
    for _ in 0..4 {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(0.26));
        app.update();
    }

    assert_eq!(
        app.world().get::<Settlement>(first_hall).unwrap().residents,
        30
    );
    assert_eq!(
        app.world()
            .get::<Settlement>(second_hall)
            .unwrap()
            .residents,
        30
    );
    let (resident_intents, failed_routes) = {
        let world = app.world_mut();
        let resident_intents = world
            .query::<&VillagerIntent>()
            .iter(world)
            .filter(|intent| intent.counts_as_resident())
            .count();
        let failed_routes = world.query::<&NavigationRouteFailed>().iter(world).count();
        (resident_intents, failed_routes)
    };
    assert_eq!(resident_intents, 60);
    assert_eq!(failed_routes, 0);
}

#[test]
fn one_hundred_twenty_real_routes_join_without_queue_starvation_at_100x() {
    use crate::player::hero::step_units;
    use crate::world::pathfinding::PathfindingBudgetSettings;
    use shared::components::{CharacterKind, TimeWarp};
    use shared::region::RegionCoord;

    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.init_resource::<VillageClock>();
    app.init_resource::<crate::world::village_roads::VillageRoadGraph>();
    app.insert_resource(PathfindingBudgetSettings {
        max_requests_per_tick: 16,
        ..default()
    });
    app.insert_resource(WorldTerrain::default());
    app.add_systems(
        Update,
        (
            claim_settlement_hall_obstacles,
            tag_villager_intent,
            seek_settlement,
            arrive_at_settlement,
            recount_residents,
            crate::world::village_roads::rebuild_village_road_graph,
            crate::world::village_roads::queue_villager_travel_routes,
            crate::world::village_roads::plan_villager_travel_routes,
            step_units,
        )
            .chain(),
    );

    let hall_y = app
        .world()
        .resource::<WorldTerrain>()
        .get_height(1_700.0, 0.0);
    let hall_position = Vec3::new(1_700.0, hall_y, 0.0);
    let hall = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Crowdford".into(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 0,
                treasury: 0,
            },
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
        ))
        .id();
    app.world_mut()
        .spawn((WorldTime::new_default(), TimeWarp::clamped(100.0)));
    for index in 0..120 {
        let x = 1_738.0 + (index % 6) as f32 * 0.35;
        let z = (index / 6) as f32 * 0.08 - 0.8;
        let y = app.world().resource::<WorldTerrain>().get_height(x, z);
        let position = Vec3::new(x, y, z);
        app.world_mut().spawn((
            CharacterName(format!("CrowdImmigrant{index:03}")),
            CharacterKind::Villager,
            CharacterActivity::Idle,
            PlayerPosition(position),
            PlayerRotation(0.0),
            RegionCoord::from_world_pos(position),
        ));
    }

    let tick = std::time::Duration::from_secs_f32(1.0 / 60.0);
    let started = std::time::Instant::now();
    // Admissions are intentionally paced at 32 people per real second, so a
    // paused 120-person burst cannot become 120 simultaneous A* requests.
    // Five real seconds covers admission plus the final embodied approach.
    for _ in 0..300 {
        app.world_mut().resource_mut::<Time>().advance_by(tick);
        app.update();
    }
    let elapsed = started.elapsed();

    let counted_residents = app.world().get::<Settlement>(hall).unwrap().residents;
    let (idle, travelling, residents, pending, failed) = {
        let world = app.world_mut();
        let mut totals = (0, 0, 0, 0, 0);
        for (intent, route_pending, route_failed) in world
            .query::<(
                &VillagerIntent,
                Has<NavigationRoutePending>,
                Has<NavigationRouteFailed>,
            )>()
            .iter(world)
        {
            totals.0 += usize::from(matches!(intent, VillagerIntent::Idle));
            totals.1 += usize::from(matches!(intent, VillagerIntent::Travelling { .. }));
            totals.2 += usize::from(intent.counts_as_resident());
            totals.3 += usize::from(route_pending);
            totals.4 += usize::from(route_failed);
        }
        totals
    };
    assert_eq!(
        (
            counted_residents,
            idle,
            travelling,
            residents,
            pending,
            failed
        ),
        (120, 0, 0, 120, 0, 0),
        "120-person migration failed after {elapsed:?}"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(8),
        "cached shared-destination migration took {elapsed:?}"
    );
}
