//! Local leisure must not steal an unfinished immigration journey or bypass
//! the shared navigation retry that protects an already admitted visitor.

use super::*;
use crate::world::pathfinding::PathfindingBudgetSettings;
use crate::world::village_roads::{
    plan_villager_travel_routes, queue_villager_travel_routes,
    retry_failed_routes_after_obstacle_change, NavigationRouteBackoff,
};
use bevy::ecs::system::RunSystemOnce;
use shared::components::{ResidentOf, SettlementId, TimeWarp};
use std::time::Duration;

fn fixture(warp: f32) -> (App, Entity, Entity) {
    let mut terrain = WorldTerrain::default();
    let mut map = terrain.generator.loaded_map().clone();
    let bounds = shared::map::MapBounds {
        min: [-256.; 2],
        max: [256.; 2],
    };
    map.definition.bounds = bounds;
    map.definition.generated = None;
    map.definition.objects.clear();
    map.objects_by_chunk.clear();
    map.heightmap = shared::map::HeightmapData::new(bounds, 2, 2, vec![4.; 4], Some(-10.));
    map.rivers = default();
    map.river_segments_by_chunk.clear();
    map.terrain_deltas_by_chunk.clear();
    terrain.generator = shared::terrain::TerrainGenerator::from_loaded_map(map);
    let mut app = App::new();
    app.init_resource::<Time>()
        .init_resource::<VillageRoadGraph>()
        .init_resource::<PathfindingBudgetSettings>()
        .init_resource::<moot_services::MootQueueClock>()
        .insert_resource(terrain);
    let mut clock = WorldTime::new_default();
    clock.set_normalized_time(18.5 / 24.);
    app.world_mut().spawn((clock, TimeWarp(warp)));
    let hall = app
        .world_mut()
        .spawn((
            SettlementId(1),
            Settlement {
                name: "Arrivalford".into(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 0,
                treasury: 100,
            },
            PlayerPosition(Vec3::new(0., 4., 0.)),
            PlayerRotation(0.),
        ))
        .id();
    let mut pantry = GoodsInventory::new(SettlementBuildingKind::Tavern.storage_bulk_capacity());
    assert_eq!(pantry.add(Good::Bread, 5), 5);
    let tavern = app
        .world_mut()
        .spawn((
            shared::components::BuildingId(2),
            shared::components::BuildingOf(SettlementId(1)),
            SettlementBuilding {
                kind: SettlementBuildingKind::Tavern,
                settlement: "Arrivalford".into(),
                owner: None,
                quality: 1.,
                workers: vec![],
            },
            PlayerPosition(Vec3::new(100., 4., 0.)),
            PlayerRotation(0.),
            BusinessCondition {
                state: BusinessState::Operating,
                ..default()
            },
            TavernService::default(),
            BusinessSalePolicy::default(),
            BusinessAccount::default(),
            shared::components::OperatedBy(shared::components::CompanyId(1)),
            pantry,
        ))
        .id();
    (app, hall, tavern)
}

fn visitor(app: &mut App, intent: VillagerIntent) -> Entity {
    let at = Vec3::new(0., 4., -80.);
    let person = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            ResidentOf(SettlementId(1)),
            intent,
            PlayerPosition(at),
            PlayerRotation(0.),
            RegionCoord::from_world_pos(at),
            CharacterActivity::Idle,
            Wallet::new(1_000),
            Nutrition::default(),
            WorkStatus::LookingForWork,
            GoodsInventory::new(shared::economy::capacity::VILLAGER),
        ))
        .id();
    app.world_mut().entity_mut(person).insert(CharacterDayPlan {
        day: 0,
        wake_minute: 360,
        work_minutes: None,
        meal_minute: 1080,
        leisure_minutes: (1080, TAVERN_CLOSE_MINUTE),
        sleep_minute: 1380,
        leisure: PlannedLeisure::TavernMeal,
        leisure_status: PlannedLeisureStatus::Planned,
        planned_work_status: WorkStatus::LookingForWork,
    });
    person
}

fn tick(app: &mut App, real_seconds: f32) {
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs_f32(real_seconds));
    app.update();
}

#[test]
fn a_migrant_finishes_the_real_hall_queue_and_departure_before_tavern_leisure() {
    for warp in [1., 25.] {
        let (mut app, hall, tavern) = fixture(warp);
        app.add_systems(
            Update,
            (
                arrive_at_settlement,
                advance_immigration_departures,
                moot_services::advance_moot_service_queues,
                assign_tavern_routines,
                run_tavern_routines,
                queue_villager_travel_routes,
                retry_failed_routes_after_obstacle_change,
                plan_villager_travel_routes,
                crate::player::hero::step_units,
            )
                .chain(),
        );
        let person = visitor(&mut app, VillagerIntent::Travelling { settlement: hall });
        let entrance = SettlementBuildingKind::Hall.entrance_position(Vec3::new(0., 4., 0.), 0.);
        app.world_mut()
            .entity_mut(person)
            .insert(MoveTarget(entrance));
        let mut saw_queue = false;
        let mut saw_departure = false;
        let mut admitted = false;
        for _ in 0..6_000 {
            tick(&mut app, 1. / 60.);
            let world = app.world();
            saw_queue |= world.get::<MootQueueTicket>(person).is_some();
            saw_departure |= world
                .get::<super::super::population::ImmigrationDeparture>(person)
                .is_some();
            let resident = matches!(world.get::<VillagerIntent>(person),
                Some(VillagerIntent::Resident { settlement }) if *settlement == hall);
            if !resident {
                assert!(
                    world.get::<TavernVisitRoutine>(person).is_none(),
                    "a chosen town is not completed residence at {warp}x"
                );
                assert_eq!(
                    world
                        .get::<CharacterDayPlan>(person)
                        .unwrap()
                        .leisure_status,
                    PlannedLeisureStatus::Planned
                );
            } else {
                assert!(
                    saw_queue && saw_departure,
                    "registration must use the real counter and exit"
                );
                assert!(world.get::<MootQueueTicket>(person).is_none());
                assert!(world.get::<Residence>(person).is_some());
                assert_eq!(
                    world.get::<TavernVisitRoutine>(person).unwrap().tavern,
                    tavern
                );
                admitted = true;
                break;
            }
        }
        assert!(
            admitted,
            "the actual journey, queue and departure must complete at {warp}x"
        );
        assert_eq!(app.world().get::<Wallet>(person).unwrap().balance(), 1_000);
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(tavern)
                .unwrap()
                .amount(Good::Bread),
            5
        );
    }
}

#[test]
fn undecided_and_sailing_migrants_cannot_start_local_leisure() {
    let (mut app, hall, _) = fixture(25.);
    app.add_systems(Update, assign_tavern_routines);
    for intent in [
        VillagerIntent::Idle,
        VillagerIntent::ArrivingBySea { settlement: hall },
    ] {
        let person = visitor(&mut app, intent);
        let journey = MoveTarget(Vec3::new(0., 4., -40.));
        app.world_mut().entity_mut(person).insert(journey);
        app.update();
        assert!(app.world().get::<TavernVisitRoutine>(person).is_none());
        assert_eq!(app.world().get::<MoveTarget>(person).unwrap().0, journey.0);
    }
}

#[test]
fn tavern_failures_keep_the_shared_backoff_and_never_enable_direct_walking() {
    for warp in [1., 25.] {
        let (mut app, hall, tavern) = fixture(warp);
        let person = visitor(&mut app, VillagerIntent::Resident { settlement: hall });
        app.world_mut()
            .run_system_once(assign_tavern_routines)
            .unwrap();
        app.world_mut()
            .run_system_once(run_tavern_routines)
            .unwrap();
        let goal = app.world().get::<MoveTarget>(person).unwrap().0;
        let start = app.world().get::<PlayerPosition>(person).unwrap().0;
        // Supply the planner's actual failure result. The production retry
        // system creates its ordinary real-time backoff; no deadline is forged.
        app.world_mut()
            .entity_mut(person)
            .insert(NavigationRouteFailed { goal });
        app.world_mut()
            .run_system_once(retry_failed_routes_after_obstacle_change)
            .unwrap();
        assert!(app.world().get::<NavigationRouteBackoff>(person).is_some());
        app.add_systems(
            Update,
            (
                run_tavern_routines,
                queue_villager_travel_routes,
                retry_failed_routes_after_obstacle_change,
                crate::player::hero::step_units,
            )
                .chain(),
        );
        for _ in 0..20 {
            tick(&mut app, 1. / 60.);
            assert_eq!(
                app.world().get::<PlayerPosition>(person).unwrap().0,
                start,
                "a failed tavern route cannot walk during retry backoff at {warp}x"
            );
            assert!(app.world().get::<NavigationRouteFailed>(person).is_some());
            assert_eq!(
                app.world()
                    .get::<TavernVisitRoutine>(person)
                    .unwrap()
                    .failed_routes,
                1,
                "waiting on one failed attempt is not many new failures"
            );
        }
        tick(&mut app, 2.);
        assert!(app.world().get::<NavigationRoutePending>(person).is_some());
        assert_eq!(app.world().get::<PlayerPosition>(person).unwrap().0, start);
        // Distinct later planner outcomes still reach the existing bounded
        // give-up; preserving backoff must not make unreachable leisure eternal.
        for failure_count in 2..=MAX_TAVERN_ROUTE_FAILURES {
            app.world_mut()
                .entity_mut(person)
                .remove::<NavigationRoutePending>()
                .insert(NavigationRouteFailed { goal });
            tick(&mut app, 1. / 60.);
            if failure_count < MAX_TAVERN_ROUTE_FAILURES {
                assert_eq!(
                    app.world()
                        .get::<TavernVisitRoutine>(person)
                        .unwrap()
                        .failed_routes,
                    failure_count
                );
            }
        }
        assert!(app.world().get::<TavernVisitRoutine>(person).is_none());
        assert!(app.world().get::<MoveTarget>(person).is_none());
        assert_eq!(
            app.world()
                .get::<CharacterDayPlan>(person)
                .unwrap()
                .leisure_status,
            PlannedLeisureStatus::CouldNotReach
        );
        assert_eq!(app.world().get::<Wallet>(person).unwrap().balance(), 1_000);
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(tavern)
                .unwrap()
                .amount(Good::Bread),
            5
        );
    }
}
