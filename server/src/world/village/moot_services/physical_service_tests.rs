//! Physical admission and retained paid claims; timeouts are never transactions.
use super::*;
use shared::components::{ResidentOf, SettlementId, TimeWarp};
use shared::economy::MarketSeller;

fn dry_ground() -> WorldTerrain {
    let mut map = WorldTerrain::default().generator.loaded_map().clone();
    let bounds = shared::map::MapBounds {
        min: [-1600.0; 2],
        max: [1600.0; 2],
    };
    map.definition.bounds = bounds;
    map.definition.generated = None;
    map.heightmap = shared::map::HeightmapData::new(bounds, 2, 2, vec![4.0; 4], Some(-10.0));
    map.rivers = default();
    map.river_segments_by_chunk.clear();
    map.terrain_deltas_by_chunk.clear();
    WorldTerrain::from_loaded_map(map)
}

fn tick(app: &mut App, seconds: f32) {
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f32(seconds));
    app.update();
}

#[test]
fn disrupted_paid_meals_recover_physically_or_lose_only_destroyed_source_stock() {
    for warp in [1.0_f32, 25.0] {
        for disruption in [
            "lost_ticket",
            "unavailable_hall",
            "destroyed_hall",
            "full_bag",
        ] {
            let mut app = App::new();
            app.init_resource::<Time>()
                .init_resource::<MootQueueClock>()
                .insert_resource(dry_ground())
                .add_systems(
                    Update,
                    (
                        advance_moot_service_queues,
                        run_moot_meal_collections,
                        crate::player::hero::step_units,
                    )
                        .chain(),
                );
            app.world_mut().spawn(TimeWarp(warp));
            let hall_at = Vec3::new(0., 4., 0.);
            let settlement = Settlement {
                name: "Retained meal".into(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 1,
                treasury: 20,
            };
            let hall = app
                .world_mut()
                .spawn((
                    settlement.clone(),
                    PlayerPosition(hall_at),
                    PlayerRotation(0.),
                ))
                .id();
            let counter = world_slot(
                hall_at,
                0.,
                MootServiceLane::Resident,
                0,
                Some(app.world().resource::<WorldTerrain>()),
            );
            let mut cargo = GoodsInventory::new(if disruption == "full_bag" {
                Good::Wood.bulk_per_unit() * 2
            } else {
                30
            });
            assert_eq!(cargo.add(Good::Wood, 2), 2);
            let actor = app
                .world_mut()
                .spawn((
                    CharacterKind::Villager,
                    PlayerPosition(counter + Vec3::X * 12.),
                    shared::region::RegionCoord::from_world_pos(counter + Vec3::X * 12.),
                    PlayerRotation(0.),
                    CharacterActivity::Idle,
                    Nutrition::default(),
                    Wallet::new(80),
                    cargo,
                ))
                .id();
            let mut clock = MootQueueClock::default();
            reserve_meal(
                &mut app.world_mut().commands(),
                &mut clock,
                actor,
                hall,
                MootServiceKind::PoorRelief,
                Good::Bread,
                1,
            );
            app.world_mut().flush();
            app.insert_resource(clock);
            match disruption {
                "lost_ticket" => {
                    app.world_mut()
                        .entity_mut(actor)
                        .remove::<MootQueueTicket>();
                }
                "unavailable_hall" => {
                    app.world_mut().entity_mut(hall).remove::<Settlement>();
                }
                "destroyed_hall" => {
                    app.world_mut().despawn(hall);
                }
                _ => {}
            }
            if disruption == "unavailable_hall" || disruption == "destroyed_hall" {
                for _ in 0..(35.0 * 60.0 / warp).ceil() as usize {
                    tick(&mut app, 1.0 / 60.0);
                }
                assert_eq!(app.world().get::<Nutrition>(actor).unwrap().total_meals, 0);
                assert_eq!(app.world().get::<Wallet>(actor).unwrap().balance(), 80);
                let stock = app.world().get::<GoodsInventory>(actor).unwrap();
                assert_eq!(stock.amount(Good::Wood), 2);
                assert_eq!(stock.amount(Good::Bread), 0);
                if disruption == "destroyed_hall" {
                    assert!(app.world().get::<MootMealRoutine>(actor).is_none());
                    assert!(app.world().get::<MootQueueTicket>(actor).is_none());
                    assert!(app.world().get::<MoveTarget>(actor).is_none());
                    continue;
                }
                assert!(app.world().get::<MootMealRoutine>(actor).is_some());
                app.world_mut().entity_mut(hall).insert(settlement);
            }
            let mut counter_witness = false;
            let mut full_bag_eating_witness = false;
            for _ in 0..(120.0 * 60.0 / warp).ceil() as usize {
                let before = app.world().get::<PlayerPosition>(actor).unwrap().0;
                let had_bread = app
                    .world()
                    .get::<GoodsInventory>(actor)
                    .unwrap()
                    .amount(Good::Bread);
                let meals = app.world().get::<Nutrition>(actor).unwrap().total_meals;
                tick(&mut app, 1.0 / 60.0);
                if let Some(ticket) = app.world().get::<MootQueueTicket>(actor) {
                    assert_eq!(ticket.kind, MootServiceKind::PoorRelief);
                }
                let has_bread = app
                    .world()
                    .get::<GoodsInventory>(actor)
                    .unwrap()
                    .amount(Good::Bread);
                let fed = app.world().get::<Nutrition>(actor).unwrap().total_meals > meals;
                if has_bread > had_bread || (disruption == "full_bag" && fed) {
                    assert!(
                        ground_distance(before, counter) <= QUEUE_REACH,
                        "reserved food received away from its counter: {disruption}"
                    );
                    counter_witness = true;
                    if disruption == "full_bag" {
                        full_bag_eating_witness = matches!(
                            app.world().get::<MootMealRoutine>(actor).unwrap().phase,
                            MootMealPhase::Eating { .. }
                        );
                    }
                }
                if app.world().get::<MootMealRoutine>(actor).is_none() {
                    break;
                }
            }
            assert!(counter_witness, "no physical handover: {disruption}");
            assert!(disruption != "full_bag" || full_bag_eating_witness);
            assert!(app.world().get::<MootMealRoutine>(actor).is_none());
            assert_eq!(app.world().get::<Nutrition>(actor).unwrap().total_meals, 1);
            assert_eq!(app.world().get::<Wallet>(actor).unwrap().balance(), 80);
            assert_eq!(app.world().get::<Settlement>(hall).unwrap().treasury, 20);
            let stock = app.world().get::<GoodsInventory>(actor).unwrap();
            assert_eq!(stock.amount(Good::Wood), 2);
            assert_eq!(stock.amount(Good::Bread), 0);
        }
    }
}

#[test]
fn an_unregistered_migrant_keeps_the_real_journey_across_daily_meals_at_one_and_twenty_five_x() {
    for warp in [1.0, 25.0] {
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<MootQueueClock>()
            .init_resource::<SettlementEconomyRuntime>()
            .init_resource::<BusinessEventQueue>()
            .insert_resource(dry_ground());
        app.add_systems(
            Update,
            (
                update_settlement_economies,
                advance_moot_service_queues,
                run_moot_meal_collections,
                crate::player::hero::step_units,
            )
                .chain(),
        );
        let clock = app
            .world_mut()
            .spawn((WorldTime::new_default(), TimeWarp(warp)))
            .id();
        let id = SettlementId(70);
        let mut store = GoodsInventory::new(100);
        store.add(Good::Bread, 5);
        let mut market = MootMarket::founding();
        market.consign(MarketSeller::Treasury(id), Good::Bread, 5, 20);
        let hall = app
            .world_mut()
            .spawn((
                id,
                Settlement {
                    name: "Actual residents".into(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 0,
                    treasury: 0,
                },
                SettlementEconomy::default(),
                PlayerPosition(Vec3::new(0., 4., 0.)),
                PlayerRotation(0.),
                store,
                market,
            ))
            .id();
        let goal = SettlementBuildingKind::Hall.entrance_position(Vec3::new(0., 4., 0.), 0.);
        let start = Vec3::new(1000., 4., -15.);
        let migrant = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                CharacterActivity::Idle,
                ResidentOf(id),
                VillagerIntent::Travelling { settlement: hall },
                PlayerPosition(start),
                shared::region::RegionCoord::from_world_pos(start),
                PlayerRotation(0.),
                Wallet::new(100),
                Nutrition::default(),
                GoodsInventory::new(30),
                MoveTarget(goal),
            ))
            .id();
        tick(&mut app, 0.01);
        app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 1;
        for _ in 0..(60.0 * 60.0 / warp).ceil() as usize {
            tick(&mut app, 1.0 / 60.0);
            assert!(app.world().get::<MootQueueTicket>(migrant).is_none());
            assert!(app.world().get::<MootMealRoutine>(migrant).is_none());
            assert_eq!(app.world().get::<MoveTarget>(migrant).unwrap().0, goal);
        }
        let end = app.world().get::<PlayerPosition>(migrant).unwrap().0;
        assert!(
            ground_distance(start, end) > 10.,
            "the real mover did not advance the migrant"
        );
        assert!(
            ground_distance(end, goal) > 500.,
            "the fixture must remain outside the town"
        );
        assert_eq!(app.world().get::<Wallet>(migrant).unwrap().balance(), 100);
        assert_eq!(
            app.world().get::<Nutrition>(migrant).unwrap(),
            &Nutrition::default()
        );
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(hall)
                .unwrap()
                .amount(Good::Bread),
            5
        );
        assert_eq!(
            app.world()
                .get::<MootMarket>(hall)
                .unwrap()
                .listed_units(Good::Bread),
            5
        );
    }
}

#[test]
fn remote_failed_services_retain_paid_rations_and_never_become_ready() {
    for warp in [1.0, 25.0] {
        for kind in [
            MootServiceKind::Permit,
            MootServiceKind::HouseholdShopping,
            MootServiceKind::PersonalMeal,
            MootServiceKind::PoorRelief,
            MootServiceKind::ConstructionMaterial,
        ] {
            let mut app = App::new();
            app.init_resource::<Time>()
                .init_resource::<MootQueueClock>()
                .insert_resource(dry_ground());
            app.add_systems(
                Update,
                (advance_moot_service_queues, run_moot_meal_collections).chain(),
            );
            app.world_mut().spawn(TimeWarp(warp));
            let hall = app
                .world_mut()
                .spawn((
                    Settlement {
                        name: "Blocked counter".into(),
                        tier: shared::components::SettlementTier::Hamlet,
                        residents: 1,
                        treasury: 0,
                    },
                    PlayerPosition(Vec3::new(0., 4., 0.)),
                    PlayerRotation(0.),
                ))
                .id();
            let mut cargo = GoodsInventory::new(30);
            cargo.add(Good::Wood, 2);
            let actor = app
                .world_mut()
                .spawn((
                    CharacterKind::Villager,
                    PlayerPosition(Vec3::new(1000., 4., 0.)),
                    shared::region::RegionCoord::from_world_pos(Vec3::new(1000., 4., 0.)),
                    PlayerRotation(0.),
                    CharacterActivity::Idle,
                    Nutrition::default(),
                    Wallet::new(80),
                    cargo.clone(),
                    MootQueueTicket {
                        hall,
                        serial: 1,
                        kind,
                        state: MootQueueState::Queued,
                        failed_routes: MAX_QUEUE_ROUTE_FAILURES - 1,
                        head_wait_seconds: 0.,
                        head_best_distance: f32::INFINITY,
                        head_route_progress: None,
                    },
                    NavigationRouteFailed {
                        goal: Vec3::new(0., 4., -6.),
                    },
                ))
                .id();
            let paid_meal = matches!(
                kind,
                MootServiceKind::PersonalMeal | MootServiceKind::PoorRelief
            );
            if paid_meal {
                app.world_mut().entity_mut(actor).insert(MootMealRoutine {
                    hall,
                    kind,
                    good: Good::Bread,
                    meal_day: 1,
                    phase: MootMealPhase::Queueing,
                });
            }
            tick(&mut app, 1.0 / 60.0);
            assert!(matches!(
                app.world().get::<MootQueueTicket>(actor).unwrap().state,
                MootQueueState::Retrying { .. }
            ));
            for _ in 0..(60.0 * 60.0 / warp).ceil() as usize {
                tick(&mut app, 1.0 / 60.0);
                assert!(
                    !app.world()
                        .get::<MootQueueTicket>(actor)
                        .unwrap()
                        .is_ready()
                );
                assert_eq!(
                    app.world().get::<MootMealRoutine>(actor).is_some(),
                    paid_meal
                );
                assert_eq!(app.world().get::<GoodsInventory>(actor), Some(&cargo));
                assert_eq!(app.world().get::<Nutrition>(actor).unwrap().total_meals, 0);
                assert_eq!(app.world().get::<Wallet>(actor).unwrap().balance(), 80);
                assert!(
                    app.world().get::<MootQueueTransit>(actor).is_none(),
                    "remote trip used a local queue bypass"
                );
            }
        }
    }
}

#[test]
fn a_paid_meal_retries_then_walks_to_the_counter_before_receiving_or_eating() {
    for warp in [1.0, 25.0] {
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<MootQueueClock>()
            .insert_resource(dry_ground());
        app.add_systems(
            Update,
            (
                advance_moot_service_queues,
                run_moot_meal_collections,
                crate::player::hero::step_units,
            )
                .chain(),
        );
        app.world_mut().spawn(TimeWarp(warp));
        let hall_at = Vec3::new(0., 4., 0.);
        let hall = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Paid retry".into(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 1,
                    treasury: 0,
                },
                PlayerPosition(hall_at),
                PlayerRotation(0.),
            ))
            .id();
        let counter = world_slot(
            hall_at,
            0.,
            MootServiceLane::Resident,
            0,
            Some(app.world().resource::<WorldTerrain>()),
        );
        let start = counter + Vec3::X * 12.;
        let mut cargo = GoodsInventory::new(30);
        cargo.add(Good::Wood, 2);
        let actor = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(start),
                shared::region::RegionCoord::from_world_pos(start),
                PlayerRotation(0.),
                CharacterActivity::Idle,
                Nutrition::default(),
                Wallet::new(80),
                cargo,
                MootQueueTicket {
                    hall,
                    serial: 1,
                    kind: MootServiceKind::PersonalMeal,
                    state: MootQueueState::Queued,
                    failed_routes: MAX_QUEUE_ROUTE_FAILURES - 1,
                    head_wait_seconds: 0.,
                    head_best_distance: f32::INFINITY,
                    head_route_progress: None,
                },
                NavigationRouteFailed { goal: counter },
                MootMealRoutine {
                    hall,
                    kind: MootServiceKind::PersonalMeal,
                    good: Good::Bread,
                    meal_day: 1,
                    phase: MootMealPhase::Queueing,
                },
            ))
            .id();
        tick(&mut app, 1.0 / 60.0);
        assert!(matches!(
            app.world().get::<MootQueueTicket>(actor).unwrap().state,
            MootQueueState::Retrying { .. }
        ));
        let mut witnessed_pickup = false;
        for _ in 0..(120.0 * 60.0 / warp).ceil() as usize {
            let before = app.world().get::<PlayerPosition>(actor).unwrap().0;
            let had_bread = app
                .world()
                .get::<GoodsInventory>(actor)
                .unwrap()
                .amount(Good::Bread)
                > 0;
            tick(&mut app, 1.0 / 60.0);
            if !had_bread
                && app
                    .world()
                    .get::<GoodsInventory>(actor)
                    .unwrap()
                    .amount(Good::Bread)
                    > 0
            {
                assert!(
                    ground_distance(before, counter) <= QUEUE_REACH,
                    "ration acquired away from its physical counter"
                );
                witnessed_pickup = true;
            }
            if app.world().get::<MootMealRoutine>(actor).is_none() {
                break;
            }
        }
        assert!(witnessed_pickup);
        assert!(app.world().get::<MootMealRoutine>(actor).is_none());
        assert_eq!(app.world().get::<Nutrition>(actor).unwrap().total_meals, 1);
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(actor)
                .unwrap()
                .amount(Good::Bread),
            0
        );
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
