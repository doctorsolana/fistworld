//! Cheap ambient life for residents who currently lack a home or a job.
//!
//! Every observed resident owns an independent next-decision timestamp. The
//! ECS pass is a cheap deadline scan; only people whose personal event is due
//! make a choice. This avoids both a per-frame behaviour tree and a global
//! crowd batch. Destinations remain deterministic roadside/gathering spots and
//! ordinary movement reuses the cached village-road routing layer. Regions
//! nobody observes receive no ambient orders at all.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use shared::components::{
    BuildingDoorUse, CharacterActivity, CharacterKind, CharacterName, Occupation, PersonId,
    PlayerPosition, PlayerRotation, Settlement, SettlementBuilding, SettlementBuildingKind,
    VillageRoad, WorkStatus, WorldTime,
};
use shared::region::{RegionCoord, SimLevel};
use shared::spatial::SpatialObstacleGrid;
use shared::terrain::WorldTerrain;

use crate::collision::library::{DerivedColliderLibrary, StaticColliders};
use crate::player::hero::{navigation_segment_clear, MoveTarget};
use crate::world::regions::RegionRegistry;
use crate::world::village_roads::{
    NavigationLoad, NavigationRouteFailed, NavigationRoutePending, RoadBuilderRoutine, TravelRoute,
};

use super::{
    ConstructionMaterialRoutine, FarmerRoutine, FishingRoutine, HomeAssignment, HomeRoutine,
    HouseholdShoppingRoutine, LumberjackRoutine, MootMealRoutine, MootQueueTicket, MootSteward,
    PierTraversal, TavernVisitRoutine, TavernWorkerRoutine, TradeRouteRoutine, VillagerIntent,
    WorkerOffDuty, WorkplaceDoorTransit,
};

const AMBIENT_REACH: f32 = 0.65;
const ROADSIDE_SPACING: f32 = 6.0;
const ROADSIDE_MARGIN: f32 = 0.62;
/// Cosmetic loitering should use the resident's neighbourhood. Picking a
/// random point from every road in a mature town created hundreds of 100-250m
/// low-priority routes and made unemployed residents appear stuck at home
/// while pathfinding caught up.
const MAX_AMBIENT_WALK_DISTANCE: f32 = 46.0;
/// A person who has just registered at the Hall gets one wider first walk so
/// a whole immigration wave does not occupy the same handful of forecourt
/// verges. Later leisure remains neighbourhood-local for routing cost.
const ARRIVAL_DISPERSAL_DISTANCE: f32 = 82.0;
const ARRIVAL_DISPERSAL_MIN_DISTANCE: f32 = 18.0;
const AMBIENT_OCCUPANCY_CELL: f32 = 1.65;
const MAX_AMBIENT_TRAVEL_SECONDS: f32 = 60.0;
/// Visible flavour is admitted, not batch-decided. Every resident keeps an
/// independent mind and deadline, but only this many optional journeys per
/// settlement may occupy the expensive tactical route queue simultaneously.
/// Night shelter, work, food and other real needs bypass this cosmetic cap.
const MAX_ACTIVE_AMBIENT_ROUTES_PER_SETTLEMENT: usize = 96;
/// Shared monotonic world-time base for independent per-person deadlines.
#[derive(Resource, Default)]
pub struct AmbientClock {
    world_seconds_elapsed: f64,
    had_tactical_observer: bool,
}

/// Roadside candidates are geometry, not a per-person decision. Cache the raw
/// road layout until a hall or built prefix changes, then validate only a
/// resident's selected destination against current buildings and streamed
/// props. A crowd never resweeps every old roadside point after one new house.
#[derive(Resource, Default)]
pub struct AmbientSpotCache {
    signature: u64,
    initialized: bool,
    by_settlement: HashMap<Entity, Vec<AmbientSpot>>,
}

/// Optional fine-grained lab telemetry. The live server does not insert this
/// resource, so ambient flavour has no timing-allocation overhead in play.
#[derive(Resource, Default)]
pub struct AmbientDiagnostics {
    pub pass_milliseconds: Vec<f64>,
    pub decisions_per_pass: Vec<usize>,
}

#[derive(Component, Debug, Clone)]
pub struct AmbientRoutine {
    settlement: Entity,
    cycle: u32,
    /// Per-person elapsed time is necessary because a large population is
    /// advanced only when its independent deadline is due.
    last_world_seconds: f64,
    next_world_seconds: f64,
    /// The first optional journey after residency should leave the civic
    /// forecourt. This is consumed as soon as a destination is admitted.
    disperse_arrival: bool,
    phase: AmbientPhase,
}

/// A newly admitted resident's first straight, pre-certified walk away from
/// the civic forecourt. It bypasses the global A* queue just like a queue step,
/// but remains separate from Moot service ownership so stale markers can be
/// cleaned without affecting a real line.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct AmbientDirectTransit;

impl AmbientRoutine {
    pub(crate) const fn settlement(&self) -> Entity {
        self.settlement
    }

    pub(crate) const fn objective(&self) -> shared::components::CharacterObjective {
        use shared::components::CharacterObjective;
        if self.disperse_arrival {
            return CharacterObjective::SettlingIntoTown;
        }
        match self.phase {
            AmbientPhase::Waiting { .. } | AmbientPhase::Resting { .. } => {
                CharacterObjective::Resting
            }
            AmbientPhase::Walking { .. } => CharacterObjective::WalkingAroundTown,
            AmbientPhase::NightPending { .. } | AmbientPhase::NightShelter { .. } => {
                CharacterObjective::ShelteringAtMoot
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum AmbientPhase {
    Waiting {
        seconds_left: f32,
    },
    Walking {
        destination: Vec3,
        facing: f32,
        sitting: bool,
        rest_seconds: f32,
        travel_seconds_left: f32,
        route_started: bool,
    },
    Resting {
        seconds_left: f32,
        sitting: bool,
    },
    /// Homeless residents notice night independently over a short window
    /// instead of the whole settlement pivoting toward the hall on one tick.
    NightPending {
        destination: Vec3,
    },
    /// Unhoused residents gather at the Moot Hall after dark. Going through
    /// the hall door can replace this once halls gain an interior routine.
    NightShelter {
        destination: Vec3,
    },
}

#[derive(Debug, Clone, Copy)]
struct AmbientSpot {
    point: Vec2,
    facing: f32,
    market: bool,
}

fn stable_hash(text: &str) -> u64 {
    text.bytes().fold(1_469_598_103_934_665_603, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(1_099_511_628_211)
    })
}

fn mix(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn duration(seed: u64, min: f32, max: f32) -> f32 {
    let fraction = (mix(seed) >> 40) as f32 / (1u64 << 24) as f32;
    min + (max - min) * fraction
}

fn facing_toward(direction: Vec2) -> f32 {
    f32::atan2(-direction.x, -direction.y)
}

fn raw_roadside_spots(road: &VillageRoad) -> Vec<AmbientSpot> {
    let offset = road.width * 0.5 + ROADSIDE_MARGIN;
    let mut spots = Vec::new();
    for pair in road.built_points().windows(2) {
        let delta = pair[1] - pair[0];
        let length = delta.length();
        if length < 1.0 {
            continue;
        }
        let tangent = delta / length;
        let normal = Vec2::new(-tangent.y, tangent.x);
        let samples = (length / ROADSIDE_SPACING).ceil().max(1.0) as usize;
        for sample in 0..samples {
            let along = (sample as f32 + 0.5) / samples as f32;
            let centre = pair[0].lerp(pair[1], along);
            for side in [-1.0, 1.0] {
                let point = centre + normal * offset * side;
                spots.push(AmbientSpot {
                    point,
                    facing: facing_toward(centre - point),
                    market: false,
                });
            }
        }
    }
    spots
}

fn point_is_safe(
    point: Vec2,
    terrain: &WorldTerrain,
    obstacles: Option<&SpatialObstacleGrid>,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
) -> bool {
    let height = terrain.get_height(point.x, point.y);
    if terrain
        .water_level()
        .is_some_and(|water| height < water + super::FREEBOARD)
    {
        return false;
    }
    if 1.0 - terrain.get_normal(point.x, point.y).y.clamp(0.0, 1.0) > 0.24 {
        return false;
    }
    navigation_segment_clear(point, point, obstacles, colliders, derived)
}

/// A sleeping body occupies more ground than a standing navigation capsule.
/// Face the road, then lie backwards onto the verge, provided the whole patch
/// is level and clear; cramped or sloped spots retain the seated rest pose.
fn lying_place_is_safe(
    point: Vec2,
    facing: f32,
    terrain: &WorldTerrain,
    obstacles: Option<&SpatialObstacleGrid>,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
) -> bool {
    let backward = Vec2::new(facing.sin(), facing.cos());
    let side = Vec2::new(backward.y, -backward.x) * 0.48;
    let height = terrain.get_height(point.x, point.y);
    [
        point - side,
        point + side,
        point + backward * 1.7 - side,
        point + backward * 1.7 + side,
    ]
    .into_iter()
    .all(|at| {
        (terrain.get_height(at.x, at.y) - height).abs() < 0.16
            && point_is_safe(at, terrain, obstacles, colliders, derived)
            && navigation_segment_clear(point, at, obstacles, colliders, derived)
    })
}

fn gathering_spots(
    settlements: &Query<
        (
            Entity,
            &Settlement,
            &shared::components::SettlementId,
            &PlayerPosition,
            Option<&PlayerRotation>,
        ),
        Without<CharacterKind>,
    >,
    roads: &Query<(&VillageRoad, &shared::components::RoadOf)>,
    buildings: &Query<
        (
            &SettlementBuilding,
            &shared::components::BuildingOf,
            &PlayerPosition,
            Option<&PlayerRotation>,
        ),
        Without<CharacterKind>,
    >,
) -> HashMap<Entity, Vec<AmbientSpot>> {
    let mut by_settlement = HashMap::new();
    let mut entity_by_id = HashMap::new();

    for (entity, _settlement, settlement_id, hall, rotation) in settlements.iter() {
        entity_by_id.insert(*settlement_id, entity);
        let yaw = rotation.map_or(0.0, |rotation| rotation.0);
        let door = SettlementBuildingKind::Hall.entrance_position(hall.0, yaw);
        let side_axis = shared::rotation::local_to_world_xz(Vec2::X, yaw);
        let mut spots = Vec::new();
        for side in [-1.0, 1.0] {
            let point = Vec2::new(door.x, door.z) + side_axis * 2.25 * side;
            spots.push(AmbientSpot {
                point,
                facing: facing_toward(Vec2::new(hall.0.x, hall.0.z) - point),
                market: false,
            });
        }
        by_settlement.insert(entity, spots);
    }

    for (road, road_of) in roads.iter() {
        let Some(entity) = entity_by_id.get(&road_of.0).copied() else {
            continue;
        };
        let destination = by_settlement.entry(entity).or_insert_with(Vec::new);
        destination.extend(raw_roadside_spots(road));
    }

    // Markets are gathering places even before stalls and luxury purchases
    // gain dedicated routines. These points share the ordinary cached ambient
    // routing path, so adding visible owner leisure has no per-frame search.
    for (building, building_of, position, rotation) in buildings.iter() {
        if building.kind != SettlementBuildingKind::Market {
            continue;
        }
        let Some(settlement) = entity_by_id.get(&building_of.0).copied() else {
            continue;
        };
        let yaw = rotation.map_or(0.0, |rotation| rotation.0);
        let entrance = building.kind.entrance_position(position.0, yaw);
        let entrance = Vec2::new(entrance.x, entrance.z);
        let centre = Vec2::new(position.0.x, position.0.z);
        let side = shared::rotation::local_to_world_xz(Vec2::X, yaw);
        let outward = (entrance - centre).try_normalize().unwrap_or(Vec2::Y);
        let spots = by_settlement.entry(settlement).or_default();
        for lateral in [-2.2_f32, 0.0, 2.2] {
            let point = entrance + side * lateral + outward * 1.1;
            spots.push(AmbientSpot {
                point,
                facing: facing_toward(entrance - point),
                market: true,
            });
        }
    }

    by_settlement
}

fn spot_geometry_signature(
    settlements: &Query<
        (
            Entity,
            &Settlement,
            &shared::components::SettlementId,
            &PlayerPosition,
            Option<&PlayerRotation>,
        ),
        Without<CharacterKind>,
    >,
    roads: &Query<(&VillageRoad, &shared::components::RoadOf)>,
    buildings: &Query<
        (
            &SettlementBuilding,
            &shared::components::BuildingOf,
            &PlayerPosition,
            Option<&PlayerRotation>,
        ),
        Without<CharacterKind>,
    >,
) -> u64 {
    let mut signature = 0_u64;
    for (entity, _settlement, settlement_id, position, rotation) in settlements.iter() {
        let value = entity.to_bits()
            ^ settlement_id.0
            ^ u64::from(position.0.x.to_bits()).rotate_left(7)
            ^ u64::from(position.0.z.to_bits()).rotate_left(19)
            ^ u64::from(rotation.map_or(0.0, |rotation| rotation.0).to_bits()).rotate_left(31);
        signature = signature.wrapping_add(mix(value));
    }
    for (road, road_of) in roads.iter() {
        let value = road_of.0 .0
            ^ u64::from(road.built_through).rotate_left(11)
            ^ (road.points.len() as u64).rotate_left(29)
            ^ u64::from(road.width.to_bits()).rotate_left(43);
        signature = signature.wrapping_add(mix(value));
    }
    for (building, building_of, position, rotation) in buildings.iter() {
        if building.kind != SettlementBuildingKind::Market {
            continue;
        }
        let value = building_of.0 .0
            ^ u64::from(position.0.x.to_bits()).rotate_left(5)
            ^ u64::from(position.0.z.to_bits()).rotate_left(23)
            ^ u64::from(rotation.map_or(0.0, |rotation| rotation.0).to_bits()).rotate_left(41);
        signature = signature.wrapping_add(mix(value));
    }
    signature
}

fn clear_owned_movement(commands: &mut Commands, entity: Entity) {
    commands
        .entity(entity)
        .remove::<AmbientRoutine>()
        .remove::<MoveTarget>()
        .remove::<TravelRoute>()
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>()
        .remove::<AmbientDirectTransit>();
}

fn direct_ambient_segment_is_safe(
    origin: Vec2,
    destination: Vec2,
    terrain: &WorldTerrain,
    obstacles: Option<&SpatialObstacleGrid>,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
) -> bool {
    if !navigation_segment_clear(origin, destination, obstacles, colliders, derived) {
        return false;
    }
    crate::player::hero::terrain_segment_walkable(terrain, origin, destination)
}

fn select_ambient_spot(
    start: usize,
    spots: &[AmbientSpot],
    prefer_market: bool,
    mut eligible: impl FnMut(AmbientSpot) -> bool,
) -> Option<AmbientSpot> {
    let find_spot = |market_only: bool, eligible: &mut dyn FnMut(AmbientSpot) -> bool| {
        (0..spots.len()).find_map(|offset| {
            let spot = spots[(start + offset) % spots.len()];
            (!market_only || spot.market)
                .then_some(spot)
                .filter(|spot| eligible(*spot))
        })
    };
    if prefer_market {
        find_spot(true, &mut eligible).or_else(|| find_spot(false, &mut eligible))
    } else {
        find_spot(false, &mut eligible)
    }
}

fn choose_spot(
    name: &str,
    identity_seed: u64,
    routine: &mut AmbientRoutine,
    origin: Vec2,
    spots: &[AmbientSpot],
    prefer_market: bool,
    disperse_arrival: bool,
    occupied: &HashSet<(i32, i32)>,
    terrain: &WorldTerrain,
    obstacles: Option<&SpatialObstacleGrid>,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
) -> Option<(Vec3, f32, bool, f32, bool)> {
    if spots.is_empty() {
        return None;
    }
    let seed = mix(stable_hash(name) ^ identity_seed ^ u64::from(routine.cycle));
    routine.cycle = routine.cycle.wrapping_add(1);
    // Cache raw road geometry and validate only actual local candidates. Walk
    // the vector from a person-specific offset so nearby actors do not all
    // choose the same first verge. Distance checks are deliberately cheap;
    // collider validation only runs for points inside the neighbourhood.
    let start = seed as usize % spots.len();
    let max_distance = if disperse_arrival {
        ARRIVAL_DISPERSAL_DISTANCE
    } else {
        MAX_AMBIENT_WALK_DISTANCE
    };
    let eligible = |spot: AmbientSpot, require_departure_distance: bool| {
        let distance_squared = origin.distance_squared(spot.point);
        distance_squared <= max_distance.powi(2)
            && (!require_departure_distance
                || distance_squared >= ARRIVAL_DISPERSAL_MIN_DISTANCE.powi(2))
            && !ambient_place_occupied(occupied, spot.point)
            && point_is_safe(spot.point, terrain, obstacles, colliders, derived)
    };
    // Most short roadside strolls need no A*: if the same live static
    // collision and terrain rules certify the straight segment, embodied
    // movement can follow it directly. This is both cheaper and more lively
    // than making every unemployed resident wait behind freight for a planner
    // to rediscover the same straight line. Difficult corners still fall back
    // to the ordinary shared road/path queue below.
    let direct = select_ambient_spot(start, spots, prefer_market, |spot| {
        eligible(spot, disperse_arrival)
            && direct_ambient_segment_is_safe(
                origin, spot.point, terrain, obstacles, colliders, derived,
            )
    });
    let (spot, direct_transit) = if let Some(spot) = direct {
        (spot, true)
    } else if disperse_arrival {
        // Prefer an actual departure from the Hall neighbourhood. Tiny or
        // roadless hamlets may not have such a point yet, so retain a local
        // fallback instead of leaving a resident permanently undecided.
        select_ambient_spot(start, spots, prefer_market, |spot| eligible(spot, true))
            .or_else(|| {
                select_ambient_spot(start, spots, prefer_market, |spot| eligible(spot, false))
            })
            .map(|spot| (spot, false))?
    } else {
        (
            select_ambient_spot(start, spots, prefer_market, |spot| eligible(spot, false))?,
            false,
        )
    };
    let point = Vec3::new(
        spot.point.x,
        terrain.get_height(spot.point.x, spot.point.y),
        spot.point.y,
    );
    let sitting = !mix(seed ^ 0x0a11_ce55).is_multiple_of(3);
    let rest = duration(seed ^ 0x5eed, 12.0, 32.0);
    Some((point, spot.facing, sitting, rest, direct_transit))
}

fn ambient_occupancy_cell(point: Vec2) -> (i32, i32) {
    (
        (point.x / AMBIENT_OCCUPANCY_CELL).floor() as i32,
        (point.y / AMBIENT_OCCUPANCY_CELL).floor() as i32,
    )
}

fn ambient_place_occupied(occupied: &HashSet<(i32, i32)>, point: Vec2) -> bool {
    let (x, y) = ambient_occupancy_cell(point);
    (-1..=1).any(|dx| (-1..=1).any(|dy| occupied.contains(&(x + dx, y + dy))))
}

/// Give unemployed or unhoused residents a little visible life when observed.
///
/// This pass is intentionally bounded by wall time and region simulation LOD.
/// It never pathfinds itself: it writes one `MoveTarget`, after which the
/// existing budgeted route queue and shared road graph do the travel work.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run_ambient_routines(
    simulation_time: crate::world::simulation_time::SimulationTime,
    runtime: (
        ResMut<AmbientClock>,
        ResMut<AmbientSpotCache>,
        Option<ResMut<AmbientDiagnostics>>,
        Option<Res<NavigationLoad>>,
    ),
    world_time: Query<&WorldTime>,
    terrain: Option<Res<WorldTerrain>>,
    regions: Option<Res<RegionRegistry>>,
    obstacles: Option<Res<SpatialObstacleGrid>>,
    colliders: Option<Res<StaticColliders>>,
    derived: Option<Res<DerivedColliderLibrary>>,
    settlements: Query<
        (
            Entity,
            &Settlement,
            &shared::components::SettlementId,
            &PlayerPosition,
            Option<&PlayerRotation>,
        ),
        Without<CharacterKind>,
    >,
    roads: Query<(&VillageRoad, &shared::components::RoadOf)>,
    buildings: Query<
        (
            &SettlementBuilding,
            &shared::components::BuildingOf,
            &PlayerPosition,
            Option<&PlayerRotation>,
        ),
        Without<CharacterKind>,
    >,
    busy: Query<
        (),
        Or<(
            With<FarmerRoutine>,
            With<FishingRoutine>,
            With<LumberjackRoutine>,
            With<HomeRoutine>,
            With<RoadBuilderRoutine>,
            With<WorkplaceDoorTransit>,
            With<BuildingDoorUse>,
            With<PierTraversal>,
            With<HouseholdShoppingRoutine>,
            With<MootQueueTicket>,
            With<MootMealRoutine>,
            With<TradeRouteRoutine>,
            Or<(With<TavernVisitRoutine>, With<TavernWorkerRoutine>)>,
            With<crate::world::settlement_development::CivicHallBuilderRoutine>,
            // The combined Moot Steward waits inside the hall between
            // collections. An unhoused founding steward is still on duty and
            // must not receive an ambient roadside order that fights the
            // market system's stable Indoors state.
            With<MootSteward>,
        )>,
    >,
    mut villagers: Query<
        (
            Entity,
            &CharacterName,
            &PlayerPosition,
            &mut PlayerRotation,
            &RegionCoord,
            Ref<VillagerIntent>,
            (&Occupation, Option<&WorkStatus>),
            Option<&PersonId>,
            Option<&HomeAssignment>,
            Option<&WorkerOffDuty>,
            Option<&ConstructionMaterialRoutine>,
            &mut CharacterActivity,
            Option<&MoveTarget>,
            (
                Option<&NavigationRouteFailed>,
                Option<&TravelRoute>,
                Has<NavigationRoutePending>,
            ),
            Option<&mut AmbientRoutine>,
        ),
        (
            With<CharacterKind>,
            Without<super::strategic::StrategicPerson>,
        ),
    >,
    mut commands: Commands,
) {
    let (mut ambient_clock, mut spot_cache, mut diagnostics, navigation_load) = runtime;
    ambient_clock.world_seconds_elapsed += f64::from(simulation_time.world_seconds());
    let pass_started = diagnostics.as_ref().map(|_| std::time::Instant::now());
    let mut decisions = 0usize;
    let mut ambient_admitted: HashMap<Entity, usize> = navigation_load
        .as_deref()
        .map(|load| {
            load.ambient_by_settlement
                .iter()
                .map(|(settlement, count)| (*settlement, *count))
                .collect()
        })
        .unwrap_or_default();
    let any_tactical_observer = regions
        .as_ref()
        .is_none_or(|registry| registry.tactical_count() > 0);
    if !any_tactical_observer && !ambient_clock.had_tactical_observer {
        return;
    }
    ambient_clock.had_tactical_observer = any_tactical_observer;
    let Some(terrain) = terrain else { return };
    let Some(daylight) = world_time.iter().next().map(WorldTime::is_day) else {
        return;
    };
    if any_tactical_observer {
        let signature = spot_geometry_signature(&settlements, &roads, &buildings);
        if !spot_cache.initialized || spot_cache.signature != signature {
            spot_cache.by_settlement = gathering_spots(&settlements, &roads, &buildings);
            spot_cache.signature = signature;
            spot_cache.initialized = true;
        }
    }

    // Positions and already selected destinations are one cheap spatial
    // reservation set. It prevents independent ambient minds from choosing
    // the same verge without introducing pairwise crowd simulation.
    let mut ambient_occupied: HashMap<Entity, HashSet<(i32, i32)>> = HashMap::new();
    for (_, _, position, _, _, intent, _, _, _, _, _, _, move_target, _, routine) in
        villagers.iter_mut()
    {
        let VillagerIntent::Resident { settlement } = *intent else {
            continue;
        };
        let occupied = ambient_occupied.entry(settlement).or_default();
        occupied.insert(ambient_occupancy_cell(Vec2::new(
            position.0.x,
            position.0.z,
        )));
        if routine.is_some() {
            if let Some(target) = move_target {
                occupied.insert(ambient_occupancy_cell(Vec2::new(target.0.x, target.0.z)));
            }
        }
    }

    for (
        entity,
        name,
        position,
        mut facing,
        region,
        intent,
        (occupation, work_status),
        person_id,
        home,
        off_duty,
        construction,
        mut activity,
        move_target,
        (route_failed, travel_route, route_pending),
        routine,
    ) in villagers.iter_mut()
    {
        let newly_resident =
            intent.is_changed() && matches!(*intent, VillagerIntent::Resident { .. });
        let waiting_builder =
            construction.is_some_and(|routine| routine.is_waiting_for_materials());
        let settlement = match &*intent {
            VillagerIntent::Resident { settlement } => *settlement,
            VillagerIntent::Building { .. } if waiting_builder => {
                // Waiting for the settlement's material turn is still a
                // committed construction job. Giving that builder a cosmetic
                // roadside destination races the next tree/store order: the
                // old ambient MoveTarget can arrive and remove the newly
                // queued work target on the same deferred-command boundary.
                // This previously left a nearly supplied priority site at
                // 9/10 Wood while every other builder politely waited.
                //
                // Clear that stale ambient movement exactly ONCE - while the
                // ambient routine is still attached - then stand aside. An
                // unconditional per-tick clear also deleted construction's
                // own wait-at-own-worksite order, pinning every stock-denied
                // builder in an overlapping pile wherever they entered the
                // wait (usually the hall counter).
                if routine.is_some() {
                    activity.set_if_neq(CharacterActivity::Idle);
                    clear_owned_movement(&mut commands, entity);
                    commands.entity(entity).remove::<AmbientRoutine>();
                }
                continue;
            }
            _ => {
                if routine.is_some() {
                    if matches!(
                        *activity,
                        CharacterActivity::Sitting | CharacterActivity::LyingDown
                    ) {
                        activity.set_if_neq(CharacterActivity::Idle);
                    }
                    // A real active work/build/home routine now owns any destination.
                    commands.entity(entity).remove::<AmbientRoutine>();
                }
                continue;
            }
        };

        if busy.get(entity).is_ok() {
            if routine.is_some() {
                if matches!(
                    *activity,
                    CharacterActivity::Sitting | CharacterActivity::LyingDown
                ) {
                    activity.set_if_neq(CharacterActivity::Idle);
                }
                commands.entity(entity).remove::<AmbientRoutine>();
            }
            continue;
        }

        let needs_ambient_life = off_duty.is_some() || occupation.0.is_none() || home.is_none();
        if !needs_ambient_life {
            if routine.is_some() {
                if matches!(
                    *activity,
                    CharacterActivity::Sitting | CharacterActivity::LyingDown
                ) {
                    activity.set_if_neq(CharacterActivity::Idle);
                }
                clear_owned_movement(&mut commands, entity);
            }
            continue;
        }

        // No tactical observer means no ambient decisions, routes, movement or
        // animation. Keeping the last authoritative position is the temporary
        // representation until strategic Person promotion/demotion lands.
        let tactical = regions.as_ref().is_none_or(|registry| {
            registry
                .get(*region)
                .is_some_and(|state| state.sim_level == SimLevel::Tactical)
        });
        if !tactical {
            if routine.is_some() {
                if matches!(
                    *activity,
                    CharacterActivity::Sitting | CharacterActivity::LyingDown
                ) {
                    activity.set_if_neq(CharacterActivity::Idle);
                }
                clear_owned_movement(&mut commands, entity);
            }
            continue;
        }

        let now = ambient_clock.world_seconds_elapsed;

        let hall = settlements.get(settlement).ok();
        if !daylight && home.is_none() {
            let Some((_, _, _, hall_position, hall_rotation)) = hall else {
                continue;
            };
            let entrance = SettlementBuildingKind::Hall.entrance_position(
                hall_position.0,
                hall_rotation.map_or(0.0, |rotation| rotation.0),
            );
            // Claim an existing validated roadside gathering spot once. Reuse
            // that destination all night instead of piling bodies in the door.
            let destination = routine
                .as_deref()
                .and_then(|routine| match routine.phase {
                    AmbientPhase::NightPending { destination }
                    | AmbientPhase::NightShelter { destination } => Some(destination),
                    _ => None,
                })
                .unwrap_or_else(|| {
                    let seed = person_id.map_or(entity.to_bits(), |id| id.0) as usize;
                    let occupied = ambient_occupied.entry(settlement).or_default();
                    let spot = spot_cache.by_settlement.get(&settlement).and_then(|spots| {
                        select_ambient_spot(seed % spots.len().max(1), spots, false, |spot| {
                            spot.point.distance_squared(entrance.xz()) > 9.0
                                && spot.point.distance_squared(hall_position.0.xz()) < 48.0 * 48.0
                                && !ambient_place_occupied(occupied, spot.point)
                                && point_is_safe(
                                    spot.point,
                                    &terrain,
                                    obstacles.as_deref(),
                                    colliders.as_deref(),
                                    derived.as_deref(),
                                )
                        })
                    });
                    if let Some(spot) = spot {
                        occupied.insert(ambient_occupancy_cell(spot.point));
                        Vec3::new(
                            spot.point.x,
                            terrain.get_height(spot.point.x, spot.point.y),
                            spot.point.y,
                        )
                    } else {
                        position.0
                    }
                });
            let already_heading_to_shelter = routine.as_deref().is_some_and(|routine| {
                matches!(
                    routine.phase,
                    AmbientPhase::NightPending { .. } | AmbientPhase::NightShelter { .. }
                )
            });
            if !already_heading_to_shelter {
                let identity_seed = person_id.map_or(entity.to_bits(), |person_id| person_id.0);
                let delay = duration(
                    stable_hash(&name.0) ^ identity_seed ^ 0x006e_6967_6874,
                    0.0,
                    5.0,
                );
                if let Some(mut routine) = routine {
                    routine.settlement = settlement;
                    routine.last_world_seconds = now;
                    routine.next_world_seconds = now + f64::from(delay);
                    routine.phase = AmbientPhase::NightPending { destination };
                } else {
                    commands.entity(entity).insert(AmbientRoutine {
                        settlement,
                        cycle: 0,
                        last_world_seconds: now,
                        next_world_seconds: now + f64::from(delay),
                        disperse_arrival: false,
                        phase: AmbientPhase::NightPending { destination },
                    });
                }
                continue;
            }
            let night_due = routine
                .as_deref()
                .is_none_or(|routine| now + f64::EPSILON >= routine.next_world_seconds);
            if !night_due {
                continue;
            }
            decisions += 1;
            activity.set_if_neq(CharacterActivity::Idle);
            if super::ground_distance(position.0, destination) <= AMBIENT_REACH {
                if let Some(spot) = spot_cache.by_settlement.get(&settlement).and_then(|spots| {
                    spots
                        .iter()
                        .find(|spot| spot.point.distance_squared(destination.xz()) < 0.1)
                }) {
                    facing.0 = spot.facing;
                }
                activity.set_if_neq(
                    if super::ground_distance(position.0, entrance) > 3.0
                        && lying_place_is_safe(
                            position.0.xz(),
                            facing.0,
                            &terrain,
                            obstacles.as_deref(),
                            colliders.as_deref(),
                            derived.as_deref(),
                        )
                    {
                        CharacterActivity::LyingDown
                    } else {
                        CharacterActivity::Sitting
                    },
                );
                commands
                    .entity(entity)
                    .remove::<MoveTarget>()
                    .remove::<TravelRoute>()
                    .remove::<NavigationRoutePending>()
                    .remove::<NavigationRouteFailed>();
                if let Some(mut routine) = routine {
                    routine.settlement = settlement;
                    routine.last_world_seconds = now;
                    routine.next_world_seconds = now + 30.0;
                    routine.phase = AmbientPhase::NightShelter { destination };
                } else {
                    commands.entity(entity).insert(AmbientRoutine {
                        settlement,
                        cycle: 0,
                        last_world_seconds: now,
                        next_world_seconds: now + 30.0,
                        disperse_arrival: false,
                        phase: AmbientPhase::NightShelter { destination },
                    });
                }
            } else {
                super::ensure_move_target(&mut commands, entity, move_target, destination);
                if let Some(mut routine) = routine {
                    routine.settlement = settlement;
                    routine.last_world_seconds = now;
                    routine.next_world_seconds = now + f64::from(MAX_AMBIENT_TRAVEL_SECONDS);
                    routine.phase = AmbientPhase::NightShelter { destination };
                } else {
                    commands.entity(entity).insert(AmbientRoutine {
                        settlement,
                        cycle: 0,
                        last_world_seconds: now,
                        next_world_seconds: now + f64::from(MAX_AMBIENT_TRAVEL_SECONDS),
                        disperse_arrival: false,
                        phase: AmbientPhase::NightShelter { destination },
                    });
                }
            }
            continue;
        }

        if !daylight {
            // A housed villager should have acquired HomeRoutine earlier in the
            // chained village schedule. If that is delayed for one pass, avoid
            // starting a fresh daytime stroll at night.
            if let Some(mut routine) = routine {
                routine.last_world_seconds = now;
                routine.next_world_seconds = now + 1.0;
            }
            continue;
        }

        let Some(mut routine) = routine else {
            let identity_seed = person_id.map_or(entity.to_bits(), |person_id| person_id.0);
            let seed = stable_hash(&name.0) ^ identity_seed;
            let wait = if newly_resident {
                // Still one independent deadline per person: at 10x this is
                // at most 0.875 real seconds, while a mass arrival does not
                // wake as one visible or computational pulse.
                duration(seed ^ 0x6172_7269_7661_6c, 0.15, 8.75)
            } else {
                duration(seed, 2.0, 12.0)
            };
            commands.entity(entity).insert(AmbientRoutine {
                settlement,
                cycle: 0,
                last_world_seconds: now,
                next_world_seconds: now + f64::from(wait),
                disperse_arrival: newly_resident,
                phase: AmbientPhase::Waiting { seconds_left: wait },
            });
            continue;
        };

        if routine.settlement != settlement {
            routine.settlement = settlement;
            routine.phase = AmbientPhase::Waiting { seconds_left: 1.0 };
            routine.next_world_seconds = now + 1.0;
        }
        let movement_event = match routine.phase {
            AmbientPhase::Walking { destination, .. } => {
                move_target.is_none()
                    || route_failed
                        .is_some_and(|failed| failed.goal.distance_squared(destination) <= 0.01)
            }
            // Dawn wakes sheltering residents immediately even when their
            // overnight idle deadline is still in the future.
            AmbientPhase::NightShelter { .. } => true,
            _ => false,
        };
        if !movement_event && now + f64::EPSILON < routine.next_world_seconds {
            continue;
        }
        decisions += 1;
        let dt = (now - routine.last_world_seconds).max(0.0) as f32;
        routine.last_world_seconds = now;

        match routine.phase {
            AmbientPhase::NightPending { destination } => {
                let _ = destination;
                activity.set_if_neq(CharacterActivity::Idle);
                routine.phase = AmbientPhase::Waiting { seconds_left: 1.0 };
                routine.next_world_seconds = now + 1.0;
            }
            AmbientPhase::NightShelter { destination } => {
                let _ = destination;
                activity.set_if_neq(CharacterActivity::Idle);
                routine.phase = AmbientPhase::Waiting { seconds_left: 1.0 };
                routine.next_world_seconds = now + 1.0;
            }
            AmbientPhase::Waiting { seconds_left } => {
                activity.set_if_neq(CharacterActivity::Idle);
                commands
                    .entity(entity)
                    .remove::<MoveTarget>()
                    .remove::<TravelRoute>()
                    .remove::<NavigationRoutePending>()
                    .remove::<NavigationRouteFailed>();
                let left = seconds_left - dt;
                if left > 0.0 {
                    routine.phase = AmbientPhase::Waiting { seconds_left: left };
                    routine.next_world_seconds = now + f64::from(left);
                    continue;
                }
                let active_for_settlement = ambient_admitted.entry(settlement).or_default();
                if *active_for_settlement >= MAX_ACTIVE_AMBIENT_ROUTES_PER_SETTLEMENT
                    && !routine.disperse_arrival
                {
                    // Keep the personal timeline independent and retry after
                    // a deterministic pause. No MoveTarget means this waiting
                    // resident consumes no route-planner work while the town
                    // is already visually busy.
                    let identity_seed = person_id.map_or(entity.to_bits(), |person_id| person_id.0);
                    let retry = duration(
                        identity_seed ^ u64::from(routine.cycle) ^ 0x6164_6d69_7373_696f,
                        4.0,
                        12.0,
                    );
                    routine.phase = AmbientPhase::Waiting {
                        seconds_left: retry,
                    };
                    routine.next_world_seconds = now + f64::from(retry);
                    continue;
                }
                let occupied = ambient_occupied.entry(settlement).or_default();
                let disperse_arrival = routine.disperse_arrival;
                let Some((destination, rest_facing, sitting, rest_seconds, direct_transit)) =
                    spot_cache.by_settlement.get(&settlement).and_then(|spots| {
                        choose_spot(
                            &name.0,
                            person_id.map_or(entity.to_bits(), |person_id| person_id.0),
                            &mut routine,
                            Vec2::new(position.0.x, position.0.z),
                            spots,
                            work_status.is_some_and(|status| *status == WorkStatus::Chilling),
                            disperse_arrival,
                            occupied,
                            &terrain,
                            obstacles.as_deref(),
                            colliders.as_deref(),
                            derived.as_deref(),
                        )
                    })
                else {
                    routine.phase = AmbientPhase::Waiting { seconds_left: 12.0 };
                    routine.next_world_seconds = now + 12.0;
                    continue;
                };
                if *active_for_settlement >= MAX_ACTIVE_AMBIENT_ROUTES_PER_SETTLEMENT
                    && !direct_transit
                {
                    // Arrival dispersal may exceed the optional-route cap only
                    // when it is a collision/terrain-certified straight walk
                    // that creates no A* request. A dense forecourt with no
                    // such corridor waits cheaply instead of flooding the
                    // committed path queue.
                    let identity_seed = person_id.map_or(entity.to_bits(), |person_id| person_id.0);
                    let retry = duration(
                        identity_seed ^ u64::from(routine.cycle) ^ 0x6172_7269_7661_6c,
                        1.0,
                        3.0,
                    );
                    routine.phase = AmbientPhase::Waiting {
                        seconds_left: retry,
                    };
                    routine.next_world_seconds = now + f64::from(retry);
                    continue;
                }
                occupied.insert(ambient_occupancy_cell(Vec2::new(
                    destination.x,
                    destination.z,
                )));
                super::ensure_move_target(&mut commands, entity, move_target, destination);
                if direct_transit {
                    commands.entity(entity).insert(AmbientDirectTransit);
                } else {
                    commands.entity(entity).remove::<AmbientDirectTransit>();
                }
                *active_for_settlement += 1;
                routine.phase = AmbientPhase::Walking {
                    destination,
                    facing: rest_facing,
                    sitting,
                    rest_seconds,
                    travel_seconds_left: MAX_AMBIENT_TRAVEL_SECONDS,
                    route_started: false,
                };
                // Movement completion is an event: `step_units` removes the
                // MoveTarget on arrival, which wakes this routine on the next
                // update. The deadline is only a stuck-travel circuit breaker.
                routine.next_world_seconds = now + 1.0;
            }
            AmbientPhase::Walking {
                destination,
                facing: rest_facing,
                sitting,
                rest_seconds,
                travel_seconds_left,
                route_started,
            } => {
                activity.set_if_neq(CharacterActivity::Idle);
                if route_failed
                    .is_some_and(|failed| failed.goal.distance_squared(destination) <= 0.01)
                {
                    // Roadside flavour must never leave an unemployed resident
                    // sleeping on a static navigation failure. Abandon this
                    // optional destination immediately and choose another spot
                    // after a short pause.
                    commands
                        .entity(entity)
                        .remove::<MoveTarget>()
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>()
                        .remove::<AmbientDirectTransit>();
                    routine.phase = AmbientPhase::Waiting { seconds_left: 4.0 };
                    routine.next_world_seconds = now + 4.0;
                    continue;
                }
                if super::ground_distance(position.0, destination) <= AMBIENT_REACH {
                    commands
                        .entity(entity)
                        .remove::<MoveTarget>()
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>()
                        .remove::<AmbientDirectTransit>();
                    routine.disperse_arrival = false;
                    facing.0 = rest_facing;
                    *activity = if sitting {
                        if home.is_none()
                            && lying_place_is_safe(
                                position.0.xz(),
                                facing.0,
                                &terrain,
                                obstacles.as_deref(),
                                colliders.as_deref(),
                                derived.as_deref(),
                            )
                        {
                            CharacterActivity::LyingDown
                        } else {
                            CharacterActivity::Sitting
                        }
                    } else {
                        CharacterActivity::Idle
                    };
                    routine.phase = AmbientPhase::Resting {
                        seconds_left: rest_seconds,
                        sitting,
                    };
                    routine.next_world_seconds = now + f64::from(rest_seconds);
                    continue;
                }
                if !route_started {
                    if travel_route.is_some() {
                        // The stuck timer starts when a route actually exists,
                        // not while this low-priority request waits behind a
                        // farmer or porter in the bounded planner.
                        routine.phase = AmbientPhase::Walking {
                            destination,
                            facing: rest_facing,
                            sitting,
                            rest_seconds,
                            travel_seconds_left: MAX_AMBIENT_TRAVEL_SECONDS,
                            route_started: true,
                        };
                        routine.next_world_seconds = now + f64::from(MAX_AMBIENT_TRAVEL_SECONDS);
                        continue;
                    }
                    if route_pending || move_target.is_some() {
                        routine.phase = AmbientPhase::Walking {
                            destination,
                            facing: rest_facing,
                            sitting,
                            rest_seconds,
                            travel_seconds_left,
                            route_started: false,
                        };
                        routine.next_world_seconds = now + 1.0;
                        continue;
                    }
                }
                let left = travel_seconds_left - dt;
                if left <= 0.0 {
                    commands
                        .entity(entity)
                        .remove::<MoveTarget>()
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>()
                        .remove::<AmbientDirectTransit>();
                    routine.phase = AmbientPhase::Waiting { seconds_left: 4.0 };
                    routine.next_world_seconds = now + 4.0;
                } else {
                    super::ensure_move_target(&mut commands, entity, move_target, destination);
                    routine.phase = AmbientPhase::Walking {
                        destination,
                        facing: rest_facing,
                        sitting,
                        rest_seconds,
                        travel_seconds_left: left,
                        route_started,
                    };
                    routine.next_world_seconds = now + f64::from(left);
                }
            }
            AmbientPhase::Resting {
                seconds_left,
                sitting,
            } => {
                commands
                    .entity(entity)
                    .remove::<MoveTarget>()
                    .remove::<TravelRoute>()
                    .remove::<NavigationRoutePending>()
                    .remove::<NavigationRouteFailed>()
                    .remove::<AmbientDirectTransit>();
                *activity = if sitting {
                    if home.is_none()
                        && lying_place_is_safe(
                            position.0.xz(),
                            facing.0,
                            &terrain,
                            obstacles.as_deref(),
                            colliders.as_deref(),
                            derived.as_deref(),
                        )
                    {
                        CharacterActivity::LyingDown
                    } else {
                        CharacterActivity::Sitting
                    }
                } else {
                    CharacterActivity::Idle
                };
                let left = seconds_left - dt;
                if left > 0.0 {
                    routine.phase = AmbientPhase::Resting {
                        seconds_left: left,
                        sitting,
                    };
                    routine.next_world_seconds = now + f64::from(left);
                } else {
                    activity.set_if_neq(CharacterActivity::Idle);
                    let seed = stable_hash(&name.0)
                        ^ person_id.map_or(entity.to_bits(), |person_id| person_id.0)
                        ^ u64::from(routine.cycle);
                    let wait = duration(seed, 4.0, 14.0);
                    routine.phase = AmbientPhase::Waiting { seconds_left: wait };
                    routine.next_world_seconds = now + f64::from(wait);
                }
            }
        }
    }
    if let (Some(diagnostics), Some(started)) = (diagnostics.as_deref_mut(), pass_started) {
        diagnostics
            .pass_milliseconds
            .push(started.elapsed().as_secs_f64() * 1_000.0);
        diagnostics.decisions_per_pass.push(decisions);
    }
}

/// Any real job, queue, household or strategic handoff may pre-empt the short
/// arrival walk. Remove its bypass marker after the owning AmbientRoutine is
/// gone so the new authoritative destination enters ordinary routing in the
/// same activity/navigation chain.
pub fn cleanup_orphaned_direct_transit(
    mut commands: Commands,
    orphaned: Query<Entity, (With<AmbientDirectTransit>, Without<AmbientRoutine>)>,
) {
    for entity in orphaned.iter() {
        commands.entity(entity).remove::<AmbientDirectTransit>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unhoused_night_rest_lies_down_and_dawn_releases_it() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<AmbientClock>();
        app.init_resource::<AmbientSpotCache>();
        let mut terrain = WorldTerrain::default();
        terrain.apply_flatten_rect(Vec3::new(12., 10., 8.), Vec2::splat(12.), 0., 10.);
        app.insert_resource(terrain);
        app.add_systems(Update, run_ambient_routines);
        let mut clock = WorldTime::new_default();
        clock.seconds_in_cycle = clock.day_duration + 30.;
        let clock_entity = app
            .world_mut()
            .spawn((clock, shared::components::TimeWarp::clamped(1.)))
            .id();
        let hall = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Restford".into(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 1,
                    treasury: 0,
                },
                shared::components::SettlementId(1),
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.),
            ))
            .id();
        let destination = Vec3::new(12., 10., 8.);
        let resident = app
            .world_mut()
            .spawn((
                CharacterName("Ada".into()),
                PersonId(82),
                CharacterKind::Villager,
                PlayerPosition(destination),
                PlayerRotation(0.),
                RegionCoord::default(),
                VillagerIntent::Resident { settlement: hall },
                Occupation::default(),
                CharacterActivity::Idle,
                AmbientRoutine {
                    settlement: hall,
                    cycle: 1,
                    last_world_seconds: 0.,
                    next_world_seconds: 0.,
                    disperse_arrival: false,
                    phase: AmbientPhase::NightShelter { destination },
                },
            ))
            .id();
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(0.3));
        app.update();
        assert_eq!(
            app.world().get::<CharacterActivity>(resident),
            Some(&CharacterActivity::LyingDown)
        );
        assert!(app.world().get::<MoveTarget>(resident).is_none());
        app.world_mut()
            .get_mut::<WorldTime>(clock_entity)
            .unwrap()
            .seconds_in_cycle = 10.;
        app.update();
        assert_eq!(
            app.world().get::<CharacterActivity>(resident),
            Some(&CharacterActivity::Idle)
        );
        assert!(matches!(
            app.world().get::<AmbientRoutine>(resident).unwrap().phase,
            AmbientPhase::Waiting { .. }
        ));
    }

    #[test]
    fn roadside_spots_are_beyond_both_edges_and_face_the_path() {
        let road = VillageRoad {
            settlement: "Test".to_string(),
            builder: "Ada".to_string(),
            points: vec![Vec2::ZERO, Vec2::new(12.0, 0.0)],
            built_through: 2,
            width: 2.6,
            reserved_width: shared::components::RoadClass::Lane.initial_reserved_width(),
            surface: default(),
            class: default(),
            stone_committed: 0,
        };
        let spots = raw_roadside_spots(&road);
        assert_eq!(spots.len(), 4);
        assert!(spots.iter().all(|spot| {
            (spot.point.y.abs() - (road.width * 0.5 + ROADSIDE_MARGIN)).abs() < 1e-4
        }));
        assert!(spots.iter().all(|spot| !spot.market));
        assert!(spots.iter().any(|spot| spot.point.y < 0.0));
        assert!(spots.iter().any(|spot| spot.point.y > 0.0));
        for spot in spots {
            let forward = Vec2::new(-spot.facing.sin(), -spot.facing.cos());
            assert!(forward.dot(Vec2::new(0.0, -spot.point.y).normalize()) > 0.99);
        }
    }

    #[test]
    fn intentional_leisure_prefers_a_nearby_market_gathering_spot() {
        let market = Vec2::new(4.0, 0.0);
        let spots = [
            AmbientSpot {
                point: Vec2::new(2.0, 0.0),
                facing: 0.0,
                market: false,
            },
            AmbientSpot {
                point: market,
                facing: 0.0,
                market: true,
            },
        ];

        let chosen = select_ambient_spot(0, &spots, true, |_| true)
            .expect("a marked market spot should be preferred");

        assert_eq!(chosen.point, market);
    }

    #[test]
    fn clear_local_ambient_walk_bypasses_the_global_path_queue() {
        let terrain = WorldTerrain::default();
        let spots = [AmbientSpot {
            point: Vec2::new(1_728.0, -24.0),
            facing: 0.0,
            market: false,
        }];
        let mut routine = AmbientRoutine {
            settlement: Entity::from_bits(1),
            cycle: 0,
            last_world_seconds: 0.0,
            next_world_seconds: 0.0,
            disperse_arrival: false,
            phase: AmbientPhase::Waiting { seconds_left: 0.0 },
        };

        let (_, _, _, _, direct) = choose_spot(
            "Ada",
            7,
            &mut routine,
            Vec2::new(1_700.0, -6.0),
            &spots,
            false,
            false,
            &HashSet::new(),
            &terrain,
            None,
            None,
            None,
        )
        .expect("the flat clear roadside point should be usable");

        assert!(direct);
    }

    #[test]
    fn failed_optional_walk_is_abandoned_without_stranding_the_resident() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<AmbientClock>();
        app.init_resource::<AmbientSpotCache>();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(Update, run_ambient_routines);
        app.world_mut().spawn((
            WorldTime::new_default(),
            shared::components::TimeWarp::clamped(1.0),
        ));
        let hall = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Wanderford".to_string(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 1,
                    treasury: 0,
                },
                shared::components::SettlementId(1),
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
            ))
            .id();
        let destination = Vec3::new(12.0, 0.0, 8.0);
        let resident = app
            .world_mut()
            .spawn((
                CharacterName("Ada".to_string()),
                CharacterKind::Villager,
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
                RegionCoord::default(),
                VillagerIntent::Resident { settlement: hall },
                Occupation::default(),
                CharacterActivity::Idle,
                MoveTarget(destination),
                NavigationRouteFailed { goal: destination },
                AmbientRoutine {
                    settlement: hall,
                    cycle: 1,
                    last_world_seconds: 0.0,
                    next_world_seconds: 0.0,
                    disperse_arrival: false,
                    phase: AmbientPhase::Walking {
                        destination,
                        facing: 0.0,
                        sitting: true,
                        rest_seconds: 12.0,
                        travel_seconds_left: MAX_AMBIENT_TRAVEL_SECONDS,
                        route_started: false,
                    },
                },
            ))
            .id();

        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(0.3));
        app.update();

        let resident = app.world().entity(resident);
        assert!(resident.get::<MoveTarget>().is_none());
        assert!(resident.get::<NavigationRouteFailed>().is_none());
        assert!(matches!(
            resident.get::<AmbientRoutine>().unwrap().phase,
            AmbientPhase::Waiting { .. }
        ));
    }

    #[test]
    fn saturated_town_defers_optional_walk_without_erasing_the_individual_mind() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<AmbientClock>();
        app.init_resource::<AmbientSpotCache>();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(Update, run_ambient_routines);
        app.world_mut().spawn((
            WorldTime::new_default(),
            shared::components::TimeWarp::clamped(10.0),
        ));
        let hall = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Busyford".to_string(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 1_000,
                    treasury: 0,
                },
                shared::components::SettlementId(81),
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
            ))
            .id();
        let mut load = NavigationLoad::default();
        load.ambient_by_settlement
            .insert(hall, MAX_ACTIVE_AMBIENT_ROUTES_PER_SETTLEMENT);
        app.insert_resource(load);
        let resident = app
            .world_mut()
            .spawn((
                CharacterName("Patient Ada".to_string()),
                PersonId(81),
                CharacterKind::Villager,
                PlayerPosition(Vec3::new(0.0, 0.0, -8.0)),
                PlayerRotation(0.0),
                RegionCoord::default(),
                VillagerIntent::Resident { settlement: hall },
                Occupation::default(),
                CharacterActivity::Idle,
                AmbientRoutine {
                    settlement: hall,
                    cycle: 3,
                    last_world_seconds: 0.0,
                    next_world_seconds: 0.0,
                    disperse_arrival: false,
                    phase: AmbientPhase::Waiting { seconds_left: 0.0 },
                },
            ))
            .id();

        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(1.0 / 60.0));
        app.update();

        let resident = app.world().entity(resident);
        assert!(resident.get::<MoveTarget>().is_none());
        assert!(matches!(
            resident.get::<AmbientRoutine>().unwrap().phase,
            AmbientPhase::Waiting { seconds_left } if seconds_left >= 4.0
        ));
    }

    #[test]
    fn optional_travel_timeout_starts_after_route_admission_not_while_pending() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<AmbientClock>();
        app.init_resource::<AmbientSpotCache>();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(Update, run_ambient_routines);
        app.world_mut().spawn((
            WorldTime::new_default(),
            shared::components::TimeWarp::clamped(10.0),
        ));
        let hall = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Queueford".to_string(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 1,
                    treasury: 0,
                },
                shared::components::SettlementId(82),
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
            ))
            .id();
        let destination = Vec3::new(30.0, 0.0, 8.0);
        let resident = app
            .world_mut()
            .spawn((
                CharacterName("Waiting Bea".to_string()),
                PersonId(82),
                CharacterKind::Villager,
                PlayerPosition(Vec3::new(0.0, 0.0, -8.0)),
                PlayerRotation(0.0),
                RegionCoord::default(),
                VillagerIntent::Resident { settlement: hall },
                Occupation::default(),
                CharacterActivity::Idle,
                MoveTarget(destination),
                NavigationRoutePending::new(destination),
                AmbientRoutine {
                    settlement: hall,
                    cycle: 1,
                    last_world_seconds: 0.0,
                    next_world_seconds: 0.0,
                    disperse_arrival: false,
                    phase: AmbientPhase::Walking {
                        destination,
                        facing: 0.0,
                        sitting: false,
                        rest_seconds: 8.0,
                        travel_seconds_left: MAX_AMBIENT_TRAVEL_SECONDS,
                        route_started: false,
                    },
                },
            ))
            .id();

        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(1.0));
        app.update();

        assert!(matches!(
            app.world().get::<AmbientRoutine>(resident).unwrap().phase,
            AmbientPhase::Walking {
                travel_seconds_left,
                route_started: false,
                ..
            } if travel_seconds_left == MAX_AMBIENT_TRAVEL_SECONDS
        ));
        assert!(app
            .world()
            .get::<NavigationRoutePending>(resident)
            .is_some());
    }

    #[test]
    fn one_thousand_ambient_minds_receive_individual_deadlines_at_ten_x() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<AmbientClock>();
        app.init_resource::<AmbientSpotCache>();
        app.init_resource::<AmbientDiagnostics>();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(Update, run_ambient_routines);
        app.world_mut().spawn((
            WorldTime::new_default(),
            shared::components::TimeWarp::clamped(10.0),
        ));
        let hall = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Smoothford".to_string(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 1_000,
                    treasury: 0,
                },
                shared::components::SettlementId(44),
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
            ))
            .id();
        for index in 0..1_000 {
            app.world_mut().spawn((
                CharacterName(format!("Idle resident {index}")),
                PersonId(index as u64 + 1),
                CharacterKind::Villager,
                PlayerPosition(Vec3::new(index as f32 * 0.01, 0.0, -8.0)),
                PlayerRotation(0.0),
                RegionCoord::default(),
                VillagerIntent::Resident { settlement: hall },
                Occupation::default(),
                CharacterActivity::Idle,
            ));
        }

        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(1.0 / 60.0));
        app.update();
        let deadlines: Vec<_> = app
            .world_mut()
            .query::<&AmbientRoutine>()
            .iter(app.world())
            .map(|routine| (routine.next_world_seconds * 6.0).floor() as i64)
            .collect();
        assert_eq!(deadlines.len(), 1_000);
        let distinct_deadlines = deadlines
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>();
        assert!(
            distinct_deadlines.len() >= 48,
            "stable personal deadlines collapsed into only {} update cohorts",
            distinct_deadlines.len()
        );

        // Ten world seconds at 10x. Each due mind chooses independently; no
        // old quarter-second crowd pulse and no four-person global throttle.
        for _ in 0..60 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(1.0 / 60.0));
            app.update();
        }
        let diagnostics = app.world().resource::<AmbientDiagnostics>();
        let busiest = diagnostics
            .decisions_per_pass
            .iter()
            .copied()
            .max()
            .unwrap_or(0);
        assert!(busiest > 0, "no personal ambient deadline fired");
        assert!(
            busiest < 60,
            "{busiest} minds woke together; personal deadlines visibly re-batched the crowd"
        );
    }

    #[test]
    fn newly_admitted_residents_disperse_from_the_hall_to_distinct_roadside_places() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<AmbientClock>();
        app.init_resource::<AmbientSpotCache>();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(Update, run_ambient_routines);
        app.world_mut().spawn((
            WorldTime::new_default(),
            shared::components::TimeWarp::clamped(10.0),
        ));
        let settlement_id = shared::components::SettlementId(91);
        let hall_position = {
            let terrain = app.world().resource::<WorldTerrain>();
            Vec3::new(1_700.0, terrain.get_height(1_700.0, 0.0), 0.0)
        };
        let hall = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Arrivalford".to_string(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 12,
                    treasury: 0,
                },
                settlement_id,
                PlayerPosition(hall_position),
                PlayerRotation(0.0),
            ))
            .id();
        app.world_mut().spawn((
            VillageRoad {
                settlement: "Arrivalford".to_string(),
                builder: "Test".to_string(),
                points: vec![
                    Vec2::new(1_700.0, -5.2),
                    Vec2::new(1_700.0, -24.0),
                    Vec2::new(1_728.0, -24.0),
                    Vec2::new(1_756.0, -24.0),
                ],
                built_through: 4,
                width: 2.6,
                reserved_width: shared::components::RoadClass::Lane.initial_reserved_width(),
                surface: default(),
                class: default(),
                stone_committed: 0,
            },
            shared::components::RoadOf(settlement_id),
        ));
        let residents: Vec<_> = (0..12)
            .map(|index| {
                app.world_mut()
                    .spawn((
                        CharacterName(format!("Arrival {index}")),
                        PersonId(index + 1),
                        CharacterKind::Villager,
                        PlayerPosition(Vec3::new(1_700.0, hall_position.y, -6.0)),
                        PlayerRotation(0.0),
                        RegionCoord::default(),
                        VillagerIntent::Resident { settlement: hall },
                        Occupation::default(),
                        CharacterActivity::Idle,
                    ))
                    .id()
            })
            .collect();

        // First pass observes the changed Resident intent and installs an
        // independent arrival deadline. Ten world seconds makes every one of
        // those 0.15..8.75 second deadlines due on the second pass.
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(1.0 / 60.0));
        app.update();
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(1.0));
        app.update();

        let destinations: Vec<_> = residents
            .iter()
            .filter_map(|resident| {
                app.world()
                    .get::<MoveTarget>(*resident)
                    .map(|target| target.0)
            })
            .collect();
        assert_eq!(destinations.len(), residents.len());
        assert!(residents.iter().all(|resident| {
            app.world().get::<AmbientDirectTransit>(*resident).is_some()
                && app
                    .world()
                    .get::<NavigationRoutePending>(*resident)
                    .is_none()
        }));
        assert!(residents.iter().all(|resident| {
            app.world()
                .get::<AmbientRoutine>(*resident)
                .is_some_and(|routine| {
                    routine.objective() == shared::components::CharacterObjective::SettlingIntoTown
                })
        }));
        assert!(destinations.iter().all(|destination| {
            Vec2::new(destination.x, destination.z).distance(Vec2::new(1_700.0, -6.0))
                >= ARRIVAL_DISPERSAL_MIN_DISTANCE
        }));
        for (index, destination) in destinations.iter().enumerate() {
            let cell = ambient_occupancy_cell(Vec2::new(destination.x, destination.z));
            assert!(destinations[index + 1..]
                .iter()
                .all(|other| { ambient_occupancy_cell(Vec2::new(other.x, other.z)) != cell }));
        }
    }

    #[test]
    fn unhoused_moot_steward_stays_indoors_instead_of_flickering_ambiently() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<AmbientClock>();
        app.init_resource::<AmbientSpotCache>();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(Update, run_ambient_routines);
        app.world_mut().spawn((
            WorldTime::new_default(),
            shared::components::TimeWarp::clamped(1.0),
        ));
        let hall = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Stewardstead".to_string(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 1,
                    treasury: 0,
                },
                shared::components::SettlementId(2),
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
            ))
            .id();
        let steward = app
            .world_mut()
            .spawn((
                CharacterName("Hilda".to_string()),
                CharacterKind::Villager,
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
                RegionCoord::default(),
                VillagerIntent::Resident { settlement: hall },
                Occupation(Some("Moot Steward".to_string())),
                CharacterActivity::Indoors,
                MootSteward { settlement: hall },
            ))
            .id();

        // Two ambient wakes reproduce the old failure: the first attached an
        // ambient routine and the second changed Indoors to Idle for one tick.
        for _ in 0..2 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(0.3));
            app.update();
        }

        let steward = app.world().entity(steward);
        assert_eq!(
            steward.get::<CharacterActivity>(),
            Some(&CharacterActivity::Indoors)
        );
        assert!(steward.get::<AmbientRoutine>().is_none());
        assert!(steward.get::<MoveTarget>().is_none());
    }

    #[test]
    fn five_thousand_unobserved_residents_receive_no_ambient_work() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<AmbientClock>();
        app.init_resource::<AmbientSpotCache>();
        app.init_resource::<RegionRegistry>();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(Update, run_ambient_routines);
        app.world_mut().spawn((
            WorldTime::new_default(),
            shared::components::TimeWarp::clamped(1.0),
        ));
        let hall = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Quiet".to_string(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 1,
                    treasury: 0,
                },
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
            ))
            .id();
        let villagers: Vec<_> = (0..5_000)
            .map(|index| {
                app.world_mut()
                    .spawn((
                        CharacterName(format!("Resident{index}")),
                        CharacterKind::Villager,
                        PlayerPosition(Vec3::new(0.0, 0.0, -8.0)),
                        PlayerRotation(0.0),
                        RegionCoord::default(),
                        VillagerIntent::Resident { settlement: hall },
                        Occupation::default(),
                        CharacterActivity::Idle,
                    ))
                    .id()
            })
            .collect();

        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(0.3));
        app.update();
        assert!(villagers.iter().all(|villager| {
            let resident = app.world().entity(*villager);
            !resident.contains::<AmbientRoutine>() && !resident.contains::<MoveTarget>()
        }));
    }
}
