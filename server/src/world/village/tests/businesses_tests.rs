//! Village businesses regression fixtures and invariants.

use super::*;

#[test]
fn insolvent_business_liquidates_stock_then_becomes_for_sale_without_rehiring() {
    let mut app = village_test_app();
    app.add_systems(Update, review_business_management);
    let clock = app.world_mut().spawn(WorldTime::new_default()).id();
    let settlement_id = shared::components::SettlementId(820);
    app.world_mut().spawn((
        settlement_id,
        Settlement {
            name: "Failford".into(),
            tier: shared::components::SettlementTier::Hamlet,
            residents: 1,
            treasury: 0,
        },
        MootMarket::founding(),
    ));
    let building_id = shared::components::BuildingId(821);
    let mut business_stock = GoodsInventory::new(shared::economy::capacity::LUMBERJACK_HUT);
    business_stock.add(Good::Wood, 3);
    let business = app
        .world_mut()
        .spawn((
            building_id,
            shared::components::BuildingOf(settlement_id),
            SettlementBuilding {
                kind: SettlementBuildingKind::LumberjackHut,
                settlement: "Failford".into(),
                owner: None,
                quality: 1.0,
                workers: vec!["Ada".into()],
            },
            business_stock,
            BusinessAccount {
                wage_arrears: PENNIES_PER_COIN,
                tax_arrears: PENNIES_PER_COIN / 2,
                ..default()
            },
            BusinessSalePolicy::for_good(Good::Wood),
            BusinessWagePolicy::default(),
            BusinessManagementPolicy::default(),
            BusinessCondition::default(),
        ))
        .id();
    let worker = app
        .world_mut()
        .spawn((
            shared::components::PersonId(822),
            CharacterKind::Villager,
            shared::components::EmployedAt(building_id),
            Occupation(Some("Woodcutter".into())),
            WorkStatus::Employed,
            Wallet::default(),
        ))
        .id();

    for day in 0..=4 {
        app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = day;
        app.update();
    }
    assert_eq!(
        app.world()
            .get::<BusinessCondition>(business)
            .unwrap()
            .state,
        shared::economy::BusinessState::Liquidating
    );
    assert!(app
        .world()
        .get::<shared::components::EmployedAt>(worker)
        .is_none());
    assert_eq!(
        *app.world().get::<WorkStatus>(worker).unwrap(),
        WorkStatus::LookingForWork
    );

    assert!(app.world().get::<BusinessLiquidation>(business).is_some());
    assert!(
        app.world()
            .get::<BusinessSalePolicy>(business)
            .unwrap()
            .collection_enabled
    );

    // Stock is not destroyed by bankruptcy. Once a porter has physically
    // removed it and the seller has no listings in flight, the shell waits two
    // empty reviews before it enters the property market.
    app.world_mut()
        .get_mut::<GoodsInventory>(business)
        .unwrap()
        .remove(Good::Wood, 3);
    for day in 5..=6 {
        app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = day;
        app.update();
    }
    app.update();
    assert_eq!(
        app.world()
            .get::<BusinessCondition>(business)
            .unwrap()
            .state,
        shared::economy::BusinessState::ForSale,
        "bankruptcy requires an explicit takeover rather than reopening because time passed"
    );
    assert_eq!(
        app.world().get::<BusinessForSale>(business).unwrap().reason,
        shared::economy::BusinessSaleReason::Insolvent,
    );
    let account = app.world().get::<BusinessAccount>(business).unwrap();
    assert_eq!(account.wage_arrears, 0);
    assert_eq!(account.tax_arrears, 0);
    assert_eq!(account.defaulted_wages, PENNIES_PER_COIN);
    assert_eq!(account.defaulted_taxes, PENNIES_PER_COIN / 2);
}

#[test]
fn player_owner_receives_only_profit_above_protected_working_capital() {
    let mut app = village_test_app();
    app.init_resource::<CompanyDividendQueue>();
    app.add_systems(Update, review_company_finance);
    let mut clock = WorldTime::new_default();
    clock.day = 4;
    app.world_mut().spawn(clock);
    let settlement_id = shared::components::SettlementId(825);
    app.world_mut().spawn((
        settlement_id,
        Settlement {
            name: "Ownerford".into(),
            tier: shared::components::SettlementTier::Hamlet,
            residents: 0,
            treasury: 0,
        },
        MootMarket::founding(),
    ));
    let owner = shared::components::PersonId(826);
    let hero = app
        .world_mut()
        .spawn((
            owner,
            shared::components::Hero {
                owner: lightyear::prelude::PeerId::Netcode(826),
            },
            Wallet::default(),
        ))
        .id();
    let building_id = shared::components::BuildingId(827);
    let company_id = shared::components::CompanyId(828);
    app.world_mut().spawn((
        company_id,
        CompanyOwnership::sole(owner),
        CompanyAccount {
            cash: 10 * PENNIES_PER_COIN,
            ..default()
        },
        CompanyManagementPolicy {
            max_daily_dividend: 2 * PENNIES_PER_COIN,
            ..default()
        },
    ));
    let account = BusinessAccount {
        gross_revenue: 10 * PENNIES_PER_COIN,
        ..default()
    };
    let management = BusinessManagementPolicy {
        max_daily_withdrawal: 2 * PENNIES_PER_COIN,
        ..default()
    };
    app.world_mut().spawn((
        building_id,
        shared::components::BuildingOf(settlement_id),
        shared::components::OwnedBy(owner),
        shared::components::OperatedBy(company_id),
        SettlementBuilding {
            kind: SettlementBuildingKind::LumberjackHut,
            settlement: "Ownerford".into(),
            owner: Some("Player".into()),
            quality: 1.0,
            workers: Vec::new(),
        },
        GoodsInventory::new(shared::economy::capacity::LUMBERJACK_HUT),
        account,
        BusinessSalePolicy::for_good(Good::Wood),
        BusinessWagePolicy::default(),
        BusinessProcurementPolicy::default(),
        management,
        BusinessCondition {
            state: BusinessState::Operating,
            opened_day: 0,
            last_review_day: 3,
            ..default()
        },
    ));

    app.update();

    assert_eq!(
        app.world().get::<Wallet>(hero).unwrap().balance(),
        2 * PENNIES_PER_COIN,
        "player owners must receive the same bounded daily draw as NPC owners"
    );
}

#[test]
fn an_unbought_inherited_business_releases_staff_and_liquidates_its_inputs() {
    let mut app = village_test_app();
    app.add_systems(Update, review_business_management);
    let clock = app.world_mut().spawn(WorldTime::new_default()).id();
    app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 8;
    let settlement_id = shared::components::SettlementId(830);
    app.world_mut().spawn((
        settlement_id,
        Settlement {
            name: "Widowmere".into(),
            tier: shared::components::SettlementTier::Hamlet,
            residents: 1,
            treasury: 0,
        },
        MootMarket::founding(),
    ));
    let building_id = shared::components::BuildingId(831);
    let mut stock = GoodsInventory::new(shared::economy::capacity::BAKERY);
    stock.add(Good::Flour, 6);
    let business = app
        .world_mut()
        .spawn((
            building_id,
            shared::components::BuildingOf(settlement_id),
            SettlementBuilding {
                kind: SettlementBuildingKind::Bakery,
                settlement: "Widowmere".into(),
                owner: None,
                quality: 1.0,
                workers: vec!["Ivo".into()],
            },
            stock,
            BusinessAccount {
                wage_arrears: PENNIES_PER_COIN,
                ..default()
            },
            BusinessSalePolicy::for_good(Good::Bread),
            BusinessWagePolicy::default(),
            BusinessManagementPolicy::default(),
            BusinessCondition {
                state: shared::economy::BusinessState::Liquidating,
                ..default()
            },
            BusinessForSale {
                previous_owner: shared::components::PersonId(832),
                asking_price: PENNIES_PER_COIN,
                listed_day: 8,
                reason: shared::economy::BusinessSaleReason::OwnerDied,
            },
            BusinessLiquidation::owner_died(8),
        ))
        .id();
    let worker = app
        .world_mut()
        .spawn((
            shared::components::PersonId(833),
            CharacterKind::Villager,
            shared::components::EmployedAt(building_id),
            Occupation(Some("Baker".into())),
            WorkStatus::Employed,
            Wallet::default(),
        ))
        .id();

    app.update();

    let liquidation = app.world().get::<BusinessLiquidation>(business).unwrap();
    assert!(liquidation.staff_released);
    assert_eq!(liquidation.outstanding_wages(), PENNIES_PER_COIN);
    assert!(app
        .world()
        .get::<shared::components::EmployedAt>(worker)
        .is_none());
    assert_eq!(
        *app.world().get::<WorkStatus>(worker).unwrap(),
        WorkStatus::LookingForWork
    );
    let sale = app.world().get::<BusinessSalePolicy>(business).unwrap();
    assert!(sale.collection_enabled);
    assert_eq!(sale.company_reserve_units, 0);
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(business)
            .unwrap()
            .amount(Good::Flour),
        6,
        "liquidation exposes stock to physical porter collection; it never teleports it"
    );
}
