//! Village civic regression fixtures and invariants.

use super::*;

#[test]
fn a_daily_market_ration_moves_food_and_exactly_conserves_coin() {
    let mut app = village_test_app();
    app.init_resource::<SettlementEconomyRuntime>();
    app.add_systems(
        Update,
        (
            ensure_settlement_economies,
            update_moot_market_targets,
            update_settlement_economies,
            apply_business_events,
        )
            .chain(),
    );
    let clock = app.world_mut().spawn(WorldTime::new_default()).id();
    let mut stock = GoodsInventory::new(shared::economy::capacity::HALL);
    assert_eq!(stock.add(Good::Food, 2), 2);
    let settlement_id = shared::components::SettlementId(700);
    let seller_id = shared::components::BuildingId(701);
    let seller_company_id = shared::components::CompanyId(702);
    let seller_company = spawn_test_company(&mut app, seller_company_id.0, 0);
    let mut market = MootMarket::founding();
    market.consign(
        shared::economy::MarketSeller::Business(seller_id),
        Good::Food,
        2,
        Good::Food.base_price(),
    );
    let hall = app
        .world_mut()
        .spawn((
            settlement_id,
            Settlement {
                name: "Coinbread".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 1,
                treasury: STARTING_TREASURY_MONEY,
            },
            stock,
            market,
        ))
        .id();
    let _business = app
        .world_mut()
        .spawn((
            seller_id,
            shared::components::BuildingOf(settlement_id),
            shared::components::OperatedBy(seller_company_id),
            BusinessAccount::default(),
        ))
        .id();
    let resident = app
        .world_mut()
        .spawn((
            CharacterName("Ada".to_string()),
            CharacterKind::Villager,
            VillagerIntent::Resident { settlement: hall },
            Wallet::founding_villager(),
        ))
        .id();

    app.update();
    let wallet_before = app.world().get::<Wallet>(resident).unwrap().balance();
    let treasury_before = app.world().get::<Settlement>(hall).unwrap().treasury;
    app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 1;
    app.update();

    let wallet_after = app.world().get::<Wallet>(resident).unwrap().balance();
    let market = app.world().get::<MootMarket>(hall).unwrap();
    assert!(wallet_after < wallet_before, "the household did not pay");
    let business_after = app
        .world()
        .get::<CompanyAccount>(seller_company)
        .unwrap()
        .cash;
    let treasury_after = app.world().get::<Settlement>(hall).unwrap().treasury;
    assert_eq!(
        wallet_before + treasury_before,
        wallet_after + business_after + treasury_after,
        "a ration must transfer coin from its consumer to its private producer and the market fee"
    );
    assert_eq!(market.pool(Good::Food).units_sold, 1);
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(hall)
            .unwrap()
            .amount(Good::Food),
        1
    );
}

#[test]
fn an_empty_pantry_spends_discretionary_coin_before_accepting_hunger() {
    let mut app = village_test_app();
    app.init_resource::<BusinessEventQueue>();
    app.add_systems(Update, update_household_budgets_and_pantries);
    app.world_mut().spawn(WorldTime::new_default());
    let settlement_id = shared::components::SettlementId(706);
    let mut hall_stock = GoodsInventory::new(shared::economy::capacity::HALL);
    hall_stock.add(Good::Bread, 3);
    let mut market = MootMarket::founding();
    market.consign(
        shared::economy::MarketSeller::Treasury(settlement_id),
        Good::Bread,
        3,
        Good::Bread.base_price(),
    );
    let hall = app
        .world_mut()
        .spawn((
            settlement_id,
            Settlement {
                name: "Needford".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            },
            PlayerPosition(Vec3::ZERO),
            hall_stock,
            market,
        ))
        .id();
    let home = app
        .world_mut()
        .spawn((
            SettlementBuilding {
                kind: SettlementBuildingKind::House,
                settlement: "Needford".to_string(),
                owner: None,
                quality: 1.0,
                workers: Vec::new(),
            },
            shared::components::BuildingOf(settlement_id),
            Household {
                resident_ids: vec![shared::components::PersonId(707)],
                residents: vec!["Ada".to_string()],
            },
            HouseholdEconomy::default(),
            GoodsInventory::new(shared::economy::capacity::HOUSE),
        ))
        .id();
    let resident = app
        .world_mut()
        .spawn((
            shared::components::PersonId(707),
            CharacterName("Ada".to_string()),
            Wallet::new(2 * PENNIES_PER_COIN),
            WorkStatus::LookingForWork,
            HomeAssignment { home },
        ))
        .id();

    app.update();

    assert_eq!(app.world().get::<Wallet>(resident).unwrap().balance(), 0);
    assert_eq!(
        app.world().get::<HouseholdEconomy>(home).unwrap().pennies,
        20,
    );
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(home)
            .unwrap()
            .amount(Good::Bread),
        1,
    );
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(hall)
            .unwrap()
            .amount(Good::Bread),
        2,
    );
}

#[test]
fn poor_relief_buys_a_ration_from_public_money() {
    let mut app = village_test_app();
    app.init_resource::<SettlementEconomyRuntime>();
    app.add_systems(
        Update,
        (
            ensure_settlement_economies,
            update_moot_market_targets,
            update_settlement_economies,
            apply_business_events,
        )
            .chain(),
    );
    let clock = app.world_mut().spawn(WorldTime::new_default()).id();
    let mut stock = GoodsInventory::new(shared::economy::capacity::HALL);
    assert_eq!(stock.add(Good::Bread, 5), 5);
    let settlement_id = shared::components::SettlementId(710);
    let seller_id = shared::components::BuildingId(711);
    let seller_company_id = shared::components::CompanyId(712);
    let seller_company = spawn_test_company(&mut app, seller_company_id.0, 0);
    let mut market = MootMarket::founding();
    market.consign(
        shared::economy::MarketSeller::Business(seller_id),
        Good::Bread,
        5,
        Good::Bread.base_price(),
    );
    let hall = app
        .world_mut()
        .spawn((
            settlement_id,
            Settlement {
                name: "Almsford".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 1,
                treasury: STARTING_TREASURY_MONEY,
            },
            stock,
            market,
            SettlementPolicies::poor_relief(),
        ))
        .id();
    let _business = app
        .world_mut()
        .spawn((
            seller_id,
            shared::components::BuildingOf(settlement_id),
            shared::components::OperatedBy(seller_company_id),
            BusinessAccount::default(),
        ))
        .id();
    let poor = app
        .world_mut()
        .spawn((
            CharacterName("Ada".to_string()),
            CharacterKind::Villager,
            VillagerIntent::Resident { settlement: hall },
            Wallet::new(0),
        ))
        .id();

    app.update();
    app.world_mut()
        .resource_mut::<SettlementEconomyRuntime>()
        .record_food_production(hall, 1);
    let money_before = app.world().get::<Settlement>(hall).unwrap().treasury;
    app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 1;
    app.update();

    let economy = app.world().get::<SettlementEconomy>(hall).unwrap();
    assert_eq!(economy.unmet_food, 0);
    assert_eq!(economy.recent_food_consumption, 1.0);
    assert_eq!(economy.edible_stock, 4);
    assert_eq!(app.world().get::<Wallet>(poor).unwrap().balance(), 0);
    assert!(
        app.world().get::<Settlement>(hall).unwrap().treasury < STARTING_TREASURY_MONEY,
        "the policy must spend public money rather than mint a meal"
    );
    let money_after = app.world().get::<Settlement>(hall).unwrap().treasury
        + app
            .world()
            .get::<CompanyAccount>(seller_company)
            .unwrap()
            .cash;
    assert_eq!(money_after, money_before);
}

#[test]
fn tactical_poor_relief_reserves_food_then_sends_the_recipient_to_the_moot_line() {
    let mut app = village_test_app();
    app.init_resource::<SettlementEconomyRuntime>();
    app.init_resource::<MootQueueClock>();
    app.init_resource::<RegionRegistry>();
    app.add_systems(
        Update,
        (
            ensure_settlement_economies,
            update_moot_market_targets,
            update_settlement_economies,
        )
            .chain(),
    );
    let region = RegionCoord::new(0, 0);
    app.world_mut()
        .resource_mut::<RegionRegistry>()
        .set_level_for_test(region, SimLevel::Tactical);
    let clock = app.world_mut().spawn(WorldTime::new_default()).id();
    let mut stock = GoodsInventory::new(shared::economy::capacity::HALL);
    stock.add(Good::Bread, 5);
    let settlement_id = shared::components::SettlementId(720);
    let seller_id = shared::components::BuildingId(721);
    let mut market = MootMarket::founding();
    market.consign(
        shared::economy::MarketSeller::Business(seller_id),
        Good::Bread,
        5,
        Good::Bread.base_price(),
    );
    let hall = app
        .world_mut()
        .spawn((
            settlement_id,
            Settlement {
                name: "Visible Almsford".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 1,
                treasury: STARTING_TREASURY_MONEY,
            },
            PlayerPosition(Vec3::ZERO),
            stock,
            market,
            SettlementPolicies::poor_relief(),
        ))
        .id();
    let poor = app
        .world_mut()
        .spawn((
            CharacterName("Ada".to_string()),
            CharacterKind::Villager,
            VillagerIntent::Resident { settlement: hall },
            Wallet::new(0),
            Nutrition::default(),
            RegionCoord::new(0, 0),
        ))
        .id();

    app.update();
    app.world_mut()
        .resource_mut::<SettlementEconomyRuntime>()
        .record_food_production(hall, 1);
    app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 1;
    app.update();

    let ticket = app.world().get::<MootQueueTicket>(poor).unwrap();
    assert_eq!(ticket.hall, hall);
    assert_eq!(ticket.kind, MootServiceKind::PoorRelief);
    assert_eq!(
        app.world().get::<MootMealRoutine>(poor).unwrap().good,
        Good::Bread
    );
    assert_eq!(
        app.world().get::<Nutrition>(poor).unwrap().last_meal_day,
        None,
        "authorization is not the same event as physically collecting the meal"
    );
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(hall)
            .unwrap()
            .amount(Good::Bread),
        4,
        "the reserved ration must no longer be sellable hall stock"
    );
    assert_eq!(
        app.world()
            .get::<SettlementEconomy>(hall)
            .unwrap()
            .unmet_food,
        0,
        "an authorized physical ration is committed consumption"
    );
}

#[test]
fn poor_relief_protects_an_unsustainable_or_thin_reserve() {
    fn run_case(stock: u32, production: u32) -> (u32, u32, u64) {
        let mut app = village_test_app();
        app.init_resource::<SettlementEconomyRuntime>();
        app.add_systems(
            Update,
            (
                ensure_settlement_economies,
                update_moot_market_targets,
                update_settlement_economies,
            )
                .chain(),
        );
        let clock = app.world_mut().spawn(WorldTime::new_default()).id();
        let mut inventory = GoodsInventory::new(shared::economy::capacity::HALL);
        assert_eq!(inventory.add(Good::Food, stock), stock);
        let hall = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Reserveford".to_string(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 1,
                    treasury: STARTING_TREASURY_MONEY,
                },
                inventory,
                MootMarket::founding(),
                SettlementPolicies::poor_relief(),
            ))
            .id();
        app.world_mut().spawn((
            CharacterKind::Villager,
            VillagerIntent::Resident { settlement: hall },
            Wallet::new(0),
        ));

        app.update();
        app.world_mut()
            .resource_mut::<SettlementEconomyRuntime>()
            .record_food_production(hall, production);
        app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 1;
        app.update();

        (
            app.world()
                .get::<SettlementEconomy>(hall)
                .unwrap()
                .unmet_food,
            app.world()
                .get::<GoodsInventory>(hall)
                .unwrap()
                .edible_amount(),
            app.world().get::<Settlement>(hall).unwrap().treasury,
        )
    }

    assert_eq!(
        run_case(8, 0),
        (1, 8, STARTING_TREASURY_MONEY),
        "stock alone must not disguise failed production"
    );
    assert_eq!(
        run_case(3, 1),
        (1, 3, STARTING_TREASURY_MONEY),
        "relief must not spend the last three reserve days"
    );
    let funded_surplus = run_case(8, 1);
    assert_eq!(funded_surplus.0, 0);
    assert_eq!(funded_surplus.1, 7);
    assert!(
        funded_surplus.2 < STARTING_TREASURY_MONEY,
        "active production plus stock above the protected floor should fund one relief ration"
    );
}

#[test]
fn a_broke_resident_declines_to_a_safe_floor_then_dies_after_ten_hungry_days() {
    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.init_resource::<SettlementEconomyRuntime>();
    app.init_resource::<MortalityLedger>();
    app.init_resource::<CompanyEscrowRefundQueue>();
    app.add_systems(
        Update,
        (
            ensure_settlement_economies,
            update_moot_market_targets,
            update_settlement_economies,
            apply_nutrition_condition,
            advance_nutrition_health,
            process_character_deaths,
        )
            .chain(),
    );
    let clock = app.world_mut().spawn(WorldTime::new_default()).id();
    let mut stock = GoodsInventory::new(shared::economy::capacity::HALL);
    stock.add(Good::Food, 1);
    let settlement_id = shared::components::SettlementId(900);
    let hall = app
        .world_mut()
        .spawn((
            settlement_id,
            Settlement {
                name: "Hardmarket".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 1,
                treasury: STARTING_TREASURY_MONEY,
            },
            stock,
            MootMarket::founding(),
            MootAdministration::default(),
            SettlementPolicies {
                poor_relief: shared::components::PoorReliefMode::Off,
                ..default()
            },
        ))
        .id();
    let resident = app
        .world_mut()
        .spawn((
            CharacterName("Hungry Ada".to_string()),
            CharacterKind::Villager,
            shared::components::CharacterAffiliation::default(),
            CharacterAttributes::default(),
            VillagerIntent::Resident { settlement: hall },
            shared::components::ResidentOf(settlement_id),
            Wallet::new(0),
            Nutrition::default(),
            shared::components::Health::default(),
        ))
        .id();

    app.update();
    app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 1;
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f32(
            WorldTime::new_default().cycle_duration(),
        ));
    app.update();

    assert_eq!(
        app.world()
            .get::<SettlementEconomy>(hall)
            .unwrap()
            .unmet_food,
        1
    );
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(hall)
            .unwrap()
            .amount(Good::Food),
        1,
        "an unaffordable ration must remain real market stock"
    );
    assert_eq!(
        app.world().get::<Settlement>(hall).unwrap().treasury,
        STARTING_TREASURY_MONEY,
        "disabled relief must not spend public money"
    );
    let nutrition = app.world().get::<Nutrition>(resident).unwrap();
    assert!(nutrition.is_hungry());
    assert_eq!(nutrition.consecutive_missed_meals, 1);
    assert_eq!(nutrition.last_meal_day, None);
    assert_eq!(
        app.world()
            .get::<shared::components::Health>(resident)
            .unwrap()
            .current,
        80.0,
        "the first hungry day should lower Health toward its nonlethal ceiling"
    );

    for day in 2..=10 {
        app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = day;
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(
                WorldTime::new_default().cycle_duration(),
            ));
        app.update();
        let nutrition = *app.world().get::<Nutrition>(resident).unwrap();
        let health = app
            .world()
            .get::<shared::components::Health>(resident)
            .unwrap()
            .current;
        let expected = f32::from(nutrition.health_ceiling_percent());
        assert!(
            (health - expected).abs() < 0.01,
            "day {day} Health {health} did not settle at nutrition ceiling {expected}",
        );
    }
    assert!(app.world().get_entity(resident).is_ok());

    app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 11;
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f32(
            WorldTime::new_default().cycle_duration(),
        ));
    app.update();
    assert!(app.world().get_entity(resident).is_err());
    let mortality = app.world().resource::<MortalityLedger>();
    assert_eq!(mortality.total_deaths, 1);
    assert_eq!(
        mortality.iter().next().unwrap().cause,
        shared::components::DeathCause::Starvation
    );
}
