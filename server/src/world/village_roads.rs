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

use construction::{
    absolute_world_seconds, building_road_status, hall_road_network, BuildingRoadStatus,
};
pub use construction::{build_village_roads, plan_requested_roads};
use routing::graph_key;
pub use routing::{
    plan_villager_travel_routes, queue_villager_travel_routes, rebuild_village_road_graph,
    retry_failed_routes_after_obstacle_change, VillageRoadGraph,
};
#[cfg(test)]
use routing::{reverse_route_clears_goal_prop_exemption, RoadGraphNode};
pub use steward::{
    audit_village_roads, ensure_moot_administrations, staff_and_pay_road_stewards,
    staff_public_positions,
};

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use lightyear::prelude::{NetworkTarget, Replicate};
use shared::building::{BuildingPosition, BuildingType, PlacedBuilding};
use shared::components::{
    BuildingDoorUse, CharacterActivity, CharacterKind, CharacterName, FarmField,
    MootAdministration, Occupation, PlayerPosition, PlayerRotation, RoadClass, RoadSurface,
    Settlement, SettlementBuilding, SettlementBuildingKind, VillageRoad, WorkStatus, WorldTime,
};
use shared::economy::{Wallet, ROAD_STEWARD_DAILY_SALARY};
use shared::spatial::SpatialObstacleGrid;
use shared::terrain::{world_pos_in_bounds, ChunkCoord, WorldTerrain, CHUNK_SIZE};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use crate::player::hero::MoveTarget;
use crate::world::navgrid::{NAVIGATION_SAMPLE_STEP, VILLAGER_PROP_RADIUS};
use crate::world::village::{HomeRoutine, PierTraversal, VillagerIntent};
use crate::{
    collision::library::{DerivedColliderLibrary, StaticColliders},
    world::pathfinding::PathfindingBudgetSettings,
};

pub(crate) use geometry::{
    road_corridor_is_dry, road_sample_is_dry, road_segment_is_dry, road_segment_is_dry_at_width,
    surface_width_for_tier,
};

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
const EXTENDED_LOCAL_SURVEY_MIN_DISTANCE: f32 = 48.0;
const EXTENDED_LOCAL_SURVEY_MAX_DISTANCE: f32 = 160.0;
const ROAD_BUILD_SECONDS: f32 = 0.55;
const ROAD_REACH: f32 = 0.45;
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
/// Connector surveys are full terrain A* searches. Rank several graph options
/// cheaply, but only survey the best couple so one unreachable villager cannot
/// monopolise a server tick trying every road combination.
const AGENT_ROAD_CANDIDATES_TO_SURVEY: usize = 2;
const ROAD_MAX_WEIGHTED_DETOUR: f32 = 1.35;
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

/// A permit must not put a permanent doorway apron through a tree or rock.
///
/// Buildings clear props inside their own authored footprint, but the road
/// apron begins outside that footprint. Checking the streamed collision truth
/// here prevents a valid-looking workplace from trapping its staff between the
/// door and scenery that neither the road builder nor ordinary navigation is
/// allowed to erase.
pub(crate) fn doorway_road_apron_is_clear_of_props(
    kind: SettlementBuildingKind,
    position: Vec3,
    rotation: f32,
    colliders: &StaticColliders,
    derived: &DerivedColliderLibrary,
) -> bool {
    let (door, approach) = doorway_approach(kind, position, rotation);
    let road_half_width = RoadClass::Lane.initial_reserved_width() * 0.5;
    !static_collider_overlaps_segment(colliders, derived, door, approach, road_half_width + 0.2)
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
    fn after_failure(previous: Option<Self>, now: f64) -> Self {
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
    last_paid_day: u32,
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
    GoingTo { point: usize },
    Working { point: usize, seconds_left: f32 },
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
    obstacle_version: u64,
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

impl NavigationRoutePending {
    pub fn new(goal: Vec3) -> Self {
        Self {
            goal,
            attempts: 0,
            obstacle_version: u64::MAX,
        }
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
                shared::props::generate_chunk_prop_spawns(&terrain.generator, chunk)
                    .into_iter()
                    .filter_map(|spawn| {
                        let kind = spawn.kind.filter(|kind| kind.blocks_village_road())?;
                        Some(CachedRouteProp {
                            point: Vec2::new(spawn.position.x, spawn.position.z),
                            kind,
                            scale: spawn.scale,
                        })
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
            self.live_buildings
                .is_some_and(|grid| grid.point_blocked(point))
                || self.buildings.iter().any(|blocker| blocker.contains(point))
                || (!endpoint_clear && self.props.blocks(point))
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
        if self
            .live_buildings
            .is_some_and(|grid| grid.segment_blocked(start, end))
            || self
                .buildings
                .iter()
                .any(|blocker| blocker.blocks_segment(start, end))
        {
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

fn survey_cell(point: Vec2) -> SurveyCell {
    SurveyCell {
        x: (point.x / SURVEY_CELL).round() as i32,
        z: (point.y / SURVEY_CELL).round() as i32,
    }
}

fn survey_point(cell: SurveyCell) -> Vec2 {
    Vec2::new(cell.x as f32 * SURVEY_CELL, cell.z as f32 * SURVEY_CELL)
}

fn survey_heuristic(a: SurveyCell, b: SurveyCell) -> f32 {
    Vec2::new((a.x - b.x) as f32, (a.z - b.z) as f32).length()
}

fn survey_a_star(survey: &RoadSurvey<'_>, scratch: &mut SurveyScratch) -> Vec<Vec2> {
    let start = survey_cell(survey.start);
    let goal = survey_cell(survey.goal);
    scratch.score.insert(start, 0.0);
    scratch.open.push(SurveyOpen {
        cost: (survey_heuristic(start, goal) * 1000.0) as i32,
        cell: start,
    });

    let mut expanded = 0usize;
    while let Some(SurveyOpen { cell: current, .. }) = scratch.open.pop() {
        if !scratch.closed.insert(current) {
            continue;
        }
        expanded += 1;
        scratch.metrics.expanded_nodes = scratch.metrics.expanded_nodes.saturating_add(1);
        if expanded > survey.max_nodes {
            break;
        }
        let current_point = if current == start {
            survey.start
        } else {
            survey_point(current)
        };
        // The exact endpoint rarely lies at its rounded grid-cell centre. A
        // nearby cell with a certified final edge is a valid virtual goal and
        // avoids forcing a short corner-cut from the rounded goal cell.
        let reaches_goal = current_point.distance(survey.goal) <= SURVEY_CELL * 1.6
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
            points.extend(cells.into_iter().skip(1).map(survey_point));
            if points
                .last()
                .is_none_or(|point| point.distance_squared(survey.goal) > 0.01)
            {
                points.push(survey.goal);
            }
            return points;
        }

        let current_height = survey.height(current_point, scratch);
        let current_score = scratch
            .score
            .get(&current)
            .copied()
            .unwrap_or(f32::INFINITY);
        for dx in -1..=1 {
            for dz in -1..=1 {
                if dx == 0 && dz == 0 {
                    continue;
                }
                let next = SurveyCell {
                    x: current.x + dx,
                    z: current.z + dz,
                };
                let next_point = survey_point(next);
                // Endpoints alone are insufficient for rotated blockers: two
                // adjacent clear grid points can have an edge that clips a
                // narrow corner. Movement checks the segment, so planning must
                // certify that same segment before accepting it.
                if !survey.line_clear(current_point, next_point, scratch) {
                    continue;
                }
                if dx != 0 && dz != 0 {
                    let side_x = survey_point(SurveyCell {
                        x: current.x + dx,
                        z: current.z,
                    });
                    let side_z = survey_point(SurveyCell {
                        x: current.x,
                        z: current.z + dz,
                    });
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
                let step_cost = distance * (1.0 + rise * 0.7 + texture);
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
    Vec::new()
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

fn doorway_approach(
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

/// Whether an authored crop/yard rectangle has enough horizontal clearance
/// from every collidable prop. This uses circle-vs-rotated-box distance and the
/// same baked horizontal radii as villager movement.
pub(crate) fn rotated_rect_is_clear_of_props(
    center: Vec2,
    half: Vec2,
    rotation: f32,
    extra_clearance: f32,
    colliders: &StaticColliders,
    derived: &DerivedColliderLibrary,
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
) -> PropBlockers {
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
            for prop in cache.chunk(terrain, ChunkCoord::new(x, z)) {
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
    add_static_collider_blockers(
        &mut blockers,
        colliders,
        derived,
        start,
        goal,
        SURVEY_PADDING + PropBlockers::CLEARANCE,
        road_half_width + 0.3,
    );
    blockers
}

fn blockers_for_agent_route(
    terrain: &WorldTerrain,
    start: Vec2,
    goal: Vec2,
    buildings: &[BuildingBlocker],
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
                if buildings
                    .iter()
                    .any(|building| building.contains(prop.point))
                {
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
}

impl NavigationBuildingCache {
    fn rebuild<'a>(
        &mut self,
        placed: impl Iterator<Item = (&'a PlacedBuilding, &'a BuildingPosition)>,
    ) {
        self.buildings.clear();
        self.buildings.extend(placed.map(|(building, position)| {
            let kind = settlement_kind_for_art(building.building_type);
            let half = building.building_type.definition().footprint * 0.5
                + Vec2::splat(crate::world::navgrid::VILLAGER_NAV_RADIUS);
            NavigationBuilding {
                blocker: BuildingBlocker {
                    center: Vec2::new(position.0.x, position.0.z),
                    half,
                    rotation: building.rotation,
                },
                kind,
                position: position.0,
                rotation: building.rotation,
            }
        }));
        self.blockers.clear();
        self.blockers
            .extend(self.buildings.iter().map(|building| building.blocker));
        self.initialized = true;
    }
}

fn settlement_kind_for_art(building_type: BuildingType) -> SettlementBuildingKind {
    match building_type {
        BuildingType::LogCabin => SettlementBuildingKind::House,
        BuildingType::LumberjackHut => SettlementBuildingKind::LumberjackHut,
        BuildingType::Farmstead => SettlementBuildingKind::Farmstead,
        BuildingType::FishermansHut => SettlementBuildingKind::FishermansHut,
        BuildingType::MootHall => SettlementBuildingKind::Hall,
        BuildingType::PlaceholderMarket => SettlementBuildingKind::Market,
        BuildingType::PlaceholderTavern => SettlementBuildingKind::Tavern,
        BuildingType::PlaceholderChurch => SettlementBuildingKind::Church,
    }
}

#[derive(Clone, Copy)]
struct NavigationEndpoint {
    actual: Vec2,
    survey: Vec2,
}

fn navigation_endpoint(point: Vec2, buildings: &[NavigationBuilding]) -> NavigationEndpoint {
    let nearest_door = buildings
        .iter()
        .filter_map(|building| {
            let (door, approach) =
                doorway_approach(building.kind, building.position, building.rotation);
            let distance = point.distance(door);
            (distance <= 1.1).then_some((distance, approach))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    NavigationEndpoint {
        actual: point,
        survey: nearest_door.map_or(point, |(_, approach)| approach),
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

fn polyline_clear_live_world(
    points: &[Vec2],
    buildings: Option<&SpatialObstacleGrid>,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
) -> bool {
    points.windows(2).all(|segment| {
        crate::player::hero::navigation_segment_clear(
            segment[0], segment[1], buildings, colliders, derived,
        )
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
