use super::*;
use shared::components::CompanyOwnership;
use shared::economy::{
    CompanyAccount, CompanyManagementPolicy, VILLAGE_MIN_PROSPERITY, VILLAGE_MIN_RESIDENTS,
    VILLAGE_REQUIRED_SECURE_DAYS,
};

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

fn village_test_app() -> App {
    let mut app = App::new();
    app.init_resource::<crate::world::identity::WorldIdAllocator>()
        .init_resource::<crate::world::identity::WorldIdentityIndex>()
        .init_resource::<BusinessEventQueue>()
        .add_systems(
            PreUpdate,
            (
                crate::world::identity::assign_stable_world_ids,
                crate::world::identity::rebuild_world_identity_index,
                crate::world::identity::reconcile_stable_world_relationships,
                crate::world::identity::reconcile_stable_adjunct_relationships,
                crate::world::identity::reconcile_stable_road_relationships,
                crate::world::identity::reconcile_stable_civic_employment,
            )
                .chain(),
        );
    app
}

fn spawn_test_company(app: &mut App, id: u64, cash: u64) -> Entity {
    app.world_mut()
        .spawn((
            shared::components::CompanyId(id),
            shared::economy::CompanyAccount { cash, ..default() },
        ))
        .id()
}

#[test]
fn player_worksite_is_not_adopted_by_the_village_crew() {
    let mut app = village_test_app();
    app.add_systems(Update, recover_orphaned_construction);
    let settlement = app.world_mut().spawn_empty().id();
    let resident = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            shared::components::Health::new(100.0),
            VillagerIntent::Resident { settlement },
            Occupation::default(),
            WorkStatus::LookingForWork,
        ))
        .id();
    let owner = shared::components::PersonId(77);
    let kind = SettlementBuildingKind::Windmill;
    let site = app
        .world_mut()
        .spawn((
            UnderConstruction {
                kind,
                position: Vec3::ZERO,
                rotation: 0.0,
                owner: Some("Player".into()),
                owner_id: Some(owner),
                builder: None,
                settlement,
                settlement_id: shared::components::SettlementId(4),
                stand: Vec3::Z,
                failed_stand_routes: 0,
                stage: BuildStage::Supplying,
                quality: 1.0,
            },
            crate::player::permits::PlayerConstructionProject { owner },
        ))
        .id();

    app.update();

    assert_eq!(
        app.world().get::<UnderConstruction>(site).unwrap().builder,
        None
    );
    assert!(app
        .world()
        .get::<ConstructionMaterialRoutine>(resident)
        .is_none());
    assert!(matches!(
        app.world().get::<VillagerIntent>(resident),
        Some(VillagerIntent::Resident { .. })
    ));
}

#[test]
fn player_assignment_advances_the_physical_supply_loop_at_night_without_villager_intent() {
    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.insert_resource(WorldTerrain::default());
    app.add_systems(Update, run_construction_material_logistics);
    app.world_mut().spawn(WorldTime::new(
        WorldTime::DEFAULT_DAY_DURATION,
        WorldTime::DEFAULT_NIGHT_DURATION,
        WorldTime::DEFAULT_DAY_DURATION + 30.0,
    ));

    let hall_position = Vec3::new(1720.0, 6.0, 0.0);
    let settlement_id = shared::components::SettlementId(88);
    let settlement = app
        .world_mut()
        .spawn((
            settlement_id,
            Settlement {
                name: "Playerbuild".into(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 0,
                treasury: 0,
            },
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
            GoodsInventory::new(shared::economy::capacity::HALL),
        ))
        .id();
    let owner = shared::components::PersonId(900);
    let hero = app
        .world_mut()
        .spawn((
            owner,
            CharacterName("Player".into()),
            CharacterKind::Hero,
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
            CharacterActivity::Idle,
            GoodsInventory::new(shared::economy::capacity::VILLAGER),
            Wallet::default(),
        ))
        .id();
    let kind = SettlementBuildingKind::Windmill;
    let position = hall_position + Vec3::X * 25.0;
    let stand = shared::components::builder_stand_position(
        position,
        0.0,
        kind.art().definition().footprint.y,
    );
    let site = app
        .world_mut()
        .spawn((
            UnderConstruction {
                kind,
                position,
                rotation: 0.0,
                owner: Some("Player".into()),
                owner_id: Some(owner),
                builder: Some(hero),
                settlement,
                settlement_id,
                stand,
                failed_stand_routes: 0,
                stage: BuildStage::Supplying,
                quality: 1.0,
            },
            shared::components::ConstructionSite {
                kind,
                settlement: "Playerbuild".into(),
                raising: false,
                stand,
                rotation: 0.0,
            },
            GoodsInventory::new(kind.construction_storage_bulk()),
            PlayerPosition(position),
            crate::player::permits::PlayerConstructionProject { owner },
        ))
        .id();
    app.world_mut().entity_mut(hero).insert((
        PlayerConstructionAssignment { site, settlement },
        ConstructionMaterialRoutine {
            site,
            cycle: 0,
            failed_tree_routes: 0,
            failed_store_routes: 0,
            failed_delivery_routes: 0,
            tree_retry_after: 0.0,
            store_retry_after: 0.0,
            phase: ConstructionMaterialPhase::Chopping {
                tree: hall_position + Vec3::X,
                seconds_left: 10.0,
            },
        },
    ));

    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs(1));
    app.update();

    assert!(app
        .world()
        .entity(hero)
        .contains::<ConstructionMaterialRoutine>());
    assert!(app
        .world()
        .entity(hero)
        .contains::<PlayerConstructionAssignment>());
    assert_eq!(
        app.world().get::<UnderConstruction>(site).unwrap().builder,
        Some(hero)
    );
    assert!(app.world().get::<VillagerIntent>(hero).is_none());
    let remaining = match app
        .world()
        .get::<ConstructionMaterialRoutine>(hero)
        .unwrap()
        .phase
    {
        ConstructionMaterialPhase::Chopping { seconds_left, .. } => seconds_left,
        ref phase => panic!("expected commanded Hero to keep chopping, got {phase:?}"),
    };
    assert!(
        (remaining - 9.0).abs() < 0.01,
        "night work did not consume world time: {remaining:.3}s remained"
    );
}

#[test]
fn completed_player_building_releases_hero_at_night_and_queues_civic_road_work() {
    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.init_resource::<PublishedTerrainDeltas>();
    app.insert_resource(WorldTerrain::default());
    app.add_systems(Update, advance_construction);
    app.world_mut().spawn(WorldTime::new(
        WorldTime::DEFAULT_DAY_DURATION,
        WorldTime::DEFAULT_NIGHT_DURATION,
        WorldTime::DEFAULT_DAY_DURATION + 30.0,
    ));

    let settlement_id = shared::components::SettlementId(89);
    let settlement = app
        .world_mut()
        .spawn((
            settlement_id,
            Settlement {
                name: "Playerbuild".into(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 0,
                treasury: 0,
            },
        ))
        .id();
    let owner = shared::components::PersonId(901);
    let position = Vec3::new(40.0, 5.0, 10.0);
    let hero = app
        .world_mut()
        .spawn((
            PlayerPosition(position),
            PlayerRotation(0.0),
            CharacterActivity::Building,
        ))
        .id();
    let kind = SettlementBuildingKind::Windmill;
    let site = app
        .world_mut()
        .spawn((
            UnderConstruction {
                kind,
                position,
                rotation: 0.0,
                owner: Some("Player".into()),
                owner_id: Some(owner),
                builder: Some(hero),
                settlement,
                settlement_id,
                stand: position,
                failed_stand_routes: 0,
                stage: BuildStage::Raising { seconds_left: 0.0 },
                quality: 1.0,
            },
            shared::components::ConstructionSite {
                kind,
                settlement: "Playerbuild".into(),
                raising: true,
                stand: position,
                rotation: 0.0,
            },
            GoodsInventory::new(kind.construction_storage_bulk()),
            crate::player::permits::PlayerConstructionProject { owner },
        ))
        .id();
    app.world_mut().entity_mut(hero).insert((
        PlayerConstructionAssignment { site, settlement },
        ConstructionMaterialRoutine::new(site),
    ));

    app.update();

    assert!(app.world().get_entity(site).is_err());
    assert!(app
        .world()
        .get::<PlayerConstructionAssignment>(hero)
        .is_none());
    assert_eq!(
        app.world().get::<CharacterActivity>(hero),
        Some(&CharacterActivity::Idle)
    );
    let completed = app
        .world_mut()
        .query_filtered::<Entity, With<SettlementBuilding>>()
        .single(app.world())
        .expect("player Windmill completed");
    assert!(app
        .world()
        .entity(completed)
        .contains::<crate::world::village_roads::RoadRepairBacklog>());
    assert!(!app
        .world()
        .entity(completed)
        .contains::<crate::world::village_roads::RoadRequest>());
}

#[test]
fn field_quality_controls_continuous_wheat_rate() {
    assert!((farmer_seconds_per_wheat(1.0) - 170.0).abs() < 0.01);
    assert!((farmer_seconds_per_wheat(2.0 / 3.0) - 255.0).abs() < 0.01);
    assert!((farmer_seconds_per_wheat(0.5) - 340.0).abs() < 0.01);
    assert!(farmer_seconds_per_wheat(0.1) > farmer_seconds_per_wheat(0.5));

    let ordinary_shift_seconds = WorldTime::DEFAULT_DAY_DURATION * WORKDAY_END_DAY_T
        - WorldTime::DEFAULT_START_SECONDS_IN_DAY;
    assert!((ordinary_shift_seconds / farmer_seconds_per_wheat(1.0) - 6.0).abs() < 0.2);
    assert!((ordinary_shift_seconds / farmer_seconds_per_wheat(2.0 / 3.0) - 4.0).abs() < 0.2);
}

#[test]
fn planned_hall_access_stays_outside_the_hall_until_its_door() {
    let terrain = WorldTerrain::default();
    let hall = Vec3::new(1_700.0, 0.0, 0.0);
    let house = Vec3::new(1_740.0, 0.0, 0.0);
    let path = planned_road_access_path(
        &terrain,
        hall,
        SettlementBuildingKind::House,
        house,
        0.0,
        &[],
        &[],
        &HashSet::new(),
    )
    .expect("the open plot should have a hall access path");

    let hall_door = SettlementBuildingKind::Hall.entrance_position(hall, 0.0);
    assert!(
        path.last()
            .is_some_and(|point| point.distance(Vec2::new(hall_door.x, hall_door.z)) < 0.01),
        "the path must still meet the authored hall door: {path:?}"
    );
    let hall_half = SettlementBuildingKind::Hall.art().definition().footprint * 0.5;
    assert!(
        path.iter().all(|point| {
            let local = *point - Vec2::new(hall.x, hall.z);
            local.x.abs() > hall_half.x || local.y.abs() > hall_half.y
        }),
        "the permit route entered the physical hall footprint before its door: {path:?}"
    );
}

#[test]
fn adaptive_wages_raise_for_vacancies_and_cut_under_payroll_stress() {
    let mut policy = BusinessWagePolicy::default();
    review_automatic_wage_offer(&mut policy, 2, 0, 2, 4 * PENNIES_PER_COIN, 0);
    assert_eq!(
        policy.daily_wage,
        FOUNDING_DAILY_WAGE + shared::economy::BUSINESS_WAGE_REVIEW_STEP
    );

    review_automatic_wage_offer(&mut policy, 2, 2, 2, 0, PENNIES_PER_COIN);
    assert_eq!(policy.daily_wage, FOUNDING_DAILY_WAGE);

    policy.automatic = false;
    review_automatic_wage_offer(&mut policy, 100, 0, 2, 10_000, 0);
    assert_eq!(policy.daily_wage, FOUNDING_DAILY_WAGE);
}

#[test]
fn payroll_does_not_raise_wages_for_intentionally_disabled_positions() {
    let mut app = village_test_app();
    app.add_systems(Update, run_business_payroll_and_owner_leisure);
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
    assert!(app
        .world()
        .get::<shared::components::EmployedAt>(first)
        .is_some());
    assert!(app
        .world()
        .get::<shared::components::EmployedAt>(second)
        .is_none());
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
    assert!(app
        .world()
        .get::<shared::components::EmployedAt>(first)
        .is_none());
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
fn later_buildings_do_not_overwrite_completed_village_paths() {
    let terrain = WorldTerrain::default();
    let hall = Vec3::new(1720.0, terrain.get_height(1720.0, 0.0), 0.0);
    let kind = SettlementBuildingKind::House;
    let (first, _) = find_site(&terrain, hall, kind, &[], &[]).unwrap();
    let road = VillageRoad {
        settlement: "Oakmead".into(),
        builder: "Mara".into(),
        points: vec![
            Vec2::new(first.x - 12.0, first.z),
            Vec2::new(first.x + 12.0, first.z),
        ],
        built_through: 2,
        width: crate::world::village_roads::VILLAGE_ROAD_WIDTH,
        reserved_width: shared::components::RoadClass::Lane.initial_reserved_width(),
        surface: default(),
        class: default(),
        stone_committed: 0,
    };

    let (replacement, _) = find_site(&terrain, hall, kind, &[], &[&road]).unwrap();
    let footprint_radius = kind.art().definition().footprint.length() * 0.5 + 0.45;
    assert!(first.distance_squared(replacement) > 1.0);
    assert!(
        !road.contains_reserved_point(Vec2::new(replacement.x, replacement.z), footprint_radius,)
    );
}

#[test]
fn seeded_layouts_face_streets_and_produce_distinct_first_plots() {
    use shared::components::{SettlementCenterStyle, SettlementDevelopment, SettlementLayoutStyle};

    let terrain = WorldTerrain::default();
    let hall = Vec3::new(1720.0, terrain.get_height(1720.0, 0.0), 0.0);
    let kind = SettlementBuildingKind::House;
    let styles = [
        SettlementLayoutStyle::Organic,
        SettlementLayoutStyle::Radial,
        SettlementLayoutStyle::Grid,
        SettlementLayoutStyle::Avenue,
        SettlementLayoutStyle::Polycentric,
    ];
    let mut first_plots = Vec::new();

    for style in styles {
        let mut plan = SettlementDevelopment::from_foundation("Planford", hall, 0);
        plan.layout = style;
        plan.center = SettlementCenterStyle::Green;
        let mut occupied = vec![(hall, SettlementBuildingKind::Hall.clearance())];
        let mut hall_facing = 0;

        for index in 0..4 {
            let (plot, rotation) = find_site_with_plan(
                &terrain,
                hall,
                kind,
                &occupied,
                &[],
                &[],
                &[],
                Some(&plan),
                None,
                None,
                None,
                None,
            )
            .unwrap_or_else(|| panic!("{style:?} must find plot {index}"));
            if index == 0 {
                first_plots.push(Vec2::new(plot.x, plot.z));
            }
            let door = kind.entrance_position(plot, rotation);
            let door_direction = Vec2::new(door.x - plot.x, door.z - plot.z).normalize();
            let hall_direction = Vec2::new(hall.x - plot.x, hall.z - plot.z).normalize();
            if door_direction.dot(hall_direction) > 0.985 {
                hall_facing += 1;
            }
            occupied.push((plot, kind.clearance()));
        }

        assert!(
            hall_facing < 4,
            "{style:?} must use street frontage instead of making every door face the hall"
        );
    }

    let mut distinct = 0;
    for (index, plot) in first_plots.iter().enumerate() {
        if first_plots[..index]
            .iter()
            .all(|other| other.distance_squared(*plot) > 4.0)
        {
            distinct += 1;
        }
    }
    assert!(
        distinct >= 4,
        "the five layout grammars must not collapse to the same first plot: {first_plots:?}"
    );
}

#[test]
fn unseeded_fallback_points_the_authored_door_toward_the_hall() {
    let terrain = WorldTerrain::default();
    let hall = Vec3::new(1720.0, terrain.get_height(1720.0, 0.0), 0.0);
    let kind = SettlementBuildingKind::House;
    let (plot, rotation) = find_site(&terrain, hall, kind, &[], &[]).unwrap();
    let door = kind.entrance_position(plot, rotation);
    let door_direction = Vec2::new(door.x - plot.x, door.z - plot.z).normalize();
    let hall_direction = Vec2::new(hall.x - plot.x, hall.z - plot.z).normalize();
    assert!(
        door_direction.dot(hall_direction) > 0.999,
        "the old sign pointed the authored -Z door away from the Moot Hall"
    );
}

#[test]
fn farmstead_siting_reserves_its_future_wheat_field_from_roads() {
    let terrain = WorldTerrain::default();
    let hall = Vec3::new(1720.0, terrain.get_height(1720.0, 0.0), 0.0);
    let kind = SettlementBuildingKind::Farmstead;
    let (first, first_rotation) = find_site(&terrain, hall, kind, &[], &[]).unwrap();
    let first_field = kind.field_position(first, first_rotation).unwrap();
    let road = VillageRoad {
        settlement: "Oakmead".into(),
        builder: "Mara".into(),
        points: vec![
            Vec2::new(first_field.x - 12.0, first_field.z),
            Vec2::new(first_field.x + 12.0, first_field.z),
        ],
        // Even an unbuilt plan is committed ground and must be reserved.
        built_through: 1,
        width: crate::world::village_roads::VILLAGE_ROAD_WIDTH,
        reserved_width: shared::components::RoadClass::Lane.initial_reserved_width(),
        surface: default(),
        class: default(),
        stone_committed: 0,
    };

    let (replacement, replacement_rotation) =
        find_site(&terrain, hall, kind, &[], &[&road]).unwrap();
    assert!(first.distance_squared(replacement) > 1.0);
    for replacement_field in kind
        .field_positions(replacement, replacement_rotation)
        .unwrap()
    {
        assert!(!road.intersects_rotated_rect(
            Vec2::new(replacement_field.x, replacement_field.z),
            kind.field_half_extents().unwrap(),
            replacement_rotation,
            shared::components::FARM_FIELD_EDGE_CLEARANCE,
        ));
    }
}

#[test]
fn an_inland_river_cannot_be_mistaken_for_a_dry_building_plot() {
    let terrain = WorldTerrain::default();
    let ocean = terrain.water_level().expect("generated world has water");
    let river_point = terrain
        .rivers()
        .iter()
        .flatten()
        .find(|point| {
            terrain
                .water_surface_height(point.x, point.z)
                .is_some_and(|surface| {
                    surface > ocean + 0.2 && terrain.get_height(point.x, point.z) < surface
                })
        })
        .expect("generated world has an inland river");
    let centre = Vec3::new(
        river_point.x,
        terrain.get_height(river_point.x, river_point.z),
        river_point.z,
    );

    assert!(
        shared::components::minimum_building_water_clearance(
            &terrain,
            centre,
            SettlementBuildingKind::Farmstead,
            0.0,
        ) < FREEBOARD
    );
}

#[test]
fn builder_rendered_front_faces_the_building() {
    let toward_building = Vec3::new(4.0, 0.0, 3.0).normalize();
    let yaw = build_clip_facing(toward_building);
    let rendered_front = Quat::from_rotation_y(yaw) * Vec3::NEG_Z;

    assert!(
        rendered_front.dot(toward_building) > 0.999,
        "the character asset's local -Z front must face the work"
    );
}

#[test]
fn twelve_failed_tree_routes_widen_the_search_instead_of_stopping_work() {
    let mut cycle = 7;
    let mut failed = 0;
    for _ in 0..11 {
        let (next_cycle, next_failed, widened) = advance_failed_tree_candidate(cycle, failed);
        cycle = next_cycle;
        failed = next_failed;
        assert!(!widened);
    }
    assert_eq!(failed, 11);
    let before_widening = cycle;
    let (next_cycle, next_failed, widened) = advance_failed_tree_candidate(cycle, failed);
    cycle = next_cycle;
    failed = next_failed;
    assert!(widened);
    assert_eq!(failed, 0);
    assert_eq!(cycle, before_widening.wrapping_add(13));

    let (_cycle, failed, widened) = advance_failed_tree_candidate(cycle, failed);
    assert!(!widened);
    assert_eq!(failed, 1, "the routine must remain live after widening");
}

#[test]
fn sparse_grove_retries_rotate_around_each_tree() {
    let choice_count = 2;
    let starts: Vec<_> = (0..16)
        .map(|cycle| tree_approach_start(cycle, choice_count))
        .collect();

    assert_eq!(&starts[..4], &[0, 0, 1, 1]);
    assert_eq!(
        starts
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        (0..TREE_APPROACH_ANGLES.len()).collect(),
        "two sparse trees must eventually be tried from all eight sides"
    );
}

#[test]
fn settlement_seek_targets_the_authored_hall_entrance() {
    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.init_resource::<VillageClock>();
    app.add_systems(Update, seek_settlement);

    let hall_position = Vec3::new(120.0, 8.0, -40.0);
    let hall_rotation = 0.73;
    app.world_mut().spawn((
        Settlement {
            name: "Doorstead".to_string(),
            tier: shared::components::SettlementTier::Hamlet,
            residents: 0,
            treasury: 0,
        },
        PlayerPosition(hall_position),
        PlayerRotation(hall_rotation),
    ));
    let villager = app
        .world_mut()
        .spawn((
            PlayerPosition(hall_position + Vec3::X * 40.0),
            VillagerIntent::Idle,
        ))
        .id();

    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f32(SEEK_INTERVAL + 0.1));
    app.update();

    let target = app.world().get::<MoveTarget>(villager).unwrap().0;
    let expected = SettlementBuildingKind::Hall.entrance_position(hall_position, hall_rotation);
    assert!(
        target.distance(expected) < 0.01,
        "migration must target the reachable authored door, not the solid hall centre: {target:?}"
    );
    assert!(target.distance(hall_position) > 1.0);
}

#[test]
fn migration_admission_is_bounded_by_real_time_even_at_high_warp() {
    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.init_resource::<VillageClock>();
    app.add_systems(Update, seek_settlement);

    let hall_position = Vec3::new(120.0, 8.0, -40.0);
    app.world_mut().spawn((
        Settlement {
            name: "Burstford".to_string(),
            tier: shared::components::SettlementTier::Hamlet,
            residents: 0,
            treasury: 0,
        },
        PlayerPosition(hall_position),
        PlayerRotation(0.0),
        shared::components::TimeWarp::clamped(100.0),
    ));
    for index in 0..40 {
        app.world_mut().spawn((
            PlayerPosition(hall_position + Vec3::new(40.0, 0.0, index as f32 * 0.2)),
            VillagerIntent::Idle,
        ));
    }

    let count_travelling = |app: &mut App| {
        let world = app.world_mut();
        world
            .query::<&VillagerIntent>()
            .iter(world)
            .filter(|intent| matches!(intent, VillagerIntent::Travelling { .. }))
            .count()
    };
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f32(SEEK_INTERVAL + 0.01));
    app.update();
    assert_eq!(
        count_travelling(&mut app),
        MAX_MIGRATION_ADMISSIONS_PER_PASS
    );

    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f32(SEEK_INTERVAL + 0.01));
    app.update();
    assert_eq!(
        count_travelling(&mut app),
        MAX_MIGRATION_ADMISSIONS_PER_PASS * 2,
        "100x must not admit the entire paused spawn burst in one tick"
    );
}

#[test]
fn a_later_successful_cohort_route_wakes_failed_immigrants_early() {
    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.init_resource::<VillageClock>();
    app.init_resource::<crate::world::village_roads::VillageRoadGraph>();
    app.add_systems(Update, seek_settlement);

    let hall_position = Vec3::new(120.0, 8.0, -40.0);
    let hall = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Wakeford".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 0,
                treasury: 0,
            },
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
        ))
        .id();
    let start3 = hall_position + Vec3::new(40.0, 0.0, 3.0);
    let entrance3 = SettlementBuildingKind::Hall.entrance_position(hall_position, 0.0);
    let villager = app
        .world_mut()
        .spawn((
            PlayerPosition(start3),
            VillagerIntent::Idle,
            MigrationCooldown::after_failure(None, hall, 0.0, 0),
        ))
        .id();

    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f32(SEEK_INTERVAL + 0.01));
    app.update();
    assert!(matches!(
        app.world().get::<VillagerIntent>(villager),
        Some(VillagerIntent::Idle)
    ));
    assert!(app.world().get::<MoveTarget>(villager).is_none());

    // This represents one of the later, closer immigrants successfully
    // proving a route to the same hall while the first cohort is cooling down.
    let start = Vec2::new(start3.x, start3.z);
    let entrance = Vec2::new(entrance3.x, entrance3.z);
    app.world_mut()
        .resource_mut::<crate::world::village_roads::VillageRoadGraph>()
        .cache_tactical_route(
            start + Vec2::new(1.0, 0.0),
            entrance,
            &[(start + Vec2::new(1.0, 0.0), false), (entrance, false)],
            false,
        );

    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f32(SEEK_INTERVAL + 0.01));
    app.update();
    assert!(matches!(
        app.world().get::<VillagerIntent>(villager),
        Some(VillagerIntent::Travelling { settlement }) if *settlement == hall
    ));
    assert_eq!(
        app.world().get::<MoveTarget>(villager).unwrap().0,
        entrance3
    );
}

#[test]
fn migration_repairs_an_obsolete_hall_centre_target() {
    let mut app = village_test_app();
    app.add_systems(Update, arrive_at_settlement);

    let hall_position = Vec3::new(120.0, 8.0, -40.0);
    let hall_rotation = 0.73;
    let settlement = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Doorstead".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 0,
                treasury: 0,
            },
            PlayerPosition(hall_position),
            PlayerRotation(hall_rotation),
        ))
        .id();
    let villager = app
        .world_mut()
        .spawn((
            PlayerPosition(hall_position + Vec3::X * 40.0),
            VillagerIntent::Travelling { settlement },
            MoveTarget(hall_position),
        ))
        .id();

    app.update();

    let target = app.world().get::<MoveTarget>(villager).unwrap().0;
    let expected = SettlementBuildingKind::Hall.entrance_position(hall_position, hall_rotation);
    assert!(
        target.distance(expected) < 0.01,
        "a live villager stranded by the old centre target must be woken and redirected"
    );
}

#[test]
fn a_nearby_failed_migrant_joins_the_forecourt_line_instead_of_cooling_down() {
    let mut app = village_test_app();
    app.init_resource::<MootQueueClock>();
    app.add_systems(Update, arrive_at_settlement);

    let hall_position = Vec3::new(120.0, 18.0, -40.0);
    let settlement = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Forecourt".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 0,
                treasury: 0,
            },
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
        ))
        .id();
    let position = hall_position + Vec3::new(8.0, -4.0, 0.0);
    let goal = SettlementBuildingKind::Hall.entrance_position(hall_position, 0.0);
    let villager = app
        .world_mut()
        .spawn((
            PlayerPosition(position),
            VillagerIntent::Travelling { settlement },
            MoveTarget(goal),
            NavigationRouteFailed { goal },
        ))
        .id();

    app.update();

    let villager = app.world().entity(villager);
    assert!(matches!(
        villager.get::<VillagerIntent>(),
        Some(VillagerIntent::Travelling { settlement: target }) if *target == settlement
    ));
    assert!(villager.contains::<MootQueueTicket>());
    assert!(!villager.contains::<MigrationCooldown>());
    assert!(!villager.contains::<NavigationRouteFailed>());
}

#[test]
fn thirty_then_thirty_immigrants_recover_across_day_two_and_warp_changes() {
    use shared::components::TimeWarp;

    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.init_resource::<VillageClock>();
    app.add_systems(
        Update,
        (seek_settlement, arrive_at_settlement, recount_residents).chain(),
    );

    let first_hall_position = Vec3::ZERO;
    let second_hall_position = Vec3::new(100.0, 0.0, 0.0);
    let first_hall = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Nearford".into(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 0,
                treasury: 0,
            },
            PlayerPosition(first_hall_position),
            PlayerRotation(0.0),
        ))
        .id();
    let second_hall = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Farford".into(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 0,
                treasury: 0,
            },
            PlayerPosition(second_hall_position),
            PlayerRotation(0.0),
        ))
        .id();
    let clock = app
        .world_mut()
        .spawn((WorldTime::new_default(), TimeWarp::clamped(100.0)))
        .id();
    let failed_goal = SettlementBuildingKind::Hall.entrance_position(first_hall_position, 0.0);

    let mut first_wave = Vec::new();
    for index in 0..30 {
        let position = Vec3::new(20.0, 0.0, index as f32 * 0.05);
        first_wave.push(
            app.world_mut()
                .spawn((
                    PlayerPosition(position),
                    VillagerIntent::Travelling {
                        settlement: first_hall,
                    },
                    MoveTarget(failed_goal),
                    NavigationRouteFailed { goal: failed_goal },
                ))
                .id(),
        );
    }

    // The failed cohort must return to decision-making, not remain counted
    // as embodied-but-not-resident travellers forever.
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f32(3.1));
    app.update();
    assert!(first_wave.iter().all(|entity| matches!(
        app.world().get::<VillagerIntent>(*entity),
        Some(VillagerIntent::Idle)
    )));

    // At 10x the next bounded seek passes exclude Nearford only for these
    // people, so they choose the other viable town. Thirty migrants require
    // four real-time admission batches; warp changes never alter that CPU
    // pacing or the state transition.
    app.world_mut().get_mut::<TimeWarp>(clock).unwrap().0 = 10.0;
    for _ in 0..4 {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(0.26));
        app.update();
    }
    assert!(first_wave.iter().all(|entity| matches!(
        app.world().get::<VillagerIntent>(*entity),
        Some(VillagerIntent::Travelling { settlement }) if *settlement == second_hall
    )));
    for entity in &first_wave {
        app.world_mut()
            .get_mut::<PlayerPosition>(*entity)
            .unwrap()
            .0 = second_hall_position;
    }
    app.update();

    // Day two receives a fresh cohort. Their choices are independent of
    // the first cohort's cooldown, so they can join the nearer town.
    app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 2;
    app.world_mut().get_mut::<TimeWarp>(clock).unwrap().0 = 100.0;
    for index in 0..30 {
        let position = first_hall_position + Vec3::new(0.0, 0.0, index as f32 * 0.01);
        app.world_mut()
            .spawn((PlayerPosition(position), VillagerIntent::Idle));
    }
    for _ in 0..4 {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(0.26));
        app.update();
    }

    assert_eq!(
        app.world().get::<Settlement>(first_hall).unwrap().residents,
        30
    );
    assert_eq!(
        app.world()
            .get::<Settlement>(second_hall)
            .unwrap()
            .residents,
        30
    );
    let (resident_intents, failed_routes) = {
        let world = app.world_mut();
        let resident_intents = world
            .query::<&VillagerIntent>()
            .iter(world)
            .filter(|intent| intent.counts_as_resident())
            .count();
        let failed_routes = world.query::<&NavigationRouteFailed>().iter(world).count();
        (resident_intents, failed_routes)
    };
    assert_eq!(resident_intents, 60);
    assert_eq!(failed_routes, 0);
}

#[test]
fn one_hundred_twenty_real_routes_join_without_queue_starvation_at_100x() {
    use crate::player::hero::step_units;
    use crate::world::pathfinding::PathfindingBudgetSettings;
    use shared::components::{CharacterKind, TimeWarp};
    use shared::region::RegionCoord;

    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.init_resource::<VillageClock>();
    app.init_resource::<crate::world::village_roads::VillageRoadGraph>();
    app.insert_resource(PathfindingBudgetSettings {
        max_requests_per_tick: 16,
        ..default()
    });
    app.insert_resource(WorldTerrain::default());
    app.add_systems(
        Update,
        (
            claim_settlement_hall_obstacles,
            tag_villager_intent,
            seek_settlement,
            arrive_at_settlement,
            recount_residents,
            crate::world::village_roads::rebuild_village_road_graph,
            crate::world::village_roads::queue_villager_travel_routes,
            crate::world::village_roads::plan_villager_travel_routes,
            step_units,
        )
            .chain(),
    );

    let hall_y = app
        .world()
        .resource::<WorldTerrain>()
        .get_height(1_700.0, 0.0);
    let hall_position = Vec3::new(1_700.0, hall_y, 0.0);
    let hall = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Crowdford".into(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 0,
                treasury: 0,
            },
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
        ))
        .id();
    app.world_mut()
        .spawn((WorldTime::new_default(), TimeWarp::clamped(100.0)));
    for index in 0..120 {
        let x = 1_738.0 + (index % 6) as f32 * 0.35;
        let z = (index / 6) as f32 * 0.08 - 0.8;
        let y = app.world().resource::<WorldTerrain>().get_height(x, z);
        let position = Vec3::new(x, y, z);
        app.world_mut().spawn((
            CharacterName(format!("CrowdImmigrant{index:03}")),
            CharacterKind::Villager,
            CharacterActivity::Idle,
            PlayerPosition(position),
            PlayerRotation(0.0),
            RegionCoord::from_world_pos(position),
        ));
    }

    let tick = std::time::Duration::from_secs_f32(1.0 / 60.0);
    let started = std::time::Instant::now();
    // Admissions are intentionally paced at 32 people per real second, so a
    // paused 120-person burst cannot become 120 simultaneous A* requests.
    // Five real seconds covers admission plus the final embodied approach.
    for _ in 0..300 {
        app.world_mut().resource_mut::<Time>().advance_by(tick);
        app.update();
    }
    let elapsed = started.elapsed();

    let counted_residents = app.world().get::<Settlement>(hall).unwrap().residents;
    let (idle, travelling, residents, pending, failed) = {
        let world = app.world_mut();
        let mut totals = (0, 0, 0, 0, 0);
        for (intent, route_pending, route_failed) in world
            .query::<(
                &VillagerIntent,
                Has<NavigationRoutePending>,
                Has<NavigationRouteFailed>,
            )>()
            .iter(world)
        {
            totals.0 += usize::from(matches!(intent, VillagerIntent::Idle));
            totals.1 += usize::from(matches!(intent, VillagerIntent::Travelling { .. }));
            totals.2 += usize::from(intent.counts_as_resident());
            totals.3 += usize::from(route_pending);
            totals.4 += usize::from(route_failed);
        }
        totals
    };
    assert_eq!(
        (
            counted_residents,
            idle,
            travelling,
            residents,
            pending,
            failed
        ),
        (120, 0, 0, 120, 0, 0),
        "120-person migration failed after {elapsed:?}"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(8),
        "cached shared-destination migration took {elapsed:?}"
    );
}

#[test]
fn construction_stops_walking_before_it_turns_the_builder_inward() {
    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.init_resource::<PublishedTerrainDeltas>();
    app.insert_resource(WorldTerrain::default());
    app.add_systems(Update, advance_construction);

    let settlement = app
        .world_mut()
        .spawn(Settlement {
            name: "Facing Test".to_string(),
            tier: shared::components::SettlementTier::Hamlet,
            residents: 1,
            treasury: 0,
        })
        .id();
    let plot = Vec3::new(40.0, 5.0, 10.0);
    let stand = Vec3::new(40.0, 5.0, 4.0);
    let builder = app
        .world_mut()
        .spawn((
            PlayerPosition(stand),
            PlayerRotation(0.0),
            VillagerIntent::Resident { settlement },
            MoveTarget(stand),
            HomeRoutine {
                home: settlement,
                phase: HomePhase::Leaving,
                failed_routes: 0,
            },
        ))
        .id();
    app.world_mut().spawn((
        UnderConstruction {
            kind: SettlementBuildingKind::House,
            position: plot,
            rotation: 0.0,
            owner: Some("Ada".to_string()),
            owner_id: Some(shared::components::PersonId(1)),
            builder: Some(builder),
            settlement,
            settlement_id: shared::components::SettlementId(1),
            stand,
            failed_stand_routes: 0,
            stage: BuildStage::Walking,
            quality: 0.5,
        },
        shared::components::ConstructionSite {
            kind: SettlementBuildingKind::House,
            settlement: "Facing Test".to_string(),
            raising: false,
            stand,
            rotation: 0.0,
        },
        GoodsInventory::new(SettlementBuildingKind::House.construction_storage_bulk()),
        PlayerPosition(plot),
    ));

    app.update();
    assert!(
        app.world().entity(builder).contains::<MoveTarget>(),
        "construction must wait until its builder has finished leaving home"
    );
    app.world_mut().entity_mut(builder).remove::<HomeRoutine>();
    app.update();

    let builder = app.world().entity(builder);
    assert!(
        builder.get::<MoveTarget>().is_none(),
        "the completed approach target must not overwrite construction facing"
    );
    let yaw = builder.get::<PlayerRotation>().unwrap().0;
    let rendered_front = Quat::from_rotation_y(yaw) * Vec3::NEG_Z;
    let toward_building = (plot - stand).normalize();
    assert!(rendered_front.dot(toward_building) > 0.999);
}

#[test]
fn the_first_worksite_chops_its_own_wood_when_no_lumber_hut_exists() {
    use crate::player::hero::step_units;
    use shared::components::{CharacterKind, TimeWarp};
    use shared::region::RegionCoord;

    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.insert_resource(WorldTerrain::default());
    app.add_systems(
        Update,
        (
            run_construction_material_logistics,
            sync_carried_load,
            step_units,
        )
            .chain(),
    );

    let hall_position = {
        let terrain = app.world().resource::<WorldTerrain>();
        Vec3::new(1720.0, terrain.get_height(1720.0, 0.0), 0.0)
    };
    app.world_mut()
        .spawn((WorldTime::new_default(), TimeWarp::clamped(100.0)));
    let settlement = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Firstwood".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            },
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
            GoodsInventory::new(shared::economy::capacity::HALL),
        ))
        .id();
    let kind = SettlementBuildingKind::Farmstead;
    let site_position = hall_position + Vec3::new(30.0, 0.0, 0.0);
    let stand = shared::components::builder_stand_position(
        site_position,
        0.0,
        kind.art().definition().footprint.y,
    );
    let builder = app
        .world_mut()
        .spawn((
            CharacterName("Ada".to_string()),
            CharacterKind::Villager,
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
            RegionCoord::from_world_pos(hall_position),
            CharacterActivity::Idle,
            GoodsInventory::new(shared::economy::capacity::VILLAGER),
            CarriedLoad::default(),
            VillagerIntent::Resident { settlement },
        ))
        .id();
    let site = app
        .world_mut()
        .spawn((
            UnderConstruction {
                kind,
                position: site_position,
                rotation: 0.0,
                owner: Some("Ada".to_string()),
                owner_id: Some(shared::components::PersonId(1)),
                builder: Some(builder),
                settlement,
                settlement_id: shared::components::SettlementId(1),
                stand,
                failed_stand_routes: 0,
                stage: BuildStage::Supplying,
                quality: 0.5,
            },
            shared::components::ConstructionSite {
                kind,
                settlement: "Firstwood".to_string(),
                raising: false,
                stand,
                rotation: 0.0,
            },
            GoodsInventory::new(kind.construction_storage_bulk()),
            PlayerPosition(site_position),
        ))
        .id();
    app.world_mut().entity_mut(builder).insert((
        VillagerIntent::Building { settlement, site },
        ConstructionMaterialRoutine {
            site,
            cycle: 0,
            failed_tree_routes: 0,
            failed_store_routes: 0,
            failed_delivery_routes: 0,
            tree_retry_after: 0.0,
            store_retry_after: 0.0,
            phase: ConstructionMaterialPhase::Seeking,
        },
    ));

    let step = std::time::Duration::from_secs_f32(1.0 / 60.0);
    let mut saw_chopping = false;
    let mut saw_carried_wood = false;
    // Emergency self-supply now carries two bundles per tree rather than the
    // professional three. Keep enough accelerated time for all six physical
    // tree trips needed by this twelve-Wood farmstead.
    for _ in 0..900 {
        app.world_mut().resource_mut::<Time>().advance_by(step);
        app.update();
        let builder_ref = app.world().entity(builder);
        saw_chopping |= builder_ref
            .get::<CharacterActivity>()
            .is_some_and(|activity| *activity == CharacterActivity::Chopping);
        saw_carried_wood |= builder_ref
            .get::<CarriedLoad>()
            .is_some_and(|load| load.good == Some(Good::Wood) && load.amount > 0);
        if app
            .world()
            .entity(site)
            .get::<GoodsInventory>()
            .is_some_and(|inventory| {
                inventory.amount(Good::Wood) >= kind.construction_wood_required()
            })
        {
            break;
        }
    }

    assert!(
        saw_chopping,
        "the founding builder must visibly chop a real tree"
    );
    assert!(
        saw_carried_wood,
        "chopped wood must travel in the builder's arms"
    );
    assert_eq!(
        app.world()
            .entity(site)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Wood),
        kind.construction_wood_required()
    );
    assert_eq!(
        app.world()
            .entity(settlement)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Wood),
        0,
        "this test has no market stock and no lumber hut to source from"
    );
}

#[test]
fn failed_market_route_releases_a_construction_supplier_to_gather_wood() {
    use shared::components::CharacterKind;

    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.insert_resource(WorldTerrain::default());
    app.add_systems(Update, run_construction_material_logistics);
    app.world_mut().spawn(WorldTime::new_default());

    let hall_position = Vec3::new(1720.0, 0.0, 0.0);
    let settlement = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Routeford".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            },
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
            {
                let mut stock = GoodsInventory::new(shared::economy::capacity::HALL);
                stock.add(Good::Wood, 20);
                stock
            },
            MootMarket::founding(),
        ))
        .id();
    let entrance = SettlementBuildingKind::Hall.entrance_position(hall_position, 0.0);
    let site_position = hall_position + Vec3::new(30.0, 0.0, 0.0);
    let stand = shared::components::builder_stand_position(
        site_position,
        0.0,
        SettlementBuildingKind::House.art().definition().footprint.y,
    );
    let builder = app
        .world_mut()
        .spawn((
            CharacterName("Ada".to_string()),
            CharacterKind::Villager,
            PlayerPosition(hall_position + Vec3::X * 12.0),
            PlayerRotation(0.0),
            CharacterActivity::Idle,
            GoodsInventory::new(shared::economy::capacity::VILLAGER),
            Wallet::default(),
            VillagerIntent::Resident { settlement },
        ))
        .id();
    let site = app
        .world_mut()
        .spawn((
            UnderConstruction {
                kind: SettlementBuildingKind::House,
                position: site_position,
                rotation: 0.0,
                owner: Some("Ada".to_string()),
                owner_id: Some(shared::components::PersonId(1)),
                builder: Some(builder),
                settlement,
                settlement_id: shared::components::SettlementId(1),
                stand,
                failed_stand_routes: 0,
                stage: BuildStage::Supplying,
                quality: 0.5,
            },
            GoodsInventory::new(SettlementBuildingKind::House.construction_storage_bulk()),
            PlayerPosition(site_position),
        ))
        .id();
    app.world_mut().entity_mut(builder).insert((
        VillagerIntent::Building { settlement, site },
        ConstructionMaterialRoutine {
            site,
            cycle: 0,
            failed_tree_routes: 0,
            failed_store_routes: 0,
            failed_delivery_routes: 0,
            tree_retry_after: 0.0,
            store_retry_after: 0.0,
            phase: ConstructionMaterialPhase::CollectingFromStore {
                source: settlement,
                entrance,
            },
        },
        MoveTarget(entrance),
        NavigationRouteFailed { goal: entrance },
    ));

    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs(1));
    app.update();

    let builder_ref = app.world().entity(builder);
    assert!(builder_ref.get::<NavigationRouteFailed>().is_none());
    assert!(builder_ref.get::<MoveTarget>().is_none());
    let routine = builder_ref.get::<ConstructionMaterialRoutine>().unwrap();
    assert!(matches!(routine.phase, ConstructionMaterialPhase::Seeking));
    assert_eq!(routine.failed_store_routes, 1);
    assert!(
        routine.store_retry_after > app.world().resource::<Time>().elapsed_secs_f64(),
        "the inaccessible entrance must be backed off instead of retried every tick"
    );
}

#[test]
fn public_construction_buys_private_wood_and_pays_its_business_owner() {
    use shared::components::CharacterKind;
    use shared::economy::{CivicAccount, MarketSeller};

    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.insert_resource(WorldTerrain::default());
    app.add_systems(
        Update,
        (run_construction_material_logistics, apply_business_events).chain(),
    );
    app.world_mut().spawn(WorldTime::new_default());

    let settlement_id = shared::components::SettlementId(91);
    let seller_id = shared::components::BuildingId(92);
    let seller_company_id = shared::components::CompanyId(920);
    let seller_company = spawn_test_company(&mut app, seller_company_id.0, 0);
    let hall_position = Vec3::new(1_720.0, 0.0, 0.0);
    let hall_entrance = SettlementBuildingKind::Hall.entrance_position(hall_position, 0.0);
    let mut hall_stock = GoodsInventory::new(shared::economy::capacity::HALL);
    assert_eq!(hall_stock.add(Good::Wood, 10), 10);
    let mut market = MootMarket::founding();
    market.consign(MarketSeller::Business(seller_id), Good::Wood, 10, 50);
    let hall = app
        .world_mut()
        .spawn((
            settlement_id,
            Settlement {
                name: "Paidworks".into(),
                tier: shared::components::SettlementTier::Village,
                residents: 2,
                treasury: 2_000,
            },
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
            hall_stock,
            market,
            MootAdministration::default(),
            SettlementPolicies::default(),
            CivicAccount::default(),
        ))
        .id();
    let _seller = app
        .world_mut()
        .spawn((
            seller_id,
            shared::components::BuildingOf(settlement_id),
            shared::components::OperatedBy(seller_company_id),
            BusinessAccount::default(),
        ))
        .id();
    let kind = SettlementBuildingKind::Market;
    let site_position = hall_position + Vec3::X * 30.0;
    let stand = shared::components::builder_stand_position(
        site_position,
        0.0,
        kind.art().definition().footprint.y,
    );
    let builder = app
        .world_mut()
        .spawn((
            shared::components::PersonId(93),
            CharacterName("Reeve Rowan".into()),
            CharacterKind::Villager,
            PlayerPosition(hall_entrance),
            PlayerRotation(0.0),
            CharacterActivity::Idle,
            GoodsInventory::new(shared::economy::capacity::VILLAGER),
            VillagerIntent::Resident { settlement: hall },
        ))
        .id();
    let site = app
        .world_mut()
        .spawn((
            UnderConstruction {
                kind,
                position: site_position,
                rotation: 0.0,
                owner: None,
                owner_id: None,
                builder: Some(builder),
                settlement: hall,
                settlement_id,
                stand,
                failed_stand_routes: 0,
                stage: BuildStage::Supplying,
                quality: 0.5,
            },
            GoodsInventory::new(kind.construction_storage_bulk()),
            PlayerPosition(site_position),
        ))
        .id();
    app.world_mut().entity_mut(builder).insert((
        VillagerIntent::Building {
            settlement: hall,
            site,
        },
        ConstructionMaterialRoutine {
            site,
            cycle: 0,
            failed_tree_routes: 0,
            failed_store_routes: 0,
            failed_delivery_routes: 0,
            tree_retry_after: 0.0,
            store_retry_after: 0.0,
            phase: ConstructionMaterialPhase::CollectingFromStore {
                source: hall,
                entrance: hall_entrance,
            },
        },
    ));

    app.update();

    assert_eq!(
        app.world()
            .get::<GoodsInventory>(builder)
            .unwrap()
            .amount(Good::Wood),
        4,
        "the builder's sixteen-bulk inventory carries four Wood"
    );
    assert_eq!(
        app.world()
            .get::<CompanyAccount>(seller_company)
            .unwrap()
            .cash,
        190,
        "the private consignor receives gross price less the ten-penny market fee"
    );
    assert_eq!(app.world().get::<Settlement>(hall).unwrap().treasury, 1_810);
    let civic = app.world().get::<CivicAccount>(hall).unwrap();
    assert_eq!(civic.current_day.material_expense, 200);
    assert_eq!(civic.current_day.market_fee_income, 10);
    assert_eq!(
        app.world()
            .get::<MootMarket>(hall)
            .unwrap()
            .seller_listed_units(MarketSeller::Business(seller_id), Good::Wood),
        6
    );
}

#[test]
fn construction_waits_for_the_last_required_wood_bundle() {
    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.init_resource::<PublishedTerrainDeltas>();
    app.insert_resource(WorldTerrain::default());
    app.add_systems(Update, advance_construction);

    let settlement = app
        .world_mut()
        .spawn(Settlement {
            name: "Tenwood".to_string(),
            tier: shared::components::SettlementTier::Hamlet,
            residents: 1,
            treasury: 0,
        })
        .id();
    let kind = SettlementBuildingKind::House;
    let plot = Vec3::new(40.0, 5.0, 10.0);
    let stand =
        shared::components::builder_stand_position(plot, 0.0, kind.art().definition().footprint.y);
    let builder = app
        .world_mut()
        .spawn((
            PlayerPosition(stand),
            PlayerRotation(0.0),
            VillagerIntent::Resident { settlement },
            CharacterActivity::Idle,
        ))
        .id();
    let mut materials = GoodsInventory::new(kind.construction_storage_bulk());
    materials.add(Good::Wood, kind.construction_wood_required() - 1);
    let site = app
        .world_mut()
        .spawn((
            UnderConstruction {
                kind,
                position: plot,
                rotation: 0.0,
                owner: Some("Ada".to_string()),
                owner_id: Some(shared::components::PersonId(1)),
                builder: Some(builder),
                settlement,
                settlement_id: shared::components::SettlementId(1),
                stand,
                failed_stand_routes: 0,
                stage: BuildStage::Supplying,
                quality: 0.5,
            },
            shared::components::ConstructionSite {
                kind,
                settlement: "Tenwood".to_string(),
                raising: false,
                stand,
                rotation: 0.0,
            },
            materials,
            PlayerPosition(plot),
        ))
        .id();
    *app.world_mut()
        .entity_mut(builder)
        .get_mut::<VillagerIntent>()
        .unwrap() = VillagerIntent::Building { settlement, site };

    app.update();
    assert_eq!(
        app.world()
            .entity(site)
            .get::<UnderConstruction>()
            .unwrap()
            .stage,
        BuildStage::Supplying
    );
    assert!(
        !app.world()
            .entity(site)
            .get::<shared::components::ConstructionSite>()
            .unwrap()
            .raising
    );

    app.world_mut()
        .entity_mut(site)
        .get_mut::<GoodsInventory>()
        .unwrap()
        .add(Good::Wood, 1);
    app.update();
    assert_eq!(
        app.world()
            .entity(site)
            .get::<UnderConstruction>()
            .unwrap()
            .stage,
        BuildStage::Walking
    );
    app.update();
    assert!(matches!(
        app.world()
            .entity(site)
            .get::<UnderConstruction>()
            .unwrap()
            .stage,
        BuildStage::Raising { .. }
    ));
    app.update();
    assert_eq!(
        app.world().get::<CharacterActivity>(builder),
        Some(&CharacterActivity::Building),
        "the replicated activity replaces the client's former N×M proximity scan"
    );
}

#[test]
fn inherited_business_escrow_becomes_completed_firm_cash() {
    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.init_resource::<PublishedTerrainDeltas>();
    app.insert_resource(WorldTerrain::default());
    app.add_systems(
        Update,
        (ensure_business_economies, advance_construction).chain(),
    );

    let settlement_id = shared::components::SettlementId(991);
    let settlement = app
        .world_mut()
        .spawn((
            settlement_id,
            Settlement {
                name: "Escrowton".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            },
        ))
        .id();
    let plot = Vec3::new(40.0, 5.0, 10.0);
    let stand = Vec3::new(40.0, 5.0, 4.0);
    let builder = app
        .world_mut()
        .spawn((
            PlayerPosition(stand),
            PlayerRotation(0.0),
            VillagerIntent::Building {
                settlement,
                site: Entity::PLACEHOLDER,
            },
            CharacterActivity::Building,
        ))
        .id();
    let capital = 250;
    let site = app
        .world_mut()
        .spawn((
            UnderConstruction {
                kind: SettlementBuildingKind::Windmill,
                position: plot,
                rotation: 0.0,
                owner: Some("Ada".to_string()),
                owner_id: Some(shared::components::PersonId(7)),
                builder: Some(builder),
                settlement,
                settlement_id,
                stand,
                failed_stand_routes: 0,
                stage: BuildStage::Raising { seconds_left: 0.0 },
                quality: 0.8,
            },
            shared::components::ConstructionSite {
                kind: SettlementBuildingKind::Windmill,
                settlement: "Escrowton".to_string(),
                raising: true,
                stand,
                rotation: 0.0,
            },
            GoodsInventory::new(SettlementBuildingKind::Windmill.construction_storage_bulk()),
            InheritedBusinessCapital(capital),
        ))
        .id();
    *app.world_mut().get_mut::<VillagerIntent>(builder).unwrap() =
        VillagerIntent::Building { settlement, site };
    app.world_mut().spawn(WorldTime::new_default());

    app.update();

    assert!(app.world().get_entity(site).is_err());
    let completed = app
        .world_mut()
        .query_filtered::<Entity, With<SettlementBuilding>>()
        .single(app.world())
        .expect("completed inherited Windmill exists");
    assert_eq!(
        app.world()
            .get::<InheritedBusinessCapital>(completed)
            .unwrap()
            .0,
        capital,
        "completion must move escrow to the firm in the same update"
    );

    app.update();
    let account = app
        .world_mut()
        .query_filtered::<&BusinessAccount, With<SettlementBuilding>>()
        .single(app.world())
        .expect("completed inherited Windmill has a business account");
    assert_eq!(account.unposted_company_capital, capital);
}

#[test]
fn failed_final_construction_route_tries_another_perimeter_work_point() {
    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.init_resource::<PublishedTerrainDeltas>();
    app.insert_resource(WorldTerrain::default());
    app.add_systems(Update, advance_construction);
    app.world_mut().spawn(WorldTime::new_default());

    let settlement_id = shared::components::SettlementId(77);
    let settlement = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Roundabout".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            },
            settlement_id,
        ))
        .id();
    let kind = SettlementBuildingKind::House;
    let plot = Vec3::new(40.0, 5.0, 10.0);
    let stand =
        shared::components::builder_stand_position(plot, 0.0, kind.art().definition().footprint.y);
    let builder = app
        .world_mut()
        .spawn((
            PlayerPosition(Vec3::ZERO),
            PlayerRotation(0.0),
            VillagerIntent::Resident { settlement },
            MoveTarget(stand),
            NavigationRouteFailed { goal: stand },
        ))
        .id();
    let mut materials = GoodsInventory::new(kind.construction_storage_bulk());
    materials.add(Good::Wood, kind.construction_wood_required());
    let site = app
        .world_mut()
        .spawn((
            UnderConstruction {
                kind,
                position: plot,
                rotation: 0.0,
                owner: Some("Ada".to_string()),
                owner_id: Some(shared::components::PersonId(1)),
                builder: Some(builder),
                settlement,
                settlement_id,
                stand,
                failed_stand_routes: 0,
                stage: BuildStage::Walking,
                quality: 0.5,
            },
            shared::components::ConstructionSite {
                kind,
                settlement: "Roundabout".to_string(),
                raising: false,
                stand,
                rotation: 0.0,
            },
            materials,
            PlayerPosition(plot),
        ))
        .id();
    *app.world_mut()
        .entity_mut(builder)
        .get_mut::<VillagerIntent>()
        .unwrap() = VillagerIntent::Building { settlement, site };

    app.update();

    let site_ref = app.world().entity(site);
    let under = site_ref.get::<UnderConstruction>().unwrap();
    assert_eq!(under.stage, BuildStage::Walking);
    assert_eq!(under.failed_stand_routes, 1);
    assert_ne!(under.stand, stand);
    assert_eq!(
        site_ref
            .get::<shared::components::ConstructionSite>()
            .unwrap()
            .stand,
        under.stand
    );
    let builder_ref = app.world().entity(builder);
    assert!(builder_ref.get::<NavigationRouteFailed>().is_none());
    assert_eq!(builder_ref.get::<MoveTarget>().unwrap().0, under.stand);
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
            + Vec2::splat(crate::world::navgrid::VILLAGER_NAV_RADIUS),
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
fn a_coastal_food_shortage_diversifies_after_the_first_farm() {
    assert!(planning::should_try_complementary_fishing(
        Some(SettlementBuildingKind::Farmstead),
        1,
        0,
    ));
    assert!(!planning::should_try_complementary_fishing(None, 1, 0));
    assert!(!planning::should_try_complementary_fishing(
        Some(SettlementBuildingKind::House),
        1,
        0,
    ));
    assert!(!planning::should_try_complementary_fishing(
        Some(SettlementBuildingKind::Farmstead),
        1,
        1,
    ));
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

#[test]
fn three_secure_days_qualify_a_hamlet_without_skipping_civic_construction() {
    let mut app = village_test_app();
    app.init_resource::<SettlementEconomyRuntime>();
    app.add_systems(
        Update,
        (ensure_settlement_economies, update_settlement_economies).chain(),
    );
    let clock = app.world_mut().spawn(WorldTime::new_default()).id();
    let mut stock = GoodsInventory::new(shared::economy::capacity::HALL);
    // Three days are consumed during the observation window and the final
    // state must still retain the advertised three-day reserve.
    let starting_food = VILLAGE_MIN_RESIDENTS * 6;
    assert_eq!(stock.add(Good::Flour, starting_food), starting_food);
    let hall = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Plenty".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: VILLAGE_MIN_RESIDENTS,
                treasury: 0,
            },
            stock,
        ))
        .id();

    app.update();
    for day in 1..=3 {
        app.world_mut()
            .resource_mut::<SettlementEconomyRuntime>()
            .record_food_production(hall, VILLAGE_MIN_RESIDENTS);
        app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = day;
        app.update();
    }

    let settlement = app.world().get::<Settlement>(hall).unwrap();
    let economy = app.world().get::<SettlementEconomy>(hall).unwrap();
    assert_eq!(
        settlement.tier,
        shared::components::SettlementTier::Hamlet,
        "economy evidence must not bypass the Village Hall worksite"
    );
    assert_eq!(economy.food_secure_days, VILLAGE_REQUIRED_SECURE_DAYS);
    assert!(economy.prosperity >= VILLAGE_MIN_PROSPERITY);
}

#[test]
fn construction_admission_grows_but_cannot_explode_with_a_population_burst() {
    assert_eq!(planning::concurrent_worksite_capacity(3), 3);
    assert_eq!(planning::concurrent_worksite_capacity(36), 3);
    assert_eq!(planning::concurrent_worksite_capacity(160), 12);
    assert_eq!(planning::concurrent_worksite_capacity(5_000), 12);

    assert!(planning::development_pipeline_has_capacity(160, 8, 3));
    assert!(
        !planning::development_pipeline_has_capacity(160, 8, 4),
        "a completed shell keeps its development slot until its road is connected"
    );
}

#[test]
fn food_and_housing_permits_are_approved_without_waiting_for_construction() {
    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.init_resource::<VillageClock>();
    app.insert_resource(WorldTerrain::default());
    app.add_systems(Update, consider_permits);

    let hall_position = {
        let terrain = app.world().resource::<WorldTerrain>();
        Vec3::new(1720.0, terrain.get_height(1720.0, 0.0), 0.0)
    };
    let settlement = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Quickstead".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 3,
                treasury: 0,
            },
            PlayerPosition(hall_position),
        ))
        .id();
    let terrain_version = app
        .world()
        .resource::<WorldTerrain>()
        .modification_version();
    app.world_mut()
        .resource_mut::<VillageClock>()
        .failed_fishing_terrain_versions
        .insert(settlement, terrain_version);
    for name in ["Ada", "Bea", "Cy"] {
        app.world_mut().spawn((
            CharacterName(name.to_string()),
            VillagerIntent::Resident { settlement },
        ));
    }

    for expected_sites in 1..=2 {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(PERMIT_INTERVAL + 0.1));
        app.update();
        let (site_count, site_kinds) = {
            let world = app.world_mut();
            let mut query = world.query::<&UnderConstruction>();
            let kinds = query.iter(world).map(|site| site.kind).collect::<Vec<_>>();
            (kinds.len(), kinds)
        };
        assert_eq!(
            site_count, expected_sites,
            "the next distinct permit must not wait for earlier construction; pending={site_kinds:?} deferred={:?}",
            app.world().resource::<VillageClock>().deferred_opportunities,
        );
    }

    let mut world = std::mem::take(&mut *app.world_mut());
    let sites: Vec<_> = world
        .query::<&UnderConstruction>()
        .iter(&world)
        .map(|site| (site.kind, site.owner.clone().unwrap(), site.position))
        .collect();
    let kinds: HashSet<_> = sites.iter().map(|(kind, _, _)| *kind).collect();
    assert_eq!(
        kinds,
        HashSet::from([
            SettlementBuildingKind::Farmstead,
            SettlementBuildingKind::House,
        ]),
        "independent food and housing opportunities should share the founding pipeline; resource businesses still need suitable geography"
    );
    let owners: HashSet<_> = sites.iter().map(|(_, owner, _)| owner.as_str()).collect();
    assert_eq!(
        owners.len(),
        2,
        "zero-holding residents must receive their first permit before repeat owners"
    );
    for (index, (_, _, position)) in sites.iter().enumerate() {
        for (_, _, other) in sites.iter().skip(index + 1) {
            assert!(
                position.distance(*other) > 1.0,
                "pending plots must reserve their ground"
            );
        }
    }
}

#[test]
fn a_permit_does_not_interrupt_an_active_fisher_mid_shift() {
    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.init_resource::<VillageClock>();
    app.insert_resource(WorldTerrain::default());
    app.add_systems(Update, consider_permits);

    let hall_position = {
        let terrain = app.world().resource::<WorldTerrain>();
        Vec3::new(1720.0, terrain.get_height(1720.0, 0.0), 0.0)
    };
    let settlement = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Shiftstead".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 2,
                treasury: 0,
            },
            PlayerPosition(hall_position),
        ))
        .id();
    let terrain_version = app
        .world()
        .resource::<WorldTerrain>()
        .modification_version();
    app.world_mut()
        .resource_mut::<VillageClock>()
        .failed_fishing_terrain_versions
        .insert(settlement, terrain_version);
    let active_fisher = app
        .world_mut()
        .spawn((
            CharacterName("Ada".to_string()),
            VillagerIntent::Resident { settlement },
            FishingRoutine {
                hut: settlement,
                pier: settlement,
                hall: settlement,
                catch_seconds: 0.0,
                failed_workplace_routes: 0,
                production_day: 0,
                produced_today: 0,
                phase: FishingPhase::Fishing,
            },
        ))
        .id();
    let available = app
        .world_mut()
        .spawn((
            CharacterName("Bea".to_string()),
            VillagerIntent::Resident { settlement },
        ))
        .id();

    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f32(PERMIT_INTERVAL + 0.1));
    app.update();

    let world = app.world_mut();
    let site = world
        .query::<&UnderConstruction>()
        .single(world)
        .expect("an available resident should still receive the needed permit");
    assert_eq!(site.builder, Some(available));
    assert_ne!(site.builder, Some(active_fisher));
    assert!(
        world.entity(active_fisher).contains::<FishingRoutine>(),
        "granting another resident's permit must not interrupt active fishing"
    );
}

#[test]
fn an_off_shift_employee_resigns_before_starting_private_construction() {
    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.init_resource::<VillageClock>();
    app.insert_resource(WorldTerrain::default());
    app.add_systems(Update, consider_permits);

    let hall_position = {
        let terrain = app.world().resource::<WorldTerrain>();
        Vec3::new(1720.0, terrain.get_height(1720.0, 0.0), 0.0)
    };
    let settlement = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Newstart".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            },
            PlayerPosition(hall_position),
        ))
        .id();
    let terrain_version = app
        .world()
        .resource::<WorldTerrain>()
        .modification_version();
    app.world_mut()
        .resource_mut::<VillageClock>()
        .failed_fishing_terrain_versions
        .insert(settlement, terrain_version);

    let worker = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            CharacterName("Ada".to_string()),
            VillagerIntent::Resident { settlement },
            PlayerPosition(hall_position + Vec3::X * 2.0),
            Occupation(Some("Company Porter".to_string())),
            WorkStatus::Employed,
            Wallet::founding_villager(),
            GoodsInventory::new(shared::economy::capacity::PORTER),
            shared::components::EmployedAt(shared::components::BuildingId(9_901)),
            CompanyPorter {
                settlement,
                settlement_id: shared::components::SettlementId(9_902),
                company: shared::components::CompanyId(9_903),
                storage_hall: shared::components::BuildingId(9_901),
            },
        ))
        .id();

    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f32(PERMIT_INTERVAL + 0.1));
    app.update();

    let world = app.world_mut();
    let site = world
        .query::<&UnderConstruction>()
        .single(world)
        .expect("the off-shift worker should be free to choose a private permit");
    assert_eq!(site.builder, Some(worker));
    let worker = world.entity(worker);
    assert!(worker.get::<shared::components::EmployedAt>().is_none());
    assert!(worker.get::<CompanyPorter>().is_none());
    assert_eq!(worker.get::<Occupation>(), Some(&Occupation(None)));
    assert_eq!(
        worker.get::<WorkStatus>(),
        Some(&WorkStatus::LookingForWork)
    );
}

#[test]
fn the_reeve_builds_public_progression_without_stopping_essential_trades() {
    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.init_resource::<VillageClock>();
    app.insert_resource(WorldTerrain::default());
    app.add_systems(Update, consider_permits);

    let hall_position = {
        let terrain = app.world().resource::<WorldTerrain>();
        Vec3::new(1720.0, terrain.get_height(1720.0, 0.0), 0.0)
    };
    let settlement = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Civicstead".to_string(),
                tier: shared::components::SettlementTier::Village,
                residents: 4,
                treasury: 0,
            },
            PlayerPosition(hall_position),
            MootAdministration {
                reeve: Some("Ada".to_string()),
                ..default()
            },
        ))
        .id();
    for kind in [
        SettlementBuildingKind::Farmstead,
        SettlementBuildingKind::Windmill,
        SettlementBuildingKind::Bakery,
        SettlementBuildingKind::LumberjackHut,
        // Keep this test focused on the Reeve's public Marketplace duty. The
        // completed Quarry prevents unrelated Stone opportunities from
        // competing with the civic permit under test.
        SettlementBuildingKind::StoneQuarry,
        SettlementBuildingKind::House,
    ] {
        app.world_mut().spawn(SettlementBuilding {
            kind,
            settlement: "Civicstead".to_string(),
            owner: Some("Founder".to_string()),
            quality: 0.5,
            workers: Vec::new(),
        });
    }
    for (name, occupation) in [
        ("Ada", "Reeve"),
        ("Bea", "Farmer"),
        ("Cy", "Woodcutter"),
        ("Dee", "Fisher"),
    ] {
        app.world_mut().spawn((
            CharacterName(name.to_string()),
            VillagerIntent::Resident { settlement },
            Occupation(Some(occupation.to_string())),
            WorkStatus::Employed,
            Wallet::new(shared::economy::STARTING_VILLAGER_MONEY),
        ));
    }

    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f32(PERMIT_INTERVAL + 0.1));
    app.update();

    let world = app.world_mut();
    let (site_entity, site) = world
        .query::<(Entity, &UnderConstruction)>()
        .iter(world)
        .next()
        .expect("the Village should request its Marketplace");
    assert_eq!(site.kind, SettlementBuildingKind::Market);
    assert_eq!(site.owner, None, "the Marketplace is a public work");
    let reeve = world
        .query::<(&CharacterName, &VillagerIntent)>()
        .iter(world)
        .find(|(name, _)| name.0 == "Ada")
        .unwrap();
    assert!(
        matches!(reeve.1, VillagerIntent::Building { site: active, .. } if *active == site_entity)
    );
    for (name, intent) in world
        .query::<(&CharacterName, &VillagerIntent)>()
        .iter(world)
    {
        if name.0 != "Ada" {
            assert!(
                matches!(intent, VillagerIntent::Resident { .. }),
                "{name:?} was pulled away from essential work"
            );
        }
    }
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
            MarketPorter { settlement },
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

    app.world_mut().entity_mut(porter).remove::<MarketPorter>();
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
fn a_market_porter_collects_a_bounded_load_while_the_woodcutter_keeps_working() {
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
            MarketPorter { settlement: hall },
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
        .get_mut::<PlayerPosition>()
        .unwrap()
        .0 = hall_entrance;
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
    app.world_mut().entity_mut(porter).remove::<MarketPorter>();
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
        porter_commands.insert(MarketPorter { settlement: hall });
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
            MarketPorter { settlement: hall },
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
            MarketPorter { settlement: hall },
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
                    MarketPorter { settlement: hall },
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
            MarketPorter { settlement: hall },
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
fn the_market_porter_buys_inputs_for_any_business_policy() {
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
            MarketPorter { settlement: hall },
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

#[test]
fn a_financially_secure_owner_delegates_only_when_payroll_and_a_replacement_are_ready() {
    let mut app = village_test_app();
    app.add_systems(
        Update,
        (run_business_payroll_and_owner_leisure, fill_vacancies).chain(),
    );
    app.world_mut().spawn({
        let mut clock = WorldTime::new_default();
        clock.day = 2;
        clock
    });
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
            VillagerIntent::Resident { settlement },
            PlayerPosition(Vec3::ZERO),
            Wallet::default(),
            Occupation::default(),
            WorkStatus::LookingForWork,
        ))
        .id();

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

/// A short headless soak of the same world the player watches, with every
/// village clock running at 100x. This is deliberately not a mocked
/// production calculation: villagers still migrate, request permits, walk,
/// build, enter workplaces, animate work and physically haul each load.
#[test]
fn hundred_x_world_runs_complete_visible_supply_loops() {
    use crate::player::hero::step_units;
    use crate::world::pathfinding::PathfindingBudgetSettings;
    use shared::components::{CharacterKind, TimeWarp};
    use shared::region::RegionCoord;

    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.init_resource::<VillageClock>();
    app.init_resource::<SettlementEconomyRuntime>();
    app.init_resource::<PublishedTerrainDeltas>();
    app.init_resource::<crate::world::village_roads::VillageRoadGraph>();
    app.insert_resource(PathfindingBudgetSettings {
        max_requests_per_tick: 8,
        ..default()
    });
    app.insert_resource(WorldTerrain::default());
    app.add_systems(
        Update,
        (
            crate::world::time::update_world_time,
            claim_settlement_hall_obstacles,
            tag_villager_intent,
            seek_settlement,
            arrive_at_settlement,
            recount_residents,
            consider_permits,
            run_construction_material_logistics,
            advance_construction,
            ensure_farm_fields,
            crate::world::village_roads::plan_requested_roads,
            fill_vacancies,
            ensure_households,
            assign_households,
            (
                run_household_schedules,
                run_workplace_door_transits,
                crate::world::village_roads::build_village_roads,
                assign_farmer_routines,
                assign_lumberjack_routines,
                run_farmer_routines,
                run_lumberjack_routines,
                sync_carried_load,
            )
                .chain(),
            // Match the live server's navigation phase. This is
            // intentionally after decisions: changed destinations are
            // queued, planned around solid buildings, then moved. The
            // following tick observes arrivals.
            crate::world::village_roads::rebuild_village_road_graph,
            crate::world::village_roads::queue_villager_travel_routes,
            crate::world::village_roads::plan_villager_travel_routes,
            step_units,
        )
            .chain(),
    );

    let hall_position = {
        let terrain = app.world().resource::<WorldTerrain>();
        let water = terrain.water_level().unwrap_or(f32::NEG_INFINITY);
        (0..400)
            .find_map(|step| {
                let x = step as f32 * 40.0;
                let height = terrain.get_height(x, 0.0);
                (height > water + FREEBOARD + 4.0 && slope_at(terrain, x, 0.0) < 0.1)
                    .then_some(Vec3::new(x, height, 0.0))
            })
            .expect("the test map must contain dry, flat settlement ground")
    };
    app.world_mut().spawn((
        Settlement {
            name: "Fast Yewcrag".to_string(),
            tier: shared::components::SettlementTier::Hamlet,
            residents: 0,
            treasury: 0,
        },
        GoodsInventory::new(shared::economy::capacity::HALL),
        PlayerPosition(hall_position),
        PlayerRotation(0.0),
        WorldTime::new_default(),
        TimeWarp::clamped(100.0),
    ));
    for index in 0..3 {
        let spot = hall_position + Vec3::new(40.0 + index as f32 * 5.0, 0.0, 25.0);
        app.world_mut().spawn((
            CharacterName(format!("FastVillager{index}")),
            CharacterKind::Villager,
            CharacterAttributes::default(),
            PlayerPosition(spot),
            PlayerRotation(0.0),
            RegionCoord::from_world_pos(spot),
        ));
    }

    let step = std::time::Duration::from_secs_f32(1.0 / 60.0);
    let mut saw_indoors = false;
    let mut saw_farming = false;
    let mut saw_chopping = false;
    let mut saw_wheat_carried = false;
    let mut saw_wood_carried = false;
    let mut saw_partially_supplied_site = false;
    let mut saw_fully_supplied_site = false;
    let mut founding_pipeline_filled = false;
    // Forty-five wall-clock seconds represent seventy-five simulated minutes.
    // Emergency builders now carry two Wood per tree while professional
    // woodcutters carry three, so bootstrap supply needs several more visible
    // journeys before the completed workplaces can begin production.
    for _ in 0..(60 * 45) {
        if founding_pipeline_filled {
            // This is a supply-loop soak, not an unlimited development soak.
            // Freeze permit review after food and housing fill the initial
            // pipeline so the founders eventually return to completed jobs.
            app.world_mut().resource_mut::<VillageClock>().permit = -1_000_000.0;
        }
        app.world_mut().resource_mut::<Time>().advance_by(step);
        app.update();
        let world = app.world_mut();
        founding_pipeline_filled |= world.query::<&UnderConstruction>().iter(world).count() >= 2;
        saw_indoors |= world
            .query::<&CharacterActivity>()
            .iter(world)
            .any(|activity| *activity == CharacterActivity::Indoors);
        saw_farming |= world
            .query::<&CharacterActivity>()
            .iter(world)
            .any(|activity| *activity == CharacterActivity::Farming);
        saw_chopping |= world
            .query::<&CharacterActivity>()
            .iter(world)
            .any(|activity| *activity == CharacterActivity::Chopping);
        saw_wheat_carried |= world
            .query::<&CarriedLoad>()
            .iter(world)
            .any(|load| load.good == Some(Good::Wheat) && load.amount > 0);
        saw_wood_carried |= world
            .query::<&CarriedLoad>()
            .iter(world)
            .any(|load| load.good == Some(Good::Wood) && load.amount > 0);
        for (site, inventory) in world
            .query::<(&shared::components::ConstructionSite, &GoodsInventory)>()
            .iter(world)
        {
            let required = site.kind.construction_wood_required();
            let delivered = inventory.amount(Good::Wood);
            saw_partially_supplied_site |= delivered > 0 && delivered < required;
            saw_fully_supplied_site |= required > 0 && delivered >= required;
        }
    }

    let mut world = std::mem::take(&mut *app.world_mut());
    let built: HashSet<_> = world
        .query::<&SettlementBuilding>()
        .iter(&world)
        .map(|building| building.kind)
        .collect();
    let pending_sites: Vec<_> = world
        .query::<&UnderConstruction>()
        .iter(&world)
        .map(|site| (site.kind, site.stage, site.position, site.builder))
        .collect();
    let supplier_states: Vec<_> = world
        .query::<(
            Entity,
            &PlayerPosition,
            Option<&MoveTarget>,
            Option<&crate::world::village_roads::NavigationRoutePending>,
            Option<&ConstructionMaterialRoutine>,
            &GoodsInventory,
        )>()
        .iter(&world)
        .map(|(entity, position, target, pending, routine, inventory)| {
            (
                entity,
                position.0,
                target.map(|target| target.0),
                pending.map(|pending| (pending.goal, pending.exhausted())),
                routine.map(|routine| format!("{:?}", routine.phase)),
                inventory.amount(Good::Wood),
            )
        })
        .collect();
    assert!(
        built.contains(&SettlementBuildingKind::Farmstead),
        "built={built:?} pending={pending_sites:?} suppliers={supplier_states:?}"
    );
    assert!(built.contains(&SettlementBuildingKind::House), "{built:?}");
    let field_count = world.query::<&FarmField>().iter(&world).count();
    assert!(field_count >= 2 && field_count % 2 == 0);
    let households: Vec<_> = world.query::<&Household>().iter(&world).collect();
    assert_eq!(households.len(), 1);
    assert_eq!(households[0].residents.len(), 3);
    assert!(
        households[0].residents.len() <= SettlementBuildingKind::House.housing_capacity() as usize
    );
    let people_states: Vec<_> = world
        .query::<(
            &CharacterName,
            &VillagerIntent,
            &CharacterActivity,
            &PlayerPosition,
            Option<&MoveTarget>,
            Option<&FarmerRoutine>,
            Option<&LumberjackRoutine>,
            &Occupation,
            Option<&shared::components::EmployedAt>,
        )>()
        .iter(&world)
        .map(
            |(
                name,
                intent,
                activity,
                position,
                target,
                farmer,
                lumberjack,
                occupation,
                employed,
            )| {
                (
                    name.0.clone(),
                    format!("{intent:?}"),
                    *activity,
                    position.0,
                    target.map(|target| target.0),
                    farmer.map(|routine| format!("{:?}", routine.phase)),
                    lumberjack.map(|routine| format!("{:?}", routine.phase)),
                    occupation.0.clone(),
                    employed.copied(),
                )
            },
        )
        .collect();
    let road_states: Vec<_> = world
        .query::<(&VillageRoad, &shared::components::RoadOf)>()
        .iter(&world)
        .map(|(road, road_of)| (road.points.len(), road.built_through, road_of.0))
        .collect();
    assert!(
        saw_indoors,
        "workers should disappear into their workplaces: {people_states:?}; roads={road_states:?}"
    );
    assert!(saw_farming, "farm work must remain observable at 100x");
    assert!(saw_chopping, "tree work must remain observable at 100x");
    assert!(saw_wheat_carried, "wheat must be physically hauled at 100x");
    assert!(
        saw_wood_carried,
        "construction wood must be physically hauled at 100x"
    );
    assert!(
        saw_partially_supplied_site,
        "construction wood must accumulate at a worksite at 100x"
    );
    assert!(
        saw_fully_supplied_site,
        "a worksite must receive its complete wood requirement at 100x"
    );

    let wheat = world
        .query::<&GoodsInventory>()
        .iter(&world)
        .map(|inventory| inventory.amount(Good::Wheat))
        .sum::<u32>();
    assert!(wheat > 0, "the accelerated farm loop must retain wheat");
    assert!(
        world
            .query::<&CharacterAttributes>()
            .iter(&world)
            .any(|attributes| attributes.physique() > 10),
        "a successful farm cycle must train physique even at 100x"
    );
    assert!(world
        .query::<&GoodsInventory>()
        .iter(&world)
        .all(|inventory| inventory.used_bulk() <= inventory.bulk_capacity()));
}

/// The whole loop, driven by the real systems.
///
/// This is the acceptance test the design was written against, run as code
/// rather than as a person watching: found a settlement, put three people
/// on the map some way off, and let the server do everything else. Nothing
/// here assigns a resident, an occupation or a plot.
///
/// It runs the ACTUAL scheduled systems, including `step_units`, so the
/// walking, the arrival radius and the permit clock are all under test. A
/// test that called the decision functions directly would pass while the
/// villagers stood still forever.
#[test]
fn three_villagers_settle_and_build_a_village_unaided() {
    use crate::player::hero::step_units;
    use shared::components::CharacterKind;
    use shared::region::RegionCoord;

    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.init_resource::<VillageClock>();
    app.init_resource::<SettlementEconomyRuntime>();
    app.init_resource::<PublishedTerrainDeltas>();
    app.insert_resource(WorldTerrain::default());
    app.add_systems(
        Update,
        (
            tag_villager_intent,
            seek_settlement,
            step_units,
            arrive_at_settlement,
            recount_residents,
            consider_permits,
            run_construction_material_logistics,
            advance_construction,
            ensure_farm_fields,
            crate::world::village_roads::plan_requested_roads,
            fill_vacancies,
            ensure_households,
            assign_households,
            (
                run_household_schedules,
                run_workplace_door_transits,
                crate::world::village_roads::build_village_roads,
                assign_farmer_routines,
                assign_lumberjack_routines,
                run_farmer_routines,
                run_lumberjack_routines,
                sync_carried_load,
            )
                .chain(),
        )
            .chain(),
    );
    app.world_mut().spawn(WorldTime::new_default());

    // A hall on DRY, buildable land. Searched for rather than hardcoded,
    // because the origin of this map happens to be underwater -- and a test
    // that founded there would fail for a reason that has nothing to do
    // with villager autonomy.
    let hall_position = {
        let terrain = app.world().resource::<WorldTerrain>();
        let water = terrain.water_level().unwrap_or(f32::NEG_INFINITY);
        (0..400)
            .find_map(|step| {
                let x = step as f32 * 40.0;
                let height = terrain.get_height(x, 0.0);
                (height > water + FREEBOARD + 4.0 && slope_at(terrain, x, 0.0) < 0.1)
                    .then_some(Vec3::new(x, height, 0.0))
            })
            .expect("the map has dry, flat ground somewhere along the x axis")
    };
    app.world_mut().spawn((
        Settlement {
            name: "Yewcrag".to_string(),
            tier: shared::components::SettlementTier::Hamlet,
            residents: 0,
            treasury: 0,
        },
        GoodsInventory::new(shared::economy::capacity::HALL),
        PlayerPosition(hall_position),
        PlayerRotation(0.0),
    ));

    // Three people, dropped well clear of the hall so they have to walk.
    for index in 0..3 {
        let offset = Vec3::new(40.0 + index as f32 * 5.0, 0.0, 25.0);
        let spot = hall_position + offset;
        app.world_mut().spawn((
            CharacterName(format!("Villager{index}")),
            CharacterKind::Villager,
            PlayerPosition(spot),
            PlayerRotation(0.0),
            RegionCoord::from_world_pos(spot),
        ));
    }

    // Twenty minutes of simulated time. It used to be forty seconds, which
    // was ample when a permit became a building on a timer. Now somebody has
    // to WALK to each plot -- up to 60 m at 3.52 m/s -- and then spend ten
    // seconds raising it, so a village of three buildings needs roughly
    // much longer now that builders must chop and carry every wood bundle.
    // The remaining time lets the newly employed workers complete observed
    // work cycles after the last material-heavy building finishes.
    let step = std::time::Duration::from_secs_f32(1.0 / 60.0);
    let mut saw_chopping = false;
    let mut saw_carrying = false;
    for _ in 0..(60 * 1_200) {
        app.world_mut().resource_mut::<Time>().advance_by(step);
        app.update();
        let world = app.world_mut();
        saw_chopping |= world
            .query::<&CharacterActivity>()
            .iter(world)
            .any(|activity| *activity == CharacterActivity::Chopping);
        saw_carrying |= world
            .query::<&CarriedLoad>()
            .iter(world)
            .any(|load| !load.is_empty());
    }

    let mut world = std::mem::take(&mut *app.world_mut());

    let settlement = world
        .query::<&Settlement>()
        .iter(&world)
        .next()
        .cloned()
        .expect("the settlement still exists");
    assert_eq!(
        settlement.residents, 3,
        "all three walked in and joined of their own accord"
    );

    let homes: Vec<String> = world
        .query::<&Residence>()
        .iter(&world)
        .map(|home| home.0.clone())
        .collect();
    assert_eq!(homes.len(), 3, "each of them is on record as living there");
    assert!(homes.iter().all(|home| home == "Yewcrag"));

    let built: HashSet<SettlementBuildingKind> = world
        .query::<&SettlementBuilding>()
        .iter(&world)
        .map(|building| building.kind)
        .collect();
    assert!(
        built.contains(&SettlementBuildingKind::Farmstead),
        "a farm went up first, because food comes first: {built:?}"
    );
    // A mill is no longer a compulsory second shell. This deliberately small
    // fixture never advances the calendar or installs the commerce systems
    // which expose Wheat stock and actual market demand to investors; the
    // market-led integration lab covers that production-chain decision.
    assert!(
        built.contains(&SettlementBuildingKind::House),
        "somewhere to live: {built:?}"
    );
    let households: Vec<_> = world.query::<&Household>().iter(&world).collect();
    assert_eq!(households.len(), 1, "the completed cabin needs a household");
    assert_eq!(
        households[0].residents.len(),
        3,
        "all three residents should have a designated bed"
    );
    assert!(
        households[0].residents.len() <= SettlementBuildingKind::House.housing_capacity() as usize
    );

    // Every building belongs to a named person. A village of anonymous
    // structures is exactly what this design refuses.
    let ownerless = world
        .query::<&SettlementBuilding>()
        .iter(&world)
        .filter(|building| building.owner.is_none())
        .count();
    assert_eq!(ownerless, 0, "somebody applied for every one of them");

    // A dedicated Lumberjack Hut is no longer a compulsory bootstrap shell.
    // These founders visibly chopped construction timber themselves; once
    // their two sites were complete, that temporary demand disappeared before
    // a private timber firm had to open. Ownership concentration and residents
    // without property are both valid market outcomes.

    // Nothing was built in the water. Flat ground is where a village
    // wants to build and a lake bed is the flattest ground there is, so
    // this is the failure the siting rule exists to prevent.
    let water = world
        .resource::<WorldTerrain>()
        .water_level()
        .unwrap_or(f32::NEG_INFINITY);
    let drowned: Vec<_> = world
        .query::<(&SettlementBuilding, &PlayerPosition)>()
        .iter(&world)
        .filter(|(_, at)| at.0.y < water)
        .map(|(building, at)| (building.kind, at.0))
        .collect();
    assert!(
        drowned.is_empty(),
        "nothing was built in the lake: {drowned:?}"
    );

    // Housing approval is free, while business permit fees move coin from the
    // applicants into the public treasury. Ordinary working capital remains
    // in the legal company's treasury, so include company cash in this
    // approval-time conservation check.
    assert!(
        settlement.treasury > 0,
        "business permits must fund the moot"
    );
    let wallet_total = world
        .query::<(&CharacterKind, Option<&Wallet>)>()
        .iter(&world)
        .filter(|(kind, _)| **kind == CharacterKind::Villager)
        .map(|(_, wallet)| {
            wallet.map_or(shared::economy::STARTING_VILLAGER_MONEY, |wallet| {
                wallet.balance()
            })
        })
        .sum::<u64>();
    let company_cash = world
        .query::<&CompanyAccount>()
        .iter(&world)
        .map(|account| account.cash)
        .sum::<u64>();
    assert_eq!(
        wallet_total + settlement.treasury + company_cash,
        3 * shared::economy::STARTING_VILLAGER_MONEY,
        "permit approval must transfer rather than create or destroy coin"
    );

    // Somebody WALKED to each plot. If construction still completed on a
    // timer alone this would pass with the builders standing at the hall,
    // so it checks the distance from the hall rather than merely that
    // buildings exist.
    let sites: Vec<Vec3> = world
        .query::<(&SettlementBuilding, &PlayerPosition)>()
        .iter(&world)
        .map(|(_, at)| at.0)
        .collect();
    assert!(
        sites.iter().all(|at| at.distance(hall_position) > 8.0),
        "buildings should stand out on their own plots, not on the hall: {sites:?}"
    );

    // The ground under every building was levelled and published. An empty
    // map here means the terrain edit never happened or never left the
    // server, and the client would draw buildings floating over a hillside.
    let published = world.resource::<PublishedTerrainDeltas>().by_chunk.len();
    assert!(
        published > 0,
        "clearing a plot must publish a terrain delta for its chunk"
    );

    // Every building claims its plot, which is what stops trees being drawn
    // inside it and what makes it a navigation obstacle.
    let claimed = world
        .query::<(&SettlementBuilding, &shared::building::PlacedBuilding)>()
        .iter(&world)
        .count();
    assert_eq!(
        claimed,
        sites.len(),
        "every completed building must claim its ground"
    );

    // Work exists and named people hold it. The market does not promise a
    // particular second business, but the Farmstead's useful vacancies still
    // need to be filled -- and the House seats nobody, which is the point of
    // homes and workplaces being different things.
    let staffed: usize = world
        .query::<&SettlementBuilding>()
        .iter(&world)
        .map(|building| building.workers.len())
        .sum();
    assert!(
        staffed >= 2,
        "residents should have taken the vacant positions, got {staffed}"
    );

    assert!(
        saw_chopping,
        "founders should visibly chop real construction timber"
    );
    assert!(
        saw_carrying,
        "construction timber should travel in a bounded carried load"
    );
    let field_count = world.query::<&FarmField>().iter(&world).count();
    let farmstead_count = world
        .query::<&SettlementBuilding>()
        .iter(&world)
        .filter(|building| building.kind == SettlementBuildingKind::Farmstead)
        .count();
    assert_eq!(
        field_count,
        farmstead_count * 2,
        "each farmstead should create two wheat fields"
    );
    let overfilled: Vec<_> = world
        .query::<&GoodsInventory>()
        .iter(&world)
        .filter(|inventory| inventory.used_bulk() > inventory.bulk_capacity())
        .map(|inventory| (inventory.used_bulk(), inventory.bulk_capacity()))
        .collect();
    assert!(
        overfilled.is_empty(),
        "no inventory may exceed capacity: {overfilled:?}"
    );
    let titles: Vec<String> = world
        .query::<&Occupation>()
        .iter(&world)
        .filter_map(|job| job.0.clone())
        .collect();
    assert!(
        titles.iter().any(|t| t == "Farmer"),
        "somebody works the farm: {titles:?}"
    );

    // Ground quality was sampled where each building stands, not defaulted.
    let qualities: Vec<f32> = world
        .query::<&SettlementBuilding>()
        .iter(&world)
        .map(|building| building.quality)
        .collect();
    assert!(
        qualities.iter().all(|q| (0.0..=1.0).contains(q)),
        "quality must be a real 0..1 sample: {qualities:?}"
    );

    // A workplace is a bounded physical store, not an infinite production
    // counter. The worker loop added next must have somewhere finite to
    // deposit its output, and every completed building must receive it.
    let stores: Vec<(SettlementBuildingKind, u32)> = world
        .query::<(&SettlementBuilding, &shared::economy::GoodsInventory)>()
        .iter(&world)
        .map(|(building, inventory)| (building.kind, inventory.bulk_capacity()))
        .collect();
    assert_eq!(
        stores.len(),
        sites.len(),
        "every completed building needs storage"
    );
    assert!(stores
        .iter()
        .all(|(kind, capacity)| { *capacity == kind.storage_bulk_capacity() }));

    // Where the test actually founded, so a failure elsewhere is diagnosable.
    println!("founded at {hall_position:?}, waterline {water}");
    println!("plots {sites:?}");
    println!("delta chunks published: {published}");
    println!("occupations: {titles:?}  ground quality: {qualities:?}");
}

#[test]
fn houses_sit_closer_to_the_hall_than_workplaces() {
    let (house_min, house_max) = SettlementBuildingKind::House.preferred_ring();
    let (farm_min, farm_max) = SettlementBuildingKind::Farmstead.preferred_ring();
    let (wood_min, wood_max) = SettlementBuildingKind::LumberjackHut.preferred_ring();
    assert!(house_min < farm_min);
    assert!(house_min < wood_min);
    assert!(house_max < farm_max);
    assert!(house_max < wood_max);
}
