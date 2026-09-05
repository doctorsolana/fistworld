//! Village production regression fixtures and invariants.

use super::*;

#[test]
fn field_quality_controls_continuous_wheat_rate() {
    assert!((farmer_seconds_per_wheat(1.0) - 120.0).abs() < 0.01);
    assert!((farmer_seconds_per_wheat(2.0 / 3.0) - 180.0).abs() < 0.01);
    assert!((farmer_seconds_per_wheat(0.5) - 240.0).abs() < 0.01);
    assert!(farmer_seconds_per_wheat(0.1) > farmer_seconds_per_wheat(0.5));

    let full_shift_seconds = WorldTime::DEFAULT_ORDINARY_SHIFT_SECONDS;
    assert!((full_shift_seconds / farmer_seconds_per_wheat(1.0) - 6.0).abs() < 0.01);
    assert!((full_shift_seconds / farmer_seconds_per_wheat(2.0 / 3.0) - 4.0).abs() < 0.01);
}

#[test]
fn a_farmer_carries_wheat_only_to_the_farmstead_store() {
    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.init_resource::<SettlementEconomyRuntime>();
    app.add_systems(Update, run_farmer_routines);

    let hall = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Barleywick".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            },
            GoodsInventory::new(shared::economy::capacity::HALL),
        ))
        .id();
    let clock = app
        .world_mut()
        .spawn((
            WorldTime::new_default(),
            shared::components::TimeWarp::clamped(1.0),
        ))
        .id();

    let farm_position = Vec3::new(20.0, 4.0, 0.0);
    let farm = app
        .world_mut()
        .spawn((
            SettlementBuilding {
                kind: SettlementBuildingKind::Farmstead,
                settlement: "Barleywick".to_string(),
                owner: Some("Ada".to_string()),
                quality: 0.9,
                workers: vec!["Ada".to_string()],
            },
            PlayerPosition(farm_position),
            PlayerRotation(0.0),
            GoodsInventory::new(shared::economy::capacity::FARMSTEAD),
        ))
        .id();
    let field_position = SettlementBuildingKind::Farmstead
        .field_position_at(farm_position, 0.0, 0)
        .unwrap();
    let field = app
        .world_mut()
        .spawn((
            FarmField {
                settlement: "Barleywick".to_string(),
                farmstead: farm_position,
                plot_index: 0,
                quality: 0.9,
            },
            PlayerPosition(field_position),
            PlayerRotation(0.0),
        ))
        .id();
    let farmer = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            CharacterName("Ada".to_string()),
            CharacterAttributes::default(),
            VillagerIntent::Resident { settlement: hall },
            PlayerPosition(field_position),
            CharacterActivity::Farming,
            GoodsInventory::new(shared::economy::capacity::VILLAGER),
            FarmerRoutine {
                farmstead: farm,
                field,
                hall,
                work_stand: field_position,
                harvest_seconds: farmer_seconds_per_wheat(0.9),
                failed_workplace_routes: 0,
                production_day: u32::MAX,
                produced_today: 0,
                phase: FarmerPhase::Farming,
            },
        ))
        .id();

    app.update();
    assert_eq!(
        app.world()
            .entity(farmer)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Wheat),
        0,
        "partial basket progress must keep the visible harvest animation active"
    );
    assert_eq!(
        app.world()
            .entity(farm)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Wheat),
        0
    );

    // Completing the second unit materialises the whole field basket and
    // begins the return trip on that same simulation tick; it is not a
    // daily production ceiling.
    app.world_mut()
        .entity_mut(farmer)
        .get_mut::<FarmerRoutine>()
        .unwrap()
        .harvest_seconds = farmer_seconds_per_wheat(0.9) * 2.0;
    app.update();
    assert_eq!(
        app.world()
            .entity(farmer)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Wheat),
        2,
        "the farmer should fill a two-Wheat carrying batch"
    );

    let farm_entrance = SettlementBuildingKind::Farmstead.entrance_position(farm_position, 0.0);
    app.world_mut()
        .entity_mut(farmer)
        .insert(NavigationRouteFailed {
            goal: farm_entrance,
        });
    app.update();
    assert!(app.world().entity(farmer).get::<FarmerRoutine>().is_some());
    assert!(app.world().entity(farmer).get::<WorkerOffDuty>().is_none());
    assert!(app
        .world()
        .entity(farmer)
        .get::<NavigationRouteFailed>()
        .is_none());
    assert_eq!(
        app.world()
            .entity(farmer)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Wheat),
        2,
        "a failed loaded return must retain the basket and retry this shift"
    );
    app.world_mut()
        .entity_mut(farmer)
        .get_mut::<PlayerPosition>()
        .unwrap()
        .0 = farm_entrance;
    app.update();

    assert_eq!(
        app.world()
            .entity(farmer)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Wheat),
        0
    );
    assert_eq!(
        app.world()
            .entity(farm)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Wheat),
        2
    );
    assert_eq!(
        app.world()
            .entity(hall)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Wheat),
        0,
        "the farmer must never bypass the Farmstead to sell at the hall"
    );
    assert!(app.world().entity(farmer).get::<FarmerRoutine>().is_some());
    assert!(app.world().entity(farmer).get::<WorkerOffDuty>().is_none());
    assert!(app
        .world()
        .entity(farmer)
        .get::<MarketCollectionRoutine>()
        .is_none());

    // A full workplace is real backpressure: the worker must keep even a
    // partial last basket until a porter creates room. The shift may end only
    // after that basket is physically deposited.
    let farm_wheat_before_backpressure = {
        let mut farm_entity = app.world_mut().entity_mut(farm);
        let mut farm_store = farm_entity.get_mut::<GoodsInventory>().unwrap();
        farm_store.add(Good::Wheat, u32::MAX);
        farm_store.amount(Good::Wheat)
    };
    assert_eq!(
        app.world_mut()
            .entity_mut(farmer)
            .get_mut::<GoodsInventory>()
            .unwrap()
            .add(Good::Wheat, 1),
        1
    );
    app.world_mut()
        .entity_mut(farmer)
        .remove::<WorkplaceDoorTransit>()
        .remove::<BuildingDoorUse>()
        .remove::<MoveTarget>();
    {
        let mut farmer_entity = app.world_mut().entity_mut(farmer);
        let mut routine = farmer_entity.get_mut::<FarmerRoutine>().unwrap();
        routine.phase = FarmerPhase::ReturningToFarmstead;
        routine.failed_workplace_routes = MAX_WORKPLACE_ROUTE_FAILURES;
    }
    app.world_mut()
        .entity_mut(farmer)
        .get_mut::<PlayerPosition>()
        .unwrap()
        .0 = field_position;
    {
        let mut time = app.world_mut().get_mut::<WorldTime>(clock).unwrap();
        time.seconds_in_cycle = time.day_duration * 0.8;
    }
    app.update();
    assert_eq!(
        app.world()
            .entity(farmer)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Wheat),
        1,
        "even exhausted route recovery must leave a last basket on its worker while the Farmstead is full"
    );
    assert!(app.world().entity(farmer).get::<FarmerRoutine>().is_some());
    assert!(app.world().entity(farmer).get::<WorkerOffDuty>().is_none());

    assert_eq!(
        app.world_mut()
            .entity_mut(farm)
            .get_mut::<GoodsInventory>()
            .unwrap()
            .remove(Good::Wheat, 1),
        1
    );
    app.update();
    assert_eq!(
        app.world()
            .entity(farmer)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Wheat),
        0
    );
    assert_eq!(
        app.world()
            .entity(farm)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Wheat),
        farm_wheat_before_backpressure,
        "the space freed by a porter must receive the final basket exactly once, without another pathfinding loop"
    );
    assert!(app.world().entity(farmer).get::<FarmerRoutine>().is_none());
    assert!(app.world().entity(farmer).get::<WorkerOffDuty>().is_some());
    assert!(app
        .world()
        .entity(farmer)
        .get::<FarmerHarvestProgress>()
        .is_some());
}

#[test]
fn a_fisher_fills_a_two_food_batch_at_the_pier_then_deposits_at_the_hut() {
    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.init_resource::<SettlementEconomyRuntime>();
    app.insert_resource(WorldTerrain::default());
    app.add_systems(Update, run_fishing_routines);

    let hall = app
        .world_mut()
        .spawn(Settlement {
            name: "Nethercove".to_string(),
            tier: shared::components::SettlementTier::Hamlet,
            residents: 1,
            treasury: 0,
        })
        .id();
    let clock = app
        .world_mut()
        .spawn((
            WorldTime::new_default(),
            shared::components::TimeWarp::clamped(1.0),
        ))
        .id();
    let hut_position = Vec3::new(20.0, 2.0, 0.0);
    let hut = app
        .world_mut()
        .spawn((
            SettlementBuilding {
                kind: SettlementBuildingKind::FishermansHut,
                settlement: "Nethercove".to_string(),
                owner: Some("Ada".to_string()),
                quality: 0.67,
                workers: vec!["Ada".to_string()],
            },
            PlayerPosition(hut_position),
            PlayerRotation(0.0),
            GoodsInventory::new(shared::economy::capacity::FISHERMANS_HUT),
        ))
        .id();
    let pier_position = Vec3::new(20.0, 0.0, 8.0);
    let pier = app
        .world_mut()
        .spawn((
            FishingPier {
                settlement: "Nethercove".to_string(),
                fishermans_hut: hut_position,
                quality: 0.67,
            },
            PlayerPosition(pier_position),
            PlayerRotation(0.0),
        ))
        .id();
    let (_, fish_spot) = fishing_deck_points(pier_position, 0.0);
    let fisher = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            CharacterName("Ada".to_string()),
            VillagerIntent::Resident { settlement: hall },
            PlayerPosition(fish_spot),
            PlayerRotation(0.0),
            CharacterActivity::Fishing,
            GoodsInventory::new(shared::economy::capacity::VILLAGER),
            FishingRoutine {
                hut,
                pier,
                hall,
                catch_seconds: fisher_seconds_per_food(0.67),
                failed_workplace_routes: 0,
                production_day: u32::MAX,
                produced_today: 0,
                phase: FishingPhase::Fishing,
            },
        ))
        .id();

    app.update();
    assert_eq!(
        app.world()
            .entity(fisher)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Food),
        0,
        "partial catch progress must keep the visible fishing animation active"
    );
    {
        let mut entity = app.world_mut().entity_mut(fisher);
        let mut routine = entity.get_mut::<FishingRoutine>().unwrap();
        routine.catch_seconds = fisher_seconds_per_food(0.67) * 2.0;
    }
    app.update();
    assert_eq!(
        app.world()
            .entity(fisher)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Food),
        2,
        "the fisher should stay at the pier until the carrying batch is full"
    );

    let entrance = SettlementBuildingKind::FishermansHut.entrance_position(hut_position, 0.0);
    app.world_mut()
        .entity_mut(fisher)
        .remove::<MoveTarget>()
        .remove::<TravelRoute>()
        .remove::<PierTraversal>()
        .get_mut::<FishingRoutine>()
        .unwrap()
        .phase = FishingPhase::ReturningToHut;
    app.world_mut()
        .entity_mut(fisher)
        .get_mut::<PlayerPosition>()
        .unwrap()
        .0 = entrance;
    app.update();
    assert_eq!(
        app.world()
            .entity(hut)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Food),
        2
    );
    assert_eq!(
        app.world()
            .entity(fisher)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Food),
        0
    );

    app.world_mut()
        .entity_mut(fisher)
        .remove::<WorkplaceDoorTransit>()
        .remove::<BuildingDoorUse>()
        .remove::<MoveTarget>()
        .get_mut::<FishingRoutine>()
        .unwrap()
        .phase = FishingPhase::ReturningToHut;
    assert_eq!(
        app.world_mut()
            .entity_mut(fisher)
            .get_mut::<GoodsInventory>()
            .unwrap()
            .add(Good::Food, 1),
        1
    );
    app.world_mut()
        .entity_mut(fisher)
        .get_mut::<PlayerPosition>()
        .unwrap()
        .0 = entrance;
    {
        let mut time = app.world_mut().get_mut::<WorldTime>(clock).unwrap();
        time.seconds_in_cycle = time.day_duration * 0.8;
    }
    app.update();
    assert_eq!(
        app.world()
            .entity(hut)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Food),
        3,
        "the fisher's partial last catch must reach the hut before clocking off"
    );
    assert_eq!(
        app.world()
            .entity(fisher)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Food),
        0
    );
    assert!(app.world().entity(fisher).get::<FishingRoutine>().is_none());
    assert!(app.world().entity(fisher).get::<WorkerOffDuty>().is_some());
}

#[test]
fn completed_tree_interactions_keep_producing_without_a_daily_cap() {
    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.insert_resource(WorldTerrain::default());
    app.add_systems(Update, run_lumberjack_routines);

    let hall = app
        .world_mut()
        .spawn(Settlement {
            name: "Pinewatch".to_string(),
            tier: shared::components::SettlementTier::Hamlet,
            residents: 1,
            treasury: 0,
        })
        .id();
    let clock = app
        .world_mut()
        .spawn((
            WorldTime::new_default(),
            shared::components::TimeWarp::clamped(1.0),
        ))
        .id();
    let hut_position = Vec3::new(20.0, 2.0, 0.0);
    let hut = app
        .world_mut()
        .spawn((
            SettlementBuilding {
                kind: SettlementBuildingKind::LumberjackHut,
                settlement: "Pinewatch".to_string(),
                owner: Some("Ada".to_string()),
                quality: 0.9,
                workers: vec!["Ada".to_string()],
            },
            PlayerPosition(hut_position),
            PlayerRotation(0.0),
            GoodsInventory::new(shared::economy::capacity::LUMBERJACK_HUT),
        ))
        .id();
    let woodcutter = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            CharacterName("Ada".to_string()),
            VillagerIntent::Resident { settlement: hall },
            PlayerPosition(Vec3::new(30.0, 2.0, 0.0)),
            PlayerRotation(0.0),
            CharacterActivity::Chopping,
            GoodsInventory::new(shared::economy::capacity::VILLAGER),
            LumberjackRoutine {
                hut,
                hall,
                cycle: 0,
                failed_tree_routes: 0,
                failed_hut_routes: 0,
                chop_seconds: lumber_seconds_per_tree(0.9),
                production_day: u32::MAX,
                produced_today: 0,
                phase: LumberjackPhase::Chopping,
            },
        ))
        .id();

    app.update();
    assert_eq!(
        app.world()
            .entity(woodcutter)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Wood),
        3,
        "Wood is created only when the chopping interaction completes"
    );
    let entrance = SettlementBuildingKind::LumberjackHut.entrance_position(hut_position, 0.0);
    app.world_mut()
        .entity_mut(woodcutter)
        .insert(NavigationRouteFailed { goal: entrance });
    app.update();
    assert!(
        app.world()
            .entity(woodcutter)
            .get::<NavigationRouteFailed>()
            .is_none(),
        "a failed loaded return route must be retried instead of disabling the woodcutter"
    );
    assert_eq!(
        app.world()
            .entity(woodcutter)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Wood),
        3,
        "route recovery must retain physically carried Wood"
    );
    app.world_mut()
        .entity_mut(woodcutter)
        .get_mut::<PlayerPosition>()
        .unwrap()
        .0 = entrance;
    app.update();
    assert_eq!(
        app.world()
            .entity(hut)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Wood),
        3
    );

    app.world_mut()
        .entity_mut(woodcutter)
        .remove::<WorkplaceDoorTransit>()
        .remove::<BuildingDoorUse>()
        .remove::<MoveTarget>();
    {
        let mut entity = app.world_mut().entity_mut(woodcutter);
        let mut routine = entity.get_mut::<LumberjackRoutine>().unwrap();
        routine.chop_seconds = lumber_seconds_per_tree(0.9);
        routine.phase = LumberjackPhase::Chopping;
    }
    app.update();
    let total_wood = app
        .world()
        .entity(hut)
        .get::<GoodsInventory>()
        .unwrap()
        .amount(Good::Wood)
        + app
            .world()
            .entity(woodcutter)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Wood);
    assert_eq!(total_wood, 6);
    assert!(
        total_wood > 4,
        "a legacy four-Wood daily ceiling must not stop a valid second tree interaction"
    );

    app.world_mut()
        .entity_mut(woodcutter)
        .remove::<MoveTarget>()
        .remove::<TravelRoute>()
        .get_mut::<LumberjackRoutine>()
        .unwrap()
        .phase = LumberjackPhase::ReturningToHut;
    app.world_mut()
        .entity_mut(woodcutter)
        .get_mut::<PlayerPosition>()
        .unwrap()
        .0 = entrance;
    {
        let mut time = app.world_mut().get_mut::<WorldTime>(clock).unwrap();
        time.seconds_in_cycle = time.day_duration * 0.8;
    }
    app.update();
    assert_eq!(
        app.world()
            .entity(hut)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Wood),
        6,
        "the last felled tree must be unloaded before the woodcutter clocks off"
    );
    assert_eq!(
        app.world()
            .entity(woodcutter)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Wood),
        0
    );
    assert!(app
        .world()
        .entity(woodcutter)
        .get::<LumberjackRoutine>()
        .is_none());
    assert!(app
        .world()
        .entity(woodcutter)
        .get::<WorkerOffDuty>()
        .is_some());
}
