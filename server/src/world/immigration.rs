//! Natural, physical arrivals from outside the playable world.
//!
//! Newcomers decide which settlement looks promising before they appear. They
//! enter on an ocean-connected dinghy, sail to the coast nearest that choice,
//! abandon the temporary boat at landfall, and then use the ordinary migration
//! route and visible Moot Hall queue. The director is deliberately small and
//! demand-sensitive: seasons can change cadence later without putting a timer
//! or a decision tree on every NPC.

use bevy::prelude::*;
use lightyear::prelude::{NetworkTarget, Replicate};

use shared::components::{
    AboardBoat, CharacterActivity, CharacterKind, CharacterMotion, CharacterObjective,
    ImmigrantArrivalBoat, PlayerBoat, PlayerPosition, PlayerRotation, Settlement,
    SettlementBuildingKind, Vessel, WorldTime,
};
use shared::economy::SettlementEconomy;
use shared::region::RegionCoord;
use shared::terrain::WorldTerrain;

use crate::player::boat::{
    coastal_voyages, water_route, CoastalVoyage, VesselNavigation, VesselRoute,
    BOAT_ARRIVE_EPSILON, HELM_LOCAL,
};
use crate::world::village::VillagerIntent;
#[cfg(test)]
use crate::world::village_roads::overland_trade_corridor_exists;
use crate::world::village_roads::{IncrementalCorridorResult, IncrementalOverlandCorridorSearch};
use std::time::Duration;

const DEFAULT_IMMIGRANTS_PER_DAY: f32 = 3.0;
const DEFAULT_WORLD_NPC_CAP: usize = 5_000;
const MIN_IMMIGRANTS_PER_DAY: f32 = 0.1;
const MAX_IMMIGRANTS_PER_DAY: f32 = 20.0;
const FIRST_ARRIVAL_DELAY_DAYS: f32 = 0.06;
const RETRY_DELAY_DAYS: f32 = 0.08;
const MIN_ATTRACTIVENESS: f32 = 18.0;
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
    settlement: Entity,
    settlement_name: String,
    entrance: Vec3,
    decision_seed: u64,
    manual: bool,
    candidates: Vec<CoastalVoyage>,
    candidate_index: usize,
    corridor: Option<IncrementalOverlandCorridorSearch>,
}

#[derive(Clone, Copy, Debug)]
enum PendingLandfallResult {
    Pending,
    Reachable(CoastalVoyage),
    Unreachable,
}

impl PendingLandfallSearch {
    fn new(
        choice: &SettlementChoice,
        decision_seed: u64,
        manual: bool,
        coastal_approaches: &[CoastalVoyage],
    ) -> Self {
        let mut candidates = coastal_approaches.to_vec();
        candidates.sort_by(|a, b| {
            a.landing
                .xz()
                .distance_squared(choice.entrance.xz())
                .total_cmp(&b.landing.xz().distance_squared(choice.entrance.xz()))
        });
        Self {
            settlement: choice.entity,
            settlement_name: choice.name.clone(),
            entrance: choice.entrance,
            decision_seed,
            manual,
            candidates,
            candidate_index: 0,
            corridor: None,
        }
    }

    fn advance(&mut self, terrain: &WorldTerrain) -> PendingLandfallResult {
        let Some(candidate) = self.candidates.get(self.candidate_index).copied() else {
            return PendingLandfallResult::Unreachable;
        };
        let corridor = self.corridor.get_or_insert_with(|| {
            IncrementalOverlandCorridorSearch::new(candidate.landing.xz(), self.entrance.xz())
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
    /// Explicit lab requests. They use the normal physical arrival pipeline
    /// but do not enable recurring immigration or disturb its calendar.
    manual_arrivals: u8,
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
            manual_arrivals: 0,
        }
    }
}

impl NaturalImmigrationDirector {
    /// Queue one complete God-mode arrival. Bounded so repeated UI clicks
    /// cannot create an unbounded pathfinding backlog.
    pub fn request_manual_arrival(&mut self) -> bool {
        if self.manual_arrivals >= MAX_QUEUED_MANUAL_ARRIVALS {
            return false;
        }
        self.manual_arrivals += 1;
        true
    }

    fn finish_manual_arrival(&mut self) {
        self.manual_arrivals = self.manual_arrivals.saturating_sub(1);
    }
}

fn interval_days_for_rate(immigrants_per_day: f32) -> f32 {
    1.0 / immigrants_per_day.clamp(MIN_IMMIGRANTS_PER_DAY, MAX_IMMIGRANTS_PER_DAY)
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

#[derive(Component, Debug, Clone, Copy)]
pub struct NaturalImmigrantVoyage {
    boat: Entity,
    settlement: Entity,
}

#[derive(Component, Debug, Clone, Copy)]
pub struct NpcArrivalBoat {
    passenger: Entity,
    settlement: Entity,
    mooring: Vec2,
    landing: Vec3,
}

#[derive(Clone, Debug)]
struct SettlementChoice {
    entity: Entity,
    name: String,
    position: Vec3,
    entrance: Vec3,
    score: f32,
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

/// Admit at most one new voyage per pass, even after a large time jump. Time
/// warp therefore advances the calendar without turning one server tick into a
/// burst of coastline searches, entity spawns, or later route requests.
pub fn plan_natural_immigration(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    clock: Query<&WorldTime>,
    settlements: Query<(
        Entity,
        &Settlement,
        &PlayerPosition,
        Option<&PlayerRotation>,
        Option<&SettlementEconomy>,
    )>,
    active: Query<(), With<NpcArrivalBoat>>,
    people: Query<&CharacterKind>,
    mut director: ResMut<NaturalImmigrationDirector>,
    mut villager_seed: ResMut<crate::world::dev::VillagerSeed>,
) {
    // God-mode requests are deliberately one-shot. In the Village Lab this
    // keeps recurring immigration disabled for deterministic scenarios, while
    // every map still exercises the exact production pipeline on demand.
    if !director.enabled
        && director.manual_arrivals == 0
        && director
            .pending_landfall
            .as_ref()
            .is_none_or(|pending| !pending.manual)
    {
        return;
    }
    let Some(clock) = clock.iter().next() else {
        return;
    };
    let now = absolute_world_seconds(clock);
    let cycle = f64::from(clock.cycle_duration());

    // A server may run for hours before anyone founds a Moot. Keep the
    // director entirely dormant in that state and discard any obsolete due
    // time so the first settlement receives the ordinary initial delay rather
    // than a burst of accumulated arrivals.
    if settlements.is_empty() {
        director.next_arrival_world_seconds = None;
        director.pending_landfall = None;
        director.settlement_landfalls.clear();
        director.manual_arrivals = 0;
        return;
    }
    director
        .settlement_landfalls
        .retain(|entity, _| settlements.get(*entity).is_ok());

    let mut resumed_request = None;
    if let Some(mut pending) = director.pending_landfall.take() {
        let pending_still_matches =
            settlements
                .get(pending.settlement)
                .is_ok_and(|(_, _, position, rotation, _)| {
                    let entrance = SettlementBuildingKind::Hall
                        .entrance_position(position.0, rotation.map_or(0.0, |rotation| rotation.0));
                    entrance.distance_squared(pending.entrance) <= LANDFALL_ENTRANCE_EPSILON_SQ
                });
        if pending_still_matches {
            match pending.advance(&terrain) {
                PendingLandfallResult::Pending => {
                    director.pending_landfall = Some(pending);
                    return;
                }
                PendingLandfallResult::Reachable(voyage) => {
                    info!(
                        "Natural immigration certified a landfall for '{}' after checking {} coastal approach(es)",
                        pending.settlement_name,
                        pending.candidate_index + 1
                    );
                    director.settlement_landfalls.insert(
                        pending.settlement,
                        CachedSettlementLandfall::Reachable {
                            entrance: pending.entrance,
                            voyage,
                        },
                    );
                    resumed_request = Some((pending.decision_seed, pending.manual));
                }
                PendingLandfallResult::Unreachable => {
                    warn!(
                        "Natural immigration found no walkable coastal approach toward '{}'; caching the terrain failure",
                        pending.settlement_name
                    );
                    director.settlement_landfalls.insert(
                        pending.settlement,
                        CachedSettlementLandfall::Unreachable {
                            entrance: pending.entrance,
                        },
                    );
                    if pending.manual {
                        director.finish_manual_arrival();
                    } else {
                        director.next_arrival_world_seconds =
                            Some(now + cycle * f64::from(RETRY_DELAY_DAYS));
                    }
                    return;
                }
            }
        } else {
            director.settlement_landfalls.remove(&pending.settlement);
        }
    }

    if active.iter().count() >= MAX_ACTIVE_VOYAGES {
        return;
    }
    let manual = resumed_request
        .map(|(_, manual)| manual)
        .unwrap_or(director.manual_arrivals > 0);
    if !manual {
        if !director.enabled {
            return;
        }
        if director.next_arrival_world_seconds.is_none() {
            director.next_arrival_world_seconds =
                Some(now + cycle * f64::from(FIRST_ARRIVAL_DELAY_DAYS));
            return;
        }
        if now < director.next_arrival_world_seconds.unwrap_or(f64::INFINITY) {
            return;
        }
    }

    // This full count happens only when an arrival is due (three times per
    // world day by default), never once per server tick. Strategic residents
    // retain CharacterKind, so this is a true world total across tactical and
    // cheap off-screen people. Player heroes deliberately do not consume the
    // NPC population budget.
    let world_npc_count = people
        .iter()
        .filter(|kind| **kind == CharacterKind::Villager)
        .count();
    if world_npc_count >= director.world_npc_cap {
        if !director.population_cap_announced {
            info!(
                "Natural immigration paused at world NPC cap ({world_npc_count}/{})",
                director.world_npc_cap
            );
            director.population_cap_announced = true;
        }
        if manual {
            director.finish_manual_arrival();
        } else {
            director.next_arrival_world_seconds =
                Some(now + cycle * f64::from(director.interval_days));
        }
        return;
    }
    if director.population_cap_announced {
        info!(
            "Natural immigration resumed below world NPC cap ({world_npc_count}/{})",
            director.world_npc_cap
        );
        director.population_cap_announced = false;
    }

    let decision_seed = resumed_request.map(|(seed, _)| seed).unwrap_or_else(|| {
        director.sequence = director.sequence.wrapping_add(1);
        director.sequence ^ (u64::from(clock.day) << 32)
    });
    if director.coastal_approaches.is_empty() {
        // Shore discovery scans the authored map edge. Cache that immutable
        // geography once so recurring arrivals never turn it into a periodic
        // hitch, especially when the calendar runs at 25x or 100x.
        director.coastal_approaches = coastal_voyages(&terrain, 0);
    }
    // Pick where this person enters the world before they judge towns. That
    // makes "a northerner heard about the nearby Moot" a real geographic fact.
    let Some(entry) = (!director.coastal_approaches.is_empty()).then(|| {
        let index = mixed(decision_seed) as usize % director.coastal_approaches.len();
        director.coastal_approaches[index]
    }) else {
        warn!("Natural immigrant could not find an ocean-connected entry coast");
        if manual {
            director.finish_manual_arrival();
        } else {
            director.next_arrival_world_seconds = Some(now + cycle * f64::from(RETRY_DELAY_DAYS));
        }
        return;
    };
    let choice = settlements
        .iter()
        .filter_map(|(entity, settlement, position, rotation, economy)| {
            let entrance = SettlementBuildingKind::Hall
                .entrance_position(position.0, rotation.map_or(0.0, |rotation| rotation.0));
            let cached = director
                .settlement_landfalls
                .get(&entity)
                .copied()
                .filter(|cached| cached.matches(entrance));
            if matches!(cached, Some(CachedSettlementLandfall::Unreachable { .. })) {
                return None;
            }
            Some(SettlementChoice {
                entity,
                name: settlement.name.clone(),
                position: position.0,
                entrance,
                score: settlement_attractiveness(
                    settlement,
                    economy,
                    decision_seed,
                    entity,
                    entry.landing,
                    position.0,
                ),
            })
        })
        .max_by(|a, b| a.score.total_cmp(&b.score));
    // A manual lab launch must remain observable even when its only staged
    // settlement is intentionally food-poor. It still picks the best real
    // settlement and uses all route validation; only the migration-interest
    // threshold is bypassed. Recurring world immigration keeps the threshold.
    let Some(choice) = choice.filter(|choice| manual || choice.score >= MIN_ATTRACTIVENESS) else {
        if manual {
            director.finish_manual_arrival();
        } else {
            director.next_arrival_world_seconds = Some(now + cycle * f64::from(RETRY_DELAY_DAYS));
        }
        return;
    };
    let cached_landfall = director
        .settlement_landfalls
        .get(&choice.entity)
        .copied()
        .filter(|cached| cached.matches(choice.entrance))
        .and_then(|cached| match cached {
            CachedSettlementLandfall::Reachable { voyage, .. } => Some(voyage),
            CachedSettlementLandfall::Unreachable { .. } => None,
        });
    let Some(voyage) = cached_landfall else {
        director.pending_landfall = Some(PendingLandfallSearch::new(
            &choice,
            decision_seed,
            manual,
            &director.coastal_approaches,
        ));
        return;
    };
    let Some(route) = water_route(&terrain, entry.start.xz(), voyage.mooring) else {
        // Never fall back to an arbitrary entry beach: it may be a dry shelf
        // below a cliff or belong to a different landmass. Waiting for another
        // entry is preferable to creating a permanently stranded person.
        warn!(
            "Natural immigrant could not find a water route toward '{}'",
            choice.name
        );
        if manual {
            director.finish_manual_arrival();
        } else {
            director.next_arrival_world_seconds = Some(now + cycle * f64::from(RETRY_DELAY_DAYS));
        }
        return;
    };
    let direction = route
        .first()
        .copied()
        .map_or(Vec2::ZERO, |waypoint| waypoint - entry.start.xz())
        .normalize_or_zero();
    let initial_yaw = if direction == Vec2::ZERO {
        entry.yaw
    } else {
        f32::atan2(-direction.x, -direction.y)
    };

    villager_seed.0 = villager_seed.0.wrapping_add(1);
    let passenger_position = entry.start + Quat::from_rotation_y(initial_yaw) * HELM_LOCAL;
    let passenger = crate::player::hero::spawn_villager(
        &mut commands,
        &terrain,
        villager_seed.0,
        passenger_position,
    );
    let boat = commands
        .spawn((
            PlayerBoat,
            ImmigrantArrivalBoat,
            Vessel,
            VesselNavigation::DINGHY,
            NpcArrivalBoat {
                passenger,
                settlement: choice.entity,
                mooring: voyage.mooring,
                landing: voyage.landing,
            },
            VesselRoute {
                waypoints: route,
                next: 0,
            },
            PlayerPosition(entry.start),
            PlayerRotation(initial_yaw),
            CharacterMotion::STATIONARY,
            RegionCoord::from_world_pos(entry.start),
            Replicate::to_clients(NetworkTarget::All),
        ))
        .id();
    commands.entity(passenger).insert((
        AboardBoat,
        NaturalImmigrantVoyage {
            boat,
            settlement: choice.entity,
        },
        VillagerIntent::ArrivingBySea {
            settlement: choice.entity,
        },
        CharacterActivity::Sitting,
        CharacterObjective::SailingToSettlement,
        PlayerPosition(passenger_position),
        PlayerRotation(initial_yaw),
        RegionCoord::from_world_pos(passenger_position),
    ));
    info!(
        "Natural immigrant {} entered {:.0}m from '{}' and chose it at {:.1} attractiveness (landing walk {:.0}m)",
        villager_seed.0,
        entry.landing.xz().distance(choice.position.xz()),
        choice.name,
        choice.score,
        voyage.landing.xz().distance(choice.position.xz())
    );

    if manual {
        director.finish_manual_arrival();
    } else {
        let preference = (mixed(decision_seed ^ 0x53a9) & 0xffff) as f32 / u16::MAX as f32;
        let jitter = 0.85 + preference * 0.30;
        let interval = director.interval_days
            * seasonal_interval_multiplier(clock.day)
            * jitter
            * if choice.score >= 65.0 { 0.8 } else { 1.0 };
        director.next_arrival_world_seconds = Some(now + cycle * f64::from(interval));
    }
}

/// Keep each NPC passenger at the helm of its own authoritative boat. Client
/// rendering adds the same local buoyancy polish used for the player voyage.
pub fn sync_natural_immigrant_passengers(
    boats: Query<(&PlayerPosition, &PlayerRotation, &CharacterMotion), With<NpcArrivalBoat>>,
    mut passengers: Query<
        (
            &NaturalImmigrantVoyage,
            &mut PlayerPosition,
            &mut PlayerRotation,
            &mut RegionCoord,
            &mut CharacterMotion,
            &mut CharacterActivity,
        ),
        Without<NpcArrivalBoat>,
    >,
) {
    for (voyage, mut position, mut rotation, mut region, mut motion, mut activity) in
        passengers.iter_mut()
    {
        let Ok((boat_position, boat_rotation, boat_motion)) = boats.get(voyage.boat) else {
            continue;
        };
        let helm = boat_position.0 + Quat::from_rotation_y(boat_rotation.0) * HELM_LOCAL;
        position.0 = helm;
        rotation.0 = boat_rotation.0;
        *region = RegionCoord::from_world_pos(helm);
        *motion = *boat_motion;
        *activity = CharacterActivity::Sitting;
    }
}

/// The temporary immigrant dinghy disappears at shore. The person is placed
/// on validated dry ground and immediately enters the existing land migration
/// and Moot Hall registration flow; no second resident-creation path exists.
pub fn finish_natural_immigrant_voyages(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    boats: Query<
        (
            Entity,
            &NpcArrivalBoat,
            &PlayerPosition,
            Option<&VesselRoute>,
        ),
        With<PlayerBoat>,
    >,
    halls: Query<(&PlayerPosition, Option<&PlayerRotation>), With<Settlement>>,
    mut passengers: Query<
        (
            &mut PlayerPosition,
            &mut PlayerRotation,
            &mut RegionCoord,
            &mut CharacterMotion,
            &mut CharacterActivity,
            &mut VillagerIntent,
            &NaturalImmigrantVoyage,
        ),
        (Without<PlayerBoat>, Without<Settlement>),
    >,
) {
    for (boat_entity, arrival, boat_position, route) in boats.iter() {
        if route.is_some()
            || boat_position.0.xz().distance(arrival.mooring) > BOAT_ARRIVE_EPSILON + 0.5
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
        )) = passengers.get_mut(arrival.passenger)
        else {
            commands.entity(boat_entity).despawn();
            continue;
        };
        debug_assert_eq!(voyage.boat, boat_entity);
        debug_assert_eq!(voyage.settlement, arrival.settlement);
        let landing = Vec3::new(
            arrival.landing.x,
            terrain.get_height(arrival.landing.x, arrival.landing.z),
            arrival.landing.z,
        );
        position.0 = landing;
        *region = RegionCoord::from_world_pos(landing);
        *motion = CharacterMotion::STATIONARY;
        *activity = CharacterActivity::Idle;

        if let Ok((hall, hall_rotation)) = halls.get(arrival.settlement) {
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
        embodied_land_route_exists, NavigationRouteFailed, TravelRoute, VillageRoadGraph,
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
        assert!(good > MIN_ATTRACTIVENESS);
        assert!(bad < MIN_ATTRACTIVENESS);
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
        assert!(score >= MIN_ATTRACTIVENESS, "frontier score was {score}");
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
    fn manual_arrival_queue_is_bounded() {
        let mut director = NaturalImmigrationDirector::default();
        for _ in 0..MAX_QUEUED_MANUAL_ARRIVALS {
            assert!(director.request_manual_arrival());
        }
        assert!(!director.request_manual_arrival());
        assert_eq!(director.manual_arrivals, MAX_QUEUED_MANUAL_ARRIVALS);
    }

    #[test]
    fn server_without_a_settlement_accumulates_no_arrival_backlog() {
        let mut app = App::new();
        app.insert_resource(WorldTerrain::default())
            .insert_resource(NaturalImmigrationDirector {
                enabled: true,
                interval_days: interval_days_for_rate(3.0),
                next_arrival_world_seconds: Some(0.0),
                sequence: 0,
                world_npc_cap: DEFAULT_WORLD_NPC_CAP,
                population_cap_announced: false,
                coastal_approaches: Vec::new(),
                settlement_landfalls: default(),
                pending_landfall: None,
                manual_arrivals: 0,
            })
            .insert_resource(crate::world::dev::VillagerSeed::default())
            .add_systems(Update, plan_natural_immigration);
        app.world_mut().spawn(WorldTime::new_default());

        for _ in 0..10 {
            app.update();
        }
        {
            let director = app.world().resource::<NaturalImmigrationDirector>();
            assert_eq!(director.sequence, 0);
            assert!(director.next_arrival_world_seconds.is_none());
            assert!(director.pending_landfall.is_none());
        }

        app.world_mut().spawn((
            settlement("First Moot", 0),
            PlayerPosition(Vec3::ZERO),
            PlayerRotation(0.0),
            SettlementEconomy::default(),
        ));
        app.update();

        let director = app.world().resource::<NaturalImmigrationDirector>();
        assert_eq!(director.sequence, 0);
        assert!(director
            .next_arrival_world_seconds
            .is_some_and(|next| next > 0.0));
        assert!(director.pending_landfall.is_none());
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
        let mut pending = PendingLandfallSearch::new(&choice, 1, false, &candidates);

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
                    .filter(|far| {
                        far.landing.xz().distance(start.landing.xz()) >= 1_200.0
                    })
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
        let mut pending = PendingLandfallSearch::new(&choice, 1, false, &[start]);

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
                    assert!(slices < 1_000_000, "landfall certification never terminated");
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
        let approach = CoastalVoyage {
            start: Vec3::new(-100.0, 0.0, -100.0),
            yaw: 0.0,
            mooring: Vec2::new(-90.0, -90.0),
            landing: Vec3::new(-80.0, 0.0, -80.0),
        };
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
                manual_arrivals: 0,
            })
            .insert_resource(crate::world::dev::VillagerSeed::default())
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

        app.update();

        let director = app.world().resource::<NaturalImmigrationDirector>();
        assert!(matches!(
            director.settlement_landfalls.get(&blocked),
            Some(CachedSettlementLandfall::Unreachable { .. })
        ));
        assert_eq!(
            director
                .pending_landfall
                .as_ref()
                .map(|pending| pending.settlement),
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
        let mut app = App::new();
        app.insert_resource(terrain)
            .insert_resource(NaturalImmigrationDirector {
                enabled: true,
                interval_days: interval_days_for_rate(3.0),
                next_arrival_world_seconds: Some(0.0),
                sequence: 0,
                world_npc_cap: DEFAULT_WORLD_NPC_CAP,
                population_cap_announced: false,
                coastal_approaches: candidates,
                settlement_landfalls: default(),
                pending_landfall: None,
                manual_arrivals: 0,
            })
            .insert_resource(crate::world::dev::VillagerSeed::default())
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
                manual_arrivals: 0,
            })
            .insert_resource(crate::world::dev::VillagerSeed::default())
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
        assert!(director
            .next_arrival_world_seconds
            .is_some_and(|next| next > 0.0));
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
                manual_arrivals: 1,
            })
            .insert_resource(crate::world::dev::VillagerSeed::default())
            .init_resource::<SimulationDelta>()
            .add_systems(Update, plan_natural_immigration);
        app.world_mut()
            .spawn((WorldTime::new_default(), TimeWarp(1.0)));
        let settlement_entity = app
            .world_mut()
            .spawn((
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

        app.add_systems(
            Update,
            (
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
            if app.world().get::<MoveTarget>(passenger).is_none() {
                break;
            }
            assert!(
                app.world()
                    .get::<NavigationRouteFailed>(passenger)
                    .is_none(),
                "the normal land planner rejected the arrival walk"
            );
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
