//! Village integration regression fixtures and invariants.

use super::*;

/// A focused bootstrap supply regression at 100x: real migration, FIFO registration and permits,
/// navigation, building, workplace doors and carried production. Development
/// is deliberately stopped after the initial sites are admitted. Personal
/// needs, market transactions and business management are not installed here;
/// connected and town-growth labs cover the complete village schedule.
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
            (arrive_at_settlement, advance_immigration_departures).chain(),
            recount_residents,
            (
                consider_permits,
                moot_services::advance_moot_service_queues,
                moot_services::complete_moot_permit_pickups,
            )
                .chain(),
            run_construction_material_logistics,
            advance_construction,
            (
                ensure_farm_fields,
                ensure_livestock_pastures,
                ensure_fishing_piers,
            )
                .chain(),
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
                assign_quarry_routines,
                assign_fishing_routines,
                run_farmer_routines,
                run_lumberjack_routines,
                run_quarry_routines,
                run_fishing_routines,
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
    let mut saw_food_work = false;
    let mut saw_chopping = false;
    let mut saw_food_carried = false;
    let mut saw_wood_carried = false;
    let mut saw_partially_supplied_site = false;
    let mut saw_fully_supplied_site = false;
    let mut founding_pipeline_filled = false;
    // Forty-five seconds of input time represent seventy-five world minutes;
    // this is independent of the machine's wall-clock execution speed.
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
        saw_food_work |= world
            .query::<&CharacterActivity>()
            .iter(world)
            .any(|activity| {
                matches!(
                    activity,
                    CharacterActivity::Farming | CharacterActivity::Fishing
                )
            });
        saw_chopping |= world
            .query::<&CharacterActivity>()
            .iter(world)
            .any(|activity| *activity == CharacterActivity::Chopping);
        saw_food_carried |= world.query::<&CarriedLoad>().iter(world).any(|load| {
            matches!(load.good, Some(Good::Wheat | Good::Meat | Good::Food)) && !load.is_empty()
        });
        saw_wood_carried |= world
            .query::<&CarriedLoad>()
            .iter(world)
            .any(|load| load.good == Some(Good::Wood) && !load.is_empty());
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
        built.iter().any(|kind| matches!(
            kind,
            SettlementBuildingKind::Farmstead
                | SettlementBuildingKind::LivestockFarm
                | SettlementBuildingKind::FishermansHut
        )),
        "built={built:?} pending={pending_sites:?} suppliers={supplier_states:?}"
    );
    assert!(built.contains(&SettlementBuildingKind::House), "{built:?}");
    let field_count = world.query::<&FarmField>().iter(&world).count();
    // Food type is the planner's economic/environmental choice. Verify the
    // corresponding real worksite instead of forcing a crop business.
    let food_sites: Vec<_> = world
        .query::<&SettlementBuilding>()
        .iter(&world)
        .map(|site| site.kind)
        .collect();
    assert_eq!(
        field_count,
        food_sites
            .iter()
            .filter(|kind| **kind == SettlementBuildingKind::Farmstead)
            .count()
            * 2
    );
    assert_eq!(
        world.query::<&LivestockPasture>().iter(&world).count(),
        food_sites
            .iter()
            .filter(|kind| **kind == SettlementBuildingKind::LivestockFarm)
            .count()
    );
    assert_eq!(
        world.query::<&FishingPier>().iter(&world).count(),
        food_sites
            .iter()
            .filter(|kind| **kind == SettlementBuildingKind::FishermansHut)
            .count()
    );
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
        "household/workplace doors should produce real indoor activity: {people_states:?}; roads={road_states:?}"
    );
    assert!(
        saw_food_work,
        "food production work must remain observable at 100x"
    );
    assert!(saw_chopping, "tree work must remain observable at 100x");
    assert!(
        saw_food_carried,
        "the selected food output must be physically hauled at 100x"
    );
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

    let deposited_food = world
        .query::<(&SettlementBuilding, &GoodsInventory)>()
        .iter(&world)
        .map(|(site, inventory)| match site.kind {
            SettlementBuildingKind::Farmstead => inventory.amount(Good::Wheat),
            SettlementBuildingKind::LivestockFarm => inventory.amount(Good::Meat),
            SettlementBuildingKind::FishermansHut => inventory.amount(Good::Food),
            _ => 0,
        })
        .sum::<u32>();
    assert!(
        deposited_food > 0,
        "food must be produced, carried and deposited in its own workplace: {people_states:?}"
    );
    assert!(
        world
            .query::<&CharacterAttributes>()
            .iter(&world)
            .any(|attributes| attributes.physique() > 10),
        "a successful production cycle must train physique even at 100x"
    );
    assert!(
        world
            .query::<&GoodsInventory>()
            .iter(&world)
            .all(|inventory| inventory.used_bulk() <= inventory.bulk_capacity())
    );
}

/// The whole loop, driven by the real systems.
///
/// This is the acceptance test the design was written against, run as code
/// rather than as a person watching: found a settlement, put three people
/// on the map some way off, and let the server do everything else. Nothing
/// here assigns a resident, an occupation or a plot.
///
/// This focused bootstrap fixture runs the production decision systems and
/// `step_units`; the calendar stays in its opening shift and movement omits
/// the full obstacle/path-queue schedule. It verifies voluntary settlement,
/// physical construction supply and observed work, not a complete economy
/// or route-feasibility soak. The 100x companion includes the route planner;
/// connected and town-growth labs include needs, commerce and calendar days.
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
            (arrive_at_settlement, advance_immigration_departures).chain(),
            recount_residents,
            (
                consider_permits,
                moot_services::advance_moot_service_queues,
                moot_services::complete_moot_permit_pickups,
            )
                .chain(),
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
    assert!(
        stores
            .iter()
            .all(|(kind, capacity)| { *capacity == kind.storage_bulk_capacity() })
    );

    // Where the test actually founded, so a failure elsewhere is diagnosable.
    println!("founded at {hall_position:?}, waterline {water}");
    println!("plots {sites:?}");
    println!("delta chunks published: {published}");
    println!("occupations: {titles:?}  ground quality: {qualities:?}");
}
