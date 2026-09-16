//! Opt-in connected shipbuilding/trade acceptance on the unchanged shared map.
//! Only initial finite fixtures and one ordinary company order are authored;
//! all hauling, labour, boarding, sailing and market transactions remain real.

use crate::player::{
    boat::{VesselNavigationQueue, VesselRoute},
    hero::{MoveTarget, spawn_villager},
};
use crate::world::{
    identity::WorldIdAllocator,
    shipping::{PortHaulJob, PortHaulRoutine},
    village::{CompanyPorter, VillagerIntent},
};
use bevy::prelude::*;
use lightyear::prelude::{NetworkTarget, Replicate};
use shared::{components::*, economy::*, region::RegionCoord, terrain::WorldTerrain};
use std::{
    collections::HashSet,
    fs::File,
    io::{BufWriter, Write},
    path::PathBuf,
};

#[derive(Resource)]
struct Fixture {
    home: Entity,
    away: Entity,
    company: Entity,
    company_id: CompanyId,
    people: Vec<Entity>,
    route_requested: bool,
    output: BufWriter<File>,
    next_sample: f64,
    initial_goods: [u64; 3],
    initial_money: u64,
    observed_heroes: HashSet<PersonId>,
    hero_endowment: u64,
}

pub(crate) fn stage_and_observe(world: &mut World, mut directory: Local<Option<Option<PathBuf>>>) {
    let Some(directory) = directory
        .get_or_insert_with(|| std::env::var_os("FISTWORLD_PORT_TRACE_DIR").map(PathBuf::from))
        .clone()
    else {
        return;
    };
    if world.query::<&WorldTime>().iter(world).next().is_none() {
        return;
    }
    if !world.contains_resource::<Fixture>() {
        let terrain = world.resource::<WorldTerrain>();
        assert_eq!(
            terrain.generator.active_map_id(),
            "village_lab",
            "connected ports use the unchanged shared village_lab map"
        );
        let sites = super::lab_sites::select(terrain, world.resource::<crate::player::boat::clearance::WaterNavigationGeometry>())
            .expect("village_lab needs two actual navigable coastal port sites; never substitute synthetic water");
        stage(world, &directory, sites);
    }
    let mut fixture = world.remove_resource::<Fixture>().unwrap();
    // The ordinary joining hero is a documented external money source, not
    // a reason to reset the conservation baseline after startup.
    for person in world.query_filtered::<&PersonId, With<Hero>>().iter(world) {
        if fixture.observed_heroes.insert(*person) {
            fixture.hero_endowment += STARTING_HERO_MONEY;
        }
    }
    let ship = world
        .query::<(Entity, &ShipId, &CompanyShip)>()
        .iter(world)
        .find(|(_, _, hull)| hull.company == fixture.company_id)
        .map(|(entity, id, _)| (entity, *id));
    if let Some((_, ship_id)) = ship {
        if !fixture.route_requested {
            let home = *world.get::<SettlementId>(fixture.home).unwrap();
            let away = *world.get::<SettlementId>(fixture.away).unwrap();
            crate::player::maritime::execute(
                world,
                fixture.company_id,
                shared::protocol::HeroMaritimeAction::CreateRoute {
                    ship: ship_id,
                    good: Good::Wood,
                    cargo_target: 12,
                    maximum_purchase_price: 200,
                    minimum_destination_price: 80,
                    automatic: false,
                    stops: vec![
                        TradeRouteStop {
                            settlement: home,
                            action: TradeRouteStopAction::Buy,
                        },
                        TradeRouteStop {
                            settlement: away,
                            action: TradeRouteStopAction::Sell,
                        },
                    ],
                },
            )
            .expect("real connected maritime route command");
            fixture.route_requested = true;
            // DispatchOnce is the same ordinary state transition made by the
            // order handler after validation. No route/cargo/position is faked.
            let entity = world
                .query::<(Entity, &CompanyTradeRoute)>()
                .iter(world)
                .find(|(_, route)| route.company == fixture.company_id)
                .map(|(entity, _)| entity)
                .unwrap();
            world.get_mut::<CompanyTradeRoute>(entity).unwrap().status =
                TradeRouteStatus::WaitingForPorter;
        }
    }
    let now = world.resource::<Time>().elapsed_secs_f64();
    let warp = world
        .query::<&TimeWarp>()
        .iter(world)
        .next()
        .map_or(1.0, |warp| warp.0);
    if warp > 1.0 || now >= fixture.next_sample {
        fixture.next_sample = now + 0.05;
        let row = snapshot(world, &fixture, now, warp);
        writeln!(fixture.output, "{row}").unwrap();
        fixture.output.flush().unwrap();
    }
    world.insert_resource(fixture);
}

#[derive(Default)]
struct TownLayout {
    occupied: Vec<(Vec3, f32)>,
    blockers: Vec<crate::world::village::RoadAccessBlocker>,
    roads: Vec<VillageRoad>,
}

fn completed_building(
    world: &mut World,
    hall: Vec3,
    town: SettlementId,
    name: &str,
    kind: SettlementBuildingKind,
    layout: &mut TownLayout,
) -> (Entity, BuildingId) {
    // Nearby fixture towns may share frontage. Every previously staged plot
    // participates in siting, not just this town's local placement history.
    let existing: Vec<_> = world
        .query::<(&SettlementBuilding, &PlayerPosition, &PlayerRotation)>()
        .iter(world)
        .map(|(building, position, rotation)| (building.kind, position.0, rotation.0))
        .collect();
    let mut occupied = layout.occupied.clone();
    let mut blockers = layout.blockers.clone();
    for (kind, position, rotation) in existing {
        occupied.push((position, kind.clearance()));
        blockers.extend(crate::world::village::road_access_blockers_for_plot(
            kind, position, rotation,
        ));
    }
    let terrain = world.resource::<WorldTerrain>();
    let roads: Vec<_> = layout.roads.iter().collect();
    let mut approved = None;
    'search: for radius in [28., 36., 44., 56., 72., 92., 120.] {
        for step in 0..32 {
            let angle = step as f32 * std::f32::consts::TAU / 32.;
            let xz = hall.xz() + Vec2::new(angle.cos(), angle.sin()) * radius;
            let position = Vec3::new(xz.x, terrain.get_height(xz.x, xz.y), xz.y);
            let toward = (hall.xz() - xz).normalize_or_zero();
            let yaw = (-toward.x).atan2(-toward.y);
            if let Ok(plot) = crate::world::village::validate_manual_plot(
                terrain,
                hall,
                kind,
                position,
                yaw,
                &occupied,
                &roads,
                &[],
                &blockers,
                world.get_resource::<crate::collision::library::StaticColliders>(),
                world.get_resource::<crate::collision::library::DerivedColliderLibrary>(),
                None,
                &[],
            ) {
                if plot.road_access.len() >= 2 {
                    approved = Some(plot);
                    break 'search;
                }
            }
        }
    }
    let approval = approved.unwrap_or_else(|| {
        panic!("Port fixture needs a genuinely valid {kind:?} plot and connector at {hall:?}")
    });
    let id = world.resource_mut::<WorldIdAllocator>().building();
    let entity = world
        .spawn((
            id,
            BuildingOf(town),
            SettlementBuilding {
                kind,
                settlement: name.into(),
                owner: None,
                quality: approval.quality,
                workers: Vec::new(),
            },
            PlayerPosition(approval.position),
            PlayerRotation(approval.rotation),
            shared::building::PlacedBuilding {
                building_type: kind.art(),
                rotation: approval.rotation,
            },
            shared::building::BuildingPosition(approval.position),
            RegionCoord::from_world_pos(approval.position),
            Replicate::to_clients(NetworkTarget::All),
        ))
        .id();
    let road = VillageRoad {
        settlement: name.into(),
        builder: "Initial port fixture".into(),
        built_through: approval.road_access.len() as u16,
        points: approval.road_access,
        width: 2.5,
        reserved_width: RoadClass::Lane.initial_reserved_width(),
        surface: RoadSurface::Dirt,
        class: RoadClass::Lane,
        stone_committed: 0,
    };
    world.spawn((
        road.clone(),
        RoadOf(town),
        crate::world::village_roads::RoadConnectorFor { building: entity },
        Replicate::to_clients(NetworkTarget::All),
    ));
    layout.roads.push(road);
    layout.occupied.push((approval.position, kind.clearance()));
    layout
        .blockers
        .extend(crate::world::village::road_access_blockers_for_plot(
            kind,
            approval.position,
            approval.rotation,
        ));
    (entity, id)
}

fn stage(world: &mut World, directory: &std::path::Path, sites: [(Vec3, f32, PortGeometry); 2]) {
    std::fs::create_dir_all(directory).unwrap();
    if let Some(mut clock) = world.query::<&mut WorldTime>().iter_mut(world).next() {
        clock.set_normalized_time(6. / 24.);
    }
    let mut halls = Vec::new();
    let mut ports = Vec::new();
    let mut towns = Vec::new();
    let mut layouts = Vec::new();
    for (index, (position, yaw, geometry)) in sites.iter().copied().enumerate() {
        let town = world.resource_mut::<WorldIdAllocator>().settlement();
        let port_id = world.resource_mut::<WorldIdAllocator>().building();
        let name = if index == 0 { "Seaward" } else { "Farhaven" };
        let mut stock = GoodsInventory::new_partitioned(capacity::HALL);
        let mut market = MootMarket::founding();
        market.unlock_trade_tier(MarketTradeTier::PavedMarketplace);
        for (good, units, ask) in [
            (Good::Wood, if index == 0 { 72 } else { 0 }, 50),
            (Good::Iron, if index == 0 { 8 } else { 0 }, 100),
            (Good::Wool, if index == 0 { 12 } else { 0 }, 80),
            (Good::Bread, 120, 10),
        ] {
            assert_eq!(stock.add(good, units), units);
            market.consign(MarketSeller::Treasury(town), good, units, ask);
        }
        let hall = world
            .spawn((
                town,
                Settlement {
                    name: name.into(),
                    tier: SettlementTier::Town,
                    residents: 0,
                    treasury: 20_000,
                },
                stock,
                market,
                SettlementPolicies {
                    autopilot: false,
                    staffing_posture: CivicStaffingPosture::Essential,
                    ..default()
                },
                MootAdministration::default(),
                CivicHallLevel::Town,
                PlayerPosition(position),
                PlayerRotation(yaw),
                Replicate::to_clients(NetworkTarget::All),
            ))
            .id();
        let port = world
            .spawn((
                port_id,
                BuildingOf(town),
                SettlementPort {
                    settlement: town,
                    geometry,
                    built: true,
                },
                PlayerPosition(geometry.shore),
                PlayerRotation(geometry.pier_yaw()),
                RegionCoord::from_world_pos(geometry.shore),
                Replicate::to_clients(NetworkTarget::All),
            ))
            .id();
        let mut layout = TownLayout {
            occupied: sites
                .iter()
                .flat_map(|(hall, _, port)| {
                    [
                        (*hall, SettlementBuildingKind::Hall.clearance()),
                        (port.shore, 10.5),
                    ]
                })
                .collect(),
            blockers: sites
                .iter()
                .flat_map(|(_, _, port)| crate::world::village::RoadAccessBlocker::for_port(*port))
                .collect(),
            ..default()
        };
        let (market_entity, _) = completed_building(
            world,
            position,
            town,
            name,
            SettlementBuildingKind::Market,
            &mut layout,
        );
        world.entity_mut(market_entity).insert(MarketLevel::Paved);
        halls.push(hall);
        ports.push(port);
        towns.push(town);
        layouts.push(layout);
    }
    // The populated home already has ordinary beds and its tier's public
    // buildings. No free House/Church permit can commandeer the acceptance crew.
    completed_building(
        world,
        sites[0].0,
        towns[0],
        "Seaward",
        SettlementBuildingKind::Church,
        &mut layouts[0],
    );
    let homes: Vec<_> = (0..2)
        .map(|_| {
            completed_building(
                world,
                sites[0].0,
                towns[0],
                "Seaward",
                SettlementBuildingKind::House,
                &mut layouts[0],
            )
        })
        .collect();
    let (warehouse, warehouse_id) = completed_building(
        world,
        sites[0].0,
        towns[0],
        "Seaward",
        SettlementBuildingKind::StorageHall,
        &mut layouts[0],
    );
    let company_id = world.resource_mut::<WorldIdAllocator>().company();
    let mut people = Vec::new();
    for index in 0..7 {
        let mut at = sites[0].0
            + Quat::from_rotation_y(sites[0].1) * Vec3::new(index as f32 * 1.4 - 4., 0., -10.);
        at.y = world.resource::<WorldTerrain>().get_height(at.x, at.z);
        let name = match index {
            0 => "Captain Rowan".to_string(),
            5 => "Reeve Elin".into(),
            6 => "Steward Mara".into(),
            _ => format!("Harbour worker {index}"),
        };
        let (person, _) = person(world, at, &name);
        let home = homes[index / 4];
        world.entity_mut(person).insert((
            ResidentOf(towns[0]),
            Residence("Seaward".into()),
            VillagerIntent::Resident {
                settlement: halls[0],
            },
            Wallet::new(100),
            Occupation(None),
            WorkStatus::LookingForWork,
            crate::world::village::HomeAssignment::new(home.0),
            LivesAt(home.1),
        ));
        people.push(person);
    }
    // Company direction is a separate person with an actual public job. The
    // paid porter is not a wealthy Master who can abandon this post to invest.
    let master = *world.get::<PersonId>(people[5]).unwrap();
    for (actor, role, title) in [
        (people[5], CivicRole::Reeve, "Reeve"),
        (people[6], CivicRole::MootSteward, "Moot Steward"),
    ] {
        world.entity_mut(actor).insert((
            CivicEmployment {
                settlement: towns[0],
                role,
            },
            Occupation(Some(title.into())),
            WorkStatus::Employed,
        ));
    }
    world
        .entity_mut(people[6])
        .insert(crate::world::village::MootSteward {
            settlement: halls[0],
        });
    {
        let mut office = world.get_mut::<MootAdministration>(halls[0]).unwrap();
        office.reeve = Some("Reeve Elin".into());
        office.lead_steward = Some("Steward Mara".into());
        office.city_workers = vec!["Steward Mara".into()];
    }
    let company = world
        .spawn((
            company_id,
            Company {
                name: "Seaward Shipping".into(),
                founded_day: 0,
            },
            CompanyLeadership { master },
            CompanyOwnership::sole(master),
            CompanyAccount {
                cash: 50_000,
                contributed_capital: 50_000,
                ..default()
            },
            CompanyBranchPolicies::default(),
            CompanyManagementPolicy {
                autopilot: false,
                automatic_dividends: false,
                ..default()
            },
            Replicate::to_clients(NetworkTarget::All),
        ))
        .id();
    world.entity_mut(warehouse).insert((
        OperatedBy(company_id),
        GoodsInventory::new(capacity::STORAGE_HALL),
        BusinessAccount::default(),
        BusinessCondition::default(),
        BusinessStaffingPolicy::new(1),
        BusinessWagePolicy::default(),
        BusinessManagementPolicy {
            autopilot: false,
            automatic_withdrawals: false,
            ..default()
        },
    ));
    world
        .get_mut::<SettlementBuilding>(warehouse)
        .unwrap()
        .workers = vec!["Captain Rowan".into()];
    world.entity_mut(people[0]).insert((
        EmployedAt(warehouse_id),
        CompanyPorter {
            settlement: halls[0],
            settlement_id: towns[0],
            company: company_id,
            storage_hall: warehouse_id,
        },
        Occupation(Some("Porter".into())),
        WorkStatus::Employed,
    ));
    let port_id = *world.get::<BuildingId>(ports[0]).unwrap();
    super::order_ship(world, company_id, port_id, ShipKind::Coaster)
        .expect("initial ordinary paid ship order");
    super::advance_ship_orders(world);
    let manifest = serde_json::json!({"scenario":"port-trade","map_hash":world.resource::<WorldTerrain>().generator.active_map_content_hash(),"home":sites[0].2,"away":sites[1].2,"hall":sites[0].0.to_array(),"home_pickup":SettlementBuildingKind::Hall.entrance_position(sites[0].0,sites[0].1).to_array(),"ship_materials":ShipKind::Coaster.materials(),"cargo":12,"scope":"Finite initial Town markets with real Paved Markets, home Church/houses/connected warehouse, staffed Essential civic offices, separate Master and paid captain, four unemployed harbour workers. Initial paid hull order reserves its actual materials before ordinary AI starts. No subsequent grants or actor overrides. Not natural investment selection or a performance benchmark."});
    std::fs::write(
        directory.join("fixture.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let initial_goods = totals(world);
    let initial_money = money(world);
    let observed_heroes = world
        .query_filtered::<&PersonId, With<Hero>>()
        .iter(world)
        .copied()
        .collect();
    world.insert_resource(Fixture {
        home: halls[0],
        away: halls[1],
        company,
        company_id,
        people,
        route_requested: false,
        output: BufWriter::new(File::create(directory.join("port.jsonl")).unwrap()),
        next_sample: 0.,
        initial_goods,
        initial_money,
        observed_heroes,
        hero_endowment: 0,
    });
    info!(
        "Port trade fixture ready: completed Town infrastructure and initial finite real ship order"
    );
}
fn person(world: &mut World, position: Vec3, name: &str) -> (Entity, PersonId) {
    let id = world.resource_mut::<WorldIdAllocator>().person();
    let mut queue = bevy::ecs::world::CommandQueue::default();
    let mut commands = Commands::new(&mut queue, world);
    let entity = spawn_villager(
        &mut commands,
        world.resource::<WorldTerrain>(),
        id.0.wrapping_add(21_000),
        position,
    );
    commands
        .entity(entity)
        .insert((id, CharacterName(name.into())));
    queue.apply(world);
    (entity, id)
}
fn totals(world: &mut World) -> [u64; 3] {
    let mut result = [0; 3];
    for stock in world.query::<&GoodsInventory>().iter(world) {
        for (index, good) in [Good::Wood, Good::Iron, Good::Wool].into_iter().enumerate() {
            result[index] += u64::from(stock.amount(good));
        }
    }
    for hull in world.query::<&CompanyShip>().iter(world) {
        for (index, (_, units)) in hull.kind.materials().into_iter().enumerate() {
            result[index] += u64::from(units);
        }
    }
    result[0] += household_wood_consumed(world);
    result
}
fn money(world: &mut World) -> u64 {
    crate::world::economic_accounting::total_money(world)
}
fn household_wood_consumed(world: &mut World) -> u64 {
    world
        .query::<&crate::world::village::HearthState>()
        .iter(world)
        .map(|hearth| hearth.consumed_wood)
        .sum()
}
fn snapshot(world: &mut World, fixture: &Fixture, elapsed: f64, warp: f32) -> serde_json::Value {
    let people:Vec<_> = fixture.people.iter().filter_map(|entity| {
        let position = world.get::<PlayerPosition>(*entity)?;
        Some(serde_json::json!({"id":world.get::<PersonId>(*entity).map(|id|id.0),"position":position.0.to_array(),"activity":world.get::<CharacterActivity>(*entity).map(|activity|format!("{activity:?}")),"crew":world.get::<crate::world::shipping::crew::ShipCrew>(*entity).map(|crew|crew.diagnostic_phase()),"food":world.get::<GoodsInventory>(*entity).map_or(0,GoodsInventory::edible_amount),"workplace_interior":world.get::<crate::world::village::WorkplaceInterior>(*entity).is_some(),"workplace_door_transit":world.get::<crate::world::village::WorkplaceDoorTransit>(*entity).is_some(),"motion":world.get::<CharacterMotion>(*entity).map(|motion|motion.velocity.to_array()),"provision_counter":world.get::<crate::world::shipping::crew::ShipCrew>(*entity).and_then(|crew|crew.diagnostic_provision_counter(world)).map(|position|position.to_array()),"wood":world.get::<GoodsInventory>(*entity).map_or(0,|stock|stock.amount(Good::Wood)),"iron":world.get::<GoodsInventory>(*entity).map_or(0,|stock|stock.amount(Good::Iron)),"wool":world.get::<GoodsInventory>(*entity).map_or(0,|stock|stock.amount(Good::Wool)),"wallet":world.get::<Wallet>(*entity).map(|w|w.balance()),"haul":world.get::<PortHaulRoutine>(*entity).is_some(),"builder":world.get::<super::PortBuilder>(*entity).is_some(),"aboard":world.get::<AboardShip>(*entity).map(|aboard|aboard.ship.0),"simulation":"canonical","move_target":world.get::<MoveTarget>(*entity).map(|target|target.0.to_array())}))
    }).collect();
    let ships:Vec<_> = world.query::<(Entity,&ShipId,&CompanyShip,&PlayerPosition,&GoodsInventory)>().iter(world).filter(|(_,_,hull,_,_)|hull.company==fixture.company_id).map(|(entity,id,hull,position,cargo)| serde_json::json!({"id":id.0,"position":position.0.to_array(),"status":hull.status,"wood":cargo.amount(Good::Wood),"water_route":world.get::<VesselRoute>(entity).is_some(),"pending":world.get_resource::<VesselNavigationQueue>().is_some_and(|queue|queue.is_pending(entity))})).collect();
    let orders: Vec<_> = world
        .query::<&ShipConstructionOrder>()
        .iter(world)
        .filter(|order| order.company == fixture.company_id)
        .copied()
        .collect();
    let routes: Vec<_> = world
        .query::<&CompanyTradeRoute>()
        .iter(world)
        .filter(|route| route.company == fixture.company_id)
        .copied()
        .collect();
    let hauls:Vec<_> = world.query::<&PortHaulJob>().iter(world).filter(|job|job.request.owner==PortCargoOwner::Company(fixture.company_id)).map(|job|serde_json::json!({"good":job.request.good,"delivered":job.delivered,"status":format!("{:?}",job.status),"fee":job.fee_remaining})).collect();
    let construction:Vec<_> = world.query::<&super::PortWorkProject>().iter(world).filter(|project|project.owner==PortCargoOwner::Company(fixture.company_id)).map(|project|serde_json::json!({"worked":project.worked,"finished":project.finished,"source":world.get::<GoodsInventory>(project.source),"destination":world.get::<GoodsInventory>(project.destination),"escrow":project.labour_escrow})).collect();
    let town_stock = |world: &World, entity| {
        world
            .get::<GoodsInventory>(entity)
            .map_or(0, |stock| stock.amount(Good::Wood))
    };
    serde_json::json!({"elapsed_real":elapsed,"warp":warp,"money":money(world),"initial_money":fixture.initial_money + fixture.hero_endowment,"initial_money_before_entry":fixture.initial_money,"hero_entry_endowment":fixture.hero_endowment,"goods":totals(world),"initial_goods":fixture.initial_goods,"household_wood_consumed":household_wood_consumed(world),"people":people,"ships":ships,"orders":orders,"routes":routes,"hauls":hauls,"construction":construction,"company_cash":world.get::<CompanyAccount>(fixture.company).map(|account|account.cash),"home_wood":town_stock(world,fixture.home),"away_wood":town_stock(world,fixture.away)})
}
