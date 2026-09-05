//! Village households regression fixtures and invariants.

use super::*;

#[test]
fn transient_actor_requests_are_aggregated_on_the_stable_building() {
    let mut app = village_test_app();
    app.add_systems(Update, sync_building_door_demands);
    let position = Vec3::new(12.0, 3.0, -8.0);
    let hall = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Doorford".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            },
            PlayerPosition(position),
        ))
        .id();
    let visitor = app
        .world_mut()
        .spawn(BuildingDoorUse { building: position })
        .id();

    app.update();
    assert!(
        app.world()
            .entity(hall)
            .get::<BuildingDoorDemand>()
            .unwrap()
            .open
    );

    app.world_mut()
        .entity_mut(visitor)
        .remove::<BuildingDoorUse>();
    app.update();
    assert!(
        !app.world()
            .entity(hall)
            .get::<BuildingDoorDemand>()
            .unwrap()
            .open
    );
}

#[test]
fn housed_villager_walks_through_the_door_at_night_and_back_out_at_dawn() {
    use crate::player::hero::step_units;
    use shared::components::{CharacterKind, TimeWarp};
    use shared::region::RegionCoord;

    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.insert_resource(WorldTerrain::default());
    app.add_systems(
        Update,
        (
            ensure_households,
            assign_households,
            run_household_schedules,
            step_units,
        )
            .chain(),
    );

    let (hall_position, house_position) = {
        let terrain = app.world().resource::<WorldTerrain>();
        let hall = Vec3::new(1720.0, terrain.get_height(1720.0, 0.0), 0.0);
        let house = Vec3::new(1702.0, terrain.get_height(1702.0, -5.0), -5.0);
        (hall, house)
    };
    let clock = app
        .world_mut()
        .spawn((WorldTime::new(100.0, 20.0, 100.0), TimeWarp::clamped(100.0)))
        .id();
    let settlement = app
        .world_mut()
        .spawn(Settlement {
            name: "Nightford".to_string(),
            tier: shared::components::SettlementTier::Hamlet,
            residents: 1,
            treasury: 0,
        })
        .id();
    let house = app
        .world_mut()
        .spawn((
            SettlementBuilding {
                kind: SettlementBuildingKind::House,
                settlement: "Nightford".to_string(),
                owner: Some("Ada".to_string()),
                quality: 0.5,
                workers: Vec::new(),
            },
            PlayerPosition(house_position),
            PlayerRotation(0.0),
        ))
        .id();
    let villager = app
        .world_mut()
        .spawn((
            CharacterName("Ada".to_string()),
            CharacterKind::Villager,
            VillagerIntent::Resident { settlement },
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
            RegionCoord::from_world_pos(hall_position),
            CharacterActivity::Idle,
        ))
        .id();

    let step = std::time::Duration::from_secs_f32(1.0 / 60.0);
    let mut door_request_ticks = 0;
    for _ in 0..60 {
        app.world_mut().resource_mut::<Time>().advance_by(step);
        app.update();
        door_request_ticks +=
            usize::from(app.world().entity(villager).contains::<BuildingDoorUse>());
    }

    let door = SettlementBuildingKind::House.entrance_position(house_position, 0.0);
    let inside = SettlementBuildingKind::House.interior_door_position(house_position, 0.0);
    let household = app.world().entity(house).get::<Household>().unwrap();
    assert_eq!(household.residents, vec!["Ada"]);
    assert!(
        door_request_ticks > 0,
        "the villager must request the door while crossing at full time warp"
    );
    assert_eq!(
        *app.world()
            .entity(villager)
            .get::<CharacterActivity>()
            .unwrap(),
        CharacterActivity::Indoors
    );
    let sleeping_at = app
        .world()
        .entity(villager)
        .get::<PlayerPosition>()
        .unwrap()
        .0;
    assert!(ground_distance(sleeping_at, inside) <= DOOR_REACH);
    assert!(
        ground_distance(sleeping_at, house_position) < ground_distance(door, house_position),
        "the villager must cross the wall plane instead of vanishing outside"
    );

    // Reproduce the real navigation footprint only after the villager is
    // asleep. The old DOOR_REACH completion released BuildingDoorUse a
    // fraction before this blocker ended, so the first route to work began
    // from an impossible point inside the cabin.
    let mut obstacles = SpatialObstacleGrid::default();
    let definition = shared::building::BuildingType::LogCabin.definition();
    obstacles.insert(shared::spatial::ObstacleEntry {
        center: Vec2::new(house_position.x, house_position.z),
        half_extents: definition.footprint * 0.5
            + Vec2::splat(shared::physics::CHARACTER_NAV_RADIUS),
        rotation: 0.0,
        obstacle_type: shared::building::BuildingType::LogCabin as u32,
    });
    app.insert_resource(obstacles);

    app.world_mut()
        .entity_mut(clock)
        .get_mut::<WorldTime>()
        .unwrap()
        .seconds_in_cycle = 0.0;
    let mut exit_door_request_ticks = 0;
    for _ in 0..30 {
        app.world_mut().resource_mut::<Time>().advance_by(step);
        app.update();
        exit_door_request_ticks +=
            usize::from(app.world().entity(villager).contains::<BuildingDoorUse>());
    }

    let villager_ref = app.world().entity(villager);
    assert!(
        exit_door_request_ticks > 0,
        "the villager must request the door while leaving at full time warp"
    );
    assert!(!villager_ref.contains::<HomeRoutine>());
    assert!(!villager_ref.contains::<BuildingDoorUse>());
    assert_eq!(
        *villager_ref.get::<CharacterActivity>().unwrap(),
        CharacterActivity::Idle
    );
    let outside = exterior_door_clearance_position(house_position, door);
    assert!(
        ground_distance(villager_ref.get::<PlayerPosition>().unwrap().0, outside) <= DOOR_REACH,
        "the morning crossing must finish beyond the barely-clear door anchor"
    );
    let exited = villager_ref.get::<PlayerPosition>().unwrap().0;
    assert!(
        !app.world()
            .resource::<SpatialObstacleGrid>()
            .point_blocked(Vec2::new(exited.x, exited.z)),
        "morning must not release the villager while still inside the cabin blocker"
    );
}

#[test]
fn night_schedule_waits_for_a_loaded_worker_to_finish_the_workplace_handoff() {
    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.add_systems(Update, run_household_schedules);
    app.world_mut().spawn(WorldTime::new(100.0, 20.0, 105.0));

    let settlement = app
        .world_mut()
        .spawn(Settlement {
            name: "Last Basket".into(),
            tier: shared::components::SettlementTier::Hamlet,
            residents: 1,
            treasury: 0,
        })
        .id();
    let resident_id = shared::components::PersonId(8_081);
    let home_position = Vec3::new(20.0, 3.0, 0.0);
    let home = app
        .world_mut()
        .spawn((
            SettlementBuilding {
                kind: SettlementBuildingKind::House,
                settlement: "Last Basket".into(),
                owner: None,
                quality: 1.0,
                workers: Vec::new(),
            },
            PlayerPosition(home_position),
            PlayerRotation(0.0),
            Household {
                resident_ids: vec![resident_id],
                residents: vec!["Ada".into()],
            },
        ))
        .id();
    let mut cargo = GoodsInventory::new(shared::economy::capacity::VILLAGER);
    assert_eq!(cargo.add(Good::Wheat, 1), 1);
    let worker = app
        .world_mut()
        .spawn((
            resident_id,
            CharacterKind::Villager,
            PlayerPosition(Vec3::ZERO),
            PlayerRotation(0.0),
            CharacterActivity::Idle,
            HomeAssignment { home },
            VillagerIntent::Resident { settlement },
            cargo,
            FarmerRoutine {
                farmstead: settlement,
                field: settlement,
                hall: settlement,
                work_stand: Vec3::ZERO,
                harvest_seconds: 0.0,
                failed_workplace_routes: 0,
                production_day: 0,
                produced_today: 1,
                phase: FarmerPhase::ReturningToFarmstead,
            },
        ))
        .id();

    app.update();
    assert!(app.world().get::<HomeRoutine>(worker).is_none());
    assert!(matches!(
        app.world().get::<FarmerRoutine>(worker).unwrap().phase,
        FarmerPhase::ReturningToFarmstead
    ));
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(worker)
            .unwrap()
            .amount(Good::Wheat),
        1,
        "nightfall must not steal or redirect a loaded trade routine"
    );

    assert_eq!(
        app.world_mut()
            .get_mut::<GoodsInventory>(worker)
            .unwrap()
            .remove(Good::Wheat, 1),
        1
    );
    app.update();
    assert!(
        app.world().get::<HomeRoutine>(worker).is_some(),
        "once unloaded, the same worker should immediately become eligible to go home"
    );
}

#[test]
fn an_unreachable_night_route_cannot_leave_a_resident_outdoors_forever() {
    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.add_systems(Update, run_household_schedules);
    app.world_mut().spawn(WorldTime::new(100.0, 20.0, 105.0));

    let settlement = app
        .world_mut()
        .spawn(Settlement {
            name: "Shelterford".into(),
            tier: shared::components::SettlementTier::Hamlet,
            residents: 1,
            treasury: 0,
        })
        .id();
    let resident_id = shared::components::PersonId(77);
    let home_position = Vec3::new(20.0, 3.0, 0.0);
    let home = app
        .world_mut()
        .spawn((
            SettlementBuilding {
                kind: SettlementBuildingKind::House,
                settlement: "Shelterford".into(),
                owner: None,
                quality: 1.0,
                workers: Vec::new(),
            },
            PlayerPosition(home_position),
            PlayerRotation(0.0),
            Household {
                resident_ids: vec![resident_id],
                residents: vec!["Ada".into()],
            },
        ))
        .id();
    let door = SettlementBuildingKind::House.entrance_position(home_position, 0.0);
    let villager = app
        .world_mut()
        .spawn((
            resident_id,
            CharacterKind::Villager,
            PlayerPosition(Vec3::new(-20.0, 3.0, 0.0)),
            PlayerRotation(0.0),
            CharacterActivity::Idle,
            HomeAssignment { home },
            VillagerIntent::Resident { settlement },
            HomeRoutine {
                home,
                phase: HomePhase::GoingToDoor,
                failed_routes: 0,
            },
            MoveTarget(door),
            NavigationRouteFailed { goal: door },
        ))
        .id();

    for expected_failures in 1..=2 {
        app.update();
        let resident = app.world().entity(villager);
        assert_eq!(
            resident.get::<HomeRoutine>().unwrap().failed_routes,
            expected_failures
        );
        assert!(resident.get::<NavigationRouteFailed>().is_none());
        assert_eq!(resident.get::<MoveTarget>().unwrap().0, door);
        app.world_mut()
            .entity_mut(villager)
            .insert(NavigationRouteFailed { goal: door });
    }

    app.update();
    let resident = app.world().entity(villager);
    assert_eq!(
        resident.get::<HomeRoutine>().unwrap().phase,
        HomePhase::Sleeping
    );
    assert_eq!(
        *resident.get::<CharacterActivity>().unwrap(),
        CharacterActivity::Indoors
    );
    assert!(resident.get::<NavigationRouteFailed>().is_none());
    assert!(resident.get::<MoveTarget>().is_none());
    assert_eq!(
        resident.get::<PlayerPosition>().unwrap().0,
        SettlementBuildingKind::House.interior_door_position(home_position, 0.0)
    );
}

#[test]
fn household_capacity_is_entity_safe_when_names_repeat() {
    let mut app = village_test_app();
    app.add_systems(Update, (ensure_households, assign_households).chain());

    let settlement = app
        .world_mut()
        .spawn(Settlement {
            name: "Twinstead".to_string(),
            tier: shared::components::SettlementTier::Hamlet,
            residents: 5,
            treasury: 0,
        })
        .id();
    let houses: Vec<_> = [Vec3::ZERO, Vec3::new(30.0, 0.0, 0.0)]
        .into_iter()
        .map(|position| {
            app.world_mut()
                .spawn((
                    SettlementBuilding {
                        kind: SettlementBuildingKind::House,
                        settlement: "Twinstead".to_string(),
                        owner: None,
                        quality: 0.5,
                        workers: Vec::new(),
                    },
                    PlayerPosition(position),
                ))
                .id()
        })
        .collect();
    let villagers: Vec<_> = (0..5)
        .map(|index| {
            app.world_mut()
                .spawn((
                    CharacterName("Robin".to_string()),
                    VillagerIntent::Resident { settlement },
                    PlayerPosition(Vec3::new(index as f32, 0.0, 0.0)),
                ))
                .id()
        })
        .collect();

    app.update();

    let mut assignments = HashMap::<Entity, usize>::new();
    for villager in &villagers {
        let assignment = app
            .world()
            .entity(*villager)
            .get::<HomeAssignment>()
            .expect("every resident fits across the two cabins");
        *assignments.entry(assignment.home).or_default() += 1;
    }
    assert_eq!(assignments.values().sum::<usize>(), 5);
    assert!(
        assignments.values().all(|used| *used <= 4),
        "a repeated display name must not overbook a four-bed cabin: {assignments:?}"
    );
    let rostered: usize = houses
        .iter()
        .map(|house| {
            app.world()
                .entity(*house)
                .get::<Household>()
                .unwrap()
                .residents
                .len()
        })
        .sum();
    assert_eq!(rostered, 5);

    let stable_roster: HashSet<_> = houses
        .iter()
        .flat_map(|house| {
            app.world()
                .entity(*house)
                .get::<Household>()
                .unwrap()
                .resident_ids
                .clone()
        })
        .collect();
    app.world_mut()
        .entity_mut(villagers[0])
        .get_mut::<CharacterName>()
        .unwrap()
        .0 = "Marian".to_string();
    app.update();
    let renamed_roster: HashSet<_> = houses
        .iter()
        .flat_map(|house| {
            app.world()
                .entity(*house)
                .get::<Household>()
                .unwrap()
                .resident_ids
                .clone()
        })
        .collect();
    assert_eq!(stable_roster, renamed_roster);
    assert!(houses.iter().any(|house| app
        .world()
        .entity(*house)
        .get::<Household>()
        .unwrap()
        .residents
        .iter()
        .any(|name| name == "Marian")));
}

#[test]
fn failed_household_shop_routes_release_or_retry_without_losing_food() {
    let mut app = village_test_app();
    app.add_systems(Update, run_household_shopping);
    app.world_mut().spawn(WorldTime::new_default());

    let settlement_id = shared::components::SettlementId(41);
    let hall_position = Vec3::new(0.0, 3.0, 0.0);
    let hall = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Shopford".into(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 2,
                treasury: 0,
            },
            settlement_id,
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
            GoodsInventory::new(shared::economy::capacity::HALL),
            MootMarket::founding(),
        ))
        .id();
    let home_position = Vec3::new(20.0, 3.0, 0.0);
    let home = app
        .world_mut()
        .spawn((
            SettlementBuilding {
                kind: SettlementBuildingKind::House,
                settlement: "Shopford".into(),
                owner: None,
                quality: 1.0,
                workers: Vec::new(),
            },
            PlayerPosition(home_position),
            PlayerRotation(0.0),
            Household::default(),
            HouseholdEconomy::default(),
            GoodsInventory::new(shared::economy::capacity::HOUSE),
        ))
        .id();

    let hall_entrance = SettlementBuildingKind::Hall.entrance_position(hall_position, 0.0);
    let outbound = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            PlayerPosition(home_position),
            CharacterActivity::Idle,
            GoodsInventory::new(shared::economy::capacity::VILLAGER),
            HouseholdShoppingRoutine {
                home,
                hall,
                counter: hall_entrance,
                phase: HouseholdShoppingPhase::GoingToMarket,
            },
            MoveTarget(hall_entrance),
            NavigationRouteFailed {
                goal: hall_entrance,
            },
        ))
        .id();

    let mut paid_food = GoodsInventory::new(shared::economy::capacity::VILLAGER);
    assert_eq!(paid_food.add(Good::Wheat, 1), 1);
    let returning = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            PlayerPosition(hall_position),
            CharacterActivity::Idle,
            paid_food,
            HouseholdShoppingRoutine {
                home,
                hall,
                counter: hall_entrance,
                phase: HouseholdShoppingPhase::ReturningHome,
            },
            MoveTarget(home_position),
            NavigationRouteFailed {
                goal: home_position,
            },
        ))
        .id();

    app.update();

    let outbound = app.world().entity(outbound);
    assert!(outbound.get::<HouseholdShoppingRoutine>().is_none());
    assert!(outbound.get::<MoveTarget>().is_none());
    assert!(outbound.get::<NavigationRouteFailed>().is_none());

    let returning = app.world().entity(returning);
    assert_eq!(
        returning
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Wheat),
        1,
        "a failed return must retain already purchased physical food"
    );
    assert_eq!(
        returning.get::<HouseholdShoppingRoutine>().unwrap().phase,
        HouseholdShoppingPhase::ReturningHome
    );
    assert!(returning.get::<NavigationRouteFailed>().is_none());
    assert_eq!(
        returning.get::<MoveTarget>().unwrap().0,
        SettlementBuildingKind::House.entrance_position(home_position, 0.0)
    );
}

#[test]
fn travelling_villagers_do_not_occupy_resident_beds() {
    let mut app = village_test_app();
    app.add_systems(Update, (ensure_households, assign_households).chain());

    let settlement = app
        .world_mut()
        .spawn(Settlement {
            name: "Arrival".to_string(),
            tier: shared::components::SettlementTier::Hamlet,
            residents: 7,
            treasury: 0,
        })
        .id();
    let houses: Vec<_> = [Vec3::ZERO, Vec3::new(30.0, 0.0, 0.0)]
        .into_iter()
        .map(|position| {
            app.world_mut()
                .spawn((
                    SettlementBuilding {
                        kind: SettlementBuildingKind::House,
                        settlement: "Arrival".to_string(),
                        owner: None,
                        quality: 0.5,
                        workers: Vec::new(),
                    },
                    PlayerPosition(position),
                ))
                .id()
        })
        .collect();
    let traveller = app
        .world_mut()
        .spawn((
            CharacterName("Still On The Road".to_string()),
            VillagerIntent::Travelling { settlement },
            PlayerPosition(Vec3::ZERO),
        ))
        .id();
    let residents: Vec<_> = (0..7)
        .map(|index| {
            app.world_mut()
                .spawn((
                    CharacterName(format!("Resident {index}")),
                    VillagerIntent::Resident { settlement },
                    PlayerPosition(Vec3::new(10.0 + index as f32, 0.0, 0.0)),
                ))
                .id()
        })
        .collect();

    app.update();

    assert!(app.world().get::<HomeAssignment>(traveller).is_none());
    assert!(residents
        .iter()
        .all(|resident| app.world().get::<HomeAssignment>(*resident).is_some()));
    let occupied: usize = houses
        .iter()
        .map(|house| {
            app.world()
                .get::<Household>(*house)
                .unwrap()
                .residents
                .len()
        })
        .sum();
    assert_eq!(occupied, 7, "occupied beds must equal actual residents");
}

#[test]
fn residents_eat_bread_fish_then_household_flour_but_never_raw_wheat() {
    let mut app = village_test_app();
    app.init_resource::<SettlementEconomyRuntime>();
    app.add_systems(
        Update,
        (ensure_settlement_economies, update_settlement_economies).chain(),
    );
    let clock = app.world_mut().spawn(WorldTime::new_default()).id();
    let mut stock = GoodsInventory::new(shared::economy::capacity::HALL);
    assert_eq!(stock.add(Good::Bread, 1), 1);
    assert_eq!(stock.add(Good::Food, 1), 1);
    assert_eq!(stock.add(Good::Flour, 3), 3);
    assert_eq!(stock.add(Good::Wheat, 3), 3);
    assert_eq!(stock.add(Good::Wood, 1), 1);
    let hall = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Dailybread".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 4,
                treasury: 0,
            },
            stock,
        ))
        .id();

    app.update();
    app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 1;
    app.update();

    let inventory = app.world().get::<GoodsInventory>(hall).unwrap();
    assert_eq!(inventory.amount(Good::Bread), 0);
    assert_eq!(inventory.amount(Good::Food), 0);
    assert_eq!(inventory.amount(Good::Flour), 1);
    assert_eq!(inventory.amount(Good::Wheat), 3);
    assert_eq!(inventory.amount(Good::Wood), 1);
    let economy = app.world().get::<SettlementEconomy>(hall).unwrap();
    assert_eq!(economy.edible_stock, 1);
    assert_eq!(economy.recent_food_consumption, 4.0);
    assert_eq!(economy.unmet_food, 0);

    app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 2;
    app.update();
    let economy = app.world().get::<SettlementEconomy>(hall).unwrap();
    assert_eq!(economy.edible_stock, 0);
    assert_eq!(economy.recent_food_consumption, 2.5);
    assert_eq!(economy.unmet_food, 3);
}

#[test]
fn marketplace_finish_unlocks_the_settlements_public_trade_tier() {
    let mut app = village_test_app();
    app.add_systems(Update, update_moot_market_targets);
    let settlement_id = shared::components::SettlementId(690);
    let hall = app
        .world_mut()
        .spawn((
            settlement_id,
            Settlement {
                name: "Tierford".to_string(),
                tier: shared::components::SettlementTier::Village,
                residents: 12,
                treasury: 0,
            },
            GoodsInventory::new(shared::economy::capacity::HALL),
            MootMarket::founding(),
        ))
        .id();

    app.update();
    let market = app.world().get::<MootMarket>(hall).unwrap();
    assert_eq!(market.trade_tier(), shared::economy::MarketTradeTier::Moot);
    assert!(!market.can_trade(Good::Iron));

    let marketplace = app
        .world_mut()
        .spawn((
            SettlementBuilding {
                kind: SettlementBuildingKind::Market,
                settlement: "Tierford".to_string(),
                owner: None,
                quality: 1.0,
                workers: Vec::new(),
            },
            shared::components::BuildingOf(settlement_id),
            shared::components::MarketLevel::Earthen,
        ))
        .id();
    app.update();
    let market = app.world().get::<MootMarket>(hall).unwrap();
    assert_eq!(
        market.trade_tier(),
        shared::economy::MarketTradeTier::Marketplace
    );
    assert!(!market.can_trade(Good::Iron));

    app.world_mut()
        .entity_mut(marketplace)
        .insert(shared::components::MarketLevel::Paved);
    app.update();
    let market = app.world().get::<MootMarket>(hall).unwrap();
    assert_eq!(
        market.trade_tier(),
        shared::economy::MarketTradeTier::PavedMarketplace
    );
    assert!(market.can_trade(Good::Iron));
}
