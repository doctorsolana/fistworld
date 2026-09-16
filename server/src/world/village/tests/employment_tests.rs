//! Village employment regression fixtures and invariants.

use super::super::economy::{WageReview, review_business_wages};
use super::*;

#[test]
fn adaptive_wages_raise_for_vacancies_and_cut_under_payroll_stress() {
    let mut policy = BusinessWagePolicy::default();
    let mut review = WageReview {
        elapsed_days: 2,
        filled: 0,
        positions: 2,
        headroom: 4 * PENNIES_PER_COIN,
        arrears: 0,
        competing_offer: 100,
        living_cost: 120,
        sustainable_wage: 200,
        scarce_labour: true,
    };
    review_automatic_wage_offer(&mut policy, review);
    assert_eq!(
        policy.daily_wage,
        FOUNDING_DAILY_WAGE + shared::economy::BUSINESS_WAGE_REVIEW_STEP
    );

    review.filled = 2;
    review.headroom = 0;
    review.arrears = PENNIES_PER_COIN;
    review_automatic_wage_offer(&mut policy, review);
    assert_eq!(policy.daily_wage, FOUNDING_DAILY_WAGE);

    policy.automatic = false;
    review.elapsed_days = 100;
    review.filled = 0;
    review.headroom = 10_000;
    review.arrears = 0;
    review_automatic_wage_offer(&mut policy, review);
    assert_eq!(policy.daily_wage, FOUNDING_DAILY_WAGE);
}

#[test]
fn payroll_does_not_raise_wages_for_intentionally_disabled_positions() {
    let mut app = village_test_app();
    app.add_systems(
        Update,
        (
            run_business_payroll_and_owner_leisure,
            review_business_wages,
        )
            .chain(),
    );
    let mut clock = WorldTime::new_default();
    clock.day = 2;
    app.world_mut().spawn(clock);
    let settlement_id = shared::components::SettlementId(39);
    let settlement = app
        .world_mut()
        .spawn((
            settlement_id,
            Settlement {
                name: "One Shift".into(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            },
        ))
        .id();
    let company_id = shared::components::CompanyId(390);
    spawn_test_company(&mut app, company_id.0, 4 * PENNIES_PER_COIN);
    let business_id = shared::components::BuildingId(391);
    let business = app
        .world_mut()
        .spawn((
            business_id,
            shared::components::BuildingOf(settlement_id),
            shared::components::OperatedBy(company_id),
            SettlementBuilding {
                kind: SettlementBuildingKind::Farmstead,
                settlement: "One Shift".into(),
                owner: None,
                quality: 0.8,
                workers: vec!["Ada".into()],
            },
            BusinessAccount {
                last_payroll_day: 0,
                ..default()
            },
            BusinessWagePolicy::default(),
            BusinessStaffingPolicy::new(1),
        ))
        .id();
    app.world_mut().spawn((
        shared::components::PersonId(392),
        CharacterName("Ada".into()),
        VillagerIntent::Resident { settlement },
        shared::components::EmployedAt(business_id),
        Wallet::default(),
        Occupation(Some("Farmer".into())),
        WorkStatus::Employed,
    ));

    app.update();

    assert_eq!(
        app.world()
            .get::<BusinessWagePolicy>(business)
            .unwrap()
            .daily_wage,
        FOUNDING_DAILY_WAGE,
    );
}

#[test]
fn closing_positions_releases_stable_workers_only_at_a_safe_logistics_boundary() {
    let mut app = village_test_app();
    app.add_systems(Update, enforce_staffing_targets);
    let settlement_id = shared::components::SettlementId(850);
    let company = shared::components::CompanyId(851);
    let storage = shared::components::BuildingId(852);
    let storage_entity = app
        .world_mut()
        .spawn((
            storage,
            shared::components::BuildingOf(settlement_id),
            shared::components::OperatedBy(company),
            SettlementBuilding {
                kind: SettlementBuildingKind::StorageHall,
                settlement: "Storeford".into(),
                owner: Some("Ada".into()),
                quality: 0.5,
                workers: vec!["Ada".into(), "Bea".into()],
            },
            BusinessStaffingPolicy::new(1),
        ))
        .id();
    let first = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            shared::components::PersonId(853),
            shared::components::EmployedAt(storage),
            Occupation(Some("Company Porter".into())),
            WorkStatus::Employed,
            GoodsInventory::new(shared::economy::capacity::VILLAGER),
        ))
        .id();
    let second = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            shared::components::PersonId(854),
            shared::components::EmployedAt(storage),
            Occupation(Some("Company Porter".into())),
            WorkStatus::Employed,
            GoodsInventory::new(shared::economy::capacity::VILLAGER),
        ))
        .id();

    app.update();
    assert!(
        app.world()
            .get::<shared::components::EmployedAt>(first)
            .is_some()
    );
    assert!(
        app.world()
            .get::<shared::components::EmployedAt>(second)
            .is_none()
    );
    assert_eq!(
        *app.world().get::<WorkStatus>(second).unwrap(),
        WorkStatus::LookingForWork
    );

    app.world_mut()
        .get_mut::<BusinessStaffingPolicy>(storage_entity)
        .unwrap()
        .enabled_positions = 0;
    app.world_mut()
        .get_mut::<GoodsInventory>(first)
        .unwrap()
        .add(Good::Wheat, 1);
    app.update();
    assert!(
        app.world()
            .get::<shared::components::EmployedAt>(first)
            .is_some(),
        "a loaded porter must finish before the position closes"
    );

    app.world_mut()
        .get_mut::<GoodsInventory>(first)
        .unwrap()
        .remove(Good::Wheat, 1);
    app.update();
    assert!(
        app.world()
            .get::<shared::components::EmployedAt>(first)
            .is_none()
    );
}

#[test]
fn closing_a_production_job_preserves_personal_materials_but_waits_for_company_outputs() {
    use crate::world::village::worker_activity::EmploymentReleaseRequested;
    for (kind, cargo, should_release) in [
        (SettlementBuildingKind::StoneQuarry, Good::Wood, true),
        (SettlementBuildingKind::StoneQuarry, Good::Stone, false),
        (SettlementBuildingKind::LivestockFarm, Good::Wool, false),
        (SettlementBuildingKind::StorageHall, Good::Wood, false),
    ] {
        let mut app = village_test_app();
        app.add_systems(Update, enforce_staffing_targets);
        let id = shared::components::BuildingId(948);
        app.world_mut().spawn((
            id,
            SettlementBuilding {
                kind,
                settlement: "Releaseford".into(),
                owner: None,
                quality: 1.0,
                workers: vec![],
            },
            BusinessStaffingPolicy::new(0),
        ));
        let mut inventory = GoodsInventory::new(shared::economy::capacity::VILLAGER);
        inventory.add(cargo, 1);
        let worker = app
            .world_mut()
            .spawn((
                shared::components::PersonId(949),
                shared::components::EmployedAt(id),
                Occupation(Some("Worker".into())),
                WorkStatus::Employed,
                CharacterActivity::Idle,
                inventory,
                EmploymentReleaseRequested,
            ))
            .id();
        app.update();
        assert_eq!(
            app.world()
                .get::<shared::components::EmployedAt>(worker)
                .is_none(),
            should_release,
            "{kind:?} carrying {cargo:?}"
        );
        assert_eq!(
            app.world()
                .get::<EmploymentReleaseRequested>(worker)
                .is_none(),
            should_release
        );
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(worker)
                .unwrap()
                .amount(cargo),
            1,
            "Changing jobs must never discard retained goods"
        );
        if should_release {
            assert_eq!(
                *app.world().get::<WorkStatus>(worker).unwrap(),
                WorkStatus::LookingForWork
            );
            assert!(app.world().get::<Occupation>(worker).unwrap().0.is_none());
        }
    }
}

#[test]
fn a_closed_fishing_position_requests_egress_before_releasing_its_employee() {
    use crate::world::village::worker_activity::EmploymentReleaseRequested;
    let mut app = village_test_app();
    app.add_systems(Update, enforce_staffing_targets);
    let id = shared::components::BuildingId(950);
    let hut = app
        .world_mut()
        .spawn((
            id,
            SettlementBuilding {
                kind: SettlementBuildingKind::FishermansHut,
                settlement: "Pierford".into(),
                owner: None,
                quality: 1.0,
                workers: vec![],
            },
            BusinessStaffingPolicy::new(0),
        ))
        .id();
    let goal = Vec3::new(10.0, 0.0, 10.0);
    let person = app
        .world_mut()
        .spawn((
            shared::components::PersonId(951),
            shared::components::EmployedAt(id),
            Occupation(Some("Fisher".into())),
            WorkStatus::Employed,
            GoodsInventory::new(shared::economy::capacity::VILLAGER),
            CharacterActivity::Fishing,
            FishingRoutine {
                hut,
                pier: hut,
                hall: hut,
                catch_seconds: 40.0,
                failed_workplace_routes: 0,
                production_day: 0,
                produced_today: 0,
                phase: FishingPhase::Fishing,
            },
            PierTraversal {
                deck_start: Vec3::ZERO,
                deck_end: goal,
            },
            MoveTarget(goal),
        ))
        .id();
    app.update();
    assert!(
        app.world()
            .get::<EmploymentReleaseRequested>(person)
            .is_some()
    );
    assert!(
        app.world()
            .get::<shared::components::EmployedAt>(person)
            .is_some()
    );
    assert!(app.world().get::<PierTraversal>(person).is_some());
    assert_eq!(app.world().get::<MoveTarget>(person).unwrap().0, goal);
    assert_eq!(
        app.world()
            .get::<FishingRoutine>(person)
            .unwrap()
            .catch_seconds,
        40.0
    );

    // A revised operator decision cancels the pending release without
    // replacing the actor's physical work or discarding invested labour.
    app.world_mut()
        .get_mut::<BusinessStaffingPolicy>(hut)
        .unwrap()
        .enabled_positions = 1;
    app.update();
    assert!(
        app.world()
            .get::<EmploymentReleaseRequested>(person)
            .is_none()
    );
    assert!(app.world().get::<FishingRoutine>(person).is_some());

    app.world_mut()
        .get_mut::<BusinessStaffingPolicy>(hut)
        .unwrap()
        .enabled_positions = 0;
    app.update();
    // The job runner, whose real deck egress is tested separately, owns this
    // completion. Staffing may release employment only after it relinquishes
    // the routine and crossing, with cargo already delivered.
    app.world_mut()
        .entity_mut(person)
        .remove::<(FishingRoutine, PierTraversal, MoveTarget)>();
    app.update();
    assert!(
        app.world()
            .get::<shared::components::EmployedAt>(person)
            .is_none()
    );
    assert!(
        app.world()
            .get::<EmploymentReleaseRequested>(person)
            .is_none()
    );
    assert_eq!(
        *app.world().get::<CharacterActivity>(person).unwrap(),
        CharacterActivity::Idle
    );
}

#[test]
fn higher_wage_and_skill_requirements_shape_recruitment() {
    let mut app = village_test_app();
    app.add_systems(Update, fill_vacancies);
    let settlement = app
        .world_mut()
        .spawn(Settlement {
            name: "Skillford".to_string(),
            tier: shared::components::SettlementTier::Hamlet,
            residents: 2,
            treasury: 0,
        })
        .id();
    let specialist = app
        .world_mut()
        .spawn((
            SettlementBuilding {
                kind: SettlementBuildingKind::Farmstead,
                settlement: "Skillford".to_string(),
                owner: None,
                quality: 0.8,
                workers: Vec::new(),
            },
            PlayerPosition(Vec3::new(20.0, 0.0, 0.0)),
            BusinessWagePolicy {
                daily_wage: 2 * PENNIES_PER_COIN,
                automatic: false,
                ..default()
            },
            WorkforceRequirements {
                minimum_physique: 50,
                ..default()
            },
        ))
        .id();
    let ordinary = app
        .world_mut()
        .spawn((
            SettlementBuilding {
                kind: SettlementBuildingKind::LumberjackHut,
                settlement: "Skillford".to_string(),
                owner: None,
                quality: 0.8,
                workers: Vec::new(),
            },
            PlayerPosition(Vec3::new(10.0, 0.0, 0.0)),
            BusinessWagePolicy::default(),
        ))
        .id();
    for (name, physique, position) in [
        ("Untrained", 20, Vec3::new(19.0, 0.0, 0.0)),
        ("Skilled", 60, Vec3::new(0.0, 0.0, 0.0)),
    ] {
        app.world_mut().spawn((
            CharacterName(name.to_string()),
            VillagerIntent::Resident { settlement },
            PlayerPosition(position),
            Occupation::default(),
            WorkStatus::LookingForWork,
            CharacterAttributes::new(physique, 10, 10),
        ));
    }

    app.update();
    assert_eq!(
        app.world()
            .get::<SettlementBuilding>(specialist)
            .unwrap()
            .workers,
        ["Skilled"]
    );
    assert_eq!(
        app.world()
            .get::<SettlementBuilding>(ordinary)
            .unwrap()
            .workers,
        ["Untrained"]
    );
}

#[test]
fn unemployed_company_master_takes_own_vacancy_before_a_rival_offer() {
    let mut app = village_test_app();
    app.add_systems(Update, fill_vacancies);
    let settlement_id = shared::components::SettlementId(901);
    let settlement = app
        .world_mut()
        .spawn((
            settlement_id,
            Settlement {
                name: "Masterford".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 2,
                treasury: 0,
            },
        ))
        .id();
    let master = shared::components::PersonId(902);
    let helper = shared::components::PersonId(903);
    let company = shared::components::CompanyId(904);
    app.world_mut().spawn((
        company,
        shared::components::Company {
            name: "Ada & Company".to_string(),
            founded_day: 0,
        },
        shared::components::CompanyLeadership { master },
        shared::components::CompanyOwnership::sole(master),
    ));
    let own_building_id = shared::components::BuildingId(905);
    let own_business = app
        .world_mut()
        .spawn((
            own_building_id,
            shared::components::BuildingOf(settlement_id),
            shared::components::OperatedBy(company),
            SettlementBuilding {
                kind: SettlementBuildingKind::LumberjackHut,
                settlement: "Masterford".to_string(),
                owner: Some("Ada".to_string()),
                quality: 0.8,
                workers: Vec::new(),
            },
            PlayerPosition(Vec3::new(100.0, 0.0, 0.0)),
            BusinessWagePolicy {
                daily_wage: FOUNDING_DAILY_WAGE,
                automatic: false,
                ..default()
            },
        ))
        .id();
    let rival_building_id = shared::components::BuildingId(906);
    let rival_business = app
        .world_mut()
        .spawn((
            rival_building_id,
            shared::components::BuildingOf(settlement_id),
            SettlementBuilding {
                kind: SettlementBuildingKind::LumberjackHut,
                settlement: "Masterford".to_string(),
                owner: Some("Rival".to_string()),
                quality: 0.8,
                workers: Vec::new(),
            },
            PlayerPosition(Vec3::ZERO),
            BusinessWagePolicy {
                daily_wage: 2 * PENNIES_PER_COIN,
                automatic: false,
                ..default()
            },
        ))
        .id();
    let founder_entity = app
        .world_mut()
        .spawn((
            master,
            CharacterName("Ada".to_string()),
            VillagerIntent::Resident { settlement },
            PlayerPosition(Vec3::ZERO),
            Occupation::default(),
            WorkStatus::LookingForWork,
            CharacterAttributes::new(20, 20, 20),
        ))
        .id();
    let helper_entity = app
        .world_mut()
        .spawn((
            helper,
            CharacterName("Bea".to_string()),
            VillagerIntent::Resident { settlement },
            PlayerPosition(Vec3::new(100.0, 0.0, 0.0)),
            Occupation::default(),
            WorkStatus::LookingForWork,
            CharacterAttributes::new(20, 20, 20),
        ))
        .id();

    app.update();

    assert_eq!(
        app.world()
            .get::<shared::components::EmployedAt>(founder_entity),
        Some(&shared::components::EmployedAt(own_building_id)),
        "the Company Master should get first refusal on their own viable vacancy"
    );
    assert_eq!(
        app.world()
            .get::<shared::components::EmployedAt>(helper_entity),
        Some(&shared::components::EmployedAt(rival_building_id))
    );
    assert_eq!(
        app.world()
            .get::<SettlementBuilding>(own_business)
            .unwrap()
            .workers,
        ["Ada"]
    );
    assert_eq!(
        app.world()
            .get::<SettlementBuilding>(rival_business)
            .unwrap()
            .workers,
        ["Bea"]
    );
}

#[test]
fn payroll_catches_up_arrears_instead_of_stranding_them() {
    let mut app = village_test_app();
    app.add_systems(Update, run_business_payroll_and_owner_leisure);
    let mut clock = WorldTime::new_default();
    clock.day = 2;
    app.world_mut().spawn(clock);
    let settlement_id = shared::components::SettlementId(40);
    let settlement = app
        .world_mut()
        .spawn((
            settlement_id,
            Settlement {
                name: "Payford".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            },
        ))
        .id();
    let company_id = shared::components::CompanyId(41);
    let company = spawn_test_company(&mut app, company_id.0, 3 * PENNIES_PER_COIN);
    let business_id = shared::components::BuildingId(42);
    let business = app
        .world_mut()
        .spawn((
            business_id,
            shared::components::BuildingOf(settlement_id),
            shared::components::OperatedBy(company_id),
            SettlementBuilding {
                kind: SettlementBuildingKind::Farmstead,
                settlement: "Payford".to_string(),
                owner: None,
                quality: 0.8,
                workers: vec!["Ada".to_string()],
            },
            BusinessAccount {
                wage_arrears: 2 * PENNIES_PER_COIN,
                last_payroll_day: 1,
                ..default()
            },
            super::super::commerce::payroll_claims::PrivatePayrollClaims {
                claims: vec![shared::economy::BusinessWageClaim {
                    worker: shared::components::PersonId(43),
                    pennies: 2 * PENNIES_PER_COIN,
                }],
            },
            BusinessWagePolicy::default(),
        ))
        .id();
    let worker = app
        .world_mut()
        .spawn((
            shared::components::PersonId(43),
            CharacterName("Ada".to_string()),
            VillagerIntent::Resident { settlement },
            shared::components::EmployedAt(business_id),
            Wallet::default(),
            Occupation(Some("Farmer".to_string())),
            WorkStatus::Employed,
        ))
        .id();

    app.update();
    assert_eq!(app.world().get::<Wallet>(worker).unwrap().balance(), 300);
    let account = app.world().get::<BusinessAccount>(business).unwrap();
    assert_eq!(app.world().get::<CompanyAccount>(company).unwrap().cash, 0);
    assert_eq!(account.wage_arrears, 0);
    assert_eq!(account.current_day.day, 1);
    assert_eq!(account.current_day.wage_expense, PENNIES_PER_COIN);
}

#[test]
fn a_financially_secure_owner_delegates_only_when_payroll_and_a_replacement_are_ready() {
    let mut app = village_test_app();
    app.add_systems(
        Update,
        (run_business_payroll_and_owner_leisure, fill_vacancies).chain(),
    );
    let clock = app
        .world_mut()
        .spawn({
            let mut clock = WorldTime::new_default();
            clock.day = 2;
            clock
        })
        .id();
    let settlement_id = shared::components::SettlementId(5_100);
    let settlement = app
        .world_mut()
        .spawn((
            settlement_id,
            Settlement {
                name: "Richford".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 2,
                treasury: 0,
            },
        ))
        .id();
    let company_id = shared::components::CompanyId(5_101);
    let company = spawn_test_company(&mut app, company_id.0, 20 * PENNIES_PER_COIN);
    let business_id = shared::components::BuildingId(5_102);
    let business = app
        .world_mut()
        .spawn((
            business_id,
            shared::components::BuildingOf(settlement_id),
            shared::components::OperatedBy(company_id),
            shared::components::OwnedBy(shared::components::PersonId(5_103)),
            SettlementBuilding {
                kind: SettlementBuildingKind::LumberjackHut,
                settlement: "Richford".to_string(),
                owner: Some("Ada".to_string()),
                quality: 0.8,
                workers: vec!["Ada".to_string()],
            },
            PlayerPosition(Vec3::new(10.0, 0.0, 0.0)),
            BusinessAccount {
                wage_arrears: 0,
                last_payroll_day: 1,
                ..default()
            },
            BusinessWagePolicy::default(),
        ))
        .id();
    let owner = app
        .world_mut()
        .spawn((
            shared::components::PersonId(5_103),
            CharacterName("Ada".to_string()),
            VillagerIntent::Resident { settlement },
            shared::components::EmployedAt(business_id),
            PlayerPosition(Vec3::new(10.0, 0.0, 0.0)),
            Wallet::new(40 * PENNIES_PER_COIN),
            Occupation(Some("Woodcutter".to_string())),
            WorkStatus::Employed,
        ))
        .id();
    let replacement = app
        .world_mut()
        .spawn((
            shared::components::PersonId(5_104),
            CharacterName("Bea".to_string()),
            VillagerIntent::ArrivingBySea { settlement },
            PlayerPosition(Vec3::ZERO),
            Wallet::default(),
            Occupation::default(),
            WorkStatus::LookingForWork,
        ))
        .id();

    for intent in [
        VillagerIntent::ArrivingBySea { settlement },
        VillagerIntent::Travelling { settlement },
    ] {
        app.world_mut().entity_mut(replacement).insert(intent);
        app.update();
        assert_eq!(
            app.world().get::<WorkStatus>(owner),
            Some(&WorkStatus::Employed),
            "an incoming newcomer cannot make an owner delegate before registration"
        );
        assert_eq!(
            app.world().get::<shared::components::EmployedAt>(owner),
            Some(&shared::components::EmployedAt(business_id))
        );
        assert!(
            app.world()
                .get::<shared::components::EmployedAt>(replacement)
                .is_none()
        );
        assert!(
            app.world()
                .get::<shared::components::ResidentOf>(replacement)
                .is_none()
        );
        assert_eq!(
            app.world()
                .get::<SettlementBuilding>(business)
                .unwrap()
                .workers,
            ["Ada"]
        );
        app.world_mut().get_mut::<WorldTime>(clock).unwrap().day += 1;
    }
    // Stage only the completed-registration fact here; the immigration test
    // separately exercises its real navigation, FIFO service and counter exit.
    // The next ordinary daily decision must now delegate and actually hire.
    app.world_mut()
        .entity_mut(replacement)
        .insert(VillagerIntent::Resident { settlement });
    app.update();

    let world = app.world();
    let building = world.get::<SettlementBuilding>(business).unwrap();
    assert_eq!(building.workers, ["Bea"]);
    assert_eq!(
        *world.get::<WorkStatus>(owner).unwrap(),
        WorkStatus::Chilling
    );
    assert_eq!(world.get::<Occupation>(owner).unwrap().0, None);
    assert_eq!(
        *world.get::<WorkStatus>(replacement).unwrap(),
        WorkStatus::Employed
    );
    assert_eq!(
        world.get::<Occupation>(replacement).unwrap().0.as_deref(),
        Some("Woodcutter")
    );
    let account = world.get::<CompanyAccount>(company).unwrap();
    assert!(
        account.cash >= 2 * FOUNDING_DAILY_WAGE,
        "the owner must leave a real payroll reserve behind"
    );
}

#[test]
fn an_owner_worker_receives_the_same_wage_as_every_other_employee() {
    let mut app = village_test_app();
    app.add_systems(Update, run_business_payroll_and_owner_leisure);
    let mut clock = WorldTime::new_default();
    clock.day = 2;
    app.world_mut().spawn(clock);
    let settlement_id = shared::components::SettlementId(5_110);
    let settlement = app
        .world_mut()
        .spawn((
            settlement_id,
            Settlement {
                name: "Wageford".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            },
        ))
        .id();
    let owner_id = shared::components::PersonId(5_111);
    let company_id = shared::components::CompanyId(5_112);
    let company = spawn_test_company(&mut app, company_id.0, FOUNDING_DAILY_WAGE);
    app.world_mut().entity_mut(company).insert((
        shared::components::CompanyLeadership { master: owner_id },
        shared::components::CompanyOwnership::sole(owner_id),
    ));
    let business_id = shared::components::BuildingId(5_113);
    let business = app
        .world_mut()
        .spawn((
            business_id,
            shared::components::BuildingOf(settlement_id),
            shared::components::OperatedBy(company_id),
            shared::components::OwnedBy(owner_id),
            SettlementBuilding {
                kind: SettlementBuildingKind::Farmstead,
                settlement: "Wageford".to_string(),
                owner: Some("Ada".to_string()),
                quality: 1.0,
                workers: vec!["Ada".to_string()],
            },
            BusinessAccount {
                last_payroll_day: 1,
                ..default()
            },
            BusinessWagePolicy::default(),
            BusinessStaffingPolicy::new(1),
        ))
        .id();
    let owner = app
        .world_mut()
        .spawn((
            owner_id,
            CharacterName("Ada".to_string()),
            VillagerIntent::Resident { settlement },
            shared::components::EmployedAt(business_id),
            Wallet::default(),
            Occupation(Some("Farmer".to_string())),
            WorkStatus::Employed,
        ))
        .id();

    app.update();

    assert_eq!(
        app.world().get::<Wallet>(owner).unwrap().balance(),
        FOUNDING_DAILY_WAGE,
        "share ownership must not turn a real shift into unpaid labour",
    );
    assert_eq!(app.world().get::<CompanyAccount>(company).unwrap().cash, 0);
    assert_eq!(
        app.world()
            .get::<BusinessAccount>(business)
            .unwrap()
            .current_day
            .wage_expense,
        FOUNDING_DAILY_WAGE,
    );
}

#[test]
fn a_resting_master_returns_when_their_company_cannot_find_a_worker() {
    let mut app = village_test_app();
    app.add_systems(
        Update,
        (run_business_payroll_and_owner_leisure, fill_vacancies).chain(),
    );
    let mut clock = WorldTime::new_default();
    clock.day = 2;
    app.world_mut().spawn(clock);
    let settlement_id = shared::components::SettlementId(5_120);
    let settlement = app
        .world_mut()
        .spawn((
            settlement_id,
            Settlement {
                name: "Needford".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            },
        ))
        .id();
    let owner_id = shared::components::PersonId(5_121);
    let company_id = shared::components::CompanyId(5_122);
    let company = spawn_test_company(&mut app, company_id.0, 10 * PENNIES_PER_COIN);
    app.world_mut().entity_mut(company).insert((
        shared::components::CompanyLeadership { master: owner_id },
        shared::components::CompanyOwnership::sole(owner_id),
    ));
    let business_id = shared::components::BuildingId(5_123);
    let business = app
        .world_mut()
        .spawn((
            business_id,
            shared::components::BuildingOf(settlement_id),
            shared::components::OperatedBy(company_id),
            shared::components::OwnedBy(owner_id),
            SettlementBuilding {
                kind: SettlementBuildingKind::Bakery,
                settlement: "Needford".to_string(),
                owner: Some("Ada".to_string()),
                quality: 1.0,
                workers: Vec::new(),
            },
            PlayerPosition(Vec3::ZERO),
            BusinessAccount {
                last_payroll_day: 2,
                ..default()
            },
            BusinessWagePolicy::default(),
            BusinessStaffingPolicy::new(1),
            BusinessCondition::default(),
        ))
        .id();
    let owner = app
        .world_mut()
        .spawn((
            owner_id,
            CharacterName("Ada".to_string()),
            VillagerIntent::Resident { settlement },
            PlayerPosition(Vec3::ZERO),
            Wallet::new(100 * PENNIES_PER_COIN),
            Occupation::default(),
            WorkStatus::Chilling,
        ))
        .id();

    app.update();

    assert_eq!(
        app.world().get::<shared::components::EmployedAt>(owner),
        Some(&shared::components::EmployedAt(business_id)),
    );
    assert_eq!(
        *app.world().get::<WorkStatus>(owner).unwrap(),
        WorkStatus::Employed
    );
    assert_eq!(
        app.world()
            .get::<SettlementBuilding>(business)
            .unwrap()
            .workers,
        ["Ada"],
        "a secure owner should still cover a needed position when no replacement exists",
    );
}

#[test]
fn retention_wages_need_real_profit_and_company_headroom() {
    let base = WageReview {
        elapsed_days: 1,
        filled: 1,
        positions: 1,
        headroom: 1_000,
        arrears: 0,
        competing_offer: 130,
        living_cost: 150,
        sustainable_wage: 180,
        scarce_labour: true,
    };
    let mut policy = BusinessWagePolicy::default();
    review_automatic_wage_offer(&mut policy, base);
    assert_eq!(policy.daily_wage, 110);
    for review in [
        WageReview {
            headroom: 0,
            ..base
        },
        WageReview {
            sustainable_wage: 100,
            ..base
        },
        WageReview { arrears: 1, ..base },
        WageReview {
            scarce_labour: false,
            ..base
        },
    ] {
        let mut policy = BusinessWagePolicy::default();
        review_automatic_wage_offer(&mut policy, review);
        assert_eq!(policy.daily_wage, 100);
    }
    let mut policy = BusinessWagePolicy::default();
    review_automatic_wage_offer(
        &mut policy,
        WageReview {
            filled: 4,
            positions: 1,
            headroom: 40,
            ..base
        },
    );
    assert_eq!(
        policy.daily_wage, 105,
        "all remaining employees receive the raise"
    );
}

#[test]
fn wages_reserve_sibling_payroll_before_advertising_raises() {
    let mut app = village_test_app();
    app.add_systems(Update, review_business_wages);
    let mut clock = WorldTime::new_default();
    clock.day = 2;
    app.world_mut().spawn(clock);
    let sid = shared::components::SettlementId(9_001);
    app.world_mut().spawn((sid, MootMarket::founding()));
    let company = shared::components::CompanyId(9_002);
    spawn_test_company(&mut app, company.0, 420);
    let mut sites = Vec::new();
    for number in 0..2 {
        sites.push(
            app.world_mut()
                .spawn((
                    shared::components::BuildingId(9_010 + number),
                    shared::components::BuildingOf(sid),
                    shared::components::OperatedBy(company),
                    SettlementBuilding {
                        kind: SettlementBuildingKind::LumberjackHut,
                        settlement: "Wage test".into(),
                        owner: None,
                        quality: 1.0,
                        workers: vec![],
                    },
                    BusinessAccount::default(),
                    BusinessWagePolicy {
                        vacancy_days: 1,
                        ..default()
                    },
                    BusinessStaffingPolicy::new(1),
                    BusinessStaffingForecast {
                        day: 2,
                        expected_sales_units: 3,
                        produced_output_units: 0,
                        optimal_positions: 1,
                        marginal_daily_profit: 100,
                    },
                ))
                .id(),
        );
    }
    app.update();
    let wages: Vec<_> = sites
        .iter()
        .map(|e| {
            app.world()
                .get::<BusinessWagePolicy>(*e)
                .unwrap()
                .daily_wage
        })
        .collect();
    assert_eq!(
        wages,
        vec![110, 100],
        "twenty spare pennies cannot back two two-day raises"
    );
    assert!(wages.iter().sum::<u64>() * 2 <= 420);
    app.update();
    assert_eq!(
        app.world()
            .get::<BusinessWagePolicy>(sites[0])
            .unwrap()
            .daily_wage,
        110,
        "same-day frames cannot repeatedly raise salaries"
    );
}

#[test]
fn busy_worker_reconsiders_better_job_after_delivering_same_day() {
    let mut app = village_test_app();
    app.add_systems(Update, review_worker_job_choices);
    let clock = app.world_mut().spawn(WorldTime::new_default()).id();
    let sid = shared::components::SettlementId(9_101);
    for (number, wage) in [(1, 100), (2, 110)] {
        app.world_mut().spawn((
            shared::components::BuildingId(number),
            shared::components::BuildingOf(sid),
            SettlementBuilding {
                kind: SettlementBuildingKind::LumberjackHut,
                settlement: "Switch test".into(),
                owner: None,
                quality: 1.0,
                workers: vec![],
            },
            BusinessWagePolicy {
                daily_wage: wage,
                ..default()
            },
            BusinessStaffingPolicy::new(1),
            BusinessAccount::default(),
        ));
    }
    let mut goods = GoodsInventory::new(shared::economy::capacity::VILLAGER);
    goods.add(Good::Wood, 1);
    let worker = app
        .world_mut()
        .spawn((
            shared::components::PersonId(9_102),
            shared::components::EmployedAt(shared::components::BuildingId(1)),
            Occupation(Some("Lumberjack".into())),
            WorkStatus::Employed,
            goods,
        ))
        .id();
    app.update();
    assert_eq!(
        app.world()
            .get::<shared::components::EmployedAt>(worker)
            .unwrap()
            .0
            .0,
        1
    );
    app.world_mut()
        .get_mut::<GoodsInventory>(worker)
        .unwrap()
        .remove(Good::Wood, 1);
    let mut time = app.world_mut().get_mut::<WorldTime>(clock).unwrap();
    time.seconds_in_cycle += time.cycle_duration() / 12.0;
    drop(time);
    app.update();
    assert_eq!(
        app.world()
            .get::<shared::components::EmployedAt>(worker)
            .unwrap()
            .0
            .0,
        2
    );
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(worker)
            .unwrap()
            .amount(Good::Wood),
        0
    );
}
