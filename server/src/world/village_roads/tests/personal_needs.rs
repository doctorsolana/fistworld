use super::*;
use crate::world::village::{self, moot_services};
use shared::components::Nutrition;
use shared::economy::{Good, GoodsInventory};

/// The real meal queue, meal runner and mover own the outing. Only the initial
/// partly completed road task and its already-paid ration are fixture state.
fn road_meal_fixture(chopping: bool) -> (App, Entity, Entity, Vec3) {
    let mut app = road_test_app();
    app.init_resource::<Time>()
        .init_resource::<VillageRoadGraph>()
        .init_resource::<crate::world::pathfinding::PathfindingBudgetSettings>();
    let mut terrain = WorldTerrain::default();
    terrain.apply_flatten_rect(Vec3::new(1_700.0, 80.0, 0.0), Vec2::splat(80.0), 0.0, 4.0);
    app.insert_resource(terrain);
    app.add_systems(
        Update,
        (
            village::advance_moot_service_queues,
            village::run_moot_meal_collections,
            build_village_roads,
            queue_villager_travel_routes,
            plan_villager_travel_routes,
            crate::player::hero::step_units,
        )
            .chain(),
    );
    let settlement = app
        .world_mut()
        .spawn((
            shared::components::SettlementId(1),
            Settlement {
                name: "Mealford".into(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            },
            PlayerPosition(Vec3::new(1_680.0, 80.0, 0.0)),
            PlayerRotation(0.0),
        ))
        .id();
    let road = app
        .world_mut()
        .spawn((
            VillageRoad {
                settlement: "Mealford".into(),
                builder: "Mara".into(),
                points: vec![Vec2::new(1_700.0, 0.0), Vec2::new(1_708.0, 0.0)],
                built_through: 1,
                width: VILLAGE_ROAD_WIDTH,
                reserved_width: RoadClass::Lane.initial_reserved_width(),
                surface: RoadSurface::Dirt,
                class: RoadClass::Lane,
                stone_committed: 0,
            },
            shared::components::RoadOf(shared::components::SettlementId(1)),
            RoadTreeClearancePlan {
                trees: if chopping {
                    vec![RoadTreeObstruction {
                        point: Vec2::new(1_708.0, 0.0),
                        radius: 0.5,
                    }]
                } else {
                    Vec::new()
                },
            },
        ))
        .id();
    // A legitimately accepted nearby stand need not be the nominal road
    // waypoint: failed-waypoint recovery can begin work within three metres.
    let stand = Vec3::new(1_708.0, 80.0, 2.5);
    let mut cargo = GoodsInventory::new(24);
    cargo.add(Good::Wood, 2);
    let builder = app
        .world_mut()
        .spawn((
            CharacterName("Mara".into()),
            CharacterKind::Villager,
            PlayerPosition(stand),
            PlayerRotation(0.0),
            CharacterActivity::Building,
            RegionCoord::from_world_pos(stand),
            VillagerIntent::RoadBuilding { settlement, road },
            cargo,
            Nutrition::default(),
            RoadBuilderRoutine {
                road,
                settlement,
                attempt: 0,
                phase: if chopping {
                    RoadBuildPhase::ChoppingTree {
                        point: 1,
                        tree: Vec2::new(1_708.0, 0.0),
                        radius: 0.5,
                        seconds_left: 1.0,
                    }
                } else {
                    RoadBuildPhase::Working {
                        point: 1,
                        seconds_left: 1.0,
                    }
                },
                resume_work_at: None,
            },
        ))
        .id();
    let mut clock = village::MootQueueClock::default();
    moot_services::reserve_meal(
        &mut app.world_mut().commands(),
        &mut clock,
        builder,
        settlement,
        village::MootServiceKind::PersonalMeal,
        Good::Bread,
        1,
    );
    app.world_mut().flush();
    app.insert_resource(clock);
    (app, builder, road, stand)
}

fn step(app: &mut App) {
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs_f32(0.1));
    app.update();
}

fn remaining(app: &App, builder: Entity) -> f32 {
    match app
        .world()
        .get::<RoadBuilderRoutine>(builder)
        .unwrap()
        .phase
    {
        RoadBuildPhase::Working { seconds_left, .. }
        | RoadBuildPhase::ChoppingTree { seconds_left, .. } => seconds_left,
        phase => panic!("partial work unexpectedly changed phase: {phase:?}"),
    }
}

#[test]
fn road_packing_and_tree_clearance_resume_at_their_stand_after_a_real_meal() {
    for chopping in [false, true] {
        let (mut app, builder, road, stand) = road_meal_fixture(chopping);
        let mut left_stand = false;
        for _ in 0..3_000 {
            step(&mut app);
            left_stand |= app
                .world()
                .get::<PlayerPosition>(builder)
                .unwrap()
                .0
                .xz()
                .distance(stand.xz())
                > 10.0;
            assert_eq!(remaining(&app, builder), 1.0, "meal trip spent road labour");
            assert_eq!(
                app.world().get::<VillageRoad>(road).unwrap().built_through,
                1
            );
            if app
                .world()
                .get::<village::MootMealRoutine>(builder)
                .is_none()
            {
                break;
            }
        }
        assert!(left_stand, "fixture must physically leave the worksite");
        assert!(
            app.world()
                .get::<village::MootMealRoutine>(builder)
                .is_none()
        );
        assert_eq!(
            app.world().get::<Nutrition>(builder).unwrap().last_meal_day,
            Some(1)
        );
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(builder)
                .unwrap()
                .amount(Good::Bread),
            0
        );
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(builder)
                .unwrap()
                .amount(Good::Wood),
            2
        );

        let mut returned_to_work = false;
        for _ in 0..3_000 {
            let before = app.world().get::<PlayerPosition>(builder).unwrap().0.xz();
            step(&mut app);
            if !returned_to_work && before.distance(stand.xz()) > ROAD_REACH {
                assert_eq!(
                    remaining(&app, builder),
                    1.0,
                    "worked before physically returning"
                );
            } else {
                returned_to_work = true;
            }
            if app.world().get::<RoadBuilderRoutine>(builder).is_none() {
                break;
            }
        }
        assert!(returned_to_work);
        assert!(app.world().get::<RoadBuilderRoutine>(builder).is_none());
        assert!(app.world().get::<VillageRoad>(road).unwrap().is_complete());
        assert!(
            app.world()
                .get::<RoadTreeClearancePlan>(road)
                .unwrap()
                .trees
                .is_empty()
        );
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(builder)
                .unwrap()
                .amount(Good::Wood),
            2
        );
    }
}

#[test]
fn a_freight_ticket_is_not_a_personal_need_that_pauses_road_work() {
    let (mut app, builder, road, _) = road_meal_fixture(false);
    app.world_mut()
        .entity_mut(builder)
        .remove::<village::MootMealRoutine>();
    app.world_mut()
        .entity_mut(builder)
        .remove::<village::MootQueueTicket>();
    let settlement = app
        .world()
        .get::<RoadBuilderRoutine>(builder)
        .unwrap()
        .settlement;
    let mut clock = village::MootQueueClock::default();
    village::enqueue_moot_service(
        &mut app.world_mut().commands(),
        &mut clock,
        builder,
        settlement,
        village::MootServiceKind::ConstructionMaterial,
    );
    app.world_mut().flush();
    // Isolate the road runner: its precise guard must not include the ticket
    // of a different service. Full queue ownership is exercised above.
    use bevy::ecs::system::RunSystemOnce;
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs_f32(2.0));
    app.world_mut()
        .run_system_once(build_village_roads)
        .unwrap();
    app.world_mut().flush();
    assert!(
        app.world()
            .get::<village::MootQueueTicket>(builder)
            .is_some()
    );
    assert!(app.world().get::<RoadBuilderRoutine>(builder).is_none());
    assert!(app.world().get::<VillageRoad>(road).unwrap().is_complete());
}
