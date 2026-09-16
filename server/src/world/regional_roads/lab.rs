//! Opt-in connected acceptance: finite initial funding, then real work and movement.
//! No terrain or actor position is rewritten after staging. The client sees the
//! same generated river as the server; sampling is observation-only.

use super::*;
use crate::player::{
    boat::{
        VesselGoal, VesselNavigation, VesselNavigationQueue, VesselRoute, VesselRouteFailed,
        clearance::{WaterNavigationGeometry, WatercraftClearance, bridge_passage_clear},
    },
    hero::{MoveTarget, spawn_villager},
};
use crate::world::{
    identity::WorldIdAllocator,
    village::VillagerIntent,
    village_roads::{NavigationRouteFailed, NavigationRoutePending, TravelRoute},
};
use lightyear::prelude::{NetworkTarget, Replicate};
use shared::{
    components::*,
    economy::{Good, GoodsInventory, MootMarket, Wallet},
    region::RegionCoord,
    terrain::WorldTerrain,
};
use std::{
    collections::VecDeque,
    fs::File,
    io::{BufWriter, Write},
    path::PathBuf,
};

const WAGES: u64 = 100;
const TREASURY: u64 = 10_000;

#[derive(Clone)]
pub(crate) struct Site {
    deck: RoadBridge,
    hall: Vec3,
    yaw: f32,
    pickup: Vec3,
    boat_start: Vec3,
    boat_end: Vec3,
}

#[derive(Resource)]
struct Fixture {
    site: Site,
    hall: Entity,
    town: SettlementId,
    project: Option<Entity>,
    worker: Option<Entity>,
    bridge: Option<Entity>,
    walker: Option<Entity>,
    boat: Option<Entity>,
    output: BufWriter<File>,
    started: f64,
    next_sample: f64,
}

/// Root schedules this after bridge work and before body movement, so the
/// journal records actual pickup/deposit positions even at accelerated time.
pub(crate) fn stage_and_observe(
    world: &mut World,
    mut directory: Local<Option<Option<PathBuf>>>,
    mut candidates: Local<Option<VecDeque<Site>>>,
) {
    let Some(directory) = directory
        .get_or_insert_with(|| std::env::var_os("FISTWORLD_BRIDGE_TRACE_DIR").map(PathBuf::from))
        .clone()
    else {
        return;
    };
    if world.query::<&WorldTime>().iter(world).next().is_none() {
        return;
    }
    if !world.contains_resource::<Fixture>() {
        if candidates.is_none() {
            let terrain = world.resource::<WorldTerrain>();
            assert_eq!(
                terrain.generator.active_map_id(),
                "village_lab",
                "bridge acceptance uses the shared village_lab recipe"
            );
            *candidates = Some(candidate_sites(terrain));
        }
        let queue = candidates.as_mut().unwrap();
        let site = queue.front().expect("No valid village_lab bridge acceptance site; do not replace the river with fake geometry").clone();
        let bridge_clear = crate::world::village_roads::regional_bridge_footprint_clear(
            world,
            &[site.deck.start.xz(), site.deck.end.xz()],
            site.deck.width,
        );
        let approach_clear = crate::world::village_roads::regional_bridge_footprint_clear(
            world,
            &[site.pickup.xz(), site.deck.start.xz()],
            2.0,
        );
        match (bridge_clear, approach_clear) {
            (Some(false), _) | (_, Some(false)) => {
                queue.pop_front();
                return;
            }
            (Some(true), Some(true)) => stage(world, &directory, site),
            _ => return,
        }
    }
    let mut fixture = world.remove_resource::<Fixture>().unwrap();
    // Starting only after the observer frames the site prevents a finished
    // bridge from masquerading as evidence of its unseen construction.
    if fixture.project.is_none()
        && directory.join("start").is_file()
        && world
            .query_filtered::<&PlayerPosition, With<Player>>()
            .iter(world)
            .any(|p| p.0.xz().distance(fixture.site.deck.midpoint().xz()) < 100.0)
    {
        begin_project(world, &mut fixture);
    }
    let built = fixture
        .bridge
        .and_then(|e| world.get::<RoadBridge>(e))
        .is_some_and(|b| b.built);
    if built && fixture.walker.is_none() {
        begin_crossings(world, &mut fixture);
    }
    let now = world.resource::<Time>().elapsed_secs_f64();
    let warp = world
        .query::<&TimeWarp>()
        .iter(world)
        .next()
        .map_or(1.0, |w| w.0);
    if fixture.project.is_some() && (warp > 1.0 || now >= fixture.next_sample) {
        fixture.next_sample = now + 0.05;
        let row = snapshot(world, &fixture, now, warp);
        writeln!(fixture.output, "{row}").expect("bridge acceptance trace write");
        fixture
            .output
            .flush()
            .expect("bridge acceptance trace flush");
    }
    world.insert_resource(fixture);
}

fn candidate_sites(terrain: &WorldTerrain) -> VecDeque<Site> {
    let mut sites = VecDeque::new();
    for river in terrain.generator.loaded_map().rivers.iter() {
        for span in river.windows(3).step_by(3) {
            let centre = span[1].xz();
            let along = (span[2].xz() - span[0].xz()).normalize_or_zero();
            if along.length_squared() < 0.5 {
                continue;
            }
            let across = Vec2::new(-along.y, along.x);
            for half in [20.0, 28.0, 36.0, 44.0, 52.0] {
                let Some(deck) = bridge::planning::survey_bridge(
                    centre - across * half,
                    centre + across * half,
                    |p| {
                        (
                            terrain.get_height(p.x, p.y),
                            terrain.get_water_height(p.x, p.y),
                        )
                    },
                ) else {
                    continue;
                };
                let hall_xz = deck.start.xz() - across * 30.0;
                let hall = Vec3::new(
                    hall_xz.x,
                    terrain.get_height(hall_xz.x, hall_xz.y),
                    hall_xz.y,
                );
                if shared::components::settlement_founding_refusal(terrain, hall, None).is_some() {
                    continue;
                }
                let yaw = (-across.x).atan2(-across.y);
                let pickup = SettlementBuildingKind::Hall.entrance_position(hall, yaw);
                if (0..=32).any(|i| {
                    let p = pickup.xz().lerp(deck.start.xz(), i as f32 / 32.0);
                    terrain.get_water_height(p.x, p.y).is_some()
                }) {
                    continue;
                }
                // The old centreline-only survey could spawn a dinghy whose
                // stern/beam already intersected the shallows. Keep the normal
                // clearance proof, including the future deck, at fixture setup.
                let Some(endpoints) = [22.0, 18.0, 14.0].into_iter().find_map(|distance| {
                    let endpoints = [centre - along * distance, centre + along * distance];
                    boat_crossing_clear(terrain, &deck, endpoints).then_some(endpoints)
                }) else {
                    continue;
                };
                let a = terrain
                    .get_water_height(endpoints[0].x, endpoints[0].y)
                    .unwrap();
                let b = terrain
                    .get_water_height(endpoints[1].x, endpoints[1].y)
                    .unwrap();
                sites.push_back(Site {
                    deck,
                    hall,
                    yaw,
                    pickup,
                    boat_start: Vec3::new(endpoints[0].x, a, endpoints[0].y),
                    boat_end: Vec3::new(endpoints[1].x, b, endpoints[1].y),
                });
                if sites.len() >= 64 {
                    return sites;
                }
            }
        }
    }
    sites
}

fn boat_crossing_clear(terrain: &WorldTerrain, deck: &RoadBridge, endpoints: [Vec2; 2]) -> bool {
    let hull = WatercraftClearance::DINGHY;
    let geometry = WaterNavigationGeometry::default();
    if !geometry.segment_clear(terrain, endpoints[0], endpoints[1], hull) {
        return false;
    }
    let steps = endpoints[0].distance(endpoints[1]).ceil().max(1.) as usize;
    (0..=steps).all(|i| {
        let point = endpoints[0].lerp(endpoints[1], i as f32 / steps as f32);
        terrain
            .get_water_height(point.x, point.y)
            .is_some_and(|water| bridge_passage_clear(deck, point, water, hull))
    })
}

fn stage(world: &mut World, directory: &std::path::Path, site: Site) {
    std::fs::create_dir_all(directory).expect("bridge acceptance output");
    let town = world.resource_mut::<WorldIdAllocator>().settlement();
    if let Some(mut clock) = world.query::<&mut WorldTime>().iter_mut(world).next() {
        clock.set_normalized_time(6.0 / 24.0);
    }
    let mut stock = GoodsInventory::new_partitioned(shared::economy::capacity::HALL);
    assert_eq!(
        stock.add(Good::Wood, site.deck.wood_required()),
        site.deck.wood_required()
    );
    assert_eq!(
        stock.add(Good::Stone, site.deck.stone_required()),
        site.deck.stone_required()
    );
    let hall = world
        .spawn((
            town,
            Settlement {
                name: "Bridge Acceptance Lab".into(),
                tier: SettlementTier::Hamlet,
                residents: 0,
                treasury: TREASURY,
            },
            stock,
            MootMarket::founding(),
            PlayerPosition(site.hall),
            PlayerRotation(site.yaw),
            Replicate::to_clients(NetworkTarget::All),
        ))
        .id();
    let manifest = serde_json::json!({"scenario":"regional-bridge","hall":site.hall.to_array(),"pickup":site.pickup.to_array(),"deck":site.deck,
        "boat_start":site.boat_start.to_array(),"boat_end":site.boat_end.to_array(),"wages":WAGES,"treasury":TREASURY,
        "wood":site.deck.wood_required(),"stone":site.deck.stone_required(),
        "cart_capacity":shared::economy::capacity::PORTER,"bank_work_seconds":bridge::required_work(&site.deck),
        "expected_pickups":site.deck.wood_required().div_ceil(shared::economy::capacity::PORTER / Good::Wood.bulk_per_unit()) + site.deck.stone_required().div_ceil(shared::economy::capacity::PORTER / Good::Stone.bulk_per_unit()),"map_hash":world.resource::<WorldTerrain>().generator.active_map_content_hash(),
        "scope":"Controlled initial funded single bridge contract on the unmodified shared village_lab river. Ordinary cargo, navigation, building and wages. Crossing orders issued once after real completion. This does not prove natural investment selection or economy equilibrium."});
    std::fs::write(
        directory.join("fixture.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let now = world.resource::<Time>().elapsed_secs_f64();
    world.insert_resource(Fixture {
        site,
        hall,
        town,
        project: None,
        worker: None,
        bridge: None,
        walker: None,
        boat: None,
        output: BufWriter::new(File::create(directory.join("bridge.jsonl")).unwrap()),
        started: now,
        next_sample: 0.0,
    });
    info!(
        "Regional bridge fixture ready: finite Hall stock; awaiting observer camera before paid construction"
    );
}

fn person(world: &mut World, position: Vec3, name: &str) -> (Entity, PersonId) {
    let id = world.resource_mut::<WorldIdAllocator>().person();
    let mut queue = bevy::ecs::world::CommandQueue::default();
    let mut commands = Commands::new(&mut queue, world);
    let entity = spawn_villager(
        &mut commands,
        world.resource::<WorldTerrain>(),
        id.0.wrapping_add(19000),
        position,
    );
    commands
        .entity(entity)
        .insert((id, CharacterName(name.into())));
    queue.apply(world);
    (entity, id)
}

fn begin_project(world: &mut World, fixture: &mut Fixture) {
    let (worker, id) = person(world, fixture.site.pickup, "Bridgewright");
    world.entity_mut(worker).insert((
        Residence("Bridge Acceptance Lab".into()),
        ResidentOf(fixture.town),
    ));
    // This fixture stages an already approved contract, including its full
    // debit. It never counts artificial trade evidence as natural investment.
    let wood = fixture.site.deck.wood_required();
    let stone = fixture.site.deck.stone_required();
    let mut supplies = GoodsInventory::new(
        wood * Good::Wood.bulk_per_unit() + stone * Good::Stone.bulk_per_unit(),
    );
    for (good, amount) in [(Good::Wood, wood), (Good::Stone, stone)] {
        assert_eq!(
            world
                .get_mut::<GoodsInventory>(fixture.hall)
                .unwrap()
                .remove(good, amount),
            amount
        );
        assert_eq!(supplies.add(good, amount), amount);
    }
    world.get_mut::<Settlement>(fixture.hall).unwrap().treasury -= WAGES;
    let other = world.resource_mut::<WorldIdAllocator>().settlement();
    let day = world.query::<&WorldTime>().iter(world).next().unwrap().day;
    let project = super::projects::start_project(
        world,
        SettlementPair::new(fixture.town, other).unwrap(),
        fixture.hall,
        fixture.town,
        "Bridge Acceptance Lab".into(),
        fixture.site.pickup,
        worker,
        id,
        "Bridgewright".into(),
        vec![RegionalStep::Bridge(fixture.site.deck.clone())],
        supplies,
        WAGES,
        day,
    );
    world
        .resource_mut::<RegionalInfrastructure>()
        .active
        .push(project);
    let bridge = world
        .query::<(Entity, &RegionalRoadSection)>()
        .iter(world)
        .find(|(_, section)| section.project == project)
        .unwrap()
        .0;
    assert!(
        world
            .get::<RoadBridge>(bridge)
            .unwrap()
            .height_at(fixture.site.deck.midpoint().xz(), 0.4)
            .is_none(),
        "unfinished deck cannot grant access"
    );
    fixture.project = Some(project);
    fixture.worker = Some(worker);
    fixture.bridge = Some(bridge);
    info!("Regional bridge fixture: paid contract begins, worker={id:?}, bridge={bridge:?}");
}

fn begin_crossings(world: &mut World, fixture: &mut Fixture) {
    let direction = (fixture.site.deck.end.xz() - fixture.site.deck.start.xz()).normalize();
    let start = fixture.site.deck.start.xz() - direction * 3.0;
    let end = fixture.site.deck.end.xz() + direction * 3.0;
    let terrain = world.resource::<WorldTerrain>();
    let start = Vec3::new(start.x, terrain.get_height(start.x, start.y), start.y);
    let end = Vec3::new(end.x, terrain.get_height(end.x, end.y), end.y);
    let (walker, _) = person(world, start, "Bridge Traveller");
    world.entity_mut(walker).insert((
        CommandedBy("BridgeAcceptanceWalker".into()),
        VillagerIntent::Idle,
        MoveTarget(end),
    ));
    let boat = world
        .spawn((
            PlayerBoat,
            Vessel,
            VesselNavigation::DINGHY,
            PlayerPosition(fixture.site.boat_start),
            PlayerRotation(0.0),
            CharacterMotion::STATIONARY,
            RegionCoord::from_world_pos(fixture.site.boat_start),
            Replicate::to_clients(NetworkTarget::All),
        ))
        .id();
    world
        .resource_mut::<VesselNavigationQueue>()
        .request(boat, VesselGoal::Sail(fixture.site.boat_end.xz()));
    fixture.walker = Some(walker);
    fixture.boat = Some(boat);
    info!(
        "Regional bridge fixture: real finished deck; pedestrian and under-deck vessel orders begin"
    );
}

fn stock(world: &World, entity: Option<Entity>) -> serde_json::Value {
    let stock = entity.and_then(|e| world.get::<GoodsInventory>(e));
    serde_json::json!({"wood":stock.map_or(0,|s|s.amount(Good::Wood)),"stone":stock.map_or(0,|s|s.amount(Good::Stone))})
}
fn actor(world: &World, entity: Option<Entity>) -> serde_json::Value {
    let Some(e) = entity else {
        return serde_json::Value::Null;
    };
    serde_json::json!({"id":world.get::<PersonId>(e).map(|id|id.0),"position":world.get::<PlayerPosition>(e).map(|p|p.0.to_array()),
    "target":world.get::<MoveTarget>(e).map(|p|p.0.to_array()),"activity":world.get::<CharacterActivity>(e),"motion":world.get::<CharacterMotion>(e),
    "simulation":"canonical",
    "bridge_builder":world.get::<bridge::BridgeBuilder>(e).is_some(),
    "cart":world.get::<shared::economy::PorterCartState>(e),
    "capacity":world.get::<GoodsInventory>(e).map_or(0,GoodsInventory::bulk_capacity),"stock":stock(world,Some(e)),"wallet":world.get::<Wallet>(e).map_or(0,|wallet| wallet.balance()),
    "land_route":world.get::<TravelRoute>(e).is_some(),"pending":world.get::<NavigationRoutePending>(e).is_some(),"failed":world.get::<NavigationRouteFailed>(e).is_some(),
    "water_route":world.get::<VesselRoute>(e).is_some(),
    "water_cursor":world.get::<VesselRoute>(e).map(|route| (route.next, route.waypoints.len())),
    "water_pending":world.get_resource::<VesselNavigationQueue>().is_some_and(|queue|queue.is_pending(e)),
    "water_failed_goal":world.get::<VesselRouteFailed>(e).map(|failed|failed.goal.to_array()),
    "water_position_clear":world.get::<Vessel>(e).and_then(|_|world.get::<PlayerPosition>(e)).map(|p| {
        world.resource::<WaterNavigationGeometry>().point_clear(world.resource::<WorldTerrain>(), p.0.xz(), WatercraftClearance::DINGHY)
    })})
}
fn snapshot(world: &mut World, fixture: &Fixture, now: f64, warp: f32) -> serde_json::Value {
    let clock = world
        .query::<&WorldTime>()
        .iter(world)
        .next()
        .cloned()
        .unwrap();
    let project = fixture
        .project
        .and_then(|e| world.get::<RegionalProject>(e));
    let bridge = fixture.bridge.and_then(|e| world.get::<RoadBridge>(e));
    serde_json::json!({"elapsed":now-fixture.started,"day":clock.day,"world_seconds":clock.seconds_in_cycle,"warp":warp,"stage":"after_bridge_before_movement",
        "built":bridge.is_some_and(|b|b.built),"project_status":project.map(|p|format!("{:?}",p.status)),"escrow":project.map_or(0,|p|p.escrow_cash),"wages_paid":project.map_or(0,|p|p.wages_paid),
        "treasury":world.get::<Settlement>(fixture.hall).map_or(0,|s|s.treasury),"hall":stock(world,Some(fixture.hall)),"source":stock(world,fixture.project),"site":stock(world,fixture.bridge),
        "worker":actor(world,fixture.worker),"walker":actor(world,fixture.walker),"boat":actor(world,fixture.boat)})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connected_bridge_fixture_has_a_valid_shared_map_river_span() {
        let mut terrain = WorldTerrain::default();
        terrain.generator = shared::terrain::TerrainGenerator::from_loaded_map(
            shared::map::load_map("village_lab").unwrap(),
        );
        let sites = candidate_sites(&terrain);
        let site = sites
            .front()
            .expect("connected fixture needs a real river span");
        assert!(sites.len() <= 64);
        assert!(site.deck.valid());
        assert!(
            site.deck
                .height_at(site.deck.midpoint().xz(), 0.4)
                .is_none()
        );
        assert!(
            terrain
                .get_water_height(site.deck.start.x, site.deck.start.z)
                .is_none()
        );
        assert!(
            terrain
                .get_water_height(site.deck.end.x, site.deck.end.z)
                .is_none()
        );
        assert!(
            terrain
                .get_water_height(site.boat_start.x, site.boat_start.z)
                .is_some()
        );
        assert!(
            terrain
                .get_water_height(site.boat_end.x, site.boat_end.z)
                .is_some()
        );
        assert!(boat_crossing_clear(
            &terrain,
            &site.deck,
            [site.boat_start.xz(), site.boat_end.xz()]
        ));
        let geometry = WaterNavigationGeometry::default();
        let old_start = Vec2::new(-370.4447, 434.4447);
        let old_end = Vec2::new(-401.55737, 465.55737);
        println!(
            "Old centreline fixture: start_clear={}, end_clear={}, segment_clear={}",
            geometry.point_clear(&terrain, old_start, WatercraftClearance::DINGHY),
            geometry.point_clear(&terrain, old_end, WatercraftClearance::DINGHY),
            geometry.segment_clear(&terrain, old_start, old_end, WatercraftClearance::DINGHY)
        );
        println!(
            "First geometry-certified bridge candidate: {:?}; Hall {:?}; boat {:?}->{:?}",
            site.deck, site.hall, site.boat_start, site.boat_end
        );

        // Exercise the real retained queue against the actual completed deck,
        // not only the fixture's survey helper or a hand-authored route.
        use crate::player::boat::{
            VesselRouteCertification, clearance::rebuild_water_navigation_geometry,
            plan_vessel_routes,
        };
        use bevy::ecs::system::RunSystemOnce;
        let site = site.clone();
        let mut world = World::new();
        world.insert_resource(terrain);
        world.init_resource::<WaterNavigationGeometry>();
        world.init_resource::<VesselNavigationQueue>();
        let mut deck = site.deck.clone();
        deck.built = true;
        world.spawn(deck);
        world
            .run_system_once(rebuild_water_navigation_geometry)
            .unwrap();
        let boat = world
            .spawn((
                Vessel,
                VesselNavigation::DINGHY,
                PlayerPosition(site.boat_start),
                CharacterMotion::STATIONARY,
            ))
            .id();
        world
            .resource_mut::<VesselNavigationQueue>()
            .request(boat, VesselGoal::Sail(site.boat_end.xz()));
        for _ in 0..512 {
            world.run_system_once(plan_vessel_routes).unwrap();
            if !world.resource::<VesselNavigationQueue>().is_pending(boat) {
                break;
            }
        }
        assert!(world.get::<VesselRouteFailed>(boat).is_none());
        assert!(world.get::<VesselRouteCertification>(boat).is_some());
        let route = world
            .get::<VesselRoute>(boat)
            .expect("full-hull fixture must yield a real route beneath its finished deck");
        assert_eq!(route.waypoints.last(), Some(&site.boat_end.xz()));
        let terrain = world.resource::<WorldTerrain>();
        let geometry = world.resource::<WaterNavigationGeometry>();
        let mut previous = site.boat_start.xz();
        for point in &route.waypoints {
            assert!(geometry.segment_clear(terrain, previous, *point, WatercraftClearance::DINGHY));
            previous = *point;
        }
    }
}
