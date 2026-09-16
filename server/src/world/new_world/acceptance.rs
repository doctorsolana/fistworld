//! Opt-in evidence for ordinary small-world sessions. Optional diagnostic time
//! speed sets the normal TimeWarp once; no actors, goods or work outcomes are staged.

mod accounting;
mod activity;
mod observation;

use super::WorldOpening;
use crate::player::boat::{VesselRoute, VesselRouteCertification};
use crate::world::{
    immigration::{ImmigrantArrival, NaturalImmigrantVoyage},
    start_config::WorldStartConfig,
    village,
};
use bevy::prelude::*;
use serde_json::{Value, json};
use shared::{
    components::*,
    economy::{Good, GoodsInventory, Wallet},
    terrain::WorldTerrain,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufWriter, Write},
    path::PathBuf,
    time::Instant,
};

#[derive(Resource)]
struct Trace {
    directory: PathBuf,
    output: BufWriter<File>,
    started: Instant,
    last_sample: Option<f64>,
    founders: BTreeSet<u64>,
    departures: BTreeSet<u64>,
    choices: BTreeMap<u64, u64>,
    arrivals: BTreeMap<u64, u64>,
    observed_towns: BTreeSet<u64>,
    immigrant_endowments: BTreeSet<u64>,
    pending_warp: Option<f32>,
    sample_seconds: f64,
    observation: Option<observation::Mode>,
}

pub(crate) fn install(app: &mut App) {
    let Some(directory) = std::env::var_os("FISTWORLD_SMALL_WORLD_TRACE_DIR") else {
        return;
    };
    let directory = PathBuf::from(directory);
    std::fs::create_dir_all(&directory).expect("small-world trace directory");
    let directory = directory.canonicalize().expect("small-world trace path");
    let logs = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../logs")
        .canonicalize()
        .expect("repository logs directory");
    assert!(
        directory.starts_with(logs),
        "small-world evidence belongs under logs/"
    );
    app.insert_resource(Trace {
        output: BufWriter::new(
            File::create(directory.join("small-world.jsonl")).expect("small-world journal"),
        ),
        directory,
        started: Instant::now(),
        last_sample: None,
        founders: BTreeSet::new(),
        departures: BTreeSet::new(),
        choices: BTreeMap::new(),
        arrivals: BTreeMap::new(),
        observed_towns: BTreeSet::new(),
        immigrant_endowments: BTreeSet::new(),
        observation: std::env::var("FISTWORLD_SMALL_WORLD_TRACE_OBSERVATION")
            .ok()
            .map(|value| observation::Mode::parse(&value)),
        pending_warp: diagnostic_number("FISTWORLD_SMALL_WORLD_TRACE_WARP", 1.0, 25.0),
        sample_seconds: f64::from(
            diagnostic_number("FISTWORLD_SMALL_WORLD_TRACE_SAMPLE_SECONDS", 1.0, 60.0)
                .unwrap_or(1.0),
        ),
    });
    app.add_systems(Startup, capture_opening.after(super::populate));
    app.add_systems(Last, observe.run_if(resource_exists::<Trace>));
    app.add_systems(
        FixedUpdate,
        observation::update.before(crate::world::regions::update_client_interest),
    );
}

fn diagnostic_number(name: &str, minimum: f32, maximum: f32) -> Option<f32> {
    std::env::var(name).ok().map(|value| {
        let value = value
            .parse::<f32>()
            .unwrap_or_else(|_| panic!("Invalid {name}"));
        assert!(
            value.is_finite() && value >= minimum && value <= maximum,
            "{name} must be finite and in {minimum}..={maximum}"
        );
        value
    })
}

fn apply_diagnostic_warp(world: &mut World) {
    let Some(factor) = world.resource::<Trace>().pending_warp else {
        return;
    };
    let applied = if let Some(mut warp) = world
        .query_filtered::<&mut TimeWarp, With<WorldTime>>()
        .iter_mut(world)
        .next()
    {
        *warp = TimeWarp::clamped(factor);
        true
    } else {
        false
    };
    if applied {
        world.resource_mut::<Trace>().pending_warp = None;
        info!("Small-world diagnostic time speed set to {factor}x");
    }
}

fn inventory(stock: &GoodsInventory) -> Value {
    let values: BTreeMap<_, _> = Good::ALL
        .into_iter()
        .map(|good| (good.label(), stock.amount(good)))
        .collect();
    json!(values)
}

fn population(world: &mut World) -> Vec<Value> {
    let mut people: Vec<_> = world.query::<(
        Entity, &PersonId, &CharacterKind, &PlayerPosition, Option<&ResidentOf>,
        Option<&ImmigrantArrival>, Has<AboardBoat>, Has<NaturalImmigrantVoyage>,
        Option<&village::MootQueueTicket>, Option<&LivesAt>, Option<&Wallet>,
        Option<&CharacterObjective>, Option<&crate::player::hero::MoveTarget>,
        (Option<&Nutrition>, Option<&Health>, Option<&EmployedAt>,
            Option<&village::VillagerIntent>, Has<village::population::ImmigrationDeparture>),
    )>().iter(world).filter(|(_, _, kind, ..)| **kind == CharacterKind::Villager)
        .map(|(entity, id, _, at, resident, arrival, aboard, voyage, queue, home, wallet, objective, movement, (nutrition, health, employed, intent, departure))| {
            let immigrating = queue.is_some_and(|ticket| ticket.kind == village::MootServiceKind::Immigration);
            json!({"id":id.0,"position":at.0.to_array(),"resident_of":resident.map(|of|of.0.0),
                "chosen_town":arrival.and_then(|arrival|arrival.chosen_settlement).map(|id|id.0),
                "entry":arrival.map(|arrival|arrival.entry.to_array()),"entered_at":arrival.map(|arrival|arrival.entered_at),
                "chosen_at":arrival.and_then(|arrival|arrival.chosen_at),"chosen_score":arrival.and_then(|arrival|arrival.chosen_score),"aboard":aboard,"voyage":voyage,
                "immigration_queue":immigrating,"queue":queue.map(|ticket|format!("{:?}",ticket.kind)),
                "counts_as_resident":intent.is_some_and(village::VillagerIntent::counts_as_resident),
                "immigration_departure":departure,
                "activity":activity::snapshot(world, entity),
                "home":home.map(|home|home.0.0),"wallet":wallet.map(|wallet|wallet.balance()),
                "objective":objective.map(|objective|format!("{objective:?}")),
                "move_target":movement.map(|target|target.0.to_array()),"simulation":"canonical","nutrition":nutrition,"health":health,"workplace":employed.map(|job|job.0.0)})
        }).collect();
    people.sort_by_key(|person| person["id"].as_u64());
    people
}

fn registered_town(person: &Value) -> Option<u64> {
    // ResidentOf names the chosen destination before a migrant reaches town.
    // Only the authoritative post-counter lifecycle establishes residence.
    (person["counts_as_resident"] == true
        && person["aboard"] == false
        && person["voyage"] == false
        && person["immigration_queue"] == false
        && person["immigration_departure"] == false)
        .then(|| person["resident_of"].as_u64())
        .flatten()
}

fn settlements(world: &mut World, people: &[Value]) -> Vec<Value> {
    let mut buildings = BTreeMap::<u64, Vec<Value>>::new();
    for (id, of, building, position) in world
        .query::<(
            &BuildingId,
            &BuildingOf,
            &SettlementBuilding,
            &PlayerPosition,
        )>()
        .iter(world)
    {
        buildings.entry(of.0.0).or_default().push(json!({"id":id.0,"kind":format!("{:?}",building.kind),
            "position":position.0.to_array(),"private_business":village::is_private_business(building.kind)}));
    }
    let mut query = world.query::<(
        &SettlementId,
        &Settlement,
        &PlayerPosition,
        &GoodsInventory,
        Option<&shared::economy::SettlementEconomy>,
    )>();
    let registry = world.get_resource::<crate::world::regions::RegionRegistry>();
    let observed = &world.resource::<Trace>().observed_towns;
    let mut settlements: Vec<_> = query.iter(world)
        .map(|(id, settlement, position, stock, economy)| {
            let region = registry.and_then(|registry|registry.get(shared::region::RegionCoord::from_world_pos(position.0)));
            let local = buildings.remove(&id.0).unwrap_or_default();
            let residents: Vec<_> = people.iter().filter(|person|person["resident_of"].as_u64() == Some(id.0))
                .map(|person|person["id"].clone()).collect();
            json!({"id":id.0,"name":settlement.name,"tier":format!("{:?}",settlement.tier),
                "position":position.0.to_array(),"resident_count_reported":settlement.residents,"resident_ids":residents,
                "treasury":settlement.treasury,"stock":inventory(stock),
                "observer_count":region.map_or(0,|region|region.observers),
                
                "ever_observed":observed.contains(&id.0),
                "simulation":"canonical","economy":economy,
                "houses":local.iter().filter(|building|building["kind"]=="House").count(),
                "businesses":local.iter().filter(|building|building["private_business"]==true).count(),"buildings":local})
        }).collect();
    settlements.sort_by_key(|settlement| settlement["id"].as_u64());
    settlements
}

fn capture_opening(world: &mut World) {
    if let Some(mode) = world.resource::<Trace>().observation {
        observation::prepare(world, mode);
    }
    let people = population(world);
    let towns = settlements(world, &people);
    let accounting = accounting::snapshot(world);
    let opening = world.resource::<WorldOpening>();
    let terrain = world.resource::<WorldTerrain>();
    let bounds = terrain.generator.active_map_bounds();
    let value = json!({"phase":"immutable_opening","seed":opening.seed,
        "config":world.resource::<WorldStartConfig>(),"map_hash":terrain.generator.active_map_content_hash(),
        "bounds":{"min":bounds.min,"max":bounds.max},"settlement_count":opening.settlements,
        "founder_count":opening.residents,"coastal_gateways":opening.arrivals.len(),
        "settlements":towns,"people":people,"accounting":accounting,
        "diagnostic_controls":{"initial_warp":world.resource::<Trace>().pending_warp,"sample_seconds":world.resource::<Trace>().sample_seconds,"observation":world.resource::<Trace>().observation.map(|mode|mode.label())},
        "scope":"Ordinary generated opening, before simulation. No staged workplaces or observer grants."});
    let mut trace = world.resource_mut::<Trace>();
    trace.founders = people
        .iter()
        .map(|person| person["id"].as_u64().unwrap())
        .collect();
    std::fs::write(
        trace.directory.join("opening.json"),
        serde_json::to_vec_pretty(&value).unwrap(),
    )
    .expect("write immutable opening");
    info!("Small-world acceptance opening recorded");
}

fn observe(world: &mut World) {
    apply_diagnostic_warp(world);
    // Count authoritative entries separately from sampled voyage witnesses. Cash
    // conservation must not depend on whether a short voyage appeared in a sample.
    let entered: Vec<_> = world
        .query_filtered::<&PersonId, With<ImmigrantArrival>>()
        .iter(world)
        .map(|id| id.0)
        .collect();
    world
        .resource_mut::<Trace>()
        .immigrant_endowments
        .extend(entered);
    // Record even a brief initial camera visit before the one-world-second
    // journal throttle, so an offscreen acceptance cannot conceal it later.
    let mut towns = world.query::<(&SettlementId, &PlayerPosition)>();
    let observed: Vec<_> = world
        .get_resource::<crate::world::regions::RegionRegistry>()
        .map(|registry| {
            towns
                .iter(world)
                .filter_map(|(id, position)| {
                    registry
                        .get(shared::region::RegionCoord::from_world_pos(position.0))
                        .is_some_and(|region| region.observers > 0)
                        .then_some(id.0)
                })
                .collect()
        })
        .unwrap_or_default();
    world
        .resource_mut::<Trace>()
        .observed_towns
        .extend(observed);
    let Some((day, seconds, cycle, warp)) = world
        .query::<(&WorldTime, &TimeWarp)>()
        .iter(world)
        .next()
        .map(|(clock, warp)| {
            (
                clock.day,
                clock.seconds_in_cycle,
                clock.cycle_duration(),
                warp.0,
            )
        })
    else {
        return;
    };
    if !observation::ready(world) {
        return;
    }
    let now = f64::from(day) * f64::from(cycle) + f64::from(seconds);
    if world
        .resource::<Trace>()
        .last_sample
        .is_some_and(|last| now - last < world.resource::<Trace>().sample_seconds)
    {
        return;
    }
    let people = population(world);
    let settlements = settlements(world, &people);
    let bounds = world
        .resource::<WorldTerrain>()
        .generator
        .active_map_bounds();
    let boats: Vec<_> = world.query::<(Entity, &PlayerPosition, Option<&VesselRoute>, Has<VesselRouteCertification>, Option<&CharacterMotion>)>()
        .iter(world).filter(|(entity, ..)|world.get::<ImmigrantArrivalBoat>(*entity).is_some())
        .map(|(entity, position, route, certified, motion)|json!({"entity":entity.to_bits(),"position":position.0.to_array(),
            "moving":motion.is_some_and(|motion|motion.is_moving()),
            "position_in_bounds":bounds.contains_xz(position.0.x,position.0.z),"route_points":route.map_or(0,|route|route.waypoints.len()),
            "route_next":route.map(|route|route.next),"certified":certified,
            "route_in_bounds":route.is_none_or(|route|route.waypoints.iter().all(|point|point.is_finite() && bounds.contains_xz(point.x,point.y))),
            "cursor_valid":route.is_none_or(|route|route.next<=route.waypoints.len())})).collect();
    let immigration_planning = world
        .get_resource::<crate::world::immigration::NaturalImmigrationDirector>()
        .map(|director| director.planning_status());
    let accounting = accounting::snapshot(world);
    let observation = observation::evidence(world);
    let observer_clients = world
        .query_filtered::<Entity, With<Player>>()
        .iter(world)
        .count();
    let connected_clients = world
        .query_filtered::<Entity, With<lightyear::prelude::server::ClientOf>>()
        .iter(world)
        .count();
    let observed_regions = world
        .get_resource::<crate::world::regions::RegionRegistry>()
        .map_or(0, |registry| registry.observed_count());
    let mut trace = world.resource_mut::<Trace>();
    let mut events = Vec::new();
    for person in &people {
        let id = person["id"].as_u64().unwrap();
        if trace.founders.contains(&id) {
            continue;
        }
        if person["aboard"] == true && trace.departures.insert(id) {
            events.push(json!({"event":"boat_entry_observed","person":id,"chosen_town":person["chosen_town"]}));
        }
        if let Some(chosen) = person["chosen_town"].as_u64() {
            if trace.choices.insert(id, chosen).is_none() {
                events
                    .push(json!({"event":"destination_observed","person":id,"settlement":chosen}));
            }
        }
        if let Some(town) = registered_town(person) {
            if !trace.arrivals.contains_key(&id) {
                trace.arrivals.insert(id, town);
                events.push(json!({"event":"registered_arrival","person":id,"settlement":town}));
            }
        }
    }
    let counts = |values: &BTreeMap<u64, u64>| {
        let mut totals = BTreeMap::<u64, usize>::new();
        for town in values.values() {
            *totals.entry(*town).or_default() += 1;
        }
        totals
    };
    let value = json!({"elapsed":trace.started.elapsed().as_secs_f64(),"day":day,"world_seconds":seconds,
        "absolute_world_seconds":now,"cycle_duration":cycle,"warp":warp,"settlements":settlements,
        "people":people,"accounting":accounting,"observer_clients":observer_clients,"connected_clients":connected_clients,
        "observed_regions":observed_regions,"observation":observation,"immigrant_endowments":trace.immigrant_endowments.len(),"simulation":"canonical",
        "incoming_boats":boats,"immigration_planning":immigration_planning,"boat_entries":trace.departures.len(),"destinations_by_town":counts(&trace.choices),
        "registered_arrivals_by_town":counts(&trace.arrivals),"events":events});
    serde_json::to_writer(&mut trace.output, &value).expect("write small-world sample");
    writeln!(trace.output).expect("finish small-world sample");
    trace.output.flush().expect("flush small-world sample");
    trace.last_sample = Some(now);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destination_membership_does_not_report_registration_before_resident_intent() {
        let mut world = World::new();
        let hall = world.spawn(SettlementId(1)).id();
        let person = world
            .spawn((
                PersonId(25),
                CharacterKind::Villager,
                PlayerPosition(Vec3::new(1_000.0, 4.0, 0.0)),
                ResidentOf(SettlementId(1)),
                village::VillagerIntent::Travelling { settlement: hall },
            ))
            .id();
        assert_eq!(registered_town(&population(&mut world)[0]), None);
        world
            .entity_mut(person)
            .insert(village::VillagerIntent::Resident { settlement: hall });
        let mut evidence = population(&mut world).remove(0);
        assert_eq!(registered_town(&evidence), Some(1));
        evidence["immigration_departure"] = json!(true);
        assert_eq!(registered_town(&evidence), None);
        // Work begun after registration must not make a settled person vanish.
        world
            .entity_mut(person)
            .insert(village::VillagerIntent::Building {
                settlement: hall,
                site: hall,
            });
        assert_eq!(registered_town(&population(&mut world)[0]), Some(1));
    }
}
