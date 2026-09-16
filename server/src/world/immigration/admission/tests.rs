use super::*;
use crate::world::dev::VillagerSeed;
use shared::components::SettlementTier;

fn fixture(manual: bool) -> (App, Entity, CoastalVoyage) {
    let terrain = WorldTerrain::default();
    let coast = coastal_voyages(&terrain, 0)[0];
    let mut app = App::new();
    app.insert_resource(terrain)
        .insert_resource(NaturalImmigrationDirector {
            enabled: !manual,
            interval_days: 1.0 / 3.0,
            next_arrival_world_seconds: Some(0.),
            sequence: 0,
            world_npc_cap: 100,
            population_cap_announced: false,
            coastal_approaches: vec![coast],
            settlement_landfalls: default(),
            pending_landfall: None,
            pending_water: None,
            manual_arrivals: u8::from(manual),
            configured: true,
            steady_rate: true,
            deciding: None,
            decision_cursor: None,
            suspended_decisions: default(),
            terrain_revision: None,
        })
        .init_resource::<VillagerSeed>()
        .init_resource::<VesselNavigationQueue>()
        .add_systems(Update, plan_natural_immigration);
    let clock = app.world_mut().spawn(WorldTime::new_default()).id();
    (app, clock, coast)
}
fn town(app: &mut App, coast: CoastalVoyage, id: u64, health: bool) -> Entity {
    let place = coast.landing;
    let entity = app
        .world_mut()
        .spawn((
            SettlementId(id),
            Settlement {
                name: format!("Town{id}"),
                tier: SettlementTier::Hamlet,
                residents: 12,
                treasury: 0,
            },
            PlayerPosition(place),
            PlayerRotation(0.),
            economy(health),
        ))
        .id();
    // This fixture tests admission/choice ownership. The full physical voyage
    // test independently certifies and executes the real land route to a Hall.
    app.world_mut()
        .resource_mut::<NaturalImmigrationDirector>()
        .settlement_landfalls
        .insert(
            entity,
            CachedSettlementLandfall::Reachable {
                entrance: SettlementBuildingKind::Hall.entrance_position(place, 0.),
                voyage: coast,
            },
        );
    entity
}
fn economy(healthy: bool) -> SettlementEconomy {
    if healthy {
        SettlementEconomy {
            reserve_days: 10.,
            housing_capacity: 30,
            private_vacant_jobs: 8,
            prosperity: 100.,
            ..default()
        }
    } else {
        SettlementEconomy {
            reserve_days: 0.,
            housing_capacity: 0,
            homeless_residents: 12,
            job_seekers: 12,
            unrest: 100.,
            ..default()
        }
    }
}

#[test]
fn hungry_towns_are_ranked_without_a_minimum_prosperity_veto() {
    let (mut app, clock, coast) = fixture(false);
    let first = town(&mut app, coast, 1, false);
    let second = town(&mut app, coast, 2, false);
    app.world_mut()
        .get_mut::<SettlementEconomy>(second)
        .unwrap()
        .unrest = 60.;
    app.update(); // The real hull enters before consulting either town.
    let (boat, passenger, seed, entry) = {
        let mut boats = app
            .world_mut()
            .query::<(Entity, &ChoosingSettlement, &PlayerPosition)>();
        let (boat, arrival, position) = boats.single(app.world()).unwrap();
        assert!(arrival.facts.chosen_settlement.is_none());
        (boat, arrival.passenger, arrival.decision_seed, position.0)
    };
    let expected = [first, second]
        .into_iter()
        .map(|entity| {
            let score = settlement_attractiveness(
                app.world().get::<Settlement>(entity).unwrap(),
                app.world().get::<SettlementEconomy>(entity),
                seed,
                entity,
                entry,
                app.world().get::<PlayerPosition>(entity).unwrap().0,
            );
            assert!(
                score < 18.,
                "both towns must fail the retired wealth cutoff"
            );
            (entity, score)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap();
    app.world_mut()
        .get_mut::<WorldTime>(clock)
        .unwrap()
        .advance(1., 1.);
    app.update();
    assert_eq!(
        app.world()
            .resource::<NaturalImmigrationDirector>()
            .pending_water
            .as_ref()
            .map(|pending| pending.choice.entity),
        Some(expected.0),
        "ranking starts on the first decision pass, without an economic wait"
    );
    for _ in 0..128 {
        app.update();
        if app.world().get::<NpcArrivalBoat>(boat).is_some() {
            break;
        }
    }
    let facts = app.world().get::<ImmigrantArrival>(passenger).unwrap();
    assert_eq!(
        facts.chosen_settlement,
        app.world().get::<SettlementId>(expected.0).copied()
    );
    assert_eq!(facts.chosen_score, Some(expected.1));
    assert!(facts.chosen_at.unwrap() > facts.entered_at);
    assert_eq!(app.world().get::<PlayerPosition>(boat).unwrap().0, entry);
    assert!(app.world().get::<VesselRoute>(boat).is_some());
    assert!(app.world().get::<AboardBoat>(passenger).is_some());
}

#[test]
fn an_unreachable_best_town_yields_to_the_next_ranked_town_without_a_day_wait() {
    let (mut app, _, coast) = fixture(false);
    let blocked = town(&mut app, coast, 1, true);
    let reachable = town(&mut app, coast, 2, false);
    // Preserve the admission fixture's certified landfall, but deliberately put
    // the first water goal outside the map. The real water planner must reject it.
    let entrance = SettlementBuildingKind::Hall.entrance_position(coast.landing, 0.);
    app.world_mut()
        .resource_mut::<NaturalImmigrationDirector>()
        .settlement_landfalls
        .insert(
            blocked,
            CachedSettlementLandfall::Reachable {
                entrance,
                voyage: CoastalVoyage {
                    mooring: Vec2::splat(100_000.),
                    ..coast
                },
            },
        );
    app.update();
    let (boat, passenger, entry) = {
        let mut boats = app
            .world_mut()
            .query::<(Entity, &ChoosingSettlement, &PlayerPosition)>();
        let (boat, arrival, position) = boats.single(app.world()).unwrap();
        (boat, arrival.passenger, position.0)
    };
    app.update();
    assert_eq!(
        app.world()
            .resource::<NaturalImmigrationDirector>()
            .pending_water
            .as_ref()
            .unwrap()
            .choice
            .entity,
        blocked
    );
    for _ in 0..128 {
        app.update();
        if app.world().get::<NpcArrivalBoat>(boat).is_some() {
            break;
        }
    }
    assert_eq!(
        app.world()
            .get::<NaturalImmigrantVoyage>(passenger)
            .unwrap()
            .settlement,
        Some(reachable)
    );
    assert_eq!(app.world().get::<PlayerPosition>(boat).unwrap().0, entry);
    assert_eq!(
        app.world_mut()
            .query::<&ImmigrantArrival>()
            .iter(app.world())
            .count(),
        1
    );
}

#[test]
fn a_hard_first_decision_yields_preserved_slices_and_cleans_up_a_disappeared_passenger() {
    let (mut app, _, coast) = fixture(true);
    let hard_town = town(&mut app, coast, 1, false);
    let easy_town = town(&mut app, coast, 2, true);
    app.update();
    let (hard_boat, hard_passenger) = {
        let mut boats = app.world_mut().query::<(Entity, &ChoosingSettlement)>();
        let (boat, arrival) = boats.single(app.world()).unwrap();
        (boat, arrival.passenger)
    };
    app.world_mut()
        .resource_mut::<NaturalImmigrationDirector>()
        .request_manual_arrival();
    app.update();
    let (easy_boat, easy_passenger) = app
        .world_mut()
        .query::<(Entity, &ChoosingSettlement)>()
        .iter(app.world())
        .find(|(boat, _)| *boat != hard_boat)
        .map(|(boat, arrival)| (boat, arrival.passenger))
        .unwrap();
    let hard_choice = SettlementChoice {
        entity: hard_town,
        name: "Pending town".into(),
        position: coast.landing,
        entrance: SettlementBuildingKind::Hall.entrance_position(coast.landing, 0.),
        score: 40.,
    };
    // An existing frontier with many individually invalid landfalls represents
    // a slow decision; the normal one-candidate-per-slice rule is not bypassed.
    let invalid = (0..512)
        .map(|i| CoastalVoyage {
            start: Vec3::splat(100_000. + i as f32),
            yaw: 0.,
            mooring: Vec2::splat(100_000. + i as f32),
            landing: Vec3::splat(100_000. + i as f32),
        })
        .collect::<Vec<_>>();
    {
        let mut director = app.world_mut().resource_mut::<NaturalImmigrationDirector>();
        director.settlement_landfalls.remove(&hard_town);
        director.deciding = Some(hard_boat);
        director.pending_landfall = Some(PendingLandfallSearch::new(&hard_choice, &invalid));
    }
    for _ in 0..128 {
        app.update();
        if app.world().get::<NpcArrivalBoat>(easy_boat).is_some() {
            break;
        }
    }
    assert_eq!(
        app.world()
            .get::<NaturalImmigrantVoyage>(easy_passenger)
            .unwrap()
            .settlement,
        Some(easy_town),
        "the easy second hull must not wait for the first full search"
    );
    {
        let director = app.world().resource::<NaturalImmigrationDirector>();
        let progress = if director.deciding == Some(hard_boat) {
            director.pending_landfall.as_ref()
        } else {
            director
                .suspended_decisions
                .get(&hard_boat)
                .and_then(|state| state.landfall.as_ref())
        }
        .expect("the slow hull must retain its original land frontier");
        assert!(progress.candidate_index > 0 && progress.candidate_index < invalid.len());
    }
    app.world_mut().despawn(hard_passenger);
    for _ in 0..3 {
        app.update();
    }
    let director = app.world().resource::<NaturalImmigrationDirector>();
    assert_ne!(director.deciding, Some(hard_boat));
    assert!(!director.suspended_decisions.contains_key(&hard_boat));
    assert!(app.world().get_entity(hard_boat).is_err());
    assert!(app.world().get::<VesselRoute>(easy_boat).is_some());
}

#[test]
fn real_entry_precedes_current_town_choice_and_retries_keep_the_same_people() {
    let (mut app, clock, coast) = fixture(true);
    let old_best = town(&mut app, coast, 1, true);
    let new_best = town(&mut app, coast, 2, false);
    app.update();
    let (passenger, entry) = {
        let mut q = app.world_mut().query::<(Entity, &ImmigrantArrival)>();
        let (entity, facts) = q.single(app.world()).unwrap();
        assert!(facts.chosen_settlement.is_none());
        assert!(facts.chosen_at.is_none());
        (entity, facts.entry)
    };
    assert!(app.world().get::<AboardBoat>(passenger).is_some());
    assert!(
        app.world()
            .resource::<NaturalImmigrationDirector>()
            .pending_landfall
            .is_none()
    );
    app.world_mut().entity_mut(old_best).insert(economy(false));
    app.world_mut().entity_mut(new_best).insert(economy(true));
    app.world_mut()
        .get_mut::<WorldTime>(clock)
        .unwrap()
        .advance(1., 1.);
    for _ in 0..2_000 {
        app.update();
        if app
            .world()
            .get::<ImmigrantArrival>(passenger)
            .unwrap()
            .chosen_at
            .is_some()
        {
            break;
        }
    }
    let facts = app.world().get::<ImmigrantArrival>(passenger).unwrap();
    assert_eq!(facts.entry, entry);
    assert_eq!(facts.chosen_settlement, Some(SettlementId(2)));
    assert!(facts.chosen_at.unwrap() > facts.entered_at);
    assert!(
        app.world()
            .get::<NaturalImmigrantVoyage>(passenger)
            .unwrap()
            .settlement
            == Some(new_best)
    );
    let count = app
        .world_mut()
        .query_filtered::<Entity, With<CharacterKind>>()
        .iter(app.world())
        .count();
    assert_eq!(
        count, 1,
        "a route retry or choice must not create another immigrant"
    );

    // Deleting a destination must re-use that very body and hull at its real
    // position. A new valid town can then be chosen without a teleport.
    let boat = app
        .world()
        .get::<NaturalImmigrantVoyage>(passenger)
        .unwrap()
        .boat;
    let before = app.world().get::<PlayerPosition>(boat).unwrap().0;
    app.world_mut().despawn(new_best);
    app.world_mut().entity_mut(old_best).insert(economy(true));
    app.add_systems(
        Update,
        finish_natural_immigrant_voyages.after(plan_natural_immigration),
    );
    app.update();
    assert!(app.world().get::<ChoosingSettlement>(boat).is_some());
    assert_eq!(app.world().get::<PlayerPosition>(boat).unwrap().0, before);
    assert!(
        app.world()
            .get::<ImmigrantArrival>(passenger)
            .unwrap()
            .chosen_at
            .is_none()
    );
    for _ in 0..2_000 {
        app.update();
        if app
            .world()
            .get::<ImmigrantArrival>(passenger)
            .unwrap()
            .chosen_at
            .is_some()
        {
            break;
        }
    }
    assert_eq!(
        app.world()
            .get::<NaturalImmigrantVoyage>(passenger)
            .unwrap()
            .boat,
        boat
    );
    assert_eq!(
        app.world()
            .get::<ImmigrantArrival>(passenger)
            .unwrap()
            .chosen_settlement,
        Some(SettlementId(1))
    );
    assert_eq!(app.world().get::<PlayerPosition>(boat).unwrap().0, before);
}

#[test]
fn no_town_waits_in_real_boats_under_the_fleet_cap_without_unbounded_catchup() {
    let (mut app, clock, coast) = fixture(false);
    for day in 0..20 {
        app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = day;
        for _ in 0..20 {
            app.update();
        }
    }
    let boats = app
        .world_mut()
        .query_filtered::<Entity, With<ImmigrantArrivalBoat>>()
        .iter(app.world())
        .count();
    let people = app
        .world_mut()
        .query::<&ImmigrantArrival>()
        .iter(app.world())
        .count();
    assert_eq!(boats, MAX_ACTIVE_VOYAGES);
    assert_eq!(people, MAX_ACTIVE_VOYAGES);
    assert!(
        app.world_mut()
            .query::<&ImmigrantArrival>()
            .iter(app.world())
            .all(|facts| facts.chosen_settlement.is_none())
    );
    assert!(
        app.world_mut()
            .query::<(&VillagerIntent, &AboardBoat)>()
            .iter(app.world())
            .all(|(intent, _)| matches!(intent, VillagerIntent::Idle))
    );
    town(&mut app, coast, 1, true);
    app.world_mut().get_mut::<WorldTime>(clock).unwrap().day += 1;
    for _ in 0..2_000 {
        app.update();
        if app
            .world_mut()
            .query::<&ImmigrantArrival>()
            .iter(app.world())
            .all(|facts| facts.chosen_at.is_some())
        {
            break;
        }
    }
    assert!(
        app.world_mut()
            .query::<&ImmigrantArrival>()
            .iter(app.world())
            .all(|facts| facts.chosen_settlement == Some(SettlementId(1)))
    );
    assert_eq!(
        app.world_mut()
            .query::<&ImmigrantArrival>()
            .iter(app.world())
            .count(),
        MAX_ACTIVE_VOYAGES
    );
}

#[test]
fn a_retained_destination_search_does_not_delay_the_next_world_entry() {
    let (mut app, clock, coast) = fixture(false);
    let hall = town(&mut app, coast, 1, true);
    app.update();
    let (boat, entry) = {
        let mut boats = app
            .world_mut()
            .query::<(Entity, &ChoosingSettlement, &PlayerPosition)>();
        let (boat, _, position) = boats.single(app.world()).unwrap();
        (boat, position.0)
    };
    let place = app.world().get::<PlayerPosition>(hall).unwrap().0;
    let choice = SettlementChoice {
        entity: hall,
        name: "Town1".into(),
        position: place,
        entrance: SettlementBuildingKind::Hall.entrance_position(place, 0.),
        score: 40.,
    };
    let search =
        app.world_mut()
            .resource_scope(|world, mut navigation: Mut<VesselNavigationQueue>| {
                navigation
                    .cache
                    .begin(world.resource::<WorldTerrain>(), entry.xz(), coast.mooring)
            });
    {
        let mut director = app.world_mut().resource_mut::<NaturalImmigrationDirector>();
        director.deciding = Some(boat);
        director.pending_water = Some(PendingWaterVoyage {
            choice,
            entry: CoastalVoyage {
                start: entry,
                ..coast
            },
            voyage: coast,
            search,
        });
    }
    let due = app
        .world()
        .resource::<NaturalImmigrationDirector>()
        .next_arrival_world_seconds
        .unwrap();
    {
        let mut clock = app.world_mut().get_mut::<WorldTime>(clock).unwrap();
        // Deadlines retain f64 precision, while the replicated clock is f32.
        // Casting alone can round just below the actual admission deadline.
        clock.seconds_in_cycle = (due as f32).next_up();
        assert!(absolute_world_seconds(&clock) >= due);
    }
    app.update();
    assert_eq!(
        app.world_mut()
            .query::<&ImmigrantArrival>()
            .iter(app.world())
            .count(),
        2,
        "an existing retained route must not postpone the world-wide arrival cadence"
    );
    let director = app.world().resource::<NaturalImmigrationDirector>();
    assert_eq!(director.deciding, Some(boat));
    assert!(
        director.pending_water.is_some(),
        "admitting a new body preserves the older search"
    );
    assert_eq!(app.world().get::<PlayerPosition>(boat).unwrap().0, entry);
    assert!(
        app.world_mut()
            .query::<&ImmigrantArrival>()
            .iter(app.world())
            .all(|facts| facts.chosen_at.is_none())
    );
}

/// Reproduce the actual second entry from the connected seed-91 failure. This
/// isolates geometric planning from town-economy changes and prints retained
/// search progress rather than treating an invisible timer as a successful trip.
#[test]
#[ignore = "real seed-91 island water/land planning diagnostic"]
fn seed_91_second_world_entry_finishes_its_geometric_decision() {
    use crate::world::start_config::WorldStartConfig;
    let config = WorldStartConfig::from_ron(include_str!(
        "../../../../../config/worlds/small-frontier.ron"
    ))
    .unwrap();
    let terrain =
        WorldTerrain::from_loaded_map(shared::map::load_session_map(&config.recipe(91)).unwrap());
    let entry = Vec3::new(223.786865, 0., -1564.651245);
    let town = Vec3::new(-31.975101, 1.757709, -708.743713);
    let choice = SettlementChoice {
        entity: Entity::PLACEHOLDER,
        name: "Eldermead".into(),
        position: town,
        entrance: SettlementBuildingKind::Hall.entrance_position(town, 0.),
        score: 40.,
    };
    let coasts = coastal_voyages(&terrain, 0);
    let mut land = PendingLandfallSearch::new(&choice, &coasts);
    let mut landing = None;
    for slice in 0..20_000 {
        match land.advance(&terrain) {
            PendingLandfallResult::Pending => {}
            PendingLandfallResult::Unreachable => panic!("seed-91 chosen town has no dry landing"),
            PendingLandfallResult::Reachable(voyage) => {
                landing = Some(voyage);
                break;
            }
        }
        if slice % 1_000 == 0 {
            eprintln!("IMMIGRATION_LAND slice={slice} state={land:?}");
        }
    }
    let landing = landing.expect("landfall must finish within bounded retained slices");
    let mut navigation = VesselNavigationQueue::default();
    let mut search = navigation
        .cache
        .begin(&terrain, entry.xz(), landing.mooring);
    for slice in 0..40_000 {
        match navigation.cache.advance(&mut search, &terrain) {
            WaterPlanResult::Pending => {}
            WaterPlanResult::Complete(route) => {
                eprintln!(
                    "IMMIGRATION_WATER complete slice={slice} route_points={:?} start={entry:?} landing={landing:?} progress={:?}",
                    route.as_ref().map(Vec::len),
                    search.progress()
                );
                return;
            }
        }
        if slice % 1_000 == 0 {
            eprintln!(
                "IMMIGRATION_WATER slice={slice} landing={landing:?} progress={:?}",
                search.progress()
            );
        }
    }
    panic!(
        "water decision failed to terminate: {:?}",
        search.progress()
    );
}

#[test]
#[ignore = "real seed-91 cross-island full-hull navigation diagnostic"]
fn seed_91_second_entry_reaches_fernhaven_with_certified_water_segments() {
    use crate::world::start_config::WorldStartConfig;
    let config = WorldStartConfig::from_ron(include_str!(
        "../../../../../config/worlds/small-frontier.ron"
    ))
    .unwrap();
    let terrain =
        WorldTerrain::from_loaded_map(shared::map::load_session_map(&config.recipe(91)).unwrap());
    let start = Vec2::new(223.786865, -1564.651245);
    let goal = Vec2::new(-1183.029053, 546.786865);
    let mut navigation = VesselNavigationQueue::default();
    let mut search = navigation.cache.begin(&terrain, start, goal);
    for slice in 0..40_000 {
        match navigation.cache.advance(&mut search, &terrain) {
            WaterPlanResult::Pending => {}
            WaterPlanResult::Complete(route) => {
                let route = route.expect("real ocean route to Fernhaven must exist");
                let mut previous = start;
                for point in &route {
                    assert!(navigation.cache.geometry.segment_clear(
                        &terrain,
                        previous,
                        *point,
                        crate::player::boat::clearance::WatercraftClearance::DINGHY
                    ));
                    previous = *point;
                }
                assert_eq!(previous, goal);
                eprintln!(
                    "FERNHAVEN_WATER complete slice={slice} grid={} points={} progress={:?}",
                    search.grid_metres(),
                    route.len(),
                    search.progress()
                );
                return;
            }
        }
        if slice % 1_000 == 0 {
            eprintln!(
                "FERNHAVEN_WATER slice={slice} grid={} progress={:?}",
                search.grid_metres(),
                search.progress()
            );
        }
    }
    panic!(
        "cross-island retained search failed to finish: {:?}",
        search.progress()
    );
}

#[test]
fn a_completed_landfall_keeps_its_selected_town_when_attractiveness_changes() {
    for reuse_another_hulls_proof in [false, true] {
        let (mut app, clock, _) = fixture(true);
        let (coast, entrance) = {
            let terrain = app.world().resource::<WorldTerrain>();
            coastal_voyages(terrain, 0)
                .into_iter()
                .find_map(|coast| {
                    let inward = (coast.landing.xz() - coast.mooring).normalize_or_zero();
                    let goal = coast.landing.xz() + inward * 16.;
                    overland_trade_corridor_exists(terrain, coast.landing.xz(), goal).then(|| {
                        (
                            coast,
                            Vec3::new(goal.x, terrain.get_height(goal.x, goal.y), goal.y),
                        )
                    })
                })
                .expect("the shipped coast must have a short genuinely walkable landing")
        };
        let hall = entrance - SettlementBuildingKind::Hall.entrance_position(Vec3::ZERO, 0.);
        app.world_mut()
            .resource_mut::<NaturalImmigrationDirector>()
            .coastal_approaches = vec![coast];
        let selected = town(&mut app, coast, 1, true);
        let newly_attractive = town(&mut app, coast, 2, false);
        for entity in [selected, newly_attractive] {
            app.world_mut().get_mut::<PlayerPosition>(entity).unwrap().0 = hall;
        }
        // Both towns start without a cached proof; the real director first
        // chooses one opportunity and creates its retained land frontier.
        app.world_mut()
            .resource_mut::<NaturalImmigrationDirector>()
            .settlement_landfalls
            .clear();
        app.update();
        let (boat, passenger, entry) = {
            let mut boats = app
                .world_mut()
                .query::<(Entity, &ChoosingSettlement, &PlayerPosition)>();
            let (boat, arrival, position) = boats.single(app.world()).unwrap();
            (boat, arrival.passenger, position.0)
        };
        app.world_mut()
            .get_mut::<WorldTime>(clock)
            .unwrap()
            .advance(1., 1.);
        app.update();
        let choice = app
            .world()
            .resource::<NaturalImmigrationDirector>()
            .pending_landfall
            .as_ref()
            .unwrap()
            .choice
            .clone();
        assert_eq!(choice.entity, selected);
        app.world_mut().entity_mut(selected).insert(economy(false));
        app.world_mut()
            .entity_mut(newly_attractive)
            .insert(economy(true));
        if reuse_another_hulls_proof {
            // The shared cache is populated only after independently checking
            // the same real land corridor above; another hull can finish first.
            app.world_mut()
                .resource_mut::<NaturalImmigrationDirector>()
                .settlement_landfalls
                .insert(
                    selected,
                    CachedSettlementLandfall::Reachable {
                        entrance,
                        voyage: coast,
                    },
                );
        }
        for _ in 0..512 {
            app.update();
            if app
                .world()
                .resource::<NaturalImmigrationDirector>()
                .pending_water
                .is_some()
            {
                break;
            }
        }
        let director = app.world().resource::<NaturalImmigrationDirector>();
        let water = director
            .pending_water
            .as_ref()
            .expect("completed land proof must start water proof");
        assert!(director.pending_landfall.is_none());
        assert_eq!(water.choice.entity, selected);
        assert_eq!(water.choice.name, choice.name);
        assert_eq!(water.choice.position, choice.position);
        assert_eq!(water.choice.entrance, choice.entrance);
        assert_eq!(water.choice.score, choice.score);
        assert_eq!(water.entry.start, entry);
        assert_eq!(water.voyage.landing, coast.landing);
        for _ in 0..128 {
            app.update();
            if app.world().get::<NpcArrivalBoat>(boat).is_some() {
                break;
            }
        }
        let arrival = app
            .world()
            .get::<NpcArrivalBoat>(boat)
            .expect("the matching water proof must commit");
        assert_eq!(arrival.settlement, selected);
        assert_eq!(
            app.world()
                .get::<ImmigrantArrival>(passenger)
                .unwrap()
                .chosen_score,
            Some(choice.score)
        );
        assert_eq!(app.world().get::<PlayerPosition>(boat).unwrap().0, entry);
    }
}

#[test]
fn pending_land_and_water_choices_revalidate_the_actual_hall_before_committing() {
    for water_phase in [false, true] {
        for invalidation in ["despawn", "ruins", "moved_entrance"] {
            let (mut app, _, coast) = fixture(true);
            let selected = town(&mut app, coast, 1, true);
            if !water_phase {
                app.world_mut()
                    .resource_mut::<NaturalImmigrationDirector>()
                    .settlement_landfalls
                    .clear();
            }
            app.update();
            let (boat, passenger, entry) = {
                let mut boats = app
                    .world_mut()
                    .query::<(Entity, &ChoosingSettlement, &PlayerPosition)>();
                let (boat, arrival, position) = boats.single(app.world()).unwrap();
                (boat, arrival.passenger, position.0)
            };
            app.update();
            let director = app.world().resource::<NaturalImmigrationDirector>();
            assert_eq!(director.pending_water.is_some(), water_phase);
            assert_eq!(director.pending_landfall.is_some(), !water_phase);
            match invalidation {
                "despawn" => {
                    app.world_mut().despawn(selected);
                }
                "ruins" => {
                    app.world_mut()
                        .get_mut::<Settlement>(selected)
                        .unwrap()
                        .tier = SettlementTier::Ruins
                }
                "moved_entrance" => {
                    app.world_mut()
                        .get_mut::<PlayerPosition>(selected)
                        .unwrap()
                        .0
                        .x += 4.
                }
                _ => unreachable!(),
            }
            app.update();
            let director = app.world().resource::<NaturalImmigrationDirector>();
            assert!(director.pending_landfall.is_none() && director.pending_water.is_none());
            assert!(app.world().get::<NpcArrivalBoat>(boat).is_none());
            let waiting = app.world().get::<ChoosingSettlement>(boat).unwrap();
            assert!(waiting.rejected_towns.contains(&selected));
            assert_eq!(waiting.passenger, passenger);
            assert_eq!(app.world().get::<PlayerPosition>(boat).unwrap().0, entry);
            assert!(
                app.world()
                    .get::<ImmigrantArrival>(passenger)
                    .unwrap()
                    .chosen_at
                    .is_none()
            );
        }
    }
}
