//! Deterministic, accelerated village integration laboratory.
//!
//! This is intentionally an ignored test rather than another runtime mode. It
//! runs the real server systems without networking or rendering, prints a
//! compact timeline, and fails with per-villager diagnostics when embodied work
//! stops making progress. Launch it from the workspace root with:
//!
//! `cargo village-lab`

use std::collections::{HashMap, HashSet, VecDeque};

use bevy::prelude::*;
use shared::components::{
    BuildingDoorDemand, BuildingDoorUse, CharacterActivity, CharacterAffiliation,
    CharacterAttributes, CharacterKind, CharacterName, FarmField, FishingPier, Household,
    MootAdministration, Nutrition, Occupation, PlayerPosition, PlayerRotation, Residence,
    Settlement, SettlementBuilding, SettlementBuildingKind, SettlementTier, TimeWarp, VillageRoad,
    WorkStatus, WorldTime,
};
use shared::economy::{
    BusinessAccount, BusinessWagePolicy, CarriedLoad, Good, GoodsInventory, HouseholdEconomy,
    MootMarket, SettlementEconomy, Wallet, FOOD_SECURITY_TARGET_DAYS, VILLAGE_MIN_PROSPERITY,
    VILLAGE_REQUIRED_SECURE_DAYS,
};
use shared::region::RegionCoord;
use shared::spatial::SpatialObstacleGrid;
use shared::terrain::WorldTerrain;

use crate::collision;
use crate::collision::library::{DerivedColliderLibrary, StaticColliders};
use crate::player::hero::{step_units, MoveTarget};
use crate::world::navgrid::ObstacleGridState;
use crate::world::pathfinding::PathfindingBudgetSettings;
use crate::world::village::{
    self, ConstructionMaterialRoutine, FarmerRoutine, FishingRoutine, HomeRoutine,
    LumberjackRoutine, PublishedTerrainDeltas, SettlementEconomyRuntime, UnderConstruction,
    VillageClock, VillagerIntent, WorkerOffDuty, WorkplaceDoorTransit,
};
use crate::world::village_lab_scenario::{
    choose_poor_site, choose_secure_site, lab_arrival_count, lab_arrival_day, LabScenario,
    POOR_VILLAGERS, SECURE_VILLAGERS,
};
use crate::world::village_roads::{
    self, NavigationRouteFailed, NavigationRoutePending, RoadBuilderRoutine, TravelRoute,
    VillageRoadGraph,
};

const DEFAULT_LAB_WARP: f32 = 100.0;
// The meadow needs one startup day followed by three fully secure day
// boundaries. Continuous quality-scaled production needs the early workplaces
// to accumulate their first physical batches before the secure streak begins;
// 190 minutes covers the third boundary without weakening the rule.
const DEFAULT_LAB_MINUTES: f32 = 190.0;
// A dual foundation can legitimately queue behind several simultaneous road
// surveys. Ten simulated minutes still catches a real deadlock quickly while
// avoiding a false alarm just before a long connector completes at high warp.
const STALL_SECONDS: f32 = 600.0;
const REPORT_SECONDS: f32 = 300.0;

#[derive(Default, Debug)]
struct Evidence {
    saw_chopping: bool,
    saw_building: bool,
    saw_indoors: bool,
    saw_farming: bool,
    saw_fishing: bool,
    saw_sitting: bool,
    saw_wheat_carried: bool,
    saw_wood_carried: bool,
    saw_food_carried: bool,
    saw_road_builder: bool,
    saw_partial_site: bool,
    saw_full_site: bool,
    saw_door_open: bool,
    saw_door_close_after_open: bool,
    saw_daily_consumption: bool,
    saw_market_buying: bool,
    saw_market_selling: bool,
    saw_hunger: bool,
    saw_village_tier: bool,
    coldbarrow_saw_hunger: bool,
    meadow_saw_hunger: bool,
    farmed_workplaces: HashSet<Entity>,
    productive_workers: HashSet<String>,
}

#[derive(Resource, Default, Debug)]
struct LastPlannedRoutes(HashMap<Entity, TravelRoute>);

#[derive(Resource, Default, Debug)]
struct LastRouteHandoffs(HashMap<Entity, String>);

fn remember_planned_routes(
    mut remembered: ResMut<LastPlannedRoutes>,
    routes: Query<(Entity, &TravelRoute)>,
) {
    for (entity, route) in routes.iter() {
        remembered.0.insert(entity, route.clone());
    }
}

fn remember_route_handoffs(
    mut remembered: ResMut<LastRouteHandoffs>,
    movers: Query<(
        Entity,
        &PlayerPosition,
        &MoveTarget,
        Option<&NavigationRoutePending>,
        Option<&TravelRoute>,
        Has<BuildingDoorUse>,
        Has<village::PierTraversal>,
    )>,
) {
    for (entity, position, target, pending, route, door, pier) in movers.iter() {
        remembered.0.insert(
            entity,
            format!(
                "pre-step pos={:.3},{:.3} goal={:.3},{:.3} pending={pending:?} route={route:?} bypass=[door:{door},pier:{pier}]",
                position.0.x, position.0.z, target.0.x, target.0.z,
            ),
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StructureMilestone {
    residents: u32,
    settlements: Vec<(String, SettlementTier, u32)>,
    sites: Vec<(SettlementBuildingKind, &'static str, u32, u32)>,
    buildings: [usize; 8],
    roads: Vec<(u16, usize)>,
    fields: usize,
    piers: usize,
    housed: usize,
}

#[derive(Debug)]
struct PersonView {
    entity: Entity,
    name: String,
    state: String,
    progress_key: String,
    active_progress_expected: bool,
}

const LIFE_EVENT_LIMIT: usize = 64;

/// Bounded, lab-only biography for one resident. This deliberately does not
/// become a production component: live worlds may contain thousands of people,
/// while the lab can afford detailed observation of its small cast.
#[derive(Debug)]
struct LabLifeRecord {
    name: String,
    initial_money: u64,
    current_money: u64,
    money_in: u64,
    money_out: u64,
    initial_attributes: CharacterAttributes,
    current_attributes: CharacterAttributes,
    current_job: String,
    current_status: String,
    current_home: String,
    current_residence: String,
    hungry: bool,
    current_activity: CharacterActivity,
    activity_seconds: HashMap<&'static str, f32>,
    activity_visits: HashMap<&'static str, u32>,
    events: VecDeque<String>,
}

impl LabLifeRecord {
    fn push_event(&mut self, event: String) {
        if self.events.back() == Some(&event) {
            return;
        }
        if self.events.len() == LIFE_EVENT_LIMIT {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }
}

#[derive(Default, Debug)]
struct LabLifeLedger {
    people: HashMap<Entity, LabLifeRecord>,
}

impl LabLifeLedger {
    #[allow(clippy::type_complexity)]
    fn observe(&mut self, world: &mut World, sim_seconds: f32, elapsed_seconds: f32) {
        let homes: HashMap<String, String> = world
            .query::<(&SettlementBuilding, &Household)>()
            .iter(world)
            .flat_map(|(building, household)| {
                household.residents.iter().map(|resident| {
                    (
                        resident.clone(),
                        format!("Cabin in {}", building.settlement),
                    )
                })
            })
            .collect();
        let workplaces: HashMap<String, String> = world
            .query::<(&SettlementBuilding, Option<&BusinessWagePolicy>)>()
            .iter(world)
            .flat_map(|(building, wage)| {
                building.workers.iter().map(move |worker| {
                    let wage = wage.map_or_else(
                        || "public/fixed wage".to_string(),
                        |wage| {
                            format!(
                                "{} coin/day",
                                shared::economy::format_money(wage.daily_wage)
                            )
                        },
                    );
                    (
                        worker.clone(),
                        format!(
                            "{} in {} ({wage})",
                            building.kind.label(),
                            building.settlement
                        ),
                    )
                })
            })
            .collect();
        let snapshots: Vec<_> = world
            .query::<(
                Entity,
                &CharacterName,
                &Wallet,
                Option<&CharacterAttributes>,
                Option<&Occupation>,
                Option<&WorkStatus>,
                Option<&Residence>,
                Option<&Nutrition>,
                &CharacterActivity,
            )>()
            .iter(world)
            .map(
                |(
                    entity,
                    name,
                    wallet,
                    attributes,
                    occupation,
                    status,
                    residence,
                    nutrition,
                    activity,
                )| {
                    (
                        entity,
                        name.0.clone(),
                        wallet.balance(),
                        attributes.copied().unwrap_or_default(),
                        occupation.and_then(|occupation| occupation.0.clone()),
                        status.copied().unwrap_or_default(),
                        residence.map(|residence| residence.0.clone()),
                        nutrition.copied().unwrap_or_default(),
                        *activity,
                    )
                },
            )
            .collect();

        let at = format!("t={:.1}m", sim_seconds / 60.0);
        for (entity, name, money, attributes, occupation, status, residence, nutrition, activity) in
            snapshots
        {
            let job = workplaces.get(&name).cloned().unwrap_or_else(|| {
                occupation
                    .clone()
                    .unwrap_or_else(|| "No workplace".to_string())
            });
            let home = homes
                .get(&name)
                .cloned()
                .unwrap_or_else(|| "Unhoused".to_string());
            let residence = residence.unwrap_or_else(|| "Unsettled".to_string());
            let hungry = nutrition.is_hungry();
            let activity_label = activity.label();
            let record = self.people.entry(entity).or_insert_with(|| {
                let mut events = VecDeque::new();
                events.push_back(format!(
                    "{at} entered ledger: {residence}, {home}, {job}, {}",
                    activity.label()
                ));
                let mut visits = HashMap::new();
                visits.insert(activity_label, 1);
                LabLifeRecord {
                    name: name.clone(),
                    initial_money: money,
                    current_money: money,
                    money_in: 0,
                    money_out: 0,
                    initial_attributes: attributes,
                    current_attributes: attributes,
                    current_job: job.clone(),
                    current_status: status.label().to_string(),
                    current_home: home.clone(),
                    current_residence: residence.clone(),
                    hungry,
                    current_activity: activity,
                    activity_seconds: HashMap::new(),
                    activity_visits: visits,
                    events,
                }
            });
            *record.activity_seconds.entry(activity_label).or_default() += elapsed_seconds;

            if money > record.current_money {
                let change = money - record.current_money;
                record.money_in = record.money_in.saturating_add(change);
                record.push_event(format!(
                    "{at} received {} coin",
                    shared::economy::format_money(change)
                ));
            } else if money < record.current_money {
                let change = record.current_money - money;
                record.money_out = record.money_out.saturating_add(change);
                record.push_event(format!(
                    "{at} paid/contributed {} coin",
                    shared::economy::format_money(change)
                ));
            }
            record.current_money = money;

            if attributes != record.current_attributes {
                record.push_event(format!(
                    "{at} attributes P{} I{} C{} -> P{} I{} C{}",
                    record.current_attributes.physique(),
                    record.current_attributes.intelligence(),
                    record.current_attributes.charm(),
                    attributes.physique(),
                    attributes.intelligence(),
                    attributes.charm(),
                ));
                record.current_attributes = attributes;
            }
            if job != record.current_job {
                record.push_event(format!(
                    "{at} work changed: {} -> {job}",
                    record.current_job
                ));
                record.current_job = job;
            }
            let status = status.label().to_string();
            if status != record.current_status {
                record.push_event(format!(
                    "{at} employment changed: {} -> {status}",
                    record.current_status
                ));
                record.current_status = status;
            }
            if home != record.current_home {
                record.push_event(format!(
                    "{at} home changed: {} -> {home}",
                    record.current_home
                ));
                record.current_home = home;
            }
            if residence != record.current_residence {
                record.push_event(format!(
                    "{at} residence changed: {} -> {residence}",
                    record.current_residence
                ));
                record.current_residence = residence;
            }
            if hungry != record.hungry {
                record.push_event(format!(
                    "{at} food state: {}",
                    if hungry { "became hungry" } else { "ate" }
                ));
                record.hungry = hungry;
            }
            if activity != record.current_activity {
                record.push_event(format!(
                    "{at} activity: {} -> {}",
                    record.current_activity.label(),
                    activity.label()
                ));
                *record.activity_visits.entry(activity_label).or_default() += 1;
                record.current_activity = activity;
            }
        }
    }

    fn print_final_report(&self) {
        if self.people.is_empty() {
            println!("LAB wealth no residents recorded");
            return;
        }
        let mut wealth: Vec<_> = self
            .people
            .values()
            .map(|person| (person.current_money, person.name.as_str()))
            .collect();
        wealth.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)));
        let poorest_money = wealth.first().unwrap().0;
        let richest_money = wealth.last().unwrap().0;
        let poorest: Vec<_> = wealth
            .iter()
            .filter(|(money, _)| *money == poorest_money)
            .map(|(_, name)| *name)
            .collect();
        let richest: Vec<_> = wealth
            .iter()
            .filter(|(money, _)| *money == richest_money)
            .map(|(_, name)| *name)
            .collect();
        let total = wealth
            .iter()
            .fold(0_u64, |sum, (money, _)| sum.saturating_add(*money));
        println!(
            "LAB wealth poorest=[{}] {} coin richest=[{}] {} coin mean={} coin",
            poorest.join(", "),
            shared::economy::format_money(poorest_money),
            richest.join(", "),
            shared::economy::format_money(richest_money),
            shared::economy::format_money(total / wealth.len() as u64),
        );
        let bottom: Vec<_> = wealth
            .iter()
            .take(3)
            .map(|(money, name)| format!("{name}={}", shared::economy::format_money(*money)))
            .collect();
        let top: Vec<_> = wealth
            .iter()
            .rev()
            .take(3)
            .map(|(money, name)| format!("{name}={}", shared::economy::format_money(*money)))
            .collect();
        println!(
            "LAB wealth bottom3=[{}] top3=[{}]",
            bottom.join(", "),
            top.join(", ")
        );

        let mut people: Vec<_> = self.people.values().collect();
        people.sort_by(|a, b| a.name.cmp(&b.name));
        for person in people {
            let mut activities: Vec<_> = person
                .activity_seconds
                .iter()
                .map(|(activity, seconds)| {
                    (
                        *seconds,
                        format!(
                            "{activity}={:.1}m/{}x",
                            seconds / 60.0,
                            person.activity_visits.get(activity).copied().unwrap_or(0)
                        ),
                    )
                })
                .collect();
            activities.sort_by(|a, b| b.0.total_cmp(&a.0));
            println!(
                "LAB life {} wallet={}->{} in={} out={} attrs=P{} I{} C{}->P{} I{} C{} residence='{}' home='{}' work='{}' status='{}' hungry={} activities=[{}]",
                person.name,
                shared::economy::format_money(person.initial_money),
                shared::economy::format_money(person.current_money),
                shared::economy::format_money(person.money_in),
                shared::economy::format_money(person.money_out),
                person.initial_attributes.physique(),
                person.initial_attributes.intelligence(),
                person.initial_attributes.charm(),
                person.current_attributes.physique(),
                person.current_attributes.intelligence(),
                person.current_attributes.charm(),
                person.current_residence,
                person.current_home,
                person.current_job,
                person.current_status,
                person.hungry,
                activities.into_iter().map(|(_, row)| row).collect::<Vec<_>>().join(", "),
            );
            println!(
                "  LAB history {}: {}",
                person.name,
                person
                    .events
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" -> ")
            );
        }
    }
}

fn env_f32(name: &str, fallback: f32) -> f32 {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.parse::<f32>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(fallback)
}

fn configure_lab(app: &mut App) {
    app.init_resource::<Time>();
    app.init_resource::<VillageClock>();
    app.init_resource::<SettlementEconomyRuntime>();
    app.init_resource::<village::ambient::AmbientClock>();
    app.init_resource::<village::ambient::AmbientSpotCache>();
    app.init_resource::<PublishedTerrainDeltas>();
    app.init_resource::<VillageRoadGraph>();
    app.init_resource::<crate::world::identity::WorldIdAllocator>();
    app.init_resource::<crate::world::identity::WorldIdentityIndex>();
    app.init_resource::<crate::world::settlement_directory::SettlementDirectory>();
    app.init_resource::<village::history::SettlementHistoryRuntime>();
    app.init_resource::<LastPlannedRoutes>();
    app.init_resource::<LastRouteHandoffs>();
    app.init_resource::<collision::building_index::BuildingSpatialIndex>();
    app.init_resource::<collision::streaming::ColliderStreamingState>();
    app.init_resource::<ObstacleGridState>();
    app.init_resource::<SpatialObstacleGrid>();
    app.insert_resource(PathfindingBudgetSettings {
        max_requests_per_tick: 16,
        ..default()
    });
    app.add_systems(Startup, collision::library::setup_baked_colliders);

    village::schedule::configure_shared_village_simulation(app, Update);
    app.configure_sets(
        Update,
        village::schedule::VillageSimulationSet::Core
            .after(crate::world::navgrid::sync_obstacle_grid),
    );

    // Environment preparation remains lab-specific; all behaviour and local
    // navigation after this point comes from the production registration.
    app.add_systems(
        Update,
        (
            village::claim_settlement_hall_obstacles,
            crate::world::time::update_world_time,
            collision::building_index::sync_building_spatial_index,
            collision::streaming::update_static_collider_streaming,
            crate::world::navgrid::sync_obstacle_grid,
        )
            .chain(),
    );
    app.add_systems(
        Update,
        (remember_planned_routes, remember_route_handoffs)
            .chain()
            .after(village_roads::plan_villager_travel_routes)
            .before(step_units),
    );
}

fn spawn_lab_village(
    world: &mut World,
    name: &str,
    resident_prefix: &str,
    hall_position: Vec3,
    resident_count: usize,
    founding_wood: u32,
) {
    let mut hall_inventory = GoodsInventory::new(shared::economy::capacity::HALL);
    hall_inventory.add(Good::Wood, founding_wood);
    world.spawn((
        Settlement {
            name: name.to_string(),
            tier: SettlementTier::Hamlet,
            residents: 0,
            treasury: shared::economy::STARTING_TREASURY_MONEY,
        },
        hall_inventory,
        shared::economy::MootMarket::founding(),
        shared::components::SettlementPolicies::poor_relief(),
        PlayerPosition(hall_position),
        PlayerRotation(0.0),
    ));

    let entrance = SettlementBuildingKind::Hall.entrance_position(hall_position, 0.0);
    for index in 0..resident_count {
        let angle = index as f32 / resident_count as f32 * std::f32::consts::TAU;
        let x = entrance.x + (index as f32 - (resident_count as f32 - 1.0) * 0.5) * 0.55;
        let z = entrance.z - 0.45 - (index % 2) as f32 * 0.45;
        let y = world.resource::<WorldTerrain>().get_height(x, z);
        let position = Vec3::new(x, y, z);
        world.spawn((
            CharacterName(format!("{resident_prefix}{index}")),
            CharacterKind::Villager,
            CharacterAffiliation::default(),
            CharacterActivity::Idle,
            Occupation::default(),
            GoodsInventory::new(shared::economy::capacity::VILLAGER),
            CarriedLoad::default(),
            shared::economy::Wallet::founding_villager(),
            PlayerPosition(position),
            PlayerRotation(angle),
            RegionCoord::from_world_pos(position),
        ));
    }
}

fn spawn_lab_arrivals(world: &mut World, hall_position: Vec3, count: usize) {
    let entrance = SettlementBuildingKind::Hall.entrance_position(hall_position, 0.0);
    for index in 0..count {
        let x = entrance.x + (index as f32 - (count as f32 - 1.0) * 0.5) * 0.55;
        let z = entrance.z - 0.45 - (index % 2) as f32 * 0.45;
        let y = world.resource::<WorldTerrain>().get_height(x, z);
        let position = Vec3::new(x, y, z);
        world.spawn((
            CharacterName(format!("MeadowArrival{index}")),
            CharacterKind::Villager,
            CharacterAffiliation::default(),
            CharacterActivity::Idle,
            Occupation::default(),
            GoodsInventory::new(shared::economy::capacity::VILLAGER),
            CarriedLoad::default(),
            Wallet::founding_villager(),
            PlayerPosition(position),
            PlayerRotation(0.0),
            RegionCoord::from_world_pos(position),
        ));
    }
}

fn spawn_scenario(world: &mut World, warp: f32, scenario: LabScenario) {
    let (secure, poor) = {
        let terrain = world.resource::<WorldTerrain>();
        let secure = scenario
            .includes_secure()
            .then(|| choose_secure_site(terrain));
        let poor = scenario
            .includes_poor()
            .then(|| choose_poor_site(terrain, secure.map(|choice| choice.0)));
        (secure, poor)
    };

    println!(
        "LAB map=village_lab scenario={scenario:?} villages={} warp={}x",
        usize::from(secure.is_some()) + usize::from(poor.is_some()),
        warp,
    );

    if let Some((hall, trees, hut, rotation, fishing_quality, farmland)) = secure {
        println!(
            "LAB secure='Lab Meadow' hall=({:.1},{:.1},{:.1}) farmland={:.0}% trees={} fishing_hut=({:.1},{:.1},{:.1}) fishing={:.0}% rotation={:.3}",
            hall.x,
            hall.y,
            hall.z,
            farmland * 100.0,
            trees,
            hut.x,
            hut.y,
            hut.z,
            fishing_quality * 100.0,
            rotation,
        );
        spawn_lab_village(
            world,
            "Lab Meadow",
            "MeadowResident",
            hall,
            SECURE_VILLAGERS,
            80,
        );
    }

    if let Some((hall, trees, farmland)) = poor {
        println!(
            "LAB poor='Lab Coldbarrow' hall=({:.1},{:.1},{:.1}) farmland={:.1}% trees={} fishing=none",
            hall.x,
            hall.y,
            hall.z,
            farmland * 100.0,
            trees,
        );
        spawn_lab_village(
            world,
            "Lab Coldbarrow",
            "ColdResident",
            hall,
            POOR_VILLAGERS,
            80,
        );
    }

    world.spawn((WorldTime::new_default(), TimeWarp::clamped(warp)));
}

fn building_index(kind: SettlementBuildingKind) -> usize {
    match kind {
        SettlementBuildingKind::Hall => 0,
        SettlementBuildingKind::Farmstead => 1,
        SettlementBuildingKind::LumberjackHut => 2,
        SettlementBuildingKind::FishermansHut => 3,
        SettlementBuildingKind::House => 4,
        SettlementBuildingKind::Market => 5,
        SettlementBuildingKind::Tavern => 6,
        SettlementBuildingKind::Church => 7,
    }
}

fn milestone(world: &mut World) -> StructureMilestone {
    let mut settlements: Vec<_> = world
        .query::<&Settlement>()
        .iter(world)
        .map(|settlement| {
            (
                settlement.name.clone(),
                settlement.tier,
                settlement.residents,
            )
        })
        .collect();
    settlements.sort_by(|a, b| a.0.cmp(&b.0));
    let residents = settlements.iter().map(|(_, _, residents)| residents).sum();
    let mut sites: Vec<_> = world
        .query::<(&UnderConstruction, &GoodsInventory)>()
        .iter(world)
        .map(|(site, inventory)| {
            let stage = match site.stage {
                village::BuildStage::Supplying => "supply",
                village::BuildStage::Walking => "walk",
                village::BuildStage::Raising { .. } => "raise",
            };
            (
                site.kind,
                stage,
                inventory.amount(Good::Wood),
                site.kind.construction_wood_required(),
            )
        })
        .collect();
    sites.sort_by_key(|(kind, _, _, _)| building_index(*kind));

    let mut buildings = [0usize; 8];
    for building in world.query::<&SettlementBuilding>().iter(world) {
        buildings[building_index(building.kind)] += 1;
    }
    let mut roads: Vec<_> = world
        .query::<&VillageRoad>()
        .iter(world)
        .map(|road| (road.built_through, road.points.len()))
        .collect();
    roads.sort_unstable();
    let fields = world.query::<&FarmField>().iter(world).count();
    let piers = world.query::<&FishingPier>().iter(world).count();
    let housed = world
        .query::<&Household>()
        .iter(world)
        .map(|house| house.residents.len())
        .sum();
    StructureMilestone {
        residents,
        settlements,
        sites,
        buildings,
        roads,
        fields,
        piers,
        housed,
    }
}

#[allow(clippy::type_complexity)]
fn people(world: &mut World) -> Vec<PersonView> {
    let failed_routes: HashMap<_, _> = world
        .query::<(Entity, &NavigationRouteFailed)>()
        .iter(world)
        .map(|(entity, failed)| (entity, *failed))
        .collect();
    let door_bypasses: HashSet<_> = world
        .query_filtered::<Entity, With<BuildingDoorUse>>()
        .iter(world)
        .collect();
    let pier_bypasses: HashSet<_> = world
        .query_filtered::<Entity, With<village::PierTraversal>>()
        .iter(world)
        .collect();
    let market_collections: HashMap<_, _> = world
        .query::<(Entity, &village::MarketCollectionRoutine)>()
        .iter(world)
        .map(|(entity, routine)| (entity, routine.clone()))
        .collect();
    let off_duty_workers: HashMap<_, _> = world
        .query::<(Entity, &WorkerOffDuty)>()
        .iter(world)
        .map(|(entity, value)| (entity, format!("{value:?}")))
        .collect();
    let work_states: HashMap<_, _> = world
        .query::<(Entity, Option<&WorkStatus>, Option<&Occupation>)>()
        .iter(world)
        .map(|(entity, status, occupation)| {
            (
                entity,
                (
                    status.copied(),
                    occupation.and_then(|occupation| occupation.0.clone()),
                ),
            )
        })
        .collect();
    world
        .query::<(
            Entity,
            &CharacterName,
            &VillagerIntent,
            &CharacterActivity,
            &PlayerPosition,
            Option<&MoveTarget>,
            Option<&NavigationRoutePending>,
            Option<&RoadBuilderRoutine>,
            Option<&FarmerRoutine>,
            Option<&LumberjackRoutine>,
            Option<&FishingRoutine>,
            Option<&HomeRoutine>,
            Option<&WorkplaceDoorTransit>,
            Option<&ConstructionMaterialRoutine>,
            &CarriedLoad,
        )>()
        .iter(world)
        .map(
            |(
                entity,
                name,
                intent,
                activity,
                position,
                target,
                pending,
                road,
                farmer,
                lumberjack,
                fisher,
                home,
                door,
                construction,
                carried,
            )| {
                let failed = failed_routes.get(&entity);
                let door_collision_bypass = door_bypasses.contains(&entity);
                let pier_collision_bypass = pier_bypasses.contains(&entity);
                let market_collection = market_collections.get(&entity);
                let off_duty = off_duty_workers.get(&entity);
                let (work_status, occupation) = work_states
                    .get(&entity)
                    .map_or((None, None), |(status, occupation)| {
                        (*status, occupation.as_deref())
                    });
                let state = format!(
                    "{} {:?} {:?} pos={:.1},{:.1} target={} pending={} failed={} road={} farm={} wood={} fish={} market={} home={} door={} off_duty={} work={:?}/{:?} bypass=[door:{door_collision_bypass},pier:{pier_collision_bypass}] supply={} carry={:?}:{}",
                    name.0,
                    intent,
                    activity,
                    position.0.x,
                    position.0.z,
                    target.map_or_else(|| "-".to_string(), |target| format!("{:.1},{:.1}", target.0.x, target.0.z)),
                    pending.map_or_else(|| "-".to_string(), |value| format!("{value:?}")),
                    failed.map_or_else(|| "-".to_string(), |value| format!("{value:?}")),
                    road.map_or_else(|| "-".to_string(), |value| format!("{value:?}")),
                    farmer.map_or_else(|| "-".to_string(), |value| format!("{value:?}")),
                    lumberjack.map_or_else(|| "-".to_string(), |value| format!("{value:?}")),
                    fisher.map_or_else(|| "-".to_string(), |value| format!("{value:?}")),
                    market_collection.map_or_else(|| "-".to_string(), |value| format!("{value:?}")),
                    home.map_or_else(|| "-".to_string(), |value| format!("{value:?}")),
                    door.map_or_else(|| "-".to_string(), |value| format!("{value:?}")),
                    off_duty.map_or("-", String::as_str),
                    work_status,
                    occupation,
                    construction.map_or_else(|| "-".to_string(), |value| format!("{value:?}")),
                    carried.good,
                    carried.amount,
                );
                let active_progress_expected = target.is_some()
                    || pending.is_some()
                    || failed.is_some()
                    || door.is_some()
                    || construction.is_some_and(|routine| !routine.is_waiting_for_materials())
                    || (road.is_some() && home.is_none());
                // Retry counters and route objects are diagnostics, not
                // embodied progress. Using the full state string here let a
                // villager stand on one spot forever while attempts cycled
                // 0→1→2→failed→0, continuously resetting the stall clock.
                let movement_owned = target.is_some()
                    || pending.is_some()
                    || failed.is_some()
                    || door.is_some();
                let progress_key = if movement_owned {
                    format!(
                        "move:{:.1},{:.1}:goal={}:activity={activity:?}",
                        position.0.x,
                        position.0.z,
                        target.map_or_else(
                            || "-".to_string(),
                            |target| format!("{:.1},{:.1}", target.0.x, target.0.z)
                        )
                    )
                } else {
                    state.clone()
                };
                PersonView {
                    entity,
                    name: name.0.clone(),
                    state,
                    progress_key,
                    active_progress_expected,
                }
            },
        )
        .collect()
}

fn update_evidence(world: &mut World, evidence: &mut Evidence) {
    for activity in world.query::<&CharacterActivity>().iter(world) {
        evidence.saw_chopping |= *activity == CharacterActivity::Chopping;
        evidence.saw_building |= *activity == CharacterActivity::Building;
        evidence.saw_indoors |= *activity == CharacterActivity::Indoors;
        evidence.saw_farming |= *activity == CharacterActivity::Farming;
        evidence.saw_fishing |= *activity == CharacterActivity::Fishing;
        evidence.saw_sitting |= *activity == CharacterActivity::Sitting;
    }
    for (routine, activity) in world
        .query::<(&FarmerRoutine, &CharacterActivity)>()
        .iter(world)
    {
        if *activity == CharacterActivity::Farming {
            evidence.farmed_workplaces.insert(routine.farmstead());
        }
    }
    for (name, activity, farmer, fisher, lumberjack) in world
        .query::<(
            &CharacterName,
            &CharacterActivity,
            Option<&FarmerRoutine>,
            Option<&FishingRoutine>,
            Option<&LumberjackRoutine>,
        )>()
        .iter(world)
    {
        let performed_job = (farmer.is_some() && *activity == CharacterActivity::Farming)
            || (fisher.is_some() && *activity == CharacterActivity::Fishing)
            || (lumberjack.is_some() && *activity == CharacterActivity::Chopping);
        if performed_job {
            evidence.productive_workers.insert(name.0.clone());
        }
    }
    evidence.saw_road_builder |= world
        .query::<&RoadBuilderRoutine>()
        .iter(world)
        .next()
        .is_some();
    for load in world.query::<&CarriedLoad>().iter(world) {
        evidence.saw_wheat_carried |= load.good == Some(Good::Wheat) && load.amount > 0;
        evidence.saw_wood_carried |= load.good == Some(Good::Wood) && load.amount > 0;
        evidence.saw_food_carried |= load.good == Some(Good::Food) && load.amount > 0;
    }
    for (site, inventory) in world
        .query::<(&UnderConstruction, &GoodsInventory)>()
        .iter(world)
    {
        let delivered = inventory.amount(Good::Wood);
        let required = site.kind.construction_wood_required();
        evidence.saw_partial_site |= delivered > 0 && delivered < required;
        evidence.saw_full_site |= required > 0 && delivered >= required;
    }
    let mut doors = world.query::<&BuildingDoorDemand>();
    let any_door = doors.iter(world).next().is_some();
    let any_open = doors.iter(world).any(|demand| demand.open);
    if evidence.saw_door_open && any_door && !any_open {
        evidence.saw_door_close_after_open = true;
    }
    evidence.saw_door_open |= any_open;
    for (settlement, economy) in world
        .query::<(&Settlement, &SettlementEconomy)>()
        .iter(world)
    {
        evidence.saw_daily_consumption |= economy.recent_food_consumption > 0.0;
        evidence.saw_hunger |= economy.unmet_food > 0;
        if settlement.name == "Lab Coldbarrow" {
            evidence.coldbarrow_saw_hunger |= economy.unmet_food > 0;
        } else if settlement.name == "Lab Meadow" {
            evidence.meadow_saw_hunger |= economy.unmet_food > 0;
        }
    }
    for market in world.query::<&MootMarket>().iter(world) {
        evidence.saw_market_buying |= Good::ALL
            .iter()
            .any(|good| market.pool(*good).units_bought > 0);
        evidence.saw_market_selling |= Good::ALL
            .iter()
            .any(|good| market.pool(*good).units_sold > 0);
    }
    evidence.saw_village_tier |= world
        .query::<&Settlement>()
        .iter(world)
        .any(|settlement| settlement.tier == SettlementTier::Village);
}

/// Every penny must be in exactly one authoritative place, including owner
/// shares queued between the producer and payment systems in the same tick.
fn total_money(world: &mut World) -> u64 {
    let wallets = world
        .query::<&Wallet>()
        .iter(world)
        .map(|wallet| wallet.balance())
        .sum::<u64>();
    let treasuries = world
        .query::<&Settlement>()
        .iter(world)
        .map(|settlement| settlement.treasury)
        .sum::<u64>();
    let market_cash = world
        .query::<&MootMarket>()
        .iter(world)
        .map(MootMarket::total_liquidity)
        .sum::<u64>();
    let pending = world
        .query::<&village::PendingMarketPayment>()
        .iter(world)
        .map(|payment| payment.pennies)
        .sum::<u64>();
    let business_cash = world
        .query::<&BusinessAccount>()
        .iter(world)
        .map(|account| account.cash)
        .sum::<u64>();
    let household_cash = world
        .query::<&HouseholdEconomy>()
        .iter(world)
        .map(|household| household.pennies)
        .sum::<u64>();
    let reserved_collections = world
        .query::<&village::MarketCollectionRoutine>()
        .iter(world)
        .map(village::MarketCollectionRoutine::reserved_pennies)
        .sum::<u64>();
    wallets
        .saturating_add(treasuries)
        .saturating_add(market_cash)
        .saturating_add(pending)
        .saturating_add(business_cash)
        .saturating_add(household_cash)
        .saturating_add(reserved_collections)
}

fn goods_totals(world: &mut World) -> (u32, u32, u32) {
    world.query::<&GoodsInventory>().iter(world).fold(
        (0, 0, 0),
        |(wood, wheat, food), inventory| {
            (
                wood + inventory.amount(Good::Wood),
                wheat + inventory.amount(Good::Wheat),
                food + inventory.amount(Good::Food),
            )
        },
    )
}

fn print_report(world: &mut World, sim_seconds: f32, verbose: bool) {
    let snapshot = milestone(world);
    let (wood, wheat, food) = goods_totals(world);
    let mut economy_rows: Vec<_> = world
        .query::<(&Settlement, &SettlementEconomy)>()
        .iter(world)
        .map(|(settlement, economy)| {
            format!(
                "{}:{} pop={} stock={} reserve={:.1}d prod={:.1}/d eaten={:.1}/d hungry={} prosperity={:.0} secure={}d",
                settlement.name,
                settlement.tier.label(),
                settlement.residents,
                economy.edible_stock,
                economy.reserve_days,
                economy.recent_food_production,
                economy.recent_food_consumption,
                economy.unmet_food,
                economy.prosperity,
                economy.food_secure_days,
            )
        })
        .collect();
    economy_rows.sort();
    let complete_roads = snapshot
        .roads
        .iter()
        .filter(|(built, total)| usize::from(*built) >= *total)
        .count();
    println!(
        "LAB t={:>5.1}m residents={} sites={:?} buildings=[farm:{} lumber:{} fisher:{} house:{}] roads={}/{} fields={} piers={} housed={} goods=[wood:{} wheat:{} food:{}] money={}",
        sim_seconds / 60.0,
        snapshot.residents,
        snapshot.sites,
        snapshot.buildings[1],
        snapshot.buildings[2],
        snapshot.buildings[3],
        snapshot.buildings[4],
        complete_roads,
        snapshot.roads.len(),
        snapshot.fields,
        snapshot.piers,
        snapshot.housed,
        wood,
        wheat,
        food,
        shared::economy::format_money(total_money(world)),
    );
    if !economy_rows.is_empty() {
        println!("LAB economy [{}]", economy_rows.join(" | "));
    }
    if verbose {
        for person in people(world) {
            println!("  {}", person.state);
        }
    }
}

fn assert_lab_outcome(
    world: &mut World,
    evidence: &Evidence,
    scenario: LabScenario,
    expected_residents: usize,
) {
    let snapshot = milestone(world);
    let mut built: HashMap<String, HashMap<SettlementBuildingKind, usize>> = HashMap::new();
    for building in world.query::<&SettlementBuilding>().iter(world) {
        *built
            .entry(building.settlement.clone())
            .or_default()
            .entry(building.kind)
            .or_default() += 1;
    }
    let roads: Vec<_> = world.query::<&VillageRoad>().iter(world).cloned().collect();
    let crop_fields: Vec<_> = world
        .query::<(&FarmField, &PlayerPosition, &PlayerRotation)>()
        .iter(world)
        .map(|(field, position, rotation)| {
            (
                field.settlement.clone(),
                Vec2::new(position.0.x, position.0.z),
                rotation.0,
            )
        })
        .collect();
    let building_doors: Vec<_> = world
        .query::<(&SettlementBuilding, &PlayerPosition, &PlayerRotation)>()
        .iter(world)
        .map(|(building, position, rotation)| {
            let door = building.kind.entrance_position(position.0, rotation.0);
            (
                building.settlement.clone(),
                building.kind,
                Vec2::new(door.x, door.z),
            )
        })
        .collect();
    let building_plots: Vec<_> = world
        .query::<(&SettlementBuilding, &PlayerPosition, &PlayerRotation)>()
        .iter(world)
        .map(|(building, position, rotation)| (building.kind, position.0, rotation.0))
        .collect();
    let economies: HashMap<_, _> = world
        .query::<(&Settlement, &SettlementEconomy)>()
        .iter(world)
        .map(|(settlement, economy)| {
            (
                settlement.name.clone(),
                (settlement.clone(), economy.clone()),
            )
        })
        .collect();
    let administrations: Vec<_> = world
        .query::<(&Settlement, &MootAdministration)>()
        .iter(world)
        .map(|(settlement, administration)| (settlement.name.clone(), administration.clone()))
        .collect();
    let people = people(world);
    let (wood, wheat, food) = goods_totals(world);

    assert_eq!(snapshot.residents, expected_residents as u32, "{people:#?}");
    assert_eq!(
        administrations.len(),
        economies.len(),
        "every Moot Hall must publish its administration"
    );
    for (settlement, administration) in administrations {
        assert!(
            administration.road_steward.is_some(),
            "{settlement} never staffed its Road Steward position"
        );
        assert!(
            administration.last_road_audit_day > 0,
            "{settlement}'s Road Steward never completed a later audit"
        );
        assert_eq!(
            (
                administration.roadless_buildings,
                administration.disconnected_buildings,
                administration.pending_road_buildings,
            ),
            (0, 0, 0),
            "{settlement}'s final road audit still found an isolated or unfinished building"
        );
    }
    assert!(!roads.is_empty(), "no village paths were built");
    assert_eq!(
        roads.len(),
        building_doors.len(),
        "every completed building must build its own connector, even beside the network"
    );
    assert!(
        roads.iter().all(|road| road.is_complete()),
        "unfinished roads: {roads:#?}\npeople: {people:#?}"
    );
    let terrain = world.resource::<WorldTerrain>();
    for (kind, position, rotation) in building_plots {
        let required = if kind == SettlementBuildingKind::FishermansHut {
            shared::components::SETTLEMENT_FREEBOARD
        } else {
            village::FREEBOARD
        };
        assert!(
            shared::components::minimum_building_water_clearance(terrain, position, kind, rotation,)
                >= required,
            "{kind:?} at {position:?} overlaps the local river/ocean surface",
        );
        if kind == SettlementBuildingKind::LumberjackHut {
            let entrance = kind.entrance_position(position, rotation);
            assert!(
                village::lumber_plot_has_reachable_tree(terrain, entrance),
                "Lumberjack Hut at {position:?} has no reachable tree within its work radius"
            );
        }
    }
    for road in &roads {
        assert!(
            road.points
                .windows(2)
                .all(|pair| { village_roads::road_segment_is_dry(terrain, pair[0], pair[1]) }),
            "village path enters local river/ocean water: {:?}",
            road.points,
        );
    }
    let field_half = SettlementBuildingKind::Farmstead
        .field_half_extents()
        .expect("Farmstead has an authored crop footprint");
    for (settlement, center, rotation) in crop_fields {
        let field_position = Vec3::new(center.x, terrain.get_height(center.x, center.y), center.y);
        assert!(
            shared::components::minimum_rotated_rect_water_clearance(
                terrain,
                field_position,
                field_half + Vec2::splat(shared::components::FARM_FIELD_EDGE_CLEARANCE),
                rotation,
            ) >= village::FREEBOARD,
            "wheat field at {center:?} overlaps the local river/ocean surface",
        );
        for road in roads.iter().filter(|road| road.settlement == settlement) {
            assert!(
                !road.intersects_rotated_rect(
                    center,
                    field_half,
                    rotation,
                    shared::components::FARM_FIELD_EDGE_CLEARANCE,
                ),
                "village path {:?} overlaps the wheat field at {center:?}",
                road.points,
            );
        }
    }
    for (settlement, kind, door) in building_doors {
        assert!(
            roads.iter().any(|road| {
                road.settlement == settlement
                    && road
                        .built_points()
                        .iter()
                        .any(|point| point.distance_squared(door) <= 2.1_f32.powi(2))
            }),
            "{kind:?} door {door:?} does not connect to a completed village path: {roads:#?}"
        );
    }
    assert!(
        world
            .query::<&village_roads::RoadRequest>()
            .iter(world)
            .next()
            .is_none(),
        "a completed building still has an unresolved road request"
    );
    let count = |place: &str, kind: SettlementBuildingKind| {
        built
            .get(place)
            .and_then(|kinds| kinds.get(&kind))
            .copied()
            .unwrap_or(0)
    };
    if scenario.includes_secure() {
        assert!(
            count("Lab Meadow", SettlementBuildingKind::Farmstead) >= 1,
            "the secure meadow never added a Farmstead: {built:#?}"
        );
        assert!(
            count("Lab Meadow", SettlementBuildingKind::FishermansHut) >= 1,
            "usable meadow shoreline never added a Fisherman's Hut: {built:#?}"
        );
        assert!(
            count("Lab Meadow", SettlementBuildingKind::House) >= 2,
            "secure village missed repeated housing despite local timber: {built:#?}"
        );
        assert!(
            count("Lab Meadow", SettlementBuildingKind::LumberjackHut) >= 1,
            "the secure meadow never used its reachable local grove: {built:#?}"
        );
        let (settlement, economy) = economies
            .get("Lab Meadow")
            .expect("secure settlement economy missing");
        assert_eq!(settlement.tier, SettlementTier::Village, "{economy:#?}");
        assert!(
            economy.reserve_days >= FOOD_SECURITY_TARGET_DAYS,
            "secure reserve was only {:.1} days: {economy:#?}",
            economy.reserve_days
        );
        assert!(
            economy.recent_food_production >= settlement.residents as f32,
            "secure food production did not cover residents: {economy:#?}"
        );
        assert_eq!(economy.unmet_food, 0, "secure residents went hungry");
        assert!(
            economy.prosperity >= VILLAGE_MIN_PROSPERITY
                && economy.food_secure_days >= VILLAGE_REQUIRED_SECURE_DAYS,
            "secure Hamlet never sustained its Village requirements: {economy:#?}"
        );
    }
    if scenario.includes_poor() {
        assert!(
            count("Lab Coldbarrow", SettlementBuildingKind::Farmstead) >= 2,
            "food shortage did not request repeated Farmsteads: {built:#?}"
        );
        assert_eq!(
            count("Lab Coldbarrow", SettlementBuildingKind::FishermansHut),
            0,
            "the inland cold control unexpectedly found fishing access"
        );
        assert!(
            count("Lab Coldbarrow", SettlementBuildingKind::LumberjackHut) >= 1
                && count("Lab Coldbarrow", SettlementBuildingKind::House) >= 2,
            "food-poor village missed lumber or repeated housing: {built:#?}"
        );
        let (settlement, economy) = economies
            .get("Lab Coldbarrow")
            .expect("food-poor settlement economy missing");
        assert_eq!(settlement.tier, SettlementTier::Hamlet, "{economy:#?}");
        assert!(
            evidence.coldbarrow_saw_hunger,
            "the frozen inland control was never measurably food-poor"
        );
        assert!(
            economy.reserve_days < FOOD_SECURITY_TARGET_DAYS
                || economy.recent_food_production < settlement.residents as f32
                || economy.unmet_food > 0
                || economy.food_secure_days < VILLAGE_REQUIRED_SECURE_DAYS,
            "the food-poor control unexpectedly satisfied every advancement requirement: {economy:#?}"
        );
        assert!(
            !evidence.meadow_saw_hunger,
            "the fertile meadow control went hungry: {evidence:#?}"
        );
    }
    let farm_count: usize = built
        .values()
        .map(|kinds| {
            kinds
                .get(&SettlementBuildingKind::Farmstead)
                .copied()
                .unwrap_or(0)
        })
        .sum();
    let fisher_count: usize = built
        .values()
        .map(|kinds| {
            kinds
                .get(&SettlementBuildingKind::FishermansHut)
                .copied()
                .unwrap_or(0)
        })
        .sum();
    assert_eq!(
        snapshot.fields,
        farm_count * shared::components::FARM_FIELDS_PER_FARMSTEAD as usize,
        "every completed Farmstead should have both production fields"
    );
    assert_eq!(snapshot.piers, fisher_count);
    assert_eq!(snapshot.housed, expected_residents);
    assert!(
        world
            .query::<&RoadBuilderRoutine>()
            .iter(world)
            .next()
            .is_none(),
        "a road builder remained stuck: {people:#?}"
    );
    assert!(evidence.saw_road_builder, "road construction never began");
    assert!(
        evidence.saw_building,
        "road/build animation was never observable"
    );
    assert!(evidence.saw_farming, "field work never occurred");
    if scenario.includes_secure() {
        assert!(evidence.saw_fishing, "pier work never occurred");
    }
    assert!(evidence.saw_indoors, "building interiors were never used");
    assert!(
        evidence.saw_sitting,
        "an unemployed or unhoused resident never used an ambient resting place"
    );
    assert!(
        evidence.saw_door_open,
        "villagers used interiors but never raised a building door-open demand"
    );
    assert!(
        evidence.saw_door_close_after_open,
        "a building door opened but never returned to closed demand"
    );
    assert!(evidence.saw_wood_carried, "wood was never visibly carried");
    if scenario.includes_secure() {
        assert!(
            evidence.saw_wheat_carried,
            "wheat was never visibly carried"
        );
        assert!(
            evidence.saw_food_carried,
            "fish Food was never visibly carried"
        );
        assert!(
            evidence.saw_daily_consumption,
            "no resident ever consumed a daily food portion"
        );
        assert!(
            evidence.saw_market_buying,
            "the Moot never bought physical producer output"
        );
        assert!(
            evidence.saw_market_selling,
            "no household or builder ever bought from the Moot"
        );
        assert!(
            evidence.saw_village_tier,
            "no food-secure Hamlet advanced to Village"
        );
    }
    if scenario.includes_poor() {
        assert!(
            evidence.saw_hunger,
            "food-poor residents never registered hunger"
        );
    }
    assert!(
        evidence.saw_partial_site,
        "worksite supply never accumulated"
    );
    assert!(
        evidence.saw_full_site,
        "no worksite received its full requirement"
    );
    assert!(wood > 0, "no wood remained anywhere in the village");
    if scenario.includes_secure() {
        assert!(wheat > 0, "no edible wheat remained in the secure village");
        assert!(food > 0, "no fish Food remained in the secure village");
    }
    assert!(world
        .query::<&GoodsInventory>()
        .iter(world)
        .all(|inventory| inventory.used_bulk() <= inventory.bulk_capacity()));
}

fn assert_arrival_stress_outcome(
    world: &mut World,
    evidence: &Evidence,
    expected_residents: usize,
) {
    let snapshot = milestone(world);
    let people = people(world);
    assert_eq!(
        snapshot.residents, expected_residents as u32,
        "late arrivals did not all settle: {people:#?}"
    );
    assert_eq!(
        snapshot.housed, expected_residents,
        "scarce founding Wood should finish houses sequentially instead of deadlocking partial sites"
    );
    let required_houses =
        expected_residents.div_ceil(SettlementBuildingKind::House.housing_capacity() as usize);
    assert!(
        snapshot.buildings[building_index(SettlementBuildingKind::House)] >= required_houses,
        "late-wave housing did not complete: {snapshot:#?}"
    );
    assert!(
        world
            .query::<&NavigationRouteFailed>()
            .iter(world)
            .next()
            .is_none(),
        "late-wave stress ended with a failed route: {people:#?}"
    );
    assert!(world
        .query::<&GoodsInventory>()
        .iter(world)
        .all(|inventory| inventory.used_bulk() <= inventory.bulk_capacity()));

    let unworked_farms: Vec<_> = world
        .query::<(Entity, &SettlementBuilding, &PlayerPosition)>()
        .iter(world)
        .filter(|(entity, building, _)| {
            building.kind == SettlementBuildingKind::Farmstead
                && !building.workers.is_empty()
                && !evidence.farmed_workplaces.contains(entity)
        })
        .map(|(_, building, position)| {
            (
                building.settlement.clone(),
                position.0,
                building.workers.clone(),
            )
        })
        .collect();
    assert!(
        unworked_farms.is_empty(),
        "staffed Farmsteads never produced observable field work: {unworked_farms:#?}\n{people:#?}"
    );
    let unworked_producers: Vec<_> = world
        .query::<&SettlementBuilding>()
        .iter(world)
        .filter(|building| {
            matches!(
                building.kind,
                SettlementBuildingKind::Farmstead
                    | SettlementBuildingKind::FishermansHut
                    | SettlementBuildingKind::LumberjackHut
            )
        })
        .flat_map(|building| {
            building
                .workers
                .iter()
                .filter(|worker| !evidence.productive_workers.contains(*worker))
                .map(|worker| (worker.clone(), building.kind))
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(
        unworked_producers.is_empty(),
        "employed producers never entered their physical work loop: {unworked_producers:#?}\n{people:#?}"
    );

    let roads: Vec<_> = world.query::<&VillageRoad>().iter(world).cloned().collect();
    let building_doors: Vec<_> = world
        .query::<(&SettlementBuilding, &PlayerPosition, &PlayerRotation)>()
        .iter(world)
        .map(|(building, position, rotation)| {
            (
                building.settlement.clone(),
                building.kind,
                building.kind.entrance_position(position.0, rotation.0),
            )
        })
        .collect();
    assert_eq!(
        roads.len(),
        building_doors.len(),
        "late construction left a building without its own connector"
    );
    assert!(roads.iter().all(VillageRoad::is_complete));
    for (settlement, kind, door) in building_doors {
        let door = Vec2::new(door.x, door.z);
        assert!(
            roads.iter().any(|road| {
                road.settlement == settlement
                    && road
                        .built_points()
                        .iter()
                        .any(|point| point.distance_squared(door) <= 2.1_f32.powi(2))
            }),
            "late-built {kind:?} door {door:?} never connected to a completed village path"
        );
    }
    for (settlement, administration) in world
        .query::<(&Settlement, &MootAdministration)>()
        .iter(world)
    {
        assert_eq!(
            (
                administration.roadless_buildings,
                administration.disconnected_buildings,
                administration.pending_road_buildings,
            ),
            (0, 0, 0),
            "{}'s final civic road audit was not clean",
            settlement.name
        );
    }
}

/// Long-running diagnostic entrypoint. Ignored in ordinary `cargo test`; use
/// `cargo village-lab`, optionally with `FISTWORLD_LAB_MINUTES`,
/// `FISTWORLD_LAB_WARP`, or `FISTWORLD_LAB_VERBOSE=1`.
#[test]
#[ignore = "run explicitly with `cargo village-lab`"]
fn village_simulation_lab() {
    // The terrain loader caches the active map once per process. The cargo
    // alias filters to this one test and runs one thread, so set it before the
    // first WorldTerrain is constructed.
    std::env::set_var("CITYSIM_MAP_ID", "village_lab");
    let warp = env_f32("FISTWORLD_LAB_WARP", DEFAULT_LAB_WARP).clamp(1.0, 1000.0);
    let minutes = env_f32("FISTWORLD_LAB_MINUTES", DEFAULT_LAB_MINUTES);
    let verbose = std::env::var("FISTWORLD_LAB_VERBOSE").is_ok_and(|value| value == "1");
    let scenario = LabScenario::from_environment();

    let mut app = App::new();
    configure_lab(&mut app);
    app.insert_resource(WorldTerrain::default());
    spawn_scenario(app.world_mut(), warp, scenario);
    let initial_money = total_money(app.world_mut());
    let arrival_count = if scenario.includes_secure() {
        lab_arrival_count()
    } else {
        0
    };
    let arrival_day = lab_arrival_day();
    let expected_residents = scenario.expected_residents() + arrival_count;
    let expected_money = initial_money
        .saturating_add(arrival_count as u64 * shared::economy::STARTING_VILLAGER_MONEY);

    let wall_step = std::time::Duration::from_secs_f32(1.0 / 60.0);
    let sim_step = warp / 60.0;
    let total_ticks = ((minutes * 60.0) / sim_step).ceil() as usize;
    let mut sim_seconds = 0.0;
    let mut next_report = 0.0;
    let mut evidence = Evidence::default();
    let mut last_structure: Option<StructureMilestone> = None;
    let mut person_progress: HashMap<Entity, (String, f32)> = HashMap::new();
    let mut life_ledger = LabLifeLedger::default();
    let mut arrivals_spawned = arrival_count == 0;

    for _ in 0..total_ticks {
        app.world_mut().resource_mut::<Time>().advance_by(wall_step);
        app.update();
        sim_seconds += sim_step;

        if !arrivals_spawned {
            let current_day = app
                .world_mut()
                .query::<&WorldTime>()
                .iter(app.world())
                .next()
                .map_or(0, |clock| clock.day);
            if current_day >= arrival_day - 1 {
                let hall_position = app
                    .world_mut()
                    .query::<(&Settlement, &PlayerPosition)>()
                    .iter(app.world())
                    .find(|(settlement, _)| settlement.name == "Lab Meadow")
                    .map(|(_, position)| position.0)
                    .expect("arrival stress requires Lab Meadow");
                spawn_lab_arrivals(app.world_mut(), hall_position, arrival_count);
                arrivals_spawned = true;
                println!(
                    "LAB arrival day={arrival_day} count={arrival_count} total_expected={expected_residents}"
                );
            }
        }

        let world = app.world_mut();
        update_evidence(world, &mut evidence);
        life_ledger.observe(world, sim_seconds, sim_step);
        let structure = milestone(world);
        if last_structure.as_ref() != Some(&structure) {
            println!("LAB event t={:.1}m {structure:?}", sim_seconds / 60.0);
            last_structure = Some(structure);
        }

        let current_people = people(world);
        for person in &current_people {
            let progress = person_progress
                .entry(person.entity)
                .or_insert_with(|| (person.progress_key.clone(), sim_seconds));
            if progress.0 != person.progress_key {
                *progress = (person.progress_key.clone(), sim_seconds);
            } else if person.active_progress_expected && sim_seconds - progress.1 > STALL_SECONDS {
                let remembered_route = world
                    .resource::<LastPlannedRoutes>()
                    .0
                    .get(&person.entity)
                    .cloned();
                let route = remembered_route.as_ref().map_or_else(
                    || "never planned".to_string(),
                    |route| {
                        format!(
                            "goal={:.1},{:.1} next={} waypoints={:?}",
                            route.goal.x, route.goal.z, route.next, route.waypoints
                        )
                    },
                );
                let route_handoff = world
                    .resource::<LastRouteHandoffs>()
                    .0
                    .get(&person.entity)
                    .cloned()
                    .unwrap_or_else(|| "no pre-step handoff recorded".to_string());
                let point = world
                    .get::<PlayerPosition>(person.entity)
                    .map(|position| Vec2::new(position.0.x, position.0.z))
                    .unwrap_or(Vec2::ZERO);
                let goal = world
                    .get::<MoveTarget>(person.entity)
                    .map(|target| Vec2::new(target.0.x, target.0.z));
                let nearby_buildings: Vec<_> = world
                    .query::<(
                        &shared::building::PlacedBuilding,
                        &shared::building::BuildingPosition,
                    )>()
                    .iter(world)
                    .filter_map(|(building, position)| {
                        let at = Vec2::new(position.0.x, position.0.z);
                        let kind = match building.building_type {
                            shared::building::BuildingType::LogCabin => {
                                SettlementBuildingKind::House
                            }
                            shared::building::BuildingType::LumberjackHut => {
                                SettlementBuildingKind::LumberjackHut
                            }
                            shared::building::BuildingType::Farmstead => {
                                SettlementBuildingKind::Farmstead
                            }
                            shared::building::BuildingType::FishermansHut => {
                                SettlementBuildingKind::FishermansHut
                            }
                            shared::building::BuildingType::TownHall => {
                                SettlementBuildingKind::Hall
                            }
                            shared::building::BuildingType::PlaceholderMarket => {
                                SettlementBuildingKind::Market
                            }
                            shared::building::BuildingType::PlaceholderTavern => {
                                SettlementBuildingKind::Tavern
                            }
                            shared::building::BuildingType::PlaceholderChurch => {
                                SettlementBuildingKind::Church
                            }
                        };
                        let door = kind.entrance_position(position.0, building.rotation);
                        (at.distance(point) <= 12.0).then_some((
                            building.building_type,
                            at,
                            building.rotation,
                            Vec2::new(door.x, door.z),
                            Vec2::new(door.x, door.z).distance(point),
                        ))
                    })
                    .collect();
                let grid = world.resource::<SpatialObstacleGrid>();
                let nearby: Vec<_> = grid
                    .get_nearby(point)
                    .map(|obstacle| {
                        (
                            obstacle.center,
                            obstacle.half_extents,
                            obstacle.rotation,
                            obstacle.contains_point(point),
                        )
                    })
                    .collect();
                let colliders = world.resource::<StaticColliders>();
                let derived = world.resource::<DerivedColliderLibrary>();
                let nearby_props: Vec<_> = colliders
                    .instances
                    .values()
                    .filter_map(|instance| {
                        let shape = derived.by_kind.get(&instance.kind)?;
                        let radius = shape.horizontal_radius * instance.scale
                            + crate::world::navgrid::VILLAGER_PROP_RADIUS;
                        let at = Vec2::new(instance.position.x, instance.position.z);
                        (at.distance(point) <= radius + 5.0
                            || goal.is_some_and(|goal| at.distance(goal) <= radius + 5.0))
                        .then_some((
                            instance.kind,
                            at,
                            radius,
                            at.distance(point),
                            goal.map(|goal| at.distance(goal)),
                        ))
                    })
                    .collect();
                let rejected_segment = remembered_route.as_ref().and_then(|route| {
                    let mut points = vec![point];
                    points.extend(
                        route
                            .waypoints
                            .iter()
                            .map(|waypoint| Vec2::new(waypoint.position.x, waypoint.position.z)),
                    );
                    points.windows(2).find_map(|segment| {
                        (!crate::player::hero::navigation_segment_clear(
                            segment[0],
                            segment[1],
                            Some(grid),
                            Some(colliders),
                            Some(derived),
                        ))
                        .then_some((segment[0], segment[1]))
                    })
                });
                panic!(
                    "LAB STALL: {} made no embodied progress for {:.0}s\n{}\nlast route: {route}\nlast handoff: {route_handoff}\nrejected segment={rejected_segment:?}\ncurrent point blocked={} nearby obstacles={nearby:?} nearby props={nearby_props:?} nearby buildings={nearby_buildings:?}\nall people: {current_people:#?}",
                    person.name,
                    sim_seconds - progress.1,
                    person.state,
                    grid.point_blocked(point),
                );
            }
        }

        if sim_seconds >= next_report {
            print_report(world, sim_seconds, verbose);
            next_report += REPORT_SECONDS;
        }
    }

    print_report(app.world_mut(), sim_seconds, true);
    life_ledger.print_final_report();
    assert!(
        arrivals_spawned,
        "requested arrival day was outside the lab run"
    );
    if arrival_count == 0 {
        assert_lab_outcome(app.world_mut(), &evidence, scenario, expected_residents);
    } else {
        assert_arrival_stress_outcome(app.world_mut(), &evidence, expected_residents);
    }
    assert_eq!(
        total_money(app.world_mut()),
        expected_money,
        "the complete village loop created or destroyed coin"
    );
    println!(
        "LAB PASS: {scenario:?} village loop remained live for {minutes:.1} simulated minutes"
    );
}
