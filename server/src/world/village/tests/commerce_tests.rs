//! Village commerce regression fixtures and invariants.

use super::*;

#[test]
fn porter_cart_presentation_follows_real_freight_and_real_load_bulk() {
    let mut app = App::new();
    app.add_systems(Update, sync_porter_cart_state);
    let hall = app.world_mut().spawn_empty().id();
    let business = app.world_mut().spawn_empty().id();
    let porter = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            GoodsInventory::new(shared::economy::capacity::PORTER),
            PorterCargoCapacity,
            MarketCollectionRoutine {
                business,
                seller: shared::components::BuildingId(1),
                hall,
                counter: Vec3::ZERO,
                good: Good::Wood,
                reserved_units: 24,
                unit_price: Good::Wood.base_price(),
                phase: MarketCollectionPhase::GoingToBusiness,
                fallback_counter_attempted: false,
            },
        ))
        .id();

    app.update();
    assert_eq!(
        app.world()
            .get::<shared::economy::PorterCartState>(porter)
            .unwrap()
            .load_slots,
        0,
        "the empty outbound leg still owns the cart"
    );

    app.world_mut()
        .get_mut::<GoodsInventory>(porter)
        .unwrap()
        .add(Good::Wood, 24);
    app.update();
    assert_eq!(
        app.world()
            .get::<shared::economy::PorterCartState>(porter)
            .unwrap()
            .load_slots,
        2
    );

    app.world_mut()
        .entity_mut(porter)
        .remove::<MarketCollectionRoutine>();
    app.world_mut()
        .get_mut::<GoodsInventory>(porter)
        .unwrap()
        .remove(Good::Wood, u32::MAX);
    app.update();
    assert!(
        app.world()
            .get::<shared::economy::PorterCartState>(porter)
            .is_none(),
        "an idle empty steward must be free to do non-porter work without a cart"
    );
}

#[test]
fn market_sale_receipt_uses_person_id_when_names_repeat() {
    let mut app = village_test_app();
    app.init_resource::<BusinessEventQueue>();
    app.add_systems(Update, apply_business_events);
    app.world_mut().spawn((
        shared::components::SettlementId(1),
        Settlement {
            name: "ID Market".into(),
            tier: shared::components::SettlementTier::Hamlet,
            residents: 2,
            treasury: 0,
        },
    ));
    let first = app
        .world_mut()
        .spawn((
            CharacterName("Robin".into()),
            shared::components::PersonId(10),
            Wallet::new(0),
        ))
        .id();
    let second = app
        .world_mut()
        .spawn((
            CharacterName("Robin".into()),
            shared::components::PersonId(11),
            Wallet::new(0),
        ))
        .id();
    app.world_mut()
        .resource_mut::<BusinessEventQueue>()
        .record_market_purchase(
            1,
            shared::components::SettlementId(1),
            [shared::economy::MarketFill {
                seller: shared::economy::MarketSeller::Person(shared::components::PersonId(11)),
                good: Good::Wheat,
                units: 1,
                unit_price: 77,
                gross: 77,
                market_fee: 0,
            }],
        );

    app.update();

    assert_eq!(app.world().get::<Wallet>(first).unwrap().balance(), 0);
    assert_eq!(app.world().get::<Wallet>(second).unwrap().balance(), 77);
}

#[test]
fn porter_roles_apply_cart_capacity_and_restore_personal_capacity_afterward() {
    let mut app = village_test_app();
    app.add_systems(Update, sync_porter_cargo_capacity);
    let settlement = app.world_mut().spawn_empty().id();
    let porter = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            GoodsInventory::new(shared::economy::capacity::VILLAGER),
            MootSteward { settlement },
        ))
        .id();

    app.update();
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(porter)
            .unwrap()
            .bulk_capacity(),
        shared::economy::capacity::PORTER
    );

    app.world_mut().entity_mut(porter).remove::<MootSteward>();
    app.world_mut().entity_mut(porter).insert(CompanyPorter {
        settlement,
        settlement_id: shared::components::SettlementId(1),
        company: shared::components::CompanyId(2),
        storage_hall: shared::components::BuildingId(3),
    });
    app.update();
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(porter)
            .unwrap()
            .bulk_capacity(),
        shared::economy::capacity::PORTER
    );

    app.world_mut().entity_mut(porter).remove::<CompanyPorter>();
    app.update();
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(porter)
            .unwrap()
            .bulk_capacity(),
        shared::economy::capacity::VILLAGER
    );
    assert!(!app.world().entity(porter).contains::<PorterCargoCapacity>());
}

#[test]
fn marketplace_expands_one_shared_store_and_migrates_legacy_stock() {
    let mut app = village_test_app();
    app.add_systems(Update, sync_public_market_storage);
    let settlement_id = shared::components::SettlementId(6_850);
    let hall = app
        .world_mut()
        .spawn((
            settlement_id,
            Settlement {
                name: "Sharedstock".into(),
                tier: shared::components::SettlementTier::Village,
                residents: 16,
                treasury: 0,
            },
            GoodsInventory::new(shared::economy::capacity::HALL),
        ))
        .id();
    let mut legacy_market_stock = GoodsInventory::new(shared::economy::capacity::MARKET);
    assert_eq!(legacy_market_stock.add(Good::Wheat, 80), 80);
    let marketplace = app
        .world_mut()
        .spawn((
            shared::components::BuildingOf(settlement_id),
            SettlementBuilding {
                kind: SettlementBuildingKind::Market,
                settlement: "Sharedstock".into(),
                owner: None,
                quality: 1.0,
                workers: Vec::new(),
            },
            legacy_market_stock,
        ))
        .id();

    app.update();

    let hall_store = app.world().get::<GoodsInventory>(hall).unwrap();
    assert_eq!(hall_store.amount(Good::Wheat), 80);
    assert_eq!(
        hall_store.partition_bulk_capacity(),
        Some(shared::economy::capacity::HALL + shared::economy::capacity::MARKET)
    );
    assert_eq!(
        hall_store.bulk_capacity(),
        (shared::economy::capacity::HALL + shared::economy::capacity::MARKET)
            * u32::try_from(Good::COUNT).unwrap()
    );
    let market_store = app.world().get::<GoodsInventory>(marketplace).unwrap();
    assert!(market_store.is_empty());
    assert_eq!(market_store.bulk_capacity(), 0);

    app.update();
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(hall)
            .unwrap()
            .amount(Good::Wheat),
        80,
        "repeated synchronization must never duplicate migrated goods"
    );
}

#[test]
fn a_moot_steward_collects_a_bounded_load_while_the_woodcutter_keeps_working() {
    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.insert_resource(WorldTerrain::default());
    app.add_systems(
        Update,
        (
            sync_porter_cargo_capacity,
            run_market_collections,
            sync_carried_load,
        )
            .chain(),
    );

    let hall_position = Vec3::new(0.0, 5.0, 0.0);
    let settlement_id = shared::components::SettlementId(6_900);
    let mut market = MootMarket::founding();
    market.set_targets(1, 14);
    let hall = app
        .world_mut()
        .spawn((
            settlement_id,
            Settlement {
                name: "Yewcrag".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            },
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
            GoodsInventory::new_partitioned(shared::economy::capacity::HALL),
            market,
        ))
        .id();

    let hut_position = Vec3::new(25.0, 5.0, 0.0);
    let hut_id = shared::components::BuildingId(6_901);
    let company_id = shared::components::CompanyId(6_902);
    let company = spawn_test_company(&mut app, company_id.0, 0);
    let mut hut_store = GoodsInventory::new(shared::economy::capacity::LUMBERJACK_HUT);
    assert_eq!(hut_store.add(Good::Wood, 45), 45, "45 bundles is 75% full");
    let hut = app
        .world_mut()
        .spawn((
            hut_id,
            shared::components::BuildingOf(settlement_id),
            shared::components::OperatedBy(company_id),
            SettlementBuilding {
                kind: SettlementBuildingKind::LumberjackHut,
                settlement: "Yewcrag".to_string(),
                owner: Some("Ada".to_string()),
                quality: 0.5,
                workers: vec!["Ada".to_string()],
            },
            PlayerPosition(hut_position),
            PlayerRotation(0.0),
            hut_store,
            BusinessSalePolicy::default(),
            BusinessAccount::default(),
            BusinessWagePolicy::default(),
            BusinessProcurementPolicy::default(),
        ))
        .id();
    let marketplace_position = Vec3::new(20.0, 5.0, 0.0);
    app.world_mut().spawn((
        shared::components::BuildingOf(settlement_id),
        SettlementBuilding {
            kind: SettlementBuildingKind::Market,
            settlement: "Yewcrag".to_string(),
            owner: None,
            quality: 1.0,
            workers: Vec::new(),
        },
        PlayerPosition(marketplace_position),
        PlayerRotation(0.0),
    ));
    let porter = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            MootSteward { settlement: hall },
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
            CharacterActivity::Indoors,
            GoodsInventory::new(shared::economy::capacity::VILLAGER),
            CarriedLoad::default(),
        ))
        .id();

    app.update();
    assert!(matches!(
        app.world()
            .entity(porter)
            .get::<MarketCollectionRoutine>()
            .unwrap()
            .phase,
        MarketCollectionPhase::GoingToBusiness
    ));
    let hut_entrance = SettlementBuildingKind::LumberjackHut.entrance_position(hut_position, 0.0);
    app.world_mut()
        .entity_mut(porter)
        .get_mut::<PlayerPosition>()
        .unwrap()
        .0 = hut_entrance;
    app.update();
    let world = app.world();
    assert_eq!(
        world
            .entity(porter)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Wood),
        24
    );
    assert_eq!(
        world
            .entity(hut)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Wood),
        21
    );
    assert_eq!(
        world.entity(porter).get::<CarriedLoad>().unwrap().amount,
        24
    );
    assert!(matches!(
        world
            .entity(porter)
            .get::<MarketCollectionRoutine>()
            .unwrap()
            .phase,
        MarketCollectionPhase::ReturningToHall
    ));

    let market_entrance =
        SettlementBuildingKind::Market.entrance_position(marketplace_position, 0.0);
    assert_eq!(
        world
            .entity(porter)
            .get::<MarketCollectionRoutine>()
            .unwrap()
            .counter,
        market_entrance,
        "the return trip should use the nearer Marketplace counter"
    );
    app.world_mut()
        .entity_mut(porter)
        .insert(NavigationRouteFailed {
            goal: market_entrance,
        });
    app.update();
    assert!(
        app.world()
            .entity(porter)
            .get::<NavigationRouteFailed>()
            .is_none(),
        "one failed return route must not permanently disable the settlement's only porter"
    );
    assert!(matches!(
        app.world()
            .entity(porter)
            .get::<MarketCollectionRoutine>()
            .unwrap()
            .phase,
        MarketCollectionPhase::ReturningToHall
    ));
    let hall_entrance = SettlementBuildingKind::Hall.entrance_position(hall_position, 0.0);
    assert_eq!(
        app.world()
            .entity(porter)
            .get::<MarketCollectionRoutine>()
            .unwrap()
            .counter,
        hall_entrance,
        "a failed Marketplace doorway should reroute the loaded porter through the Hall counter"
    );
    assert_eq!(
        app.world()
            .entity(porter)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Wood),
        24,
        "a retry must retain the physical load and reserved market cash"
    );
    app.world_mut()
        .entity_mut(porter)
        .insert(NavigationRouteFailed {
            goal: hall_entrance,
        });
    app.update();
    assert_eq!(
        app.world()
            .entity(porter)
            .get::<MarketCollectionRoutine>()
            .unwrap()
            .phase,
        MarketCollectionPhase::ReturningToBusinessAfterFailedSale,
        "two unreachable public counters must reverse the physical collection instead of ping-ponging forever"
    );
    assert_eq!(
        app.world().entity(porter).get::<MoveTarget>().unwrap().0,
        hut_entrance,
    );
    app.world_mut()
        .entity_mut(porter)
        .get_mut::<PlayerPosition>()
        .unwrap()
        .0 = hut_entrance;
    app.update();
    assert_eq!(
        app.world()
            .entity(hut)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Wood),
        45,
        "the seller must physically recover an undeliverable consignment"
    );
    assert!(app
        .world()
        .entity(porter)
        .get::<MarketCollectionRoutine>()
        .is_none());

    // A later collection remains possible after geometry changes. Let the
    // same steward collect again and complete it through the Marketplace.
    app.update();
    app.update();
    assert_eq!(
        app.world()
            .entity(porter)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Wood),
        24,
    );
    app.world_mut()
        .entity_mut(porter)
        .get_mut::<PlayerPosition>()
        .unwrap()
        .0 = market_entrance;
    app.update();
    let world = app.world();
    assert_eq!(
        world
            .entity(porter)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Wood),
        0
    );
    assert_eq!(
        world
            .entity(hall)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Wood),
        24
    );
    assert!(world
        .entity(porter)
        .get::<MarketCollectionRoutine>()
        .is_none());
    let seller = hut_id;
    assert_eq!(
        world
            .entity(hall)
            .get::<MootMarket>()
            .unwrap()
            .seller_listed_units(shared::economy::MarketSeller::Business(seller), Good::Wood),
        24
    );
    assert_eq!(
        world.get::<CompanyAccount>(company).unwrap().cash,
        0,
        "the porter consigned goods but no customer has bought them"
    );

    // A processor-input purchase is already paid for before the porter walks
    // the final leg. If that doorway is unreachable, the buyer-owned cargo is
    // returned and relisted instead of being trapped on the porter forever.
    app.world_mut()
        .entity_mut(porter)
        .get_mut::<GoodsInventory>()
        .unwrap()
        .add(Good::Wood, 1);
    app.world_mut().entity_mut(porter).insert((
        MarketCollectionRoutine {
            business: hut,
            seller,
            hall,
            counter: market_entrance,
            good: Good::Wood,
            reserved_units: 1,
            unit_price: 0,
            phase: MarketCollectionPhase::DeliveringInput,
            fallback_counter_attempted: false,
        },
        NavigationRouteFailed { goal: hut_entrance },
        MoveTarget(hut_entrance),
    ));
    app.update();
    assert_eq!(
        app.world()
            .entity(porter)
            .get::<MarketCollectionRoutine>()
            .unwrap()
            .phase,
        MarketCollectionPhase::ReturningFailedInput,
    );
    app.update();
    assert!(app
        .world()
        .entity(porter)
        .get::<MarketCollectionRoutine>()
        .is_none());
    assert_eq!(
        app.world()
            .entity(hall)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Wood),
        25,
    );
    assert_eq!(
        app.world()
            .entity(hall)
            .get::<MootMarket>()
            .unwrap()
            .seller_listed_units(shared::economy::MarketSeller::Business(seller), Good::Wood),
        25,
    );

    // A porter improves throughput but is not a hard dependency. With the
    // specialist role removed, the woodcutter interrupts work, takes one
    // personal-capacity load, and uses the same ownership-preserving market
    // collection routine.
    app.world_mut().entity_mut(porter).remove::<MootSteward>();
    let mut returning_job_load = GoodsInventory::new(shared::economy::capacity::VILLAGER);
    assert_eq!(returning_job_load.add(Good::Wood, 2), 2);
    let worker = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            PlayerPosition(hut_entrance),
            CharacterActivity::Indoors,
            returning_job_load,
            LumberjackRoutine {
                hut,
                hall,
                cycle: 0,
                failed_tree_routes: 0,
                failed_hut_routes: 0,
                chop_seconds: 0.0,
                production_day: 0,
                produced_today: 0,
                phase: LumberjackPhase::Inside { seconds_left: 1.0 },
            },
        ))
        .id();

    app.update();
    assert!(app.world().entity(worker).contains::<LumberjackRoutine>());
    assert!(!app
        .world()
        .entity(worker)
        .contains::<MarketCollectionRoutine>());
    assert_eq!(
        app.world()
            .entity(worker)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Wood),
        2,
        "ordinary job cargo must not be mistaken for orphaned porter freight"
    );
    assert_eq!(
        app.world_mut()
            .entity_mut(worker)
            .get_mut::<GoodsInventory>()
            .unwrap()
            .remove(Good::Wood, 2),
        2,
    );

    app.update();
    assert!(app
        .world()
        .entity(worker)
        .contains::<MarketCollectionRoutine>());
    assert!(!app.world().entity(worker).contains::<LumberjackRoutine>());
    app.update();
    assert!(
        app.world()
            .entity(worker)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Wood)
            > 0,
        "the employee should carry one bounded load while no porter exists"
    );
}

#[test]
fn owned_farm_supplies_owned_windmill_without_touching_the_public_market() {
    run_owned_farm_supply_test(false);
}

#[test]
fn company_porter_moves_owned_inputs_without_a_municipal_delivery_fee() {
    run_owned_farm_supply_test(true);
}

fn run_owned_farm_supply_test(private_porter: bool) {
    let mut app = village_test_app();
    app.add_systems(Update, run_internal_deliveries);
    app.world_mut().spawn(WorldTime::new_default());

    let settlement_id = shared::components::SettlementId(7_001);
    let company = shared::components::CompanyId(7_002);
    let company_entity = spawn_test_company(&mut app, company.0, PENNIES_PER_COIN);
    let hall_position = Vec3::ZERO;
    let hall = app
        .world_mut()
        .spawn((
            settlement_id,
            Settlement {
                name: "Chainford".into(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 3,
                treasury: 0,
            },
            shared::economy::CivicAccount::default(),
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
            GoodsInventory::new(shared::economy::capacity::HALL),
            MootMarket::founding(),
        ))
        .id();

    let farm_position = Vec3::new(12.0, 0.0, 0.0);
    let mut farm_stock = GoodsInventory::new(shared::economy::capacity::FARMSTEAD);
    assert_eq!(farm_stock.add(Good::Wheat, 6), 6);
    let farm = app
        .world_mut()
        .spawn((
            shared::components::BuildingId(7_003),
            shared::components::BuildingOf(settlement_id),
            shared::components::OperatedBy(company),
            SettlementBuilding {
                kind: SettlementBuildingKind::Farmstead,
                settlement: "Chainford".into(),
                owner: Some("Alda".into()),
                quality: 1.0,
                workers: vec!["Alda".into()],
            },
            PlayerPosition(farm_position),
            PlayerRotation(0.0),
            farm_stock,
            BusinessSalePolicy {
                company_reserve_days: 7,
                company_reserve_units: 100,
                ..BusinessSalePolicy::for_good(Good::Wheat)
            },
            BusinessProcurementPolicy::none(),
            BusinessSupplyPolicy::none(),
            BusinessManagementPolicy::default(),
            BusinessAccount {
                estimated_unit_cost: Good::Wheat.base_price(),
                ..BusinessAccount::default()
            },
            BusinessCondition {
                state: BusinessState::Operating,
                ..default()
            },
        ))
        .id();

    let mut procurement = BusinessProcurementPolicy::none();
    procurement.set_rule(
        Good::Wheat,
        BusinessInputRule {
            enabled: true,
            coverage_days: 2,
            reorder_below: 2,
            target_units: 4,
            maximum_unit_price: 2 * PENNIES_PER_COIN,
        },
    );
    let supply = BusinessSupplyPolicy::none().with_rule(
        Good::Wheat,
        BusinessPrivateInputRule {
            enabled: true,
            sourcing: BusinessSourcingMode::OwnedOnly,
            preferred_supplier: Some(shared::components::BuildingId(7_003)),
        },
    );
    let mill_position = Vec3::new(24.0, 0.0, 0.0);
    let mill = app
        .world_mut()
        .spawn((
            shared::components::BuildingId(7_004),
            shared::components::BuildingOf(settlement_id),
            shared::components::OperatedBy(company),
            SettlementBuilding {
                kind: SettlementBuildingKind::Windmill,
                settlement: "Chainford".into(),
                owner: Some("Alda".into()),
                quality: 1.0,
                workers: vec!["Bera".into()],
            },
            PlayerPosition(mill_position),
            PlayerRotation(0.0),
            GoodsInventory::new(shared::economy::capacity::WINDMILL),
            BusinessSalePolicy::for_good(Good::Flour),
            procurement,
            supply,
            BusinessManagementPolicy::default(),
            BusinessAccount::default(),
            BusinessCondition {
                state: BusinessState::Operating,
                ..default()
            },
        ))
        .id();
    let mut porter_commands = app.world_mut().spawn((
        CharacterKind::Villager,
        PlayerPosition(SettlementBuildingKind::Hall.entrance_position(hall_position, 0.0)),
        CharacterActivity::Indoors,
        GoodsInventory::new(shared::economy::capacity::VILLAGER),
    ));
    if private_porter {
        porter_commands.insert(CompanyPorter {
            settlement: hall,
            settlement_id,
            company,
            storage_hall: shared::components::BuildingId(7_005),
        });
    } else {
        porter_commands.insert(MootSteward { settlement: hall });
    }
    let porter = porter_commands.id();

    app.update();
    let routine = app
        .world()
        .get::<InternalDeliveryRoutine>(porter)
        .expect("an eligible porter should claim the owned input request");
    assert_eq!(routine.phase, InternalDeliveryPhase::GoingToSupplier);
    assert_eq!(routine.municipal_fee, !private_porter);
    app.world_mut().get_mut::<PlayerPosition>(porter).unwrap().0 =
        SettlementBuildingKind::Farmstead.entrance_position(farm_position, 0.0);
    app.update();
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(porter)
            .unwrap()
            .amount(Good::Wheat),
        4
    );
    app.world_mut().get_mut::<PlayerPosition>(porter).unwrap().0 =
        SettlementBuildingKind::Windmill.entrance_position(mill_position, 0.0);
    app.update();

    assert_eq!(
        app.world()
            .get::<GoodsInventory>(farm)
            .unwrap()
            .amount(Good::Wheat),
        2,
        "an active company input request has first claim even when every supplier unit is reserved from public sale"
    );
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(mill)
            .unwrap()
            .amount(Good::Wheat),
        4
    );
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(hall)
            .unwrap()
            .amount(Good::Wheat),
        0
    );
    assert_eq!(
        app.world()
            .get::<MootMarket>(hall)
            .unwrap()
            .listed_units(Good::Wheat),
        0
    );
    let expected_fee = if private_porter {
        0
    } else {
        4 * u64::from(Good::Wheat.bulk_per_unit())
    };
    assert_eq!(
        app.world().get::<Settlement>(hall).unwrap().treasury,
        expected_fee
    );
    assert_eq!(
        app.world()
            .get::<CompanyAccount>(company_entity)
            .unwrap()
            .cash,
        PENNIES_PER_COIN - expected_fee
    );
    let farm_ledger = app
        .world()
        .get::<BusinessAccount>(farm)
        .unwrap()
        .current_day;
    let mill_ledger = app
        .world()
        .get::<BusinessAccount>(mill)
        .unwrap()
        .current_day;
    assert!(farm_ledger.internal_revenue > 0);
    assert_eq!(
        farm_ledger.internal_revenue,
        mill_ledger.internal_input_expense
    );
    assert_eq!(mill_ledger.delivery_fees, expected_fee);
    assert!(app.world().get::<InternalDeliveryRoutine>(porter).is_none());
}

#[test]
fn a_liquidating_business_consigns_inputs_instead_of_trapping_food() {
    let mut app = village_test_app();
    app.add_systems(Update, run_market_collections);
    let settlement_id = shared::components::SettlementId(8_800);
    let hall_position = Vec3::ZERO;
    let mut market = MootMarket::founding();
    market.set_targets(2, 0);
    let hall = app
        .world_mut()
        .spawn((
            settlement_id,
            Settlement {
                name: "Millfall".into(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 4,
                treasury: 0,
            },
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
            GoodsInventory::new(shared::economy::capacity::HALL),
            market,
        ))
        .id();
    let business_id = shared::components::BuildingId(8_801);
    let company_id = shared::components::CompanyId(8_802);
    spawn_test_company(&mut app, company_id.0, 0);
    let business_position = Vec3::new(18.0, 0.0, 0.0);
    let mut store = GoodsInventory::new(shared::economy::capacity::BAKERY);
    store.add(Good::Flour, 6);
    let business = app
        .world_mut()
        .spawn((
            business_id,
            shared::components::BuildingOf(settlement_id),
            shared::components::OperatedBy(company_id),
            SettlementBuilding {
                kind: SettlementBuildingKind::Bakery,
                settlement: "Millfall".into(),
                owner: None,
                quality: 1.0,
                workers: Vec::new(),
            },
            PlayerPosition(business_position),
            PlayerRotation(0.0),
            store,
            BusinessSalePolicy {
                collection_enabled: true,
                company_reserve_units: 0,
                max_units_per_collection: 8,
                ..BusinessSalePolicy::for_good(Good::Bread)
            },
            BusinessCondition {
                state: BusinessState::Liquidating,
                liquidation_days: 2,
                ..default()
            },
            BusinessAccount::default(),
            BusinessWagePolicy::default(),
            BusinessProcurementPolicy::default(),
        ))
        .id();
    let porter = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            MootSteward { settlement: hall },
            PlayerPosition(hall_position),
            CharacterActivity::Indoors,
            GoodsInventory::new(shared::economy::capacity::VILLAGER),
        ))
        .id();

    app.update();
    let business_entrance =
        SettlementBuildingKind::Bakery.entrance_position(business_position, 0.0);
    app.world_mut().get_mut::<PlayerPosition>(porter).unwrap().0 = business_entrance;
    app.update();
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(porter)
            .unwrap()
            .amount(Good::Flour),
        6,
        "liquidation must collect input stock, not only the bakery's normal Bread output",
    );
    app.world_mut().get_mut::<PlayerPosition>(porter).unwrap().0 =
        SettlementBuildingKind::Hall.entrance_position(hall_position, 0.0);
    app.update();
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(business)
            .unwrap()
            .amount(Good::Flour),
        0
    );
    assert_eq!(
        app.world()
            .get::<MootMarket>(hall)
            .unwrap()
            .seller_listed_units(MarketSeller::Business(business_id), Good::Flour),
        6,
    );
}

#[test]
fn a_full_wood_compartment_does_not_block_bread_collection() {
    let mut app = village_test_app();
    app.add_systems(Update, run_market_collections);
    let settlement_id = shared::components::SettlementId(8_850);
    let hall_position = Vec3::ZERO;
    let mut hall_store = GoodsInventory::new_partitioned(shared::economy::capacity::HALL);
    assert_eq!(
        hall_store.add(
            Good::Wood,
            shared::economy::capacity::HALL / Good::Wood.bulk_per_unit()
        ),
        shared::economy::capacity::HALL / Good::Wood.bulk_per_unit()
    );
    assert_eq!(hall_store.free_units(Good::Wood), 0);
    assert!(hall_store.free_units(Good::Bread) > 0);
    let hall = app
        .world_mut()
        .spawn((
            settlement_id,
            Settlement {
                name: "Compartment Bay".into(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 4,
                treasury: 0,
            },
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
            hall_store,
            MootMarket::founding(),
        ))
        .id();
    let business_id = shared::components::BuildingId(8_851);
    let company_id = shared::components::CompanyId(8_852);
    spawn_test_company(&mut app, company_id.0, 0);
    let mut bakery_store = GoodsInventory::new(shared::economy::capacity::BAKERY);
    bakery_store.add(Good::Bread, 12);
    let bakery = app
        .world_mut()
        .spawn((
            business_id,
            shared::components::BuildingOf(settlement_id),
            shared::components::OperatedBy(company_id),
            SettlementBuilding {
                kind: SettlementBuildingKind::Bakery,
                settlement: "Compartment Bay".into(),
                owner: Some("Baker".into()),
                quality: 1.0,
                workers: Vec::new(),
            },
            PlayerPosition(Vec3::new(20.0, 0.0, 0.0)),
            PlayerRotation(0.0),
            bakery_store,
            BusinessSalePolicy::for_good(Good::Bread),
            BusinessAccount::default(),
            BusinessWagePolicy::default(),
            BusinessProcurementPolicy::default(),
        ))
        .id();
    let porter = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            MootSteward { settlement: hall },
            PlayerPosition(hall_position),
            CharacterActivity::Indoors,
            GoodsInventory::new(shared::economy::capacity::VILLAGER),
        ))
        .id();

    app.update();

    let routine = app
        .world()
        .get::<MarketCollectionRoutine>(porter)
        .expect("the free Bread bay should admit a collection while Wood is full");
    assert_eq!(routine.business, bakery);
    assert_eq!(routine.good, Good::Bread);
}

#[test]
fn two_moot_stewards_reserve_distinct_collection_work() {
    let mut app = village_test_app();
    app.add_systems(Update, run_market_collections);

    let hall_position = Vec3::ZERO;
    let settlement_id = shared::components::SettlementId(8_900);
    let hall = app
        .world_mut()
        .spawn((
            settlement_id,
            Settlement {
                name: "Twinporter".into(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 4,
                treasury: 0,
            },
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
            GoodsInventory::new_partitioned(shared::economy::capacity::HALL),
            MootMarket::founding(),
        ))
        .id();

    let mut businesses = Vec::new();
    for (index, x) in [20.0, -20.0].into_iter().enumerate() {
        let building_id = shared::components::BuildingId(8_901 + index as u64);
        let company_id = shared::components::CompanyId(8_911 + index as u64);
        spawn_test_company(&mut app, company_id.0, 0);
        let mut inventory = GoodsInventory::new(shared::economy::capacity::LUMBERJACK_HUT);
        inventory.add(Good::Wood, 30);
        businesses.push(
            app.world_mut()
                .spawn((
                    building_id,
                    shared::components::BuildingOf(settlement_id),
                    shared::components::OperatedBy(company_id),
                    SettlementBuilding {
                        kind: SettlementBuildingKind::LumberjackHut,
                        settlement: "Twinporter".into(),
                        owner: Some(format!("Owner{index}")),
                        quality: 0.8,
                        workers: Vec::new(),
                    },
                    PlayerPosition(Vec3::new(x, 0.0, 0.0)),
                    PlayerRotation(0.0),
                    inventory,
                    BusinessSalePolicy::default(),
                    BusinessAccount::default(),
                    BusinessWagePolicy::default(),
                    BusinessProcurementPolicy::default(),
                ))
                .id(),
        );
    }
    let porters: Vec<_> = (0..2)
        .map(|_| {
            app.world_mut()
                .spawn((
                    CharacterKind::Villager,
                    MootSteward { settlement: hall },
                    PlayerPosition(hall_position),
                    CharacterActivity::Indoors,
                    GoodsInventory::new(shared::economy::capacity::VILLAGER),
                ))
                .id()
        })
        .collect();

    app.update();

    let claimed: HashSet<_> = porters
        .iter()
        .map(|porter| {
            app.world()
                .get::<MarketCollectionRoutine>(*porter)
                .expect("both stewards should accept a collection")
                .business
        })
        .collect();
    assert_eq!(
        claimed.len(),
        2,
        "two stewards must not be dispatched to the same workplace at once"
    );
    assert!(businesses
        .into_iter()
        .all(|business| claimed.contains(&business)));
}

#[test]
fn a_backed_off_road_repair_still_takes_priority_over_the_porters_next_market_trip() {
    let mut app = village_test_app();
    app.add_systems(Update, run_market_collections);

    let hall_position = Vec3::ZERO;
    let hall = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Roadmarket".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 2,
                treasury: 0,
            },
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
            GoodsInventory::new(shared::economy::capacity::HALL),
            MootMarket::founding(),
        ))
        .id();
    let mut store = GoodsInventory::new(shared::economy::capacity::LUMBERJACK_HUT);
    store.add(Good::Wood, 12);
    app.world_mut().spawn((
        SettlementBuilding {
            kind: SettlementBuildingKind::LumberjackHut,
            settlement: "Roadmarket".to_string(),
            owner: Some("Owner".to_string()),
            quality: 0.7,
            workers: vec!["Owner".to_string()],
        },
        PlayerPosition(Vec3::X * 20.0),
        PlayerRotation(0.0),
        store,
        BusinessSalePolicy::default(),
        BusinessAccount::default(),
        BusinessWagePolicy::default(),
    ));
    let porter = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            MootSteward { settlement: hall },
            PlayerPosition(hall_position),
            CharacterActivity::Indoors,
            GoodsInventory::new(shared::economy::capacity::VILLAGER),
        ))
        .id();
    let roadless_building = app.world_mut().spawn_empty().id();
    app.world_mut().entity_mut(roadless_building).insert((
        RoadRequest {
            builder: porter,
            settlement: hall,
            completed_site: roadless_building,
            attempt: 1,
        },
        crate::world::village_roads::RoadSurveyBackoff::after_failure(None, 0.0),
    ));

    app.update();

    assert!(app.world().get::<MarketCollectionRoutine>(porter).is_none());
    assert_eq!(
        *app.world().get::<CharacterActivity>(porter).unwrap(),
        CharacterActivity::Idle
    );
}

#[test]
fn the_moot_steward_buys_inputs_for_any_business_policy() {
    let mut app = village_test_app();
    app.add_systems(
        Update,
        (run_market_collections, apply_business_events).chain(),
    );
    app.world_mut().spawn(WorldTime::new_default());

    let settlement_id = shared::components::SettlementId(800);
    let buyer_id = shared::components::BuildingId(801);
    let buyer_company_id = shared::components::CompanyId(802);
    let buyer_company = spawn_test_company(&mut app, buyer_company_id.0, 10 * PENNIES_PER_COIN);
    let hall_position = Vec3::ZERO;
    let mut hall_store = GoodsInventory::new(shared::economy::capacity::HALL);
    assert_eq!(hall_store.add(Good::Wheat, 5), 5);
    let mut market = MootMarket::founding();
    market.consign(
        shared::economy::MarketSeller::Treasury(settlement_id),
        Good::Wheat,
        5,
        Good::Wheat.base_price(),
    );
    let hall = app
        .world_mut()
        .spawn((
            settlement_id,
            Settlement {
                name: "Inputford".into(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            },
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
            hall_store,
            market,
        ))
        .id();

    let mut procurement = BusinessProcurementPolicy::default();
    procurement.set_rule(
        Good::Wheat,
        shared::economy::BusinessInputRule {
            enabled: true,
            coverage_days: 2,
            reorder_below: 1,
            target_units: 3,
            maximum_unit_price: Good::Wheat.base_price(),
        },
    );
    let business_position = Vec3::new(12.0, 0.0, 0.0);
    let buyer = app
        .world_mut()
        .spawn((
            buyer_id,
            shared::components::BuildingOf(settlement_id),
            shared::components::OperatedBy(buyer_company_id),
            SettlementBuilding {
                kind: SettlementBuildingKind::Farmstead,
                settlement: "Inputford".into(),
                owner: None,
                quality: 1.0,
                workers: Vec::new(),
            },
            PlayerPosition(business_position),
            PlayerRotation(0.0),
            GoodsInventory::new(shared::economy::capacity::FARMSTEAD),
            BusinessSalePolicy::for_good(Good::Wheat),
            BusinessAccount::default(),
            BusinessWagePolicy::default(),
            procurement,
            BusinessCondition::default(),
        ))
        .id();
    let porter = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            MootSteward { settlement: hall },
            PlayerPosition(SettlementBuildingKind::Hall.entrance_position(hall_position, 0.0)),
            CharacterActivity::Indoors,
            GoodsInventory::new(shared::economy::capacity::VILLAGER),
        ))
        .id();

    app.update();
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(porter)
            .unwrap()
            .amount(Good::Wheat),
        3
    );
    let account = app.world().get::<BusinessAccount>(buyer).unwrap();
    assert_eq!(
        app.world()
            .get::<CompanyAccount>(buyer_company)
            .unwrap()
            .cash,
        10 * PENNIES_PER_COIN - 3 * Good::Wheat.base_price()
    );
    assert_eq!(
        account.current_day.input_expense,
        3 * Good::Wheat.base_price()
    );

    app.world_mut().get_mut::<PlayerPosition>(porter).unwrap().0 =
        SettlementBuildingKind::Farmstead.entrance_position(business_position, 0.0);
    app.update();
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(buyer)
            .unwrap()
            .amount(Good::Wheat),
        3
    );
    assert!(app.world().get::<MarketCollectionRoutine>(porter).is_none());
    assert_eq!(
        app.world().get::<Settlement>(hall).unwrap().treasury,
        3 * Good::Wheat.base_price(),
        "Treasury-owned migration stock receives the purchase through the same seller path"
    );
}
