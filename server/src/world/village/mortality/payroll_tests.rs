use super::*;

fn fixture() -> (App, Entity, Entity, Entity, Entity, Entity) {
    let mut app = App::new();
    app.init_resource::<MortalityLedger>()
        .init_resource::<BusinessEventQueue>()
        .init_resource::<CompanyEscrowRefundQueue>()
        .add_systems(Update, process_character_deaths);
    let deceased = PersonId(2);
    let survivor = PersonId(3);
    let household_id = HouseholdId(1);
    let household = app
        .world_mut()
        .spawn((
            household_id,
            HouseholdMembers {
                resident_ids: vec![deceased, survivor],
                settlement: shared::components::SettlementId(1),
                dwelling: None,
            },
            HouseholdEconomy {
                pennies: 10,
                ..default()
            },
        ))
        .id();
    let dead = app
        .world_mut()
        .spawn((
            deceased,
            HouseholdMember(household_id),
            CharacterName("Former worker".into()),
            CharacterKind::Villager,
            CharacterAffiliation::default(),
            CharacterAttributes::default(),
            Health {
                current: 0.0,
                ..default()
            },
            Wallet::new(20),
        ))
        .id();
    app.world_mut().spawn((
        survivor,
        CharacterName("Former worker".into()),
        CharacterKind::Villager,
        Health::default(),
    ));
    let company_id = CompanyId(1);
    let company = app
        .world_mut()
        .spawn((
            company_id,
            CompanyAccount {
                cash: 100,
                wage_arrears: 210,
                ..default()
            },
        ))
        .id();
    let operating = app
        .world_mut()
        .spawn((
            BuildingId(1),
            OperatedBy(company_id),
            BusinessAccount {
                wage_arrears: 130,
                ..default()
            },
            PrivatePayrollClaims {
                claims: vec![
                    BusinessWageClaim {
                        worker: deceased,
                        pennies: 90,
                    },
                    BusinessWageClaim {
                        worker: survivor,
                        pennies: 40,
                    },
                ],
            },
        ))
        .id();
    let liquidating = app
        .world_mut()
        .spawn((
            BuildingId(2),
            OperatedBy(company_id),
            BusinessAccount {
                wage_arrears: 80,
                ..default()
            },
            BusinessLiquidation::insolvency(
                0,
                vec![BusinessWageClaim {
                    worker: deceased,
                    pennies: 80,
                }],
            ),
        ))
        .id();
    (app, dead, household, company, operating, liquidating)
}

#[test]
fn death_settles_exact_former_worker_claims_from_both_stores_into_the_household() {
    let (mut app, dead, household, company, operating, liquidating) = fixture();
    app.update();
    assert!(app.world().get_entity(dead).is_err());
    assert_eq!(
        app.world()
            .get::<HouseholdEconomy>(household)
            .unwrap()
            .pennies,
        130
    );
    let treasury = app.world().get::<CompanyAccount>(company).unwrap();
    assert_eq!(treasury.cash, 0);
    assert_eq!(treasury.wage_arrears, 40);
    let claims = &app
        .world()
        .get::<PrivatePayrollClaims>(operating)
        .unwrap()
        .claims;
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].worker, PersonId(3));
    assert_eq!(claims[0].pennies, 40);
    assert_eq!(
        app.world()
            .get::<BusinessAccount>(operating)
            .unwrap()
            .wage_arrears,
        40
    );
    assert_eq!(
        app.world()
            .get::<BusinessAccount>(liquidating)
            .unwrap()
            .defaulted_wages,
        70
    );
    assert!(app
        .world()
        .get::<BusinessLiquidation>(liquidating)
        .unwrap()
        .wage_claims
        .is_empty());
    app.update();
    assert_eq!(
        app.world()
            .get::<HouseholdEconomy>(household)
            .unwrap()
            .pennies,
        130,
        "estate must settle once"
    );
}

#[test]
fn an_owner_death_preserves_other_liquidation_creditors_and_defaults_only_its_own_unfunded_claim() {
    let (mut app, _, household, company, _, liquidating) = fixture();
    app.world_mut()
        .entity_mut(company)
        .remove::<CompanyAccount>();
    app.world_mut()
        .get_mut::<BusinessAccount>(liquidating)
        .unwrap()
        .wage_arrears += 25;
    app.world_mut()
        .get_mut::<BusinessLiquidation>(liquidating)
        .unwrap()
        .wage_claims
        .push(BusinessWageClaim {
            worker: PersonId(3),
            pennies: 25,
        });
    app.world_mut().entity_mut(liquidating).insert((
        OwnedBy(PersonId(2)),
        SettlementBuilding {
            kind: SettlementBuildingKind::LumberjackHut,
            settlement: "Moot".into(),
            owner: Some("Former worker".into()),
            quality: 1.0,
            workers: Vec::new(),
        },
        BusinessCondition {
            state: BusinessState::Liquidating,
            ..default()
        },
    ));
    app.update();
    assert_eq!(
        app.world()
            .get::<HouseholdEconomy>(household)
            .unwrap()
            .pennies,
        30,
        "missing treasury cannot mint an estate payment"
    );
    let account = app.world().get::<BusinessAccount>(liquidating).unwrap();
    assert_eq!(account.defaulted_wages, 80);
    assert_eq!(account.wage_arrears, 25);
    let claims = &app
        .world()
        .get::<BusinessLiquidation>(liquidating)
        .unwrap()
        .wage_claims;
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].worker, PersonId(3));
    assert_eq!(claims[0].pennies, 25);
}
