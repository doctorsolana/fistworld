use super::*;
use crate::world::village_roads::RouteWaypoint;
use shared::components::{SettlementTier, TimeWarp};
use shared::economy::Wallet;

fn flat_ground() -> WorldTerrain {
    let mut map = WorldTerrain::default().generator.loaded_map().clone();
    let bounds = shared::map::MapBounds {
        min: [-256.; 2],
        max: [256.; 2],
    };
    map.definition.bounds = bounds;
    map.definition.generated = None;
    map.heightmap = shared::map::HeightmapData::new(bounds, 2, 2, vec![4.; 4], Some(-10.));
    map.rivers = default();
    map.river_segments_by_chunk.clear();
    map.terrain_deltas_by_chunk.clear();
    WorldTerrain::from_loaded_map(map)
}

fn app(warp: f32) -> App {
    let mut app = App::new();
    app.init_resource::<Time>()
        .init_resource::<MootQueueClock>()
        .insert_resource(flat_ground())
        .init_resource::<crate::world::village_roads::VillageRoadGraph>()
        .init_resource::<crate::world::pathfinding::PathfindingBudgetSettings>()
        .add_systems(
            Update,
            (
                advance_moot_service_queues,
                run_moot_meal_collections,
                crate::world::village_roads::queue_villager_travel_routes,
                crate::world::village_roads::retry_failed_routes_after_obstacle_change,
                crate::world::village_roads::plan_villager_travel_routes,
                crate::player::hero::step_units,
            )
                .chain(),
        );
    app.world_mut().spawn(TimeWarp(warp));
    app
}

fn tick(app: &mut App) {
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f64(1. / 60.));
    app.update();
}

fn hall(app: &mut App) -> Entity {
    app.world_mut()
        .spawn((
            Settlement {
                name: "Long detour".into(),
                tier: SettlementTier::Hamlet,
                residents: 2,
                treasury: 0,
            },
            PlayerPosition(Vec3::new(0., 4., 0.)),
            PlayerRotation(0.),
        ))
        .id()
}

fn ticket(hall: Entity, serial: u64, kind: MootServiceKind) -> MootQueueTicket {
    MootQueueTicket {
        hall,
        serial,
        kind,
        state: MootQueueState::Queued,
        failed_routes: 0,
        head_wait_seconds: 0.,
        head_best_distance: f32::INFINITY,
        head_route_progress: None,
    }
}

fn target(kind: MootServiceKind) -> Vec3 {
    if kind == MootServiceKind::ConstructionMaterial {
        SettlementBuildingKind::Hall.entrance_position(Vec3::new(0., 4., 0.), 0.)
    } else {
        world_slot(Vec3::new(0., 4., 0.), 0., kind.lane(), 0, None)
    }
}

#[test]
fn route_replacement_does_not_fake_progress_but_small_steps_accumulate() {
    let counter = Vec3::ZERO;
    let mut route = TravelRoute {
        goal: counter,
        next: 0,
        geometry_version: 0,
        waypoints: vec![
            RouteWaypoint {
                position: Vec3::X * 10.,
                on_road: false,
            },
            RouteWaypoint {
                position: counter,
                on_road: false,
            },
        ],
    };
    let mut progress = None;
    assert!(!approach_progress::observe(
        &mut progress,
        counter,
        counter,
        Some(&route)
    ));
    assert!(!approach_progress::observe(
        &mut progress,
        Vec3::X * 0.01,
        counter,
        Some(&route)
    ));
    assert!(approach_progress::observe(
        &mut progress,
        Vec3::X * 0.03,
        counter,
        Some(&route)
    ));
    route.waypoints[0].position = Vec3::Z * 10.;
    assert!(!approach_progress::observe(
        &mut progress,
        Vec3::X * 0.03,
        counter,
        Some(&route)
    ));
    route.next = 1;
    assert!(approach_progress::observe(
        &mut progress,
        Vec3::Z * 10.,
        counter,
        Some(&route)
    ));
    route.next = 0;
    assert!(!approach_progress::observe(
        &mut progress,
        Vec3::Z * 10.,
        counter,
        Some(&route)
    ));
}

#[test]
fn household_material_and_paid_meal_detours_keep_real_progress_at_both_warps() {
    for warp in [1.0_f32, 25.] {
        for kind in [
            MootServiceKind::HouseholdShopping,
            MootServiceKind::ConstructionMaterial,
            MootServiceKind::PersonalMeal,
        ] {
            let mut app = app(warp);
            let hall = hall(&mut app);
            let target = target(kind);
            let start = target + Vec3::X * 35.;
            let corner = start + Vec3::Z * 80.;
            let return_corner = target + Vec3::Z * 80.;
            let mut obstacles = SpatialObstacleGrid::default();
            obstacles.insert(shared::spatial::ObstacleEntry {
                center: target.xz() + Vec2::new(17.5, 35.),
                half_extents: Vec2::new(1., 40.),
                rotation: 0.,
                obstacle_type: 0,
            });
            assert!(obstacles.segment_blocked(start.xz(), target.xz()));
            for (a, b) in [
                (start, corner),
                (corner, return_corner),
                (return_corner, target),
            ] {
                assert!(!obstacles.segment_blocked(a.xz(), b.xz()));
            }
            app.insert_resource(obstacles);
            let mut cargo = GoodsInventory::new(30);
            cargo.add(Good::Wood, 2);
            // Authored clear retained corridor, validated by the real mover.
            // This tests continuing a detour, not discovering its geometry.
            let actor = app
                .world_mut()
                .spawn((
                    CharacterKind::Villager,
                    PlayerPosition(start),
                    PlayerRotation(0.),
                    RegionCoord::from_world_pos(start),
                    CharacterActivity::Idle,
                    shared::components::CharacterMotion::STATIONARY,
                    cargo,
                    Wallet::new(80),
                    Nutrition::default(),
                    MoveTarget(target),
                    ticket(hall, 1, kind),
                    TravelRoute {
                        goal: target,
                        next: 0,
                        geometry_version: 0,
                        waypoints: [corner, return_corner, target]
                            .into_iter()
                            .map(|position| RouteWaypoint {
                                position,
                                on_road: false,
                            })
                            .collect(),
                    },
                ))
                .id();
            let paid_meal = kind == MootServiceKind::PersonalMeal;
            if paid_meal {
                // One already-paid reservation at the Hall; receipt and eating
                // must still occur through the real service/meal systems.
                app.world_mut().entity_mut(actor).insert(MootMealRoutine {
                    hall,
                    kind,
                    good: Good::Bread,
                    meal_day: 1,
                    phase: MootMealPhase::Queueing,
                });
            }
            let mut away_seconds = 0.;
            let mut served = false;
            let mut received_food = false;
            for _ in 0..(180. * 60. / warp).ceil() as usize {
                let before = app.world().get::<PlayerPosition>(actor).unwrap().0;
                let bread_before = app
                    .world()
                    .get::<GoodsInventory>(actor)
                    .unwrap()
                    .amount(Good::Bread);
                tick(&mut app);
                let at = app.world().get::<PlayerPosition>(actor).unwrap().0;
                if ground_distance(at, target) > ground_distance(start, target) + 1. {
                    away_seconds += warp / 60.;
                }
                if let Some(ticket) = app.world().get::<MootQueueTicket>(actor) {
                    assert!(
                        !matches!(ticket.state, MootQueueState::Retrying { .. }),
                        "a progressing {kind:?} detour retried at {warp}×: {ticket:?}, {at:?}"
                    );
                    if ticket.is_ready() {
                        assert!(ground_distance(at, target) <= QUEUE_REACH);
                        served = true;
                    }
                }
                if app
                    .world()
                    .get::<GoodsInventory>(actor)
                    .unwrap()
                    .amount(Good::Bread)
                    > bread_before
                {
                    assert!(ground_distance(before, target) <= QUEUE_REACH);
                    received_food = true;
                }
                assert_eq!(
                    app.world()
                        .get::<GoodsInventory>(actor)
                        .unwrap()
                        .amount(Good::Wood),
                    2
                );
                assert_eq!(app.world().get::<Wallet>(actor).unwrap().balance(), 80);
                if (!paid_meal && served)
                    || (paid_meal && app.world().get::<MootMealRoutine>(actor).is_none())
                {
                    break;
                }
            }
            assert!(
                away_seconds > MAX_QUEUE_HEAD_WAIT_SECONDS * 2.,
                "fixture must expose a long detour"
            );
            if paid_meal {
                assert!(received_food);
                assert_eq!(app.world().get::<Nutrition>(actor).unwrap().total_meals, 1);
                assert!(app.world().get::<MootMealRoutine>(actor).is_none());
            } else {
                assert!(
                    served,
                    "{kind:?} failed to reach the actual counter at {warp}×"
                );
            }
        }
    }
}

#[test]
fn a_motionless_route_retries_without_spending_its_claim_and_yields_to_the_next_person() {
    for warp in [1.0_f32, 25.] {
        let mut app = app(warp);
        let hall = hall(&mut app);
        let target = target(MootServiceKind::PersonalMeal);
        let start = target + Vec3::X * 40.;
        let mut cargo = GoodsInventory::new(30);
        cargo.add(Good::Wood, 2);
        // Deliberately omit RegionCoord to hold this body's motor unavailable.
        // The queue sees an unchanged body with an accepted route, rather than
        // a fabricated successful arrival or a still-pending route search.
        let actor = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(start),
                PlayerRotation(0.),
                CharacterActivity::Idle,
                cargo,
                Wallet::new(80),
                Nutrition::default(),
                MoveTarget(target),
                ticket(hall, 1, MootServiceKind::PersonalMeal),
                TravelRoute {
                    goal: target,
                    next: 0,
                    geometry_version: 0,
                    waypoints: vec![RouteWaypoint {
                        position: target,
                        on_road: false,
                    }],
                },
                MootMealRoutine {
                    hall,
                    kind: MootServiceKind::PersonalMeal,
                    good: Good::Bread,
                    meal_day: 1,
                    phase: MootMealPhase::Queueing,
                },
            ))
            .id();
        let follower = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(target),
                PlayerRotation(0.),
                CharacterActivity::Idle,
                RegionCoord::from_world_pos(target),
                ticket(hall, 2, MootServiceKind::Permit),
            ))
            .id();
        for _ in 0..(22. * 60. / warp).ceil() as usize {
            tick(&mut app);
        }
        assert!(matches!(
            app.world().get::<MootQueueTicket>(actor).unwrap().state,
            MootQueueState::Retrying { .. }
        ));
        assert_eq!(app.world().get::<PlayerPosition>(actor).unwrap().0, start);
        assert!(
            !app.world()
                .get::<MootQueueTicket>(actor)
                .unwrap()
                .is_ready()
        );
        assert!(app.world().get::<MootMealRoutine>(actor).is_some());
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(actor)
                .unwrap()
                .amount(Good::Bread),
            0
        );
        assert_eq!(app.world().get::<Nutrition>(actor).unwrap().total_meals, 0);
        assert_eq!(app.world().get::<Wallet>(actor).unwrap().balance(), 80);
        assert!(
            app.world()
                .get::<MootQueueTicket>(follower)
                .unwrap()
                .is_ready()
        );
        // Completing the follower and restoring the ordinary mover must
        // permit real collection, without regranting the existing claim.
        app.world_mut()
            .entity_mut(follower)
            .remove::<MootQueueTicket>();
        app.world_mut()
            .entity_mut(actor)
            .insert(RegionCoord::from_world_pos(start));
        let mut received_food = false;
        for _ in 0..(100. * 60. / warp).ceil() as usize {
            let before = app.world().get::<PlayerPosition>(actor).unwrap().0;
            let bread = app
                .world()
                .get::<GoodsInventory>(actor)
                .unwrap()
                .amount(Good::Bread);
            tick(&mut app);
            if app
                .world()
                .get::<GoodsInventory>(actor)
                .unwrap()
                .amount(Good::Bread)
                > bread
            {
                assert!(ground_distance(before, target) <= QUEUE_REACH);
                received_food = true;
            }
            if app.world().get::<MootMealRoutine>(actor).is_none() {
                break;
            }
        }
        assert!(received_food);
        assert_eq!(app.world().get::<Nutrition>(actor).unwrap().total_meals, 1);
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(actor)
                .unwrap()
                .amount(Good::Wood),
            2
        );
        assert_eq!(app.world().get::<Wallet>(actor).unwrap().balance(), 80);
    }
}
