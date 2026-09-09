//! Short, inspectable growth runs through the real village schedule.
//!
//! Only initial people, migration timing and the charter seed are controlled.
//! Permits, wages, materials, construction, roads and admission remain live.

use super::*;
use shared::components::{
    BuildingOf, CivicHallLevel, CivicHallUpgradeWorksite, FortificationSegment, HouseAppearance,
    LivestockPasture, MarketLevel, RoadOf, SettlementDefenses, SettlementDevelopment,
};
use shared::settlement_snapshot::*;
use shared::terrain::TerrainDeltaChunk;

use crate::world::village_lab_scenario::{
    choose_town_growth_site, town_growth_seed, GrowthProfile, TOWN_GROWTH_FOUNDERS,
};

fn percentile(values: &[f64], fraction: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted[((sorted.len() - 1) as f64 * fraction).ceil() as usize]
}

fn building_record(
    id: Option<BuildingId>,
    settlement_id: SettlementId,
    kind: SettlementBuildingKind,
    position: Vec3,
    rotation: f32,
    house: Option<HouseAppearance>,
    market_level: Option<MarketLevel>,
    construction: Option<shared::components::ConstructionSite>,
    civic_upgrade: Option<CivicHallUpgradeWorksite>,
    inventory: Option<GoodsInventory>,
    quality: f32,
) -> SnapshotBuilding {
    let definition = kind.placement_definition();
    SnapshotBuilding {
        id,
        settlement_id,
        kind,
        position,
        rotation,
        house,
        market_level,
        construction,
        civic_upgrade,
        inventory,
        quality,
        footprint: definition.footprint,
        footprint_center: definition.world_footprint_center(position, rotation),
        door: kind.entrance_position(position, rotation),
    }
}

fn snapshot(
    world: &mut World,
    profile: &str,
    seed: u64,
    elapsed: f32,
    arrivals: usize,
    updates: &[f64],
) -> TownSnapshot {
    let mut settlements: Vec<_> = world
        .query::<(
            &SettlementId,
            &Settlement,
            &PlayerPosition,
            &SettlementDevelopment,
            Option<&PlayerRotation>,
            Option<&CivicHallLevel>,
            Option<&SettlementDefenses>,
            Option<&shared::components::SettlementCivicSquare>,
        )>()
        .iter(world)
        .map(
            |(id, town, at, development, rotation, level, defenses, square)| {
                let hall_level = level
                    .copied()
                    .unwrap_or_else(|| CivicHallLevel::for_tier(town.tier));
                let rotation = rotation.map_or(0.0, |value| value.0);
                let definition = hall_level.building_type().definition();
                SnapshotSettlement {
                    id: *id,
                    name: town.name.clone(),
                    tier: town.tier,
                    residents: town.residents,
                    treasury: town.treasury,
                    position: at.0,
                    rotation,
                    development: development.clone(),
                    hall_level,
                    footprint: definition.footprint,
                    footprint_center: definition.world_footprint_center(at.0, rotation),
                    defenses: defenses.cloned(),
                    civic_square: square.cloned(),
                }
            },
        )
        .collect();
    settlements.sort_by_key(|town| town.id.0);
    let mut districts: Vec<_> = world
        .query::<(&SettlementId, &village::SettlementUrbanPlan)>()
        .iter(world)
        .flat_map(|(id, plan)| {
            plan.wards.iter().map(move |ward| SnapshotDistrict {
                settlement_id: *id,
                id: u32::from(ward.id),
                center: ward.center,
                axis: ward.axis,
                half_extents: ward.half_extents,
                seed: ward.seed,
            })
        })
        .collect();
    districts.sort_by_key(|district| (district.settlement_id.0, district.id));
    let mut fortifications: Vec<_> = world
        .query::<&FortificationSegment>()
        .iter(world)
        .cloned()
        .collect();
    fortifications.sort_by(|a, b| {
        a.settlement_id
            .0
            .cmp(&b.settlement_id.0)
            .then_with(|| a.circuit.cmp(&b.circuit))
            .then_with(|| a.start.x.total_cmp(&b.start.x))
            .then_with(|| a.start.z.total_cmp(&b.start.z))
    });
    let mut buildings: Vec<_> = world
        .query::<(
            &SettlementBuilding,
            &BuildingOf,
            &PlayerPosition,
            Option<&PlayerRotation>,
            Option<&BuildingId>,
            Option<&HouseAppearance>,
            Option<&MarketLevel>,
            Option<&GoodsInventory>,
        )>()
        .iter(world)
        .map(
            |(building, town, at, rotation, id, house, market, inventory)| {
                building_record(
                    id.copied(),
                    town.0,
                    building.kind,
                    at.0,
                    rotation.map_or(0.0, |r| r.0),
                    house.copied(),
                    market.copied(),
                    None,
                    None,
                    inventory.cloned(),
                    building.quality,
                )
            },
        )
        .collect();
    buildings.extend(
        world
            .query::<(
                &shared::components::ConstructionSite,
                &PlayerPosition,
                Option<&UnderConstruction>,
                Option<&BuildingOf>,
                Option<&BuildingId>,
                Option<&HouseAppearance>,
                Option<&CivicHallUpgradeWorksite>,
                Option<&GoodsInventory>,
            )>()
            .iter(world)
            .map(|(site, at, under, town, id, house, upgrade, inventory)| {
                let settlement_id = under
                    .map(|under| under.settlement_id)
                    .or_else(|| town.map(|town| town.0))
                    .expect("every worksite belongs to a settlement");
                building_record(
                    id.copied(),
                    settlement_id,
                    site.kind,
                    at.0,
                    site.rotation,
                    house.copied(),
                    None,
                    Some(site.clone()),
                    upgrade.copied(),
                    inventory.cloned(),
                    under.map_or(0.5, |under| under.quality),
                )
            }),
    );
    buildings.sort_by(|a, b| {
        a.settlement_id
            .0
            .cmp(&b.settlement_id.0)
            .then_with(|| a.position.x.total_cmp(&b.position.x))
            .then_with(|| a.position.z.total_cmp(&b.position.z))
    });
    let mut roads: Vec<_> = world
        .query::<(&VillageRoad, &RoadOf)>()
        .iter(world)
        .map(|(road, town)| SnapshotRoad {
            settlement_id: town.0,
            road: road.clone(),
        })
        .collect();
    roads.sort_by(|a, b| {
        a.settlement_id.0.cmp(&b.settlement_id.0).then_with(|| {
            let a = a.road.points.first().copied().unwrap_or_default();
            let b = b.road.points.first().copied().unwrap_or_default();
            a.x.total_cmp(&b.x).then_with(|| a.y.total_cmp(&b.y))
        })
    });
    let fields = world
        .query::<(&FarmField, &PlayerPosition, &PlayerRotation)>()
        .iter(world)
        .map(|(component, at, rotation)| SnapshotField {
            component: component.clone(),
            position: at.0,
            rotation: rotation.0,
            footprint: SettlementBuildingKind::Farmstead
                .field_half_extents()
                .unwrap()
                * 2.0,
            footprint_center: at.0.xz(),
        })
        .collect();
    let pastures = world
        .query::<(&LivestockPasture, &PlayerPosition, &PlayerRotation)>()
        .iter(world)
        .map(|(component, at, rotation)| SnapshotPasture {
            component: component.clone(),
            position: at.0,
            rotation: rotation.0,
            footprint: SettlementBuildingKind::LivestockFarm
                .pasture_half_extents()
                .unwrap()
                * 2.0,
            footprint_center: at.0.xz(),
        })
        .collect();
    let piers = world
        .query::<(&FishingPier, &PlayerPosition, &PlayerRotation)>()
        .iter(world)
        .map(|(component, at, rotation)| SnapshotPier {
            component: component.clone(),
            position: at.0,
            rotation: rotation.0,
        })
        .collect();
    let housed = world
        .query::<&Household>()
        .iter(world)
        .map(|home| home.resident_ids.len())
        .sum();
    let clock = world
        .query::<&WorldTime>()
        .iter(world)
        .next()
        .expect("lab clock");
    let day = clock.day;
    let seconds_in_cycle = clock.seconds_in_cycle;
    let terrain = world.resource::<WorldTerrain>();
    let mut terrain_deltas: Vec<_> = terrain
        .delta_chunks()
        .iter()
        .map(|(coord, data)| TerrainDeltaChunk::from_delta_data(*coord, data))
        .collect();
    terrain_deltas.sort_by_key(|chunk| (chunk.coord.x, chunk.coord.z));
    let mut output = TownSnapshot {
        version: TOWN_SNAPSHOT_VERSION,
        map_id: terrain.generator.active_map_id().into(),
        map_content_hash: terrain.generator.active_map_content_hash(),
        profile: profile.into(),
        seed,
        elapsed_world_seconds: elapsed,
        day,
        seconds_in_cycle,
        settlements,
        buildings,
        roads,
        fields,
        pastures,
        piers,
        districts,
        fortifications,
        terrain_deltas,
        metrics: TownMetrics::default(),
    };
    let diagnostics = world.resource::<village::PermitPlanningDiagnostics>();
    let planning: Vec<_> = diagnostics
        .primary_site_milliseconds
        .iter()
        .chain(&diagnostics.fishing_site_milliseconds)
        .chain(&diagnostics.final_access_milliseconds)
        .copied()
        .collect();
    output.metrics = measure(&output, housed, arrivals, &planning, updates);
    let economies: Vec<_> = world.query::<&SettlementEconomy>().iter(world).collect();
    let metrics = &mut output.metrics;
    metrics.housing_capacity = economies
        .iter()
        .map(|economy| economy.housing_capacity as usize)
        .sum();
    metrics.unhoused = (metrics.residents as usize).saturating_sub(housed);
    metrics.food_inventory = economies
        .iter()
        .map(|economy| u64::from(economy.edible_stock))
        .sum();
    metrics.recent_food_production = economies
        .iter()
        .map(|economy| economy.recent_food_production)
        .sum();
    metrics.recent_food_consumption = economies
        .iter()
        .map(|economy| economy.recent_food_consumption)
        .sum();
    metrics.unmet_food = economies.iter().map(|economy| economy.unmet_food).sum();
    let count = economies.len().max(1) as f32;
    metrics.food_reserve_days = economies
        .iter()
        .map(|economy| economy.reserve_days)
        .sum::<f32>()
        / count;
    metrics.mean_prosperity = economies
        .iter()
        .map(|economy| economy.prosperity)
        .sum::<f32>()
        / count;
    output
}

/// Snapshot-only geometric diagnostic. Pairwise work is bounded by exported
/// homes and does not add a per-person/per-tick runtime graph.
fn residential_cluster_sizes(positions: &[Vec2]) -> Vec<usize> {
    let mut visited = vec![false; positions.len()];
    let mut sizes = Vec::new();
    for first in 0..positions.len() {
        if visited[first] {
            continue;
        }
        visited[first] = true;
        let mut frontier = vec![first];
        let mut size = 0;
        while let Some(index) = frontier.pop() {
            size += 1;
            for next in 0..positions.len() {
                if !visited[next]
                    && positions[index].distance_squared(positions[next]) <= 30.0_f32.powi(2)
                {
                    visited[next] = true;
                    frontier.push(next);
                }
            }
        }
        sizes.push(size);
    }
    sizes
}

fn measure(
    state: &TownSnapshot,
    housed: usize,
    arrivals: usize,
    planning: &[f64],
    updates: &[f64],
) -> TownMetrics {
    let complete: Vec<_> = state
        .buildings
        .iter()
        .filter(|building| building.construction.is_none())
        .collect();
    let houses: Vec<_> = complete
        .iter()
        .filter(|building| building.kind == SettlementBuildingKind::House)
        .collect();
    let distances: Vec<f32> = houses
        .iter()
        .enumerate()
        .filter_map(|(index, house)| {
            houses
                .iter()
                .enumerate()
                .filter(|(other, _)| *other != index)
                .map(|(_, other)| house.position.xz().distance(other.position.xz()))
                .min_by(f32::total_cmp)
        })
        .collect();
    let mut metrics = TownMetrics {
        residents: state.settlements.iter().map(|town| town.residents).sum(),
        arrivals_spawned: arrivals,
        housed,
        completed_buildings: complete.len(),
        pending_buildings: state.buildings.len() - complete.len(),
        completed_roads: state
            .roads
            .iter()
            .filter(|entry| entry.road.is_complete())
            .count(),
        pending_roads: state
            .roads
            .iter()
            .filter(|entry| !entry.road.is_complete())
            .count(),
        road_length: state
            .roads
            .iter()
            .map(|entry| {
                entry
                    .road
                    .built_points()
                    .windows(2)
                    .map(|pair| pair[0].distance(pair[1]))
                    .sum::<f32>()
            })
            .sum(),
        houses: houses.len(),
        houses_with_neighbor: distances
            .iter()
            .filter(|distance| **distance <= 18.0)
            .count(),
        mean_house_neighbor_distance: (!distances.is_empty())
            .then(|| distances.iter().sum::<f32>() / distances.len() as f32),
        permit_planning_p95_ms: percentile(planning, 0.95),
        permit_planning_max_ms: percentile(planning, 1.0),
        update_p95_ms: percentile(updates, 0.95),
        update_max_ms: percentile(updates, 1.0),
        ..Default::default()
    };
    for town in &state.settlements {
        let positions: Vec<_> = houses
            .iter()
            .filter(|house| house.settlement_id == town.id)
            .map(|house| house.position.xz())
            .collect();
        let clusters = residential_cluster_sizes(&positions);
        metrics.residential_clusters += clusters.len();
        metrics.largest_residential_cluster = metrics
            .largest_residential_cluster
            .max(clusters.into_iter().max().unwrap_or(0));
        metrics.outermost_house_distance = positions
            .iter()
            .map(|position| position.distance(town.position.xz()))
            .fold(metrics.outermost_house_distance, f32::max);
        let roads: Vec<_> = state
            .roads
            .iter()
            .filter(|entry| entry.settlement_id == town.id)
            .map(|entry| &entry.road)
            .collect();
        let hall_door = SettlementBuildingKind::Hall
            .entrance_position(town.position, town.rotation)
            .xz();
        let connected = village_roads::hall_connected_road_keys(hall_door, &roads);
        for building in complete
            .iter()
            .filter(|building| building.settlement_id == town.id)
        {
            let touching: Vec<_> = roads
                .iter()
                .filter(|road| {
                    road.points.first().is_some_and(|point| {
                        point.distance_squared(building.door.xz()) <= 0.75_f32.powi(2)
                    })
                })
                .collect();
            if touching.is_empty() {
                metrics.roadless_buildings += 1;
            } else if touching.iter().any(|road| road.is_complete())
                && !touching.iter().any(|road| {
                    road.is_complete()
                        && road
                            .built_points()
                            .iter()
                            .any(|point| connected.contains(&village_roads::road_point_key(*point)))
                })
            {
                metrics.disconnected_buildings += 1;
            }
        }
    }
    for (index, a) in complete.iter().enumerate() {
        if !matches!(
            a.kind,
            SettlementBuildingKind::Farmstead
                | SettlementBuildingKind::LivestockFarm
                | SettlementBuildingKind::LumberjackHut
                | SettlementBuildingKind::StoneQuarry
                | SettlementBuildingKind::FishermansHut
        ) {
            continue;
        }
        metrics.same_resource_neighbor_pairs += complete
            .iter()
            .skip(index + 1)
            .filter(|b| {
                a.kind == b.kind
                    && a.position.xz().distance_squared(b.position.xz()) <= 48.0_f32.powi(2)
            })
            .count();
    }
    metrics
}

/// Export bounded, real growth separately from the long lab's acceptance gates.
#[test]
#[ignore = "run explicitly with cargo town-growth-lab"]
fn town_growth_lab() {
    std::env::set_var("CITYSIM_MAP_ID", "village_lab");
    let profile_name = std::env::var("FISTWORLD_TOWN_PROFILE").unwrap_or_else(|_| "steady".into());
    let profile = GrowthProfile::parse(&profile_name);
    let seed = town_growth_seed();
    let minutes = env_f32("FISTWORLD_TOWN_MINUTES", profile.default_minutes()).clamp(1.0, 2_880.0);
    let warp = env_f32("FISTWORLD_TOWN_WARP", 25.0).clamp(1.0, 1_000.0);
    let interval = env_f32("FISTWORLD_TOWN_SNAPSHOT_MINUTES", 20.0).max(1.0) * 60.0;
    let directory = std::path::PathBuf::from(
        std::env::var("FISTWORLD_TOWN_OUTPUT")
            .unwrap_or_else(|_| format!("logs/town-growth/{seed}-{profile_name}")),
    );
    std::fs::create_dir_all(&directory).expect("create ignored growth output directory");
    let mut app = App::new();
    configure_lab(&mut app);
    app.insert_resource(WorldTerrain::default());
    let (hall, trees, farmland) = choose_town_growth_site(app.world().resource::<WorldTerrain>());
    println!(
        "TOWN inland site x={} z={} farmland={} trees={}",
        hall.x, hall.z, farmland, trees
    );
    let settlement = spawn_lab_village(
        app.world_mut(),
        "Lab Meadow",
        "SecureResident",
        CivicStrategy::Balanced,
        hall,
        TOWN_GROWTH_FOUNDERS,
        SettlementTier::Hamlet,
    );
    let development = SettlementDevelopment::from_seed(seed, 0);
    app.world_mut().entity_mut(settlement).insert(development);
    app.world_mut()
        .spawn((WorldTime::new_default(), TimeWarp::clamped(warp)));
    let initial_money = total_money(app.world_mut());
    let waves = profile.waves();
    println!(
        "TOWN offered_population={} minutes={} warp={} last_arrival_scenario_day={}",
        profile.target_population(),
        minutes,
        warp,
        waves.last().map_or(1, |wave| wave.day)
    );
    let mut next_wave = 0;
    let mut arrivals = 0;
    let step = warp / 60.0;
    let ticks = (minutes * 60.0 / step).ceil() as usize;
    let mut updates = Vec::with_capacity(ticks);
    let mut next_capture = 0.0;
    let mut capture_index = 0;
    let mut accepted = HashMap::<BuildingId, (SettlementBuildingKind, Vec3, f32)>::new();
    for tick in 0..ticks {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(1.0 / 60.0));
        let started = Instant::now();
        app.update();
        updates.push(started.elapsed().as_secs_f64() * 1_000.0);
        let day = app
            .world_mut()
            .query::<&WorldTime>()
            .iter(app.world())
            .next()
            .unwrap()
            .day;
        while let Some(wave) = waves.get(next_wave) {
            if day < wave.day.saturating_sub(1) {
                break;
            }
            spawn_lab_arrivals(app.world_mut(), hall, wave.day, wave.count, wave.target);
            arrivals += wave.count;
            next_wave += 1;
        }
        let elapsed = (tick + 1) as f32 * step;
        if elapsed >= next_capture || tick + 1 == ticks {
            let state = snapshot(
                app.world_mut(),
                &profile_name,
                seed,
                elapsed,
                arrivals,
                &updates,
            );
            assert!(
                state.metrics.residents as usize <= TOWN_GROWTH_FOUNDERS + arrivals,
                "growth created phantom residents"
            );
            assert!(
                state.metrics.housed <= state.metrics.residents as usize,
                "housing has ghost residents"
            );
            assert_accepted_geometry(&state, &mut accepted);
            assert_eq!(
                total_money(app.world_mut()),
                initial_money + arrivals as u64 * shared::economy::STARTING_VILLAGER_MONEY,
                "real growth created or destroyed money"
            );
            let path = directory.join(format!("snapshot-{capture_index:04}.json"));
            state
                .write(&path)
                .expect("write actual accepted town snapshot");
            println!("TOWN snapshot={} elapsed={:.1}m day={} seed={} profile={} residents={} housed={} buildings={}/{} roads={}/{} neighbors={}/{} planning_p95={:.2}ms", path.display(), elapsed / 60.0, state.day, seed, profile_name, state.metrics.residents, state.metrics.housed, state.metrics.completed_buildings, state.metrics.pending_buildings, state.metrics.completed_roads, state.metrics.pending_roads, state.metrics.houses_with_neighbor, state.metrics.houses, state.metrics.permit_planning_p95_ms);
            for town in &state.settlements {
                println!("TOWN development id={} tier={:?} gate={:?} progress={}/{} prosperity={:.1} food_stock={} produced={:.1}/day consumed={:.1}/day unmet={} reserve={:.1}days clusters={} largest_cluster={} radius={:.1}m", town.id.0, town.tier, town.development.next_gate, town.development.progress_days, town.development.required_days, state.metrics.mean_prosperity, state.metrics.food_inventory, state.metrics.recent_food_production, state.metrics.recent_food_consumption, state.metrics.unmet_food, state.metrics.food_reserve_days, state.metrics.residential_clusters, state.metrics.largest_residential_cluster, state.metrics.outermost_house_distance);
            }
            print_report(app.world_mut(), elapsed, false);
            capture_index += 1;
            next_capture = elapsed + interval;
        }
    }
    let state = snapshot(
        app.world_mut(),
        &profile_name,
        seed,
        ticks as f32 * step,
        arrivals,
        &updates,
    );
    if minutes >= 60.0 {
        assert!(
            state.metrics.completed_buildings > 0,
            "growth never completed a building"
        );
    }
    println!("TOWN complete snapshots={capture_index} updates={ticks} update_p95={:.2}ms update_max={:.2}ms", state.metrics.update_p95_ms, state.metrics.update_max_ms);
    print_business_report(app.world_mut());
    print_structure_report(app.world_mut());
    print_resource_flow_report(app.world_mut());
}

fn assert_accepted_geometry(
    state: &TownSnapshot,
    accepted: &mut HashMap<BuildingId, (SettlementBuildingKind, Vec3, f32)>,
) {
    for building in state
        .buildings
        .iter()
        .filter(|building| building.construction.is_none())
    {
        if let Some(id) = building.id {
            let current = (building.kind, building.position, building.rotation);
            if let Some(previous) = accepted.insert(id, current) {
                assert_eq!(
                    previous, current,
                    "growth moved an accepted building {id:?}"
                );
            }
        }
    }
    for id in accepted.keys() {
        assert!(
            state
                .buildings
                .iter()
                .any(|building| building.id == Some(*id)),
            "growth removed accepted building {id:?}"
        );
    }
    let plots: Vec<_> = state
        .buildings
        .iter()
        .filter(|building| building.civic_upgrade.is_none())
        .collect();
    for (index, a) in plots.iter().enumerate() {
        for b in plots.iter().skip(index + 1) {
            assert!(
                !rectangles_overlap(a, b),
                "accepted {:?} at {:?} overlaps {:?} at {:?}",
                a.kind,
                a.position,
                b.kind,
                b.position
            );
        }
    }
}

fn rectangles_overlap(a: &SnapshotBuilding, b: &SnapshotBuilding) -> bool {
    let axes = |angle| {
        [
            shared::rotation::local_to_world_xz(Vec2::X, angle),
            shared::rotation::local_to_world_xz(Vec2::Y, angle),
        ]
    };
    let a_axes = axes(a.rotation);
    let b_axes = axes(b.rotation);
    a_axes.into_iter().chain(b_axes).all(|axis| {
        let radius = |plot: &SnapshotBuilding, directions: [Vec2; 2]| {
            plot.footprint.x * 0.5 * axis.dot(directions[0]).abs()
                + plot.footprint.y * 0.5 * axis.dot(directions[1]).abs()
        };
        (b.footprint_center - a.footprint_center).dot(axis).abs()
            < radius(a, a_axes) + radius(b, b_axes) - 0.05
    })
}

#[test]
fn growth_profiles_preserve_founders_and_only_control_arrivals() {
    assert_eq!(
        GrowthProfile::Low
            .waves()
            .iter()
            .map(|wave| wave.count)
            .sum::<usize>(),
        6
    );
    assert_eq!(
        GrowthProfile::Steady
            .waves()
            .iter()
            .map(|wave| wave.count)
            .sum::<usize>(),
        21
    );
    assert_eq!(GrowthProfile::Burst.waves().len(), 1);
    assert_eq!(GrowthProfile::Burst.waves()[0].day, 4);
    assert_eq!(GrowthProfile::Burst.waves()[0].count, 24);
}

#[test]
fn city_growth_profiles_offer_exact_population_and_allow_recovery() {
    for population in [100, 250, 500] {
        for pace in ["gradual", "surge"] {
            let profile = GrowthProfile::parse(&format!("city-{population}-{pace}"));
            let waves = profile.waves();
            assert_eq!(profile.target_population(), population);
            assert!(waves.windows(2).all(|pair| pair[0].day < pair[1].day));
            assert!(waves.iter().all(|wave| wave.count > 0));
            assert_eq!(
                waves.iter().take(7).map(|wave| wave.count).sum::<usize>(),
                21
            );
            assert!(
                waves.last().unwrap().day <= 32,
                "allow nearly 30 days after migration for recovery"
            );
            if pace == "surge" {
                assert_eq!(waves.len(), 8);
                assert_eq!(waves[7].day, 12);
            } else {
                assert!(waves.iter().all(|wave| wave.count <= 20));
            }
        }
    }
}

#[test]
fn residential_components_join_chains_without_bridging_distant_groups() {
    let positions = [
        Vec2::ZERO,
        Vec2::new(25.0, 0.0),
        Vec2::new(50.0, 0.0),
        Vec2::new(150.0, 0.0),
        Vec2::new(160.0, 0.0),
    ];
    assert_eq!(residential_cluster_sizes(&positions), vec![3, 2]);
    assert!(residential_cluster_sizes(&[]).is_empty());
}
