//! Village planning regression fixtures and invariants.

use super::*;

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
fn houses_sit_closer_to_the_hall_than_workplaces() {
    let (house_min, house_max) = SettlementBuildingKind::House.preferred_ring();
    let (farm_min, farm_max) = SettlementBuildingKind::Farmstead.preferred_ring();
    let (wood_min, wood_max) = SettlementBuildingKind::LumberjackHut.preferred_ring();
    assert!(house_min < farm_min);
    assert!(house_min < wood_min);
    assert!(house_max < farm_max);
    assert!(house_max < wood_max);
}
