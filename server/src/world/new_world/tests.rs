use super::*;
use bevy::ecs::system::RunSystemOnce;
use shared::components::*;

#[test]
fn founding_names_are_unique_and_seeded() {
    for seed in [0, 1, 91, 12345, u64::MAX] {
        let names: std::collections::HashSet<_> = (0..SETTLEMENT_COUNT)
            .map(|i| sites::name(seed, i))
            .collect();
        assert_eq!(names.len(), SETTLEMENT_COUNT);
    }
    assert_ne!(sites::name(1, 0), sites::name(2, 0));
}

/// Actual generated geography, props, coastal access and spawning. This is
/// opt-in because map construction sets the process-wide legacy bounds and
/// because an ordinary unit-test runner should not generate a 3.7 km world.
#[test]
#[ignore = "real small Frontier founding; run with FISTWORLD_WORLD_SEED to audit a seed"]
fn small_frontier_founds_exactly_four_bare_halls_and_twenty_four_people() {
    use crate::world::start_config::WorldStartConfig;
    let config =
        WorldStartConfig::from_ron(include_str!("../../../../config/worlds/small-frontier.ron"))
            .unwrap();
    let seed = std::env::var("FISTWORLD_WORLD_SEED").map_or(91, |raw| raw.parse().unwrap());
    let terrain =
        WorldTerrain::from_loaded_map(shared::map::load_session_map(&config.recipe(seed)).unwrap());
    let mut world = World::new();
    world.insert_resource(terrain);
    world.insert_resource(config);
    world.init_resource::<crate::world::identity::WorldIdAllocator>();
    world.init_resource::<crate::world::village::PublishedTerrainDeltas>();
    world
        .run_system_once(crate::collision::library::setup_baked_colliders)
        .unwrap();
    world.run_system_once(populate).unwrap();
    // Bootstrap is once-only: another startup pass cannot refill or duplicate.
    let entities = world.entities().len();
    world.run_system_once(populate).unwrap();
    assert_eq!(world.entities().len(), entities);
    let opening = world.resource::<WorldOpening>();
    assert_eq!((opening.settlements, opening.residents), (4, 24));
    assert!(!opening.arrivals.is_empty());
    let halls: Vec<_> = world
        .query::<(&SettlementId, &Settlement, &PlayerPosition)>()
        .iter(&world)
        .map(|(id, settlement, position)| (*id, settlement.residents, position.0))
        .collect();
    assert_eq!(halls.len(), 4);
    for (index, (id, residents, position)) in halls.iter().enumerate() {
        assert_eq!(*residents, 6);
        assert!(
            halls[..index]
                .iter()
                .all(|(_, _, other)| other.xz().distance(position.xz()) >= MIN_SETTLEMENT_DISTANCE)
        );
        let actual = world
            .query::<(&PersonId, &ResidentOf, Option<&LivesAt>)>()
            .iter(&world)
            .filter(|(_, resident, _)| resident.0 == *id)
            .map(|(_, _, home)| {
                assert!(home.is_none());
                1
            })
            .sum::<usize>();
        assert_eq!(actual, 6);
    }
    assert_eq!(world.query::<&SettlementBuilding>().iter(&world).count(), 0);
    assert_eq!(world.query::<&Company>().iter(&world).count(), 0);
    assert_eq!(world.query::<&VillageRoad>().iter(&world).count(), 0);
    eprintln!(
        "FRONTIER_OPENING seed={seed} halls={halls:?} people=24 prebuilt_private_buildings=0"
    );
}

/// Full-size geography is deliberately opt-in; small unit tests cannot prove
/// that a random 8 km island supports ten connected, buildable settlements.
#[test]
#[ignore = "full-size world founding; run across seeds with FISTWORLD_WORLD_SEED"]
fn inhabited_world_has_real_homes_companies_stock_and_access() {
    let mut logging = App::new();
    logging.add_plugins(bevy::log::LogPlugin::default());
    let seed = std::env::var("FISTWORLD_WORLD_SEED").map_or(91, |s| s.parse().unwrap());
    let terrain = WorldTerrain::from_loaded_map(
        shared::map::load_session_map(&shared::map::new_world_recipe(seed)).unwrap(),
    );
    let mut world = World::new();
    world.insert_resource(terrain);
    world.init_resource::<crate::world::identity::WorldIdAllocator>();
    world.init_resource::<crate::world::village::PublishedTerrainDeltas>();
    world
        .run_system_once(crate::collision::library::setup_baked_colliders)
        .unwrap();
    world.run_system_once(populate).unwrap();
    let opening = world.resource::<WorldOpening>();
    assert!((MIN_SETTLEMENT_COUNT..=SETTLEMENT_COUNT).contains(&opening.settlements));
    assert!(!opening.arrivals.is_empty());
    let population = opening.residents;
    assert!((MIN_SETTLEMENT_COUNT * 12..=SETTLEMENT_COUNT * 76).contains(&population));
    let residents = world
        .query::<(&PersonId, &ResidentOf, &LivesAt)>()
        .iter(&world)
        .count();
    assert_eq!(residents, population);
    let people: std::collections::HashSet<_> =
        world.query::<&PersonId>().iter(&world).copied().collect();
    let buildings: std::collections::HashSet<_> =
        world.query::<&BuildingId>().iter(&world).copied().collect();
    for (person, _, home) in world
        .query::<(&PersonId, &ResidentOf, &LivesAt)>()
        .iter(&world)
    {
        assert!(people.contains(person));
        assert!(buildings.contains(&home.0));
    }
    for ownership in world.query::<&CompanyOwnership>().iter(&world) {
        assert!(people.contains(&ownership.controlling_shareholder().unwrap()));
    }
    let halls: Vec<_> = world
        .query::<(&SettlementId, &PlayerPosition, &Settlement)>()
        .iter(&world)
        .map(|(id, p, s)| (*id, p.0, s.residents))
        .collect();
    for (settlement, square) in world
        .query::<(&Settlement, Option<&SettlementCivicSquare>)>()
        .iter(&world)
    {
        if settlement.residents >= 24 {
            assert!(
                square.is_some(),
                "{} has no room reserved for its civic core",
                settlement.name
            );
        }
    }
    for (id, hall, residents) in &halls {
        let workplaces: Vec<_> = world
            .query::<(
                &BuildingOf,
                &SettlementBuilding,
                &shared::economy::BusinessStaffingPolicy,
            )>()
            .iter(&world)
            .filter(|(of, _, _)| of.0 == *id)
            .map(|(_, building, staffing)| {
                assert_eq!(
                    staffing.target_for(building.kind),
                    building.kind.positions(),
                    "opening capacity must use its real funded staffing target"
                );
                (building.kind, building.quality)
            })
            .collect();
        let audit = layout::audit_economy(&workplaces);
        assert!(
            audit.supports(*residents as usize),
            "{id:?}: {audit:?} cannot support {residents} residents"
        );
        let homes = world
            .query::<(&BuildingOf, &SettlementBuilding)>()
            .iter(&world)
            .filter(|(of, b)| of.0 == *id && b.kind == SettlementBuildingKind::House)
            .count();
        assert!(homes * 4 >= *residents as usize);
        let roads: Vec<_> = world
            .query::<(&RoadOf, &VillageRoad)>()
            .iter(&world)
            .filter(|(of, _)| of.0 == *id)
            .map(|(_, road)| road.clone())
            .collect();
        for (of, building, position, rotation) in world
            .query::<(
                &BuildingOf,
                &SettlementBuilding,
                &PlayerPosition,
                &PlayerRotation,
            )>()
            .iter(&world)
        {
            if of.0 == *id {
                assert!(crate::world::village_roads::building_has_connected_road(
                    building.kind,
                    position.0,
                    rotation.0,
                    *hall,
                    0.0,
                    &roads.iter().collect::<Vec<_>>()
                ));
            }
        }
    }
    let entities = world.entities().len();
    world.run_system_once(populate).unwrap();
    assert_eq!(
        world.entities().len(),
        entities,
        "a second startup/join must not duplicate the society"
    );
    eprintln!(
        "WORLD_OPENING_ACCEPTANCE seed={seed} settlements={} residents={population} buildings={} entities={entities}",
        halls.len(),
        buildings.len()
    );
}

#[test]
#[ignore = "ordinary physical economy over eight days on a full-size seeded world"]
fn inhabited_world_continues_without_opening_subsidies() {
    use crate::world::{regions, village_lab};
    use shared::economy::SettlementEconomy;
    let seed = std::env::var("FISTWORLD_WORLD_SEED").map_or(91, |s| s.parse().unwrap());
    let days: u32 = std::env::var("FISTWORLD_WORLD_SOAK_DAYS").map_or(8, |s| s.parse().unwrap());
    let mut app = App::new();
    village_lab::configure_lab(&mut app);
    app.insert_resource(WorldTerrain::from_loaded_map(
        shared::map::load_session_map(&shared::map::new_world_recipe(seed)).unwrap(),
    ));
    app.init_resource::<regions::RegionRegistry>();
    app.add_systems(
        Startup,
        (
            populate.after(crate::collision::library::setup_baked_colliders),
            regions::build_region_registry.after(populate),
        ),
    );
    app.world_mut()
        .spawn((WorldTime::new_default(), TimeWarp(1.0)));
    let mut money = None;
    let mut last_day = u32::MAX;
    let mut produced = 0.0_f32;
    for _ in 0..days * 1440 {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs(1));
        app.update();
        let world = app.world_mut();
        let day = world.query::<&WorldTime>().single(world).unwrap().day;
        let total = village_lab::total_money(world);
        if let Some(expected) = money {
            assert_eq!(total, expected, "the running world minted or lost money");
        }
        money = Some(total);
        if day != last_day {
            last_day = day;
            for (settlement, economy) in world
                .query::<(&Settlement, &SettlementEconomy)>()
                .iter(world)
            {
                produced = produced.max(economy.recent_food_production);
                eprintln!(
                    "WORLD_SOAK seed={seed} day={day} {} residents={} reserve_days={:.2} food_produced={:.1} hunger={} homeless={} employed={}",
                    settlement.name,
                    settlement.residents,
                    economy.reserve_days,
                    economy.recent_food_production,
                    economy.unmet_food,
                    economy.homeless_residents,
                    economy.private_filled_jobs
                );
            }
        }
    }
    let world = app.world_mut();
    assert!(
        produced > 0.0,
        "the opening stores hid a non-producing world"
    );
    let population = world.resource::<WorldOpening>().residents;
    let residents = world
        .query::<&CharacterKind>()
        .iter(world)
        .filter(|kind| **kind == CharacterKind::Villager)
        .count();
    assert_eq!(
        residents, population,
        "the unattended opening lost residents"
    );
    for (settlement, economy) in world
        .query::<(&Settlement, &SettlementEconomy)>()
        .iter(world)
    {
        assert!(
            economy.recent_food_production > 0.0,
            "{} survived on opening stock without recent local food production: {:?}",
            settlement.name,
            economy
        );
        assert_eq!(
            economy.unmet_food, 0,
            "{} ended the unattended opening with unmet food demand: {:?}",
            settlement.name, economy
        );
        assert_eq!(
            economy.homeless_residents, 0,
            "{} lost its prepared housing",
            settlement.name
        );
        assert!(
            economy.reserve_days >= 1.0,
            "{} exhausted its food supply: {:?}",
            settlement.name,
            economy
        );
    }
}
