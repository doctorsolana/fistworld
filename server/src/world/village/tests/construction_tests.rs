//! Village construction regression fixtures and invariants.

use super::*;

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
            last_tree: None,
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
fn emergency_builder_batches_two_trees_into_one_full_delivery() {
    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.insert_resource(WorldTerrain::default());
    app.add_systems(Update, run_construction_material_logistics);
    app.world_mut().spawn(WorldTime::new_default());

    let hall_position = Vec3::new(1720.0, 6.0, 0.0);
    let settlement_id = shared::components::SettlementId(90);
    let settlement = app
        .world_mut()
        .spawn((
            settlement_id,
            Settlement {
                name: "Batchwood".into(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            },
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
            GoodsInventory::new(shared::economy::capacity::HALL),
        ))
        .id();
    let owner = shared::components::PersonId(902);
    let builder = app
        .world_mut()
        .spawn((
            owner,
            CharacterName("Batch Builder".into()),
            CharacterKind::Hero,
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
            CharacterActivity::Chopping,
            GoodsInventory::new(shared::economy::capacity::VILLAGER),
            Wallet::default(),
        ))
        .id();
    let kind = SettlementBuildingKind::House;
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
                owner: Some("Batch Builder".into()),
                owner_id: Some(owner),
                builder: Some(builder),
                settlement,
                settlement_id,
                stand,
                failed_stand_routes: 0,
                stage: BuildStage::Supplying,
                quality: 1.0,
            },
            shared::components::ConstructionSite {
                kind,
                settlement: "Batchwood".into(),
                raising: false,
                stand,
                rotation: 0.0,
            },
            GoodsInventory::new(kind.construction_storage_bulk()),
            PlayerPosition(position),
            crate::player::permits::PlayerConstructionProject { owner },
        ))
        .id();
    app.world_mut().entity_mut(builder).insert((
        PlayerConstructionAssignment { site, settlement },
        ConstructionMaterialRoutine {
            site,
            cycle: 0,
            last_tree: None,
            failed_tree_routes: 0,
            failed_store_routes: 0,
            failed_delivery_routes: 0,
            tree_retry_after: 0.0,
            store_retry_after: 0.0,
            phase: ConstructionMaterialPhase::Chopping {
                tree: hall_position + Vec3::X,
                seconds_left: 0.0,
            },
        },
    ));

    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs(1));
    app.update();

    assert_eq!(
        app.world()
            .get::<GoodsInventory>(builder)
            .unwrap()
            .amount(Good::Wood),
        2
    );
    assert!(matches!(
        app.world()
            .get::<ConstructionMaterialRoutine>(builder)
            .unwrap()
            .phase,
        ConstructionMaterialPhase::Seeking
    ));
    assert_eq!(
        app.world()
            .get::<ConstructionMaterialRoutine>(builder)
            .unwrap()
            .last_tree,
        Some(hall_position + Vec3::X),
        "top-up selection must remember and avoid the visible trunk just felled"
    );
    assert!(
        app.world().get::<MoveTarget>(builder).is_none(),
        "a half-load should seek another tree rather than start the site trip"
    );

    app.world_mut()
        .get_mut::<ConstructionMaterialRoutine>(builder)
        .unwrap()
        .phase = ConstructionMaterialPhase::Chopping {
        tree: hall_position + Vec3::Z,
        seconds_left: 0.0,
    };
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs(1));
    app.update();

    assert_eq!(
        app.world()
            .get::<GoodsInventory>(builder)
            .unwrap()
            .amount(Good::Wood),
        4
    );
    assert!(matches!(
        app.world()
            .get::<ConstructionMaterialRoutine>(builder)
            .unwrap()
            .phase,
        ConstructionMaterialPhase::Delivering { .. }
    ));
    assert_eq!(app.world().get::<MoveTarget>(builder).unwrap().0, stand);
}

#[test]
fn construction_top_up_tree_is_distinct_from_the_tree_just_felled() {
    let terrain = WorldTerrain::default();
    let hall = Vec3::new(1720.0, terrain.get_height(1720.0, 0.0), 0.0);
    let mut cache = TreeWorkCandidateCache::default();
    let mut first = None;
    for _ in 0..32 {
        match find_tree_for_cycle_cached(&mut cache, &terrain, None, None, hall, 0, 71) {
            TreeCandidateLookup::Pending => {}
            TreeCandidateLookup::Found { tree, stand } => {
                first = Some((tree, stand));
                break;
            }
            TreeCandidateLookup::Unavailable => panic!("fixture should contain usable woodland"),
        }
    }
    let (first_tree, first_stand) = first.expect("tree cache should complete within 32 ticks");

    let second = find_nearby_tree_for_cycle_cached(
        &mut cache,
        &terrain,
        None,
        None,
        hall,
        first_stand,
        Some(first_tree),
        1,
        71,
    );
    let TreeCandidateLookup::Found {
        tree: second_tree, ..
    } = second
    else {
        panic!("fixture should offer a distinct top-up tree");
    };
    assert!(
        second_tree.distance_squared(first_tree) >= 0.01,
        "generated props do not deplete yet, so a batch must not chop one visible trunk twice"
    );
}

/// Dusk must not freeze a hauler mid-task: wood on a shoulder still reaches
/// the site, a partially loaded tree walker turns back to deliver, and an
/// empty builder merely WALKING toward a tree stands down cleanly (phase reset
/// to Seeking, no stale walk order) instead of finishing a dead march and
/// posing beside the worksite until dawn.
#[test]
fn nightfall_finishes_deliveries_in_flight_and_stands_down_the_rest_cleanly() {
    let mut app = village_test_app();
    app.init_resource::<Time>();
    app.insert_resource(WorldTerrain::default());
    app.add_systems(Update, run_construction_material_logistics);
    // 30 seconds past sunset.
    app.world_mut().spawn(WorldTime::new(
        WorldTime::DEFAULT_DAY_DURATION,
        WorldTime::DEFAULT_NIGHT_DURATION,
        WorldTime::DEFAULT_DAY_DURATION + 30.0,
    ));

    let hall_position = Vec3::new(1720.0, 6.0, 0.0);
    let settlement = app
        .world_mut()
        .spawn((
            shared::components::SettlementId(21),
            Settlement {
                name: "Nightwood".into(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 2,
                treasury: 0,
            },
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
            GoodsInventory::new(shared::economy::capacity::HALL),
        ))
        .id();
    let kind = SettlementBuildingKind::Farmstead;
    let site_position = hall_position + Vec3::X * 40.0;
    let stand = shared::components::builder_stand_position(
        site_position,
        0.0,
        kind.art().definition().footprint.y,
    );
    let spawn_builder = |app: &mut App, id: u64, phase: ConstructionMaterialPhase| {
        app.world_mut()
            .spawn((
                shared::components::PersonId(id),
                CharacterName(format!("Builder {id}")),
                CharacterKind::Villager,
                PlayerPosition(hall_position),
                PlayerRotation(0.0),
                CharacterActivity::Building,
                GoodsInventory::new(shared::economy::capacity::VILLAGER),
                ConstructionMaterialRoutine {
                    site: Entity::PLACEHOLDER,
                    cycle: 0,
                    last_tree: None,
                    failed_tree_routes: 0,
                    failed_store_routes: 0,
                    failed_delivery_routes: 0,
                    tree_retry_after: 0.0,
                    store_retry_after: 0.0,
                    phase,
                },
            ))
            .id()
    };
    let deliverer = spawn_builder(
        &mut app,
        31,
        ConstructionMaterialPhase::Delivering {
            destination: site_position,
        },
    );
    let tree_walker = spawn_builder(
        &mut app,
        32,
        ConstructionMaterialPhase::WalkingToTree {
            tree: hall_position + Vec3::Z * 60.0,
            stand: hall_position + Vec3::Z * 58.0,
        },
    );
    let partial_tree_walker = spawn_builder(
        &mut app,
        33,
        ConstructionMaterialPhase::WalkingToTree {
            tree: hall_position - Vec3::Z * 60.0,
            stand: hall_position - Vec3::Z * 58.0,
        },
    );
    app.world_mut()
        .get_mut::<GoodsInventory>(partial_tree_walker)
        .unwrap()
        .add(Good::Wood, 2);
    let site = app
        .world_mut()
        .spawn((
            UnderConstruction {
                kind,
                position: site_position,
                rotation: 0.0,
                owner: None,
                owner_id: None,
                builder: Some(deliverer),
                settlement,
                settlement_id: shared::components::SettlementId(21),
                stand,
                failed_stand_routes: 0,
                stage: BuildStage::Supplying,
                quality: 0.5,
            },
            shared::components::ConstructionSite {
                kind,
                settlement: "Nightwood".into(),
                raising: false,
                stand,
                rotation: 0.0,
            },
            GoodsInventory::new(kind.construction_storage_bulk()),
            PlayerPosition(site_position),
        ))
        .id();
    for builder in [deliverer, tree_walker, partial_tree_walker] {
        app.world_mut().entity_mut(builder).insert((
            VillagerIntent::Building { settlement, site },
            MoveTarget(site_position),
        ));
        let mut routine = app
            .world_mut()
            .get_mut::<ConstructionMaterialRoutine>(builder)
            .unwrap();
        routine.site = site;
    }

    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs(1));
    app.update();

    // The delivery in flight continues through the night...
    assert!(matches!(
        app.world()
            .get::<ConstructionMaterialRoutine>(deliverer)
            .unwrap()
            .phase,
        ConstructionMaterialPhase::Delivering { .. }
    ));
    assert!(
        app.world().get::<MoveTarget>(deliverer).is_some(),
        "the deliverer must keep walking its cargo to the site"
    );
    // ...while the un-invested walk stands down cleanly for the morning.
    assert!(matches!(
        app.world()
            .get::<ConstructionMaterialRoutine>(tree_walker)
            .unwrap()
            .phase,
        ConstructionMaterialPhase::Seeking
    ));
    assert!(
        app.world().get::<MoveTarget>(tree_walker).is_none(),
        "a stood-down builder must not finish a stale march"
    );
    assert_eq!(
        *app.world().get::<CharacterActivity>(tree_walker).unwrap(),
        CharacterActivity::Idle
    );
    let partial_phase = app
        .world()
        .get::<ConstructionMaterialRoutine>(partial_tree_walker)
        .unwrap()
        .phase;
    assert!(
        matches!(partial_phase, ConstructionMaterialPhase::Delivering { .. }),
        "partial dusk load did not begin delivery: {partial_phase:?}"
    );
    assert_eq!(
        app.world()
            .get::<MoveTarget>(partial_tree_walker)
            .unwrap()
            .0,
        stand,
        "a partial dusk load must turn back toward its worksite"
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
            last_tree: None,
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
            last_tree: None,
            failed_tree_routes: 0,
            failed_store_routes: 0,
            failed_delivery_routes: 0,
            tree_retry_after: 0.0,
            store_retry_after: 0.0,
            phase: ConstructionMaterialPhase::CollectingFromStore {
                source: settlement,
                entrance,
                reserved_units: 4,
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
            last_tree: None,
            failed_tree_routes: 0,
            failed_store_routes: 0,
            failed_delivery_routes: 0,
            tree_retry_after: 0.0,
            store_retry_after: 0.0,
            phase: ConstructionMaterialPhase::CollectingFromStore {
                source: hall,
                entrance: hall_entrance,
                reserved_units: 4,
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

/// The give-up path must fully hand the person back to ordinary resident
/// life and park the site: a builder pinned "finding wood" forever was the
/// exact fear this mechanism exists to remove.
#[test]
fn a_starved_supplier_returns_to_resident_life_and_parks_the_site() {
    use bevy::ecs::system::RunSystemOnce;

    let mut world = World::new();
    let settlement = world.spawn_empty().id();
    let builder = world
        .spawn((
            CharacterName("Odo".to_string()),
            VillagerIntent::Idle,
            CharacterActivity::Idle,
        ))
        .id();
    let site = world
        .spawn(UnderConstruction {
            kind: SettlementBuildingKind::House,
            position: Vec3::ZERO,
            rotation: 0.0,
            owner: Some("Odo".to_string()),
            owner_id: Some(shared::components::PersonId(7)),
            builder: Some(builder),
            settlement,
            settlement_id: shared::components::SettlementId(1),
            stand: Vec3::ZERO,
            failed_stand_routes: 0,
            stage: BuildStage::Supplying,
            quality: 0.5,
        })
        .id();
    world
        .entity_mut(builder)
        .insert(ConstructionMaterialRoutine::new(site));

    world
        .run_system_once(
            move |mut commands: Commands,
                  mut sites: Query<&mut UnderConstruction>,
                  names: Query<&CharacterName>| {
                let mut site_view = sites.get_mut(site).unwrap();
                crate::world::village::construction::give_up_starved_supply(
                    &mut commands,
                    builder,
                    names.get(builder).unwrap(),
                    site,
                    &mut site_view,
                    None,
                    100.0,
                );
            },
        )
        .unwrap();

    assert_eq!(world.get::<UnderConstruction>(site).unwrap().builder, None);
    let cooldown = world
        .get::<ConstructionSupplyCooldown>(site)
        .expect("the parked site must carry a give-up cooldown");
    assert_eq!(cooldown.failures, 1);
    assert!(cooldown.blocks(100.0) && !cooldown.blocks(100.0 + 100_000.0));
    assert!(
        world.get::<ConstructionMaterialRoutine>(builder).is_none(),
        "the material routine must be released"
    );
    assert!(matches!(
        world.get::<VillagerIntent>(builder),
        Some(VillagerIntent::Resident { settlement: s }) if *s == settlement
    ));
}

/// A parked site rests until its cooldown passes, then the ordinary orphan
/// recovery re-drafts a free resident to try the market and woodland again.
#[test]
fn a_parked_site_is_re_drafted_only_after_its_cooldown_expires() {
    use bevy::ecs::system::RunSystemOnce;
    use shared::components::{CharacterKind, Health};

    let mut world = World::new();
    world.spawn(WorldTime::new_default());
    let settlement = world.spawn_empty().id();
    let free_resident = world
        .spawn((
            CharacterKind::Villager,
            Health::default(),
            VillagerIntent::Resident { settlement },
            WorkStatus::LookingForWork,
        ))
        .id();
    let site = world
        .spawn((
            UnderConstruction {
                kind: SettlementBuildingKind::House,
                position: Vec3::ZERO,
                rotation: 0.0,
                owner: None,
                owner_id: None,
                builder: None,
                settlement,
                settlement_id: shared::components::SettlementId(1),
                stand: Vec3::ZERO,
                failed_stand_routes: 0,
                stage: BuildStage::Supplying,
                quality: 0.5,
            },
            // WorldTime starts at day 0, second 0, so this cooldown is live.
            ConstructionSupplyCooldown {
                retry_after: 10_000.0,
                failures: 1,
            },
        ))
        .id();

    world
        .run_system_once(crate::world::village::mortality::recover_orphaned_construction)
        .unwrap();
    assert_eq!(
        world.get::<UnderConstruction>(site).unwrap().builder,
        None,
        "a resting site must not be re-drafted while its cooldown blocks"
    );

    world.entity_mut(site).insert(ConstructionSupplyCooldown {
        retry_after: 0.0,
        failures: 1,
    });
    world
        .run_system_once(crate::world::village::mortality::recover_orphaned_construction)
        .unwrap();
    assert_eq!(
        world.get::<UnderConstruction>(site).unwrap().builder,
        Some(free_resident),
        "an expired cooldown must let recovery re-draft a free resident"
    );
    assert!(world
        .get::<ConstructionMaterialRoutine>(free_resident)
        .is_some());
}
