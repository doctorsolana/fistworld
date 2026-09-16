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
    app.add_systems(
        Update,
        (arrive_at_settlement, advance_moot_service_queues).chain(),
    );

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
            PlayerRotation(0.0),
            CharacterActivity::Idle,
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
    assert!(
        villager
            .get::<MoveTarget>()
            .is_some_and(|target| target.0.distance_squared(entrance) < 0.01)
    );
    assert!(matches!(
        villager.get::<VillagerIntent>(),
        Some(VillagerIntent::Travelling { settlement: target }) if *target == settlement
    ));
}

#[test]
fn rear_reservation_migrant_walks_around_the_moot_and_clears_the_line_at_one_and_ten_x() {
    use crate::collision::building_index::{BuildingSpatialIndex, sync_building_spatial_index};
    use crate::player::hero::step_units;
    use crate::world::navgrid::{ObstacleGridState, sync_obstacle_grid};
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

        assert!(
            matches!(
                app.world().get::<VillagerIntent>(villager),
                Some(VillagerIntent::Resident { settlement: home }) if *home == settlement
            ),
            "the {warp}x migrant never reached the front queue from the rear reservation; position={:?} target={:?} pending={} failed={} ticket={:?}",
            app.world().get::<PlayerPosition>(villager),
            app.world().get::<MoveTarget>(villager),
            app.world()
                .get::<NavigationRoutePending>(villager)
                .is_some(),
            app.world().get::<NavigationRouteFailed>(villager).is_some(),
            app.world().get::<MootQueueTicket>(villager),
        );
        assert_eq!(
            app.world().get::<Settlement>(settlement).unwrap().residents,
            1
        );
        assert!(app.world().get::<MootQueueTicket>(villager).is_none());
        assert!(
            app.world()
                .get::<NavigationRoutePending>(villager)
                .is_none()
        );
        assert!(app.world().get::<NavigationRouteFailed>(villager).is_none());
    }
}

/// Complete shared immigration flow on explicitly dry, finite ground. The final
/// walk-away order stands in for downstream daily activity, but uses the real
/// navigation/movement systems and never rewrites a migrant's position.
fn physical_immigration_app() -> App {
    use crate::collision::building_index::{BuildingSpatialIndex, sync_building_spatial_index};
    use crate::world::navgrid::{ObstacleGridState, sync_obstacle_grid};
    let mut app = village_test_app();
    app.init_resource::<Time>()
        .init_resource::<VillageClock>()
        .init_resource::<BuildingSpatialIndex>()
        .init_resource::<ObstacleGridState>()
        .init_resource::<SpatialObstacleGrid>()
        .init_resource::<VillageRoadGraph>()
        .insert_resource(super::fixtures::dry_test_terrain())
        .insert_resource(crate::world::pathfinding::PathfindingBudgetSettings {
            max_requests_per_tick: 16,
            max_milliseconds_per_tick: 50.0,
        });
    app.add_systems(
        Update,
        (
            claim_settlement_hall_obstacles,
            sync_building_spatial_index,
            sync_obstacle_grid,
            tag_villager_intent,
            seek_settlement,
            arrive_at_settlement,
            advance_immigration_departures,
            advance_moot_service_queues,
            walk_registered_immigrants_away,
            recount_residents,
            crate::world::village_roads::rebuild_village_road_graph,
            crate::world::village_roads::queue_villager_travel_routes,
            crate::world::village_roads::plan_villager_travel_routes,
            crate::player::hero::step_units,
        )
            .chain(),
    );
    app
}

fn walk_registered_immigrants_away(
    mut commands: Commands,
    people: Query<
        (Entity, &shared::components::PersonId, &VillagerIntent),
        Changed<VillagerIntent>,
    >,
    halls: Query<&PlayerPosition, With<Settlement>>,
) {
    for (entity, id, intent) in &people {
        let VillagerIntent::Resident { settlement } = intent else {
            continue;
        };
        let Ok(hall) = halls.get(*settlement) else {
            continue;
        };
        let rank = id.0 % 120;
        let destination = hall.0
            + Vec3::new(
                30.0 + (rank % 10) as f32 * 1.5,
                0.0,
                -35.0 - (rank / 10) as f32 * 1.5,
            );
        commands.entity(entity).insert(MoveTarget(destination));
    }
}

fn migrant(app: &mut App, index: usize, position: Vec3, intent: VillagerIntent) -> Entity {
    app.world_mut()
        .spawn((
            CharacterName(format!("Migrant{index:03}")),
            CharacterKind::Villager,
            CharacterActivity::Idle,
            PlayerPosition(position),
            PlayerRotation(0.0),
            shared::region::RegionCoord::from_world_pos(position),
            intent,
        ))
        .id()
}

fn tick_immigration(app: &mut App, seconds: f32) {
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f32(seconds));
    app.update();
}

fn registered(app: &App, entity: Entity, hall: Entity) -> bool {
    matches!(app.world().get::<VillagerIntent>(entity), Some(VillagerIntent::Resident { settlement }) if *settlement == hall)
}

#[test]
fn thirty_then_thirty_immigrants_recover_across_day_two_and_warp_changes() {
    let mut app = physical_immigration_app();
    let first_hall_position = Vec3::ZERO;
    let second_hall_position = Vec3::new(100.0, 0.0, 0.0);
    let halls: Vec<_> = [
        ("Nearford", first_hall_position),
        ("Farford", second_hall_position),
    ]
    .into_iter()
    .map(|(name, position)| {
        app.world_mut()
            .spawn((
                Settlement {
                    name: name.into(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 0,
                    treasury: 0,
                },
                PlayerPosition(position),
                PlayerRotation(0.0),
            ))
            .id()
    })
    .collect();
    let (first_hall, second_hall) = (halls[0], halls[1]);
    let clock = app
        .world_mut()
        .spawn((
            WorldTime::new_default(),
            shared::components::TimeWarp::clamped(100.0),
        ))
        .id();
    let failed_goal = SettlementBuildingKind::Hall.entrance_position(first_hall_position, 0.0);
    let mut first_wave = Vec::new();
    for index in 0..30 {
        let at = Vec3::new(
            20.0 + (index % 6) as f32 * 1.1,
            0.0,
            -20.0 + (index / 6) as f32 * 1.1,
        );
        let entity = migrant(
            &mut app,
            index,
            at,
            VillagerIntent::Travelling {
                settlement: first_hall,
            },
        );
        app.world_mut().entity_mut(entity).insert((
            MoveTarget(failed_goal),
            NavigationRouteFailed { goal: failed_goal },
        ));
        first_wave.push(entity);
    }
    tick_immigration(&mut app, 3.1);
    assert!(first_wave.iter().all(|entity| matches!(
        app.world().get::<VillagerIntent>(*entity),
        Some(VillagerIntent::Idle)
    )));
    assert!(
        first_wave
            .iter()
            .all(|entity| app.world().get::<NavigationRouteFailed>(*entity).is_none())
    );

    // A failed cohort chooses the other town in bounded real-time batches,
    // then actually walks to its FIFO counter and clears the protected exit.
    app.world_mut()
        .get_mut::<shared::components::TimeWarp>(clock)
        .unwrap()
        .0 = 10.0;
    for _ in 0..4 {
        tick_immigration(&mut app, 0.26);
    }
    assert!(first_wave.iter().all(
        |entity| matches!(app.world().get::<VillagerIntent>(*entity),
        Some(VillagerIntent::Travelling { settlement }) if *settlement == second_hall)
    ));
    let mut saw_queue = false;
    let mut saw_departure = false;
    for _ in 0..6_000 {
        tick_immigration(&mut app, 1.0 / 60.0);
        saw_queue |= first_wave
            .iter()
            .any(|entity| app.world().get::<MootQueueTicket>(*entity).is_some());
        saw_departure |= first_wave.iter().any(|entity| {
            app.world()
                .get::<super::super::population::ImmigrationDeparture>(*entity)
                .is_some()
        });
        if first_wave
            .iter()
            .all(|entity| registered(&app, *entity, second_hall))
        {
            break;
        }
    }
    assert!(
        saw_queue && saw_departure,
        "first cohort skipped physical registration"
    );
    assert!(
        first_wave
            .iter()
            .all(|entity| registered(&app, *entity, second_hall)),
        "first cohort did not clear Farford's queue"
    );

    app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 2;
    app.world_mut()
        .get_mut::<shared::components::TimeWarp>(clock)
        .unwrap()
        .0 = 100.0;
    let mut second_wave = Vec::new();
    for index in 0..30 {
        let at = Vec3::new(
            20.0 + (index % 6) as f32 * 1.1,
            0.0,
            -20.0 + (index / 6) as f32 * 1.1,
        );
        second_wave.push(migrant(&mut app, index + 30, at, VillagerIntent::Idle));
    }
    saw_queue = false;
    saw_departure = false;
    for _ in 0..3_600 {
        tick_immigration(&mut app, 1.0 / 60.0);
        saw_queue |= second_wave
            .iter()
            .any(|entity| app.world().get::<MootQueueTicket>(*entity).is_some());
        saw_departure |= second_wave.iter().any(|entity| {
            app.world()
                .get::<super::super::population::ImmigrationDeparture>(*entity)
                .is_some()
        });
        if second_wave
            .iter()
            .all(|entity| registered(&app, *entity, first_hall))
        {
            break;
        }
    }
    assert!(
        saw_queue && saw_departure,
        "second cohort skipped physical registration"
    );
    assert!(
        second_wave
            .iter()
            .all(|entity| registered(&app, *entity, first_hall)),
        "second cohort did not clear Nearford's queue"
    );
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
    for entity in first_wave.into_iter().chain(second_wave) {
        assert!(app.world().get::<MootQueueTicket>(entity).is_none());
        assert!(
            app.world()
                .get::<super::super::population::ImmigrationDeparture>(entity)
                .is_none()
        );
        assert!(app.world().get::<NavigationRouteFailed>(entity).is_none());
    }
}

#[test]
fn one_hundred_twenty_immigrants_pass_bounded_admission_then_the_physical_counter_at_100x() {
    let mut app = physical_immigration_app();
    let hall_position = Vec3::ZERO;
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
    app.world_mut().spawn((
        WorldTime::new_default(),
        shared::components::TimeWarp::clamped(100.0),
    ));
    let mut people = Vec::new();
    for index in 0..120 {
        let at = Vec3::new(
            38.0 + (index % 6) as f32 * 1.2,
            0.0,
            -24.0 + (index / 6) as f32 * 1.2,
        );
        people.push(migrant(&mut app, index, at, VillagerIntent::Idle));
    }
    let started = std::time::Instant::now();
    let mut queued = HashSet::new();
    let mut departing = HashSet::new();
    for tick in 0..3_600 {
        tick_immigration(&mut app, 1.0 / 60.0);
        for entity in &people {
            if app.world().get::<MootQueueTicket>(*entity).is_some() {
                queued.insert(*entity);
            }
            if app
                .world()
                .get::<super::super::population::ImmigrationDeparture>(*entity)
                .is_some()
            {
                departing.insert(*entity);
            }
        }
        if tick == 299 {
            assert!(
                people.iter().all(|entity| !matches!(
                    app.world().get::<VillagerIntent>(*entity),
                    Some(VillagerIntent::Idle)
                )),
                "bounded seek admissions starved part of the 120-person cohort"
            );
        }
        if tick >= 299 && people.iter().all(|entity| registered(&app, *entity, hall)) {
            break;
        }
    }
    assert_eq!(
        queued.len(),
        120,
        "every applicant must physically join the Moot queue"
    );
    assert_eq!(
        departing.len(),
        120,
        "every applicant must retain the physical departure phase"
    );
    assert_eq!(app.world().get::<Settlement>(hall).unwrap().residents, 120);
    for entity in people {
        assert!(registered(&app, entity, hall));
        assert!(app.world().get::<MootQueueTicket>(entity).is_none());
        assert!(
            app.world()
                .get::<super::super::population::ImmigrationDeparture>(entity)
                .is_none()
        );
        assert!(app.world().get::<NavigationRouteFailed>(entity).is_none());
    }
    eprintln!(
        "120-person bounded admission, FIFO and physical departure completed in {:?}",
        started.elapsed()
    );
}

#[test]
fn citizenship_begins_after_actual_hall_service_and_departure_at_both_warps() {
    use shared::components::{EmployedAt, ResidentOf, SettlementId, TimeWarp};
    for warp in [1.0, 25.0] {
        let mut app = physical_immigration_app();
        // The production chain reconciles identity after departures. The
        // reusable fixture also has a PreUpdate mirror; this last pass makes
        // the same-tick registration boundary explicit for this regression.
        app.add_systems(
            PostUpdate,
            crate::world::identity::reconcile_stable_world_relationships,
        );
        app.world_mut()
            .spawn((WorldTime::new_default(), TimeWarp::clamped(warp)));
        let id = SettlementId(91);
        let hall = app
            .world_mut()
            .spawn((
                id,
                Settlement {
                    name: "Registration boundary".into(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 0,
                    treasury: 0,
                },
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.),
            ))
            .id();
        let start = Vec3::new(-22., 0., -26.);
        let actor = migrant(
            &mut app,
            0,
            start,
            VillagerIntent::Travelling { settlement: hall },
        );
        let entrance = SettlementBuildingKind::Hall.entrance_position(Vec3::ZERO, 0.);
        app.world_mut()
            .entity_mut(actor)
            .insert(MoveTarget(entrance));
        let mut saw_queue = false;
        let mut saw_ready = false;
        let mut saw_departure = false;
        for _ in 0..3_600 {
            tick_immigration(&mut app, 1. / 60.);
            if let Some(ticket) = app.world().get::<MootQueueTicket>(actor) {
                saw_queue = true;
                saw_ready |= ticket.is_ready();
            }
            saw_departure |= app
                .world()
                .get::<crate::world::village::population::ImmigrationDeparture>(actor)
                .is_some();
            if registered(&app, actor, hall) {
                break;
            }
            assert!(
                app.world().get::<ResidentOf>(actor).is_none(),
                "{warp}x travel/queue published citizenship early"
            );
            assert!(app.world().get::<Residence>(actor).is_none());
            assert!(app.world().get::<EmployedAt>(actor).is_none());
            assert!(app.world().get::<HomeAssignment>(actor).is_none());
            assert_eq!(app.world().get::<Settlement>(hall).unwrap().residents, 0);
        }
        assert!(
            registered(&app, actor, hall),
            "{warp}x physical registration did not finish"
        );
        assert!(
            saw_queue && saw_ready && saw_departure,
            "registration must include real FIFO service and protected exit"
        );
        assert_eq!(app.world().get::<ResidentOf>(actor), Some(&ResidentOf(id)));
        assert_eq!(app.world().get::<Settlement>(hall).unwrap().residents, 1);
        assert_eq!(
            app.world().get::<Residence>(actor).unwrap().0,
            "Registration boundary"
        );
        assert!(app.world().get::<MootQueueTicket>(actor).is_none());
        assert!(
            app.world()
                .get::<crate::world::village::population::ImmigrationDeparture>(actor)
                .is_none()
        );
        assert!(
            app.world()
                .get::<PlayerPosition>(actor)
                .unwrap()
                .0
                .distance(start)
                > 10.
        );
    }
}
