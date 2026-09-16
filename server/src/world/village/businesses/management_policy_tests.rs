use super::*;
use shared::components::{
    BuildingId, BuildingOf, CompanyId, CompanyOwnership, OperatedBy, OwnedBy, PersonId,
    SettlementId,
};
use shared::economy::CompanyAccount;

fn management_fixture(
    ownership: Option<CompanyOwnership>,
) -> (App, Entity, Entity, Entity, Entity) {
    let mut app = App::new();
    app.add_systems(Update, review_business_management);
    let mut time = WorldTime::new_default();
    time.day = 8;
    let clock = app.world_mut().spawn(time).id();
    let town = SettlementId(951);
    app.world_mut().spawn((
        town,
        Settlement {
            name: "Ledgerford".into(),
            tier: shared::components::SettlementTier::Village,
            residents: 1,
            treasury: 0,
        },
        MootMarket::founding(),
    ));
    let founder = PersonId(952);
    let person = app
        .world_mut()
        .spawn((
            founder,
            Wallet::new(1_000),
            Occupation(None),
            WorkStatus::LookingForWork,
        ))
        .id();
    let company_id = CompanyId(953);
    let company = app
        .world_mut()
        .spawn((
            company_id,
            CompanyAccount {
                wage_arrears: 100,
                ..default()
            },
        ))
        .id();
    if let Some(ownership) = ownership {
        app.world_mut().entity_mut(company).insert(ownership);
    }
    let mut inventory = GoodsInventory::new(100);
    inventory.add(Good::Wood, 4);
    let business = app
        .world_mut()
        .spawn((
            BuildingId(954),
            BuildingOf(town),
            OperatedBy(company_id),
            OwnedBy(founder),
            SettlementBuilding {
                kind: SettlementBuildingKind::LumberjackHut,
                settlement: "Ledgerford".into(),
                owner: Some("Founder".into()),
                quality: 1.0,
                workers: Vec::new(),
            },
            inventory,
            BusinessAccount {
                wage_arrears: 100,
                ..default()
            },
            BusinessSalePolicy::for_good(Good::Wood),
            BusinessWagePolicy::default(),
            BusinessManagementPolicy::default(),
            BusinessCondition {
                state: BusinessState::Insolvent,
                opened_day: 0,
                ..default()
            },
        ))
        .id();
    (app, clock, company, person, business)
}

#[test]
fn liquidation_continues_marking_down_uncollected_stock_to_one_penny() {
    let (mut app, clock, _, _, business) = management_fixture(None);
    app.world_mut()
        .entity_mut(business)
        .insert(BusinessLiquidation::insolvency(7, Vec::new()));
    let mut expected = app
        .world()
        .get::<BusinessSalePolicy>(business)
        .unwrap()
        .asking_unit_price;
    for day in 8..48 {
        app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = day;
        expected = price_step(expected, LIQUIDATION_DAILY_MARKDOWN_BPS, false);
        app.update();
        assert_eq!(
            app.world()
                .get::<BusinessSalePolicy>(business)
                .unwrap()
                .asking_unit_price,
            expected
        );
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(business)
                .unwrap()
                .amount(Good::Wood),
            4
        );
        assert_eq!(
            app.world()
                .get::<BusinessCondition>(business)
                .unwrap()
                .state,
            BusinessState::Liquidating
        );
    }
    assert_eq!(expected, 1);
}

#[test]
fn automatic_rescue_requires_the_actual_sole_owner_and_a_receiving_treasury() {
    let founder = PersonId(952);
    let mut shared = CompanyOwnership::sole(founder);
    assert!(shared.transfer(founder, PersonId(955), 400));
    for ownership in [
        None,
        Some(shared),
        Some(CompanyOwnership::sole(PersonId(956))),
    ] {
        let (mut app, _, company, person, business) = management_fixture(ownership);
        app.update();
        assert_eq!(app.world().get::<Wallet>(person).unwrap().balance(), 1_000);
        assert_eq!(app.world().get::<CompanyAccount>(company).unwrap().cash, 0);
        assert_eq!(
            app.world()
                .get::<BusinessAccount>(business)
                .unwrap()
                .contributed_capital,
            0
        );
        assert_eq!(
            app.world()
                .get::<BusinessCondition>(business)
                .unwrap()
                .state,
            BusinessState::Insolvent
        );
    }
    let (mut app, _, company, person, business) =
        management_fixture(Some(CompanyOwnership::sole(founder)));
    app.world_mut()
        .entity_mut(company)
        .remove::<CompanyAccount>();
    app.update();
    assert_eq!(app.world().get::<Wallet>(person).unwrap().balance(), 1_000);
    assert_eq!(
        app.world()
            .get::<BusinessAccount>(business)
            .unwrap()
            .contributed_capital,
        0
    );
}

#[test]
fn a_sole_owner_rescue_is_conserved_capital_not_revenue() {
    let (mut app, _, company, person, business) =
        management_fixture(Some(CompanyOwnership::sole(PersonId(952))));
    app.update();
    assert_eq!(app.world().get::<Wallet>(person).unwrap().balance(), 800);
    assert_eq!(
        app.world().get::<CompanyAccount>(company).unwrap().cash,
        200
    );
    let account = app.world().get::<BusinessAccount>(business).unwrap();
    assert_eq!(account.contributed_capital, 200);
    assert_eq!(account.gross_revenue, 0);
    assert_eq!(
        app.world()
            .get::<BusinessCondition>(business)
            .unwrap()
            .state,
        BusinessState::Distressed
    );
}

#[test]
fn personal_rescue_caps_at_the_treasurys_actual_room() {
    let mut wallet = Wallet::new(1_000);
    let mut company = CompanyAccount {
        cash: u64::MAX - 5,
        ..default()
    };
    let mut account = BusinessAccount::default();
    assert!(contribute_rescue_capital(
        &mut wallet,
        &mut company,
        &mut account
    ));
    assert_eq!(wallet.balance(), 995);
    assert_eq!(company.cash, u64::MAX);
    assert_eq!(account.contributed_capital, 5);
    assert!(!contribute_rescue_capital(
        &mut wallet,
        &mut company,
        &mut account
    ));
    assert_eq!(wallet.balance(), 995);
}

#[test]
fn bankruptcy_requests_a_real_final_load_and_exit_before_releasing_staff() {
    let (mut app, clock, _, _, business) = management_fixture(None);
    // Replace the single-system fixture with the real manager -> employment
    // ordering, while inspecting the handoff before the trade runner acts.
    app.add_systems(
        Update,
        enforce_staffing_targets.after(review_business_management),
    );
    app.world_mut()
        .get_mut::<BusinessCondition>(business)
        .unwrap()
        .insolvent_days = 4;
    app.world_mut()
        .get_mut::<SettlementBuilding>(business)
        .unwrap()
        .kind = SettlementBuildingKind::FishermansHut;
    app.world_mut()
        .get_mut::<GoodsInventory>(business)
        .unwrap()
        .remove(Good::Wood, 4);
    let mut cargo = GoodsInventory::new(shared::economy::capacity::VILLAGER);
    cargo.add(Good::Food, 2);
    let destination = Vec3::new(4.0, 1.0, 3.0);
    let worker = app
        .world_mut()
        .spawn((
            PersonId(958),
            shared::components::EmployedAt(BuildingId(954)),
            Occupation(Some("Fisher".into())),
            WorkStatus::Employed,
            Wallet::new(0),
            cargo,
            CharacterActivity::Fishing,
            MoveTarget(destination),
            PierTraversal {
                deck_start: Vec3::ZERO,
                deck_end: destination,
            },
            FishingRoutine {
                hut: business,
                pier: business,
                hall: business,
                catch_seconds: 0.4,
                failed_workplace_routes: 0,
                production_day: 8,
                produced_today: 2,
                phase: FishingPhase::Fishing,
            },
        ))
        .id();
    app.update();
    assert_eq!(
        app.world()
            .get::<BusinessCondition>(business)
            .unwrap()
            .state,
        BusinessState::Liquidating
    );
    assert_eq!(
        app.world()
            .get::<BusinessStaffingPolicy>(business)
            .unwrap()
            .enabled_positions,
        0
    );
    assert!(app
        .world()
        .get::<super::super::super::worker_activity::EmploymentReleaseRequested>(worker)
        .is_some());
    assert!(app
        .world()
        .get::<shared::components::EmployedAt>(worker)
        .is_some());
    assert!(app.world().get::<FishingRoutine>(worker).is_some());
    assert!(app.world().get::<PierTraversal>(worker).is_some());
    assert_eq!(
        app.world().get::<MoveTarget>(worker).unwrap().0,
        destination
    );
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(worker)
            .unwrap()
            .amount(Good::Food),
        2
    );
    assert!(
        !app.world()
            .get::<BusinessLiquidation>(business)
            .unwrap()
            .staff_released
    );
    for day in 9..=11 {
        app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = day;
        app.update();
        assert_eq!(
            app.world()
                .get::<BusinessLiquidation>(business)
                .unwrap()
                .empty_days,
            0
        );
        assert!(app.world().get::<BusinessForSale>(business).is_none());
    }

    // Simulate only the trade-specific final deposit/exit completing. The
    // shared release pass must now clear the employment exactly once.
    app.world_mut()
        .get_mut::<GoodsInventory>(worker)
        .unwrap()
        .remove(Good::Food, 2);
    app.world_mut()
        .entity_mut(worker)
        .remove::<FishingRoutine>()
        .remove::<PierTraversal>();
    app.update();
    assert!(app
        .world()
        .get::<shared::components::EmployedAt>(worker)
        .is_none());
    assert_eq!(
        *app.world().get::<WorkStatus>(worker).unwrap(),
        WorkStatus::LookingForWork
    );
    for day in 12..=13 {
        app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = day;
        app.update();
    }
    assert_eq!(
        app.world()
            .get::<BusinessCondition>(business)
            .unwrap()
            .state,
        BusinessState::ForSale
    );
}
