//! Deterministic, accelerated village integration laboratory.
//!
//! This is intentionally an ignored test rather than another runtime mode. It
//! runs the real server systems without networking or rendering, prints a
//! compact timeline, and fails with per-villager diagnostics when embodied work
//! stops making progress. Launch it from the workspace root with:
//!
//! `cargo village-lab`

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Instant;

use bevy::prelude::*;
use shared::components::{
    BuildingDoorDemand, BuildingDoorUse, BuildingId, CharacterActivity, CharacterAffiliation,
    CharacterAttributes, CharacterKind, CharacterName, CivicStrategy, CivicTradeContract, Company,
    CompanyId, CompanyLeadership, CompanyOwnership, CompanyTradeRoute, EmployedAt, FarmField,
    FishingPier, Health, Household, MootAdministration, Nutrition, Occupation, OperatedBy, OwnedBy,
    PersonId, PlayerPosition, PlayerRotation, Residence, Settlement, SettlementBuilding,
    SettlementBuildingKind, SettlementId, SettlementOpportunityBoard, SettlementPolicies,
    SettlementTier, TimeWarp, TradeContractId, TradeRouteHistory, TradeRouteId, VillageRoad,
    WorkStatus, WorldTime, COMPANY_TOTAL_SHARES,
};
use shared::economy::{
    BusinessAccount, BusinessCondition, BusinessManagementPolicy, BusinessWagePolicy, CarriedLoad,
    CivicAccount, CompanyAccount, CompanyDecisionHistory, CompanyManagementPolicy, Good,
    GoodsInventory, HouseholdEconomy, MarketSeller, MootMarket, SettlementEconomy, Wallet,
    FOOD_SECURITY_TARGET_DAYS, VILLAGE_MIN_PROSPERITY, VILLAGE_REQUIRED_SECURE_DAYS,
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
    self, BuildStage, ConstructionMaterialRoutine, FarmerRoutine, FishingRoutine, HomeRoutine,
    InheritedBusinessCapital, LumberjackRoutine, MootQueueTicket, MootServiceKind,
    PermitPickupRoutine, ProcessingRoutine, PublishedTerrainDeltas, QuarryRoutine,
    SettlementEconomyRuntime, UnderConstruction, VillageClock, VillagerIntent, WorkerOffDuty,
    WorkplaceDoorTransit,
};
use crate::world::village_lab_scenario::{
    choose_greenwood_site, choose_inland_meadow_site, choose_policy_comparison_sites,
    choose_poor_site, choose_secure_site, choose_stone_site, lab_arrival_offset, lab_arrival_waves,
    LabArrivalTarget, LabScenario,
};
use crate::world::village_roads::{
    self, NavigationRouteFailed, NavigationRoutePending, PlannedRoadAccess, RoadBuilderRoutine,
    RoadConnectorFor, RoadRepairBacklog, RoadRequest, TravelRoute, VillageRoadGraph,
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
const ARRIVAL_STRESS_SETTLE_GRACE_DAYS: f32 = 2.0;
/// A connector normally needs one survey, a walk of at most a few minutes and
/// less than a second of work per point. After ten world minutes it must be
/// complete or have an explicit active/requested/audited owner; a single
/// physical steward may still have older repairs ahead of it in a stress town.
const STRESS_ROAD_COMPLETION_GRACE_SECONDS: f32 = 600.0;

fn lab_world_seconds(clock: &WorldTime) -> f32 {
    clock.day as f32 * clock.cycle_duration() + clock.seconds_in_cycle
}

#[derive(Default, Debug)]
struct Evidence {
    saw_chopping: bool,
    saw_building: bool,
    saw_indoors: bool,
    saw_farming: bool,
    saw_fishing: bool,
    saw_mining: bool,
    saw_sitting: bool,
    saw_wheat_carried: bool,
    saw_flour_present: bool,
    saw_bread_present: bool,
    saw_wood_carried: bool,
    saw_stone_carried: bool,
    saw_stone_present: bool,
    saw_food_carried: bool,
    saw_road_builder: bool,
    saw_partial_site: bool,
    saw_full_site: bool,
    saw_door_open: bool,
    saw_door_close_after_open: bool,
    saw_daily_consumption: bool,
    saw_market_consignment: bool,
    saw_customer_purchase: bool,
    saw_hunger: bool,
    saw_village_tier: bool,
    max_moot_queue_depth: usize,
    max_immigration_queue_depth: usize,
    peak_recent_food_production: f32,
    coldbarrow_saw_hunger: bool,
    meadow_saw_hunger: bool,
    farmed_workplaces: HashSet<Entity>,
    productive_workers: HashSet<String>,
    building_first_seen_seconds: HashMap<Entity, f32>,
    producer_staffed_since_seconds: HashMap<Entity, f32>,
    producer_worker_since_seconds: HashMap<(Entity, String), f32>,
}

#[derive(Resource, Default, Debug)]
struct LastPlannedRoutes(HashMap<Entity, TravelRoute>);

#[derive(Resource, Default)]
struct LabPhaseTimings {
    core_started: Option<Instant>,
    navigation_started: Option<Instant>,
    core_section_started: [Option<Instant>; 6],
    core_section_milliseconds: [Vec<f64>; 6],
    economy_section_started: [Option<Instant>; 4],
    economy_section_milliseconds: [Vec<f64>; 4],
    construction_section_started: [Option<Instant>; 6],
    construction_section_milliseconds: [Vec<f64>; 6],
    core_milliseconds: Vec<f64>,
    navigation_milliseconds: Vec<f64>,
    burst_core_milliseconds: Vec<f64>,
    burst_navigation_milliseconds: Vec<f64>,
    burst_ticks_remaining: usize,
}

fn begin_lab_core_section<const INDEX: usize>(mut timing: ResMut<LabPhaseTimings>) {
    timing.core_section_started[INDEX] = Some(Instant::now());
}

fn advance_lab_core_section<const FINISHED: usize, const NEXT: usize>(
    mut timing: ResMut<LabPhaseTimings>,
) {
    if let Some(started) = timing.core_section_started[FINISHED].take() {
        let elapsed = started.elapsed().as_secs_f64() * 1_000.0;
        timing.core_section_milliseconds[FINISHED].push(elapsed);
    }
    timing.core_section_started[NEXT] = Some(Instant::now());
}

fn end_lab_core_section<const INDEX: usize>(mut timing: ResMut<LabPhaseTimings>) {
    if let Some(started) = timing.core_section_started[INDEX].take() {
        let elapsed = started.elapsed().as_secs_f64() * 1_000.0;
        timing.core_section_milliseconds[INDEX].push(elapsed);
    }
}

fn begin_lab_economy_section<const INDEX: usize>(mut timing: ResMut<LabPhaseTimings>) {
    timing.economy_section_started[INDEX] = Some(Instant::now());
}

fn advance_lab_economy_section<const FINISHED: usize, const NEXT: usize>(
    mut timing: ResMut<LabPhaseTimings>,
) {
    if let Some(started) = timing.economy_section_started[FINISHED].take() {
        let elapsed = started.elapsed().as_secs_f64() * 1_000.0;
        timing.economy_section_milliseconds[FINISHED].push(elapsed);
    }
    timing.economy_section_started[NEXT] = Some(Instant::now());
}

fn end_lab_economy_section<const INDEX: usize>(mut timing: ResMut<LabPhaseTimings>) {
    if let Some(started) = timing.economy_section_started[INDEX].take() {
        let elapsed = started.elapsed().as_secs_f64() * 1_000.0;
        timing.economy_section_milliseconds[INDEX].push(elapsed);
    }
}

fn begin_lab_construction_section<const INDEX: usize>(mut timing: ResMut<LabPhaseTimings>) {
    timing.construction_section_started[INDEX] = Some(Instant::now());
}

fn advance_lab_construction_section<const FINISHED: usize, const NEXT: usize>(
    mut timing: ResMut<LabPhaseTimings>,
) {
    if let Some(started) = timing.construction_section_started[FINISHED].take() {
        let elapsed = started.elapsed().as_secs_f64() * 1_000.0;
        timing.construction_section_milliseconds[FINISHED].push(elapsed);
    }
    timing.construction_section_started[NEXT] = Some(Instant::now());
}

fn end_lab_construction_section<const INDEX: usize>(mut timing: ResMut<LabPhaseTimings>) {
    if let Some(started) = timing.construction_section_started[INDEX].take() {
        let elapsed = started.elapsed().as_secs_f64() * 1_000.0;
        timing.construction_section_milliseconds[INDEX].push(elapsed);
    }
}

fn begin_lab_core_timing(mut timing: ResMut<LabPhaseTimings>) {
    timing.core_started = Some(Instant::now());
}

fn end_lab_core_timing(mut timing: ResMut<LabPhaseTimings>) {
    let Some(started) = timing.core_started.take() else {
        return;
    };
    let elapsed = started.elapsed().as_secs_f64() * 1_000.0;
    timing.core_milliseconds.push(elapsed);
    if timing.burst_ticks_remaining > 0 {
        timing.burst_core_milliseconds.push(elapsed);
    }
}

fn begin_lab_navigation_timing(mut timing: ResMut<LabPhaseTimings>) {
    timing.navigation_started = Some(Instant::now());
}

fn end_lab_navigation_timing(mut timing: ResMut<LabPhaseTimings>) {
    let Some(started) = timing.navigation_started.take() else {
        return;
    };
    let elapsed = started.elapsed().as_secs_f64() * 1_000.0;
    timing.navigation_milliseconds.push(elapsed);
    if timing.burst_ticks_remaining > 0 {
        timing.burst_navigation_milliseconds.push(elapsed);
        timing.burst_ticks_remaining -= 1;
    }
}

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
    buildings: [usize; 12],
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
const STRESS_UNACCOUNTED_WORK_SECONDS: f32 = 180.0;

#[derive(Debug, Default)]
struct StressWorkerRecord {
    name: String,
    settlement: String,
    job: String,
    current_unaccounted: f32,
    longest_unaccounted: f32,
    total_unaccounted: f32,
}

/// Large-lab guard for the failure that is hardest to see in aggregate logs:
/// someone retains a real job but stands idle through their shift with no
/// route, work routine, queue, door or off-screen aggregate state explaining
/// it. Brief hand-offs are expected; the ledger keeps both the longest
/// uninterrupted spell and the current spell so a persistent state-machine
/// hole is distinct from ordinary downtime.
#[derive(Default)]
struct StressTaskLedger {
    workers: HashMap<Entity, StressWorkerRecord>,
}

impl StressTaskLedger {
    fn observe(&mut self, world: &mut World, elapsed_world_seconds: f32) {
        let on_shift = world
            .query::<&WorldTime>()
            .iter(world)
            .next()
            .is_some_and(|clock| {
                clock.seconds_in_cycle < clock.day_duration * village::WORKDAY_END_DAY_T
            });
        let settlement_names: HashMap<_, _> = world
            .query::<(Entity, &Settlement)>()
            .iter(world)
            .map(|(entity, settlement)| (entity, settlement.name.clone()))
            .collect();
        let people: Vec<_> = world
            .query::<(
                Entity,
                &CharacterName,
                &WorkStatus,
                &Occupation,
                Option<&shared::components::EmployedAt>,
                &CharacterActivity,
                &VillagerIntent,
            )>()
            .iter(world)
            .map(
                |(entity, name, status, occupation, employed_at, activity, intent)| {
                    (
                        entity,
                        name.0.clone(),
                        *status,
                        occupation.0.is_some(),
                        format!(
                            "{}@{}",
                            occupation.0.as_deref().unwrap_or("Unemployed"),
                            employed_at
                                .map(|employment| employment.0 .0.to_string())
                                .unwrap_or_else(|| "none".to_string()),
                        ),
                        *activity,
                        intent.settlement(),
                    )
                },
            )
            .collect();

        for (entity, name, status, has_occupation, job, activity, settlement) in people {
            let record = self.workers.entry(entity).or_default();
            record.name = name;
            record.job = job;
            record.settlement = settlement
                .and_then(|entity| settlement_names.get(&entity))
                .cloned()
                .unwrap_or_else(|| "Unaffiliated".to_string());

            let has_embodied_task = world.get::<MoveTarget>(entity).is_some()
                || world.get::<FarmerRoutine>(entity).is_some()
                || world.get::<FishingRoutine>(entity).is_some()
                || world.get::<LumberjackRoutine>(entity).is_some()
                || world.get::<RoadBuilderRoutine>(entity).is_some()
                || world.get::<ConstructionMaterialRoutine>(entity).is_some()
                || world
                    .get::<village::MarketCollectionRoutine>(entity)
                    .is_some()
                || world
                    .get::<village::HouseholdShoppingRoutine>(entity)
                    .is_some()
                || world.get::<MootQueueTicket>(entity).is_some()
                || world.get::<BuildingDoorUse>(entity).is_some()
                || world.get::<WorkplaceDoorTransit>(entity).is_some()
                || world.get::<HomeRoutine>(entity).is_some();
            let intentionally_abstract_or_off_duty = world
                .get::<shared::components::CivicEmployment>(entity)
                .is_some()
                || world.get::<WorkerOffDuty>(entity).is_some()
                || world
                    .get::<village::strategic::StrategicPerson>(entity)
                    .is_some();
            let visibly_working = matches!(
                activity,
                CharacterActivity::Building
                    | CharacterActivity::Chopping
                    | CharacterActivity::Farming
                    | CharacterActivity::Fishing
                    | CharacterActivity::Mining
                    | CharacterActivity::Indoors
            );
            let unexplained = on_shift
                && status == WorkStatus::Employed
                && has_occupation
                && !has_embodied_task
                && !intentionally_abstract_or_off_duty
                && !visibly_working;
            if unexplained {
                record.current_unaccounted += elapsed_world_seconds;
                record.total_unaccounted += elapsed_world_seconds;
                record.longest_unaccounted =
                    record.longest_unaccounted.max(record.current_unaccounted);
            } else {
                record.current_unaccounted = 0.0;
            }
        }
    }

    fn print_report(&self, world: &mut World) {
        let settlement_names: HashMap<_, _> = world
            .query::<(Entity, &Settlement)>()
            .iter(world)
            .map(|(entity, settlement)| (entity, settlement.name.clone()))
            .collect();
        let mut status_by_settlement = HashMap::<String, (usize, usize, usize, usize)>::new();
        for (status, intent) in world.query::<(&WorkStatus, &VillagerIntent)>().iter(world) {
            let settlement = intent
                .settlement()
                .and_then(|entity| settlement_names.get(&entity))
                .cloned()
                .unwrap_or_else(|| "Unaffiliated".to_string());
            let counts = status_by_settlement.entry(settlement).or_default();
            counts.3 += 1;
            match status {
                WorkStatus::Employed => counts.0 += 1,
                WorkStatus::LookingForWork => counts.1 += 1,
                WorkStatus::Chilling => counts.2 += 1,
            }
        }
        let mut status_rows: Vec<_> = status_by_settlement.into_iter().collect();
        status_rows.sort_by(|a, b| a.0.cmp(&b.0));
        for (settlement, (employed, looking, chilling, total)) in status_rows {
            println!(
                "LAB task health '{settlement}': total={total} employed={employed} looking={looking} chilling={chilling}"
            );
        }

        let mut worst: Vec<_> = self
            .workers
            .values()
            .filter(|record| record.longest_unaccounted >= STRESS_UNACCOUNTED_WORK_SECONDS)
            .collect();
        worst.sort_by(|a, b| b.longest_unaccounted.total_cmp(&a.longest_unaccounted));
        let currently_stuck = self
            .workers
            .values()
            .filter(|record| record.current_unaccounted >= STRESS_UNACCOUNTED_WORK_SECONDS)
            .count();
        println!(
            "LAB task health historical_unexplained_employed_idle={} currently_stuck={} threshold={:.0}s world",
            worst.len(),
            currently_stuck,
            STRESS_UNACCOUNTED_WORK_SECONDS,
        );
        for record in worst.into_iter().take(20) {
            println!(
                "  LAB task idle actor='{}' settlement='{}' job='{}' longest={:.1}m current={:.1}m total={:.1}m",
                record.name,
                record.settlement,
                record.job,
                record.longest_unaccounted / 60.0,
                record.current_unaccounted / 60.0,
                record.total_unaccounted / 60.0,
            );
        }
    }

    fn assert_no_currently_stuck_workers(&self) {
        let stuck: Vec<_> = self
            .workers
            .values()
            .filter(|record| record.current_unaccounted >= STRESS_UNACCOUNTED_WORK_SECONDS)
            .map(|record| {
                format!(
                    "{} in {} [{}] ({:.1} world minutes)",
                    record.name,
                    record.settlement,
                    record.job,
                    record.current_unaccounted / 60.0,
                )
            })
            .collect();
        assert!(
            stuck.is_empty(),
            "employed residents remained without work, travel, service or off-duty state: {}",
            stuck.join(", "),
        );
    }
}

/// Bounded, lab-only biography for one resident. This deliberately does not
/// become a production component: live worlds may contain thousands of people,
/// while the lab can afford detailed observation of its small cast.
#[derive(Debug)]
struct LabLifeRecord {
    person_id: Option<PersonId>,
    name: String,
    initial_money: u64,
    current_money: u64,
    money_in: u64,
    money_out: u64,
    initial_health: f32,
    current_health: f32,
    max_health: f32,
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
                Option<&PersonId>,
                &Wallet,
                Option<&CharacterAttributes>,
                Option<&Occupation>,
                Option<&WorkStatus>,
                Option<&Residence>,
                Option<&Nutrition>,
                &Health,
                &CharacterActivity,
            )>()
            .iter(world)
            .map(
                |(
                    entity,
                    name,
                    person_id,
                    wallet,
                    attributes,
                    occupation,
                    status,
                    residence,
                    nutrition,
                    health,
                    activity,
                )| {
                    (
                        entity,
                        name.0.clone(),
                        person_id.copied(),
                        wallet.balance(),
                        attributes.copied().unwrap_or_default(),
                        occupation.and_then(|occupation| occupation.0.clone()),
                        status.copied().unwrap_or_default(),
                        residence.map(|residence| residence.0.clone()),
                        nutrition.copied().unwrap_or_default(),
                        health.clone(),
                        *activity,
                    )
                },
            )
            .collect();

        let at = format!("t={:.1}m", sim_seconds / 60.0);
        for (
            entity,
            name,
            person_id,
            money,
            attributes,
            occupation,
            status,
            residence,
            nutrition,
            health,
            activity,
        ) in snapshots
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
                    person_id,
                    name: name.clone(),
                    initial_money: money,
                    current_money: money,
                    money_in: 0,
                    money_out: 0,
                    initial_health: health.current,
                    current_health: health.current,
                    max_health: health.max,
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
            if record.person_id.is_none() {
                record.person_id = person_id;
            }
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

            if (health.current - record.current_health).abs() > f32::EPSILON {
                record.push_event(format!(
                    "{at} health {:.0} -> {:.0}",
                    record.current_health, health.current,
                ));
                record.current_health = health.current;
                record.max_health = health.max;
            }

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

    fn print_final_report(&self, world: &mut World) {
        let death_records: Vec<_> = world
            .resource::<village::MortalityLedger>()
            .iter()
            .map(|death| (death.id, death.name.clone(), death.day, death.cause))
            .collect();
        let deaths: HashMap<PersonId, (u32, shared::components::DeathCause)> = death_records
            .iter()
            .map(|(id, _, day, cause)| (*id, (*day, *cause)))
            .collect();
        let total_deaths = world.resource::<village::MortalityLedger>().total_deaths;
        let starvation_deaths = deaths
            .values()
            .filter(|(_, cause)| *cause == shared::components::DeathCause::Starvation)
            .count();
        println!(
            "LAB mortality total={} starvation={} retained_records={}",
            total_deaths,
            starvation_deaths,
            deaths.len(),
        );
        let mut deaths_by_day = std::collections::BTreeMap::<u32, usize>::new();
        for (day, _) in deaths.values() {
            *deaths_by_day.entry(*day).or_default() += 1;
        }
        println!(
            "LAB mortality days=[{}]",
            deaths_by_day
                .into_iter()
                .map(|(day, count)| format!("{day}:{count}"))
                .collect::<Vec<_>>()
                .join(", "),
        );
        for (id, name, day, cause) in &death_records {
            println!(
                "LAB mortality person=#{} '{}' day={} cause={cause:?}",
                id.0, name, day,
            );
        }
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

        // Personal wallets alone make a cash-poor shareholder look destitute
        // even when they own a valuable firm. Keep the original liquid ranking
        // above, then add share-weighted company equity. Company cash remains
        // company property; this is a net-worth estimate, not spendable money.
        // Household purses remain shared and are deliberately not assigned to
        // one member.
        let mut operating_by_company = HashMap::<CompanyId, (i64, u64)>::new();
        for (company, account) in world.query::<(&OperatedBy, &BusinessAccount)>().iter(world) {
            let entry = operating_by_company.entry(company.0).or_default();
            entry.0 = entry.0.saturating_add(account.lifetime_profit());
            entry.1 = entry.1.saturating_add(account.owner_withdrawals);
        }
        let companies: Vec<_> = world
            .query::<(&CompanyId, &Company, &CompanyOwnership, &CompanyAccount)>()
            .iter(world)
            .map(|(id, company, ownership, account)| {
                (*id, company.clone(), ownership.clone(), *account)
            })
            .collect();
        let mut equity_by_person = HashMap::<PersonId, (Vec<String>, u64, i64, u64)>::new();
        for (company_id, company, ownership, account) in companies {
            let equity_value = account
                .cash
                .saturating_add(account.book_value)
                .saturating_sub(account.wage_arrears)
                .saturating_sub(account.tax_arrears);
            let (profit, withdrawals) = operating_by_company
                .get(&company_id)
                .copied()
                .unwrap_or_default();
            for share in ownership.shares() {
                let pro_rata = |value: u64| {
                    ((u128::from(value) * u128::from(share.shares))
                        / u128::from(COMPANY_TOTAL_SHARES)) as u64
                };
                let profit_share = if profit < 0 {
                    -(pro_rata(profit.unsigned_abs()) as i64)
                } else {
                    pro_rata(profit as u64) as i64
                };
                let entry = equity_by_person.entry(share.shareholder).or_default();
                entry.0.push(format!(
                    "{}#{}:{}sh",
                    company.name, company_id.0, share.shares
                ));
                entry.1 = entry.1.saturating_add(pro_rata(equity_value));
                entry.2 = entry.2.saturating_add(profit_share);
                entry.3 = entry.3.saturating_add(pro_rata(withdrawals));
            }
        }
        let mut controlled: Vec<_> = self
            .people
            .values()
            .map(|person| {
                let (firms, company_equity, profit, withdrawals) = person
                    .person_id
                    .and_then(|id| equity_by_person.get(&id))
                    .map_or_else(
                        || (String::new(), 0, 0, 0),
                        |(firms, equity, profit, withdrawals)| {
                            (firms.join("|"), *equity, *profit, *withdrawals)
                        },
                    );
                (
                    person.current_money.saturating_add(company_equity),
                    person,
                    company_equity,
                    profit,
                    withdrawals,
                    firms,
                )
            })
            .collect();
        controlled.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.name.cmp(&b.1.name)));
        for (side, ranked) in [
            ("poorest", controlled.iter().take(3).collect::<Vec<_>>()),
            (
                "richest",
                controlled.iter().rev().take(3).collect::<Vec<_>>(),
            ),
        ] {
            for (index, (net_worth, person, company_equity, profit, withdrawals, firms)) in
                ranked.into_iter().enumerate()
            {
                println!(
                    "LAB wealth detail side={} rank={} name='{}' net_worth={} wallet={} company_equity={} initial={} in={} out={} company_profit_share={}{} dividends={} job='{}' status='{}' residence='{}' home='{}' hungry={} holdings=[{}]",
                    side,
                    index + 1,
                    person.name,
                    shared::economy::format_money(*net_worth),
                    shared::economy::format_money(person.current_money),
                    shared::economy::format_money(*company_equity),
                    shared::economy::format_money(person.initial_money),
                    shared::economy::format_money(person.money_in),
                    shared::economy::format_money(person.money_out),
                    if *profit < 0 { "-" } else { "+" },
                    shared::economy::format_money(profit.unsigned_abs()),
                    shared::economy::format_money(*withdrawals),
                    person.current_job,
                    person.current_status,
                    person.current_residence,
                    person.current_home,
                    person.hungry,
                    firms,
                );
            }
        }

        let mut people: Vec<_> = self.people.values().collect();
        people.sort_by(|a, b| a.name.cmp(&b.name));
        for person in people {
            let death = person.person_id.and_then(|id| deaths.get(&id).copied());
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
                "LAB life {} life={} health={:.0}->{:.0}/{:.0} wallet={}->{} in={} out={} attrs=P{} I{} C{}->P{} I{} C{} residence='{}' home='{}' work='{}' status='{}' hungry={} activities=[{}]",
                person.name,
                death.map_or_else(
                    || "alive".to_string(),
                    |(day, cause)| format!("dead day {day} ({})", cause.label()),
                ),
                person.initial_health,
                if death.is_some() { 0.0 } else { person.current_health },
                person.max_health,
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
    app.init_resource::<village::ambient::AmbientDiagnostics>();
    app.init_resource::<PublishedTerrainDeltas>();
    app.init_resource::<VillageRoadGraph>();
    app.init_resource::<crate::world::identity::WorldIdAllocator>();
    app.init_resource::<crate::world::identity::WorldIdentityIndex>();
    app.init_resource::<crate::world::settlement_directory::SettlementDirectory>();
    app.init_resource::<village::history::SettlementHistoryRuntime>();
    app.init_resource::<LastPlannedRoutes>();
    app.init_resource::<LastRouteHandoffs>();
    app.init_resource::<LabPhaseTimings>();
    app.init_resource::<village::PermitPlanningDiagnostics>();
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
    app.add_systems(
        Update,
        (
            begin_lab_core_timing
                .after(crate::world::navgrid::sync_obstacle_grid)
                .before(village::schedule::VillageSimulationSet::Core),
            end_lab_core_timing
                .after(village::schedule::VillageSimulationSet::Core)
                .before(village::schedule::VillageSimulationSet::Navigation),
            begin_lab_navigation_timing
                .after(end_lab_core_timing)
                .before(village::schedule::VillageSimulationSet::Navigation),
            end_lab_navigation_timing.after(village::schedule::VillageSimulationSet::Navigation),
        ),
    );
    use village::schedule::VillageEconomySet;
    app.add_systems(
        Update,
        (
            begin_lab_economy_section::<0>
                .after(advance_lab_core_section::<1, 2>)
                .before(VillageEconomySet::MarketsBusinesses),
            advance_lab_economy_section::<0, 1>
                .after(VillageEconomySet::MarketsBusinesses)
                .before(VillageEconomySet::Households),
            advance_lab_economy_section::<1, 2>
                .after(VillageEconomySet::Households)
                .before(VillageEconomySet::SettlementAccounts),
            advance_lab_economy_section::<2, 3>
                .after(VillageEconomySet::SettlementAccounts)
                .before(VillageEconomySet::Permits),
            end_lab_economy_section::<3>
                .after(VillageEconomySet::Permits)
                .before(advance_lab_core_section::<2, 3>),
        ),
    );
    use village::schedule::VillageConstructionSet;
    app.add_systems(
        Update,
        (
            begin_lab_construction_section::<0>
                .after(advance_lab_core_section::<2, 3>)
                .before(VillageConstructionSet::MootServices),
            advance_lab_construction_section::<0, 1>
                .after(VillageConstructionSet::MootServices)
                .before(VillageConstructionSet::MaterialLogistics),
            advance_lab_construction_section::<1, 2>
                .after(VillageConstructionSet::MaterialLogistics)
                .before(VillageConstructionSet::BuildingProgress),
            advance_lab_construction_section::<2, 3>
                .after(VillageConstructionSet::BuildingProgress)
                .before(VillageConstructionSet::Fields),
            advance_lab_construction_section::<3, 4>
                .after(VillageConstructionSet::Fields)
                .before(VillageConstructionSet::RoadPlanning),
            advance_lab_construction_section::<4, 5>
                .after(VillageConstructionSet::RoadPlanning)
                .before(VillageConstructionSet::Employment),
            end_lab_construction_section::<5>
                .after(VillageConstructionSet::Employment)
                .before(advance_lab_core_section::<3, 4>),
        ),
    );
    use village::schedule::VillageCoreSet;
    app.add_systems(
        Update,
        (
            begin_lab_core_section::<0>
                .after(begin_lab_core_timing)
                .before(VillageCoreSet::IdentityPopulation),
            advance_lab_core_section::<0, 1>
                .after(VillageCoreSet::IdentityPopulation)
                .before(VillageCoreSet::Civic),
            advance_lab_core_section::<1, 2>
                .after(VillageCoreSet::Civic)
                .before(VillageCoreSet::EconomyPlanning),
            advance_lab_core_section::<2, 3>
                .after(VillageCoreSet::EconomyPlanning)
                .before(VillageCoreSet::Construction),
            advance_lab_core_section::<3, 4>
                .after(VillageCoreSet::Construction)
                .before(VillageCoreSet::Activity),
            advance_lab_core_section::<4, 5>
                .after(VillageCoreSet::Activity)
                .before(VillageCoreSet::Directory),
            end_lab_core_section::<5>
                .after(VillageCoreSet::Directory)
                .before(end_lab_core_timing),
        ),
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
    strategy: CivicStrategy,
    hall_position: Vec3,
    resident_count: usize,
    initial_tier: SettlementTier,
) {
    let hall_inventory = GoodsInventory::new_partitioned(shared::economy::capacity::HALL);
    let mut policies = shared::components::SettlementPolicies::poor_relief();
    policies.strategy = strategy;
    world.spawn((
        Settlement {
            name: name.to_string(),
            tier: initial_tier,
            residents: 0,
            treasury: shared::economy::STARTING_TREASURY_MONEY,
        },
        hall_inventory,
        shared::economy::MootMarket::founding(),
        policies,
        PlayerPosition(hall_position),
        PlayerRotation(0.0),
    ));

    let entrance = SettlementBuildingKind::Hall.entrance_position(hall_position, 0.0);
    let policy_pair = matches!(name, "Lab Frugal" | "Lab Mutual Aid");
    let seed_key = if policy_pair {
        "PolicyResident"
    } else {
        resident_prefix
    };
    let seed_base = seed_key.bytes().fold(0_u64, |hash, byte| {
        hash.wrapping_mul(109).wrapping_add(u64::from(byte))
    });
    let positions: Vec<_> = {
        let terrain = world.resource::<WorldTerrain>();
        (0..resident_count)
            .map(|index| {
                let requested = Vec3::new(
                    entrance.x,
                    terrain.get_height(entrance.x, entrance.z - 0.45),
                    entrance.z - 0.45,
                );
                crate::world::dev::safe_villager_spawn_position(
                    requested,
                    seed_base.wrapping_add(index as u64),
                    terrain,
                    None,
                    None,
                    None,
                )
                .unwrap_or_else(|| {
                    panic!("{name} resident {index} had no navigable spawn near the Moot entrance")
                })
            })
            .collect()
    };
    for (index, position) in positions.into_iter().enumerate() {
        let angle = index as f32 / resident_count as f32 * std::f32::consts::TAU;
        let mut resident = world.spawn((
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
        if policy_pair {
            resident.insert(CharacterAttributes::from_seed(
                0x504f_4c49_4359_u64.wrapping_add(index as u64),
            ));
        }
    }
}

fn spawn_lab_arrivals(
    world: &mut World,
    hall_position: Vec3,
    day: u32,
    count: usize,
    target: LabArrivalTarget,
) {
    let entrance = SettlementBuildingKind::Hall.entrance_position(hall_position, 0.0);
    let offset = lab_arrival_offset();
    let positions: Vec<_> = {
        let terrain = world.resource::<WorldTerrain>();
        let obstacles = world.get_resource::<SpatialObstacleGrid>();
        let colliders = world.get_resource::<StaticColliders>();
        let derived = world.get_resource::<DerivedColliderLibrary>();
        (0..count)
            .map(|index| {
                // Reproduce a god-mode burst at one click. The safe-spawn
                // helper supplies its deterministic compact scatter; laying
                // hundreds of arrivals in one 135-metre row made the lab test
                // several unrelated terrain patches instead of one crowd.
                let x = entrance.x + offset.x;
                let z = entrance.z + offset.y - 0.45;
                let requested = Vec3::new(x, terrain.get_height(x, z), z);
                let seed = (u64::from(day) << 32)
                    ^ target.cohort_seed_salt()
                    ^ u64::try_from(index).unwrap_or(u64::MAX);
                crate::world::dev::safe_villager_spawn_position(
                    requested, seed, terrain, obstacles, colliders, derived,
                )
                .unwrap_or_else(|| {
                    panic!(
                        "arrival {index} on day {day} had no navigable spawn point near {requested:?}"
                    )
                })
            })
            .collect()
    };
    for (index, position) in positions.into_iter().enumerate() {
        let mut arrival = world.spawn((
            CharacterName(format!("{}ArrivalD{day}_{index}", target.resident_prefix())),
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
        if matches!(
            target,
            LabArrivalTarget::FrugalMeadow | LabArrivalTarget::MutualAidMeadow
        ) {
            arrival.insert(CharacterAttributes::from_seed(
                (u64::from(day) << 32)
                    ^ 0x504f_4c49_4359_u64
                    ^ u64::try_from(index).unwrap_or(u64::MAX),
            ));
        }
    }
}

fn spawn_scenario(world: &mut World, warp: f32, scenario: LabScenario) {
    let (secure, inland_meadow, policy_comparison, poor, stonefield, greenwood) = {
        let terrain = world.resource::<WorldTerrain>();
        let secure = scenario
            .includes_secure()
            .then(|| choose_secure_site(terrain));
        let inland_meadow = scenario
            .includes_inland_meadow()
            .then(|| choose_inland_meadow_site(terrain));
        let policy_comparison = scenario
            .is_policy_comparison()
            .then(|| choose_policy_comparison_sites(terrain));
        let poor = scenario
            .includes_poor()
            .then(|| choose_poor_site(terrain, secure.map(|choice| choice.0)));
        let stonefield = scenario.includes_stonefield().then(|| {
            choose_stone_site(
                terrain,
                secure
                    .map(|choice| choice.0)
                    .or_else(|| inland_meadow.map(|choice| choice.0))
                    .expect("stone comparison includes a meadow control"),
            )
        });
        let greenwood = scenario.includes_greenwood().then(|| {
            let mut occupied = Vec::new();
            if let Some(choice) = secure {
                occupied.push(choice.0);
            }
            if let Some(choice) = poor {
                occupied.push(choice.0);
            }
            choose_greenwood_site(terrain, &occupied)
        });
        (
            secure,
            inland_meadow,
            policy_comparison,
            poor,
            stonefield,
            greenwood,
        )
    };
    let residents_per_village = scenario.residents_per_village();
    // The trade fixture isolates Village -> Town commerce. Its controls are
    // established Villages so an unrelated Hamlet food-security oscillation
    // cannot prevent the first Stone tender from ever existing.
    let initial_tier = if scenario.is_trade_comparison() {
        SettlementTier::Village
    } else {
        SettlementTier::Hamlet
    };

    println!(
        "LAB map=village_lab scenario={scenario:?} villages={} warp={}x",
        usize::from(secure.is_some())
            + usize::from(inland_meadow.is_some())
            + if policy_comparison.is_some() { 2 } else { 0 }
            + usize::from(poor.is_some())
            + usize::from(stonefield.is_some())
            + usize::from(greenwood.is_some()),
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
            CivicStrategy::Balanced,
            hall,
            residents_per_village,
            initial_tier,
        );
    }

    if let Some((hall, trees, farmland)) = inland_meadow {
        println!(
            "LAB inland-meadow='Lab Meadow' hall=({:.1},{:.1},{:.1}) farmland={:.0}% trees={} fishing=none",
            hall.x,
            hall.y,
            hall.z,
            farmland * 100.0,
            trees,
        );
        spawn_lab_village(
            world,
            "Lab Meadow",
            "MeadowResident",
            CivicStrategy::Balanced,
            hall,
            residents_per_village,
            initial_tier,
        );
    }

    if let Some((frugal, mutual)) = policy_comparison {
        println!(
            "LAB policy-comparison frugal='Lab Frugal' hall=({:.1},{:.1},{:.1}) farmland={:.0}% trees={} mutual='Lab Mutual Aid' hall=({:.1},{:.1},{:.1}) farmland={:.0}% trees={} fishing=none/none",
            frugal.0.x,
            frugal.0.y,
            frugal.0.z,
            frugal.2 * 100.0,
            frugal.1,
            mutual.0.x,
            mutual.0.y,
            mutual.0.z,
            mutual.2 * 100.0,
            mutual.1,
        );
        spawn_lab_village(
            world,
            "Lab Frugal",
            "FrugalResident",
            CivicStrategy::Frugal,
            frugal.0,
            residents_per_village,
            initial_tier,
        );
        spawn_lab_village(
            world,
            "Lab Mutual Aid",
            "MutualResident",
            CivicStrategy::MutualAid,
            mutual.0,
            residents_per_village,
            initial_tier,
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
            CivicStrategy::Balanced,
            hall,
            residents_per_village,
            initial_tier,
        );
    }

    if let Some((hall, stone, farmland, biome)) = stonefield {
        println!(
            "LAB stone='Lab Stonefield' hall=({:.1},{:.1},{:.1}) nearby_stone={:.0}% farmland={:.0}% biome={biome:?}",
            hall.x,
            hall.y,
            hall.z,
            stone * 100.0,
            farmland * 100.0,
        );
        spawn_lab_village(
            world,
            "Lab Stonefield",
            "StoneResident",
            CivicStrategy::Balanced,
            hall,
            residents_per_village,
            initial_tier,
        );
    }

    if let Some((hall, trees, farmland)) = greenwood {
        println!(
            "LAB inland='Lab Greenwood' hall=({:.1},{:.1},{:.1}) farmland={:.0}% trees={}",
            hall.x,
            hall.y,
            hall.z,
            farmland * 100.0,
            trees,
        );
        spawn_lab_village(
            world,
            "Lab Greenwood",
            "GreenResident",
            CivicStrategy::Balanced,
            hall,
            residents_per_village,
            initial_tier,
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
        SettlementBuildingKind::Windmill => 8,
        SettlementBuildingKind::Bakery => 9,
        SettlementBuildingKind::StorageHall => 10,
        SettlementBuildingKind::StoneQuarry => 11,
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

    let mut buildings = [0usize; 12];
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
    let daylight = world
        .query::<&WorldTime>()
        .iter(world)
        .next()
        .is_none_or(WorldTime::is_day);
    let failed_routes: HashMap<_, _> = world
        .query::<(Entity, &NavigationRouteFailed)>()
        .iter(world)
        .map(|(entity, failed)| (entity, *failed))
        .collect();
    let travel_routes: HashMap<_, _> = world
        .query::<(Entity, &TravelRoute)>()
        .iter(world)
        .map(|(entity, route)| {
            let next = route
                .waypoints
                .get(route.next)
                .map_or("done".to_string(), |waypoint| {
                    format!("{:.1},{:.1}", waypoint.position.x, waypoint.position.z)
                });
            (
                entity,
                format!(
                    "{}/{}@{}→{:.1},{:.1}",
                    route.next,
                    route.waypoints.len(),
                    next,
                    route.goal.x,
                    route.goal.z
                ),
            )
        })
        .collect();
    let door_bypasses: HashSet<_> = world
        .query_filtered::<Entity, With<BuildingDoorUse>>()
        .iter(world)
        .collect();
    let pier_bypasses: HashSet<_> = world
        .query_filtered::<Entity, With<village::PierTraversal>>()
        .iter(world)
        .collect();
    let moot_transits: HashSet<_> = world
        .query_filtered::<Entity, With<village::MootQueueTransit>>()
        .iter(world)
        .collect();
    let moot_tickets: HashSet<_> = world
        .query_filtered::<Entity, With<MootQueueTicket>>()
        .iter(world)
        .collect();
    let market_collections: HashMap<_, _> = world
        .query::<(Entity, &village::MarketCollectionRoutine)>()
        .iter(world)
        .map(|(entity, routine)| (entity, routine.clone()))
        .collect();
    let processing_states: HashMap<_, _> = world
        .query::<(Entity, &ProcessingRoutine)>()
        .iter(world)
        .map(|(entity, routine)| (entity, format!("{routine:?}")))
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
    let strategic_people: HashSet<_> = world
        .query_filtered::<Entity, With<village::strategic::StrategicPerson>>()
        .iter(world)
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
                let travel = travel_routes.get(&entity);
                let door_collision_bypass = door_bypasses.contains(&entity);
                let pier_collision_bypass = pier_bypasses.contains(&entity);
                let moot_transit = moot_transits.contains(&entity);
                let moot_ticket = moot_tickets.contains(&entity);
                let market_collection = market_collections.get(&entity);
                let processor = processing_states.get(&entity);
                let off_duty = off_duty_workers.get(&entity);
                let strategic = strategic_people.contains(&entity);
                let (work_status, occupation) = work_states
                    .get(&entity)
                    .map_or((None, None), |(status, occupation)| {
                        (*status, occupation.as_deref())
                    });
                let state = format!(
                    "{} {:?} {:?} pos={:.1},{:.1} target={} pending={} travel={} failed={} road={} farm={} wood={} fish={} process={} market={} home={} door={} off_duty={} strategic={} work={:?}/{:?} bypass=[door:{door_collision_bypass},pier:{pier_collision_bypass},moot:{moot_transit}/{moot_ticket}] supply={} carry={:?}:{}",
                    name.0,
                    intent,
                    activity,
                    position.0.x,
                    position.0.z,
                    target.map_or_else(|| "-".to_string(), |target| format!("{:.1},{:.1}", target.0.x, target.0.z)),
                    pending.map_or_else(|| "-".to_string(), |value| format!("{value:?}")),
                    travel.map_or("-", String::as_str),
                    failed.map_or_else(|| "-".to_string(), |value| format!("{value:?}")),
                    road.map_or_else(|| "-".to_string(), |value| format!("{value:?}")),
                    farmer.map_or_else(|| "-".to_string(), |value| format!("{value:?}")),
                    lumberjack.map_or_else(|| "-".to_string(), |value| format!("{value:?}")),
                    fisher.map_or_else(|| "-".to_string(), |value| format!("{value:?}")),
                    processor.map_or("-", String::as_str),
                    market_collection.map_or_else(|| "-".to_string(), |value| format!("{value:?}")),
                    home.map_or_else(|| "-".to_string(), |value| format!("{value:?}")),
                    door.map_or_else(|| "-".to_string(), |value| format!("{value:?}")),
                    off_duty.map_or("-", String::as_str),
                    strategic,
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
                    // Ordinary construction is deliberately suspended after
                    // dark. A supplier may retain its delivery phase while
                    // the queue handoff has already cleared MoveTarget; dawn
                    // will issue the worksite target again. Keep detecting
                    // that exact state as a stall in daylight, but do not
                    // mistake a legitimate night pause for a deadlock.
                    || (daylight
                        && construction
                            .is_some_and(|routine| !routine.is_waiting_for_materials()))
                    || (road.is_some() && home.is_none());
                // Retry counters and route objects are diagnostics, not
                // embodied progress. Using the full state string here let a
                // villager stand on one spot forever while attempts cycled
                // 0→1→2→failed→0, continuously resetting the stall clock.
                let movement_owned = target.is_some()
                    || pending.is_some()
                    || travel.is_some()
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
    let now = world
        .query::<&WorldTime>()
        .iter(world)
        .next()
        .map_or(0.0, lab_world_seconds);
    let buildings: Vec<_> = world
        .query::<(Entity, &SettlementBuilding)>()
        .iter(world)
        .map(|(entity, building)| (entity, building.kind, building.workers.clone()))
        .collect();
    for (entity, kind, workers) in buildings {
        evidence
            .building_first_seen_seconds
            .entry(entity)
            .or_insert(now);
        if matches!(
            kind,
            SettlementBuildingKind::Farmstead
                | SettlementBuildingKind::FishermansHut
                | SettlementBuildingKind::LumberjackHut
                | SettlementBuildingKind::Windmill
                | SettlementBuildingKind::Bakery
                | SettlementBuildingKind::StoneQuarry
        ) && !workers.is_empty()
        {
            evidence
                .producer_staffed_since_seconds
                .entry(entity)
                .or_insert(now);
            for worker in workers {
                evidence
                    .producer_worker_since_seconds
                    .entry((entity, worker))
                    .or_insert(now);
            }
        } else {
            evidence.producer_staffed_since_seconds.remove(&entity);
        }
    }
    let mut queue_depths: HashMap<Entity, usize> = HashMap::new();
    let mut immigration_depths: HashMap<Entity, usize> = HashMap::new();
    for ticket in world.query::<&MootQueueTicket>().iter(world) {
        *queue_depths.entry(ticket.hall).or_default() += 1;
        if ticket.kind == MootServiceKind::Immigration {
            *immigration_depths.entry(ticket.hall).or_default() += 1;
        }
    }
    evidence.max_moot_queue_depth = evidence
        .max_moot_queue_depth
        .max(queue_depths.values().copied().max().unwrap_or_default());
    evidence.max_immigration_queue_depth = evidence.max_immigration_queue_depth.max(
        immigration_depths
            .values()
            .copied()
            .max()
            .unwrap_or_default(),
    );
    for economy in world.query::<&SettlementEconomy>().iter(world) {
        evidence.peak_recent_food_production = evidence
            .peak_recent_food_production
            .max(economy.recent_food_production);
    }
    for activity in world.query::<&CharacterActivity>().iter(world) {
        evidence.saw_chopping |= *activity == CharacterActivity::Chopping;
        evidence.saw_building |= *activity == CharacterActivity::Building;
        evidence.saw_indoors |= *activity == CharacterActivity::Indoors;
        evidence.saw_farming |= *activity == CharacterActivity::Farming;
        evidence.saw_fishing |= *activity == CharacterActivity::Fishing;
        evidence.saw_mining |= *activity == CharacterActivity::Mining;
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
    for (name, activity, farmer, fisher, lumberjack, quarry, processor) in world
        .query::<(
            &CharacterName,
            &CharacterActivity,
            Option<&FarmerRoutine>,
            Option<&FishingRoutine>,
            Option<&LumberjackRoutine>,
            Option<&QuarryRoutine>,
            Option<&ProcessingRoutine>,
        )>()
        .iter(world)
    {
        let performed_job = (farmer.is_some() && *activity == CharacterActivity::Farming)
            || (fisher.is_some() && *activity == CharacterActivity::Fishing)
            || (lumberjack.is_some() && *activity == CharacterActivity::Chopping)
            || (quarry.is_some() && *activity == CharacterActivity::Mining)
            || (processor.is_some() && *activity == CharacterActivity::Indoors);
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
        evidence.saw_stone_carried |= load.good == Some(Good::Stone) && load.amount > 0;
        evidence.saw_food_carried |= load.good == Some(Good::Food) && load.amount > 0;
    }
    for inventory in world.query::<&GoodsInventory>().iter(world) {
        evidence.saw_flour_present |= inventory.amount(Good::Flour) > 0;
        evidence.saw_bread_present |= inventory.amount(Good::Bread) > 0;
        evidence.saw_stone_present |= inventory.amount(Good::Stone) > 0;
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
        evidence.saw_market_consignment |= Good::ALL
            .iter()
            .any(|good| market.pool(*good).units_bought > 0);
        evidence.saw_customer_purchase |= Good::ALL
            .iter()
            .any(|good| market.pool(*good).units_sold > 0);
    }
    evidence.saw_village_tier |= world
        .query::<&Settlement>()
        .iter(world)
        .any(|settlement| settlement.tier == SettlementTier::Village);
}

#[derive(Debug, Clone, Copy)]
struct MoneyBreakdown {
    wallets: u64,
    treasuries: u64,
    companies: u64,
    households: u64,
    construction_escrow: u64,
    trade_escrow: u64,
    clearing: u64,
}

impl MoneyBreakdown {
    fn total(self) -> u64 {
        self.wallets
            .saturating_add(self.treasuries)
            .saturating_add(self.companies)
            .saturating_add(self.households)
            .saturating_add(self.construction_escrow)
            .saturating_add(self.trade_escrow)
            .saturating_add(self.clearing)
    }
}

/// Every penny must be in exactly one authoritative wallet, household purse,
/// company treasury, civic treasury or unfinished-business escrow after the
/// tick's event queue settles. `unposted_company_capital` is included only for the
/// brief construction/upgrade seam before it is posted to a company.
fn money_breakdown(world: &mut World) -> MoneyBreakdown {
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
    let company_cash = world
        .query::<&CompanyAccount>()
        .iter(world)
        .map(|account| account.cash)
        .sum::<u64>();
    let unposted_site_cash = world
        .query::<&BusinessAccount>()
        .iter(world)
        .map(|account| account.unposted_company_capital)
        .sum::<u64>();
    let household_cash = world
        .query::<&HouseholdEconomy>()
        .iter(world)
        .map(|household| household.pennies)
        .sum::<u64>();
    let construction_escrow = world
        .query::<&InheritedBusinessCapital>()
        .iter(world)
        .map(|capital| capital.0)
        .sum::<u64>();
    let trade_escrow = world
        .query::<&shared::components::CivicTradeContract>()
        .iter(world)
        .map(|contract| contract.escrow_cash)
        .sum::<u64>();
    let clearing = world
        .get_resource::<village::BusinessEventQueue>()
        .map_or(0, |queue| queue.pending_sale_gross());
    MoneyBreakdown {
        wallets,
        treasuries,
        companies: company_cash.saturating_add(unposted_site_cash),
        households: household_cash,
        construction_escrow,
        trade_escrow,
        clearing,
    }
}

fn total_money(world: &mut World) -> u64 {
    money_breakdown(world).total()
}

/// Per-owner trace used only by the long economy soak. Capturing it outside
/// the timed server update keeps performance measurements honest while making
/// a one-frame conservation failure identify the exact accounts involved.
fn money_trace(world: &mut World) -> HashMap<String, u64> {
    let mut trace = HashMap::new();
    for (entity, name, kind, wallet) in world
        .query::<(Entity, &CharacterName, &CharacterKind, Option<&Wallet>)>()
        .iter(world)
    {
        trace.insert(
            format!("wallet:{entity:?}:{}", name.0),
            wallet.map_or_else(
                || {
                    if *kind == CharacterKind::Villager {
                        shared::economy::STARTING_VILLAGER_MONEY
                    } else {
                        0
                    }
                },
                |wallet| wallet.balance(),
            ),
        );
    }
    for (settlement_id, settlement) in world.query::<(&SettlementId, &Settlement)>().iter(world) {
        trace.insert(
            format!("treasury:{}:{}", settlement_id.0, settlement.name),
            settlement.treasury,
        );
    }
    for (company_id, company, account) in world
        .query::<(&CompanyId, &Company, &CompanyAccount)>()
        .iter(world)
    {
        trace.insert(
            format!("company:{}:{}", company_id.0, company.name),
            account.cash,
        );
    }
    for (building_id, account) in world
        .query::<(&BuildingId, &BusinessAccount)>()
        .iter(world)
        .filter(|(_, account)| account.unposted_company_capital > 0)
    {
        trace.insert(
            format!("unposted-site-capital:{}", building_id.0),
            account.unposted_company_capital,
        );
    }
    for (building_id, household) in world
        .query::<(&BuildingId, &HouseholdEconomy)>()
        .iter(world)
    {
        trace.insert(format!("household:{}", building_id.0), household.pennies);
    }
    for (entity, capital) in world
        .query::<(Entity, &InheritedBusinessCapital)>()
        .iter(world)
    {
        trace.insert(format!("escrow:{entity:?}"), capital.0);
    }
    for (entity, contract) in world
        .query::<(Entity, &shared::components::CivicTradeContract)>()
        .iter(world)
    {
        trace.insert(format!("trade-escrow:{entity:?}"), contract.escrow_cash);
    }
    let clearing = world
        .get_resource::<village::BusinessEventQueue>()
        .map_or(0, |queue| queue.pending_sale_gross());
    trace.insert("market-clearing".to_string(), clearing);
    trace
}

fn money_trace_total(trace: &HashMap<String, u64>) -> u64 {
    trace.values().copied().sum()
}

fn money_trace_changes(before: &HashMap<String, u64>, after: &HashMap<String, u64>) -> Vec<String> {
    let mut keys: HashSet<_> = before.keys().chain(after.keys()).cloned().collect();
    let mut keys: Vec<_> = keys.drain().collect();
    keys.sort();
    keys.into_iter()
        .filter_map(|key| {
            let old = before.get(&key).copied().unwrap_or(0);
            let new = after.get(&key).copied().unwrap_or(0);
            (old != new).then(|| {
                format!(
                    "{key}: {} -> {} ({:+})",
                    shared::economy::format_money(old),
                    shared::economy::format_money(new),
                    i128::from(new) - i128::from(old),
                )
            })
        })
        .collect()
}

/// Print firm accounts separately from personal wealth so a successful owner
/// is visible without pretending company working capital is pocket money.
fn print_business_report(world: &mut World) {
    let business_histories: HashMap<BuildingId, shared::economy::BusinessHistoryArchive> = world
        .resource::<village::history::SettlementHistoryRuntime>()
        .all_business_archives()
        .into_iter()
        .map(|history| (history.id, history))
        .collect();
    let people: HashMap<PersonId, (String, u64)> = world
        .query::<(&PersonId, &CharacterName, &Wallet)>()
        .iter(world)
        .map(|(id, name, wallet)| (*id, (name.0.clone(), wallet.balance())))
        .collect();
    let company_cash: HashMap<CompanyId, u64> = world
        .query::<(&CompanyId, &CompanyAccount)>()
        .iter(world)
        .map(|(id, account)| (*id, account.cash))
        .collect();
    let mut listed: HashMap<BuildingId, u32> = HashMap::new();
    for market in world.query::<&MootMarket>().iter(world) {
        for listing in market.listings() {
            let MarketSeller::Business(id) = listing.seller else {
                continue;
            };
            let units = listed.entry(id).or_default();
            *units = units.saturating_add(listing.units);
        }
    }
    let businesses: Vec<_> = world
        .query::<(
            &BuildingId,
            &SettlementBuilding,
            Option<&OwnedBy>,
            &OperatedBy,
            &BusinessAccount,
            Option<&BusinessManagementPolicy>,
            Option<&BusinessCondition>,
        )>()
        .iter(world)
        .map(
            |(id, building, owner, operated_by, account, management, condition)| {
                (
                    *id,
                    building.kind,
                    building.settlement.clone(),
                    owner.copied(),
                    operated_by.0,
                    *account,
                    management.copied(),
                    condition.copied(),
                )
            },
        )
        .collect();
    if businesses.is_empty() {
        println!("LAB businesses none");
        return;
    }

    let mut owners: HashMap<PersonId, (u32, HashSet<CompanyId>, i64, u64)> = HashMap::new();
    for (id, kind, settlement, owner, company, account, management, condition) in businesses {
        let owner_name = owner
            .and_then(|owner| people.get(&owner.0).map(|person| person.0.as_str()))
            .unwrap_or("public/unresolved");
        println!(
            "LAB business {}#{} '{}' owner='{}' state={} strategy={} company=#{} company_treasury={} revenue={} expenses={} lifetime_profit={}{} contributed={} capex={} book_value={} withdrawals={} wage_arrears={} tax_arrears={} wage_defaults={} tax_defaults={} listed={}",
            kind.label(),
            id.0,
            settlement,
            owner_name,
            condition.map_or("Unreviewed", |condition| condition.state.label()),
            management.map_or("Unmanaged", |management| management.strategy.label()),
            company.0,
            shared::economy::format_money(company_cash.get(&company).copied().unwrap_or(0)),
            shared::economy::format_money(account.gross_revenue),
            shared::economy::format_money(account.operating_expenses),
            if account.lifetime_profit() < 0 { "-" } else { "+" },
            shared::economy::format_money(account.lifetime_profit().unsigned_abs()),
            shared::economy::format_money(account.contributed_capital),
            shared::economy::format_money(account.capital_expenditures),
            shared::economy::format_money(account.book_value),
            shared::economy::format_money(account.owner_withdrawals),
            shared::economy::format_money(account.wage_arrears),
            shared::economy::format_money(account.tax_arrears),
            shared::economy::format_money(account.defaulted_wages),
            shared::economy::format_money(account.defaulted_taxes),
            listed.get(&id).copied().unwrap_or(0),
        );
        if let Some(history) = business_histories.get(&id) {
            let recent = history.days.iter().rev().take(7).collect::<Vec<_>>();
            for day in recent.into_iter().rev() {
                println!(
                    "  LAB business history #{} day={} observed={} state={} strategy={} company_treasury={} protected={} drawable={} revenue={} costs={} profit={}{} capex={} book_value={} ask={} wage={} made={} sold={} inputs={} draws={} wage_arrears={} tax_arrears={} listed={}",
                    id.0,
                    day.day,
                    day.observed,
                    day.state.label(),
                    day.strategy.label(),
                    shared::economy::format_money(day.cash),
                    shared::economy::format_money(day.protected_working_capital),
                    shared::economy::format_money(day.withdrawable_profit),
                    shared::economy::format_money(day.gross_revenue),
                    shared::economy::format_money(
                        day.wage_expense
                            .saturating_add(day.input_expense)
                            .saturating_add(day.market_fees)
                            .saturating_add(day.delivery_fees)
                            .saturating_add(day.profit_taxes)
                    ),
                    if day.profit < 0 { "-" } else { "+" },
                    shared::economy::format_money(day.profit.unsigned_abs()),
                    shared::economy::format_money(day.capital_expenditures),
                    shared::economy::format_money(day.book_value),
                    shared::economy::format_money(day.asking_unit_price),
                    shared::economy::format_money(day.daily_wage),
                    day.produced_units,
                    day.sold_units,
                    day.purchased_input_units,
                    shared::economy::format_money(day.owner_withdrawals),
                    shared::economy::format_money(day.wage_arrears),
                    shared::economy::format_money(day.tax_arrears),
                    day.listed_output_units,
                );
            }
        }
        if let Some(owner) = owner {
            let total = owners.entry(owner.0).or_default();
            total.0 = total.0.saturating_add(1);
            total.1.insert(company);
            total.2 = total.2.saturating_add(account.lifetime_profit());
            total.3 = total.3.saturating_add(account.owner_withdrawals);
        }
    }
    let mut moguls: Vec<_> = owners
        .into_iter()
        .map(|(owner, (firm_count, companies, profit, withdrawals))| {
            let (name, wallet) = people
                .get(&owner)
                .cloned()
                .unwrap_or_else(|| (format!("Person#{}", owner.0), 0));
            let company_cash = companies
                .iter()
                .map(|company| company_cash.get(company).copied().unwrap_or(0))
                .fold(0u64, u64::saturating_add);
            (
                wallet.saturating_add(company_cash),
                name,
                wallet,
                firm_count,
                company_cash,
                profit,
                withdrawals,
            )
        })
        .collect();
    moguls.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    if let Some((controlled_wealth, name, wallet, firm_count, company_cash, profit, withdrawals)) =
        moguls.first()
    {
        println!(
            "LAB mogul leader='{}' firms={} controlled_wealth={} wallet={} company_cash={} lifetime_profit={}{} withdrawals={}",
            name,
            firm_count,
            shared::economy::format_money(*controlled_wealth),
            shared::economy::format_money(*wallet),
            shared::economy::format_money(*company_cash),
            if *profit < 0 { "-" } else { "+" },
            shared::economy::format_money(profit.unsigned_abs()),
            shared::economy::format_money(*withdrawals),
        );
    }
}

/// Print the legal company above its individual operating sites. This makes
/// the common one-shop craft firm visible while also exposing genuine pooled
/// expansion, vertical integration, share ownership and executive decisions.
fn print_company_report(world: &mut World) {
    let people: HashMap<PersonId, (String, Option<BuildingId>)> = world
        .query::<(&PersonId, &CharacterName, Option<&EmployedAt>)>()
        .iter(world)
        .map(|(id, name, workplace)| (*id, (name.0.clone(), workplace.map(|work| work.0))))
        .collect();
    let sites: Vec<_> = world
        .query::<(
            &BuildingId,
            &SettlementBuilding,
            &OperatedBy,
            &BusinessAccount,
            Option<&BusinessCondition>,
        )>()
        .iter(world)
        .map(|(id, building, company, account, condition)| {
            (
                company.0,
                *id,
                building.kind,
                building.settlement.clone(),
                *account,
                condition.copied(),
            )
        })
        .collect();
    let mut companies: Vec<_> = world
        .query::<(
            &CompanyId,
            &Company,
            &CompanyLeadership,
            &CompanyOwnership,
            &CompanyAccount,
            &CompanyManagementPolicy,
            &CompanyDecisionHistory,
        )>()
        .iter(world)
        .map(
            |(id, company, leadership, ownership, account, management, decisions)| {
                (
                    *id,
                    company.clone(),
                    *leadership,
                    ownership.clone(),
                    *account,
                    *management,
                    decisions.clone(),
                )
            },
        )
        .collect();
    companies.sort_by_key(|company| company.0);

    if companies.is_empty() {
        println!("LAB companies none");
        return;
    }

    let mut one_site_craft_firms = 0usize;
    let mut multi_site_firms = 0usize;
    let mut integrated_firms = 0usize;
    let mut empty_firms = 0usize;
    let mut strategy_counts = [0usize; 5];
    let mut decision_count = 0usize;
    for (id, company, leadership, ownership, account, management, decisions) in &companies {
        let mut company_sites: Vec<_> = sites.iter().filter(|site| site.0 == *id).collect();
        company_sites.sort_by_key(|site| site.1);
        let site_ids: HashSet<BuildingId> = company_sites.iter().map(|site| site.1).collect();
        let kinds: HashSet<SettlementBuildingKind> =
            company_sites.iter().map(|site| site.2).collect();
        let vertically_integrated = (kinds.contains(&SettlementBuildingKind::Farmstead)
            && kinds.contains(&SettlementBuildingKind::Windmill))
            || (kinds.contains(&SettlementBuildingKind::Windmill)
                && kinds.contains(&SettlementBuildingKind::Bakery));
        if company_sites.len() > 1 {
            multi_site_firms += 1;
        }
        if company_sites.is_empty() {
            empty_firms += 1;
        }
        if vertically_integrated {
            integrated_firms += 1;
        }
        let strategy_index = match management.strategy {
            shared::economy::BusinessStrategy::Balanced => 0,
            shared::economy::BusinessStrategy::Cautious => 1,
            shared::economy::BusinessStrategy::Growth => 2,
            shared::economy::BusinessStrategy::HighMargin => 3,
            shared::economy::BusinessStrategy::Opportunistic => 4,
        };
        strategy_counts[strategy_index] += 1;
        decision_count += decisions.entries().len();
        let sole_owner = ownership
            .shares()
            .first()
            .filter(|_| ownership.shares().len() == 1)
            .map(|share| share.shareholder);
        let master_is_worker = people
            .get(&leadership.master)
            .and_then(|person| person.1)
            .is_some_and(|workplace| site_ids.contains(&workplace));
        let ordinary_craft_firm =
            company_sites.len() == 1 && sole_owner == Some(leadership.master) && master_is_worker;
        if ordinary_craft_firm {
            one_site_craft_firms += 1;
        }
        let master_name = people.get(&leadership.master).map_or_else(
            || format!("Person#{}", leadership.master.0),
            |person| person.0.clone(),
        );
        let shareholders = ownership
            .shares()
            .iter()
            .map(|share| {
                let name = people.get(&share.shareholder).map_or_else(
                    || format!("Person#{}", share.shareholder.0),
                    |person| person.0.clone(),
                );
                format!("{name}={}sh", share.shares)
            })
            .collect::<Vec<_>>()
            .join(", ");
        let site_rows = company_sites
            .iter()
            .map(
                |(_, building_id, kind, settlement, site_account, condition)| {
                    format!(
                        "{}#{}@{}:{} profit={}{} liabilities={}",
                        kind.label(),
                        building_id.0,
                        settlement,
                        condition.map_or("Unreviewed", |condition| condition.state.label()),
                        if site_account.lifetime_profit() < 0 {
                            "-"
                        } else {
                            "+"
                        },
                        shared::economy::format_money(
                            site_account.lifetime_profit().unsigned_abs()
                        ),
                        shared::economy::format_money(
                            site_account
                                .wage_arrears
                                .saturating_add(site_account.tax_arrears)
                        ),
                    )
                },
            )
            .collect::<Vec<_>>()
            .join(" | ");
        let lifetime_profit = company_sites.iter().fold(0_i64, |total, site| {
            total.saturating_add(site.4.lifetime_profit())
        });
        println!(
            "LAB company #{} '{}' founded={} master='{}' master_is_worker={} ordinary_craft_firm={} integrated={} strategy={} autopilot={} sites={} shareholders=[{}] cash={} liabilities={} contributed={} capex={} book_value={} lifetime_profit={}{} dividends={} today_profit={}{} sites=[{}]",
            id.0,
            company.name,
            company.founded_day,
            master_name,
            master_is_worker,
            ordinary_craft_firm,
            vertically_integrated,
            management.strategy.label(),
            management.autopilot,
            company_sites.len(),
            shareholders,
            shared::economy::format_money(account.cash),
            shared::economy::format_money(
                account.wage_arrears.saturating_add(account.tax_arrears)
            ),
            shared::economy::format_money(account.contributed_capital),
            shared::economy::format_money(account.capital_expenditures),
            shared::economy::format_money(account.book_value),
            if lifetime_profit < 0 { "-" } else { "+" },
            shared::economy::format_money(lifetime_profit.unsigned_abs()),
            shared::economy::format_money(account.owner_withdrawals),
            if account.current_day.profit() < 0 { "-" } else { "+" },
            shared::economy::format_money(account.current_day.profit().unsigned_abs()),
            site_rows,
        );
        for decision in decisions.entries() {
            println!(
                "  LAB company decision #{} day={} master={} {} -> {} reason='{}'",
                id.0,
                decision.day,
                decision.master.0,
                decision.from.label(),
                decision.to.label(),
                decision.reason.label(),
            );
        }
    }
    println!(
        "LAB companies summary total={} ordinary_one_site_owner_master_workers={} multi_site={} vertically_integrated={} empty_living_holding={} strategies=[balanced:{}, cautious:{}, growth:{}, high_margin:{}, opportunistic:{}] decisions={}",
        companies.len(),
        one_site_craft_firms,
        multi_site_firms,
        integrated_firms,
        empty_firms,
        strategy_counts[0],
        strategy_counts[1],
        strategy_counts[2],
        strategy_counts[3],
        strategy_counts[4],
        decision_count,
    );
}

/// Print municipal cash flow and policy beside firm accounts. This keeps a
/// long lab run able to explain a dry treasury rather than merely reporting it.
fn print_civic_report(world: &mut World) {
    let halls: Vec<_> = world
        .query::<(
            Entity,
            &SettlementId,
            &Settlement,
            Option<&MootAdministration>,
            Option<&SettlementPolicies>,
            Option<&CivicAccount>,
        )>()
        .iter(world)
        .map(
            |(entity, id, settlement, administration, policies, account)| {
                (
                    entity,
                    *id,
                    settlement.clone(),
                    administration.cloned().unwrap_or_default(),
                    policies.copied().unwrap_or_default(),
                    account.copied().unwrap_or_default(),
                )
            },
        )
        .collect();
    for (entity, id, settlement, administration, policy, account) in halls {
        let current = account.current_day;
        println!(
            "LAB civic '{}' strategy={} autopilot={} treasury={} wage_arrears={} fee={:.1}% profit_levy={:.1}% relief={} food_target={}d payroll_target={}d staffing={} permit_subsidy={:.1}% current_day={} income={} spending={} lifetime_income={} lifetime_spending={} last_change={} ({})",
            settlement.name,
            policy.strategy.label(),
            policy.autopilot,
            shared::economy::format_money(settlement.treasury),
            shared::economy::format_money(administration.wage_arrears),
            policy.market_fee_bps as f32 / 100.0,
            policy.business_profit_tax_bps as f32 / 100.0,
            policy.poor_relief.label(),
            policy.food_reserve_target_days,
            policy.civic_payroll_reserve_days,
            policy.staffing_posture.label(),
            policy.business_permit_subsidy_bps as f32 / 100.0,
            current.day,
            shared::economy::format_money(current.income()),
            shared::economy::format_money(current.spending()),
            shared::economy::format_money(account.lifetime_income),
            shared::economy::format_money(account.lifetime_spending),
            policy.last_adjustment.label(),
            policy.last_reason.label(),
        );
        for entry in &administration.payroll {
            println!(
                "  LAB civic payroll person=#{} '{}' role={} active={} daily_wage={} arrears={}",
                entry.person_id.0,
                entry.name,
                entry.role.label(),
                entry.active,
                shared::economy::format_money(entry.daily_wage),
                shared::economy::format_money(entry.arrears),
            );
        }
        let archive = world
            .resource::<village::history::SettlementHistoryRuntime>()
            .archive(entity, id, &settlement.name);
        for day in archive.days.iter().rev().take(7).rev() {
            let income = day
                .civic
                .permit_income
                .saturating_add(day.civic.market_fee_income)
                .saturating_add(day.civic.profit_tax_income)
                .saturating_add(day.civic.public_sale_income);
            let spending = day
                .civic
                .wage_expense
                .saturating_add(day.civic.poor_relief_expense)
                .saturating_add(day.civic.material_expense)
                .saturating_add(day.civic.freight_expense);
            println!(
                "  LAB civic history day={} observed={} treasury={} income={} [permit={} fee={} levy={} sale={}] spending={} [wage={} relief={} materials={} freight={}] positions={}/+{} staffing={} rates={:.1}%/{:.1}% subsidy={:.1}% relief={} targets=food{}d/payroll{}d change={} ({})",
                day.day,
                day.civic.observed,
                shared::economy::format_money(day.civic_treasury),
                shared::economy::format_money(income),
                shared::economy::format_money(day.civic.permit_income),
                shared::economy::format_money(day.civic.market_fee_income),
                shared::economy::format_money(day.civic.profit_tax_income),
                shared::economy::format_money(day.civic.public_sale_income),
                shared::economy::format_money(spending),
                shared::economy::format_money(day.civic.wage_expense),
                shared::economy::format_money(day.civic.poor_relief_expense),
                shared::economy::format_money(day.civic.material_expense),
                shared::economy::format_money(day.civic.freight_expense),
                day.civic.filled_positions,
                day.civic.vacant_positions,
                day.civic.staffing_posture.label(),
                day.civic.market_fee_bps as f32 / 100.0,
                day.civic.business_profit_tax_bps as f32 / 100.0,
                day.civic.business_permit_subsidy_bps as f32 / 100.0,
                day.civic.poor_relief.label(),
                day.civic.food_reserve_target_days,
                day.civic.civic_payroll_reserve_days,
                day.civic.adjustment.label(),
                day.civic.reason.label(),
            );
        }
    }
}

fn print_trade_report(world: &mut World) {
    let settlements: HashMap<_, _> = world
        .query::<(&SettlementId, &Settlement)>()
        .iter(world)
        .map(|(id, settlement)| (*id, settlement.name.clone()))
        .collect();
    let people: HashMap<_, _> = world
        .query::<(&PersonId, &CharacterName)>()
        .iter(world)
        .map(|(id, name)| (*id, name.0.clone()))
        .collect();
    let contracts: Vec<_> = world
        .query::<(&TradeContractId, &CivicTradeContract)>()
        .iter(world)
        .map(|(id, contract)| (*id, *contract))
        .collect();
    let routes: Vec<_> = world
        .query::<(&TradeRouteId, &CompanyTradeRoute, &TradeRouteHistory)>()
        .iter(world)
        .map(|(id, route, history)| (*id, *route, history.clone()))
        .collect();

    println!(
        "LAB inter-settlement trade contracts={} routes={}",
        contracts.len(),
        routes.len()
    );
    for (id, contract) in contracts {
        println!(
            "  LAB contract #{} {} {} -> {} status={} delivered={}/{} max_price={} reserved={} escrow={} goods_spend={} freight_spend={}",
            id.0,
            contract.good.label(),
            contract
                .origin
                .and_then(|origin| settlements.get(&origin))
                .map_or("unknown", String::as_str),
            settlements
                .get(&contract.destination)
                .map_or("unknown", String::as_str),
            contract.status.label(),
            contract.delivered_units,
            contract.requested_units,
            shared::economy::format_money(contract.maximum_unit_price),
            shared::economy::format_money(contract.reserved_cash),
            shared::economy::format_money(contract.escrow_cash),
            shared::economy::format_money(contract.spent_on_goods),
            shared::economy::format_money(contract.spent_on_freight),
        );
    }
    for (id, route, history) in routes {
        println!(
            "  LAB route #{} company=#{} warehouse=#{} {} {} -> {} status={} porter={} trips={} units={} freight={}",
            id.0,
            route.company.0,
            route.warehouse.0,
            route.good.label(),
            settlements
                .get(&route.origin)
                .map_or("unknown", String::as_str),
            settlements
                .get(&route.destination)
                .map_or("unknown", String::as_str),
            route.status.label(),
            route.assigned_caravaner.map_or_else(
                || "none".to_string(),
                |person| people
                    .get(&person)
                    .cloned()
                    .unwrap_or_else(|| format!("Person#{}", person.0)),
            ),
            route.completed_trips,
            route.lifetime_units,
            shared::economy::format_money(route.lifetime_delivery_revenue),
        );
        for trip in history.trips().iter().rev().take(5).rev() {
            println!(
                "    LAB route trip departed={} completed={} units={} source_cost={} source_fees={} freight={} travel={:.1}m",
                trip.departed_day,
                trip.completed_day,
                trip.units,
                shared::economy::format_money(trip.source_purchase_cost),
                shared::economy::format_money(trip.source_market_fees),
                shared::economy::format_money(trip.delivery_revenue),
                trip.travel_world_seconds as f32 / 60.0,
            );
        }
    }
}

fn print_structure_report(world: &mut World) {
    let roads: Vec<_> = world.query::<&VillageRoad>().iter(world).cloned().collect();
    let connector_roads: HashMap<Entity, Vec<(Entity, String, u16, usize)>> = world
        .query::<(Entity, &VillageRoad, &RoadConnectorFor)>()
        .iter(world)
        .fold(
            HashMap::new(),
            |mut by_building, (entity, road, connector)| {
                by_building.entry(connector.building).or_default().push((
                    entity,
                    road.builder.clone(),
                    road.built_through,
                    road.points.len(),
                ));
                by_building
            },
        );
    let road_workers: HashMap<Entity, String> = world
        .query::<(
            Entity,
            &CharacterName,
            &RoadBuilderRoutine,
            Option<&HomeRoutine>,
            Option<&village::MarketCollectionRoutine>,
        )>()
        .iter(world)
        .map(|(entity, name, routine, home, market)| {
            (
                routine.road,
                format!(
                    "{}({entity:?}) routine={routine:?} home={} market={}",
                    name.0,
                    home.is_some(),
                    market.is_some(),
                ),
            )
        })
        .collect();
    let person_states: HashMap<_, _> = people(world)
        .into_iter()
        .map(|person| (person.entity, person.state))
        .collect();
    let active_sites: Vec<_> = world
        .query::<(
            Entity,
            &UnderConstruction,
            &GoodsInventory,
            Option<&PlannedRoadAccess>,
        )>()
        .iter(world)
        .map(|(entity, site, inventory, access)| {
            (
                entity,
                site.kind,
                site.position,
                site.stage,
                inventory.amount(Good::Wood),
                site.kind.construction_wood_required(),
                site.builder,
                access.map(|access| access.points.clone()),
            )
        })
        .collect();
    for (entity, kind, position, stage, delivered, required, builder, access) in active_sites {
        let builder_state = builder
            .and_then(|builder| person_states.get(&builder))
            .map(String::as_str)
            .unwrap_or("missing builder");
        let permit_queue = builder.is_some_and(|builder| {
            world.get::<MootQueueTicket>(builder).is_some()
                && world.get::<PermitPickupRoutine>(builder).is_some()
        });
        println!(
            "LAB worksite entity={entity:?} kind={} at={:.1},{:.1} stage={stage:?} wood={delivered}/{required} builder={builder:?} permit_queue={permit_queue} access={access:?} state=[{builder_state}]",
            kind.label(),
            position.x,
            position.z,
        );
    }
    let buildings: Vec<_> = world
        .query::<(
            Entity,
            &BuildingId,
            &SettlementBuilding,
            &PlayerPosition,
            &PlayerRotation,
            Option<&crate::world::village_roads::RoadRequest>,
            Option<&crate::world::village_roads::RoadSurveyBackoff>,
            Option<&RoadRepairBacklog>,
            Option<&PlannedRoadAccess>,
        )>()
        .iter(world)
        .map(
            |(
                entity,
                id,
                building,
                position,
                rotation,
                request,
                backoff,
                repair_backlog,
                planned_access,
            )| {
                (
                    entity,
                    *id,
                    building.clone(),
                    position.0,
                    rotation.0,
                    request.copied(),
                    backoff.copied(),
                    repair_backlog.copied(),
                    planned_access.cloned(),
                )
            },
        )
        .collect();
    for (
        entity,
        id,
        building,
        position,
        rotation,
        request,
        backoff,
        repair_backlog,
        planned_access,
    ) in buildings
    {
        let door = building.kind.entrance_position(position, rotation);
        let door2 = Vec2::new(door.x, door.z);
        let connected = roads.iter().any(|road| {
            road.settlement == building.settlement
                && road.is_complete()
                && road
                    .built_points()
                    .iter()
                    .any(|point| point.distance_squared(door2) <= 2.1_f32.powi(2))
        });
        println!(
            "LAB building #{} entity={:?} '{}' kind={} at={:.1},{:.1} owner='{}' workers={} road={} connectors={:?} connector_workers={:?} request={:?} backoff={:?} repair_backlog={:?} access={:?}",
            id.0,
            entity,
            building.settlement,
            building.kind.label(),
            position.x,
            position.z,
            building.owner.as_deref().unwrap_or("public"),
            building.workers.len(),
            if connected { "connected" } else { "missing" },
            connector_roads.get(&entity),
            connector_roads.get(&entity).map(|connectors| connectors
                .iter()
                .filter_map(|(road, ..)| road_workers.get(road))
                .cloned()
                .collect::<Vec<_>>()),
            request,
            backoff,
            repair_backlog,
            planned_access.as_ref().map(|access| &access.points),
        );
    }
}

fn goods_totals(world: &mut World) -> (u32, u32, u32, u32, u32) {
    world.query::<&GoodsInventory>().iter(world).fold(
        (0, 0, 0, 0, 0),
        |(wood, wheat, flour, bread, fish), inventory| {
            (
                wood + inventory.amount(Good::Wood),
                wheat + inventory.amount(Good::Wheat),
                flour + inventory.amount(Good::Flour),
                bread + inventory.amount(Good::Bread),
                fish + inventory.amount(Good::Food),
            )
        },
    )
}

fn print_report(world: &mut World, sim_seconds: f32, verbose: bool) {
    let snapshot = milestone(world);
    let (wood, wheat, flour, bread, fish) = goods_totals(world);
    let company_cash: HashMap<CompanyId, u64> = world
        .query::<(&CompanyId, &CompanyAccount)>()
        .iter(world)
        .map(|(id, account)| (*id, account.cash))
        .collect();
    let mut circulation = HashMap::<String, (u32, u64, HashSet<CompanyId>)>::new();
    for (building, inventory, account, operated_by) in world
        .query::<(
            &SettlementBuilding,
            &GoodsInventory,
            Option<&BusinessAccount>,
            Option<&OperatedBy>,
        )>()
        .iter(world)
    {
        let Some(account) = account else {
            continue;
        };
        let entry = circulation.entry(building.settlement.clone()).or_default();
        entry.0 = entry.0.saturating_add(inventory.edible_amount());
        entry.1 = entry
            .1
            .saturating_add(account.wage_arrears)
            .saturating_add(account.tax_arrears);
        if let Some(operated_by) = operated_by {
            entry.2.insert(operated_by.0);
        }
    }
    let market_food: HashMap<String, (u32, [u64; 3])> = world
        .query::<(&Settlement, &MootMarket)>()
        .iter(world)
        .map(|(settlement, market)| {
            (
                settlement.name.clone(),
                (
                    market.listed_edible_units(),
                    [
                        market.pool(Good::Food).ask,
                        market.pool(Good::Flour).ask,
                        market.pool(Good::Bread).ask,
                    ],
                ),
            )
        })
        .collect();
    let mut economy_rows: Vec<_> = world
        .query::<(
            &Settlement,
            &SettlementEconomy,
            Option<&SettlementOpportunityBoard>,
        )>()
        .iter(world)
        .map(|(settlement, economy, opportunities)| {
            let (business_food, business_arrears, company_cash) = circulation
                .get(&settlement.name)
                .map(|(food, arrears, companies)| {
                    let cash = companies
                        .iter()
                        .map(|company| company_cash.get(company).copied().unwrap_or(0))
                        .fold(0u64, u64::saturating_add);
                    (*food, *arrears, cash)
                })
                .unwrap_or_default();
            let permits = opportunities.map_or_else(
                || "none".to_string(),
                |board| {
                    board
                        .opportunities
                        .iter()
                        .take(3)
                        .map(|opportunity| {
                            format!(
                                "{}:{}{}",
                                opportunity.kind.label(),
                                opportunity.score,
                                if opportunity.subsidized { "*" } else { "" },
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(",")
                },
            );
            let (purchasable, asks) = market_food
                .get(&settlement.name)
                .copied()
                .unwrap_or((0, [0; 3]));
            format!(
                "{}:{} pop={} stock={} purchasable={} asks=[fish:{} flour:{} bread:{}] at_businesses={} reserve={:.1}d prod={:.1}/d eaten={:.1}/d hungry={} prosperity={:.0} secure={}d jobs=[private:{}/{} vacant:{} civic:{}/{} vacant:{} seeking:{} best:{}] company_cash={} business_arrears={} permits=[{}]",
                settlement.name,
                settlement.tier.label(),
                settlement.residents,
                economy.edible_stock,
                purchasable,
                shared::economy::format_money(asks[0]),
                shared::economy::format_money(asks[1]),
                shared::economy::format_money(asks[2]),
                business_food,
                economy.reserve_days,
                economy.recent_food_production,
                economy.recent_food_consumption,
                economy.unmet_food,
                economy.prosperity,
                economy.food_secure_days,
                economy.private_filled_jobs,
                economy.private_job_positions,
                economy.private_vacant_jobs,
                economy.civic_filled_jobs,
                economy.civic_job_positions,
                economy.civic_vacant_jobs,
                economy.job_seekers,
                shared::economy::format_money(economy.best_open_private_wage),
                shared::economy::format_money(company_cash),
                shared::economy::format_money(business_arrears),
                permits,
            )
        })
        .collect();
    economy_rows.sort();
    let complete_roads = snapshot
        .roads
        .iter()
        .filter(|(built, total)| usize::from(*built) >= *total)
        .count();
    let money = money_breakdown(world);
    println!(
        "LAB t={:>5.1}m residents={} sites={:?} buildings=[farm:{} mill:{} bakery:{} lumber:{} fisher:{} house:{}] roads={}/{} fields={} piers={} housed={} goods=[wood:{} wheat:{} flour:{} bread:{} fish:{}] money={} [wallets={} households={} companies={} treasury={} construction_escrow={} trade_escrow={} clearing={}]",
        sim_seconds / 60.0,
        snapshot.residents,
        snapshot.sites,
        snapshot.buildings[1],
        snapshot.buildings[8],
        snapshot.buildings[9],
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
        flour,
        bread,
        fish,
        shared::economy::format_money(money.total()),
        shared::economy::format_money(money.wallets),
        shared::economy::format_money(money.households),
        shared::economy::format_money(money.companies),
        shared::economy::format_money(money.treasuries),
        shared::economy::format_money(money.construction_escrow),
        shared::economy::format_money(money.trade_escrow),
        shared::economy::format_money(money.clearing),
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
    let (wood, _wheat, _flour, _bread, _fish) = goods_totals(world);

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
            count("Lab Meadow", SettlementBuildingKind::Windmill) >= 1,
            "the secure meadow never built the mill needed to make Flour: {built:#?}"
        );
        assert!(
            count("Lab Meadow", SettlementBuildingKind::Bakery) >= 1,
            "the eight-person Hamlet never added its Bread business: {built:#?}"
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
    if scenario.includes_stonefield() {
        assert!(
            count("Lab Stonefield", SettlementBuildingKind::StoneQuarry) >= 1,
            "the Stone-rich settlement never approved a Quarry: {built:#?}"
        );
        assert!(
            evidence.saw_mining,
            "a completed Stone Quarry never performed embodied quarry work"
        );
        assert!(
            evidence.saw_stone_carried && evidence.saw_stone_present,
            "Stone was not physically carried and deposited before civic procurement"
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
        evidence.max_moot_queue_depth >= 2,
        "concurrent permits never formed a visible Moot line: {evidence:#?}"
    );
    assert!(
        evidence.saw_building,
        "road/build animation was never observable"
    );
    assert!(evidence.saw_farming, "field work never occurred");
    if scenario.includes_secure() {
        assert!(
            fisher_count > 0,
            "the coastal scenario never completed a Fisherman's Hut: {built:#?}"
        );
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
            evidence.saw_market_consignment,
            "the Moot Steward never consigned physical producer output"
        );
        assert!(
            evidence.saw_customer_purchase,
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
        assert!(
            evidence.saw_flour_present,
            "the secure village never milled raw Wheat into Flour"
        );
        assert!(
            evidence.saw_bread_present,
            "the secure village never baked its Flour into Bread"
        );
    }
    assert!(world
        .query::<&GoodsInventory>()
        .iter(world)
        .all(|inventory| inventory.used_bulk() <= inventory.bulk_capacity()));

    // A permit line and physical timber gathering are both legitimate ways
    // for a supplying plot to wait. Losing both is not: that leaves a builder
    // with Building intent and a site that can never advance. Assert the
    // ownership seam explicitly so aggregate "sites=N" output cannot conceal
    // a state-machine hole.
    let abandoned_supply_sites: Vec<_> = world
        .query::<(Entity, &UnderConstruction, &GoodsInventory)>()
        .iter(world)
        .filter_map(|(site_entity, site, inventory)| {
            if site.stage != BuildStage::Supplying
                || inventory.amount(Good::Wood) >= site.kind.construction_wood_required()
            {
                return None;
            }
            let builder = site.builder?;
            let gathering = world.get::<ConstructionMaterialRoutine>(builder).is_some();
            let collecting_permit = world.get::<MootQueueTicket>(builder).is_some()
                && world.get::<PermitPickupRoutine>(builder).is_some();
            (!gathering && !collecting_permit).then_some((site_entity, builder, site.kind))
        })
        .collect();
    assert!(
        abandoned_supply_sites.is_empty(),
        "under-supplied worksites lost both their permit pickup and material routine: {abandoned_supply_sites:?}"
    );
}

fn assert_trade_comparison_outcome(world: &mut World, expected_residents: usize) {
    let snapshot = milestone(world);
    let total_deaths = world.resource::<village::MortalityLedger>().total_deaths;
    assert_eq!(
        u64::from(snapshot.residents).saturating_add(total_deaths),
        expected_residents as u64,
        "the deterministic trade cohort was not fully accounted for"
    );
    let settlements: HashMap<_, _> = world
        .query::<(&SettlementId, &Settlement)>()
        .iter(world)
        .map(|(id, settlement)| (settlement.name.clone(), (*id, settlement.tier)))
        .collect();
    let _meadow = settlements
        .get("Lab Meadow")
        .copied()
        .expect("trade fixture lost Lab Meadow");
    let stonefield = settlements
        .get("Lab Stonefield")
        .copied()
        .expect("trade fixture lost Lab Stonefield");

    let buildings: Vec<_> = world
        .query::<(
            &BuildingId,
            &shared::components::BuildingOf,
            &SettlementBuilding,
        )>()
        .iter(world)
        .map(|(id, owner, building)| (*id, owner.0, building.kind))
        .collect();
    assert!(
        buildings.iter().any(|(_, owner, kind)| {
            *owner == stonefield.0 && *kind == SettlementBuildingKind::StoneQuarry
        }),
        "Stonefield never established its source Quarry: {buildings:?}"
    );
    let contracts: Vec<_> = world
        .query::<&CivicTradeContract>()
        .iter(world)
        .copied()
        .collect();
    let fulfilled = contracts.iter().find(|contract| {
        contract
            .origin
            .is_some_and(|origin| origin != contract.destination)
            && contract.good == Good::Stone
            && contract.status == shared::components::TradeContractStatus::Fulfilled
            && contract.delivered_units >= contract.requested_units
            && contract.spent_on_goods > 0
            && contract.spent_on_freight > 0
            && contract.escrow_cash == 0
    });
    let fulfilled = fulfilled.unwrap_or_else(|| {
        panic!("no Town Works completed a paid remote Stone contract: {contracts:#?}")
    });
    let origin = fulfilled
        .origin
        .expect("fulfilled remote contract lost origin");
    let destination_tier = settlements
        .values()
        .find_map(|(id, tier)| (*id == fulfilled.destination).then_some(*tier));
    assert!(
        destination_tier
            .is_some_and(|tier| matches!(tier, SettlementTier::Town | SettlementTier::City)),
        "the Stone buyer did not complete its physical Town Works: {settlements:?}"
    );
    assert!(
        buildings.iter().any(|(_, owner, kind)| {
            *owner == origin && *kind == SettlementBuildingKind::StorageHall
        }),
        "the exporting settlement never established a Storage Hall: {buildings:?}"
    );

    let routes: Vec<_> = world
        .query::<(&CompanyTradeRoute, &TradeRouteHistory)>()
        .iter(world)
        .map(|(route, history)| (*route, history.clone()))
        .collect();
    assert!(
        routes.iter().any(|(route, history)| {
            route.origin == origin
                && route.destination == fulfilled.destination
                && route.good == Good::Stone
                && route.completed_trips > 0
                && route.lifetime_units > 0
                && route.lifetime_delivery_revenue > 0
                && !history.trips().is_empty()
        }),
        "no reusable company route retained the physical trip and freight history: {routes:#?}"
    );
}

fn assert_crowd_stress_outcome(
    world: &mut World,
    evidence: &Evidence,
    scenario: LabScenario,
    expected_residents: usize,
) {
    let mut settlements: Vec<_> = world
        .query::<(Entity, &Settlement)>()
        .iter(world)
        .map(|(entity, settlement)| (entity, settlement.name.clone(), settlement.residents))
        .collect();
    settlements.sort_by(|a, b| a.1.cmp(&b.1));
    let expected_settlements = if scenario.is_triple_stress() { 3 } else { 1 };
    assert_eq!(
        settlements.len(),
        expected_settlements,
        "the crowd stress fixture lost or duplicated a settlement: {settlements:?}"
    );
    assert_eq!(
        settlements
            .iter()
            .map(|(_, _, residents)| *residents)
            .sum::<u32>(),
        expected_residents as u32,
        "not every stress villager completed immigration: {settlements:?}"
    );
    let expected_per_settlement = if scenario.is_triple_stress() {
        crate::world::village_lab_scenario::TRIPLE_STRESS_VILLAGERS_PER_VILLAGE
    } else {
        crate::world::village_lab_scenario::DENSE_STRESS_VILLAGERS
    } as u32;
    assert!(
        settlements
            .iter()
            .all(|(_, _, residents)| *residents == expected_per_settlement),
        "the founding crowd did not remain with its intended settlement: {settlements:?}"
    );
    assert!(
        evidence.max_immigration_queue_depth >= 100,
        "the stress crowds never exercised a genuinely long visible immigration line"
    );

    let buildings: Vec<_> = world
        .query::<&SettlementBuilding>()
        .iter(world)
        .map(|building| (building.settlement.clone(), building.kind))
        .collect();
    for (settlement_entity, name, residents) in &settlements {
        let houses = buildings
            .iter()
            .filter(|(settlement, kind)| {
                settlement == name && *kind == SettlementBuildingKind::House
            })
            .count();
        let producers = buildings
            .iter()
            .filter(|(settlement, kind)| {
                settlement == name
                    && matches!(
                        kind,
                        SettlementBuildingKind::Farmstead
                            | SettlementBuildingKind::LumberjackHut
                            | SettlementBuildingKind::FishermansHut
                            | SettlementBuildingKind::Windmill
                            | SettlementBuildingKind::Bakery
                    )
            })
            .count();
        assert!(houses > 0, "{name} never converted demand into a cabin");
        assert!(
            producers > 0,
            "{name} never converted demand into a productive workplace"
        );

        let approved_lumber_huts = buildings
            .iter()
            .filter(|(settlement, kind)| {
                settlement == name && *kind == SettlementBuildingKind::LumberjackHut
            })
            .count()
            + world
                .query::<&UnderConstruction>()
                .iter(world)
                .filter(|site| {
                    site.settlement == *settlement_entity
                        && site.kind == SettlementBuildingKind::LumberjackHut
                })
                .count();
        let anticipated_capacity = (*residents).max(1).div_ceil(50) as usize;
        let generous_market_overshoot = anticipated_capacity.saturating_mul(2).saturating_add(2);
        assert!(
            approved_lumber_huts <= generous_market_overshoot,
            "{name} approved {approved_lumber_huts} lumber businesses for {residents} residents; finite construction demand must anticipate pending supply (generous ceiling {generous_market_overshoot})"
        );
    }
    assert!(
        evidence.saw_building,
        "no embodied construction was observed"
    );
    assert!(
        evidence.saw_chopping,
        "no embodied timber work was observed"
    );
    assert!(evidence.saw_farming, "no embodied field work was observed");
    assert!(
        world
            .query::<&NavigationRouteFailed>()
            .iter(world)
            .next()
            .is_none(),
        "the 600-person run ended with unresolved route failures"
    );
    assert!(world
        .query::<&GoodsInventory>()
        .iter(world)
        .all(|inventory| inventory.used_bulk() <= inventory.bulk_capacity()));

    let now = world
        .query::<&WorldTime>()
        .iter(world)
        .next()
        .map_or(0.0, lab_world_seconds);
    let connector_states: HashMap<Entity, Vec<(Entity, bool)>> = world
        .query::<(Entity, &VillageRoad, &RoadConnectorFor)>()
        .iter(world)
        .fold(
            HashMap::new(),
            |mut states, (road_entity, road, connector)| {
                states
                    .entry(connector.building)
                    .or_default()
                    .push((road_entity, road.is_complete()));
                states
            },
        );
    let active_road_builders: HashSet<Entity> = world
        .query::<&RoadBuilderRoutine>()
        .iter(world)
        .map(|routine| routine.road)
        .collect();
    for (building_entity, building) in world.query::<(Entity, &SettlementBuilding)>().iter(world) {
        let mature = evidence
            .building_first_seen_seconds
            .get(&building_entity)
            .is_some_and(|built_at| now - built_at >= STRESS_ROAD_COMPLETION_GRACE_SECONDS);
        let connector = connector_states.get(&building_entity);
        let complete = connector.is_some_and(|roads| roads.iter().any(|(_, complete)| *complete));
        if mature {
            assert!(
                complete
                    || connector.is_some()
                    || world.get::<RoadRequest>(building_entity).is_some()
                    || world.get::<RoadRepairBacklog>(building_entity).is_some(),
                "mature {:?} {:?} remained outside every completed, active, requested, or audited road lifecycle for at least {:.0} world seconds",
                building.kind,
                building_entity,
                STRESS_ROAD_COMPLETION_GRACE_SECONDS,
            );
        } else {
            assert!(
                complete
                    || connector.is_some()
                    || world.get::<PlannedRoadAccess>(building_entity).is_some()
                    || world.get::<RoadRequest>(building_entity).is_some(),
                "recent {:?} {:?} lost its connector and protected access claim",
                building.kind,
                building_entity,
            );
        }
    }
    for (building, _) in world.query::<(Entity, &RoadRepairBacklog)>().iter(world) {
        assert!(
            connector_states.get(&building).is_none(),
            "audited road backlog {:?} also owns an active connector",
            building,
        );
        assert!(
            world.get::<RoadRequest>(building).is_none(),
            "audited road backlog {:?} also owns an assigned request",
            building,
        );
    }
    for roads in connector_states.values() {
        for (road, complete) in roads {
            if !complete {
                assert!(
                    active_road_builders.contains(road),
                    "unfinished connector {road:?} has no live RoadBuilderRoutine"
                );
            }
        }
    }

    let mut requests_by_builder = HashMap::<Entity, Vec<Entity>>::new();
    for (building, request) in world.query::<(Entity, &RoadRequest)>().iter(world) {
        requests_by_builder
            .entry(request.builder)
            .or_default()
            .push(building);
    }
    let duplicate_requests: Vec<_> = requests_by_builder
        .iter()
        .filter(|(_, buildings)| buildings.len() > 1)
        .map(|(builder, buildings)| (*builder, buildings.clone()))
        .collect();
    assert!(
        duplicate_requests.is_empty(),
        "one road worker owned several queued connectors: {duplicate_requests:?}"
    );
    let request_and_routine: Vec<_> = requests_by_builder
        .keys()
        .filter(|builder| world.get::<RoadBuilderRoutine>(**builder).is_some())
        .copied()
        .collect();
    assert!(
        request_and_routine.is_empty(),
        "road workers simultaneously owned an active road and another request: {request_and_routine:?}"
    );
}

fn assert_arrival_stress_outcome(
    world: &mut World,
    evidence: &Evidence,
    expected_residents: usize,
) {
    let snapshot = milestone(world);
    let people = people(world);
    let clock = world
        .query::<&WorldTime>()
        .iter(world)
        .next()
        .expect("arrival stress requires the world clock");
    let now = lab_world_seconds(clock);
    let settle_grace = clock.cycle_duration() * ARRIVAL_STRESS_SETTLE_GRACE_DAYS;
    let construction_people: Vec<_> = people
        .iter()
        .filter(|person| !person.state.contains("supply=-"))
        .map(|person| person.state.as_str())
        .collect();
    let farms = snapshot.buildings[building_index(SettlementBuildingKind::Farmstead)];
    let mills = snapshot.buildings[building_index(SettlementBuildingKind::Windmill)];
    let fishers = snapshot.buildings[building_index(SettlementBuildingKind::FishermansHut)];
    let food_businesses = fishers + farms.min(mills.saturating_mul(2));
    assert_eq!(
        snapshot.residents, expected_residents as u32,
        "late arrivals did not all settle: {people:#?}"
    );
    assert!(
        evidence.max_immigration_queue_depth >= 2,
        "late arrivals never formed the visible Moot immigration line: {evidence:#?}"
    );
    // A modest wave must be absorbed almost completely. A triple-digit shock
    // is intentionally larger than the founding food, timber and construction
    // economy can erase during a short performance run. Critical settlements
    // now approve Farmsteads before cabins and, after genuinely exhausting the
    // farm envelope, allow only food-backed housing. Test that actual rule
    // rather than demanding free shelter for people the town cannot feed.
    let minimum_housed = if expected_residents < 100 {
        expected_residents.saturating_mul(4).div_ceil(5)
    } else {
        let house_capacity = SettlementBuildingKind::House.housing_capacity() as usize;
        let structural_food_capacity = food_businesses.saturating_mul(4);
        let measured_food_capacity = (evidence.peak_recent_food_production.ceil() as usize)
            .div_ceil(house_capacity)
            .saturating_mul(house_capacity);
        expected_residents
            .div_ceil(2)
            .min(structural_food_capacity.max(measured_food_capacity))
    };
    assert!(
        snapshot.housed >= minimum_housed,
        "the population shock did not produce its scale-appropriate minimum of {minimum_housed} housed residents; snapshot={snapshot:#?}; active builders={construction_people:#?}"
    );
    let required_houses = snapshot
        .housed
        .div_ceil(SettlementBuildingKind::House.housing_capacity() as usize);
    assert!(
        snapshot.buildings[building_index(SettlementBuildingKind::House)] >= required_houses,
        "completed housing capacity disagreed with its physical cabins: {snapshot:#?}"
    );
    let road_requests: Vec<_> = world.query::<&RoadRequest>().iter(world).copied().collect();
    let stale_building_intents: Vec<_> = world
        .query::<(
            Entity,
            &CharacterName,
            &VillagerIntent,
            Option<&ConstructionMaterialRoutine>,
            Option<&RoadBuilderRoutine>,
        )>()
        .iter(world)
        .filter_map(|(entity, name, intent, construction, road)| {
            let VillagerIntent::Building { site, .. } = intent else {
                return None;
            };
            let completed_connector_pending = road_requests
                .iter()
                .any(|request| request.builder == entity && request.completed_site == *site);
            (world.get_entity(*site).is_err()
                && construction.is_none()
                && road.is_none()
                && !completed_connector_pending)
                .then_some((name.0.clone(), *site))
        })
        .collect();
    assert!(
        stale_building_intents.is_empty(),
        "residents retained Building intent for despawned worksites: {stale_building_intents:#?}"
    );
    let (residents, recent_production, recent_consumption) = {
        let (settlement, economy) = world
            .query::<(&Settlement, &SettlementEconomy)>()
            .iter(world)
            .find(|(settlement, _)| settlement.name == "Lab Meadow")
            .expect("arrival stress requires Lab Meadow's economy");
        (
            settlement.residents,
            economy.recent_food_production,
            economy.recent_food_consumption,
        )
    };
    // The 160-person day-two shock intentionally creates unemployed,
    // temporarily insolvent households. In a private market their hunger is
    // not authority to demand output nobody can buy: requiring production to
    // equal the whole population—or one producer building per four people—
    // rewards unsold overproduction and endless duplicate farms. The stress
    // invariant is instead that the settlement has diversified food capacity,
    // embodied producers remain within the same critical-production threshold
    // used by planning, and at least one day of realised demand physically
    // exists somewhere in its economy. Normal-sized scenarios retain their
    // stricter food-security and promotion assertions.
    assert!(
        food_businesses >= 2,
        "arrival economy never established diversified food capacity: population={} food_businesses={} buildings={:?}",
        residents,
        food_businesses,
        snapshot.buildings,
    );
    assert!(
        mills > 0,
        "arrival economy grew Farmsteads without constructing the Windmill required to make Wheat edible: buildings={:?}",
        snapshot.buildings,
    );
    assert!(
        snapshot.buildings[building_index(SettlementBuildingKind::Bakery)] > 0,
        "arrival economy never constructed its Hamlet-level Bakery: buildings={:?}",
        snapshot.buildings,
    );
    assert!(
        evidence.saw_flour_present,
        "arrival economy never physically milled Wheat into Flour: {evidence:#?}"
    );
    assert!(
        evidence.saw_bread_present,
        "arrival economy never physically baked Flour into Bread: {evidence:#?}"
    );
    assert!(
        recent_production >= recent_consumption * 0.75,
        "producer activity fell below 75% of realised food consumption: production={:.1}/day consumption={:.1}/day",
        recent_production,
        recent_consumption,
    );
    let physical_edible = world
        .query::<&GoodsInventory>()
        .iter(world)
        .map(GoodsInventory::edible_amount)
        .fold(0_u32, u32::saturating_add);
    let realised_one_day_reserve = recent_consumption.ceil() as u32;
    assert!(
        physical_edible >= realised_one_day_reserve,
        "arrival economy never retained one day of realised food demand: population={} consumption={recent_consumption:.1}/day reserve_target={realised_one_day_reserve} edible={physical_edible}",
        residents,
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
                && evidence
                    .producer_staffed_since_seconds
                    .get(entity)
                    .is_some_and(|staffed_at| now - staffed_at >= settle_grace)
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
        .query::<(Entity, &SettlementBuilding)>()
        .iter(world)
        .filter(|(_, building)| {
            matches!(
                building.kind,
                SettlementBuildingKind::Farmstead
                    | SettlementBuildingKind::FishermansHut
                    | SettlementBuildingKind::LumberjackHut
                    | SettlementBuildingKind::Windmill
                    | SettlementBuildingKind::Bakery
            )
        })
        .flat_map(|(entity, building)| {
            building
                .workers
                .iter()
                .filter(|worker| {
                    !evidence.productive_workers.contains(*worker)
                        && evidence
                            .producer_worker_since_seconds
                            .get(&(entity, (*worker).clone()))
                            .is_some_and(|hired_at| now - hired_at >= settle_grace)
                })
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
        .query::<(
            Entity,
            &SettlementBuilding,
            &PlayerPosition,
            &PlayerRotation,
        )>()
        .iter(world)
        .map(|(entity, building, position, rotation)| {
            (
                entity,
                building.settlement.clone(),
                building.kind,
                building.kind.entrance_position(position.0, rotation.0),
            )
        })
        .collect();
    let obstacle_grid = world.resource::<SpatialObstacleGrid>();
    for road in roads.iter().filter(|road| road.is_complete()) {
        assert!(
            road.built_points()
                .windows(2)
                .all(|segment| !obstacle_grid.segment_blocked(segment[0], segment[1])),
            "a completed road was later covered by a building: {:?}",
            road.built_points()
        );
    }
    for (entity, settlement, kind, door) in building_doors {
        let door = Vec2::new(door.x, door.z);
        let mature = evidence
            .building_first_seen_seconds
            .get(&entity)
            .is_some_and(|built_at| now - built_at >= settle_grace);
        let has_complete_connector = roads.iter().any(|road| {
            road.is_complete()
                && road.settlement == settlement
                && road
                    .built_points()
                    .iter()
                    .any(|point| point.distance_squared(door) <= 2.1_f32.powi(2))
        });
        if mature {
            assert!(
                has_complete_connector,
                "mature {kind:?} door {door:?} remained without a completed village path for at least {ARRIVAL_STRESS_SETTLE_GRACE_DAYS:.0} days"
            );
        } else {
            let has_any_connector = roads.iter().any(|road| {
                road.settlement == settlement
                    && road
                        .points
                        .first()
                        .is_some_and(|point| point.distance_squared(door) <= 2.1_f32.powi(2))
            });
            assert!(
                has_complete_connector
                    || has_any_connector
                    || world.get::<PlannedRoadAccess>(entity).is_some()
                    || world.get::<RoadRequest>(entity).is_some(),
                "recent {kind:?} door {door:?} lost both its connector and protected access claim"
            );
        }
    }
}

fn assert_policy_comparison_outcome(world: &mut World, expected_residents: usize) {
    let expected_each = expected_residents / 2;
    let settlements: Vec<_> = world
        .query::<(&Settlement, &SettlementEconomy, &SettlementPolicies)>()
        .iter(world)
        .filter(|(settlement, ..)| {
            matches!(settlement.name.as_str(), "Lab Frugal" | "Lab Mutual Aid")
        })
        .map(|(settlement, economy, policies)| (settlement.clone(), economy.clone(), *policies))
        .collect();
    assert_eq!(settlements.len(), 2, "policy comparison lost a town");
    let (frugal_deaths, mutual_deaths) = {
        let mortality = world.resource::<village::MortalityLedger>();
        (
            mortality
                .iter()
                .filter(|record| record.name.starts_with("Frugal"))
                .count(),
            mortality
                .iter()
                .filter(|record| record.name.starts_with("Mutual"))
                .count(),
        )
    };
    assert_eq!(
        settlements
            .iter()
            .map(|(settlement, ..)| settlement.residents as usize)
            .sum::<usize>()
            .saturating_add(frugal_deaths)
            .saturating_add(mutual_deaths),
        expected_residents,
        "the policy comparison did not account for every admitted resident"
    );
    let frugal = settlements
        .iter()
        .find(|(settlement, ..)| settlement.name == "Lab Frugal")
        .expect("Frugal policy town missing");
    let mutual = settlements
        .iter()
        .find(|(settlement, ..)| settlement.name == "Lab Mutual Aid")
        .expect("Mutual Aid policy town missing");
    assert_eq!(
        frugal.0.residents as usize + frugal_deaths,
        expected_each,
        "the Frugal town did not receive its complete cohort"
    );
    assert_eq!(
        mutual.0.residents as usize + mutual_deaths,
        expected_each,
        "the Mutual Aid town did not receive its complete cohort"
    );
    assert_eq!(frugal.2.strategy, CivicStrategy::Frugal);
    assert_eq!(mutual.2.strategy, CivicStrategy::MutualAid);
    assert!(
        settlements.iter().all(|(_, economy, _)| {
            economy.private_job_positions
                == economy
                    .private_filled_jobs
                    .saturating_add(economy.private_vacant_jobs)
        }),
        "live private vacancy accounting is inconsistent: {settlements:#?}"
    );

    let mut kinds = HashMap::<String, HashMap<SettlementBuildingKind, usize>>::new();
    let building_kind_by_id: HashMap<_, _> = world
        .query::<(&BuildingId, &SettlementBuilding)>()
        .iter(world)
        .map(|(id, building)| {
            *kinds
                .entry(building.settlement.clone())
                .or_default()
                .entry(building.kind)
                .or_default() += 1;
            (*id, building.kind)
        })
        .collect();
    for name in ["Lab Frugal", "Lab Mutual Aid"] {
        let built = kinds.get(name).expect("comparison town built nothing");
        for required in [
            SettlementBuildingKind::Farmstead,
            SettlementBuildingKind::Windmill,
            SettlementBuildingKind::Bakery,
        ] {
            assert!(
                built.get(&required).copied().unwrap_or(0) > 0,
                "{name} never completed its grain chain: {built:#?}"
            );
        }
        assert_eq!(
            built
                .get(&SettlementBuildingKind::FishermansHut)
                .copied()
                .unwrap_or(0),
            0,
            "{name} found fishing in a grain-only policy comparison"
        );
    }
    let false_public_jobs: Vec<_> = world
        .query::<(&CharacterName, &EmployedAt)>()
        .iter(world)
        .filter_map(|(name, employment)| {
            building_kind_by_id
                .get(&employment.0)
                .copied()
                .filter(|kind| !village::is_private_business(*kind))
                .map(|kind| (name.0.clone(), kind))
        })
        .collect();
    assert!(
        false_public_jobs.is_empty(),
        "civic buildings advertised unfunded private jobs: {false_public_jobs:?}"
    );
    assert!(
        world
            .query::<&NavigationRouteFailed>()
            .iter(world)
            .next()
            .is_none(),
        "policy comparison ended with a failed route"
    );
    println!(
        "LAB policy result Frugal living={} deaths={} reserve={:.1}d hungry={} private_jobs={}/{} vacant={} civic_jobs={}/{} vacant={} seeking={} best_wage={} | MutualAid living={} deaths={} reserve={:.1}d hungry={} private_jobs={}/{} vacant={} civic_jobs={}/{} vacant={} seeking={} best_wage={}",
        frugal.0.residents,
        frugal_deaths,
        frugal.1.reserve_days,
        frugal.1.unmet_food,
        frugal.1.private_filled_jobs,
        frugal.1.private_job_positions,
        frugal.1.private_vacant_jobs,
        frugal.1.civic_filled_jobs,
        frugal.1.civic_job_positions,
        frugal.1.civic_vacant_jobs,
        frugal.1.job_seekers,
        shared::economy::format_money(frugal.1.best_open_private_wage),
        mutual.0.residents,
        mutual_deaths,
        mutual.1.reserve_days,
        mutual.1.unmet_food,
        mutual.1.private_filled_jobs,
        mutual.1.private_job_positions,
        mutual.1.private_vacant_jobs,
        mutual.1.civic_filled_jobs,
        mutual.1.civic_job_positions,
        mutual.1.civic_vacant_jobs,
        mutual.1.job_seekers,
        shared::economy::format_money(mutual.1.best_open_private_wage),
    );
}

fn assert_economy_soak_outcome(world: &mut World, evidence: &Evidence) {
    const RESIDENTS_PER_SETTLEMENT: u32 = 30;
    const TOTAL_RESIDENTS: u32 = RESIDENTS_PER_SETTLEMENT * 3;

    let snapshot = milestone(world);
    let people = people(world);
    let total_deaths = world.resource::<village::MortalityLedger>().total_deaths;
    assert_eq!(
        u64::from(snapshot.residents).saturating_add(total_deaths),
        u64::from(TOTAL_RESIDENTS),
        "the economy soak did not admit all scheduled residents: {people:#?}"
    );
    assert_eq!(
        snapshot.housed, snapshot.residents as usize,
        "twenty migration-free days did not house every surviving resident: {snapshot:#?}"
    );
    assert!(
        evidence.max_immigration_queue_depth >= 2,
        "the staged arrivals never exercised a physical immigration line"
    );

    let settlements: Vec<_> = world
        .query::<&Settlement>()
        .iter(world)
        .map(|settlement| (settlement.name.clone(), settlement.residents))
        .collect();
    for (name, arrival_prefix) in [
        ("Lab Meadow", "MeadowArrival"),
        ("Lab Coldbarrow", "ColdArrival"),
        ("Lab Greenwood", "GreenArrival"),
    ] {
        let residents = settlements
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, residents)| *residents)
            .unwrap_or_default();
        let deaths = world
            .resource::<village::MortalityLedger>()
            .iter()
            .filter(|record| record.name.starts_with(arrival_prefix))
            .count() as u32;
        assert_eq!(
            residents.saturating_add(deaths),
            RESIDENTS_PER_SETTLEMENT,
            "{name} did not receive its even share of the arrival schedule: {settlements:?}"
        );

        let mut kinds = HashMap::<SettlementBuildingKind, usize>::new();
        for building in world
            .query::<&SettlementBuilding>()
            .iter(world)
            .filter(|building| building.settlement == name)
        {
            *kinds.entry(building.kind).or_default() += 1;
        }
        assert!(
            kinds
                .get(&SettlementBuildingKind::Farmstead)
                .copied()
                .unwrap_or_default()
                + kinds
                    .get(&SettlementBuildingKind::FishermansHut)
                    .copied()
                    .unwrap_or_default()
                > 0,
            "{name} reached day 50 without a food extractor: {kinds:?}"
        );
    }

    assert!(
        world
            .query::<&NavigationRouteFailed>()
            .iter(world)
            .next()
            .is_none(),
        "the economy soak ended with a failed route: {people:#?}"
    );
    assert!(world
        .query::<&GoodsInventory>()
        .iter(world)
        .all(|inventory| inventory.used_bulk() <= inventory.bulk_capacity()));
    for (household, economy, pantry) in world
        .query::<(&Household, &HouseholdEconomy, &GoodsInventory)>()
        .iter(world)
    {
        if household.resident_ids.is_empty() {
            assert_eq!(
                economy.pennies, 0,
                "an empty household retained a ghost necessities purse",
            );
            assert!(
                pantry.is_empty(),
                "an empty household retained food or goods outside the estate flow",
            );
        }
    }

    let living_people: HashSet<PersonId> = world
        .query::<(&PersonId, &Health)>()
        .iter(world)
        .filter_map(|(person, health)| (!health.is_dead()).then_some(*person))
        .collect();
    let companies: HashMap<CompanyId, CompanyOwnership> = world
        .query::<(&CompanyId, &CompanyOwnership)>()
        .iter(world)
        .map(|(company, ownership)| (*company, ownership.clone()))
        .collect();
    let active_companies: HashSet<CompanyId> = world
        .query::<&OperatedBy>()
        .iter(world)
        .map(|company| company.0)
        .collect();
    for (company, ownership) in &companies {
        assert_eq!(
            ownership
                .shares()
                .iter()
                .map(|holding| u32::from(holding.shares))
                .sum::<u32>(),
            u32::from(COMPANY_TOTAL_SHARES),
            "company #{} lost part of its 1,000-share cap table",
            company.0,
        );
        assert!(
            active_companies.contains(company)
                || ownership
                    .shares()
                    .iter()
                    .any(|holding| living_people.contains(&holding.shareholder)),
            "ownerless company #{} survived without a site or living shareholder",
            company.0,
        );
    }
    for (building, owner, operated_by) in world
        .query::<(&SettlementBuilding, Option<&OwnedBy>, Option<&OperatedBy>)>()
        .iter(world)
        .filter(|(building, _, _)| village::is_private_business(building.kind))
    {
        let company = operated_by.unwrap_or_else(|| {
            panic!(
                "private {} in '{}' has no legal operating company",
                building.kind.label(),
                building.settlement,
            )
        });
        let company_id = company.0;
        assert!(
            companies.contains_key(&company_id),
            "private {} in '{}' refers to missing company #{}",
            building.kind.label(),
            building.settlement,
            company_id.0,
        );
        if let Some(owner) = owner {
            assert!(
                companies
                    .get(&company_id)
                    .is_some_and(|ownership| ownership.share_count(owner.0) > 0),
                "owner #{} holds no shares in company #{} operating their {} in '{}'",
                owner.0 .0,
                company_id.0,
                building.kind.label(),
                building.settlement,
            );
        }
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
    let arrival_waves = if scenario.runs_arrival_waves() {
        lab_arrival_waves()
    } else {
        Vec::new()
    };
    let arrival_count = arrival_waves.iter().map(|wave| wave.count).sum::<usize>();
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
    let mut next_stress_structure_log = 0.0;
    let mut next_stress_progress_audit = 0.0;
    let mut person_progress: HashMap<Entity, (String, f32)> = HashMap::new();
    let mut life_ledger = LabLifeLedger::default();
    let mut stress_task_ledger = StressTaskLedger::default();
    let mut next_arrival_wave = 0usize;
    let mut update_milliseconds = Vec::with_capacity(total_ticks);
    let mut burst_update_milliseconds = Vec::new();
    let mut burst_timing_ticks_remaining = 0usize;
    // The first update intentionally funds each newly founded hall. Audit
    // every subsequent update once that one-time world bootstrap is complete.
    let mut money_audit_ready = false;

    for _ in 0..total_ticks {
        app.world_mut().resource_mut::<Time>().advance_by(wall_step);
        let money_before =
            (scenario.is_economy_soak() && money_audit_ready).then(|| money_trace(app.world_mut()));
        let update_started = Instant::now();
        app.update();
        let update_millis = update_started.elapsed().as_secs_f64() * 1_000.0;
        if let Some(before) = money_before {
            let after = money_trace(app.world_mut());
            assert_eq!(
                money_trace_total(&after),
                money_trace_total(&before),
                "one server update created or destroyed coin:\n{}",
                money_trace_changes(&before, &after).join("\n"),
            );
        }
        money_audit_ready = true;
        update_milliseconds.push(update_millis);
        if burst_timing_ticks_remaining > 0 {
            burst_update_milliseconds.push(update_millis);
            burst_timing_ticks_remaining -= 1;
        }
        sim_seconds += sim_step;

        if next_arrival_wave < arrival_waves.len() {
            let current_day = app
                .world_mut()
                .query::<&WorldTime>()
                .iter(app.world())
                .next()
                .map_or(0, |clock| clock.day);
            while let Some(wave) = arrival_waves.get(next_arrival_wave).copied() {
                if current_day < wave.day.saturating_sub(1) {
                    break;
                }
                let hall_position = app
                    .world_mut()
                    .query::<(&Settlement, &PlayerPosition)>()
                    .iter(app.world())
                    .find(|(settlement, _)| settlement.name == wave.target.settlement_name())
                    .map(|(_, position)| position.0)
                    .unwrap_or_else(|| {
                        panic!("arrival wave requires {}", wave.target.settlement_name())
                    });
                spawn_lab_arrivals(
                    app.world_mut(),
                    hall_position,
                    wave.day,
                    wave.count,
                    wave.target,
                );
                if wave.count >= 40 {
                    // Ten real-time seconds (600 server updates) includes the
                    // route-queue drain and first migration decisions without
                    // allowing a short spike to hide in an all-run average.
                    burst_timing_ticks_remaining = 600;
                    burst_update_milliseconds.clear();
                    let mut timing = app.world_mut().resource_mut::<LabPhaseTimings>();
                    timing.burst_ticks_remaining = 600;
                    timing.burst_core_milliseconds.clear();
                    timing.burst_navigation_milliseconds.clear();
                }
                next_arrival_wave += 1;
                println!(
                    "LAB arrival day={} target='{}' count={} spawned={}/{} total_expected={expected_residents}",
                    wave.day,
                    wave.target.settlement_name(),
                    wave.count,
                    next_arrival_wave,
                    arrival_waves.len()
                );
            }
        }

        let world = app.world_mut();
        update_evidence(world, &mut evidence);
        if scenario.is_crowd_stress() {
            stress_task_ledger.observe(world, sim_step);
        } else {
            life_ledger.observe(world, sim_seconds, sim_step);
        }
        let structure = milestone(world);
        if last_structure.as_ref() != Some(&structure) {
            let structure_log_interval = if verbose {
                0.0
            } else if scenario.is_economy_soak() {
                REPORT_SECONDS
            } else if scenario.is_crowd_stress() {
                60.0
            } else {
                REPORT_SECONDS
            };
            if structure_log_interval == 0.0 || sim_seconds >= next_stress_structure_log {
                println!("LAB event t={:.1}m {structure:?}", sim_seconds / 60.0);
                next_stress_structure_log = sim_seconds + structure_log_interval;
            }
            last_structure = Some(structure);
        }

        if !scenario.is_crowd_stress() || sim_seconds >= next_stress_progress_audit {
            next_stress_progress_audit = sim_seconds + 1.0;
            let current_people = people(world);
            for person in &current_people {
                let progress = person_progress
                    .entry(person.entity)
                    .or_insert_with(|| (person.progress_key.clone(), sim_seconds));
                // Only consecutive time spent in an actively progress-owned
                // state counts as a stall. A completed route can briefly
                // leave its diagnostic TravelRoute behind after MoveTarget is
                // gone; if that same actor later begins a door crossing from
                // the same rounded position, carrying the old inactive age
                // forward produces an immediate false stall.
                if progress.0 != person.progress_key || !person.active_progress_expected {
                    *progress = (person.progress_key.clone(), sim_seconds);
                } else if person.active_progress_expected
                    && sim_seconds - progress.1 > STALL_SECONDS
                {
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
                                shared::building::BuildingType::MootHall
                                | shared::building::BuildingType::VillageHall
                                | shared::building::BuildingType::TownHall => {
                                    SettlementBuildingKind::Hall
                                }
                                shared::building::BuildingType::Market
                                | shared::building::BuildingType::MarketPaved => {
                                    SettlementBuildingKind::Market
                                }
                                shared::building::BuildingType::PlaceholderTavern => {
                                    SettlementBuildingKind::Tavern
                                }
                                shared::building::BuildingType::PlaceholderChurch => {
                                    SettlementBuildingKind::Church
                                }
                                shared::building::BuildingType::Windmill => {
                                    SettlementBuildingKind::Windmill
                                }
                                shared::building::BuildingType::Bakery => {
                                    SettlementBuildingKind::Bakery
                                }
                                shared::building::BuildingType::PlaceholderStorageHall => {
                                    SettlementBuildingKind::StorageHall
                                }
                                shared::building::BuildingType::PlaceholderStoneQuarry => {
                                    SettlementBuildingKind::StoneQuarry
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
                    let mut nearest_roads: Vec<_> = world
                        .iter_entities()
                        .filter_map(|entity| entity.get::<VillageRoad>())
                        .filter_map(|road| {
                            let start_distance = road
                                .built_points()
                                .iter()
                                .map(|road_point| road_point.distance(point))
                                .reduce(f32::min)?;
                            let goal_distance = goal.map(|goal| {
                                road.built_points()
                                    .iter()
                                    .map(|road_point| road_point.distance(goal))
                                    .fold(f32::INFINITY, f32::min)
                            });
                            Some((
                                start_distance,
                                goal_distance,
                                road.is_complete(),
                                road.built_through,
                                road.points.len(),
                            ))
                        })
                        .collect();
                    nearest_roads.sort_by(|a, b| {
                        a.0.min(a.1.unwrap_or(f32::INFINITY))
                            .total_cmp(&b.0.min(b.1.unwrap_or(f32::INFINITY)))
                    });
                    nearest_roads.truncate(12);
                    let rejected_segment =
                        remembered_route.as_ref().and_then(|route| {
                            let mut points = vec![point];
                            points.extend(route.waypoints.iter().map(|waypoint| {
                                Vec2::new(waypoint.position.x, waypoint.position.z)
                            }));
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
                    "LAB STALL: {} made no embodied progress for {:.0}s\n{}\nlast route: {route}\nlast handoff: {route_handoff}\nrejected segment={rejected_segment:?}\ncurrent point blocked={} nearby obstacles={nearby:?} nearby props={nearby_props:?} nearby buildings={nearby_buildings:?} nearest roads (start, goal, complete, built, total)={nearest_roads:?}\nall people: {current_people:#?}",
                    person.name,
                    sim_seconds - progress.1,
                    person.state,
                    grid.point_blocked(point),
                    );
                }
            }
        }

        if sim_seconds >= next_report {
            print_report(world, sim_seconds, verbose);
            next_report += REPORT_SECONDS;
        }
    }

    print_report(app.world_mut(), sim_seconds, !scenario.is_crowd_stress());
    print_structure_report(app.world_mut());
    print_civic_report(app.world_mut());
    print_business_report(app.world_mut());
    print_company_report(app.world_mut());
    print_trade_report(app.world_mut());
    if scenario.is_crowd_stress() {
        stress_task_ledger.print_report(app.world_mut());
        stress_task_ledger.assert_no_currently_stuck_workers();
    } else {
        life_ledger.print_final_report(app.world_mut());
    }
    print_update_timing("all", &update_milliseconds);
    if !burst_update_milliseconds.is_empty() {
        print_update_timing("arrival burst", &burst_update_milliseconds);
    }
    let timing = app.world().resource::<LabPhaseTimings>();
    print_update_timing("core", &timing.core_milliseconds);
    print_update_timing("navigation", &timing.navigation_milliseconds);
    for (label, samples) in [
        "core identity/population",
        "core civic",
        "core economy/planning",
        "core construction",
        "core activity",
        "core directory",
    ]
    .into_iter()
    .zip(timing.core_section_milliseconds.iter())
    {
        print_optional_update_timing(label, samples);
    }
    if scenario.is_crowd_stress() {
        print_stress_timing_summary(timing, &update_milliseconds);
    }
    for (label, samples) in [
        "economy markets/businesses",
        "economy households",
        "economy settlement accounts",
        "economy permits",
    ]
    .into_iter()
    .zip(timing.economy_section_milliseconds.iter())
    {
        print_optional_update_timing(label, samples);
    }
    for (label, samples) in [
        "construction moot services",
        "construction material logistics",
        "construction building progress",
        "construction fields",
        "construction road planning",
        "construction employment",
    ]
    .into_iter()
    .zip(timing.construction_section_milliseconds.iter())
    {
        print_optional_update_timing(label, samples);
    }
    let permit_timing = app.world().resource::<village::PermitPlanningDiagnostics>();
    print_optional_update_timing(
        "permit primary site",
        &permit_timing.primary_site_milliseconds,
    );
    print_optional_update_timing(
        "permit alternative site",
        &permit_timing.alternative_site_milliseconds,
    );
    print_optional_update_timing(
        "permit fishing site",
        &permit_timing.fishing_site_milliseconds,
    );
    print_optional_update_timing(
        "permit final access",
        &permit_timing.final_access_milliseconds,
    );
    print_optional_update_timing(
        "ambient pass",
        &app.world()
            .resource::<village::ambient::AmbientDiagnostics>()
            .pass_milliseconds,
    );
    if !timing.burst_core_milliseconds.is_empty() {
        print_update_timing("arrival burst core", &timing.burst_core_milliseconds);
        print_update_timing(
            "arrival burst navigation",
            &timing.burst_navigation_milliseconds,
        );
    }
    if arrival_count >= 100 {
        assert_tick_timing_budget(
            "arrival burst navigation",
            &timing.burst_navigation_milliseconds,
            15.0,
            50.0,
        );
    }
    assert!(
        next_arrival_wave == arrival_waves.len(),
        "one or more requested arrival days were outside the lab run"
    );
    if scenario.is_economy_soak() {
        assert_economy_soak_outcome(app.world_mut(), &evidence);
    } else if scenario.is_crowd_stress() {
        assert_crowd_stress_outcome(app.world_mut(), &evidence, scenario, expected_residents);
    } else if scenario.is_trade_comparison() {
        assert_trade_comparison_outcome(app.world_mut(), expected_residents);
    } else if scenario.is_policy_comparison() {
        assert_policy_comparison_outcome(app.world_mut(), expected_residents);
    } else if arrival_count == 0 {
        assert_lab_outcome(app.world_mut(), &evidence, scenario, expected_residents);
    } else {
        assert_arrival_stress_outcome(app.world_mut(), &evidence, expected_residents);
    }
    assert_eq!(
        total_money(app.world_mut()),
        expected_money,
        "the complete village loop created or destroyed coin"
    );
    assert_eq!(
        app.world()
            .resource::<village::BusinessEventQueue>()
            .pending_sale_count(),
        0,
        "the village loop ended with market money still waiting for settlement"
    );
    println!(
        "LAB queue evidence: max_moot={} max_immigration={}",
        evidence.max_moot_queue_depth, evidence.max_immigration_queue_depth,
    );
    println!(
        "LAB PASS: {scenario:?} village loop remained live for {minutes:.1} simulated minutes"
    );
}

fn print_update_timing(label: &str, samples: &[f64]) {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let percentile = |fraction: f64| {
        let index = ((sorted.len().saturating_sub(1)) as f64 * fraction).round() as usize;
        sorted[index]
    };
    let average = sorted.iter().sum::<f64>() / sorted.len() as f64;
    let over_16 = sorted.iter().filter(|sample| **sample > 16.67).count();
    let over_50 = sorted.iter().filter(|sample| **sample > 50.0).count();
    let over_100 = sorted.iter().filter(|sample| **sample > 100.0).count();
    println!(
        "LAB tick timing {label}: samples={} avg={average:.3}ms p50={:.3}ms p95={:.3}ms p99={:.3}ms max={:.3}ms over16.67={over_16} over50={over_50} over100={over_100}",
        sorted.len(),
        percentile(0.50),
        percentile(0.95),
        percentile(0.99),
        sorted.last().copied().unwrap_or_default(),
    );
}

fn average_milliseconds(samples: &[f64]) -> f64 {
    if samples.is_empty() {
        0.0
    } else {
        samples.iter().sum::<f64>() / samples.len() as f64
    }
}

fn print_stress_timing_summary(timing: &LabPhaseTimings, all: &[f64]) {
    let total = average_milliseconds(all);
    let core = average_milliseconds(&timing.core_milliseconds);
    let navigation = average_milliseconds(&timing.navigation_milliseconds);
    println!(
        "LAB bottleneck top-level total={total:.3}ms core={core:.3}ms ({:.1}%) navigation={navigation:.3}ms ({:.1}%) harness/other={:.3}ms",
        if total > 0.0 { core / total * 100.0 } else { 0.0 },
        if total > 0.0 {
            navigation / total * 100.0
        } else {
            0.0
        },
        (total - core - navigation).max(0.0),
    );

    let mut core_sections: Vec<_> = [
        "identity/population",
        "civic",
        "economy/planning",
        "construction",
        "activity",
        "directory",
    ]
    .into_iter()
    .zip(timing.core_section_milliseconds.iter())
    .map(|(label, samples)| (average_milliseconds(samples), label))
    .collect();
    core_sections.sort_by(|a, b| b.0.total_cmp(&a.0));
    println!(
        "LAB bottleneck core ranking [{}]",
        core_sections
            .iter()
            .map(|(average, label)| format!("{label}={average:.3}ms"))
            .collect::<Vec<_>>()
            .join(" > ")
    );

    let mut economy_sections: Vec<_> = [
        "markets/businesses",
        "households",
        "settlement accounts",
        "permits",
    ]
    .into_iter()
    .zip(timing.economy_section_milliseconds.iter())
    .map(|(label, samples)| (average_milliseconds(samples), label))
    .collect();
    economy_sections.sort_by(|a, b| b.0.total_cmp(&a.0));
    println!(
        "LAB bottleneck economy ranking [{}]",
        economy_sections
            .iter()
            .map(|(average, label)| format!("{label}={average:.3}ms"))
            .collect::<Vec<_>>()
            .join(" > ")
    );

    let mut construction_sections: Vec<_> = [
        "moot services",
        "material logistics",
        "building progress",
        "fields",
        "road planning",
        "employment",
    ]
    .into_iter()
    .zip(timing.construction_section_milliseconds.iter())
    .map(|(label, samples)| (average_milliseconds(samples), label))
    .collect();
    construction_sections.sort_by(|a, b| b.0.total_cmp(&a.0));
    println!(
        "LAB bottleneck construction ranking [{}]",
        construction_sections
            .iter()
            .map(|(average, label)| format!("{label}={average:.3}ms"))
            .collect::<Vec<_>>()
            .join(" > ")
    );
}

fn print_optional_update_timing(label: &str, samples: &[f64]) {
    if !samples.is_empty() {
        print_update_timing(label, samples);
    }
}

fn assert_tick_timing_budget(label: &str, samples: &[f64], p99_limit: f64, max_limit: f64) {
    assert!(!samples.is_empty(), "{label} did not record any samples");
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let p99_index = ((sorted.len().saturating_sub(1)) as f64 * 0.99).round() as usize;
    let p99 = sorted[p99_index];
    let max = sorted.last().copied().unwrap_or_default();
    assert!(
        p99 <= p99_limit && max <= max_limit,
        "{label} exceeded its regression budget: p99={p99:.3}ms (limit {p99_limit:.3}), max={max:.3}ms (limit {max_limit:.3})"
    );
}
