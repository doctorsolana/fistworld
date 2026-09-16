//! Natural, physical arrivals from outside the playable world.
//!
//! Newcomers enter on a real ocean-connected dinghy, then decide which
//! settlement looks promising. They sail to the coast nearest that choice,
//! abandon the temporary boat at landfall, and then use the ordinary migration
//! route and visible Moot Hall queue. The director is deliberately small and
//! demand-sensitive, without adding a timer or decision tree to every NPC.

use bevy::prelude::*;
use lightyear::prelude::{NetworkTarget, Replicate};

use shared::components::{
    AboardBoat, CharacterActivity, CharacterKind, CharacterMotion, CharacterObjective,
    ImmigrantArrivalBoat, PlayerBoat, PlayerPosition, PlayerRotation, Settlement,
    SettlementBuildingKind, SettlementId, Vessel, WorldTime,
};
use shared::economy::SettlementEconomy;
use shared::region::RegionCoord;
use shared::terrain::WorldTerrain;

use crate::player::boat::{
    BOAT_ARRIVE_EPSILON, CoastalVoyage, HELM_LOCAL, TerrainDependencies, VesselNavigation,
    VesselNavigationQueue, VesselRoute, WaterPlanResult, WaterSearch,
    arrival::{ArrivalBodies, ArrivalOccupancy},
    coastal_voyages,
};
use crate::world::village::VillagerIntent;
#[cfg(test)]
use crate::world::village_roads::overland_trade_corridor_exists;
use crate::world::village_roads::{IncrementalCorridorResult, IncrementalOverlandCorridorSearch};
use std::time::Duration;

mod admission;
mod director;
pub(crate) use admission::ChoosingSettlement;
pub use director::plan_natural_immigration;

const DEFAULT_IMMIGRANTS_PER_DAY: f32 = 3.0;
const DEFAULT_WORLD_NPC_CAP: usize = 5_000;
const MIN_IMMIGRANTS_PER_DAY: f32 = 0.1;
const MAX_IMMIGRANTS_PER_DAY: f32 = 20.0;
const FIRST_ARRIVAL_DELAY_DAYS: f32 = 0.06;
const RETRY_DELAY_DAYS: f32 = 0.08;
const MAX_ACTIVE_VOYAGES: usize = 8;
const MAX_QUEUED_MANUAL_ARRIVALS: u8 = 8;
/// Landfall discovery is terrain work, not a reason to suspend an entire
/// hosted world. One retained A* receives this small slice per fixed tick and
/// preserves its frontier when it yields.
const LANDFALL_SEARCH_SLICE: Duration = Duration::from_micros(500);
const LANDFALL_ENTRANCE_EPSILON_SQ: f32 = 0.01;

#[derive(Clone, Copy, Debug)]
enum CachedSettlementLandfall {
    Reachable {
        entrance: Vec3,
        voyage: CoastalVoyage,
    },
    Unreachable {
        entrance: Vec3,
    },
}

impl CachedSettlementLandfall {
    fn entrance(self) -> Vec3 {
        match self {
            Self::Reachable { entrance, .. } | Self::Unreachable { entrance } => entrance,
        }
    }

    fn matches(self, entrance: Vec3) -> bool {
        self.entrance().distance_squared(entrance) <= LANDFALL_ENTRANCE_EPSILON_SQ
    }
}

#[derive(Debug)]
struct PendingLandfallSearch {
    // The selected opportunity owns both proof phases. Changing day-to-day
    // scores must not discard a completed landfall and chase another town.
    choice: SettlementChoice,
    candidates: Vec<CoastalVoyage>,
    candidate_index: usize,
    corridor: Option<IncrementalOverlandCorridorSearch>,
    terrain_reads: TerrainDependencies,
}

/// One suspended decision per actual waiting hull, bounded by the arrival cap.
/// Moving a frontier between turns preserves all of its certified work.
#[derive(Debug, Default)]
struct PendingDecision {
    landfall: Option<PendingLandfallSearch>,
    water: Option<PendingWaterVoyage>,
}

#[derive(Clone, Copy, Debug)]
enum PendingLandfallResult {
    Pending,
    Reachable(CoastalVoyage),
    Unreachable,
}

impl PendingLandfallSearch {
    fn new(choice: &SettlementChoice, coastal_approaches: &[CoastalVoyage]) -> Self {
        let mut candidates = coastal_approaches.to_vec();
        candidates.sort_by(|a, b| {
            a.landing
                .xz()
                .distance_squared(choice.entrance.xz())
                .total_cmp(&b.landing.xz().distance_squared(choice.entrance.xz()))
        });
        Self {
            choice: choice.clone(),
            candidates,
            candidate_index: 0,
            corridor: None,
            terrain_reads: default(),
        }
    }

    fn advance(&mut self, terrain: &WorldTerrain) -> PendingLandfallResult {
        if self.terrain_reads.changed(terrain) {
            self.candidate_index = 0;
            self.corridor = None;
        }
        let Some(candidate) = self.candidates.get(self.candidate_index).copied() else {
            return PendingLandfallResult::Unreachable;
        };
        let corridor = self.corridor.get_or_insert_with(|| {
            let corridor = IncrementalOverlandCorridorSearch::new(
                candidate.landing.xz(),
                self.choice.entrance.xz(),
            );
            let (min, max) = corridor.terrain_bounds();
            self.terrain_reads.observe_bounds(terrain, min, max);
            corridor
        });
        match corridor.advance(terrain, LANDFALL_SEARCH_SLICE) {
            IncrementalCorridorResult::Pending => PendingLandfallResult::Pending,
            IncrementalCorridorResult::Reachable => PendingLandfallResult::Reachable(candidate),
            IncrementalCorridorResult::Unreachable => {
                self.candidate_index += 1;
                self.corridor = None;
                if self.candidate_index >= self.candidates.len() {
                    PendingLandfallResult::Unreachable
                } else {
                    // Starting at most one new A* per fixed tick prevents a
                    // run of immediately blocked candidates from bypassing
                    // the caller's intended work budget.
                    PendingLandfallResult::Pending
                }
            }
        }
    }
}

#[derive(Resource, Debug)]
pub struct NaturalImmigrationDirector {
    enabled: bool,
    interval_days: f32,
    next_arrival_world_seconds: Option<f64>,
    sequence: u64,
    world_npc_cap: usize,
    population_cap_announced: bool,
    coastal_approaches: Vec<CoastalVoyage>,
    settlement_landfalls: bevy::platform::collections::HashMap<Entity, CachedSettlementLandfall>,
    pending_landfall: Option<PendingLandfallSearch>,
    pending_water: Option<PendingWaterVoyage>,
    /// Explicit lab requests. They use the normal physical arrival pipeline
    /// but do not enable recurring immigration or disturb its calendar.
    manual_arrivals: u8,
    configured: bool,
    steady_rate: bool,
    deciding: Option<Entity>,
    decision_cursor: Option<(u64, u64)>,
    suspended_decisions: bevy::platform::collections::HashMap<Entity, PendingDecision>,
    terrain_revision: Option<(u64, u32)>,
}

impl Default for NaturalImmigrationDirector {
    fn default() -> Self {
        let lab = env_flag("FISTWORLD_VILLAGE_LAB_RUNTIME")
            || env_flag("FISTWORLD_REALWORLD_LAB_RUNTIME");
        let enabled = std::env::var("FISTWORLD_NATURAL_IMMIGRATION")
            .ok()
            .map(|raw| parse_bool(&raw))
            .unwrap_or(!lab);
        let immigrants_per_day = std::env::var("FISTWORLD_IMMIGRANTS_PER_DAY")
            .ok()
            .and_then(|raw| raw.parse::<f32>().ok())
            .filter(|rate| rate.is_finite() && *rate > 0.0)
            .map(|rate| rate.clamp(MIN_IMMIGRANTS_PER_DAY, MAX_IMMIGRANTS_PER_DAY))
            // Preserve old deployments that still set the less intuitive
            // interval variable. The arrivals-per-day setting wins whenever
            // both are present.
            .or_else(|| {
                std::env::var("FISTWORLD_IMMIGRATION_INTERVAL_DAYS")
                    .ok()
                    .and_then(|raw| raw.parse::<f32>().ok())
                    .filter(|days| days.is_finite() && *days > 0.0)
                    .map(|days| {
                        (1.0 / days.clamp(0.05, 10.0))
                            .clamp(MIN_IMMIGRANTS_PER_DAY, MAX_IMMIGRANTS_PER_DAY)
                    })
            })
            .unwrap_or(DEFAULT_IMMIGRANTS_PER_DAY);
        let interval_days = interval_days_for_rate(immigrants_per_day);
        let world_npc_cap = std::env::var("FISTWORLD_WORLD_NPC_CAP")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .unwrap_or(DEFAULT_WORLD_NPC_CAP);
        if enabled {
            info!(
                "Natural immigration enabled (base {:.2} immigrants/world day, world NPC cap {})",
                immigrants_per_day, world_npc_cap
            );
        }
        Self {
            enabled,
            interval_days,
            next_arrival_world_seconds: None,
            sequence: 0,
            world_npc_cap,
            population_cap_announced: false,
            coastal_approaches: Vec::new(),
            settlement_landfalls: default(),
            pending_landfall: None,
            pending_water: None,
            manual_arrivals: 0,
            configured: false,
            steady_rate: false,
            deciding: None,
            decision_cursor: None,
            suspended_decisions: default(),
            terrain_revision: None,
        }
    }
}

impl NaturalImmigrationDirector {
    /// Allocates only when the opt-in world observer explicitly requests a sample.
    pub(crate) fn planning_status(&self) -> serde_json::Value {
        let water = self.pending_water.as_ref().map(|pending| {
            let (phase, expanded, frontier, sampled_points, revision) = pending.search.progress();
            serde_json::json!({"town_entity":pending.choice.entity.to_bits(),
                "town_name":pending.choice.name, "start":pending.entry.start.to_array(),
                "mooring":pending.voyage.mooring.to_array(), "phase":phase,
                "expanded":expanded,"frontier":frontier,"sampled_points":sampled_points,"revision":revision,
                "grid_metres":pending.search.grid_metres()})
        });
        let land = self.pending_landfall.as_ref().map(|pending| {
            serde_json::json!({"town_entity":pending.choice.entity.to_bits(),
                "candidate":pending.candidate_index,"candidates":pending.candidates.len(),
                "corridor":pending.corridor.as_ref().map(|corridor|format!("{corridor:?}"))})
        });
        serde_json::json!({"boat":self.deciding.map(Entity::to_bits),"land":land,"water":water,
            "suspended_decisions":self.suspended_decisions.len(),
            "next_entry_at":self.next_arrival_world_seconds,"terrain_revision":self.terrain_revision})
    }

    /// Controlled labs retain real voyages without ambient extra arrivals.
    pub(crate) fn manual_only() -> Self {
        Self {
            enabled: false,
            ..Self::default()
        }
    }

    /// Queue one complete God-mode arrival. Bounded so repeated UI clicks
    /// cannot create an unbounded pathfinding backlog.
    pub fn request_manual_arrival(&mut self) -> bool {
        if self.manual_arrivals >= MAX_QUEUED_MANUAL_ARRIVALS {
            return false;
        }
        self.manual_arrivals += 1;
        true
    }

    fn defer_arrival(&mut self, manual: bool, now: f64, cycle: f64) {
        if manual {
            self.finish_manual_arrival();
        } else {
            self.next_arrival_world_seconds = Some(now + cycle * f64::from(RETRY_DELAY_DAYS));
        }
    }

    fn finish_manual_arrival(&mut self) {
        self.manual_arrivals = self.manual_arrivals.saturating_sub(1);
    }
}

fn interval_days_for_rate(immigrants_per_day: f32) -> f32 {
    // Config accepts any positive rate. Keep sparse arrivals sparse, with f64
    // arithmetic so subnormal inputs cannot overflow into an infinite deadline.
    // Only an interval beyond f32's representation saturates; zero is handled
    // by the caller's explicit recurring-arrivals disable branch.
    let rate = immigrants_per_day.min(MAX_IMMIGRANTS_PER_DAY);
    (1.0_f64 / f64::from(rate)).min(f64::from(f32::MAX)) as f32
}

/// Discover immutable edge approaches while the server is starting, before a
/// playing client can observe a first-arrival hitch. The planning system keeps
/// a fallback for narrow unit-test apps and any future live map reload.
pub fn prepare_natural_immigration_coasts(
    terrain: Res<WorldTerrain>,
    mut director: ResMut<NaturalImmigrationDirector>,
) {
    if !director.coastal_approaches.is_empty() {
        return;
    }
    director.coastal_approaches = coastal_voyages(&terrain, 0);
    info!(
        "Natural immigration prepared {} ocean-connected approaches",
        director.coastal_approaches.len()
    );
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|raw| parse_bool(&raw))
}

fn parse_bool(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Authoritative admission facts retained after ordinary Moot registration.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct ImmigrantArrival {
    pub entry: Vec3,
    pub entered_at: f64,
    pub chosen_settlement: Option<SettlementId>,
    pub chosen_score: Option<f32>,
    pub chosen_at: Option<f64>,
}

#[derive(Component, Debug, Clone, Copy)]
pub struct NaturalImmigrantVoyage {
    boat: Entity,
    settlement: Option<Entity>,
}

#[derive(Component, Debug, Clone, Copy)]
pub struct NpcArrivalBoat {
    passenger: Entity,
    settlement: Entity,
    mooring: Vec2,
    landing: Vec3,
    retry_at: f64,
    retry_count: u8,
    route_version: u32,
    decision_seed: u64,
    manual: bool,
}

#[derive(Clone, Debug)]
struct SettlementChoice {
    entity: Entity,
    name: String,
    position: Vec3,
    entrance: Vec3,
    score: f32,
}

#[derive(Debug)]
struct PendingWaterVoyage {
    choice: SettlementChoice,
    entry: CoastalVoyage,
    voyage: CoastalVoyage,
    search: WaterSearch,
}

fn mixed(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn personal_preference(seed: u64, settlement: Entity) -> f32 {
    let unit = (mixed(seed ^ settlement.to_bits()) & 0xffff) as f32 / u16::MAX as f32;
    unit * 12.0 - 6.0
}

/// Broad public facts plus bounded personal taste. This is not omniscient
/// optimization: the newcomer sees the same town-scale evidence a player sees
/// and carries a six-point subjective variation, so several viable towns share
/// arrivals instead of every person selecting one mathematical optimum.
fn settlement_attractiveness(
    settlement: &Settlement,
    economy: Option<&SettlementEconomy>,
    seed: u64,
    entity: Entity,
    entry_landing: Vec3,
    settlement_position: Vec3,
) -> f32 {
    if settlement.tier == shared::components::SettlementTier::Ruins {
        return f32::NEG_INFINITY;
    }
    let residents = settlement.residents;
    let frontier = (12_u32.saturating_sub(residents) as f32 / 12.0) * 42.0;
    // Geography is a bias, not a veto. A nearby town can beat a slightly
    // healthier distant one, while severe hunger or unrest still outweighs
    // even the largest possible distance pressure.
    let distance = entry_landing.xz().distance(settlement_position.xz());
    let distance_pressure = (distance / 280.0).clamp(0.0, 18.0);
    let proximity = 9.0 / (1.0 + distance / 450.0);
    let Some(economy) = economy else {
        return frontier + 12.0 + proximity - distance_pressure + personal_preference(seed, entity);
    };
    let people = residents.max(1) as f32;
    let food = if residents < 4 {
        12.0
    } else {
        (economy.reserve_days / 3.0).clamp(0.0, 1.5) * 30.0 - 20.0
            + (economy.recent_food_production / people).clamp(0.0, 1.5) * 9.0
    };
    let spare_homes = economy.housing_capacity.saturating_sub(residents) as f32;
    let housing = (spare_homes / people).clamp(0.0, 0.5) * 30.0
        - (economy.homeless_residents as f32 / people).clamp(0.0, 1.0) * 28.0;
    let net_open_work = i32::from(economy.private_vacant_jobs)
        + i32::from(economy.civic_vacant_jobs)
        - i32::from(economy.job_seekers);
    let work = (net_open_work as f32 * 3.0).clamp(-16.0, 22.0);
    let reliability = if economy.unpaid_workers == 0 {
        4.0
    } else {
        -10.0
    };
    frontier + food + housing + work + reliability + economy.prosperity * 0.22
        - economy.unrest * 0.22
        + proximity
        - distance_pressure
        + personal_preference(seed, entity)
}

fn seasonal_interval_multiplier(day: u32) -> f32 {
    match (day % 28) / 7 {
        0 => 0.75, // spring movement
        1 => 1.0,  // summer baseline
        2 => 1.15, // autumn slows
        _ => 1.65, // winter crossings are rarer
    }
}

fn absolute_world_seconds(clock: &WorldTime) -> f64 {
    f64::from(clock.day) * f64::from(clock.cycle_duration()) + f64::from(clock.seconds_in_cycle)
}

/// Keep each NPC passenger at the helm of its own authoritative boat. Client
/// rendering adds the same local buoyancy polish used for the player voyage.
pub fn sync_natural_immigrant_passengers(
    boats: Query<(&PlayerPosition, &PlayerRotation, &CharacterMotion), With<ImmigrantArrivalBoat>>,
    mut passengers: Query<
        (
            &NaturalImmigrantVoyage,
            &mut PlayerPosition,
            &mut PlayerRotation,
            &mut RegionCoord,
            &mut CharacterMotion,
            &mut CharacterActivity,
        ),
        Without<ImmigrantArrivalBoat>,
    >,
) {
    for (voyage, mut position, mut rotation, mut region, mut motion, mut activity) in
        passengers.iter_mut()
    {
        let Ok((boat_position, boat_rotation, boat_motion)) = boats.get(voyage.boat) else {
            continue;
        };
        let helm = boat_position.0 + Quat::from_rotation_y(boat_rotation.0) * HELM_LOCAL;
        // Guarded exactly like the player-hero equivalent in
        // `player/boat.rs`: an unconditional write re-replicates a passenger's
        // whole transform every tick even while the boat holds its heading.
        if position.0 != helm {
            position.0 = helm;
        }
        if rotation.0 != boat_rotation.0 {
            rotation.0 = boat_rotation.0;
        }
        let next_region = RegionCoord::from_world_pos(helm);
        if *region != next_region {
            *region = next_region;
        }
        if *motion != *boat_motion {
            *motion = *boat_motion;
        }
        activity.set_if_neq(CharacterActivity::Sitting);
    }
}

/// The temporary immigrant dinghy disappears at shore. The person is placed
/// on validated dry ground and immediately enters the existing land migration
/// and Moot Hall registration flow; no second resident-creation path exists.
pub fn finish_natural_immigrant_voyages(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    simulation_time: crate::world::simulation_time::SimulationTime,
    mut water_navigation: ResMut<VesselNavigationQueue>,
    mut boats: Query<
        (
            Entity,
            &mut NpcArrivalBoat,
            &PlayerPosition,
            Option<&VesselRoute>,
        ),
        With<PlayerBoat>,
    >,
    halls: Query<(&PlayerPosition, Option<&PlayerRotation>, &Settlement)>,
    mut passengers: Query<
        (
            &mut PlayerPosition,
            &mut PlayerRotation,
            &mut RegionCoord,
            &mut CharacterMotion,
            &mut CharacterActivity,
            &mut VillagerIntent,
            &mut NaturalImmigrantVoyage,
            Option<&mut ImmigrantArrival>,
        ),
        (Without<PlayerBoat>, Without<Settlement>),
    >,
) {
    let now = simulation_time.elapsed_real_seconds_f64();
    for (boat_entity, mut arrival, boat_position, route) in boats.iter_mut() {
        if passengers.get(arrival.passenger).is_err() {
            water_navigation.take(boat_entity);
            commands.entity(boat_entity).despawn();
            continue;
        }
        let invalid_destination = !halls
            .get(arrival.settlement)
            .is_ok_and(|(_, _, town)| town.tier != shared::components::SettlementTier::Ruins);
        let flooded_landing = terrain
            .get_water_height(arrival.landing.x, arrival.landing.z)
            .is_some();
        if invalid_destination || flooded_landing {
            if let Ok((
                _,
                _,
                _,
                mut motion,
                mut activity,
                mut intent,
                mut voyage,
                Some(mut facts),
            )) = passengers.get_mut(arrival.passenger)
            {
                facts.chosen_settlement = None;
                facts.chosen_score = None;
                facts.chosen_at = None;
                voyage.settlement = None;
                *intent = VillagerIntent::Idle;
                *motion = CharacterMotion::STATIONARY;
                activity.set_if_neq(CharacterActivity::Sitting);
                commands
                    .entity(boat_entity)
                    .remove::<(
                        NpcArrivalBoat,
                        VesselRoute,
                        crate::player::boat::VesselRouteCertification,
                    )>()
                    .insert((
                        ChoosingSettlement {
                            passenger: arrival.passenger,
                            facts: *facts,
                            decision_seed: arrival.decision_seed,
                            manual: arrival.manual,
                            retry_at: 0.,
                            rejected_towns: vec![arrival.settlement],
                        },
                        CharacterMotion::STATIONARY,
                    ));
                water_navigation.take(boat_entity);
            }
            continue;
        }
        if route.is_some() {
            continue;
        }
        if boat_position.0.xz().distance(arrival.mooring) > BOAT_ARRIVE_EPSILON + 0.5 {
            if !water_navigation.is_pending(boat_entity)
                && (now >= arrival.retry_at
                    || arrival.route_version != terrain.modification_version())
            {
                arrival.retry_count = arrival.retry_count.saturating_add(1);
                arrival.retry_at =
                    now + (2_f64.powi(i32::from(arrival.retry_count.min(6))) * 2.0).min(120.0);
                arrival.route_version = terrain.modification_version();
                water_navigation.request(
                    boat_entity,
                    crate::player::boat::VesselGoal::Sail(arrival.mooring),
                );
            }
            continue;
        }
        // A terrain change must not put the immigrant down in newly flooded water.
        if terrain
            .get_water_height(arrival.landing.x, arrival.landing.z)
            .is_some()
        {
            continue;
        }
        let Ok((
            mut position,
            mut rotation,
            mut region,
            mut motion,
            mut activity,
            mut intent,
            voyage,
            _,
        )) = passengers.get_mut(arrival.passenger)
        else {
            commands.entity(boat_entity).despawn();
            continue;
        };
        debug_assert_eq!(voyage.boat, boat_entity);
        debug_assert_eq!(voyage.settlement, Some(arrival.settlement));
        let landing = Vec3::new(
            arrival.landing.x,
            terrain.get_height(arrival.landing.x, arrival.landing.z),
            arrival.landing.z,
        );
        position.0 = landing;
        *region = RegionCoord::from_world_pos(landing);
        *motion = CharacterMotion::STATIONARY;
        activity.set_if_neq(CharacterActivity::Idle);

        if let Ok((hall, hall_rotation, _)) = halls.get(arrival.settlement) {
            let entrance = SettlementBuildingKind::Hall
                .entrance_position(hall.0, hall_rotation.map_or(0.0, |rotation| rotation.0));
            let direction = entrance.xz() - landing.xz();
            if direction.length_squared() > 1.0e-4 {
                rotation.0 = f32::atan2(-direction.x, -direction.y);
            }
            *intent = VillagerIntent::Travelling {
                settlement: arrival.settlement,
            };
            commands
                .entity(arrival.passenger)
                .insert(crate::player::hero::MoveTarget(entrance));
        } else {
            *intent = VillagerIntent::Idle;
        }
        commands
            .entity(arrival.passenger)
            .remove::<AboardBoat>()
            .remove::<NaturalImmigrantVoyage>();
        commands.entity(boat_entity).despawn();
        info!(
            "Natural immigrant {} landed and began the walk to settlement {:?}",
            arrival.passenger, arrival.settlement
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::hero::MoveTarget;
    use crate::world::pathfinding::PathfindingBudgetSettings;
    use crate::world::simulation_time::SimulationDelta;
    use crate::world::village_roads::{
        NavigationRouteFailed, TravelRoute, VillageRoadGraph, embodied_land_route_exists,
    };
    use shared::components::{SettlementTier, TimeWarp};

    fn settlement(name: &str, residents: u32) -> Settlement {
        Settlement {
            name: name.to_string(),
            tier: shared::components::SettlementTier::Hamlet,
            residents,
            treasury: 0,
        }
    }

    #[test]
    fn healthy_opportunity_outscores_starving_overcrowding() {
        let entity = Entity::from_bits(7);
        let mut healthy = SettlementEconomy {
            reserve_days: 4.0,
            recent_food_production: 24.0,
            housing_capacity: 20,
            private_vacant_jobs: 4,
            prosperity: 62.0,
            ..default()
        };
        let origin = Vec3::ZERO;
        let place = Vec3::new(200.0, 0.0, 0.0);
        let good = settlement_attractiveness(
            &settlement("Good", 12),
            Some(&healthy),
            9,
            entity,
            origin,
            place,
        );
        healthy.reserve_days = 0.0;
        healthy.recent_food_production = 0.0;
        healthy.housing_capacity = 4;
        healthy.homeless_residents = 8;
        healthy.private_vacant_jobs = 0;
        healthy.job_seekers = 8;
        healthy.prosperity = 5.0;
        healthy.unrest = 70.0;
        let bad = settlement_attractiveness(
            &settlement("Bad", 12),
            Some(&healthy),
            9,
            entity,
            origin,
            place,
        );
        assert!(good.is_finite() && bad.is_finite());
        assert!(good > bad + 50.0);
    }

    #[test]
    fn an_empty_frontier_moot_can_receive_founders() {
        let score = settlement_attractiveness(
            &settlement("New", 0),
            Some(&SettlementEconomy::default()),
            1,
            Entity::from_bits(1),
            Vec3::ZERO,
            Vec3::ZERO,
        );
        assert!(score.is_finite(), "frontier score was {score}");
    }

    #[test]
    fn winter_arrivals_are_slower_than_spring_arrivals() {
        assert!(seasonal_interval_multiplier(24) > seasonal_interval_multiplier(2));
    }

    #[test]
    fn three_immigrants_per_day_maps_to_a_third_day_base_interval() {
        assert!((interval_days_for_rate(3.0) - (1.0 / 3.0)).abs() < f32::EPSILON);
    }

    #[test]
    fn sparse_configured_arrivals_keep_their_rate_and_a_finite_deadline() {
        assert_eq!(interval_days_for_rate(0.01), 100.);
        assert_eq!(interval_days_for_rate(0.0001), 10_000.);
        let smallest_rate = f32::from_bits(1);
        let interval = interval_days_for_rate(smallest_rate);
        assert!(interval.is_finite() && interval > 10_000.);
        let deadline = f64::from(WorldTime::new_default().cycle_duration()) * f64::from(interval);
        assert!(deadline.is_finite());
    }

    #[test]
    fn natural_arrivals_avoid_player_and_earlier_immigrant_boats() {
        use crate::player::boat::arrival::ARRIVAL_HULL_RADIUS;

        let terrain = WorldTerrain::default();
        let coast = coastal_voyages(&terrain, 0)[0];
        let mut app = App::new();
        app.insert_resource(terrain)
            .insert_resource(NaturalImmigrationDirector {
                enabled: false,
                interval_days: interval_days_for_rate(DEFAULT_IMMIGRANTS_PER_DAY),
                next_arrival_world_seconds: None,
                sequence: 0,
                world_npc_cap: DEFAULT_WORLD_NPC_CAP,
                population_cap_announced: false,
                coastal_approaches: vec![coast],
                settlement_landfalls: default(),
                pending_landfall: None,
                pending_water: None,
                manual_arrivals: 2,
                configured: false,
                steady_rate: false,
                deciding: None,
                decision_cursor: None,
                suspended_decisions: default(),
                terrain_revision: None,
            })
            .insert_resource(crate::world::dev::VillagerSeed::default())
            .init_resource::<VesselNavigationQueue>()
            .add_systems(Update, plan_natural_immigration);
        app.world_mut().spawn(WorldTime::new_default());
        app.world_mut().spawn((
            PlayerBoat,
            Vessel,
            shared::components::CommandedBy("waitingplayer".into()),
            PlayerPosition(coast.start),
        ));
        let hall = coast.landing;
        let settlement = app
            .world_mut()
            .spawn((
                settlement("Arrival Spacing", 0),
                PlayerPosition(hall),
                PlayerRotation(0.0),
            ))
            .id();
        // This fixture isolates physical admission after landfall proof;
        // the end-to-end voyage test below certifies the actual Hall route.
        app.world_mut()
            .resource_mut::<NaturalImmigrationDirector>()
            .settlement_landfalls
            .insert(
                settlement,
                CachedSettlementLandfall::Reachable {
                    entrance: SettlementBuildingKind::Hall.entrance_position(hall, 0.0),
                    voyage: coast,
                },
            );
        for _ in 0..2_000 {
            app.update();
            if app
                .world_mut()
                .query_filtered::<Entity, With<NpcArrivalBoat>>()
                .iter(app.world())
                .count()
                == 2
            {
                break;
            }
        }

        assert_eq!(
            app.world()
                .resource::<NaturalImmigrationDirector>()
                .manual_arrivals,
            0
        );
        let arrivals: Vec<_> = app
            .world_mut()
            .query_filtered::<(
                &NpcArrivalBoat,
                &PlayerPosition,
                &PlayerRotation,
                &VesselRoute,
            ), With<NpcArrivalBoat>>()
            .iter(app.world())
            .map(|(boat, position, rotation, route)| {
                (*boat, position.0, rotation.0, route.waypoints.clone())
            })
            .collect();
        assert_eq!(arrivals.len(), 2);
        let minimum = ARRIVAL_HULL_RADIUS * 2.0;
        assert!(arrivals[0].1.distance(arrivals[1].1) >= minimum);
        for (boat, position, yaw, route) in arrivals {
            assert!(position.distance(coast.start) >= minimum);
            assert_eq!(
                app.world().get::<PlayerPosition>(boat.passenger).unwrap().0,
                position + Quat::from_rotation_y(yaw) * HELM_LOCAL
            );
            let terrain = app.world().resource::<WorldTerrain>();
            let mut from = position.xz();
            for waypoint in route {
                let samples = (from.distance(waypoint) / 1.5).ceil().max(1.0) as usize;
                assert!((0..=samples).all(|step| {
                    let point = from.lerp(waypoint, step as f32 / samples as f32);
                    terrain.get_water_height(point.x, point.y).is_some()
                }));
                from = waypoint;
            }
        }
    }

    #[test]
    fn manual_arrival_queue_is_bounded() {
        let mut director = NaturalImmigrationDirector::default();
        for _ in 0..MAX_QUEUED_MANUAL_ARRIVALS {
            assert!(director.request_manual_arrival());
        }
        assert!(!director.request_manual_arrival());
        assert_eq!(director.manual_arrivals, MAX_QUEUED_MANUAL_ARRIVALS);
    }

    #[test]
    fn failed_landfall_candidates_advance_one_per_slice_and_terminate() {
        let terrain = WorldTerrain::default();
        let choice = SettlementChoice {
            entity: Entity::from_bits(91),
            name: "Unreachable".to_string(),
            position: Vec3::ZERO,
            entrance: Vec3::ZERO,
            score: 100.0,
        };
        let candidates = (0..32)
            .map(|index| {
                let outside = 100_000.0 + index as f32 * 10.0;
                CoastalVoyage {
                    start: Vec3::new(outside, 0.0, outside),
                    yaw: 0.0,
                    mooring: Vec2::new(outside, outside),
                    landing: Vec3::new(outside, 0.0, outside),
                }
            })
            .collect::<Vec<_>>();
        let mut pending = PendingLandfallSearch::new(&choice, &candidates);

        for expected_checked in 1..candidates.len() {
            assert!(matches!(
                pending.advance(&terrain),
                PendingLandfallResult::Pending
            ));
            assert_eq!(pending.candidate_index, expected_checked);
        }
        assert!(matches!(
            pending.advance(&terrain),
            PendingLandfallResult::Unreachable
        ));
        assert_eq!(pending.candidate_index, candidates.len());
    }

    /// The property the whole landfall rework exists to guarantee: a real
    /// kilometres-long certification over shipped-map terrain must YIELD across
    /// many small slices instead of running to completion inside one call. The
    /// per-slice fixture above cannot prove this — its out-of-map candidates
    /// die on a blocked first cell — so a regression that ignores the deadline
    /// (or inflates `LANDFALL_SEARCH_SLICE`) would pass every other test while
    /// reintroducing the multi-minute hosted-server freeze this fixes.
    #[test]
    fn a_long_landfall_certification_yields_across_many_slices() {
        let terrain = WorldTerrain::default();
        assert!(
            matches!(
                terrain.generator.active_map_id(),
                "big_world" | "village_lab"
            ),
            "slice regression requires a shipped playable map, got '{}'",
            terrain.generator.active_map_id()
        );
        let approaches = coastal_voyages(&terrain, 0);
        assert!(
            approaches.len() >= 2,
            "the active map must expose several ocean-connected coasts"
        );

        // Start from a genuinely walkable beach (same criterion as the
        // physical-arrival test), then aim at another coast's hinterland far
        // enough away that certification needs thousands of A* expansions.
        // Precondition the pair with the synchronous corridor check so the
        // incremental search is guaranteed to terminate Reachable — this test
        // measures HOW it gets there, not whether.
        let (start, entrance) = approaches
            .iter()
            .flat_map(|start| {
                let mut targets = approaches
                    .iter()
                    .filter(|far| far.landing.xz().distance(start.landing.xz()) >= 1_200.0)
                    .collect::<Vec<_>>();
                targets.sort_by(|a, b| {
                    a.landing
                        .xz()
                        .distance_squared(start.landing.xz())
                        .total_cmp(&b.landing.xz().distance_squared(start.landing.xz()))
                });
                targets.into_iter().map(move |far| (start, far))
            })
            .find_map(|(start, far)| {
                let inward = (far.landing.xz() - far.mooring).normalize_or_zero();
                let entrance_xz = far.landing.xz() + inward * 40.0;
                if terrain
                    .get_water_height(entrance_xz.x, entrance_xz.y)
                    .is_some()
                {
                    return None;
                }
                let entrance = Vec3::new(
                    entrance_xz.x,
                    terrain.get_height(entrance_xz.x, entrance_xz.y),
                    entrance_xz.y,
                );
                overland_trade_corridor_exists(&terrain, start.landing.xz(), entrance.xz())
                    .then_some((*start, entrance))
            })
            .expect("the map must offer two coasts joined by a kilometres-long corridor");

        let choice = SettlementChoice {
            entity: Entity::from_bits(17),
            name: "Far Moot".to_string(),
            position: entrance,
            entrance,
            score: 100.0,
        };
        let mut pending = PendingLandfallSearch::new(&choice, &[start]);

        let mut slices = 0u32;
        let voyage = loop {
            let begin = std::time::Instant::now();
            let result = pending.advance(&terrain);
            let elapsed = begin.elapsed();
            // The budget is a floor-then-deadline (at least 8 expansions, then
            // yield), so a slice can overshoot 0.5ms — but never by orders of
            // magnitude. One quarter second means the deadline is being ignored.
            assert!(
                elapsed < Duration::from_millis(250),
                "one landfall slice ran {elapsed:?}; the per-tick budget has regressed toward the old synchronous freeze"
            );
            slices += 1;
            match result {
                PendingLandfallResult::Pending => {
                    assert!(
                        slices < 1_000_000,
                        "landfall certification never terminated"
                    );
                }
                PendingLandfallResult::Reachable(voyage) => break voyage,
                PendingLandfallResult::Unreachable => {
                    panic!("preconditioned corridor came back unreachable — search parity broke")
                }
            }
        };
        assert_eq!(voyage.start, start.start);
        // A kilometres-long certification costs orders of magnitude more than
        // one 0.5ms slice on any plausible hardware. A synchronous regression
        // finishes in a single call and fails here.
        assert!(
            slices >= 4,
            "landfall certification completed in {slices} slice(s); the incremental search no longer yields"
        );
    }

    #[test]
    fn cached_unreachable_moot_is_skipped_for_another_settlement() {
        let terrain = WorldTerrain::default();
        let approach = coastal_voyages(&terrain, 0)[0];
        let mut app = App::new();
        app.insert_resource(terrain)
            .insert_resource(NaturalImmigrationDirector {
                enabled: true,
                interval_days: interval_days_for_rate(3.0),
                next_arrival_world_seconds: Some(0.0),
                sequence: 0,
                world_npc_cap: DEFAULT_WORLD_NPC_CAP,
                population_cap_announced: false,
                coastal_approaches: vec![approach],
                settlement_landfalls: default(),
                pending_landfall: None,
                pending_water: None,
                manual_arrivals: 0,
                configured: false,
                steady_rate: false,
                deciding: None,
                decision_cursor: None,
                suspended_decisions: default(),
                terrain_revision: None,
            })
            .insert_resource(crate::world::dev::VillagerSeed::default())
            .init_resource::<VesselNavigationQueue>()
            .add_systems(Update, plan_natural_immigration);
        app.world_mut().spawn(WorldTime::new_default());
        let blocked_position = Vec3::ZERO;
        let blocked = app
            .world_mut()
            .spawn((
                settlement("Blocked Moot", 0),
                PlayerPosition(blocked_position),
                PlayerRotation(0.0),
                SettlementEconomy::default(),
            ))
            .id();
        let alternative = app
            .world_mut()
            .spawn((
                settlement("Alternative Moot", 0),
                PlayerPosition(Vec3::new(300.0, 0.0, 300.0)),
                PlayerRotation(0.0),
                SettlementEconomy::default(),
            ))
            .id();
        let blocked_entrance =
            SettlementBuildingKind::Hall.entrance_position(blocked_position, 0.0);
        app.world_mut()
            .resource_mut::<NaturalImmigrationDirector>()
            .settlement_landfalls
            .insert(
                blocked,
                CachedSettlementLandfall::Unreachable {
                    entrance: blocked_entrance,
                },
            );

        app.update(); // Actual entry, with no preselected town.
        app.update(); // Choose from current reachable opportunities.

        let director = app.world().resource::<NaturalImmigrationDirector>();
        assert!(matches!(
            director.settlement_landfalls.get(&blocked),
            Some(CachedSettlementLandfall::Unreachable { .. })
        ));
        assert_eq!(
            director
                .pending_landfall
                .as_ref()
                .map(|pending| pending.choice.entity),
            Some(alternative)
        );
    }

    #[test]
    fn all_candidate_failure_is_cached_across_later_arrival_attempts() {
        let terrain = WorldTerrain::default();
        let candidates = (0..24)
            .map(|index| {
                let outside = 100_000.0 + index as f32 * 10.0;
                CoastalVoyage {
                    start: Vec3::new(outside, 0.0, outside),
                    yaw: 0.0,
                    mooring: Vec2::new(outside, outside),
                    landing: Vec3::new(outside, 0.0, outside),
                }
            })
            .collect::<Vec<_>>();
        let entry = coastal_voyages(&terrain, 0)[0];
        let mut app = App::new();
        app.insert_resource(terrain)
            .insert_resource(NaturalImmigrationDirector {
                enabled: true,
                interval_days: interval_days_for_rate(3.0),
                next_arrival_world_seconds: Some(0.0),
                sequence: 0,
                world_npc_cap: DEFAULT_WORLD_NPC_CAP,
                population_cap_announced: false,
                coastal_approaches: vec![entry],
                settlement_landfalls: default(),
                pending_landfall: None,
                pending_water: None,
                manual_arrivals: 0,
                configured: false,
                steady_rate: false,
                deciding: None,
                decision_cursor: None,
                suspended_decisions: default(),
                terrain_revision: None,
            })
            .insert_resource(crate::world::dev::VillagerSeed::default())
            .init_resource::<VesselNavigationQueue>()
            .add_systems(Update, plan_natural_immigration);
        let clock = app.world_mut().spawn(WorldTime::new_default()).id();
        let position = Vec3::ZERO;
        let settlement = app
            .world_mut()
            .spawn((
                settlement("No Coast", 0),
                PlayerPosition(position),
                PlayerRotation(0.0),
                SettlementEconomy::default(),
            ))
            .id();

        app.update(); // Real entry is a separate stage from the deliberately invalid survey.
        app.world_mut()
            .resource_mut::<NaturalImmigrationDirector>()
            .coastal_approaches = candidates;
        for _ in 0..64 {
            app.update();
            let cached = app
                .world()
                .resource::<NaturalImmigrationDirector>()
                .settlement_landfalls
                .get(&settlement)
                .copied();
            if matches!(cached, Some(CachedSettlementLandfall::Unreachable { .. })) {
                break;
            }
        }
        {
            let director = app.world().resource::<NaturalImmigrationDirector>();
            assert!(matches!(
                director.settlement_landfalls.get(&settlement),
                Some(CachedSettlementLandfall::Unreachable { .. })
            ));
            assert!(director.pending_landfall.is_none());
        }

        app.world_mut()
            .get_mut::<WorldTime>(clock)
            .expect("test clock disappeared")
            .day += 1;
        app.update();

        let director = app.world().resource::<NaturalImmigrationDirector>();
        assert!(director.pending_landfall.is_none());
        assert!(matches!(
            director.settlement_landfalls.get(&settlement),
            Some(CachedSettlementLandfall::Unreachable { .. })
        ));
    }

    #[test]
    fn natural_immigration_stops_at_the_world_npc_cap() {
        let mut app = App::new();
        app.insert_resource(WorldTerrain::default())
            .insert_resource(NaturalImmigrationDirector {
                enabled: true,
                interval_days: interval_days_for_rate(3.0),
                next_arrival_world_seconds: Some(0.0),
                sequence: 0,
                world_npc_cap: 1,
                population_cap_announced: false,
                coastal_approaches: Vec::new(),
                settlement_landfalls: default(),
                pending_landfall: None,
                pending_water: None,
                manual_arrivals: 0,
                configured: false,
                steady_rate: false,
                deciding: None,
                decision_cursor: None,
                suspended_decisions: default(),
                terrain_revision: None,
            })
            .insert_resource(crate::world::dev::VillagerSeed::default())
            .init_resource::<VesselNavigationQueue>()
            .add_systems(Update, plan_natural_immigration);
        app.world_mut().spawn(WorldTime::new_default());
        app.world_mut().spawn((
            settlement("Cap Test", 1),
            PlayerPosition(Vec3::ZERO),
            PlayerRotation(0.0),
            SettlementEconomy::default(),
        ));
        app.world_mut().spawn(CharacterKind::Villager);

        app.update();

        let arrivals = app
            .world_mut()
            .query_filtered::<Entity, With<NpcArrivalBoat>>()
            .iter(app.world())
            .count();
        assert_eq!(arrivals, 0);
        let director = app.world().resource::<NaturalImmigrationDirector>();
        assert!(director.population_cap_announced);
        assert!(
            director
                .next_arrival_world_seconds
                .is_some_and(|next| next > 0.0)
        );
    }

    #[test]
    fn distance_is_a_bias_rather_than_an_absolute_rule() {
        let entity = Entity::from_bits(2);
        let economy = SettlementEconomy {
            reserve_days: 3.0,
            recent_food_production: 12.0,
            housing_capacity: 14,
            private_vacant_jobs: 2,
            prosperity: 50.0,
            ..default()
        };
        let near = settlement_attractiveness(
            &settlement("Near", 10),
            Some(&economy),
            4,
            entity,
            Vec3::ZERO,
            Vec3::new(100.0, 0.0, 0.0),
        );
        let far = settlement_attractiveness(
            &settlement("Far", 10),
            Some(&economy),
            4,
            entity,
            Vec3::ZERO,
            Vec3::new(3_000.0, 0.0, 0.0),
        );
        assert!(near > far);
        assert!(near - far < 30.0, "distance became a hard prohibition");
    }

    #[test]
    fn manual_arrival_sails_disembarks_and_completes_land_route_while_recurring_is_off() {
        let terrain = WorldTerrain::default();
        assert!(
            matches!(
                terrain.generator.active_map_id(),
                "big_world" | "village_lab"
            ),
            "physical arrival regression requires a shipped playable map, got '{}'",
            terrain.generator.active_map_id()
        );

        // Find a genuine map coast whose inland terrain can host a Moot
        // and whose door has a certified dry embodied route from the beach.
        let mut coastal_approaches = coastal_voyages(&terrain, 0);
        assert!(
            !coastal_approaches.is_empty(),
            "the active map must expose an ocean-connected arrival coast"
        );
        let (site_coast, hall_position, hall_rotation, hall_entrance) = coastal_approaches
            .iter()
            .find_map(|approach| {
                let inward = (approach.landing.xz() - approach.mooring).normalize_or_zero();
                (20..=120).step_by(4).find_map(|distance| {
                    let centre_xz = approach.landing.xz() + inward * distance as f32;
                    if terrain.get_water_height(centre_xz.x, centre_xz.y).is_some() {
                        return None;
                    }
                    // Face the Hall back toward the beach, keeping its
                    // canonical door on the immigrant's side of the plot.
                    let toward_beach = -inward;
                    let rotation = f32::atan2(-toward_beach.x, -toward_beach.y);
                    let centre = Vec3::new(
                        centre_xz.x,
                        terrain.get_height(centre_xz.x, centre_xz.y),
                        centre_xz.y,
                    );
                    let entrance = SettlementBuildingKind::Hall.entrance_position(centre, rotation);
                    (terrain.get_water_height(entrance.x, entrance.z).is_none()
                        && embodied_land_route_exists(&terrain, approach.landing, entrance))
                    .then_some((*approach, centre, rotation, entrance))
                })
            })
            .expect("big_world arrival beach must connect to a nearby dry Hall site");
        // Make the director's deterministic first entry use this same ocean
        // approach. Other tests cover entry/settlement distance variation;
        // this one isolates the complete physical lifecycle.
        let entry_index = mixed(1) as usize % coastal_approaches.len();
        let site_index = coastal_approaches
            .iter()
            .position(|approach| approach.start == site_coast.start)
            .unwrap();
        coastal_approaches.swap(entry_index, site_index);

        let mut app = App::new();
        app.insert_resource(terrain)
            .insert_resource(NaturalImmigrationDirector {
                enabled: false,
                interval_days: interval_days_for_rate(DEFAULT_IMMIGRANTS_PER_DAY),
                next_arrival_world_seconds: None,
                sequence: 0,
                world_npc_cap: DEFAULT_WORLD_NPC_CAP,
                population_cap_announced: false,
                coastal_approaches,
                settlement_landfalls: default(),
                pending_landfall: None,
                pending_water: None,
                manual_arrivals: 1,
                configured: false,
                steady_rate: false,
                deciding: None,
                decision_cursor: None,
                suspended_decisions: default(),
                terrain_revision: None,
            })
            .insert_resource(crate::world::dev::VillagerSeed::default())
            .init_resource::<SimulationDelta>()
            .init_resource::<VesselNavigationQueue>()
            .add_systems(Update, plan_natural_immigration);
        app.world_mut()
            .spawn((WorldTime::new_default(), TimeWarp(1.0)));
        let settlement_entity = app
            .world_mut()
            .spawn((
                SettlementId(1),
                Settlement {
                    name: "Arrival Test".to_string(),
                    tier: SettlementTier::Hamlet,
                    residents: 0,
                    treasury: 0,
                },
                PlayerPosition(hall_position),
                PlayerRotation(hall_rotation),
                SettlementEconomy::default(),
            ))
            .id();

        let mut arrival_created = false;
        for _ in 0..2_000 {
            app.update();
            let count = app
                .world_mut()
                .query_filtered::<Entity, With<NpcArrivalBoat>>()
                .iter(app.world())
                .count();
            if count == 1 {
                arrival_created = true;
                break;
            }
        }
        assert!(
            arrival_created,
            "bounded landfall discovery never admitted the known-reachable arrival"
        );
        let director = app.world().resource::<NaturalImmigrationDirector>();
        assert_eq!(director.manual_arrivals, 0);
        assert!(
            director.next_arrival_world_seconds.is_none(),
            "the one-shot lab request disturbed recurring immigration time"
        );

        let (boat, passenger, route_start, route_points, expected_mooring, expected_landing) = {
            let mut boats = app.world_mut().query_filtered::<(
                    Entity,
                    &NpcArrivalBoat,
                    &PlayerPosition,
                    &VesselRoute,
                ), With<PlayerBoat>>();
            let (boat, arrival, position, route) = boats
                .single(app.world())
                .expect("the due arrival must create exactly one immigrant dinghy");
            assert!(app.world().entity(boat).contains::<ImmigrantArrivalBoat>());
            assert_eq!(arrival.settlement, settlement_entity);
            (
                boat,
                arrival.passenger,
                position.0.xz(),
                route.waypoints.clone(),
                arrival.mooring,
                arrival.landing,
            )
        };
        assert!(
            app.world()
                .resource::<WorldTerrain>()
                .get_water_height(route_start.x, route_start.y)
                .is_some(),
            "the dinghy spawned off the water"
        );
        let terrain = app.world().resource::<WorldTerrain>();
        let mut segment_start = route_start;
        for waypoint in &route_points {
            let length = segment_start.distance(*waypoint);
            let samples = (length / 1.5).ceil().max(1.0) as usize;
            for sample in 0..=samples {
                let point = segment_start.lerp(*waypoint, sample as f32 / samples as f32);
                assert!(
                    terrain
                        .generator
                        .active_map_bounds()
                        .contains_xz(point.x, point.y),
                    "arrival route escaped its own terrain bounds"
                );
                assert!(
                    terrain.get_water_height(point.x, point.y).is_some(),
                    "certified boat route crossed dry land at {point:?}"
                );
            }
            segment_start = *waypoint;
        }
        assert!(
            terrain
                .get_water_height(expected_landing.x, expected_landing.z)
                .is_none(),
            "the selected disembark point was not dry"
        );
        assert!(
            overland_trade_corridor_exists(terrain, expected_landing.xz(), hall_entrance.xz()),
            "the selected coast did not share a land corridor with the Hall"
        );
        assert!(
            site_coast.landing.xz().distance(expected_landing.xz()) < 100.0,
            "the director ignored the viable coast beside its chosen settlement"
        );

        // Losing a route must trigger bounded replanning, retaining this same
        // body/boat and the certified beach rather than stranding the arrival.
        app.world_mut().entity_mut(boat).remove::<VesselRoute>();
        app.add_systems(
            Update,
            (
                crate::player::boat::plan_vessel_routes,
                crate::player::boat::step_boats,
                sync_natural_immigrant_passengers,
                finish_natural_immigrant_voyages,
            )
                .chain(),
        );
        for _ in 0..2_000 {
            app.update();
            if app.world().get_entity(boat).is_err() {
                break;
            }
        }
        assert!(
            app.world().get_entity(boat).is_err(),
            "the immigrant dinghy never reached its mooring at {expected_mooring:?}"
        );
        let landed = app
            .world()
            .get::<PlayerPosition>(passenger)
            .expect("the immigrant disappeared with the boat")
            .0;
        assert!(
            app.world()
                .resource::<WorldTerrain>()
                .get_water_height(landed.x, landed.z)
                .is_none(),
            "the immigrant disembarked in water at {landed:?}"
        );
        assert!(!app.world().entity(passenger).contains::<AboardBoat>());
        assert!(matches!(
            app.world().get::<VillagerIntent>(passenger),
            Some(VillagerIntent::Travelling { settlement }) if *settlement == settlement_entity
        ));
        assert_eq!(
            app.world()
                .get::<MoveTarget>(passenger)
                .map(|target| target.0),
            Some(hall_entrance)
        );

        // Run the ordinary villager planner and embodied movement, rather
        // than treating creation of a MoveTarget as proof of pathfinding.
        app.init_resource::<VillageRoadGraph>()
            .insert_resource(PathfindingBudgetSettings {
                max_requests_per_tick: 1,
                max_milliseconds_per_tick: 50.0,
            })
            .add_systems(
                Update,
                (
                    crate::world::village_roads::queue_villager_travel_routes,
                    crate::world::village_roads::plan_villager_travel_routes,
                    crate::player::hero::step_units,
                )
                    .chain(),
            );
        for _ in 0..600 {
            app.update();
            assert!(
                app.world()
                    .get::<NavigationRouteFailed>(passenger)
                    .is_none(),
                "the normal land planner rejected the arrival walk"
            );
            // A rejected corrupt/out-of-bounds order also removes its target.
            // Only absence without failure is a successful arrival signal.
            if app.world().get::<MoveTarget>(passenger).is_none() {
                break;
            }
        }
        let final_position = app.world().get::<PlayerPosition>(passenger).unwrap().0;
        assert!(
            app.world().get::<MoveTarget>(passenger).is_none(),
            "the immigrant remained stranded after disembarking; position={final_position:?} route={:?}",
            app.world().get::<TravelRoute>(passenger)
        );
        assert!(
            final_position.xz().distance(hall_entrance.xz()) <= 0.3,
            "the immigrant stopped away from the Moot Hall: {final_position:?} vs {hall_entrance:?}"
        );
    }
}
