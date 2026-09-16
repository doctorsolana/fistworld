use super::*;

#[test]
fn takeover_waits_for_old_employee_and_freight_handoffs() {
    use crate::world::village::{MarketCollectionPhase, MarketCollectionRoutine};
    let (mut app, clock, hall, property, buyer) = fixture(2_000, true);
    let building = shared::components::BuildingId(97);
    app.world_mut().entity_mut(property).insert(building);
    let worker = app
        .world_mut()
        .spawn(shared::components::EmployedAt(building))
        .id();
    app.update();
    assert!(app.world().get::<OwnedBy>(property).is_none());
    assert_eq!(app.world().get::<Wallet>(buyer).unwrap().balance(), 2_000);

    app.world_mut()
        .entity_mut(worker)
        .remove::<shared::components::EmployedAt>();
    let carrier = app
        .world_mut()
        .spawn(MarketCollectionRoutine {
            business: property,
            seller: building,
            hall,
            counter: Vec3::ZERO,
            good: Good::Wood,
            reserved_units: 2,
            unit_price: 50,
            phase: MarketCollectionPhase::ReturningToHall,
            fallback_counter_attempted: false,
        })
        .id();
    app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 2;
    app.update();
    assert!(app.world().get::<OwnedBy>(property).is_none());
    assert_eq!(app.world().get::<Wallet>(buyer).unwrap().balance(), 2_000);

    app.world_mut()
        .entity_mut(carrier)
        .remove::<MarketCollectionRoutine>();
    app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 3;
    app.update();
    assert_eq!(
        app.world().get::<OwnedBy>(property),
        Some(&OwnedBy(PersonId(2)))
    );
    let capital = app
        .world()
        .get::<BusinessAccount>(property)
        .unwrap()
        .unposted_company_capital;
    assert_eq!(
        app.world().get::<Wallet>(buyer).unwrap().balance() + capital,
        2_000
    );
}

#[test]
fn acquisition_returns_original_liquidation_creditors_to_live_payroll() {
    use shared::economy::BusinessWageClaim;
    let (mut app, _, _, property, _) = fixture(2_000, true);
    app.world_mut()
        .get_mut::<BusinessAccount>(property)
        .unwrap()
        .wage_arrears = 50;
    app.world_mut().entity_mut(property).insert((
        BusinessLiquidation::insolvency(
            0,
            vec![BusinessWageClaim {
                worker: PersonId(91),
                pennies: 30,
            }],
        ),
        PrivatePayrollClaims {
            claims: vec![BusinessWageClaim {
                worker: PersonId(92),
                pennies: 20,
            }],
        },
    ));
    app.update();
    assert!(app.world().get::<OwnedBy>(property).is_some());
    assert!(app.world().get::<BusinessLiquidation>(property).is_none());
    let claims = &app
        .world()
        .get::<PrivatePayrollClaims>(property)
        .unwrap()
        .claims;
    assert_eq!(
        claims
            .iter()
            .find(|claim| claim.worker == PersonId(91))
            .unwrap()
            .pennies,
        30
    );
    assert_eq!(
        claims
            .iter()
            .find(|claim| claim.worker == PersonId(92))
            .unwrap()
            .pennies,
        20
    );
    assert_eq!(
        app.world()
            .get::<BusinessAccount>(property)
            .unwrap()
            .wage_arrears,
        50
    );
}

#[test]
fn acquisition_reindexes_rotated_liquidation_claims_when_live_payroll_is_absent() {
    use shared::economy::BusinessWageClaim;
    let (mut app, _, _, property, _) = fixture(2_000, true);
    app.world_mut()
        .get_mut::<BusinessAccount>(property)
        .unwrap()
        .wage_arrears = 50;
    app.world_mut()
        .entity_mut(property)
        .insert(BusinessLiquidation::insolvency(
            0,
            vec![
                BusinessWageClaim {
                    worker: PersonId(92),
                    pennies: 20,
                },
                BusinessWageClaim {
                    worker: PersonId(91),
                    pennies: 30,
                },
            ],
        ));
    app.update();
    let mut payroll = app
        .world_mut()
        .get_mut::<PrivatePayrollClaims>(property)
        .unwrap();
    assert_eq!(
        payroll
            .claims
            .iter()
            .map(|claim| claim.worker)
            .collect::<Vec<_>>(),
        vec![PersonId(91), PersonId(92)]
    );
    payroll.accrue(PersonId(91), 5);
    assert_eq!(payroll.claims.len(), 2);
    assert_eq!(payroll.claims[0].pennies, 35);
}

fn fixture(cash: u64, funded: bool) -> (App, Entity, Entity, Entity, Entity) {
    let mut app = App::new();
    app.add_systems(Update, acquire_businesses_for_sale);
    let mut time = WorldTime::new_default();
    time.day = 1;
    let clock = app.world_mut().spawn(time).id();
    let settlement_id = shared::components::SettlementId(1);
    let mut market = MootMarket::founding();
    if funded {
        market.record_unmet_demand(Good::Wood, 6, 0, 6, 50);
    }
    let hall = app.world_mut().spawn((settlement_id, market)).id();
    let property = app
        .world_mut()
        .spawn((
            shared::components::BuildingOf(settlement_id),
            SettlementBuilding {
                kind: SettlementBuildingKind::LumberjackHut,
                settlement: "Moot".into(),
                owner: None,
                quality: 1.0,
                workers: vec![],
            },
            BusinessForSale {
                previous_owner: PersonId(1),
                asking_price: 250,
                listed_day: 0,
                reason: BusinessSaleReason::VoluntaryClosure,
            },
            BusinessAccount::default(),
            BusinessCondition {
                state: BusinessState::ForSale,
                ..default()
            },
            BusinessSalePolicy::for_good(Good::Wood),
            GoodsInventory::new(100),
            BusinessWagePolicy::default(),
        ))
        .id();
    let buyer = app
        .world_mut()
        .spawn((
            PersonId(2),
            CharacterName("Borin".into()),
            ResidentOf(settlement_id),
            VillagerIntent::Resident { settlement: hall },
            Wallet::new(cash),
            WorkStatus::LookingForWork,
            Occupation::default(),
            Health::default(),
        ))
        .id();
    (app, clock, hall, property, buyer)
}

#[test]
fn wealth_alone_does_not_reopen_a_failed_business_even_after_markdown() {
    let (mut app, clock, _, property, buyer) = fixture(10_000, false);
    app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 20;
    app.update();
    assert_eq!(
        app.world()
            .get::<BusinessForSale>(property)
            .unwrap()
            .asking_price,
        0
    );
    assert!(app.world().get::<OwnedBy>(property).is_none());
    assert_eq!(app.world().get::<Wallet>(buyer).unwrap().balance(), 10_000);
}

#[test]
fn viable_purchase_preserves_personal_runway_and_posts_exact_cash() {
    let (mut app, _, _, property, buyer) = fixture(2_000, true);
    app.update();
    assert_eq!(
        app.world().get::<OwnedBy>(property),
        Some(&OwnedBy(PersonId(2)))
    );
    let capital = app
        .world()
        .get::<BusinessAccount>(property)
        .unwrap()
        .unposted_company_capital;
    assert!(capital >= 250);
    assert_eq!(
        app.world().get::<Wallet>(buyer).unwrap().balance() + capital,
        2_000
    );
    assert!(app.world().get::<BusinessForSale>(property).is_none());
}

fn competing_workplace(app: &mut App, wage: u64) -> Entity {
    app.world_mut()
        .spawn((
            shared::components::BuildingOf(shared::components::SettlementId(1)),
            SettlementBuilding {
                kind: SettlementBuildingKind::Farmstead,
                settlement: "Moot".into(),
                owner: None,
                quality: 1.0,
                workers: vec![],
            },
            BusinessCondition {
                state: BusinessState::Operating,
                ..default()
            },
            BusinessWagePolicy {
                daily_wage: wage,
                ..default()
            },
        ))
        .id()
}

#[test]
fn a_stale_cheap_offer_cannot_make_restart_viable_in_an_expensive_labor_market() {
    for unfinished in [false, true] {
        let (mut app, _, hall, property, buyer) = fixture(2_000, true);
        competing_workplace(&mut app, 300);
        if unfinished {
            app.world_mut()
                .entity_mut(property)
                .remove::<SettlementBuilding>()
                .insert(UnderConstruction {
                    kind: SettlementBuildingKind::LumberjackHut,
                    position: Vec3::ZERO,
                    rotation: 0.0,
                    owner: None,
                    owner_id: None,
                    builder: None,
                    settlement: hall,
                    settlement_id: shared::components::SettlementId(1),
                    stand: Vec3::ZERO,
                    failed_stand_routes: 0,
                    stage: BuildStage::Supplying,
                    quality: 1.0,
                });
        }
        app.update();
        assert!(app.world().get::<BusinessForSale>(property).is_some());
        assert_eq!(app.world().get::<Wallet>(buyer).unwrap().balance(), 2_000);
    }
}

#[test]
fn a_viable_takeover_posts_the_same_local_wage_it_budgeted() {
    let (mut app, _, _, property, buyer) = fixture(2_000, true);
    competing_workplace(&mut app, 200);
    app.update();
    assert!(app.world().get::<BusinessForSale>(property).is_none());
    let wage = app.world().get::<BusinessWagePolicy>(property).unwrap();
    assert_eq!(wage.daily_wage, 200);
    assert!(wage.automatic);
    let capital = app
        .world()
        .get::<BusinessAccount>(property)
        .unwrap()
        .unposted_company_capital;
    assert_eq!(capital, 400);
    assert_eq!(
        app.world().get::<Wallet>(buyer).unwrap().balance() + capital,
        2_000
    );
}

#[test]
fn service_takeovers_recost_the_demonstrated_roster_at_the_next_offer() {
    let ledger = shared::economy::BusinessDayLedger {
        day: 1,
        gross_revenue: 500,
        wage_expense: 200,
        input_expense: 100,
        ..default()
    };
    let old = service_restart_plan(SettlementBuildingKind::Tavern, ledger, 100, 100).unwrap();
    assert_eq!(old.daily_profit, 200);
    assert_eq!(old.working_cash, 600);
    assert!(service_restart_plan(SettlementBuildingKind::Tavern, ledger, 100, 200).is_none());
}

#[test]
fn price_affordability_without_remaining_operating_and_personal_cash_is_insufficient() {
    let (mut app, _, _, property, buyer) = fixture(250, true);
    app.update();
    assert!(app.world().get::<BusinessForSale>(property).is_some());
    assert_eq!(app.world().get::<Wallet>(buyer).unwrap().balance(), 250);
}

#[test]
fn takeover_review_is_daily_and_marks_down_only_by_elapsed_calendar_age() {
    let (mut app, clock, hall, property, _) = fixture(2_000, false);
    app.update();
    assert_eq!(
        app.world()
            .get::<BusinessForSale>(property)
            .unwrap()
            .asking_price,
        250
    );
    app.world_mut()
        .get_mut::<MootMarket>(hall)
        .unwrap()
        .record_unmet_demand(Good::Wood, 6, 0, 6, 50);
    app.update();
    assert!(
        app.world().get::<BusinessForSale>(property).is_some(),
        "new offers are considered on the next daily pass"
    );
    app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 2;
    app.update();
    assert!(app.world().get::<BusinessForSale>(property).is_none());
}

#[test]
fn liability_and_payback_checks_use_cash_required_not_the_sticker_price() {
    let plan = RestartPlan {
        output_units: 4,
        daily_profit: 10,
        working_cash: 200,
        asking_price: 80,
    };
    assert!(required_contribution(1, 0, 0, plan).is_none());
    assert!(required_contribution(1, 200, 500, plan).is_none());
    assert_eq!(required_contribution(100, 200, 0, plan), Some(100));
}

#[test]
fn an_unfinished_property_waits_until_the_buyer_finishes_protected_cargo_work() {
    let (mut app, clock, hall, property, buyer) = fixture(2_000, true);
    app.world_mut()
        .entity_mut(property)
        .remove::<SettlementBuilding>()
        .insert(UnderConstruction {
            kind: SettlementBuildingKind::LumberjackHut,
            position: Vec3::ZERO,
            rotation: 0.0,
            owner: None,
            owner_id: None,
            builder: None,
            settlement: hall,
            settlement_id: shared::components::SettlementId(1),
            stand: Vec3::ZERO,
            failed_stand_routes: 0,
            stage: BuildStage::Supplying,
            quality: 1.0,
        });
    let mut cargo = [0; Good::COUNT];
    cargo[Good::Wood.index()] = 2;
    app.world_mut()
        .entity_mut(buyer)
        .insert(HouseholdShoppingRoutine {
            account: hall,
            household: HouseholdId(1),
            home: hall,
            hall,
            counter: Vec3::ZERO,
            phase: HouseholdShoppingPhase::ReturningHome,
            cargo,
        });
    app.update();
    assert!(app.world().get::<BusinessForSale>(property).is_some());
    assert_eq!(app.world().get::<Wallet>(buyer).unwrap().balance(), 2_000);
    assert_eq!(
        app.world()
            .get::<HouseholdShoppingRoutine>(buyer)
            .unwrap()
            .cargo,
        cargo
    );
    assert!(app
        .world()
        .get::<ConstructionMaterialRoutine>(buyer)
        .is_none());
    app.world_mut()
        .entity_mut(buyer)
        .remove::<HouseholdShoppingRoutine>();
    app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 2;
    app.update();
    assert!(app.world().get::<BusinessForSale>(property).is_none());
    assert_eq!(
        app.world()
            .get::<UnderConstruction>(property)
            .unwrap()
            .builder,
        Some(buyer)
    );
}
