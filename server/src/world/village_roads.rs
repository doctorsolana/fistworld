//! Builder-made village paths and the tiny movement graph they create.
//!
//! An obstacle-aware grid search is paid once, when a building completes. The
//! resulting polyline is then shared by rendering, prop clearance and cached
//! villager routes. A new endpoint pair pays one bounded local survey suite,
//! then follows cheap waypoints and shared road-graph paths; repeated commutes
//! reuse the complete obstacle-versioned route in either certified direction.

mod construction;
mod geometry;
mod routing;
mod steward;

pub(crate) use construction::hall_connected_road_keys;
use construction::{
    absolute_world_seconds, building_road_status, hall_road_network, BuildingRoadStatus,
};
pub use construction::{build_village_roads, plan_requested_roads};
use routing::graph_key;
pub use routing::{
    plan_villager_travel_routes, queue_villager_travel_routes, rebuild_village_road_graph,
    retry_failed_routes_after_obstacle_change, VillageRoadGraph,
};

pub(crate) fn road_point_key(point: Vec2) -> (i32, i32) {
    graph_key(point)
}
#[cfg(test)]
use routing::{reverse_route_clears_goal_prop_exemption, RoadGraphNode};
#[cfg(test)]
pub use steward::staff_moot_stewards as staff_and_pay_road_stewards;
pub use steward::{
    audit_village_roads, ensure_moot_administrations, staff_moot_stewards, staff_public_positions,
};

use bevy::ecs::system::SystemParam;
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use lightyear::prelude::{NetworkTarget, Replicate};
use shared::building::{BuildingPosition, BuildingType, PlacedBuilding};
use shared::components::{
    BuildingDoorUse, CharacterActivity, CharacterKind, CharacterName, CharacterObjective,
    FarmField, MootAdministration, Occupation, PlayerPosition, PlayerRotation, RoadClass,
    RoadSurface, Settlement, SettlementBuilding, SettlementBuildingKind, VillageRoad, WorkStatus,
    WorldTime,
};
#[cfg(test)]
use shared::economy::Wallet;
use shared::economy::ROAD_STEWARD_DAILY_SALARY;
use shared::spatial::SpatialObstacleGrid;
use shared::terrain::{world_pos_in_bounds, ChunkCoord, WorldTerrain, CHUNK_SIZE};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, VecDeque};
use std::time::{Duration, Instant};

use crate::player::hero::MoveTarget;
use crate::world::navgrid::{NAVIGATION_SAMPLE_STEP, VILLAGER_NAV_RADIUS, VILLAGER_PROP_RADIUS};
use crate::world::village::{
    ambient::AmbientRoutine, FarmerRoutine, FishingRoutine, HomeRoutine, HouseholdShoppingRoutine,
    LumberjackRoutine, MarketCollectionRoutine, MootQueueTicket, PierTraversal, UnderConstruction,
    VillagerIntent, CHOP_SECONDS,
};
use crate::{
    collision::library::{DerivedColliderLibrary, StaticColliders},
    world::pathfinding::PathfindingBudgetSettings,
};

pub(crate) use geometry::{
    road_corridor_is_dry, road_sample_is_dry, road_segment_is_coarsely_dry,
    road_segment_is_coarsely_dry_at_width, road_segment_is_dry, road_segment_is_dry_at_width,
    surface_width_for_tier,
};

/// Whether a completed building's own connector belongs to the component
/// rooted at the Moot Hall door. Production jobs use this before sending an
/// embodied worker across town; a merely complete but detached lane is still
/// a Road Steward problem, not an operational commute.
pub(crate) fn building_has_connected_road(
    kind: SettlementBuildingKind,
    position: Vec3,
    rotation: f32,
    hall_position: Vec3,
    hall_rotation: f32,
    roads: &[&VillageRoad],
) -> bool {
    let door3 = kind.entrance_position(position, rotation);
    let door = Vec2::new(door3.x, door3.z);
    let hall3 = SettlementBuildingKind::Hall.entrance_position(hall_position, hall_rotation);
    let network = hall_road_network(Vec2::new(hall3.x, hall3.z), roads);
    building_road_status(door, roads, &network, false) == BuildingRoadStatus::Connected
}

/// The live collision sources used by tactical travel planning. They must be
/// read together: buildings alone are not enough when a doorway or connector
/// is blocked by a streamed tree or rock.
#[derive(SystemParam)]
pub struct TravelCollisionResources<'w> {
    obstacles: Option<Res<'w, SpatialObstacleGrid>>,
    colliders: Option<Res<'w, StaticColliders>>,
    derived: Option<Res<'w, DerivedColliderLibrary>>,
}

pub const VILLAGE_ROAD_WIDTH: f32 = 2.6;
const SURVEY_CELL: f32 = 1.5;
/// Cross-settlement caravans need a strategic corridor, not a fine town-scale
/// search across the whole space between two settlements. Four ordinary cells
/// make a six-metre middle-country step. The search returns to fine cells near
/// both halls so a crowded doorway cannot trap the coarse lattice.
const INTERSETTLEMENT_SURVEY_STRIDE: i32 = 4;
const INTERSETTLEMENT_FINE_ENDPOINT_RADIUS: f32 = 36.0;
/// Rivers, lakes and steep foothills can require a caravan to leave the
/// straight-line town-to-town band. This is deliberately much wider than a
/// local commute but remains finite; together with the coarse middle lattice
/// it covers at most a bounded regional corridor rather than the whole map.
const INTERSETTLEMENT_SURVEY_PADDING: f32 = 192.0;
const SURVEY_PADDING: f32 = 20.0;
const SURVEY_MAX_NODES: usize = 12_000;
/// Embodied villagers only travel locally. A failed temporary route must not
/// spend the road-construction survey's much larger budget searching an
/// unreachable destination in a dense settlement.
// Temporary actor routes are disposable: their caller retries after geometry
// changes. This stays far below the 12,000-cell road survey while leaving
// enough room to skirt a dense local prop/building cluster.
const AGENT_SURVEY_MAX_NODES: usize = 400;
/// Long but still local trips (outer plots, homes and the Moot Hall) need more
/// room to detour than a walk across one street. The larger search is selected
/// by distance, remains bounded, and is never used for cross-world migration;
/// that preserves the cheap common case while avoiding false failures on the
/// settlement's outer envelope.
const EXTENDED_LOCAL_SURVEY_MAX_NODES: usize = 2_400;
/// A company caravan is the one embodied actor which deliberately crosses
/// between otherwise disconnected settlement road networks. Its route is
/// still collision-certified and solved incrementally under the ordinary
/// per-tick pathfinding budget, but may inspect a wider bounded corridor than
/// a local commute. Ordinary villagers retain the 2,400-node cap.
const INTERSETTLEMENT_TRADE_SURVEY_MAX_NODES: usize = 24_000;
/// A retained long-distance A* must make enough progress that one difficult
/// commute cannot remain at zero expansion merely because preparation spent
/// this tick's deadline. Eight cells keep that overrun bounded; the fair
/// cursor rotates retained jobs, while transaction-specific failure recovery
/// remains the owning routine's responsibility.
const MIN_INCREMENTAL_ROUTE_CELLS_PER_SLICE: usize = 8;
const EXTENDED_LOCAL_SURVEY_MIN_DISTANCE: f32 = 48.0;
// Mature settlements can legitimately place an outer workplace more than
// 200 m from a resident's cabin. Keep a finite direct-search envelope for
// broken or cross-world destinations, but do not misclassify ordinary town
// commutes as the cheap 400-node case.
const EXTENDED_LOCAL_SURVEY_MAX_DISTANCE: f32 = 512.0;
const ROAD_BUILD_SECONDS: f32 = 0.55;
const ROAD_REACH: f32 = 0.45;
/// Surveyed road points are spaced about two metres apart. Generic actor
/// routing can reject an exact point on the inflated edge of a source
/// building even after bringing the worker beside it. At that distance the
/// road segment is physically within the worker's construction area and must
/// not trigger an identical destroy/resurvey loop.
const ROAD_FAILED_WAYPOINT_WORK_REACH: f32 = 3.0;
/// Civic inspections are frequent enough to recover a stranded connector in
/// the same play session, but remain settlement-level work rather than a scan
/// performed for every villager on every frame.
const ROAD_AUDIT_INTERVAL_SECONDS: f64 = 60.0;
/// A connector whose owner neither moves nor completes another point for this
/// much daylight is no longer "actively under construction". Five world
/// minutes comfortably covers an ordinary village commute and route queue,
/// while preventing one orphaned routine from fooling the steward for days.
const ROAD_BUILDER_STALL_SECONDS: f64 = 300.0;
/// A builder gets several deterministic survey variants before releasing the
/// site to the public repair queue. This bounds both route work and failure
/// latency when a doorway has genuinely become inaccessible.
const MAX_ROAD_SURVEY_ATTEMPTS: u8 = 8;
/// Failed road surveys are CPU work, so their retry clock is deliberately real
/// time rather than warped world time. At 100x, a world-minute delay would
/// still wake the same impossible house every 0.6 seconds.
const ROAD_SURVEY_RETRY_BASE_SECONDS: f64 = 0.5;
const ROAD_SURVEY_RETRY_MAX_SECONDS: f64 = 30.0;
const ROAD_ROUTE_JOIN_DISTANCE: f32 = 32.0;
const ROAD_ROUTE_CANDIDATES: usize = 4;
/// Search beyond the four geometrically closest road nodes before ranking
/// usable connector pairs. A resident can stand among several short cabin
/// spurs which are still under construction; those disconnected nodes must
/// not hide the completed street only a few metres farther away.
const ROAD_ROUTE_CANDIDATE_POOL: usize = 16;
/// Connector surveys are full terrain A* searches. Rank several graph options
/// cheaply, but only survey the best couple so one unreachable villager cannot
/// monopolise a server tick trying every road combination.
const AGENT_ROAD_CANDIDATES_TO_SURVEY: usize = 8;
const ROAD_MAX_WEIGHTED_DETOUR: f32 = 1.35;
/// A failed embodied route is deterministic until the road/building/prop
/// geometry or destination changes. Keep retries on real time so a work
/// routine reasserting the same doorway cannot turn one bad plot into a
/// pathfinding request every server tick.
const AGENT_ROUTE_RETRY_BASE_SECONDS: f64 = 1.0;
const AGENT_ROUTE_RETRY_MAX_SECONDS: f64 = 30.0;
/// Interior threshold points sit 1.35 m behind an authored door and the
/// movement reach tolerance can leave a few additional centimetres. Keep the
/// recovery band large enough to recognize that legitimate interior state,
/// but far too small to grant an arbitrary actor passage across a building.
const NAVIGATION_DOOR_RECOVERY_DISTANCE: f32 = 2.0;
/// Grid samples must not ride the exact edge of a continuous field/ribbon
/// intersection. This tiny cushion absorbs rounding without visibly widening
/// the requested crop clearance.
const ROAD_SURVEY_FIELD_EPSILON: f32 = 0.25;
/// Authored entrances sit just outside the wall. Surveying begins beyond a
/// short front apron so A* cannot approach the same door through a side or the
/// rear of the building.
const DOOR_APPROACH_LENGTH: f32 = 2.25;
pub const ROAD_SPEED_MULTIPLIER: f32 = 1.22;

pub(crate) fn doorway_road_apron_is_dry(
    terrain: &WorldTerrain,
    kind: SettlementBuildingKind,
    position: Vec3,
    rotation: f32,
) -> bool {
    let (door, approach) = doorway_approach(kind, position, rotation);
    road_segment_is_dry_at_width(
        terrain,
        door,
        approach,
        RoadClass::Lane.initial_reserved_width(),
    )
}

/// A permit must not put a permanent doorway apron through a rock.
///
/// Buildings clear props inside their own authored footprint, but the road
/// apron begins outside that footprint. Trees are legal only because the road
/// crew now performs a visible chopping job before laying that section; rocks
/// remain an immutable obstruction.
pub(crate) fn doorway_road_apron_is_clear_of_props(
    kind: SettlementBuildingKind,
    position: Vec3,
    rotation: f32,
    colliders: &StaticColliders,
    derived: &DerivedColliderLibrary,
) -> bool {
    let (door, approach) = doorway_approach(kind, position, rotation);
    let road_half_width = RoadClass::Lane.initial_reserved_width() * 0.5;
    !static_collider_overlaps_segment_filtered(
        colliders,
        derived,
        door,
        approach,
        road_half_width + 0.2,
        false,
    )
}

/// Building ground reserved at permit time is not enough: the authored door
/// also needs a narrow, dry route to the existing public network. This
/// server-only claim survives construction and remains on the completed shell
/// until its real [`VillageRoad`] is physically complete. Later permits treat
/// it as occupied access space, preventing a simultaneous batch of cabins
/// from boxing in one another's entrances while the road is only surveyed or
/// partially built.
#[derive(Component, Debug, Clone)]
pub(crate) struct PlannedRoadAccess {
    pub(crate) settlement_id: shared::components::SettlementId,
    pub(crate) points: Vec<Vec2>,
    pub(crate) half_width: f32,
}

/// Keeps a surveyed-but-unfinished connector tied to the building whose
/// permit-time access corridor it is replacing. The old reservation remains
/// authoritative until this road is physically complete; if embodied
/// construction rejects the survey and despawns the road, later permits still
/// cannot box in the doorway before the retry.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct RoadConnectorFor {
    pub(crate) building: Entity,
}

/// A completed building which the latest civic audit found roadless or on a
/// detached component while every physical steward was occupied.
///
/// This is an explicit deterministic backlog claim, not just a panel counter. It lets
/// diagnostics distinguish a deliberately queued repair from a building that
/// silently lost its request. The next free audit takes detached components
/// before roadless plots, then the oldest stable `BuildingId` in that class.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct RoadRepairBacklog;

impl PlannedRoadAccess {
    pub(crate) fn intersects_circle(&self, center: Vec2, radius: f32) -> bool {
        let clearance = radius + self.half_width;
        self.points.windows(2).any(|segment| {
            point_segment_distance_squared(center, segment[0], segment[1]) <= clearance * clearance
        })
    }
}

/// Attached to a newly completed building until its original builder surveys
/// and adopts the door-to-network path.
#[derive(Component, Debug, Clone, Copy)]
pub struct RoadRequest {
    pub builder: Entity,
    pub settlement: Entity,
    /// The construction site this request finishes. If the retained request
    /// is retried after its builder has accepted a different permit, it must
    /// wait rather than stealing that person from the newer worksite.
    pub completed_site: Entity,
    /// Deterministic alternate-survey salt after embodied navigation rejected
    /// an otherwise geometrically valid path.
    pub attempt: u8,
}

/// Real-time backoff for a completed building whose connector cannot currently
/// be surveyed. Kept separately from [`RoadRequest::attempt`]: that counter
/// describes embodied failures on an accepted survey, while this one prevents
/// an impossible survey from consuming a full search and warning every frame.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct RoadSurveyBackoff {
    failures: u8,
    retry_after: f64,
}

impl RoadSurveyBackoff {
    pub(crate) fn after_failure(previous: Option<Self>, now: f64) -> Self {
        let failures = previous.map_or(1, |state| state.failures.saturating_add(1));
        let exponent = u32::from(failures.saturating_sub(1)).min(10);
        let delay = (ROAD_SURVEY_RETRY_BASE_SECONDS * 2_f64.powi(exponent as i32))
            .min(ROAD_SURVEY_RETRY_MAX_SECONDS);
        Self {
            failures,
            retry_after: now + delay,
        }
    }

    fn should_warn(self) -> bool {
        self.failures == 1 || self.failures.is_power_of_two()
    }
}

#[derive(Component, Debug, Clone, Copy)]
pub struct RoadBuilderRoutine {
    pub road: Entity,
    pub settlement: Entity,
    attempt: u8,
    phase: RoadBuildPhase,
}

/// Marks the one resident holding the Moot Hall's public road position.
///
/// This server-only identity prevents the ordinary permit and vacancy systems
/// from treating the steward as unemployed while they are between audits.
#[derive(Component, Debug, Clone, Copy)]
pub struct RoadSteward {
    pub settlement: Entity,
}

#[derive(Component, Debug, Clone)]
pub(crate) struct MootAdministrationRuntime {
    last_audit_at: Option<f64>,
    road_progress: HashMap<Entity, RoadProgressObservation>,
}

#[derive(Debug, Clone, Copy)]
struct RoadProgressObservation {
    built_through: u16,
    builder_position: Vec2,
    last_progress_at: f64,
}

#[derive(Debug, Clone, Copy)]
enum RoadBuildPhase {
    GoingTo {
        point: usize,
    },
    GoingToTree {
        point: usize,
        tree: Vec2,
        stand: Vec2,
        radius: f32,
        approach: u8,
    },
    ChoppingTree {
        point: usize,
        tree: Vec2,
        radius: f32,
        seconds_left: f32,
    },
    Working {
        point: usize,
        seconds_left: f32,
    },
}

#[derive(Clone, Copy, Debug)]
struct RoadTreeObstruction {
    point: Vec2,
    radius: f32,
}

/// Trees intersecting an accepted road ribbon, ordered from its source door
/// toward the public network. This stays server-only: clients remove the
/// matching visual prop as each built road prefix reaches it.
#[derive(Component, Debug, Default)]
pub(crate) struct RoadTreeClearancePlan {
    trees: Vec<RoadTreeObstruction>,
}

impl RoadBuilderRoutine {
    /// Night sends a builder home. Morning resumes from the last completed
    /// point, never from a half-finished timer beside their bed.
    pub fn restart_from_built_road(&mut self, road: &VillageRoad) {
        let last_built = usize::from(road.built_through)
            .min(road.points.len())
            .saturating_sub(1);
        self.phase = RoadBuildPhase::GoingTo { point: last_built };
    }

    pub(crate) const fn objective(&self) -> shared::components::CharacterObjective {
        use shared::components::CharacterObjective;
        match self.phase {
            RoadBuildPhase::GoingToTree { .. } | RoadBuildPhase::ChoppingTree { .. } => {
                CharacterObjective::ClearingRoadTree
            }
            RoadBuildPhase::GoingTo { .. } | RoadBuildPhase::Working { .. } => {
                CharacterObjective::BuildingRoad
            }
        }
    }
}

/// Server-only route followed by [`crate::player::hero::step_units`].
#[derive(Component, Debug, Clone)]
pub struct TravelRoute {
    pub goal: Vec3,
    pub waypoints: Vec<RouteWaypoint>,
    pub next: usize,
}

/// A changed destination waits here until its one-time route is ready. Units
/// with this component do not take even one speculative straight-line step,
/// which makes collision avoidance an invariant rather than a visual hope.
#[derive(Component, Debug, Clone, Copy)]
pub struct NavigationRoutePending {
    pub goal: Vec3,
    attempts: u8,
}

/// The bounded planner could not certify a route to this destination.
///
/// This is an AI result, not merely a warning. Work routines can choose a
/// different tree or interaction point instead of waiting forever for an
/// unrelated building obstacle version to change.
#[derive(Component, Debug, Clone, Copy)]
pub struct NavigationRouteFailed {
    pub goal: Vec3,
}

/// One-route permission to leave a building footprint through its certified
/// doorway apron.
///
/// This is recovery state, not ordinary noclip. It is installed only when a
/// route begins just inside an authored door (for example when a completed
/// building's obstacle expands around its builder) and movement removes it as
/// soon as the actor reaches clear ground.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct NavigationObstacleEscape;

/// Persistent circuit breaker for an embodied route failure.
///
/// High-level routines may consume [`NavigationRouteFailed`] while preserving
/// their work transaction or carried goods. This separate component retains
/// the expensive negative result, preventing that routine from immediately
/// launching the identical A* again. It is removed after a successful route
/// or a genuinely different destination.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct NavigationRouteBackoff {
    goal: Vec3,
    failures: u8,
    retry_after: f64,
    /// Building and prop collision truth. Road growth is tracked separately:
    /// a new walkable ribbon can create an opportunity, but cannot make a
    /// previously certified route unsafe.
    geometry_version: u64,
    /// Revision of road points close enough to affect either route endpoint.
    road_opportunity_version: u64,
}

impl NavigationRouteBackoff {
    fn after_failure(
        previous: Option<Self>,
        goal: Vec3,
        geometry_version: u64,
        road_opportunity_version: u64,
        now: f64,
        entity: Entity,
    ) -> Self {
        let failures = previous
            .filter(|state| {
                state.goal.distance_squared(goal) <= 0.01
                    && state.geometry_version == geometry_version
                    && state.road_opportunity_version == road_opportunity_version
            })
            .map_or(1, |state| state.failures.saturating_add(1));
        let exponent = u32::from(failures.saturating_sub(1)).min(10);
        let delay = (AGENT_ROUTE_RETRY_BASE_SECONDS * 2_f64.powi(exponent as i32))
            .min(AGENT_ROUTE_RETRY_MAX_SECONDS);
        // A burst of immigrants often shares one destination. Stable jitter
        // prevents all failed routes from waking on the same future tick.
        let hash = entity.to_bits().wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let jitter = 0.75 + (hash & 1023) as f64 / 1023.0 * 0.5;
        Self {
            goal,
            failures,
            retry_after: now + delay * jitter,
            geometry_version,
            road_opportunity_version,
        }
    }

    fn matches(self, goal: Vec3) -> bool {
        self.goal.distance_squared(goal) <= 0.01
    }

    fn should_warn(self) -> bool {
        self.failures == 1 || self.failures.is_power_of_two()
    }
}

impl NavigationRoutePending {
    pub fn new(goal: Vec3) -> Self {
        Self { goal, attempts: 0 }
    }

    pub fn exhausted(&self) -> bool {
        self.attempts >= 3
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RouteWaypoint {
    pub position: Vec3,
    pub on_road: bool,
}

#[derive(Clone, Copy)]
struct BuildingBlocker {
    center: Vec2,
    half: Vec2,
    rotation: f32,
}

#[derive(Clone, Copy)]
struct CachedRouteProp {
    point: Vec2,
    kind: shared::props::PropKind,
    scale: f32,
}

/// Authored prop generation is deterministic but not cheap (river clearance
/// alone scans nearby river segments). Cache the collision-relevant result per
/// chunk for the lifetime of each planning system instead of regenerating the
/// same forest for every villager and every road candidate.
#[derive(Default)]
pub(crate) struct RoutePropChunkCache {
    chunks: HashMap<ChunkCoord, Vec<CachedRouteProp>>,
}

impl RoutePropChunkCache {
    fn chunk(&mut self, terrain: &WorldTerrain, chunk: ChunkCoord) -> &[CachedRouteProp] {
        self.chunks
            .entry(chunk)
            .or_insert_with(|| {
                shared::props::generate_chunk_blocking_props(&terrain.generator, chunk)
                    .into_iter()
                    .map(|spawn| CachedRouteProp {
                        point: spawn.position,
                        kind: spawn.kind,
                        scale: spawn.scale,
                    })
                    .collect()
            })
            .as_slice()
    }
}

impl BuildingBlocker {
    fn contains(&self, point: Vec2) -> bool {
        let local = shared::rotation::world_to_local_xz(point - self.center, self.rotation);
        local.x.abs() <= self.half.x && local.y.abs() <= self.half.y
    }

    fn blocks_segment(&self, start: Vec2, end: Vec2) -> bool {
        shared::spatial::segment_intersects_box_after_start(
            shared::rotation::world_to_local_xz(start - self.center, self.rotation),
            shared::rotation::world_to_local_xz(end - self.center, self.rotation),
            self.half,
        )
    }
}

#[derive(Default)]
struct PropBlockers {
    cells: HashMap<(i32, i32), Vec<PropBlocker>>,
    max_radius: f32,
}

#[derive(Clone, Copy)]
struct PropBlocker {
    point: Vec2,
    radius: f32,
}

impl PropBlockers {
    const CELL: f32 = 6.0;
    const CLEARANCE: f32 = 2.35;

    fn key(point: Vec2) -> (i32, i32) {
        (
            (point.x / Self::CELL).floor() as i32,
            (point.y / Self::CELL).floor() as i32,
        )
    }

    fn insert_radius(&mut self, point: Vec2, radius: f32) {
        self.max_radius = self.max_radius.max(radius);
        self.cells
            .entry(Self::key(point))
            .or_default()
            .push(PropBlocker { point, radius });
    }

    fn blocks(&self, point: Vec2) -> bool {
        let cell = Self::key(point);
        let cells = (self.max_radius / Self::CELL).ceil().max(1.0) as i32;
        for dx in -cells..=cells {
            for dz in -cells..=cells {
                let Some(points) = self.cells.get(&(cell.0 + dx, cell.1 + dz)) else {
                    continue;
                };
                if points.iter().any(|obstacle| {
                    obstacle.point.distance_squared(point) < obstacle.radius.powi(2)
                }) {
                    return true;
                }
            }
        }
        false
    }
}

struct RoadSurvey<'a> {
    terrain: &'a WorldTerrain,
    buildings: &'a [BuildingBlocker],
    live_buildings: Option<&'a SpatialObstacleGrid>,
    props: &'a PropBlockers,
    start: Vec2,
    goal: Vec2,
    min: Vec2,
    max: Vec2,
    max_nodes: usize,
    cell_size: f32,
    coarse_stride: i32,
    fine_endpoint_radius: f32,
}

#[derive(Default, Clone, Copy)]
struct SurveyMetrics {
    searches: u64,
    expanded_nodes: u64,
    blocked_checks: u64,
    blocked_cache_hits: u64,
    line_checks: u64,
    line_cache_hits: u64,
}

impl std::ops::Sub for SurveyMetrics {
    type Output = Self;

    fn sub(self, earlier: Self) -> Self::Output {
        Self {
            searches: self.searches.saturating_sub(earlier.searches),
            expanded_nodes: self.expanded_nodes.saturating_sub(earlier.expanded_nodes),
            blocked_checks: self.blocked_checks.saturating_sub(earlier.blocked_checks),
            blocked_cache_hits: self
                .blocked_cache_hits
                .saturating_sub(earlier.blocked_cache_hits),
            line_checks: self.line_checks.saturating_sub(earlier.line_checks),
            line_cache_hits: self.line_cache_hits.saturating_sub(earlier.line_cache_hits),
        }
    }
}

type SurveyPointKey = (u32, u32);
type SurveyLineKey = (SurveyPointKey, SurveyPointKey);

/// Reusable memory and per-search geometry memoization for the local survey.
///
/// A difficult route used to allocate four hash collections for every direct
/// or road-connector attempt, then repeatedly sample the same cell edges.
/// Clearing retains those allocations, while the point and line caches are
/// deliberately scoped to one survey because endpoint exemptions differ.
#[derive(Default)]
pub(crate) struct SurveyScratch {
    open: BinaryHeap<SurveyOpen>,
    came_from: HashMap<SurveyCell, SurveyCell>,
    score: HashMap<SurveyCell, f32>,
    closed: HashSet<SurveyCell>,
    blocked: HashMap<SurveyPointKey, bool>,
    heights: HashMap<SurveyPointKey, f32>,
    lines: HashMap<SurveyLineKey, bool>,
    metrics: SurveyMetrics,
}

impl SurveyScratch {
    fn begin_search(&mut self) {
        self.open.clear();
        self.came_from.clear();
        self.score.clear();
        self.closed.clear();
        self.blocked.clear();
        self.heights.clear();
        self.lines.clear();
        self.metrics.searches = self.metrics.searches.saturating_add(1);
    }

    fn point_key(point: Vec2) -> SurveyPointKey {
        (point.x.to_bits(), point.y.to_bits())
    }

    fn line_key(start: Vec2, end: Vec2) -> SurveyLineKey {
        let start = Self::point_key(start);
        let end = Self::point_key(end);
        if start <= end {
            (start, end)
        } else {
            (end, start)
        }
    }
}

impl RoadSurvey<'_> {
    fn stride_at(&self, point: Vec2) -> i32 {
        if self.coarse_stride <= 1
            || point.distance_squared(self.start) <= self.fine_endpoint_radius.powi(2)
            || point.distance_squared(self.goal) <= self.fine_endpoint_radius.powi(2)
        {
            1
        } else {
            self.coarse_stride
        }
    }

    fn blocked(&self, point: Vec2, scratch: &mut SurveyScratch) -> bool {
        scratch.metrics.blocked_checks = scratch.metrics.blocked_checks.saturating_add(1);
        let key = SurveyScratch::point_key(point);
        if let Some(blocked) = scratch.blocked.get(&key).copied() {
            scratch.metrics.blocked_cache_hits =
                scratch.metrics.blocked_cache_hits.saturating_add(1);
            return blocked;
        }
        let blocked = if point.x < self.min.x
            || point.y < self.min.y
            || point.x > self.max.x
            || point.y > self.max.y
            || !road_sample_is_dry(self.terrain, point)
        {
            true
        } else {
            // Props may overlap a chosen chopping/interaction target, so the
            // goal gets a small exemption: the job routine begins work once it
            // reaches interaction range. Buildings never receive exemptions.
            let endpoint_clear = point.distance_squared(self.start)
                <= (NAVIGATION_SAMPLE_STEP * 0.25).powi(2)
                || point.distance_squared(self.goal) < 2.0f32.powi(2);
            let building_blocked = self.live_buildings.map_or_else(
                || self.buildings.iter().any(|blocker| blocker.contains(point)),
                |grid| grid.point_blocked(point),
            );
            building_blocked || (!endpoint_clear && self.props.blocks(point))
        };
        scratch.blocked.insert(key, blocked);
        blocked
    }

    fn height(&self, point: Vec2, scratch: &mut SurveyScratch) -> f32 {
        let key = SurveyScratch::point_key(point);
        if let Some(height) = scratch.heights.get(&key).copied() {
            return height;
        }
        let height = self.terrain.get_height(point.x, point.y);
        scratch.heights.insert(key, height);
        height
    }

    fn line_clear(&self, start: Vec2, end: Vec2, scratch: &mut SurveyScratch) -> bool {
        scratch.metrics.line_checks = scratch.metrics.line_checks.saturating_add(1);
        let key = SurveyScratch::line_key(start, end);
        if let Some(clear) = scratch.lines.get(&key).copied() {
            scratch.metrics.line_cache_hits = scratch.metrics.line_cache_hits.saturating_add(1);
            return clear;
        }
        // Movement certifies full segments against exact rotated building
        // geometry. Do the same here before the terrain/prop samples below;
        // otherwise A* can repeatedly choose a leg whose sample points happen
        // to straddle a thin corner that movement correctly rejects.
        let building_blocked = self.live_buildings.map_or_else(
            || {
                self.buildings
                    .iter()
                    .any(|blocker| blocker.blocks_segment(start, end))
            },
            |grid| grid.segment_blocked(start, end),
        );
        if building_blocked {
            scratch.lines.insert(key, false);
            return false;
        }
        let length = start.distance(end);
        let steps = (length / NAVIGATION_SAMPLE_STEP).ceil().max(1.0) as usize;
        let mut previous_height = None;
        let mut clear = true;
        for step in 0..=steps {
            let t = step as f32 / steps as f32;
            let point = start.lerp(end, t);
            if self.blocked(point, scratch) {
                clear = false;
                break;
            }
            let height = self.height(point, scratch);
            let step_clear =
                previous_height.is_none_or(|previous: f32| (height - previous).abs() <= 0.47);
            previous_height = Some(height);
            if !step_clear {
                clear = false;
                break;
            }
        }
        scratch.lines.insert(key, clear);
        clear
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct SurveyCell {
    x: i32,
    z: i32,
}

#[derive(Clone, Copy, Debug)]
struct SurveyOpen {
    cost: i32,
    cell: SurveyCell,
}

#[derive(Default)]
struct SurveySearchState {
    initialized: bool,
    expanded: usize,
}

enum SurveySearchResult {
    Pending,
    Found(Vec<Vec2>),
    Failed,
}

impl Eq for SurveyOpen {}
impl PartialEq for SurveyOpen {
    fn eq(&self, other: &Self) -> bool {
        self.cost == other.cost && self.cell == other.cell
    }
}
impl Ord for SurveyOpen {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .cmp(&self.cost)
            .then_with(|| self.cell.x.cmp(&other.cell.x))
            .then_with(|| self.cell.z.cmp(&other.cell.z))
    }
}
impl PartialOrd for SurveyOpen {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn survey_cell(point: Vec2, cell_size: f32) -> SurveyCell {
    SurveyCell {
        x: (point.x / cell_size).round() as i32,
        z: (point.y / cell_size).round() as i32,
    }
}

fn survey_point(cell: SurveyCell, cell_size: f32) -> Vec2 {
    Vec2::new(cell.x as f32 * cell_size, cell.z as f32 * cell_size)
}

fn survey_heuristic(a: SurveyCell, b: SurveyCell) -> f32 {
    Vec2::new((a.x - b.x) as f32, (a.z - b.z) as f32).length()
}

fn resume_survey_a_star(
    survey: &RoadSurvey<'_>,
    scratch: &mut SurveyScratch,
    state: &mut SurveySearchState,
    deadline: Option<Instant>,
) -> SurveySearchResult {
    let start = survey_cell(survey.start, survey.cell_size);
    let goal = survey_cell(survey.goal, survey.cell_size);
    if !state.initialized {
        scratch.begin_search();
        scratch.score.insert(start, 0.0);
        scratch.open.push(SurveyOpen {
            cost: (survey_heuristic(start, goal) * 1000.0) as i32,
            cell: start,
        });
        state.initialized = true;
        state.expanded = 0;
    }

    let mut expanded_this_slice = 0usize;
    loop {
        // A long tactical search yields after a small guaranteed slice. One
        // cell per tick made a worst-case 2,400-cell proof hold the priority
        // lane for forty seconds, leaving an entire town waiting at its doors.
        if expanded_this_slice >= MIN_INCREMENTAL_ROUTE_CELLS_PER_SLICE
            && deadline.is_some_and(|deadline| Instant::now() >= deadline)
        {
            return SurveySearchResult::Pending;
        }
        let Some(SurveyOpen { cell: current, .. }) = scratch.open.pop() else {
            return SurveySearchResult::Failed;
        };
        if !scratch.closed.insert(current) {
            continue;
        }
        if state.expanded >= survey.max_nodes {
            return SurveySearchResult::Failed;
        }
        state.expanded += 1;
        expanded_this_slice += 1;
        scratch.metrics.expanded_nodes = scratch.metrics.expanded_nodes.saturating_add(1);
        let current_point = if current == start {
            survey.start
        } else {
            survey_point(current, survey.cell_size)
        };
        // The exact endpoint rarely lies at its rounded grid-cell centre. A
        // nearby cell with a certified final edge is a valid virtual goal and
        // avoids forcing a short corner-cut from the rounded goal cell.
        let reaches_goal = current_point.distance(survey.goal) <= survey.cell_size * 1.6
            && survey.line_clear(current_point, survey.goal, scratch);
        if reaches_goal {
            let mut cells = vec![current];
            let mut cursor = current;
            while let Some(previous) = scratch.came_from.get(&cursor).copied() {
                cells.push(previous);
                cursor = previous;
            }
            cells.reverse();
            let mut points = Vec::with_capacity(cells.len() + 2);
            points.push(survey.start);
            points.extend(
                cells
                    .into_iter()
                    .skip(1)
                    .map(|cell| survey_point(cell, survey.cell_size)),
            );
            if points
                .last()
                .is_none_or(|point| point.distance_squared(survey.goal) > 0.01)
            {
                points.push(survey.goal);
            }
            return SurveySearchResult::Found(points);
        }

        let current_height = survey.height(current_point, scratch);
        let current_score = scratch
            .score
            .get(&current)
            .copied()
            .unwrap_or(f32::INFINITY);
        let stride = survey.stride_at(current_point);
        for dx in -1..=1 {
            for dz in -1..=1 {
                if dx == 0 && dz == 0 {
                    continue;
                }
                let next = SurveyCell {
                    x: current.x + dx * stride,
                    z: current.z + dz * stride,
                };
                let next_point = survey_point(next, survey.cell_size);
                // Endpoints alone are insufficient for rotated blockers: two
                // adjacent clear grid points can have an edge that clips a
                // narrow corner. Movement checks the segment, so planning must
                // certify that same segment before accepting it.
                if !survey.line_clear(current_point, next_point, scratch) {
                    continue;
                }
                if dx != 0 && dz != 0 {
                    let side_x = survey_point(
                        SurveyCell {
                            x: current.x + dx * stride,
                            z: current.z,
                        },
                        survey.cell_size,
                    );
                    let side_z = survey_point(
                        SurveyCell {
                            x: current.x,
                            z: current.z + dz * stride,
                        },
                        survey.cell_size,
                    );
                    if survey.blocked(side_x, scratch) || survey.blocked(side_z, scratch) {
                        continue;
                    }
                }
                let next_height = survey.height(next_point, scratch);
                let rise = (next_height - current_height).abs();
                if rise > 1.15 {
                    continue;
                }
                let diagonal = dx != 0 && dz != 0;
                let distance = if diagonal {
                    std::f32::consts::SQRT_2
                } else {
                    1.0
                };
                // A tiny deterministic unevenness stops equal-cost open ground
                // from producing ruler-perfect spokes while remaining stable.
                let hash = (next.x as u32).wrapping_mul(73_856_093)
                    ^ (next.z as u32).wrapping_mul(19_349_663);
                let texture = (hash & 255) as f32 / 255.0 * 0.06;
                let step_cost = distance * stride as f32 * (1.0 + rise * 0.7 + texture);
                let tentative = current_score + step_cost;
                if tentative >= scratch.score.get(&next).copied().unwrap_or(f32::INFINITY) {
                    continue;
                }
                scratch.came_from.insert(next, current);
                scratch.score.insert(next, tentative);
                let estimate = tentative + survey_heuristic(next, goal);
                scratch.open.push(SurveyOpen {
                    cost: (estimate * 1000.0) as i32,
                    cell: next,
                });
            }
        }
    }
}

fn survey_a_star(survey: &RoadSurvey<'_>, scratch: &mut SurveyScratch) -> Vec<Vec2> {
    let mut state = SurveySearchState::default();
    match resume_survey_a_star(survey, scratch, &mut state, None) {
        SurveySearchResult::Found(points) => points,
        SurveySearchResult::Failed => Vec::new(),
        SurveySearchResult::Pending => unreachable!("an unbounded survey cannot yield"),
    }
}

/// Cheap terrain-only proof used when a controlled scenario needs two towns
/// that a wagon can actually connect. Buildings and generated props remain
/// the embodied planner's responsibility; this rejects different landmasses
/// before an economy creates an impossible first caravan contract.
pub(crate) fn overland_trade_corridor_exists(
    terrain: &WorldTerrain,
    start: Vec2,
    goal: Vec2,
) -> bool {
    let props = PropBlockers::default();
    let survey = RoadSurvey {
        terrain,
        buildings: &[],
        live_buildings: None,
        props: &props,
        start,
        goal,
        min: start.min(goal) - Vec2::splat(INTERSETTLEMENT_SURVEY_PADDING),
        max: start.max(goal) + Vec2::splat(INTERSETTLEMENT_SURVEY_PADDING),
        max_nodes: INTERSETTLEMENT_TRADE_SURVEY_MAX_NODES,
        // Hall centres are selected on clear, flat terrain, so this coarse
        // proof does not need the embodied route's fine doorway escapes.
        cell_size: SURVEY_CELL * INTERSETTLEMENT_SURVEY_STRIDE as f32,
        coarse_stride: 1,
        fine_endpoint_radius: 0.0,
    };
    let mut scratch = SurveyScratch::default();
    !survey_a_star(&survey, &mut scratch).is_empty()
}

fn simplify_visible(
    points: &[Vec2],
    survey: &RoadSurvey<'_>,
    scratch: &mut SurveyScratch,
) -> Vec<Vec2> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let mut result = vec![points[0]];
    let mut current = 0usize;
    while current + 1 < points.len() {
        // A* must only hand us certified edges. Keep this guard here too: an
        // adjacent pair whose endpoints are clear can still cut through the
        // corner of a rotated rectangle.
        if !survey.line_clear(points[current], points[current + 1], scratch) {
            return Vec::new();
        }
        let mut farthest = current + 1;
        for candidate in (current + 2)..points.len() {
            if survey.line_clear(points[current], points[candidate], scratch) {
                farthest = candidate;
            } else {
                break;
            }
        }
        result.push(points[farthest]);
        current = farthest;
    }
    result
}

fn add_natural_bends(
    points: &[Vec2],
    survey: &RoadSurvey<'_>,
    seed: u32,
    scratch: &mut SurveyScratch,
) -> Vec<Vec2> {
    let mut result = Vec::new();
    for (segment_index, pair) in points.windows(2).enumerate() {
        let start = pair[0];
        let end = pair[1];
        if result.is_empty() {
            result.push(start);
        }
        let direction = end - start;
        let length = direction.length();
        let divisions = (length / 6.0).ceil().max(1.0) as usize;
        let side = Vec2::new(-direction.y, direction.x).normalize_or_zero();
        let mut candidate = Vec::with_capacity(divisions + 1);
        candidate.push(start);
        for step in 1..divisions {
            let t = step as f32 / divisions as f32;
            let hash = seed
                .wrapping_add((segment_index as u32).wrapping_mul(1_103_515_245))
                .wrapping_add((step as u32).wrapping_mul(12_345));
            let signed = ((hash.rotate_left(13) & 1023) as f32 / 1023.0) * 2.0 - 1.0;
            let offset = signed * 0.62 * (std::f32::consts::PI * t).sin();
            candidate.push(start.lerp(end, t) + side * offset);
        }
        candidate.push(end);
        if candidate
            .windows(2)
            .all(|segment| survey.line_clear(segment[0], segment[1], scratch))
        {
            result.extend(candidate.into_iter().skip(1));
        } else {
            result.push(end);
        }
    }
    result
}

fn round_corners(
    points: &[Vec2],
    survey: &RoadSurvey<'_>,
    scratch: &mut SurveyScratch,
) -> Vec<Vec2> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let mut rounded = Vec::with_capacity(points.len() * 2);
    rounded.push(points[0]);
    for pair in points.windows(2) {
        rounded.push(pair[0].lerp(pair[1], 0.25));
        rounded.push(pair[0].lerp(pair[1], 0.75));
    }
    rounded.push(*points.last().unwrap());
    rounded.dedup_by(|a, b| a.distance_squared(*b) < 0.01);
    if rounded
        .windows(2)
        .all(|segment| survey.line_clear(segment[0], segment[1], scratch))
    {
        rounded
    } else {
        points.to_vec()
    }
}

fn resample_path(points: &[Vec2], spacing: f32) -> Vec<Vec2> {
    let Some(&first) = points.first() else {
        return Vec::new();
    };
    let mut result = vec![first];
    for pair in points.windows(2) {
        let start = pair[0];
        let end = pair[1];
        let length = start.distance(end);
        if length <= 1e-4 {
            continue;
        }
        // Preserve every certified bend. Carrying sample spacing across a
        // corner omitted the corner itself, and the resulting chord could cut
        // back through the obstacle the original two segments wrapped around.
        let divisions = (length / spacing).ceil().max(1.0) as usize;
        for step in 1..=divisions {
            result.push(start.lerp(end, step as f32 / divisions as f32));
        }
    }
    result
}

fn survey_village_road(
    terrain: &WorldTerrain,
    start: Vec2,
    goal: Vec2,
    buildings: &[BuildingBlocker],
    props: &PropBlockers,
    seed: u32,
    scratch: &mut SurveyScratch,
) -> Vec<Vec2> {
    let survey = RoadSurvey {
        terrain,
        buildings,
        live_buildings: None,
        props,
        start,
        goal,
        min: start.min(goal) - Vec2::splat(SURVEY_PADDING),
        max: start.max(goal) + Vec2::splat(SURVEY_PADDING),
        max_nodes: SURVEY_MAX_NODES,
        cell_size: SURVEY_CELL,
        coarse_stride: 1,
        fine_endpoint_radius: 0.0,
    };
    scratch.begin_search();
    let raw = survey_a_star(&survey, scratch);
    if raw.is_empty() {
        return Vec::new();
    }
    let simple = simplify_visible(&raw, &survey, scratch);
    let bent = add_natural_bends(&simple, &survey, seed, scratch);
    let rounded = round_corners(&bent, &survey, scratch);
    resample_path(&rounded, 2.0)
}

pub(crate) fn doorway_approach(
    kind: SettlementBuildingKind,
    building_position: Vec3,
    rotation: f32,
) -> (Vec2, Vec2) {
    let door3 = kind.entrance_position(building_position, rotation);
    let door = Vec2::new(door3.x, door3.z);
    let center = Vec2::new(building_position.x, building_position.z);
    let outward = (door - center).normalize_or_zero();
    (door, door + outward * DOOR_APPROACH_LENGTH)
}

fn point_segment_distance_squared(point: Vec2, start: Vec2, end: Vec2) -> f32 {
    let segment = end - start;
    let length_squared = segment.length_squared();
    if length_squared <= 1e-6 {
        return point.distance_squared(start);
    }
    let t = ((point - start).dot(segment) / length_squared).clamp(0.0, 1.0);
    point.distance_squared(start + segment * t)
}

fn static_collider_overlaps_segment(
    colliders: &StaticColliders,
    derived: &DerivedColliderLibrary,
    start: Vec2,
    end: Vec2,
    extra_clearance: f32,
) -> bool {
    static_collider_overlaps_segment_filtered(colliders, derived, start, end, extra_clearance, true)
}

fn static_collider_overlaps_segment_filtered(
    colliders: &StaticColliders,
    derived: &DerivedColliderLibrary,
    start: Vec2,
    end: Vec2,
    extra_clearance: f32,
    clearable_trees_are_blockers: bool,
) -> bool {
    const COLLIDER_CELL: f32 = 16.0;
    // Baked prop radii are small relative to a collision cell. Two extra cells
    // cover the largest authored rock/tree plus a future road reservation.
    let padding = extra_clearance + 8.0;
    let min = start.min(end) - Vec2::splat(padding);
    let max = start.max(end) + Vec2::splat(padding);
    let min_cell = (
        (min.x / COLLIDER_CELL).floor() as i32,
        (min.y / COLLIDER_CELL).floor() as i32,
    );
    let max_cell = (
        (max.x / COLLIDER_CELL).floor() as i32,
        (max.y / COLLIDER_CELL).floor() as i32,
    );
    let mut seen = HashSet::new();
    for x in min_cell.0..=max_cell.0 {
        for z in min_cell.1..=max_cell.1 {
            let Some(ids) = colliders.cells.get(&(x, z)) else {
                continue;
            };
            for id in ids {
                if !seen.insert(*id) {
                    continue;
                }
                let Some(instance) = colliders.instances.get(id) else {
                    continue;
                };
                if !clearable_trees_are_blockers && instance.kind.is_road_clearable() {
                    continue;
                }
                let Some(shape) = derived.by_kind.get(&instance.kind) else {
                    continue;
                };
                let radius = shape.horizontal_radius * instance.scale + extra_clearance;
                let point = Vec2::new(instance.position.x, instance.position.z);
                if point_segment_distance_squared(point, start, end) < radius * radius {
                    return true;
                }
            }
        }
    }
    false
}

pub(crate) fn navigation_point_is_clear_of_props(
    point: Vec2,
    colliders: &StaticColliders,
    derived: &DerivedColliderLibrary,
) -> bool {
    !static_collider_overlaps_segment(colliders, derived, point, point, VILLAGER_PROP_RADIUS)
}

fn embodied_segment_is_dry(terrain: &WorldTerrain, start: Vec2, end: Vec2) -> bool {
    let steps = (start.distance(end) / NAVIGATION_SAMPLE_STEP)
        .ceil()
        .max(1.0) as usize;
    let mut previous_height = None;
    (0..=steps).all(|step| {
        let point = start.lerp(end, step as f32 / steps as f32);
        if !road_sample_is_dry(terrain, point) {
            return false;
        }
        let height = terrain.get_height(point.x, point.y);
        let clear = previous_height.is_none_or(|previous: f32| (height - previous).abs() <= 0.47);
        previous_height = Some(height);
        clear
    })
}

/// Select a field standing point with a certified corridor from the authored
/// Farmstead door around the building shell.
///
/// Checking only the final point allowed a clear patch behind a Farmstead to
/// be chosen even when water, a prop, or dense neighboring geometry sealed the
/// route to it. The returned corridor is deliberately simple and cheap to
/// prove; the ordinary tactical planner may later choose an equivalent route.
pub(crate) fn reachable_farm_work_stand(
    terrain: &WorldTerrain,
    farm: Vec3,
    rotation: f32,
    field: Vec3,
    worker_salt: u32,
    obstacles: Option<&SpatialObstacleGrid>,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
) -> Option<Vec3> {
    let kind = SettlementBuildingKind::Farmstead;
    let side = if worker_salt & 1 == 0 { -1.5 } else { 1.5 };
    let candidates = [
        Vec2::new(side, 0.0),
        Vec2::new(-side, 0.0),
        Vec2::new(0.0, 2.0),
        Vec2::new(0.0, -2.0),
        Vec2::new(side, 2.5),
        Vec2::new(-side, 2.5),
        Vec2::new(side, -2.5),
        Vec2::new(-side, -2.5),
    ];
    let door = kind.entrance_position(farm, rotation);
    let outward = Vec2::new(door.x - farm.x, door.z - farm.z).normalize_or_zero();
    let start = Vec2::new(door.x, door.z) + outward * 0.75;
    let definition = kind.art().definition();
    let footprint_half = definition.footprint * 0.5 + Vec2::splat(VILLAGER_NAV_RADIUS);
    let own_building = BuildingBlocker {
        center: definition.world_footprint_center(farm, rotation),
        half: footprint_half,
        rotation,
    };
    let front_y = kind.door_offset().y - 0.75;

    for local_offset in candidates {
        let offset = shared::rotation::local_to_world_xz(local_offset, rotation);
        let point = Vec2::new(field.x + offset.x, field.z + offset.y);
        if obstacles.is_some_and(|grid| grid.point_blocked(point))
            || colliders.zip(derived).is_some_and(|(colliders, derived)| {
                !navigation_point_is_clear_of_props(point, colliders, derived)
            })
        {
            continue;
        }
        let local_goal =
            shared::rotation::world_to_local_xz(point - Vec2::new(farm.x, farm.z), rotation);
        let preferred_sign = if local_goal.x.abs() > 0.25 {
            local_goal.x.signum()
        } else if worker_salt & 1 == 0 {
            -1.0
        } else {
            1.0
        };
        for sign in [preferred_sign, -preferred_sign] {
            let side_x = sign * (footprint_half.x + 0.55);
            let local_route = [Vec2::new(side_x, front_y), Vec2::new(side_x, local_goal.y)];
            let mut route = vec![start];
            route.extend(local_route.into_iter().map(|local| {
                let world = shared::rotation::local_to_world_xz(local, rotation);
                Vec2::new(farm.x + world.x, farm.z + world.y)
            }));
            route.push(point);
            let own_shell_clear = route
                .windows(2)
                .all(|segment| !own_building.blocks_segment(segment[0], segment[1]));
            let terrain_clear = route
                .windows(2)
                .all(|segment| embodied_segment_is_dry(terrain, segment[0], segment[1]));
            if own_shell_clear
                && terrain_clear
                && polyline_clear_live_world_with_start_escape(
                    &route, obstacles, colliders, derived, false,
                )
            {
                return Some(Vec3::new(
                    point.x,
                    terrain.get_height(point.x, point.y),
                    point.y,
                ));
            }
        }
    }
    None
}

/// Whether an authored crop/yard rectangle has enough horizontal clearance
/// from every collidable prop. This uses circle-vs-rotated-box distance and the
/// same baked horizontal radii as villager movement.
#[cfg(test)]
pub(crate) fn rotated_rect_is_clear_of_props(
    center: Vec2,
    half: Vec2,
    rotation: f32,
    extra_clearance: f32,
    colliders: &StaticColliders,
    derived: &DerivedColliderLibrary,
) -> bool {
    rotated_rect_is_clear_of_props_matching(
        center,
        half,
        rotation,
        extra_clearance,
        colliders,
        derived,
        |_| true,
    )
}

/// Whether a crop rectangle is free of permanent authored obstacles.
///
/// Trees and dead trunks are deliberately ignored: an approved Farmstead now
/// clears those when its ground claim becomes active. Rocks remain blockers,
/// so agricultural earthworks cannot silently erase a boulder.
pub(crate) fn rotated_rect_is_clear_of_permanent_props(
    center: Vec2,
    half: Vec2,
    rotation: f32,
    extra_clearance: f32,
    colliders: &StaticColliders,
    derived: &DerivedColliderLibrary,
) -> bool {
    rotated_rect_is_clear_of_props_matching(
        center,
        half,
        rotation,
        extra_clearance,
        colliders,
        derived,
        |kind| !kind.is_road_clearable(),
    )
}

fn rotated_rect_is_clear_of_props_matching(
    center: Vec2,
    half: Vec2,
    rotation: f32,
    extra_clearance: f32,
    colliders: &StaticColliders,
    derived: &DerivedColliderLibrary,
    mut blocks: impl FnMut(shared::props::PropKind) -> bool,
) -> bool {
    const COLLIDER_CELL: f32 = 16.0;
    const MAX_AUTHORED_PROP_RADIUS: f32 = 8.0;
    let (sin, cos) = rotation.sin_cos();
    let world_half = Vec2::new(
        half.x * cos.abs() + half.y * sin.abs(),
        half.x * sin.abs() + half.y * cos.abs(),
    );
    let padding = extra_clearance + MAX_AUTHORED_PROP_RADIUS;
    let min = center - world_half - Vec2::splat(padding);
    let max = center + world_half + Vec2::splat(padding);
    let min_cell = (
        (min.x / COLLIDER_CELL).floor() as i32,
        (min.y / COLLIDER_CELL).floor() as i32,
    );
    let max_cell = (
        (max.x / COLLIDER_CELL).floor() as i32,
        (max.y / COLLIDER_CELL).floor() as i32,
    );
    let mut seen = HashSet::new();
    for x in min_cell.0..=max_cell.0 {
        for z in min_cell.1..=max_cell.1 {
            let Some(ids) = colliders.cells.get(&(x, z)) else {
                continue;
            };
            for id in ids {
                if !seen.insert(*id) {
                    continue;
                }
                let Some(instance) = colliders.instances.get(id) else {
                    continue;
                };
                if !blocks(instance.kind) {
                    continue;
                }
                let Some(shape) = derived.by_kind.get(&instance.kind) else {
                    continue;
                };
                let point = Vec2::new(instance.position.x, instance.position.z);
                let local = shared::rotation::world_to_local_xz(point - center, rotation);
                let outside = (local.abs() - half).max(Vec2::ZERO);
                let radius = shape.horizontal_radius * instance.scale + extra_clearance;
                if outside.length_squared() < radius * radius {
                    return false;
                }
            }
        }
    }
    true
}

fn add_static_collider_blockers(
    blockers: &mut PropBlockers,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
    start: Vec2,
    goal: Vec2,
    padding: f32,
    extra_clearance: f32,
) {
    add_static_collider_blockers_filtered(
        blockers,
        colliders,
        derived,
        start,
        goal,
        padding,
        extra_clearance,
        true,
    );
}

#[allow(clippy::too_many_arguments)]
fn add_static_collider_blockers_filtered(
    blockers: &mut PropBlockers,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
    start: Vec2,
    goal: Vec2,
    padding: f32,
    extra_clearance: f32,
    clearable_trees_are_blockers: bool,
) {
    let (Some(colliders), Some(derived)) = (colliders, derived) else {
        return;
    };
    const COLLIDER_CELL: f32 = 16.0;
    let min = start.min(goal) - Vec2::splat(padding);
    let max = start.max(goal) + Vec2::splat(padding);
    let min_cell = (
        (min.x / COLLIDER_CELL).floor() as i32,
        (min.y / COLLIDER_CELL).floor() as i32,
    );
    let max_cell = (
        (max.x / COLLIDER_CELL).floor() as i32,
        (max.y / COLLIDER_CELL).floor() as i32,
    );
    let mut seen = HashSet::new();
    for x in min_cell.0..=max_cell.0 {
        for z in min_cell.1..=max_cell.1 {
            let Some(ids) = colliders.cells.get(&(x, z)) else {
                continue;
            };
            for id in ids {
                if !seen.insert(*id) {
                    continue;
                }
                let Some(instance) = colliders.instances.get(id) else {
                    continue;
                };
                if !clearable_trees_are_blockers && instance.kind.is_road_clearable() {
                    continue;
                }
                let Some(shape) = derived.by_kind.get(&instance.kind) else {
                    continue;
                };
                blockers.insert_radius(
                    Vec2::new(instance.position.x, instance.position.z),
                    shape.horizontal_radius * instance.scale + extra_clearance,
                );
            }
        }
    }
}

fn blockers_for_route(
    terrain: &WorldTerrain,
    start: Vec2,
    goal: Vec2,
    buildings: &[BuildingBlocker],
    road_half_width: f32,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
    cache: &mut RoutePropChunkCache,
    prop_generation_used: &mut bool,
    clearable_trees_are_blockers: bool,
) -> Option<PropBlockers> {
    let min = start.min(goal) - Vec2::splat(SURVEY_PADDING + PropBlockers::CLEARANCE);
    let max = start.max(goal) + Vec2::splat(SURVEY_PADDING + PropBlockers::CLEARANCE);
    let min_chunk = ChunkCoord::new(
        (min.x / CHUNK_SIZE).floor() as i32,
        (min.y / CHUNK_SIZE).floor() as i32,
    );
    let max_chunk = ChunkCoord::new(
        (max.x / CHUNK_SIZE).floor() as i32,
        (max.y / CHUNK_SIZE).floor() as i32,
    );
    let mut blockers = PropBlockers::default();
    for x in min_chunk.x..=max_chunk.x {
        for z in min_chunk.z..=max_chunk.z {
            let chunk = ChunkCoord::new(x, z);
            if !cache.chunks.contains_key(&chunk) {
                if *prop_generation_used {
                    return None;
                }
                // Authored prop generation includes river clearance and can
                // be a meaningful unit of work. Populate only one unseen
                // survey chunk per server tick; the retained RoadRequest
                // resumes on the next tick with exactly the same candidates.
                let _ = cache.chunk(terrain, chunk);
                *prop_generation_used = true;
            }
            for prop in cache.chunk(terrain, chunk) {
                if !clearable_trees_are_blockers && prop.kind.is_road_clearable() {
                    continue;
                }
                if buildings
                    .iter()
                    .any(|building| building.contains(prop.point))
                {
                    continue;
                }
                // Reserve the eventual road edge, not merely enough room for
                // today's narrow dirt ribbon. The extra 1.05 m represents a
                // conservative authored trunk/rock radius plus a soft shoulder.
                blockers.insert_radius(prop.point, road_half_width + 1.05);
            }
        }
    }
    add_static_collider_blockers_filtered(
        &mut blockers,
        colliders,
        derived,
        start,
        goal,
        SURVEY_PADDING + PropBlockers::CLEARANCE,
        road_half_width + 0.3,
        clearable_trees_are_blockers,
    );
    Some(blockers)
}

fn clearable_trees_intersecting_road(
    terrain: &WorldTerrain,
    points: &[Vec2],
    road_width: f32,
    derived: Option<&DerivedColliderLibrary>,
    cache: &mut RoutePropChunkCache,
) -> Vec<RoadTreeObstruction> {
    let Some(first) = points.first().copied() else {
        return Vec::new();
    };
    let (mut min, mut max) = (first, first);
    for point in points.iter().copied().skip(1) {
        min = min.min(point);
        max = max.max(point);
    }
    let padding = road_width * 0.5 + 3.0;
    min -= Vec2::splat(padding);
    max += Vec2::splat(padding);
    let min_chunk = ChunkCoord::new(
        (min.x / CHUNK_SIZE).floor() as i32,
        (min.y / CHUNK_SIZE).floor() as i32,
    );
    let max_chunk = ChunkCoord::new(
        (max.x / CHUNK_SIZE).floor() as i32,
        (max.y / CHUNK_SIZE).floor() as i32,
    );

    let mut trees = Vec::new();
    for x in min_chunk.x..=max_chunk.x {
        for z in min_chunk.z..=max_chunk.z {
            for prop in cache.chunk(terrain, ChunkCoord::new(x, z)) {
                if !prop.kind.is_road_clearable() {
                    continue;
                }
                let radius = derived
                    .and_then(|library| library.by_kind.get(&prop.kind))
                    .map_or(1.05, |shape| shape.horizontal_radius * prop.scale);
                let clearance = radius + road_width * 0.5 + 0.15;
                let mut progress = 0.0;
                let mut nearest = None;
                for segment in points.windows(2) {
                    let length = segment[0].distance(segment[1]);
                    let delta = segment[1] - segment[0];
                    let t = if length <= f32::EPSILON {
                        0.0
                    } else {
                        ((prop.point - segment[0]).dot(delta) / delta.length_squared())
                            .clamp(0.0, 1.0)
                    };
                    let distance_sq = prop.point.distance_squared(segment[0] + delta * t);
                    if nearest.is_none_or(|(_, best)| distance_sq < best) {
                        nearest = Some((progress + length * t, distance_sq));
                    }
                    progress += length;
                }
                if let Some((progress, distance_sq)) = nearest {
                    if distance_sq <= clearance * clearance {
                        trees.push((
                            progress,
                            RoadTreeObstruction {
                                point: prop.point,
                                radius,
                            },
                        ));
                    }
                }
            }
        }
    }
    trees.sort_by(|a, b| {
        a.0.total_cmp(&b.0)
            .then_with(|| a.1.point.x.total_cmp(&b.1.point.x))
            .then_with(|| a.1.point.y.total_cmp(&b.1.point.y))
    });
    trees.dedup_by(|a, b| a.1.point.distance_squared(b.1.point) <= 0.01);
    trees.into_iter().map(|(_, tree)| tree).collect()
}

fn blockers_for_agent_route(
    terrain: &WorldTerrain,
    start: Vec2,
    goal: Vec2,
    buildings: &SpatialObstacleGrid,
    derived: Option<&DerivedColliderLibrary>,
    colliders: Option<&StaticColliders>,
    connector_reach: f32,
    cache: &mut RoutePropChunkCache,
) -> PropBlockers {
    let padding = SURVEY_PADDING + connector_reach;
    let min = start.min(goal) - Vec2::splat(padding);
    let max = start.max(goal) + Vec2::splat(padding);
    let min_chunk = ChunkCoord::new(
        (min.x / CHUNK_SIZE).floor() as i32,
        (min.y / CHUNK_SIZE).floor() as i32,
    );
    let max_chunk = ChunkCoord::new(
        (max.x / CHUNK_SIZE).floor() as i32,
        (max.y / CHUNK_SIZE).floor() as i32,
    );
    let mut blockers = PropBlockers::default();
    for x in min_chunk.x..=max_chunk.x {
        for z in min_chunk.z..=max_chunk.z {
            for prop in cache.chunk(terrain, ChunkCoord::new(x, z)) {
                // These deterministic props are cleared from completed plots.
                // Do not resurrect an invisible trunk inside a building.
                if buildings.point_blocked(prop.point) {
                    continue;
                }
                let authored_radius = derived
                    .and_then(|library| library.by_kind.get(&prop.kind))
                    .map_or(0.75, |shape| shape.horizontal_radius)
                    * prop.scale;
                blockers.insert_radius(prop.point, authored_radius + VILLAGER_PROP_RADIUS);
            }
        }
    }
    add_static_collider_blockers(
        &mut blockers,
        colliders,
        derived,
        start,
        goal,
        padding,
        VILLAGER_PROP_RADIUS,
    );
    blockers
}

fn survey_agent_route(
    terrain: &WorldTerrain,
    start: Vec2,
    goal: Vec2,
    buildings: &[BuildingBlocker],
    live_buildings: Option<&SpatialObstacleGrid>,
    props: &PropBlockers,
    scratch: &mut SurveyScratch,
    max_nodes: usize,
) -> Vec<Vec2> {
    if start.distance_squared(goal) <= 0.01 {
        return vec![start, goal];
    }
    let survey = RoadSurvey {
        terrain,
        buildings,
        live_buildings,
        props,
        start,
        goal,
        min: start.min(goal) - Vec2::splat(SURVEY_PADDING),
        max: start.max(goal) + Vec2::splat(SURVEY_PADDING),
        max_nodes,
        cell_size: SURVEY_CELL,
        coarse_stride: 1,
        fine_endpoint_radius: 0.0,
    };
    scratch.begin_search();
    if survey.line_clear(start, goal, scratch) {
        return vec![start, goal];
    }
    let raw = survey_a_star(&survey, scratch);
    if raw.is_empty() {
        return Vec::new();
    }
    let simple = simplify_visible(&raw, &survey, scratch);
    let resampled = resample_path(&simple, 1.5);
    if live_buildings.is_none_or(|grid| polyline_clear_live_buildings(&resampled, grid)) {
        return resampled;
    }

    // Near a rotated corner, simplification and floating-point resampling can
    // very occasionally turn two certified A* legs into a tangent chord that
    // enters the live blocker. The raw A* edges were certified individually,
    // so preserve them rather than returning a route movement will reject.
    if live_buildings.is_none_or(|grid| polyline_clear_live_buildings(&raw, grid)) {
        raw
    } else {
        Vec::new()
    }
}

/// Cheap permit-time proof that a proposed remote plot belongs to the same
/// walkable landmass as its Moot Hall.
///
/// Buildings and props are intentionally absent here: the ordinary embodied
/// planner handles those changing obstacles. This rejects the permanent case
/// that planner cannot repair—a coastal plot across water—using the same dry
/// terrain rules and bounded search an actual villager will use.
pub(crate) fn embodied_land_route_exists(terrain: &WorldTerrain, start: Vec3, goal: Vec3) -> bool {
    let start = Vec2::new(start.x, start.z);
    let goal = Vec2::new(goal.x, goal.z);
    survey_agent_route(
        terrain,
        start,
        goal,
        &[],
        None,
        &PropBlockers::default(),
        &mut SurveyScratch::default(),
        EXTENDED_LOCAL_SURVEY_MAX_NODES,
    )
    .len()
        >= 2
}

/// Permit-time proof that two points share a road-width dry landmass.
///
/// This deliberately answers only connectivity; it does not build an actor
/// route. Running the 1.5 m embodied planner with 0.2 m edge samples during a
/// permit decision produced multi-second server stalls beside rivers. A 4 m
/// A* with one-metre, full-road-width edge samples remains conservative about
/// water and slope while reducing the search graph by roughly an order of
/// magnitude. Construction still performs the precise obstacle-aware survey
/// before a road or actor actually traverses the result.
pub(crate) fn permit_land_route_exists(terrain: &WorldTerrain, start: Vec3, goal: Vec3) -> bool {
    const CELL: f32 = 4.0;
    const EDGE_SAMPLE: f32 = 1.0;
    const PADDING: f32 = 28.0;
    const MAX_NODES: usize = 2_000;

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    struct Cell {
        x: i32,
        z: i32,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Open {
        estimate: i32,
        cell: Cell,
    }

    impl Ord for Open {
        fn cmp(&self, other: &Self) -> Ordering {
            other
                .estimate
                .cmp(&self.estimate)
                .then_with(|| self.cell.x.cmp(&other.cell.x))
                .then_with(|| self.cell.z.cmp(&other.cell.z))
        }
    }

    impl PartialOrd for Open {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }

    let start = Vec2::new(start.x, start.z);
    let goal = Vec2::new(goal.x, goal.z);
    let min = start.min(goal) - Vec2::splat(PADDING);
    let max = start.max(goal) + Vec2::splat(PADDING);
    let cell_for = |point: Vec2| Cell {
        x: (point.x / CELL).round() as i32,
        z: (point.y / CELL).round() as i32,
    };
    let point_for = |cell: Cell| Vec2::new(cell.x as f32 * CELL, cell.z as f32 * CELL);
    let heuristic = |a: Cell, b: Cell| Vec2::new((a.x - b.x) as f32, (a.z - b.z) as f32).length();
    let edge_is_dry = |from: Vec2, to: Vec2| {
        let steps = (from.distance(to) / EDGE_SAMPLE).ceil().max(1.0) as usize;
        let mut previous_height = None;
        (0..=steps).all(|step| {
            let point = from.lerp(to, step as f32 / steps as f32);
            if !road_sample_is_dry(terrain, point) {
                return false;
            }
            let height = terrain.get_height(point.x, point.y);
            let acceptable =
                previous_height.is_none_or(|previous: f32| (height - previous).abs() <= 0.7);
            previous_height = Some(height);
            acceptable
        })
    };

    if edge_is_dry(start, goal) {
        return true;
    }

    let start_cell = cell_for(start);
    let goal_cell = cell_for(goal);
    let mut open = BinaryHeap::new();
    let mut closed = HashSet::new();
    let mut scores = HashMap::new();
    scores.insert(start_cell, 0.0f32);
    open.push(Open {
        estimate: (heuristic(start_cell, goal_cell) * 1_000.0) as i32,
        cell: start_cell,
    });

    while let Some(Open { cell: current, .. }) = open.pop() {
        if !closed.insert(current) || closed.len() > MAX_NODES {
            continue;
        }
        let current_point = if current == start_cell {
            start
        } else {
            point_for(current)
        };
        if current_point.distance(goal) <= CELL * 1.6 && edge_is_dry(current_point, goal) {
            return true;
        }
        let current_score = scores.get(&current).copied().unwrap_or(f32::INFINITY);
        for dx in -1..=1 {
            for dz in -1..=1 {
                if dx == 0 && dz == 0 {
                    continue;
                }
                let next = Cell {
                    x: current.x + dx,
                    z: current.z + dz,
                };
                if closed.contains(&next) {
                    continue;
                }
                let next_point = point_for(next);
                if next_point.x < min.x
                    || next_point.y < min.y
                    || next_point.x > max.x
                    || next_point.y > max.y
                    || !edge_is_dry(current_point, next_point)
                {
                    continue;
                }
                let step_cost = if dx != 0 && dz != 0 {
                    std::f32::consts::SQRT_2
                } else {
                    1.0
                };
                let tentative = current_score + step_cost;
                if tentative >= scores.get(&next).copied().unwrap_or(f32::INFINITY) {
                    continue;
                }
                scores.insert(next, tentative);
                open.push(Open {
                    estimate: ((tentative + heuristic(next, goal_cell)) * 1_000.0) as i32,
                    cell: next,
                });
            }
        }
    }
    false
}

#[derive(Clone, Copy)]
struct NavigationBuilding {
    blocker: BuildingBlocker,
    kind: SettlementBuildingKind,
    position: Vec3,
    rotation: f32,
}

/// Immutable building navigation data, rebuilt only when a placed building is
/// added, changed or removed. Previously every planner tick allocated both of
/// these vectors even when the village geometry had not changed for minutes.
#[derive(Default)]
pub(crate) struct NavigationBuildingCache {
    initialized: bool,
    buildings: Vec<NavigationBuilding>,
    blockers: Vec<BuildingBlocker>,
    spatial: SpatialObstacleGrid,
}

impl NavigationBuildingCache {
    fn rebuild<'a>(
        &mut self,
        placed: impl Iterator<Item = (&'a PlacedBuilding, &'a BuildingPosition)>,
    ) -> Vec<BuildingBlocker> {
        let previous = std::mem::take(&mut self.blockers);
        self.buildings.clear();
        self.spatial.clear();
        for (building, position) in placed {
            if !building.building_type.blocks_ground_navigation() {
                continue;
            }
            let kind = settlement_kind_for_art(building.building_type);
            let definition = building.building_type.definition();
            let half = definition.footprint * 0.5
                + Vec2::splat(crate::world::navgrid::VILLAGER_NAV_RADIUS);
            let navigation = NavigationBuilding {
                blocker: BuildingBlocker {
                    center: definition.world_footprint_center(position.0, building.rotation),
                    half,
                    rotation: building.rotation,
                },
                kind,
                position: position.0,
                rotation: building.rotation,
            };
            self.spatial.insert(shared::spatial::ObstacleEntry {
                center: navigation.blocker.center,
                half_extents: navigation.blocker.half,
                rotation: navigation.blocker.rotation,
                obstacle_type: building.building_type as u32,
            });
            self.blockers.push(navigation.blocker);
            self.buildings.push(navigation);
        }
        self.initialized = true;
        let same = |a: &BuildingBlocker, b: &BuildingBlocker| {
            a.center == b.center && a.half == b.half && a.rotation.to_bits() == b.rotation.to_bits()
        };
        previous
            .iter()
            .filter(|old| !self.blockers.iter().any(|new| same(old, new)))
            .chain(
                self.blockers
                    .iter()
                    .filter(|new| !previous.iter().any(|old| same(old, new))),
            )
            .copied()
            .collect()
    }
}

fn settlement_kind_for_art(building_type: BuildingType) -> SettlementBuildingKind {
    match building_type {
        BuildingType::LogCabin => SettlementBuildingKind::House,
        BuildingType::LumberjackHut => SettlementBuildingKind::LumberjackHut,
        BuildingType::Farmstead => SettlementBuildingKind::Farmstead,
        BuildingType::FishermansHut => SettlementBuildingKind::FishermansHut,
        BuildingType::MootHall | BuildingType::VillageHall | BuildingType::TownHall => {
            SettlementBuildingKind::Hall
        }
        BuildingType::Market | BuildingType::MarketPaved => SettlementBuildingKind::Market,
        BuildingType::PlaceholderTavern => SettlementBuildingKind::Tavern,
        BuildingType::PlaceholderChurch => SettlementBuildingKind::Church,
        BuildingType::Windmill => SettlementBuildingKind::Windmill,
        BuildingType::Bakery => SettlementBuildingKind::Bakery,
        BuildingType::PlaceholderStorageHall => SettlementBuildingKind::StorageHall,
        BuildingType::PlaceholderStoneQuarry => SettlementBuildingKind::StoneQuarry,
        BuildingType::PlaceholderLivestockFarm => SettlementBuildingKind::LivestockFarm,
    }
}

#[derive(Clone, Copy)]
struct NavigationEndpoint {
    actual: Vec2,
    survey: Vec2,
    escaping_building: bool,
}

fn navigation_endpoint(point: Vec2, buildings: &[NavigationBuilding]) -> NavigationEndpoint {
    let nearest_door = buildings
        .iter()
        .filter_map(|building| {
            let (door, approach) =
                doorway_approach(building.kind, building.position, building.rotation);
            let distance = point.distance(door);
            let inside = building.blocker.contains(point);
            // Near-door recovery is the normal case (a shell completed around
            // its builder). A god-mode or migration burst can also be caught
            // deeper inside a newly published footprint. Once containment is
            // proven, route that actor to the authored apron regardless of
            // door distance; this is bounded recovery state and is removed as
            // soon as movement reaches clear ground.
            (inside || distance <= NAVIGATION_DOOR_RECOVERY_DISTANCE)
                .then_some((distance, approach, inside))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    NavigationEndpoint {
        actual: point,
        survey: nearest_door.map_or(point, |(_, approach, _)| approach),
        escaping_building: nearest_door.is_some_and(|(_, _, inside)| inside),
    }
}

fn prop_safe_navigation_endpoint(
    endpoint: NavigationEndpoint,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
) -> NavigationEndpoint {
    let (Some(colliders), Some(derived)) = (colliders, derived) else {
        return endpoint;
    };
    if endpoint.actual.distance_squared(endpoint.survey) > 0.01
        && static_collider_overlaps_segment(
            colliders,
            derived,
            endpoint.actual,
            endpoint.survey,
            VILLAGER_PROP_RADIUS,
        )
    {
        // The authored straight apron is a preference, not permission to walk
        // through a trunk. Route directly to the already-outside door and let
        // A* approach it from a clear angle.
        NavigationEndpoint {
            actual: endpoint.actual,
            survey: endpoint.actual,
            escaping_building: false,
        }
    } else {
        endpoint
    }
}

fn polyline_length(points: &[Vec2]) -> f32 {
    points
        .windows(2)
        .map(|pair| pair[0].distance(pair[1]))
        .sum()
}

fn polyline_clear_live_buildings(points: &[Vec2], grid: &SpatialObstacleGrid) -> bool {
    points.windows(2).all(|segment| {
        let start = segment[0];
        let end = segment[1];
        let steps = (start.distance(end) / NAVIGATION_SAMPLE_STEP)
            .ceil()
            .max(1.0) as usize;
        // Movement begins at step one too. A doorway transit may leave an
        // actor exactly on a blocker boundary; what matters is that every
        // point they are about to enter is clear.
        (1..=steps).all(|step| !grid.point_blocked(start.lerp(end, step as f32 / steps as f32)))
    })
}

#[cfg(test)]
fn polyline_clear_live_world(
    points: &[Vec2],
    buildings: Option<&SpatialObstacleGrid>,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
) -> bool {
    polyline_clear_live_world_with_start_escape(points, buildings, colliders, derived, false)
}

fn polyline_clear_live_world_with_start_escape(
    points: &[Vec2],
    buildings: Option<&SpatialObstacleGrid>,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
    allow_start_escape: bool,
) -> bool {
    let mut escaping = allow_start_escape;
    points.windows(2).all(|segment| {
        let clear = crate::player::hero::navigation_segment_clear(
            segment[0],
            segment[1],
            escaping.then_some(None).unwrap_or(buildings),
            colliders,
            derived,
        );
        if escaping && buildings.is_none_or(|grid| !grid.point_blocked(segment[1])) {
            escaping = false;
        }
        clear
    })
}

fn append_distinct(points: &mut Vec<Vec2>, point: Vec2) {
    if points
        .last()
        .is_none_or(|last| last.distance_squared(point) > 0.01)
    {
        points.push(point);
    }
}

fn complete_agent_route(
    start: NavigationEndpoint,
    goal: NavigationEndpoint,
    start_apron: &[Vec2],
    middle: Vec<Vec2>,
    goal_apron: &[Vec2],
) -> Vec<Vec2> {
    if start_apron.is_empty() || middle.is_empty() || goal_apron.is_empty() {
        return Vec::new();
    }
    let mut route = Vec::with_capacity(start_apron.len() + middle.len() + goal_apron.len());
    append_distinct(&mut route, start.actual);
    for point in start_apron.iter().copied() {
        append_distinct(&mut route, point);
    }
    for point in middle {
        append_distinct(&mut route, point);
    }
    for point in goal_apron.iter().copied() {
        append_distinct(&mut route, point);
    }
    append_distinct(&mut route, goal.actual);
    route
}

#[cfg(test)]
mod tests;
