//! Turn an approved founding plan into ordinary entities and finite assets.

use super::Community;
use crate::world::{identity::WorldIdAllocator, village};
use bevy::prelude::*;
use lightyear::prelude::{NetworkTarget, Replicate};
use shared::components::*;
use shared::economy::*;
use shared::terrain::WorldTerrain;

fn opening_stock(kind: SettlementBuildingKind, stock: &mut GoodsInventory) {
    use SettlementBuildingKind as K;
    let goods: &[(Good, u32)] = match kind {
        K::House => &[(Good::Bread, 8)],
        K::Farmstead => &[(Good::Wheat, 24)],
        K::Windmill => &[(Good::Wheat, 24), (Good::Flour, 16)],
        K::Bakery => &[(Good::Flour, 24), (Good::Bread, 24)],
        K::FishermansHut => &[(Good::Food, 16)],
        K::LumberjackHut => &[(Good::Wood, 12)],
        K::StoneQuarry => &[(Good::Stone, 12)],
        K::LivestockFarm => &[(Good::Meat, 12), (Good::Wool, 8)],
        K::Tavern => &[(Good::Bread, 8), (Good::Meat, 4)],
        _ => &[],
    };
    for &(good, amount) in goods {
        assert_eq!(stock.add(good, amount), amount);
    }
}

pub(super) fn community(
    commands: &mut Commands,
    terrain: &mut WorldTerrain,
    ids: &mut WorldIdAllocator,
    deltas: &mut village::PublishedTerrainDeltas,
    community: &Community,
    config: &crate::world::start_config::WorldStartConfig,
) {
    let id = ids.settlement();
    let hall = community.site.hall;
    let population = community.layout.population;
    let tier = if config.opening == crate::world::start_config::OpeningProfile::Frontier {
        SettlementTier::Hamlet
    } else if population >= 48 {
        SettlementTier::Town
    } else if population >= 24 {
        SettlementTier::Village
    } else {
        SettlementTier::Hamlet
    };
    let level = CivicHallLevel::for_tier(tier);
    let mut inventory = GoodsInventory::new_partitioned(capacity::HALL);
    let mut market = MootMarket::founding();
    for (good, amount) in config.opening_stock(population).goods() {
        let stocked = inventory.add(good, amount);
        assert_eq!(stocked, amount, "validated opening stock must fit the Hall");
        market.consign(MarketSeller::Treasury(id), good, stocked, good.base_price());
    }
    let entity = commands
        .spawn((
            id,
            super::FoundingLandNetwork(community.land_network),
            Settlement {
                name: community.name.clone(),
                tier,
                residents: population as u32,
                treasury: config
                    .hall_treasury_pennies
                    .unwrap_or((20 + population as u64 / 2) * PENNIES_PER_COIN),
            },
            inventory,
            market,
            SettlementPolicies::default(),
            level,
            PlayerPosition(hall),
            PlayerRotation(0.0),
            shared::building::PlacedBuilding {
                building_type: level.building_type(),
                rotation: 0.0,
            },
            shared::building::BuildingPosition(hall),
            Replicate::to_clients(NetworkTarget::All),
        ))
        .id();

    if let Some(square) = &community.layout.square {
        commands.entity(entity).insert(square.clone());
    }

    let def = level.building_type().definition();
    let center = def.world_footprint_center(hall, 0.0);
    let affected = terrain.apply_flatten_rect(
        Vec3::new(center.x, hall.y, center.y),
        def.terrain_flat_half_extents(),
        0.0,
        def.terrain_blend_width(),
    );
    village::publish_terrain_chunks(terrain, deltas, commands, affected);

    let mut homes = Vec::new();
    let mut businesses = Vec::new();
    for plot in &community.layout.plots {
        let building_id = ids.building();
        let under = village::UnderConstruction {
            kind: plot.kind,
            position: plot.position,
            rotation: plot.rotation,
            owner: None,
            owner_id: None,
            builder: None,
            settlement: entity,
            settlement_id: id,
            stand: plot.position,
            failed_stand_routes: 0,
            stage: village::BuildStage::Walking,
            quality: plot.quality,
        };
        let affected = village::level_construction_ground(terrain, &under);
        village::publish_terrain_chunks(terrain, deltas, commands, affected);
        let mut stock = GoodsInventory::new(plot.kind.storage_bulk_capacity());
        opening_stock(plot.kind, &mut stock);
        let building = commands
            .spawn((
                building_id,
                BuildingOf(id),
                SettlementBuilding {
                    kind: plot.kind,
                    settlement: community.name.clone(),
                    owner: None,
                    quality: plot.quality,
                    workers: Vec::new(),
                },
                stock,
                PlayerPosition(plot.position),
                PlayerRotation(plot.rotation),
                shared::building::PlacedBuilding {
                    building_type: plot.kind.art(),
                    rotation: plot.rotation,
                },
                shared::building::BuildingPosition(plot.position),
                Replicate::to_clients(NetworkTarget::All),
            ))
            .id();
        let mut road = plot.road.clone();
        road.settlement = community.name.clone();
        road.builder = community.name.clone();
        commands.spawn((
            road,
            RoadOf(id),
            crate::world::village_roads::RoadConnectorFor { building },
            Replicate::to_clients(NetworkTarget::All),
        ));
        if plot.kind == SettlementBuildingKind::House {
            homes.push((building, building_id, plot));
        }
        if village::is_private_business(plot.kind) {
            businesses.push((building, plot.kind));
        }
    }
    let mut people = Vec::new();
    for (ordinal, position) in community.layout.frontier_stands.iter().enumerate() {
        let person = ids.person();
        let seed = shared::worldgen::splitmix64(community.site.salt ^ ordinal as u64);
        let resident = crate::player::hero::spawn_villager(commands, terrain, seed, *position);
        commands.entity(resident).insert((
            person,
            Residence(community.name.clone()),
            ResidentOf(id),
            village::VillagerIntent::Resident { settlement: entity },
        ));
        people.push((person, shared::names::person_name(seed)));
    }
    for (home_index, (home, home_id, plot)) in homes.into_iter().enumerate() {
        let mut household = Household::default();
        for member in 0..4 {
            let ordinal = home_index * 4 + member;
            if ordinal >= population {
                break;
            }
            let person = ids.person();
            let seed = shared::worldgen::splitmix64(community.site.salt ^ ordinal as u64);
            // Start on this home's certified access lane. The road's ordinary
            // tree clearance governs both clients and collider streaming.
            let points = &plot.road.points;
            let point = points[(member + 1).min(points.len() - 1)];
            let position = Vec3::new(point.x, terrain.get_height(point.x, point.y), point.y);
            let resident = crate::player::hero::spawn_villager(commands, terrain, seed, position);
            let name = shared::names::person_name(seed);
            commands.entity(resident).insert((
                person,
                Residence(community.name.clone()),
                ResidentOf(id),
                LivesAt(home_id),
                village::HomeAssignment::new(home),
                village::VillagerIntent::Resident { settlement: entity },
            ));
            household.resident_ids.push(person);
            household.residents.push(name.clone());
            people.push((person, name));
        }
        commands
            .entity(home)
            .insert((household, HouseholdEconomy::default()));
    }
    assert_eq!(
        people.len(),
        population,
        "opening founding plan must match its actual residents"
    );
    for (index, (building, kind)) in businesses.into_iter().enumerate() {
        let (owner, name) = &people[index % people.len()];
        let company = ids.company();
        let capital = (8 + u64::from(kind.positions()) * 4) * PENNIES_PER_COIN;
        commands.spawn(village::new_company_bundle(
            company,
            format!("{name}'s {}", kind.label()),
            0,
            *owner,
            capital,
            capital,
        ));
        commands.entity(building).insert((
            OwnedBy(*owner),
            OperatedBy(company),
            BusinessAccount::default(),
            BusinessStaffingPolicy::new(kind.positions()),
        ));
        let owner_name = name.clone();
        commands.queue(move |world: &mut World| {
            world
                .get_mut::<SettlementBuilding>(building)
                .expect("seeded business")
                .owner = Some(owner_name);
        });
    }
}
