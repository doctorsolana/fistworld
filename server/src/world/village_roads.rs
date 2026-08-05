//! Builder-made village paths and the tiny movement graph they create.
//!
//! An obstacle-aware grid search is paid once, when a building completes. The
//! resulting polyline is then shared by rendering, prop clearance and cached
//! villager routes. A new endpoint pair pays one bounded local survey suite,
//! then follows cheap waypoints and shared road-graph paths; repeated commutes
//! reuse the complete obstacle-versioned route in either certified direction.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use lightyear::prelude::{NetworkTarget, Replicate};
use shared::building::{BuildingPosition, BuildingType, PlacedBuilding};
use shared::components::{
    BuildingDoorUse, CharacterActivity, CharacterKind, CharacterName, FarmField,
    MootAdministration, Occupation, PlayerPosition, PlayerRotation, RoadClass, RoadSurface,
    Settlement, SettlementBuilding, SettlementBuildingKind, VillageRoad, WorldTime,
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
const ROAD_WATER_FREEBOARD: f32 = 0.25;
/// Grid samples must not ride the exact edge of a continuous field/ribbon
/// intersection. This tiny cushion absorbs rounding without visibly widening
/// the requested crop clearance.
const ROAD_SURVEY_FIELD_EPSILON: f32 = 0.25;
/// Authored entrances sit just outside the wall. Surveying begins beyond a
/// short front apron so A* cannot approach the same door through a side or the
/// rear of the building.
const DOOR_APPROACH_LENGTH: f32 = 2.25;
pub const ROAD_SPEED_MULTIPLIER: f32 = 1.22;

fn surface_width_for_tier(tier: shared::components::SettlementTier, class: RoadClass) -> f32 {
    match (tier, class) {
        (shared::components::SettlementTier::Hamlet, _) => VILLAGE_ROAD_WIDTH,
        (shared::components::SettlementTier::Village, RoadClass::Main) => 3.4,
        (shared::components::SettlementTier::Village, RoadClass::Lane) => 2.8,
        (_, RoadClass::Main) => 4.0,
        (_, RoadClass::Lane) => 3.0,
    }
}

fn road_sample_is_dry_at_width(terrain: &WorldTerrain, point: Vec2, width: f32) -> bool {
    let shoulder = width * 0.5 + 0.2;
    let diagonal = shoulder * std::f32::consts::FRAC_1_SQRT_2;
    [
        Vec2::ZERO,
        Vec2::X * shoulder,
        Vec2::NEG_X * shoulder,
        Vec2::Y * shoulder,
        Vec2::NEG_Y * shoulder,
        Vec2::new(diagonal, diagonal),
        Vec2::new(-diagonal, diagonal),
        Vec2::new(diagonal, -diagonal),
        Vec2::new(-diagonal, -diagonal),
    ]
    .into_iter()
    .all(|offset| {
        let sample = point + offset;
        terrain
            .water_surface_height(sample.x, sample.y)
            .is_none_or(|water| {
                terrain.get_height(sample.x, sample.y) >= water + ROAD_WATER_FREEBOARD
            })
    })
}

fn road_sample_is_dry(terrain: &WorldTerrain, point: Vec2) -> bool {
    road_sample_is_dry_at_width(terrain, point, VILLAGE_ROAD_WIDTH)
}

pub(crate) fn road_segment_is_dry(terrain: &WorldTerrain, start: Vec2, end: Vec2) -> bool {
    road_segment_is_dry_at_width(terrain, start, end, VILLAGE_ROAD_WIDTH)
}

fn road_segment_is_dry_at_width(
    terrain: &WorldTerrain,
    start: Vec2,
    end: Vec2,
    width: f32,
) -> bool {
    let steps = (start.distance(end) / NAVIGATION_SAMPLE_STEP)
        .ceil()
        .max(1.0) as usize;
    (0..=steps).all(|step| {
        road_sample_is_dry_at_width(terrain, start.lerp(end, step as f32 / steps as f32), width)
    })
}

fn road_corridor_is_dry(terrain: &WorldTerrain, points: &[Vec2], width: f32) -> bool {
    points
        .windows(2)
        .all(|pair| road_segment_is_dry_at_width(terrain, pair[0], pair[1], width))
}

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
        {
            true
        } else if !road_sample_is_dry(self.terrain, point) {
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
        BuildingType::TownHall => SettlementBuildingKind::Hall,
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

#[derive(Default)]
struct HallRoadNetwork {
    connected_keys: HashSet<(i32, i32)>,
    connected_points: Vec<Vec2>,
    disconnected_components: usize,
}

/// Build the completed road component that can actually reach the Moot Hall.
///
/// Planned and half-built roads are deliberately excluded. Paths are raised
/// from a building outward, so their visible prefix is not public network yet;
/// letting another house connect to it is how detached road islands formed.
fn hall_road_network(settlement: &str, hall_door: Vec2, roads: &[&VillageRoad]) -> HallRoadNetwork {
    let mut points = HashMap::<(i32, i32), Vec2>::new();
    let mut edges = HashMap::<(i32, i32), HashSet<(i32, i32)>>::new();
    for road in roads
        .iter()
        .copied()
        .filter(|road| road.settlement == settlement && road.is_complete())
    {
        let mut previous = None;
        for point in road.built_points().iter().copied() {
            let key = graph_key(point);
            points.entry(key).or_insert(point);
            edges.entry(key).or_default();
            if let Some(previous) = previous {
                edges.entry(previous).or_default().insert(key);
                edges.entry(key).or_default().insert(previous);
            }
            previous = Some(key);
        }
    }

    let mut queue = VecDeque::new();
    let mut connected_keys = HashSet::new();
    for (key, point) in &points {
        if point.distance_squared(hall_door) <= 0.5_f32.powi(2) {
            connected_keys.insert(*key);
            queue.push_back(*key);
        }
    }
    while let Some(node) = queue.pop_front() {
        for next in edges.get(&node).into_iter().flatten() {
            if connected_keys.insert(*next) {
                queue.push_back(*next);
            }
        }
    }

    let mut connected_points: Vec<_> = connected_keys
        .iter()
        .filter_map(|key| points.get(key).copied())
        .collect();
    connected_points.sort_by(|a, b| a.x.total_cmp(&b.x).then_with(|| a.y.total_cmp(&b.y)));

    let mut unseen: HashSet<_> = points
        .keys()
        .copied()
        .filter(|key| !connected_keys.contains(key))
        .collect();
    let mut disconnected_components = 0usize;
    while let Some(start) = unseen.iter().next().copied() {
        disconnected_components += 1;
        unseen.remove(&start);
        queue.push_back(start);
        while let Some(node) = queue.pop_front() {
            for next in edges.get(&node).into_iter().flatten() {
                if unseen.remove(next) {
                    queue.push_back(*next);
                }
            }
        }
    }

    HallRoadNetwork {
        connected_keys,
        connected_points,
        disconnected_components,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BuildingRoadStatus {
    Connected,
    Pending,
    Disconnected,
    Roadless,
}

fn road_starts_at(road: &VillageRoad, door: Vec2) -> bool {
    road.points
        .first()
        .is_some_and(|point| point.distance_squared(door) <= 0.75_f32.powi(2))
}

fn absolute_world_seconds(clock: &WorldTime) -> f64 {
    f64::from(clock.day) * f64::from(clock.cycle_duration()) + f64::from(clock.seconds_in_cycle)
}

fn building_road_status(
    settlement: &str,
    door: Vec2,
    roads: &[&VillageRoad],
    network: &HallRoadNetwork,
    has_request: bool,
) -> BuildingRoadStatus {
    let touching: Vec<_> = roads
        .iter()
        .copied()
        .filter(|road| road.settlement == settlement && road_starts_at(road, door))
        .collect();
    if touching.iter().any(|road| {
        road.is_complete()
            && road
                .built_points()
                .iter()
                .any(|point| network.connected_keys.contains(&graph_key(*point)))
    }) {
        return BuildingRoadStatus::Connected;
    }
    if has_request || touching.iter().any(|road| !road.is_complete()) {
        return BuildingRoadStatus::Pending;
    }
    if touching.iter().any(|road| road.is_complete()) {
        BuildingRoadStatus::Disconnected
    } else {
        BuildingRoadStatus::Roadless
    }
}

/// Add the Moot Hall's first public office without bloating the founding path.
pub fn ensure_moot_administrations(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    halls: Query<
        (
            Entity,
            Option<&MootAdministration>,
            Option<&MootAdministrationRuntime>,
        ),
        With<Settlement>,
    >,
) {
    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    for (hall, administration, runtime) in halls.iter() {
        let mut entity = commands.entity(hall);
        if administration.is_none() {
            entity.insert(MootAdministration::default());
        }
        if runtime.is_none() {
            entity.insert(MootAdministrationRuntime {
                last_paid_day: day,
                last_audit_at: None,
                road_progress: HashMap::new(),
            });
        }
    }
}

/// Reserve one resident for civic road work and pay their daily public wage.
///
/// Unpaid wages remain explicit arrears when the treasury is short. Coin is
/// transferred directly from the public treasury to the worker's wallet, so
/// the salary neither creates money nor drains a commodity market pool.
pub fn staff_and_pay_road_stewards(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    buildings: Query<&SettlementBuilding>,
    mut halls: Query<(
        Entity,
        &mut Settlement,
        &mut MootAdministration,
        &mut MootAdministrationRuntime,
    )>,
    mut villagers: Query<(
        Entity,
        &CharacterName,
        &VillagerIntent,
        &mut Occupation,
        &mut Wallet,
        Option<&RoadSteward>,
    )>,
) {
    let Some(day) = world_time.iter().next().map(|clock| clock.day) else {
        return;
    };
    let employed: HashSet<&str> = buildings
        .iter()
        .flat_map(|building| building.workers.iter().map(String::as_str))
        .collect();

    for (hall, mut settlement, mut administration, mut runtime) in halls.iter_mut() {
        let current_name = administration.road_steward.clone();
        let current = current_name.as_deref().and_then(|wanted| {
            villagers
                .iter()
                .find(|(_, name, intent, _, _, _)| {
                    name.0 == wanted
                        && intent.settlement() == Some(hall)
                        && intent.counts_as_resident()
                })
                .map(|(entity, ..)| entity)
        });

        if administration.road_steward.is_some() && current.is_none() {
            if let Some(wanted) = current_name.as_deref() {
                if let Some(entity) = villagers
                    .iter()
                    .find(|(_, name, ..)| name.0 == wanted)
                    .map(|(entity, ..)| entity)
                {
                    if let Ok((_, _, _, mut occupation, _, _)) = villagers.get_mut(entity) {
                        if occupation.0.as_deref() == Some("Road Steward") {
                            occupation.0 = None;
                        }
                    }
                    commands.entity(entity).remove::<RoadSteward>();
                }
            }
            administration.road_steward = None;
            administration.wage_arrears = 0;
            runtime.last_paid_day = day;
        }

        let worker = if let Some(worker) = current {
            worker
        } else {
            let candidate = villagers
                .iter()
                .filter(|(_, name, intent, occupation, _, steward)| {
                    matches!(intent, VillagerIntent::Resident { settlement } if *settlement == hall)
                        && occupation.0.is_none()
                        && steward.is_none()
                        && !employed.contains(name.0.as_str())
                })
                .min_by(|a, b| a.1 .0.cmp(&b.1 .0))
                .map(|(entity, ..)| entity);
            let Some(candidate) = candidate else {
                runtime.last_paid_day = day;
                continue;
            };
            let Ok((_, name, _, mut occupation, _, _)) = villagers.get_mut(candidate) else {
                continue;
            };
            occupation.0 = Some("Road Steward".to_string());
            administration.road_steward = Some(name.0.clone());
            administration.road_steward_daily_salary = ROAD_STEWARD_DAILY_SALARY;
            administration.wage_arrears = 0;
            runtime.last_paid_day = day;
            runtime.last_audit_at = None;
            commands
                .entity(candidate)
                .insert(RoadSteward { settlement: hall });
            info!(
                "Village '{}': {} took the public Road Steward position at the Moot Hall",
                settlement.name, name.0
            );
            candidate
        };

        if let Ok((_, _, _, mut occupation, _, steward)) = villagers.get_mut(worker) {
            if occupation.0.as_deref() != Some("Road Steward") {
                occupation.0 = Some("Road Steward".to_string());
            }
            if steward.is_none_or(|steward| steward.settlement != hall) {
                commands
                    .entity(worker)
                    .insert(RoadSteward { settlement: hall });
            }
        }

        let elapsed_days = day.saturating_sub(runtime.last_paid_day);
        if elapsed_days > 0 {
            administration.wage_arrears = administration.wage_arrears.saturating_add(
                administration
                    .road_steward_daily_salary
                    .saturating_mul(u64::from(elapsed_days)),
            );
            runtime.last_paid_day = day;
        }
        let payment = settlement.treasury.min(administration.wage_arrears);
        if payment > 0 {
            if let Ok((_, name, _, _, mut wallet, _)) = villagers.get_mut(worker) {
                settlement.treasury -= payment;
                administration.wage_arrears -= payment;
                wallet.credit(payment);
                info!(
                    "Village '{}': paid {} {:.2} coin in Road Steward wages{}",
                    settlement.name,
                    name.0,
                    payment as f64 / 100.0,
                    if administration.wage_arrears > 0 {
                        " (some wages remain in arrears)"
                    } else {
                        ""
                    }
                );
            }
        }
    }
}

/// Fill the tier-bounded public roster with named residents.
///
/// Guards are intentionally jobs before they are combat AI: the vacancy is
/// visible, it consumes one person's time, and later patrol behaviour can be
/// attached without changing the settlement model. The first city worker is
/// the existing accountable Road Steward.
pub fn staff_public_positions(
    buildings: Query<&SettlementBuilding>,
    mut halls: Query<(Entity, &Settlement, &mut MootAdministration)>,
    mut villagers: Query<(
        Entity,
        &CharacterName,
        &VillagerIntent,
        &mut Occupation,
        Option<&RoadSteward>,
    )>,
) {
    let building_workers: HashSet<String> = buildings
        .iter()
        .flat_map(|building| building.workers.iter().cloned())
        .collect();

    for (hall, settlement, mut administration) in halls.iter_mut() {
        let is_current_resident = |wanted: &str| {
            villagers.iter().any(|(_, name, intent, _, _)| {
                name.0 == wanted && intent.settlement() == Some(hall) && intent.counts_as_resident()
            })
        };
        let mut city_workers = administration.city_workers.clone();
        let mut guards = administration.guards.clone();
        city_workers.retain(|name| is_current_resident(name));
        guards.retain(|name| is_current_resident(name));

        if let Some(steward) = administration.road_steward.clone() {
            if !city_workers.contains(&steward) {
                city_workers.insert(0, steward);
            }
        }

        let desired_workers = usize::from(settlement.tier.public_worker_positions());
        let desired_guards = usize::from(settlement.tier.public_guard_positions());
        // A settlement may advertise jobs it cannot yet fill. Do not add
        // further civic hires past population minus one; vacancies are safer
        // than letting a tiny foundation consume every new arrival at the hall.
        let staffing_budget = settlement.residents.saturating_sub(1) as usize;
        city_workers.truncate(desired_workers);
        guards.truncate(desired_guards);

        while city_workers.len() < desired_workers
            && city_workers.len() + guards.len() < staffing_budget
        {
            let occupied: HashSet<&str> = city_workers
                .iter()
                .chain(guards.iter())
                .map(String::as_str)
                .collect();
            let candidate = villagers
                .iter()
                .filter(|(_, name, intent, occupation, _)| {
                    matches!(intent, VillagerIntent::Resident { settlement } if *settlement == hall)
                        && occupation.0.is_none()
                        && !building_workers.contains(&name.0)
                        && !occupied.contains(name.0.as_str())
                })
                .min_by(|a, b| a.1 .0.cmp(&b.1 .0))
                .map(|(entity, name, _, _, _)| (entity, name.0.clone()));
            let Some((entity, name)) = candidate else {
                break;
            };
            if let Ok((_, _, _, mut occupation, steward)) = villagers.get_mut(entity) {
                if steward.is_none() {
                    occupation.0 = Some("City Worker".to_string());
                }
            }
            city_workers.push(name);
        }

        while guards.len() < desired_guards && city_workers.len() + guards.len() < staffing_budget {
            let occupied: HashSet<&str> = city_workers
                .iter()
                .chain(guards.iter())
                .map(String::as_str)
                .collect();
            let candidate = villagers
                .iter()
                .filter(|(_, name, intent, occupation, _)| {
                    matches!(intent, VillagerIntent::Resident { settlement } if *settlement == hall)
                        && occupation.0.is_none()
                        && !building_workers.contains(&name.0)
                        && !occupied.contains(name.0.as_str())
                })
                .min_by(|a, b| a.1 .0.cmp(&b.1 .0))
                .map(|(entity, name, _, _, _)| (entity, name.0.clone()));
            let Some((entity, name)) = candidate else {
                break;
            };
            if let Ok((_, _, _, mut occupation, _)) = villagers.get_mut(entity) {
                occupation.0 = Some("Town Guard".to_string());
            }
            guards.push(name);
        }

        if administration.city_workers != city_workers {
            administration.city_workers = city_workers;
        }
        if administration.guards != guards {
            administration.guards = guards;
        }
    }
}

/// Periodically audit every completed building against the hall-connected road
/// component and adopt one repair at a time.
///
/// Wages remain daily, but civic safety is not a once-per-day lottery. An
/// unfinished road only counts as active pending work while a live builder
/// still owns its routine; abandoned ribbons are removed before classification
/// so they can never hide a roadless building forever.
#[allow(clippy::too_many_arguments)]
pub fn audit_village_roads(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    mut halls: Query<(
        Entity,
        &Settlement,
        &PlayerPosition,
        Option<&PlayerRotation>,
        &mut MootAdministration,
        &mut MootAdministrationRuntime,
    )>,
    buildings: Query<(
        Entity,
        &SettlementBuilding,
        &PlayerPosition,
        &PlayerRotation,
        Option<&RoadRequest>,
    )>,
    roads: Query<(Entity, &VillageRoad)>,
    road_workers: Query<(
        Entity,
        &VillagerIntent,
        Option<&RoadBuilderRoutine>,
        &PlayerPosition,
        Has<HomeRoutine>,
    )>,
    stewards: Query<(
        Entity,
        &CharacterName,
        &VillagerIntent,
        &RoadSteward,
        Option<&RoadBuilderRoutine>,
    )>,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let day = clock.day;
    let now = absolute_world_seconds(clock);
    let road_owners: HashMap<_, _> = road_workers
        .iter()
        .filter_map(|(builder, intent, routine, position, at_home)| {
            let routine = routine?;
            let owns_current_intent = matches!(
                intent,
                VillagerIntent::RoadBuilding { road, .. } if *road == routine.road
            );
            Some((
                routine.road,
                (
                    builder,
                    Vec2::new(position.0.x, position.0.z),
                    at_home,
                    owns_current_intent,
                ),
            ))
        })
        .collect();

    for (hall, settlement, hall_position, hall_rotation, mut administration, mut runtime) in
        halls.iter_mut()
    {
        if runtime
            .last_audit_at
            .is_some_and(|last| now - last < ROAD_AUDIT_INTERVAL_SECONDS)
        {
            continue;
        }
        let Some((steward, steward_name, intent, _, road_work)) =
            stewards.iter().find(|(_, name, intent, steward, _)| {
                steward.settlement == hall
                    && administration.road_steward.as_deref() == Some(name.0.as_str())
                    && intent.settlement() == Some(hall)
            })
        else {
            continue;
        };
        if road_work.is_some() || !intent.is_settled() {
            continue;
        }

        let mut abandoned_roads = 0usize;
        let mut stalled_roads = 0usize;
        let mut observed_roads = HashSet::new();
        let mut settlement_roads = Vec::new();
        for (entity, road) in roads.iter() {
            if road.settlement != settlement.name {
                continue;
            }
            if road.is_complete() {
                runtime.road_progress.remove(&entity);
                settlement_roads.push(road);
                continue;
            }

            let Some((builder, builder_position, at_home, owns_current_intent)) =
                road_owners.get(&entity).copied()
            else {
                commands.entity(entity).despawn();
                runtime.road_progress.remove(&entity);
                abandoned_roads += 1;
                continue;
            };
            if !owns_current_intent {
                // An old road task must never survive after another system has
                // legitimately given the actor a newer commitment.
                commands.entity(entity).despawn();
                commands.entity(builder).remove::<RoadBuilderRoutine>();
                runtime.road_progress.remove(&entity);
                abandoned_roads += 1;
                continue;
            }

            let observation =
                runtime
                    .road_progress
                    .entry(entity)
                    .or_insert(RoadProgressObservation {
                        built_through: road.built_through,
                        builder_position,
                        last_progress_at: now,
                    });
            let made_progress = observation.built_through != road.built_through
                || observation
                    .builder_position
                    .distance_squared(builder_position)
                    > 0.5f32.powi(2);
            if made_progress || !clock.is_day() {
                *observation = RoadProgressObservation {
                    built_through: road.built_through,
                    builder_position,
                    last_progress_at: now,
                };
            }
            let stalled =
                clock.is_day() && now - observation.last_progress_at >= ROAD_BUILDER_STALL_SECONDS;
            if stalled {
                commands.entity(entity).despawn();
                let mut builder_commands = commands.entity(builder);
                builder_commands
                    .insert(VillagerIntent::Resident { settlement: hall })
                    .remove::<RoadBuilderRoutine>();
                if !at_home {
                    builder_commands
                        .remove::<MoveTarget>()
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>();
                }
                runtime.road_progress.remove(&entity);
                stalled_roads += 1;
                continue;
            }
            observed_roads.insert(entity);
            settlement_roads.push(road);
        }
        runtime
            .road_progress
            .retain(|road, _| observed_roads.contains(road));
        let hall_door3 = SettlementBuildingKind::Hall.entrance_position(
            hall_position.0,
            hall_rotation.map_or(0.0, |rotation| rotation.0),
        );
        let hall_door = Vec2::new(hall_door3.x, hall_door3.z);
        let network = hall_road_network(&settlement.name, hall_door, &settlement_roads);
        let mut roadless = 0usize;
        let mut disconnected = 0usize;
        let mut pending = 0usize;
        let mut repairs = Vec::new();
        for (building_entity, building, position, rotation, request) in buildings
            .iter()
            .filter(|(_, building, ..)| building.settlement == settlement.name)
        {
            let door3 = building.kind.entrance_position(position.0, rotation.0);
            let request_is_active = request.is_some_and(|request| {
                road_workers
                    .get(request.builder)
                    .is_ok_and(|(_, intent, routine, _, _)| {
                        routine.is_none()
                            && (matches!(
                                intent,
                                VillagerIntent::Resident { settlement }
                                    if *settlement == hall
                            ) || matches!(
                                intent,
                                VillagerIntent::Building { settlement, site }
                                    if *settlement == hall && *site == request.completed_site
                            ))
                    })
            });
            let status = building_road_status(
                &settlement.name,
                Vec2::new(door3.x, door3.z),
                &settlement_roads,
                &network,
                request_is_active,
            );
            match status {
                BuildingRoadStatus::Disconnected => {
                    disconnected += 1;
                    repairs.push((0u8, building_entity, building.kind));
                }
                BuildingRoadStatus::Roadless => {
                    roadless += 1;
                    repairs.push((1u8, building_entity, building.kind));
                }
                BuildingRoadStatus::Pending => pending += 1,
                BuildingRoadStatus::Connected => {}
            }
        }
        administration.roadless_buildings = roadless.min(u16::MAX as usize) as u16;
        administration.disconnected_buildings = disconnected.min(u16::MAX as usize) as u16;
        administration.pending_road_buildings = pending.min(u16::MAX as usize) as u16;
        administration.last_road_audit_day = day;
        runtime.last_audit_at = Some(now);

        if abandoned_roads > 0 {
            warn!(
                "Village '{}': Road Steward {} cleared {} abandoned unfinished road(s)",
                settlement.name, steward_name.0, abandoned_roads
            );
        }
        if stalled_roads > 0 {
            warn!(
                "Village '{}': Road Steward {} reclaimed {} connector(s) with no daylight progress for {:.0} world seconds",
                settlement.name,
                steward_name.0,
                stalled_roads,
                ROAD_BUILDER_STALL_SECONDS,
            );
        }

        repairs.sort_unstable_by_key(|(priority, entity, _)| (*priority, entity.to_bits()));
        if let Some((_, building, kind)) = repairs.first().copied() {
            commands.entity(building).insert(RoadRequest {
                builder: steward,
                settlement: hall,
                completed_site: building,
                attempt: 0,
            });
            info!(
                "Village '{}': Road Steward {} found {} roadless, {} disconnected and {} actively pending building(s) across {} detached road component(s); repairing the {}",
                settlement.name,
                steward_name.0,
                roadless,
                disconnected,
                pending,
                network.disconnected_components,
                kind.label(),
            );
        } else if pending > 0 {
            info!(
                "Village '{}': Road Steward {} found every completed connector healthy, with {} building connector(s) still actively under construction",
                settlement.name, steward_name.0, pending
            );
        } else {
            info!(
                "Village '{}': Road Steward {} completed the road audit; every building reaches the Moot Hall",
                settlement.name, steward_name.0
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn plan_requested_roads(
    mut commands: Commands,
    time: Option<Res<Time>>,
    terrain: Option<Res<WorldTerrain>>,
    colliders: Option<Res<StaticColliders>>,
    derived: Option<Res<DerivedColliderLibrary>>,
    requests: Query<(
        Entity,
        &RoadRequest,
        &SettlementBuilding,
        &PlayerPosition,
        &PlayerRotation,
        Option<&RoadSurveyBackoff>,
    )>,
    placed_buildings: Query<(
        &shared::building::PlacedBuilding,
        &shared::building::BuildingPosition,
    )>,
    fields: Query<(&FarmField, &PlayerPosition, &PlayerRotation)>,
    settlements: Query<(&Settlement, &PlayerPosition, Option<&PlayerRotation>)>,
    roads: Query<&VillageRoad>,
    mut builders: Query<
        (
            &CharacterName,
            &mut VillagerIntent,
            &mut CharacterActivity,
            Option<&RoadSteward>,
        ),
        With<CharacterKind>,
    >,
    mut prop_cache: Local<RoutePropChunkCache>,
    mut survey_scratch: Local<SurveyScratch>,
) {
    let Some(terrain) = terrain else { return };
    let now = time.as_ref().map_or(0.0, |time| time.elapsed_secs_f64());
    for (building_entity, request, building, position, rotation, backoff) in requests.iter() {
        if backoff.is_some_and(|backoff| now < backoff.retry_after) {
            continue;
        }
        // A Farmstead and its two separate authored crop plots are published in
        // consecutive systems. Never survey in the one-frame seam between
        // them: a road planned without both field blockers remains physically
        // wrong after the missing field appears. Retaining the request lets the
        // same builder adopt it once the complete field layout is authoritative.
        if building.kind == SettlementBuildingKind::Farmstead
            && fields
                .iter()
                .filter(|(field, _, _)| field.farmstead == position.0)
                .count()
                < shared::components::FARM_FIELDS_PER_FARMSTEAD as usize
        {
            continue;
        }
        let Ok((builder_name, mut intent, mut activity, steward)) =
            builders.get_mut(request.builder)
        else {
            commands
                .entity(building_entity)
                .remove::<RoadRequest>()
                .remove::<RoadSurveyBackoff>();
            continue;
        };
        let may_claim_builder = matches!(
            *intent,
            VillagerIntent::Resident { settlement }
                if settlement == request.settlement
        ) || matches!(
            *intent,
            VillagerIntent::Building { settlement, site }
                if settlement == request.settlement && site == request.completed_site
        );
        if !may_claim_builder {
            // A failed survey intentionally retains its request for retry.
            // During that wait the resident may receive another permit; the
            // old request cannot overwrite that newer Building intent.
            continue;
        }
        let Ok((settlement, hall_position, hall_rotation)) = settlements.get(request.settlement)
        else {
            commands
                .entity(building_entity)
                .remove::<RoadRequest>()
                .remove::<RoadSurveyBackoff>();
            continue;
        };
        let (start, survey_start) = doorway_approach(building.kind, position.0, rotation.0);
        if !road_segment_is_dry(&terrain, start, survey_start) {
            let next_backoff = RoadSurveyBackoff::after_failure(backoff.copied(), now);
            if next_backoff.should_warn() {
                warn!(
                    "Village '{}': the {} doorway has no dry road apron; retry {} in {:.1}s",
                    settlement.name,
                    building.kind.label(),
                    next_backoff.failures,
                    next_backoff.retry_after - now,
                );
            }
            commands.entity(building_entity).insert(next_backoff);
            *intent = VillagerIntent::Resident {
                settlement: request.settlement,
            };
            *activity = CharacterActivity::Idle;
            continue;
        }

        let (hall_door, hall_approach) = doorway_approach(
            SettlementBuildingKind::Hall,
            hall_position.0,
            hall_rotation.map_or(0.0, |rotation| rotation.0),
        );
        let settlement_roads: Vec<_> = roads
            .iter()
            .filter(|road| road.settlement == settlement.name)
            .collect();
        let network = hall_road_network(&settlement.name, hall_door, &settlement_roads);
        let mut existing_goals = network.connected_points;
        existing_goals.sort_by(|a, b| {
            a.distance_squared(start)
                .total_cmp(&b.distance_squared(start))
        });
        existing_goals.dedup_by(|a, b| a.distance_squared(*b) <= 0.01);
        // A nearest point can sit in the wide door apron of the hall or
        // another building. Try several existing network points before the
        // hall fallback; abandoning the whole request after one bad candidate
        // is how completed houses silently ended up roadless.
        let mut candidates: Vec<_> = existing_goals
            .into_iter()
            .take(12)
            .map(|goal| (goal, goal, None))
            .collect();
        candidates.push((hall_door, hall_approach, Some(hall_door)));

        let survey_failure = backoff.map_or(0, |backoff| backoff.failures);
        let seed = stable_road_seed(&settlement.name, start)
            ^ u32::from(request.attempt).wrapping_mul(0x9E37_79B9)
            ^ u32::from(survey_failure).wrapping_mul(0x85EB_CA6B);
        let main_count = settlement_roads
            .iter()
            .filter(|road| road.class == RoadClass::Main)
            .count();
        let selected = candidates
            .into_iter()
            .find_map(|(goal, survey_goal, hall_door_goal)| {
                let class = if hall_door_goal.is_some() && main_count < 2 {
                    RoadClass::Main
                } else {
                    RoadClass::Lane
                };
                let width = surface_width_for_tier(settlement.tier, class);
                let reserved_width = class.initial_reserved_width();
                if hall_door_goal
                    .is_some_and(|door| !road_segment_is_dry(&terrain, survey_goal, door))
                {
                    return None;
                }
                let route_min = survey_start.min(survey_goal) - Vec2::splat(SURVEY_PADDING);
                let route_max = survey_start.max(survey_goal) + Vec2::splat(SURVEY_PADDING);
                let mut building_blockers: Vec<_> = placed_buildings
                    .iter()
                    .filter(|(_, other_position)| {
                        let point = Vec2::new(other_position.0.x, other_position.0.z);
                        point.cmpge(route_min).all() && point.cmple(route_max).all()
                    })
                    .map(|(other, other_position)| {
                        let footprint = other.building_type.definition().footprint;
                        let other_center = Vec2::new(other_position.0.x, other_position.0.z);
                        // The road is intentionally allowed to enter the front
                        // apron of its source building and the Moot Hall. Every
                        // unrelated structure receives the full future corridor.
                        let is_endpoint = other_center
                            .distance_squared(Vec2::new(position.0.x, position.0.z))
                            <= 0.01
                            || other_center
                                .distance_squared(Vec2::new(hall_position.0.x, hall_position.0.z))
                                <= 0.01;
                        let protected_width = if is_endpoint { width } else { reserved_width };
                        BuildingBlocker {
                            center: other_center,
                            half: footprint * 0.5 + Vec2::splat(protected_width * 0.5 + 0.45),
                            rotation: other.rotation,
                        }
                    })
                    .collect();
                building_blockers.extend(
                    fields
                        .iter()
                        .filter(|(field, _, _)| field.settlement == settlement.name)
                        .map(|(_, field_position, field_rotation)| BuildingBlocker {
                            center: Vec2::new(field_position.0.x, field_position.0.z),
                            half: SettlementBuildingKind::Farmstead
                                .field_half_extents()
                                .expect("Farmstead has an authored wheat-field footprint")
                                + Vec2::splat(
                                    reserved_width * 0.5
                                        + shared::components::FARM_FIELD_EDGE_CLEARANCE
                                        + ROAD_SURVEY_FIELD_EPSILON,
                                ),
                            rotation: field_rotation.0,
                        }),
                );
                if building_blockers
                    .iter()
                    .any(|blocker| blocker.contains(survey_goal))
                {
                    return None;
                }
                let prop_blockers = blockers_for_route(
                    &terrain,
                    survey_start,
                    survey_goal,
                    &building_blockers,
                    reserved_width * 0.5,
                    colliders.as_deref(),
                    derived.as_deref(),
                    &mut prop_cache,
                );
                let surveyed = survey_village_road(
                    &terrain,
                    survey_start,
                    survey_goal,
                    &building_blockers,
                    &prop_blockers,
                    seed,
                    &mut survey_scratch,
                );
                if surveyed.len() < 2 {
                    return None;
                }
                // Sampling and simplification are deliberately fast, but the
                // shared continuous ribbon test is the final authority. Run
                // it before accepting a candidate so a tangent grid route can
                // never become a permanent crop overlap.
                let mut certified_points = Vec::with_capacity(surveyed.len() + 2);
                certified_points.push(start);
                certified_points.extend(surveyed.iter().copied());
                if let Some(hall_door) = hall_door_goal {
                    certified_points.push(hall_door);
                }
                let certified = VillageRoad {
                    settlement: settlement.name.clone(),
                    builder: builder_name.0.clone(),
                    built_through: certified_points.len() as u16,
                    points: certified_points,
                    width,
                    reserved_width,
                    surface: RoadSurface::Dirt,
                    class,
                    stone_committed: 0,
                };
                let crosses_field = fields
                    .iter()
                    .filter(|(field, _, _)| field.settlement == settlement.name)
                    .any(|(_, field_position, field_rotation)| {
                        certified.intersects_rotated_rect(
                            Vec2::new(field_position.0.x, field_position.0.z),
                            SettlementBuildingKind::Farmstead
                                .field_half_extents()
                                .expect("Farmstead has an authored wheat-field footprint"),
                            field_rotation.0,
                            shared::components::FARM_FIELD_EDGE_CLEARANCE,
                        )
                    });
                let dry = road_corridor_is_dry(
                    &terrain,
                    &certified.points,
                    certified.reservation_width(),
                );
                (!crosses_field && dry).then_some((
                    goal,
                    surveyed,
                    hall_door_goal,
                    class,
                    width,
                    reserved_width,
                ))
            });

        let Some((_goal, surveyed, hall_door_goal, class, width, reserved_width)) = selected else {
            let next_backoff = RoadSurveyBackoff::after_failure(backoff.copied(), now);
            if next_backoff.should_warn() {
                warn!(
                    "Village '{}': {} could not survey a clear path from the {}; retry {} in {:.1}s",
                    settlement.name,
                    builder_name.0,
                    building.kind.label(),
                    next_backoff.failures,
                    next_backoff.retry_after - now,
                );
            }
            if next_backoff.failures >= MAX_ROAD_SURVEY_ATTEMPTS && steward.is_none() {
                warn!(
                    "Village '{}': {} released the blocked {} connector after {} surveys for Road Steward repair",
                    settlement.name,
                    builder_name.0,
                    building.kind.label(),
                    next_backoff.failures,
                );
                commands
                    .entity(building_entity)
                    .remove::<RoadRequest>()
                    .remove::<RoadSurveyBackoff>();
            } else {
                commands.entity(building_entity).insert(next_backoff);
            }
            *intent = VillagerIntent::Resident {
                settlement: request.settlement,
            };
            *activity = CharacterActivity::Idle;
            continue;
        };

        // The only portion allowed through an inflated building blocker is
        // this authored, straight front apron. The expensive survey starts
        // outside it, so the remaining path cannot choose a rear shortcut.
        let mut points = Vec::with_capacity(surveyed.len() + 2);
        points.push(start);
        points.extend(surveyed);
        if let Some(hall_door) = hall_door_goal {
            points.push(hall_door);
        }

        let road = commands
            .spawn((
                VillageRoad {
                    settlement: settlement.name.clone(),
                    builder: builder_name.0.clone(),
                    points,
                    built_through: 1,
                    width,
                    reserved_width,
                    surface: RoadSurface::Dirt,
                    class,
                    stone_committed: 0,
                },
                Replicate::to_clients(NetworkTarget::All),
            ))
            .id();
        *intent = VillagerIntent::RoadBuilding {
            settlement: request.settlement,
            road,
        };
        *activity = CharacterActivity::Idle;
        // The connector is a new navigation owner. A failed household,
        // ambient, or construction destination left on the actor makes
        // movement deliberately sleep, so adopting the road must clear every
        // route artifact before its first point is published next tick.
        commands
            .entity(request.builder)
            .remove::<MoveTarget>()
            .remove::<TravelRoute>()
            .remove::<NavigationRoutePending>()
            .remove::<NavigationRouteFailed>()
            .insert(RoadBuilderRoutine {
                road,
                settlement: request.settlement,
                attempt: request.attempt,
                phase: RoadBuildPhase::GoingTo { point: 0 },
            });
        commands
            .entity(building_entity)
            .remove::<RoadRequest>()
            .remove::<RoadSurveyBackoff>();
        info!(
            "Village '{}': {} began the path from the {}",
            settlement.name,
            builder_name.0,
            building.kind.label()
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub fn build_village_roads(
    time: Res<Time>,
    warp: Query<&shared::components::TimeWarp>,
    terrain: Option<Res<WorldTerrain>>,
    mut commands: Commands,
    mut roads: Query<&mut VillageRoad>,
    buildings: Query<
        (
            Entity,
            &SettlementBuilding,
            &PlayerPosition,
            &PlayerRotation,
        ),
        Without<CharacterKind>,
    >,
    mut builders: Query<
        (
            Entity,
            &PlayerPosition,
            &mut PlayerRotation,
            &mut CharacterActivity,
            &mut VillagerIntent,
            Option<&MoveTarget>,
            &mut RoadBuilderRoutine,
            Option<&HomeRoutine>,
            Option<&NavigationRouteFailed>,
        ),
        With<CharacterKind>,
    >,
) {
    let Some(terrain) = terrain else { return };
    let factor = warp.iter().next().map_or(1.0, |warp| warp.0);
    let dt = time.delta_secs() * factor;
    for (
        builder,
        position,
        mut facing,
        mut activity,
        mut intent,
        move_target,
        mut routine,
        home,
        route_failed,
    ) in builders.iter_mut()
    {
        if home.is_some() {
            continue;
        }
        let Ok(mut road) = roads.get_mut(routine.road) else {
            *activity = CharacterActivity::Idle;
            *intent = VillagerIntent::Resident {
                settlement: routine.settlement,
            };
            commands
                .entity(builder)
                .remove::<RoadBuilderRoutine>()
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();
            continue;
        };
        if road.points.len() < 2 {
            warn!(
                "Village '{}': discarded a malformed road with fewer than two points",
                road.settlement
            );
            let road_entity = routine.road;
            let settlement = routine.settlement;
            commands.entity(road_entity).despawn();
            finish_road_builder(
                &mut commands,
                builder,
                &mut intent,
                &mut activity,
                settlement,
            );
            continue;
        }

        if let (RoadBuildPhase::GoingTo { point }, Some(failed)) = (routine.phase, route_failed) {
            let point = point.min(road.points.len() - 1);
            let xz = road.points[point];
            let target = Vec3::new(xz.x, terrain.get_height(xz.x, xz.y), xz.y);
            if failed.goal.distance_squared(target) <= 0.01 {
                commands
                    .entity(builder)
                    .remove::<MoveTarget>()
                    .remove::<TravelRoute>()
                    .remove::<NavigationRoutePending>()
                    .remove::<NavigationRouteFailed>();

                if point < usize::from(road.built_through) {
                    // The first point is the already-built doorway anchor. It
                    // need not be revisited merely to advance to point one;
                    // doing so stranded Aud's connector when the exact door
                    // target was rejected despite the surveyed apron beyond it
                    // being reachable.
                    let next = point + 1;
                    if next >= road.points.len() {
                        finish_road_builder(
                            &mut commands,
                            builder,
                            &mut intent,
                            &mut activity,
                            routine.settlement,
                        );
                    } else {
                        routine.phase = RoadBuildPhase::GoingTo { point: next };
                        let xz = road.points[next];
                        commands.entity(builder).insert(MoveTarget(Vec3::new(
                            xz.x,
                            terrain.get_height(xz.x, xz.y),
                            xz.y,
                        )));
                    }
                    continue;
                }

                let start = road.points[0];
                let road_entity = routine.road;
                let next_attempt = routine.attempt.saturating_add(1);
                let building = buildings
                    .iter()
                    .find(|(_, building, position, rotation)| {
                        if building.settlement != road.settlement {
                            return false;
                        }
                        let door = building.kind.entrance_position(position.0, rotation.0);
                        start.distance_squared(Vec2::new(door.x, door.z)) <= 0.75_f32.powi(2)
                    })
                    .map(|(entity, ..)| entity);
                warn!(
                    "Village '{}': {} could not reach road point {} on survey attempt {}; resurveying",
                    road.settlement,
                    road.builder,
                    point,
                    routine.attempt.saturating_add(1)
                );
                commands.entity(road_entity).despawn();
                *intent = VillagerIntent::Resident {
                    settlement: routine.settlement,
                };
                *activity = CharacterActivity::Idle;
                commands.entity(builder).remove::<RoadBuilderRoutine>();
                if next_attempt < MAX_ROAD_SURVEY_ATTEMPTS {
                    if let Some(building) = building {
                        commands.entity(building).insert(RoadRequest {
                            builder,
                            settlement: routine.settlement,
                            completed_site: building,
                            attempt: next_attempt,
                        });
                    }
                } else {
                    warn!(
                        "Road connector exhausted {} surveys; releasing it to the Road Steward audit",
                        MAX_ROAD_SURVEY_ATTEMPTS
                    );
                }
                continue;
            }
        }

        if route_failed.is_some() {
            // This failure belongs to an older household, ambient, or road
            // point destination. Movement intentionally pauses while any
            // NavigationRouteFailed exists, so ignoring a mismatched one
            // strands an otherwise healthy connector forever.
            commands
                .entity(builder)
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();
            if let RoadBuildPhase::GoingTo { point } = routine.phase {
                let xz = road.points[point.min(road.points.len() - 1)];
                commands.entity(builder).insert(MoveTarget(Vec3::new(
                    xz.x,
                    terrain.get_height(xz.x, xz.y),
                    xz.y,
                )));
            }
            continue;
        }

        match routine.phase {
            RoadBuildPhase::GoingTo { point } => {
                *activity = CharacterActivity::Idle;
                let point = point.min(road.points.len() - 1);
                let xz = road.points[point];
                let target = Vec3::new(xz.x, terrain.get_height(xz.x, xz.y), xz.y);
                if ground_distance(position.0, target) > ROAD_REACH {
                    ensure_move_target(&mut commands, builder, move_target, target);
                    continue;
                }
                commands.entity(builder).remove::<MoveTarget>();
                if point < usize::from(road.built_through) {
                    let next = point + 1;
                    if next >= road.points.len() {
                        finish_road_builder(
                            &mut commands,
                            builder,
                            &mut intent,
                            &mut activity,
                            routine.settlement,
                        );
                    } else {
                        routine.phase = RoadBuildPhase::GoingTo { point: next };
                    }
                } else {
                    let direction = if point > 0 {
                        // Face the section being packed, not the untouched
                        // ground ahead. The authored build clip works in front
                        // of the character, so this makes the action meet the
                        // newly appearing ribbon instead of pointing away.
                        road.points[point - 1] - road.points[point]
                    } else {
                        road.points[0] - road.points[1]
                    };
                    if direction.length_squared() > 1e-4 {
                        facing.0 = f32::atan2(-direction.x, -direction.y);
                    }
                    *activity = CharacterActivity::Building;
                    // Arrival and the first work slice happen in the same
                    // fixed tick. Deferring the subtraction until next tick
                    // discarded almost the entire time slice at 100x-1000x,
                    // making roads perversely slower in simulated time as warp
                    // increased. Travel to the next point remains physical.
                    let left = ROAD_BUILD_SECONDS - dt;
                    if left > 0.0 {
                        routine.phase = RoadBuildPhase::Working {
                            point,
                            seconds_left: left,
                        };
                    } else {
                        road.built_through = road
                            .built_through
                            .max((point + 1).min(road.points.len()) as u16);
                        let next = point + 1;
                        if next >= road.points.len() {
                            info!(
                                "Village '{}': {} completed {:.0} m of path",
                                road.settlement,
                                road.builder,
                                road.total_length()
                            );
                            finish_road_builder(
                                &mut commands,
                                builder,
                                &mut intent,
                                &mut activity,
                                routine.settlement,
                            );
                        } else {
                            // Keep the packing pose observable for this server
                            // frame even when warp consumed the whole timer.
                            // The next GoingTo tick returns to Idle before the
                            // villager starts toward the following point.
                            routine.phase = RoadBuildPhase::GoingTo { point: next };
                            let xz = road.points[next];
                            commands.entity(builder).insert(MoveTarget(Vec3::new(
                                xz.x,
                                terrain.get_height(xz.x, xz.y),
                                xz.y,
                            )));
                        }
                    }
                }
            }
            RoadBuildPhase::Working {
                point,
                seconds_left,
            } => {
                *activity = CharacterActivity::Building;
                commands.entity(builder).remove::<MoveTarget>();
                let left = seconds_left - dt;
                if left > 0.0 {
                    routine.phase = RoadBuildPhase::Working {
                        point,
                        seconds_left: left,
                    };
                    continue;
                }
                road.built_through = road
                    .built_through
                    .max((point + 1).min(road.points.len()) as u16);
                let next = point + 1;
                if next >= road.points.len() {
                    info!(
                        "Village '{}': {} completed {:.0} m of path",
                        road.settlement,
                        road.builder,
                        road.total_length()
                    );
                    finish_road_builder(
                        &mut commands,
                        builder,
                        &mut intent,
                        &mut activity,
                        routine.settlement,
                    );
                } else {
                    *activity = CharacterActivity::Idle;
                    routine.phase = RoadBuildPhase::GoingTo { point: next };
                    // Movement runs later in the fixed schedule, so publish
                    // the next physical destination now. Waiting for another
                    // server tick merely to create this target made extreme
                    // time warp lose one whole tick per road point.
                    let xz = road.points[next];
                    commands.entity(builder).insert(MoveTarget(Vec3::new(
                        xz.x,
                        terrain.get_height(xz.x, xz.y),
                        xz.y,
                    )));
                }
            }
        }
    }
}

fn finish_road_builder(
    commands: &mut Commands,
    builder: Entity,
    intent: &mut VillagerIntent,
    activity: &mut CharacterActivity,
    settlement: Entity,
) {
    *intent = VillagerIntent::Resident { settlement };
    *activity = CharacterActivity::Idle;
    commands
        .entity(builder)
        .remove::<RoadBuilderRoutine>()
        .remove::<MoveTarget>()
        .remove::<TravelRoute>()
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>();
}

fn ensure_move_target(
    commands: &mut Commands,
    entity: Entity,
    current: Option<&MoveTarget>,
    expected: Vec3,
) {
    if current.is_none_or(|target| ground_distance(target.0, expected) > 0.05) {
        commands.entity(entity).insert(MoveTarget(expected));
    }
}

fn ground_distance(a: Vec3, b: Vec3) -> f32 {
    Vec2::new(a.x - b.x, a.z - b.z).length()
}

fn stable_road_seed(settlement: &str, start: Vec2) -> u32 {
    settlement.bytes().fold(2_166_136_261u32, |hash, byte| {
        (hash ^ byte as u32).wrapping_mul(16_777_619)
    }) ^ start.x.to_bits().rotate_left(7)
        ^ start.y.to_bits().rotate_left(19)
}

// -- Shared, cached movement graph -------------------------------------------------------------

#[derive(Default)]
struct RoadGraphNode {
    point: Vec2,
    edges: Vec<(usize, f32)>,
}

#[derive(Resource, Default)]
pub struct VillageRoadGraph {
    nodes: Vec<RoadGraphNode>,
    routes: HashMap<(usize, usize), Vec<usize>>,
    tactical_routes: HashMap<TacticalRouteKey, Vec<(Vec2, bool)>>,
    tactical_order: VecDeque<TacticalRouteKey>,
    tactical_obstacle_version: Option<u64>,
    road_revision: u64,
    initialized: bool,
}

const MAX_TACTICAL_ROUTE_CACHE_ENTRIES: usize = 8_192;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct TacticalRoutePoint(u32, u32);

impl From<Vec2> for TacticalRoutePoint {
    fn from(point: Vec2) -> Self {
        // Exact endpoints preserve the planner's collision proof. Work and
        // household destinations are stable authored points, so repeated
        // commutes still hit without joining an approximate cached start.
        Self(point.x.to_bits(), point.y.to_bits())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct TacticalRouteKey {
    start: TacticalRoutePoint,
    goal: TacticalRoutePoint,
}

impl TacticalRouteKey {
    fn new(start: Vec2, goal: Vec2) -> Self {
        Self {
            start: start.into(),
            goal: goal.into(),
        }
    }

    fn reversed(self) -> Self {
        Self {
            start: self.goal,
            goal: self.start,
        }
    }
}

fn graph_key(point: Vec2) -> (i32, i32) {
    (
        (point.x * 10.0).round() as i32,
        (point.y * 10.0).round() as i32,
    )
}

pub fn rebuild_village_road_graph(
    mut graph: ResMut<VillageRoadGraph>,
    roads: Query<&VillageRoad>,
    changed: Query<(), Changed<VillageRoad>>,
    mut removed: RemovedComponents<VillageRoad>,
) {
    let removed_any = removed.read().next().is_some();
    if graph.initialized && changed.is_empty() && !removed_any {
        return;
    }
    graph.nodes.clear();
    graph.routes.clear();
    graph.clear_tactical_routes();
    graph.road_revision = graph.road_revision.wrapping_add(1);
    let mut by_point = HashMap::<(i32, i32), usize>::new();
    for road in roads.iter() {
        let mut previous: Option<usize> = None;
        for point in road.built_points() {
            let key = graph_key(*point);
            let node = *by_point.entry(key).or_insert_with(|| {
                let index = graph.nodes.len();
                graph.nodes.push(RoadGraphNode {
                    point: *point,
                    edges: Vec::new(),
                });
                index
            });
            if let Some(previous) = previous {
                let distance = graph.nodes[previous].point.distance(*point);
                if distance > 0.01 {
                    graph.nodes[previous].edges.push((node, distance));
                    graph.nodes[node].edges.push((previous, distance));
                }
            }
            previous = Some(node);
        }
    }
    graph.initialized = true;
}

#[derive(Clone, Copy)]
struct GraphOpen {
    cost: i32,
    node: usize,
}
impl Eq for GraphOpen {}
impl PartialEq for GraphOpen {
    fn eq(&self, other: &Self) -> bool {
        self.cost == other.cost && self.node == other.node
    }
}
impl Ord for GraphOpen {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .cmp(&self.cost)
            .then_with(|| self.node.cmp(&other.node))
    }
}
impl PartialOrd for GraphOpen {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl VillageRoadGraph {
    fn clear_tactical_routes(&mut self) {
        self.tactical_routes.clear();
        self.tactical_order.clear();
    }

    fn sync_tactical_obstacle_version(&mut self, obstacle_version: u64) {
        if self.tactical_obstacle_version != Some(obstacle_version) {
            self.clear_tactical_routes();
            self.tactical_obstacle_version = Some(obstacle_version);
        }
    }

    fn tactical_route(&self, start: Vec2, goal: Vec2) -> Option<&[(Vec2, bool)]> {
        self.tactical_routes
            .get(&TacticalRouteKey::new(start, goal))
            .map(Vec::as_slice)
    }

    fn cache_tactical_route(
        &mut self,
        start: Vec2,
        goal: Vec2,
        route: &[(Vec2, bool)],
        reverse_is_certified: bool,
    ) {
        let key = TacticalRouteKey::new(start, goal);
        self.cache_tactical_route_one(key, route.to_vec());

        if reverse_is_certified {
            let mut reverse = route.to_vec();
            reverse.reverse();
            self.cache_tactical_route_one(key.reversed(), reverse);
        }
    }

    fn cache_tactical_route_one(&mut self, key: TacticalRouteKey, route: Vec<(Vec2, bool)>) {
        if self.tactical_routes.insert(key, route).is_none() {
            self.tactical_order.push_back(key);
        }
        while self.tactical_routes.len() > MAX_TACTICAL_ROUTE_CACHE_ENTRIES {
            let Some(oldest) = self.tactical_order.pop_front() else {
                break;
            };
            self.tactical_routes.remove(&oldest);
        }
    }

    fn nearest_candidates(
        &self,
        point: Vec2,
        max_distance: f32,
        count: usize,
    ) -> Vec<(usize, f32)> {
        let mut candidates: Vec<_> = self
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| {
                let distance = node.point.distance(point);
                (distance <= max_distance).then_some((index, distance))
            })
            .collect();
        candidates.sort_by(|a, b| a.1.total_cmp(&b.1));
        candidates.truncate(count);
        candidates
    }

    fn shortest_path(&mut self, start: usize, goal: usize) -> Option<Vec<usize>> {
        if let Some(path) = self.routes.get(&(start, goal)) {
            return Some(path.clone());
        }
        let mut open = BinaryHeap::new();
        let mut score = vec![f32::INFINITY; self.nodes.len()];
        let mut previous = vec![None; self.nodes.len()];
        score[start] = 0.0;
        open.push(GraphOpen {
            cost: 0,
            node: start,
        });
        while let Some(GraphOpen { cost, node }) = open.pop() {
            if cost > (score[node] * 1000.0) as i32 {
                continue;
            }
            if node == goal {
                let mut path = vec![goal];
                let mut cursor = goal;
                while let Some(parent) = previous[cursor] {
                    path.push(parent);
                    cursor = parent;
                }
                path.reverse();
                self.routes.insert((start, goal), path.clone());
                let mut reverse = path.clone();
                reverse.reverse();
                self.routes.insert((goal, start), reverse);
                return Some(path);
            }
            for &(next, edge) in &self.nodes[node].edges {
                let tentative = score[node] + edge;
                if tentative >= score[next] {
                    continue;
                }
                score[next] = tentative;
                previous[next] = Some(node);
                open.push(GraphOpen {
                    cost: (tentative * 1000.0) as i32,
                    node: next,
                });
            }
        }
        None
    }
}

/// Record changed villager destinations before movement. Route work is split
/// into a bounded second system, so a crowd receiving jobs on one tick cannot
/// create an unbounded A* spike.
pub fn queue_villager_travel_routes(
    mut commands: Commands,
    movers: Query<
        (
            Entity,
            &CharacterKind,
            &MoveTarget,
            Option<&NavigationRouteFailed>,
        ),
        (
            Changed<MoveTarget>,
            Without<BuildingDoorUse>,
            Without<PierTraversal>,
        ),
    >,
) {
    for (entity, kind, target, failed) in movers.iter() {
        if *kind != CharacterKind::Villager {
            continue;
        }
        // Several high-level routines deliberately re-assert their current
        // destination when a work phase changes. Inserting the same value
        // still marks a Bevy component as Changed; without this guard that
        // write erased a static failure and launched the identical A* search
        // again every three fixed ticks.
        if failed.is_some_and(|failed| failed.goal.distance_squared(target.0) <= 0.01) {
            continue;
        }
        commands
            .entity(entity)
            .remove::<TravelRoute>()
            .remove::<NavigationRouteFailed>()
            .insert(NavigationRoutePending::new(target.0));
    }
}

/// A static failure sleeps until the AI chooses a genuinely new destination.
///
/// A global obstacle version is intentionally not a retry trigger: completing
/// any house changes that version, even if the failed point is on the opposite
/// side of the village. At high time warp that used to wake every blocked
/// villager repeatedly while a settlement was expanding.
pub fn retry_failed_routes_after_obstacle_change(
    mut commands: Commands,
    failed: Query<(Entity, &MoveTarget, &NavigationRouteFailed)>,
) {
    for (entity, target, failed) in failed.iter() {
        if failed.goal.distance_squared(target.0) > 0.01 {
            commands
                .entity(entity)
                .remove::<NavigationRouteFailed>()
                .insert(NavigationRoutePending::new(target.0));
        }
    }
}

#[derive(Clone)]
struct RoadRouteCandidate {
    nodes: Vec<usize>,
    road_length: f32,
    estimate: f32,
}

fn append_tagged(points: &mut Vec<(Vec2, bool)>, point: Vec2, on_road: bool) {
    if let Some((last, last_on_road)) = points.last_mut() {
        if last.distance_squared(point) <= 0.01 {
            *last_on_road |= on_road;
            return;
        }
    }
    points.push((point, on_road));
}

/// Goal interactions may finish inside a prop's work radius, while a route's
/// start only exempts its exact first sample. Every other clearance predicate
/// is direction-symmetric, so reverse caching only needs to re-check samples
/// which were protected by the original goal exemption.
fn reverse_route_clears_goal_prop_exemption(route: &[(Vec2, bool)], props: &PropBlockers) -> bool {
    let (Some((forward_start, _)), Some((forward_goal, _))) = (route.first(), route.last()) else {
        return true;
    };
    route.windows(2).all(|segment| {
        let start = segment[0].0;
        let end = segment[1].0;
        let steps = (start.distance(end) / NAVIGATION_SAMPLE_STEP)
            .ceil()
            .max(1.0) as usize;
        (0..=steps).all(|step| {
            let point = start.lerp(end, step as f32 / steps as f32);
            let used_forward_goal_exemption =
                point.distance_squared(*forward_goal) < 2.0f32.powi(2);
            if !used_forward_goal_exemption {
                return true;
            }
            let reverse_endpoint_clear = point.distance_squared(*forward_goal)
                <= (NAVIGATION_SAMPLE_STEP * 0.25).powi(2)
                || point.distance_squared(*forward_start) < 2.0f32.powi(2);
            reverse_endpoint_clear || !props.blocks(point)
        })
    })
}

pub(crate) struct RoutePlannerTelemetry {
    last_report: Instant,
    invocations: u64,
    requests: u64,
    routes_installed: u64,
    failed_attempts: u64,
    cache_hits: u64,
    cache_misses: u64,
    budget_yields: u64,
    blocker_rebuilds: u64,
    pending_peak: usize,
    surveys: u64,
    expanded_nodes: u64,
    blocked_checks: u64,
    blocked_cache_hits: u64,
    line_checks: u64,
    line_cache_hits: u64,
    blocker_time: Duration,
    prop_time: Duration,
    direct_survey_time: Duration,
    graph_time: Duration,
    connector_survey_time: Duration,
    certification_time: Duration,
    total_time: Duration,
    max_invocation: Duration,
}

impl Default for RoutePlannerTelemetry {
    fn default() -> Self {
        Self {
            last_report: Instant::now(),
            invocations: 0,
            requests: 0,
            routes_installed: 0,
            failed_attempts: 0,
            cache_hits: 0,
            cache_misses: 0,
            budget_yields: 0,
            blocker_rebuilds: 0,
            pending_peak: 0,
            surveys: 0,
            expanded_nodes: 0,
            blocked_checks: 0,
            blocked_cache_hits: 0,
            line_checks: 0,
            line_cache_hits: 0,
            blocker_time: Duration::ZERO,
            prop_time: Duration::ZERO,
            direct_survey_time: Duration::ZERO,
            graph_time: Duration::ZERO,
            connector_survey_time: Duration::ZERO,
            certification_time: Duration::ZERO,
            total_time: Duration::ZERO,
            max_invocation: Duration::ZERO,
        }
    }
}

impl RoutePlannerTelemetry {
    fn record_surveys(&mut self, metrics: SurveyMetrics) {
        self.surveys = self.surveys.saturating_add(metrics.searches);
        self.expanded_nodes = self.expanded_nodes.saturating_add(metrics.expanded_nodes);
        self.blocked_checks = self.blocked_checks.saturating_add(metrics.blocked_checks);
        self.blocked_cache_hits = self
            .blocked_cache_hits
            .saturating_add(metrics.blocked_cache_hits);
        self.line_checks = self.line_checks.saturating_add(metrics.line_checks);
        self.line_cache_hits = self.line_cache_hits.saturating_add(metrics.line_cache_hits);
    }

    fn finish_invocation(&mut self, started: Instant) {
        let elapsed = started.elapsed();
        self.invocations = self.invocations.saturating_add(1);
        self.total_time += elapsed;
        self.max_invocation = self.max_invocation.max(elapsed);
    }

    fn maybe_report(&mut self, graph: &VillageRoadGraph) {
        if self.last_report.elapsed() < Duration::from_secs(10) {
            return;
        }
        let total_lookups = self.cache_hits + self.cache_misses;
        let cache_hit_percent = if total_lookups == 0 {
            0.0
        } else {
            self.cache_hits as f64 / total_lookups as f64 * 100.0
        };
        let blocked_hit_percent = if self.blocked_checks == 0 {
            0.0
        } else {
            self.blocked_cache_hits as f64 / self.blocked_checks as f64 * 100.0
        };
        let line_hit_percent = if self.line_checks == 0 {
            0.0
        } else {
            self.line_cache_hits as f64 / self.line_checks as f64 * 100.0
        };
        info!(
            "VillageRoutePerf calls={} requests={} installed={} failed_attempts={} pending_peak={} budget_yields={} cache={}/{} ({:.1}%) entries={} road_rev={} blockers_rebuilt={} surveys={} expanded={} memo_blocked={:.1}% memo_lines={:.1}% time_ms blockers={:.2} props={:.2} direct={:.2} graph={:.2} connectors={:.2} certify={:.2} total={:.2} max_call={:.2}",
            self.invocations,
            self.requests,
            self.routes_installed,
            self.failed_attempts,
            self.pending_peak,
            self.budget_yields,
            self.cache_hits,
            total_lookups,
            cache_hit_percent,
            graph.tactical_routes.len(),
            graph.road_revision,
            self.blocker_rebuilds,
            self.surveys,
            self.expanded_nodes,
            blocked_hit_percent,
            line_hit_percent,
            self.blocker_time.as_secs_f64() * 1_000.0,
            self.prop_time.as_secs_f64() * 1_000.0,
            self.direct_survey_time.as_secs_f64() * 1_000.0,
            self.graph_time.as_secs_f64() * 1_000.0,
            self.connector_survey_time.as_secs_f64() * 1_000.0,
            self.certification_time.as_secs_f64() * 1_000.0,
            self.total_time.as_secs_f64() * 1_000.0,
            self.max_invocation.as_secs_f64() * 1_000.0,
        );
        *self = Self::default();
    }
}

fn install_tactical_route(
    commands: &mut Commands,
    entity: Entity,
    target: Vec3,
    goal_actual: Vec2,
    terrain: &WorldTerrain,
    tagged: &[(Vec2, bool)],
) {
    let mut waypoints = Vec::with_capacity(tagged.len());
    for (point, on_road) in tagged.iter().copied().skip(1) {
        let is_goal = point.distance_squared(goal_actual) <= 0.01;
        waypoints.push(RouteWaypoint {
            position: if is_goal {
                target
            } else {
                Vec3::new(point.x, terrain.get_height(point.x, point.y), point.y)
            },
            on_road,
        });
    }
    if waypoints.is_empty() {
        waypoints.push(RouteWaypoint {
            position: target,
            on_road: false,
        });
    }
    commands
        .entity(entity)
        .insert(TravelRoute {
            goal: target,
            waypoints,
            next: 0,
        })
        .remove::<NavigationRoutePending>();
}

#[allow(clippy::too_many_arguments)]
pub fn plan_villager_travel_routes(
    terrain: Option<Res<WorldTerrain>>,
    collision: TravelCollisionResources,
    budget: Res<PathfindingBudgetSettings>,
    mut next_request: Local<usize>,
    mut commands: Commands,
    mut graph: ResMut<VillageRoadGraph>,
    mut prop_cache: Local<RoutePropChunkCache>,
    mut survey_scratch: Local<SurveyScratch>,
    mut building_cache: Local<NavigationBuildingCache>,
    mut telemetry: Local<RoutePlannerTelemetry>,
    placed_buildings: Query<(&PlacedBuilding, &BuildingPosition)>,
    changed_buildings: Query<(), Or<(Changed<PlacedBuilding>, Changed<BuildingPosition>)>>,
    mut removed_buildings: RemovedComponents<PlacedBuilding>,
    mut movers: Query<
        (
            Entity,
            &PlayerPosition,
            &MoveTarget,
            &mut NavigationRoutePending,
        ),
        (
            With<CharacterKind>,
            Without<BuildingDoorUse>,
            Without<PierTraversal>,
        ),
    >,
    stale: Query<Entity, (With<NavigationRoutePending>, Without<MoveTarget>)>,
) {
    let planner_started = Instant::now();
    let Some(terrain) = terrain else { return };
    let obstacles = collision.obstacles.as_deref();
    let colliders = collision.colliders.as_deref();
    let derived = collision.derived.as_deref();
    let building_obstacle_version = obstacles.map_or(0, |grid| grid.version);
    let prop_obstacle_version = colliders.map_or(0, |props| props.version);
    // Tactical cache entries are certified against both kinds of collision.
    // A streamed or newly cleared prop must invalidate them just as surely as
    // a newly completed building.
    let obstacle_version =
        building_obstacle_version ^ prop_obstacle_version.rotate_left(29) ^ 0x9E37_79B9_7F4A_7C15;
    graph.sync_tactical_obstacle_version(obstacle_version);

    let removed_any = removed_buildings.read().next().is_some();
    if !building_cache.initialized || !changed_buildings.is_empty() || removed_any {
        let rebuild_started = Instant::now();
        building_cache.rebuild(placed_buildings.iter());
        // The live obstacle grid normally invalidates this too. Explicitly
        // clear here so tests and lightweight labs without that grid retain
        // the same safety guarantee.
        graph.clear_tactical_routes();
        telemetry.blocker_rebuilds = telemetry.blocker_rebuilds.saturating_add(1);
        telemetry.blocker_time += rebuild_started.elapsed();
    }
    for entity in stale.iter() {
        commands.entity(entity).remove::<NavigationRoutePending>();
    }
    if movers.is_empty() {
        telemetry.finish_invocation(planner_started);
        telemetry.maybe_report(&graph);
        return;
    }

    // A stable query order plus a hard per-tick budget can starve the last
    // villagers forever when people near the front continually receive new
    // destinations. Rotate the first request every tick so bounded planning
    // remains fair even for crowds and at very high time warp.
    let mut request_order: Vec<_> = movers.iter_mut().map(|(entity, ..)| entity).collect();
    request_order.sort_unstable_by_key(|entity| entity.to_bits());
    let request_count = request_order.len();
    telemetry.pending_peak = telemetry.pending_peak.max(request_count);
    let start = *next_request % request_count;
    request_order.rotate_left(start);
    let mut processed = 0usize;
    let mut visited = 0usize;
    for entity in request_order {
        visited += 1;
        let Ok((entity, position, target, mut pending)) = movers.get_mut(entity) else {
            continue;
        };
        let local_distance = position.0.distance(target.0);
        let survey_max_nodes = if (EXTENDED_LOCAL_SURVEY_MIN_DISTANCE
            ..=EXTENDED_LOCAL_SURVEY_MAX_DISTANCE)
            .contains(&local_distance)
        {
            EXTENDED_LOCAL_SURVEY_MAX_NODES
        } else {
            AGENT_SURVEY_MAX_NODES
        };
        if pending.goal.distance_squared(target.0) > 0.01 {
            *pending = NavigationRoutePending::new(target.0);
        }
        // Never let one corrupt, stale or hostile destination expand prop
        // generation and A* over an unbounded rectangle. Valid long-distance
        // movement remains inside the active map; strategic travel will later
        // replace embodied cross-world routing altogether.
        if !position.0.is_finite()
            || !target.0.is_finite()
            || !world_pos_in_bounds(position.0.x, position.0.z)
            || !world_pos_in_bounds(target.0.x, target.0.z)
        {
            commands
                .entity(entity)
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>();
            continue;
        }
        if pending.attempts >= 3 {
            if pending.obstacle_version == obstacle_version {
                continue;
            }
            pending.attempts = 0;
        }
        if processed >= budget.max_requests_per_tick
            || (processed > 0 && planner_started.elapsed() >= budget.max_duration())
        {
            // This request has not had its turn yet; begin with it next tick.
            telemetry.budget_yields = telemetry.budget_yields.saturating_add(1);
            visited -= 1;
            break;
        }
        processed += 1;
        telemetry.requests = telemetry.requests.saturating_add(1);

        let start = prop_safe_navigation_endpoint(
            navigation_endpoint(
                Vec2::new(position.0.x, position.0.z),
                &building_cache.buildings,
            ),
            colliders,
            derived,
        );
        let goal = prop_safe_navigation_endpoint(
            navigation_endpoint(Vec2::new(target.0.x, target.0.z), &building_cache.buildings),
            colliders,
            derived,
        );

        if let Some(cached) = graph.tactical_route(start.actual, goal.actual) {
            // Tactical routes are cleared whenever the live obstacle version,
            // placed-building cache, or built road graph changes. A cache hit
            // is therefore already certified for this exact geometry; cloning
            // and resampling the whole polyline here cost roughly 0.5 seconds
            // per ten-second player trace with no additional safety.
            telemetry.cache_hits = telemetry.cache_hits.saturating_add(1);
            telemetry.routes_installed = telemetry.routes_installed.saturating_add(1);
            install_tactical_route(
                &mut commands,
                entity,
                target.0,
                goal.actual,
                &terrain,
                cached,
            );
            continue;
        }
        telemetry.cache_misses = telemetry.cache_misses.saturating_add(1);

        // Scatter deterministic collidable props once for this order, covering
        // the direct path and every possible road connector.
        let prop_started = Instant::now();
        let prop_blockers = blockers_for_agent_route(
            &terrain,
            start.survey,
            goal.survey,
            &building_cache.blockers,
            derived,
            colliders,
            ROAD_ROUTE_JOIN_DISTANCE,
            &mut prop_cache,
        );
        telemetry.prop_time += prop_started.elapsed();
        // Being near a doorway changes the route-survey endpoint to the
        // building's front apron. These short joins still need certification:
        // blindly drawing actual -> apron can cut the corner of this or a
        // neighboring building, after which embodied movement rejects the same
        // cached route forever.
        let direct_started = Instant::now();
        let survey_metrics_before = survey_scratch.metrics;
        let start_apron = survey_agent_route(
            &terrain,
            start.actual,
            start.survey,
            &building_cache.blockers,
            obstacles,
            &prop_blockers,
            &mut survey_scratch,
            survey_max_nodes,
        );
        let goal_apron = survey_agent_route(
            &terrain,
            goal.survey,
            goal.actual,
            &building_cache.blockers,
            obstacles,
            &prop_blockers,
            &mut survey_scratch,
            survey_max_nodes,
        );
        let direct_middle = survey_agent_route(
            &terrain,
            start.survey,
            goal.survey,
            &building_cache.blockers,
            obstacles,
            &prop_blockers,
            &mut survey_scratch,
            survey_max_nodes,
        );
        telemetry.direct_survey_time += direct_started.elapsed();
        telemetry.record_surveys(survey_scratch.metrics - survey_metrics_before);
        let direct = complete_agent_route(start, goal, &start_apron, direct_middle, &goal_apron);
        let direct_length = (!direct.is_empty()).then(|| polyline_length(&direct));

        let graph_started = Instant::now();
        let start_candidates = graph.nearest_candidates(
            start.survey,
            ROAD_ROUTE_JOIN_DISTANCE,
            ROAD_ROUTE_CANDIDATES,
        );
        let goal_candidates =
            graph.nearest_candidates(goal.survey, ROAD_ROUTE_JOIN_DISTANCE, ROAD_ROUTE_CANDIDATES);
        let mut road_candidates = Vec::new();
        for &(start_node, start_distance) in &start_candidates {
            for &(goal_node, goal_distance) in &goal_candidates {
                if start_node == goal_node {
                    continue;
                }
                let Some(nodes) = graph.shortest_path(start_node, goal_node) else {
                    continue;
                };
                let road_length: f32 = nodes
                    .windows(2)
                    .map(|pair| {
                        graph.nodes[pair[0]]
                            .point
                            .distance(graph.nodes[pair[1]].point)
                    })
                    .sum();
                if road_length < 2.0 {
                    continue;
                }
                road_candidates.push(RoadRouteCandidate {
                    nodes,
                    road_length,
                    estimate: start_distance + goal_distance + road_length / ROAD_SPEED_MULTIPLIER,
                });
            }
        }
        road_candidates.sort_by(|a, b| a.estimate.total_cmp(&b.estimate));
        telemetry.graph_time += graph_started.elapsed();

        let mut chosen_road: Option<(Vec<(Vec2, bool)>, f32)> = None;
        for candidate in road_candidates
            .into_iter()
            .take(AGENT_ROAD_CANDIDATES_TO_SURVEY)
        {
            if direct_length
                .is_some_and(|direct| candidate.estimate > direct * ROAD_MAX_WEIGHTED_DETOUR)
            {
                continue;
            }
            let first = graph.nodes[*candidate.nodes.first().unwrap()].point;
            let last = graph.nodes[*candidate.nodes.last().unwrap()].point;
            let connectors_started = Instant::now();
            let survey_metrics_before = survey_scratch.metrics;
            let start_connector = survey_agent_route(
                &terrain,
                start.survey,
                first,
                &building_cache.blockers,
                obstacles,
                &prop_blockers,
                &mut survey_scratch,
                survey_max_nodes,
            );
            let goal_connector = survey_agent_route(
                &terrain,
                last,
                goal.survey,
                &building_cache.blockers,
                obstacles,
                &prop_blockers,
                &mut survey_scratch,
                survey_max_nodes,
            );
            telemetry.connector_survey_time += connectors_started.elapsed();
            telemetry.record_surveys(survey_scratch.metrics - survey_metrics_before);
            if start_connector.is_empty() || goal_connector.is_empty() {
                continue;
            }
            let connector_length = polyline_length(&start_apron)
                + polyline_length(&start_connector)
                + polyline_length(&goal_connector)
                + polyline_length(&goal_apron);
            let weighted = connector_length + candidate.road_length / ROAD_SPEED_MULTIPLIER;
            if direct_length.is_some_and(|direct| weighted > direct * ROAD_MAX_WEIGHTED_DETOUR) {
                continue;
            }

            let mut route = Vec::new();
            for point in start_apron.iter().copied() {
                append_tagged(&mut route, point, false);
            }
            for point in start_connector {
                append_tagged(&mut route, point, false);
            }
            for node in candidate.nodes {
                append_tagged(&mut route, graph.nodes[node].point, true);
            }
            for point in goal_connector {
                append_tagged(&mut route, point, false);
            }
            for point in goal_apron.iter().copied() {
                append_tagged(&mut route, point, false);
            }
            if obstacles.is_some_and(|grid| {
                let points: Vec<_> = route.iter().map(|(point, _)| *point).collect();
                !polyline_clear_live_buildings(&points, grid)
            }) {
                continue;
            }
            chosen_road = Some((route, weighted));
            break;
        }

        let mut tagged: Vec<(Vec2, bool)> = if let Some((road, road_cost)) = chosen_road {
            if direct_length.is_none_or(|direct| road_cost <= direct * ROAD_MAX_WEIGHTED_DETOUR) {
                road
            } else {
                direct.iter().copied().map(|point| (point, false)).collect()
            }
        } else if !direct.is_empty() {
            direct.iter().copied().map(|point| (point, false)).collect()
        } else {
            telemetry.failed_attempts = telemetry.failed_attempts.saturating_add(1);
            pending.attempts = pending.attempts.saturating_add(1);
            pending.obstacle_version = obstacle_version;
            if pending.attempts == 3 {
                warn!(
                    "Villager route to {:.1},{:.1} is blocked; returning failure to its AI routine",
                    target.0.x, target.0.z
                );
                commands
                    .entity(entity)
                    .remove::<TravelRoute>()
                    .remove::<NavigationRoutePending>()
                    .insert(NavigationRouteFailed { goal: target.0 });
            }
            continue;
        };

        // The live spatial grid is movement's final authority. Keep this
        // inexpensive last certification even though every surveyed connector
        // already reads the grid: graph composition and endpoint stitching
        // must never be able to hand movement a segment it will immediately
        // reject and re-request forever.
        let certification_started = Instant::now();
        let tagged_points: Vec<_> = tagged.iter().map(|(point, _)| *point).collect();
        let live_route_clear =
            polyline_clear_live_world(&tagged_points, obstacles, colliders, derived);
        telemetry.certification_time += certification_started.elapsed();
        if !live_route_clear {
            let certification_started = Instant::now();
            let direct_clear = polyline_clear_live_world(&direct, obstacles, colliders, derived);
            telemetry.certification_time += certification_started.elapsed();
            if direct_clear && !direct.is_empty() {
                tagged = direct.iter().copied().map(|point| (point, false)).collect();
            } else {
                telemetry.failed_attempts = telemetry.failed_attempts.saturating_add(1);
                pending.attempts = pending.attempts.saturating_add(1);
                pending.obstacle_version = obstacle_version;
                if pending.attempts == 3 {
                    warn!(
                        "Villager route to {:.1},{:.1} failed final live-obstacle certification; returning failure to its AI routine",
                        target.0.x, target.0.z
                    );
                    commands
                        .entity(entity)
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .insert(NavigationRouteFailed { goal: target.0 });
                }
                continue;
            }
        }

        let reverse_is_certified =
            reverse_route_clears_goal_prop_exemption(&tagged, &prop_blockers);
        graph.cache_tactical_route(start.actual, goal.actual, &tagged, reverse_is_certified);
        telemetry.routes_installed = telemetry.routes_installed.saturating_add(1);
        install_tactical_route(
            &mut commands,
            entity,
            target.0,
            goal.actual,
            &terrain,
            &tagged,
        );
    }
    *next_request = (start + visited) % request_count;
    telemetry.finish_invocation(planner_started);
    telemetry.maybe_report(&graph);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::library::{DerivedCollider, StaticColliderInstance};
    use shared::components::{SettlementTier, TimeWarp, WorkStatus};
    use shared::props::PropKind;
    use shared::region::RegionCoord;

    fn one_static_prop(
        kind: PropKind,
        position: Vec3,
        horizontal_radius: f32,
    ) -> (StaticColliders, DerivedColliderLibrary) {
        let cell = (
            (position.x / 16.0).floor() as i32,
            (position.z / 16.0).floor() as i32,
        );
        let mut colliders = StaticColliders::default();
        colliders.instances.insert(
            1,
            StaticColliderInstance {
                kind,
                position,
                rotation: Quat::IDENTITY,
                scale: 1.0,
                cell,
            },
        );
        colliders.cells.insert(cell, vec![1]);
        let derived = DerivedColliderLibrary {
            by_kind: HashMap::from([(
                kind,
                DerivedCollider {
                    bounding_radius: horizontal_radius,
                    horizontal_radius,
                    hulls: Vec::new(),
                },
            )]),
        };
        (colliders, derived)
    }

    #[test]
    fn farmstead_permit_rejects_a_tree_across_its_door_apron() {
        let kind = SettlementBuildingKind::Farmstead;
        let position = Vec3::ZERO;
        let rotation = 0.0;
        let (door, approach) = doorway_approach(kind, position, rotation);
        let tree = door.lerp(approach, 0.55);
        let (colliders, derived) =
            one_static_prop(PropKind::Tree_08, Vec3::new(tree.x, 0.0, tree.y), 0.9);

        assert!(!doorway_road_apron_is_clear_of_props(
            kind, position, rotation, &colliders, &derived,
        ));
        assert!(doorway_road_apron_is_clear_of_props(
            kind,
            position + Vec3::X * 20.0,
            rotation,
            &colliders,
            &derived,
        ));
    }

    #[test]
    fn farm_field_reservation_rejects_props_inside_its_rotated_rows() {
        let rotation = 0.63;
        let center = Vec2::new(20.0, -10.0);
        let local_tree = Vec2::new(3.7, 4.9);
        let tree = center + shared::rotation::local_to_world_xz(local_tree, rotation);
        let (colliders, derived) =
            one_static_prop(PropKind::Tree_09, Vec3::new(tree.x, 0.0, tree.y), 0.7);

        assert!(!rotated_rect_is_clear_of_props(
            center,
            Vec2::new(4.0, 5.5),
            rotation,
            shared::components::FARM_FIELD_EDGE_CLEARANCE,
            &colliders,
            &derived,
        ));
        assert!(rotated_rect_is_clear_of_props(
            center + Vec2::X * 20.0,
            Vec2::new(4.0, 5.5),
            rotation,
            shared::components::FARM_FIELD_EDGE_CLEARANCE,
            &colliders,
            &derived,
        ));
    }

    #[test]
    fn route_certification_uses_the_same_tree_collision_as_movement() {
        let (colliders, derived) = one_static_prop(PropKind::Tree_08, Vec3::ZERO, 1.0);
        let blocked = [Vec2::new(-4.0, 0.0), Vec2::new(4.0, 0.0)];
        let detour = [
            Vec2::new(-4.0, 0.0),
            Vec2::new(-4.0, 4.0),
            Vec2::new(4.0, 4.0),
            Vec2::new(4.0, 0.0),
        ];

        assert!(!polyline_clear_live_world(
            &blocked,
            None,
            Some(&colliders),
            Some(&derived),
        ));
        assert!(polyline_clear_live_world(
            &detour,
            None,
            Some(&colliders),
            Some(&derived),
        ));
    }

    #[test]
    fn failed_road_surveys_use_bounded_real_time_backoff() {
        let first = RoadSurveyBackoff::after_failure(None, 10.0);
        assert_eq!(first.failures, 1);
        assert_eq!(first.retry_after, 10.5);
        assert!(first.should_warn());

        let second = RoadSurveyBackoff::after_failure(Some(first), first.retry_after);
        assert_eq!(second.failures, 2);
        assert_eq!(second.retry_after, 11.5);
        assert!(second.should_warn());

        let mut state = second;
        let mut now = state.retry_after;
        let mut last_delay = 0.0;
        for _ in 0..12 {
            state = RoadSurveyBackoff::after_failure(Some(state), now);
            last_delay = state.retry_after - now;
            assert!(last_delay <= ROAD_SURVEY_RETRY_MAX_SECONDS);
            now = state.retry_after;
        }
        assert_eq!(last_delay, ROAD_SURVEY_RETRY_MAX_SECONDS);
    }
    use std::time::{Duration, Instant};

    /// Diagnostic for the full generated map near a reported live settlement.
    /// Kept ignored because it measures wall time rather than correctness.
    #[test]
    #[ignore = "diagnostic: run explicitly with --ignored --nocapture"]
    fn real_world_village_route_profile() {
        let mut app = App::new();
        app.insert_resource(WorldTerrain::default());
        app.init_resource::<VillageRoadGraph>();
        app.init_resource::<SpatialObstacleGrid>();
        app.insert_resource(PathfindingBudgetSettings {
            max_requests_per_tick: 1,
            ..default()
        });
        app.add_systems(Update, plan_villager_travel_routes);

        let village_centre = Vec2::new(-346.0, 306.0);
        for (ring, count) in [(15.0_f32, 8_usize), (26.0, 12)] {
            for index in 0..count {
                let angle = index as f32 / count as f32 * std::f32::consts::TAU;
                let point = village_centre + Vec2::from_angle(angle) * ring;
                let position = Vec3::new(
                    point.x,
                    app.world()
                        .resource::<WorldTerrain>()
                        .get_height(point.x, point.y),
                    point.y,
                );
                app.world_mut().spawn((
                    PlacedBuilding {
                        building_type: BuildingType::LogCabin,
                        rotation: angle + std::f32::consts::PI,
                    },
                    BuildingPosition(position),
                ));
            }
        }

        let goals = [
            Vec2::new(-371.4, 311.4),
            Vec2::new(-343.1, 301.4),
            Vec2::new(-331.9, 278.2),
            Vec2::new(-343.7, 285.0),
            Vec2::new(-374.1, 310.7),
            Vec2::new(-346.2, 282.1),
            Vec2::new(-356.9, 281.1),
            Vec2::new(-350.2, 337.6),
            Vec2::new(-317.3, 299.5),
            Vec2::new(-374.7, 330.1),
            Vec2::new(-321.6, 302.9),
            Vec2::new(-361.1, 278.7),
        ];
        for (index, goal) in goals.into_iter().enumerate() {
            let start = Vec2::new(-345.0 + (index % 4) as f32 * 2.0, 309.0);
            let start = Vec3::new(
                start.x,
                app.world()
                    .resource::<WorldTerrain>()
                    .get_height(start.x, start.y),
                start.y,
            );
            let goal = Vec3::new(
                goal.x,
                app.world()
                    .resource::<WorldTerrain>()
                    .get_height(goal.x, goal.y),
                goal.y,
            );
            app.world_mut().spawn((
                CharacterKind::Villager,
                PlayerPosition(start),
                MoveTarget(goal),
                NavigationRoutePending::new(goal),
            ));
        }

        for tick in 0..6 {
            let started = Instant::now();
            app.update();
            let elapsed = started.elapsed();
            let pending = app
                .world_mut()
                .query::<&NavigationRoutePending>()
                .iter(app.world())
                .count();
            println!("REAL ROUTES tick={tick} elapsed={elapsed:?} pending={pending}");
        }

        let terrain = app.world().resource::<WorldTerrain>();
        let hall = Vec3::new(
            village_centre.x,
            terrain.get_height(village_centre.x, village_centre.y),
            village_centre.y,
        );
        let mut occupied = vec![(hall, SettlementBuildingKind::Hall.clearance())];
        let mut radial_roads = Vec::new();
        let mut houses = 0usize;
        while let Some((position, rotation)) = crate::world::village::find_site(
            terrain,
            hall,
            SettlementBuildingKind::House,
            &occupied,
            &radial_roads.iter().collect::<Vec<_>>(),
        ) {
            occupied.push((position, SettlementBuildingKind::House.clearance()));
            let door = SettlementBuildingKind::House.entrance_position(position, rotation);
            let hall_door = SettlementBuildingKind::Hall.entrance_position(hall, 0.0);
            radial_roads.push(VillageRoad {
                settlement: "Profile".into(),
                builder: format!("Builder {houses}"),
                points: vec![
                    Vec2::new(door.x, door.z),
                    Vec2::new(hall_door.x, hall_door.z),
                ],
                built_through: 2,
                width: VILLAGE_ROAD_WIDTH,
                reserved_width: RoadClass::Lane.initial_reserved_width(),
                surface: RoadSurface::Dirt,
                class: RoadClass::Lane,
                stone_committed: 0,
            });
            houses += 1;
            if houses >= 40 {
                break;
            }
        }
        println!("REAL SITING houses_with_radial_roads={houses}");
    }

    #[test]
    fn a_failed_route_retries_only_after_its_destination_changes() {
        let mut app = App::new();
        app.add_systems(Update, retry_failed_routes_after_obstacle_change);
        let goal = Vec3::new(20.0, 0.0, 12.0);
        let mover = app
            .world_mut()
            .spawn((MoveTarget(goal), NavigationRouteFailed { goal }))
            .id();

        app.update();
        assert!(app.world().get::<NavigationRouteFailed>(mover).is_some());
        assert!(app.world().get::<NavigationRoutePending>(mover).is_none());

        app.world_mut()
            .entity_mut(mover)
            .insert(MoveTarget(goal + Vec3::X));
        app.update();
        assert!(app.world().get::<NavigationRouteFailed>(mover).is_none());
        assert!(app.world().get::<NavigationRoutePending>(mover).is_some());
    }

    #[test]
    fn an_out_of_bounds_agent_goal_is_cancelled_before_route_survey() {
        let mut app = App::new();
        app.insert_resource(WorldTerrain::default());
        app.init_resource::<VillageRoadGraph>();
        app.insert_resource(PathfindingBudgetSettings {
            max_requests_per_tick: 16,
            ..default()
        });
        app.add_systems(Update, plan_villager_travel_routes);
        let invalid = Vec3::new(1.0e9, 0.0, 1.0e9);
        let mover = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(Vec3::ZERO),
                MoveTarget(invalid),
                NavigationRoutePending::new(invalid),
            ))
            .id();

        app.update();

        let mover = app.world().entity(mover);
        assert!(!mover.contains::<MoveTarget>());
        assert!(!mover.contains::<NavigationRoutePending>());
        assert!(!mover.contains::<TravelRoute>());
    }

    #[test]
    fn survey_wraps_around_a_building_instead_of_crossing_it() {
        let terrain = WorldTerrain::default();
        let mut scratch = SurveyScratch::default();
        let start = Vec2::new(1700.0, 0.0);
        let goal = Vec2::new(1740.0, 0.0);
        let blocker = BuildingBlocker {
            center: Vec2::new(1720.0, 0.0),
            half: Vec2::new(5.0, 7.0),
            rotation: 0.0,
        };
        let path = survey_village_road(
            &terrain,
            start,
            goal,
            &[blocker],
            &PropBlockers::default(),
            7,
            &mut scratch,
        );
        assert!(path.len() > 2, "the path needs a bend: {path:?}");
        assert!(path.iter().all(|point| !blocker.contains(*point)));
        assert!(path.windows(2).all(|pair| {
            let steps = (pair[0].distance(pair[1]) / 0.25).ceil() as usize;
            (0..=steps).all(|step| {
                !blocker.contains(pair[0].lerp(pair[1], step as f32 / steps.max(1) as f32))
            })
        }));
    }

    #[test]
    fn farmer_can_walk_from_front_door_around_farmstead_to_rear_field() {
        let terrain = WorldTerrain::default();
        // Preserve the exact relative geometry from the lab regression while
        // translating it onto the generated world's stable dry test plateau.
        let center = Vec2::new(1700.0, 0.0);
        let start = center + Vec2::new(3.66172, -0.18534);
        let goal = center + Vec2::new(-8.83977, 3.40013);
        let blocker = BuildingBlocker {
            center,
            half: Vec2::new(2.985, 3.59),
            rotation: -1.520343,
        };
        let mut live = SpatialObstacleGrid::new();
        live.insert(shared::spatial::ObstacleEntry {
            center: blocker.center,
            half_extents: blocker.half,
            rotation: blocker.rotation,
            obstacle_type: 0,
        });

        let building = NavigationBuilding {
            blocker,
            kind: SettlementBuildingKind::Farmstead,
            position: Vec3::new(blocker.center.x, 0.0, blocker.center.y),
            rotation: blocker.rotation,
        };
        let start_endpoint = navigation_endpoint(start, &[building]);
        let goal_endpoint = navigation_endpoint(goal, &[building]);
        let mut scratch = SurveyScratch::default();
        let start_apron = survey_agent_route(
            &terrain,
            start_endpoint.actual,
            start_endpoint.survey,
            &[blocker],
            Some(&live),
            &PropBlockers::default(),
            &mut scratch,
            EXTENDED_LOCAL_SURVEY_MAX_NODES,
        );
        let middle = survey_agent_route(
            &terrain,
            start_endpoint.survey,
            goal_endpoint.survey,
            &[blocker],
            Some(&live),
            &PropBlockers::default(),
            &mut scratch,
            EXTENDED_LOCAL_SURVEY_MAX_NODES,
        );
        let goal_apron = survey_agent_route(
            &terrain,
            goal_endpoint.survey,
            goal_endpoint.actual,
            &[blocker],
            Some(&live),
            &PropBlockers::default(),
            &mut scratch,
            EXTENDED_LOCAL_SURVEY_MAX_NODES,
        );
        let route = complete_agent_route(
            start_endpoint,
            goal_endpoint,
            &start_apron,
            middle,
            &goal_apron,
        );
        assert!(
            route.len() >= 2,
            "a farmer released outside the front door must route around the Farmstead to field 2"
        );
        assert!(route
            .windows(2)
            .all(|edge| !live.segment_blocked(edge[0], edge[1])));
    }

    #[test]
    fn surveyed_road_keeps_its_full_ribbon_out_of_a_wheat_field() {
        let terrain = WorldTerrain::default();
        let mut scratch = SurveyScratch::default();
        let start = Vec2::new(1700.0, 0.0);
        let goal = Vec2::new(1740.0, 0.0);
        let field_center = Vec2::new(1720.0, 0.0);
        let field_rotation = 0.37;
        let field_half = SettlementBuildingKind::Farmstead
            .field_half_extents()
            .unwrap();
        let blocker = BuildingBlocker {
            center: field_center,
            half: field_half
                + Vec2::splat(
                    RoadClass::Lane.initial_reserved_width() * 0.5
                        + shared::components::FARM_FIELD_EDGE_CLEARANCE
                        + ROAD_SURVEY_FIELD_EPSILON,
                ),
            rotation: field_rotation,
        };
        let path = survey_village_road(
            &terrain,
            start,
            goal,
            &[blocker],
            &PropBlockers::default(),
            17,
            &mut scratch,
        );
        assert!(path.len() > 2, "the crop needs a visible detour: {path:?}");

        let road = VillageRoad {
            settlement: "Fieldford".into(),
            builder: "Mara".into(),
            built_through: path.len() as u16,
            points: path,
            width: VILLAGE_ROAD_WIDTH,
            reserved_width: RoadClass::Lane.initial_reserved_width(),
            surface: RoadSurface::Dirt,
            class: RoadClass::Lane,
            stone_committed: 0,
        };
        assert!(
            !road.intersects_rotated_rect(
                field_center,
                field_half,
                field_rotation,
                shared::components::FARM_FIELD_EDGE_CLEARANCE,
            ),
            "the surveyed road ribbon clipped the authored 8x11 metre crop: {:?}",
            road.points
        );
    }

    #[test]
    fn a_farmstead_retains_its_road_request_until_both_fields_exist() {
        let mut app = App::new();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(Update, plan_requested_roads);
        let settlement = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Fieldford".into(),
                    tier: SettlementTier::Hamlet,
                    residents: 1,
                    treasury: 0,
                },
                PlayerPosition(Vec3::new(1_700.0, 0.0, 0.0)),
                PlayerRotation(0.0),
            ))
            .id();
        let position = Vec3::new(1_740.0, 0.0, 0.0);
        let builder = app
            .world_mut()
            .spawn((
                CharacterName("Mara".into()),
                CharacterKind::Villager,
                CharacterActivity::Idle,
                PlayerPosition(position),
                VillagerIntent::Building {
                    settlement,
                    site: Entity::PLACEHOLDER,
                },
            ))
            .id();
        let farm = app
            .world_mut()
            .spawn((
                SettlementBuilding {
                    kind: SettlementBuildingKind::Farmstead,
                    settlement: "Fieldford".into(),
                    owner: Some("Mara".into()),
                    quality: 0.8,
                    workers: Vec::new(),
                },
                PlayerPosition(position),
                PlayerRotation(0.0),
                RoadRequest {
                    builder,
                    settlement,
                    completed_site: Entity::PLACEHOLDER,
                    attempt: 0,
                },
            ))
            .id();

        app.update();

        assert!(app.world().get::<RoadRequest>(farm).is_some());
        assert!(app.world().get::<RoadBuilderRoutine>(builder).is_none());
        assert_eq!(
            app.world_mut()
                .query::<&VillageRoad>()
                .iter(app.world())
                .count(),
            0
        );

        let first_field = SettlementBuildingKind::Farmstead
            .field_position_at(position, 0.0, 0)
            .expect("Farmstead has its first field");
        app.world_mut().spawn((
            FarmField {
                settlement: "Fieldford".into(),
                farmstead: position,
                plot_index: 0,
                quality: 0.8,
            },
            PlayerPosition(first_field),
            PlayerRotation(0.0),
        ));
        app.update();

        assert!(
            app.world().get::<RoadRequest>(farm).is_some(),
            "one field is still an incomplete Farmstead layout"
        );
        assert!(app.world().get::<RoadBuilderRoutine>(builder).is_none());
    }

    #[test]
    fn inland_river_above_sea_level_is_not_dry_road_ground() {
        let terrain = WorldTerrain::default();
        let ocean = terrain.water_level().expect("generated world has water");
        let river_point = terrain
            .rivers()
            .iter()
            .flatten()
            .find(|point| {
                terrain
                    .water_surface_height(point.x, point.z)
                    .is_some_and(|surface| {
                        surface > ocean + 0.2 && terrain.get_height(point.x, point.z) < surface
                    })
            })
            .expect("generated world has an inland river");

        assert!(!road_sample_is_dry(
            &terrain,
            Vec2::new(river_point.x, river_point.z),
        ));
    }

    #[test]
    fn agent_survey_uses_the_live_obstacle_grid_as_a_hard_constraint() {
        let terrain = WorldTerrain::default();
        let mut scratch = SurveyScratch::default();
        let mut grid = SpatialObstacleGrid::default();
        grid.insert(shared::spatial::ObstacleEntry {
            center: Vec2::new(1720.0, 0.0),
            half_extents: Vec2::new(2.985, 3.59),
            rotation: -std::f32::consts::FRAC_PI_4,
            obstacle_type: 0,
        });
        let route = survey_agent_route(
            &terrain,
            Vec2::new(1724.5452, -1.7452),
            Vec2::new(1712.5754, 5.3033),
            &[],
            Some(&grid),
            &PropBlockers::default(),
            &mut scratch,
            AGENT_SURVEY_MAX_NODES,
        );

        assert!(!route.is_empty());
        assert!(route.windows(2).all(|segment| {
            let steps = (segment[0].distance(segment[1]) / NAVIGATION_SAMPLE_STEP)
                .ceil()
                .max(1.0) as usize;
            (0..=steps).all(|step| {
                !grid.point_blocked(segment[0].lerp(segment[1], step as f32 / steps as f32))
            })
        }));
    }

    #[test]
    fn graph_reuses_one_cached_route_for_both_directions() {
        let mut graph = VillageRoadGraph {
            nodes: vec![
                RoadGraphNode {
                    point: Vec2::ZERO,
                    edges: vec![(1, 5.0)],
                },
                RoadGraphNode {
                    point: Vec2::X * 5.0,
                    edges: vec![(0, 5.0), (2, 5.0)],
                },
                RoadGraphNode {
                    point: Vec2::X * 10.0,
                    edges: vec![(1, 5.0)],
                },
            ],
            ..default()
        };
        assert_eq!(graph.shortest_path(0, 2), Some(vec![0, 1, 2]));
        assert_eq!(graph.routes.len(), 2);
        assert_eq!(graph.shortest_path(2, 0), Some(vec![2, 1, 0]));
        assert_eq!(graph.routes.len(), 2, "reverse lookup should hit the cache");
    }

    #[test]
    fn tactical_cache_reuses_reverse_commutes_until_geometry_changes() {
        let mut graph = VillageRoadGraph::default();
        graph.sync_tactical_obstacle_version(4);
        let home = Vec2::new(10.0, -3.0);
        let work = Vec2::new(24.0, 8.0);
        let route = vec![(home, false), (Vec2::new(16.0, 1.0), true), (work, false)];

        graph.cache_tactical_route(home, work, &route, true);
        assert_eq!(graph.tactical_route(home, work), Some(route.as_slice()));

        let mut reverse = route.clone();
        reverse.reverse();
        assert_eq!(graph.tactical_route(work, home), Some(reverse.as_slice()));

        graph.sync_tactical_obstacle_version(4);
        assert!(graph.tactical_route(home, work).is_some());
        graph.sync_tactical_obstacle_version(5);
        assert!(
            graph.tactical_route(home, work).is_none(),
            "a changed obstacle grid must invalidate every certified commute"
        );
    }

    #[test]
    fn tactical_cache_does_not_reverse_an_asymmetric_prop_exemption() {
        let home = Vec2::ZERO;
        let work = Vec2::X * 4.0;
        let route = vec![(home, false), (work, false)];
        let mut props = PropBlockers::default();
        props.insert_radius(Vec2::X * 3.0, 0.45);
        assert!(!reverse_route_clears_goal_prop_exemption(&route, &props));

        let mut graph = VillageRoadGraph::default();
        graph.cache_tactical_route(home, work, &route, false);
        assert!(graph.tactical_route(home, work).is_some());
        assert!(
            graph.tactical_route(work, home).is_none(),
            "the reverse trip needs its own survey when the forward goal used a prop exemption"
        );
    }

    #[test]
    fn survey_memoizes_repeated_geometry_checks_within_one_search() {
        let terrain = WorldTerrain::default();
        let mut scratch = SurveyScratch::default();
        let props = PropBlockers::default();
        let survey = RoadSurvey {
            terrain: &terrain,
            buildings: &[],
            live_buildings: None,
            props: &props,
            start: Vec2::new(1700.0, 0.0),
            goal: Vec2::new(1710.0, 0.0),
            min: Vec2::new(1680.0, -20.0),
            max: Vec2::new(1730.0, 20.0),
            max_nodes: AGENT_SURVEY_MAX_NODES,
        };
        scratch.begin_search();
        let first = survey.line_clear(survey.start, survey.goal, &mut scratch);
        let hits_before = scratch.metrics.line_cache_hits;
        let second = survey.line_clear(survey.start, survey.goal, &mut scratch);

        assert_eq!(first, second);
        assert_eq!(scratch.metrics.line_cache_hits, hits_before + 1);
    }

    #[test]
    fn villager_route_uses_the_road_and_never_crosses_a_building() {
        let mut app = App::new();
        app.insert_resource(WorldTerrain::default());
        app.insert_resource(PathfindingBudgetSettings {
            max_requests_per_tick: 8,
            ..default()
        });

        let road_points = [
            Vec2::new(1700.0, 0.0),
            Vec2::new(1710.0, 10.0),
            Vec2::new(1730.0, 10.0),
            Vec2::new(1740.0, 0.0),
        ];
        let mut graph = VillageRoadGraph::default();
        for point in road_points {
            graph.nodes.push(RoadGraphNode {
                point,
                edges: Vec::new(),
            });
        }
        for index in 0..graph.nodes.len() - 1 {
            let distance = graph.nodes[index]
                .point
                .distance(graph.nodes[index + 1].point);
            graph.nodes[index].edges.push((index + 1, distance));
            graph.nodes[index + 1].edges.push((index, distance));
        }
        graph.initialized = true;
        app.insert_resource(graph);
        app.add_systems(
            Update,
            (queue_villager_travel_routes, plan_villager_travel_routes).chain(),
        );

        let building_type = BuildingType::LogCabin;
        let building_position = Vec3::new(1720.0, 0.0, 0.0);
        app.world_mut().spawn((
            PlacedBuilding {
                building_type,
                rotation: 0.0,
            },
            BuildingPosition(building_position),
        ));
        let start = Vec3::new(1700.0, 0.0, 0.0);
        let goal = Vec3::new(1740.0, 0.0, 0.0);
        let villager = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(start),
                MoveTarget(goal),
            ))
            .id();

        app.update();

        let route = app.world().get::<TravelRoute>(villager).unwrap();
        assert!(
            route.waypoints.iter().any(|waypoint| waypoint.on_road),
            "the safe precomputed road should beat a fresh direct detour"
        );
        assert!(!app
            .world()
            .entity(villager)
            .contains::<NavigationRoutePending>());

        let footprint = building_type.definition().footprint;
        let blocker = BuildingBlocker {
            center: Vec2::new(building_position.x, building_position.z),
            half: footprint * 0.5 + Vec2::splat(crate::world::navgrid::VILLAGER_NAV_RADIUS),
            rotation: 0.0,
        };
        let mut points = vec![Vec2::new(start.x, start.z)];
        points.extend(
            route
                .waypoints
                .iter()
                .map(|waypoint| Vec2::new(waypoint.position.x, waypoint.position.z)),
        );
        assert!(points.windows(2).all(|pair| {
            let steps = (pair[0].distance(pair[1]) / 0.2).ceil().max(1.0) as usize;
            (0..=steps)
                .all(|step| !blocker.contains(pair[0].lerp(pair[1], step as f32 / steps as f32)))
        }));
    }

    #[test]
    fn bounded_route_planning_serves_every_pending_villager_fairly() {
        let mut app = App::new();
        app.insert_resource(WorldTerrain::default());
        app.insert_resource(PathfindingBudgetSettings {
            max_requests_per_tick: 1,
            ..default()
        });
        app.init_resource::<VillageRoadGraph>();
        app.add_systems(
            Update,
            (queue_villager_travel_routes, plan_villager_travel_routes).chain(),
        );

        let villagers: Vec<_> = (0..3)
            .map(|index| {
                let start = Vec3::new(1700.0, 0.0, index as f32 * 4.0);
                app.world_mut()
                    .spawn((
                        CharacterKind::Villager,
                        PlayerPosition(start),
                        MoveTarget(start + Vec3::X * 12.0),
                    ))
                    .id()
            })
            .collect();

        for _ in 0..villagers.len() {
            app.update();
        }

        assert!(villagers.iter().all(|entity| {
            app.world().get::<TravelRoute>(*entity).is_some()
                && app.world().get::<NavigationRoutePending>(*entity).is_none()
        }));
    }

    #[test]
    fn the_building_builder_owns_and_finishes_its_road_at_100x() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(
            Update,
            (
                crate::world::village::claim_settlement_hall_obstacles,
                plan_requested_roads,
                build_village_roads,
                crate::player::hero::step_units,
            )
                .chain(),
        );

        let hall_position = Vec3::new(1700.0, 0.0, 0.0);
        let settlement = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Oakmead".into(),
                    tier: SettlementTier::Hamlet,
                    residents: 1,
                    treasury: 0,
                },
                PlayerPosition(hall_position),
                PlayerRotation(0.0),
            ))
            .id();
        app.world_mut().spawn(TimeWarp(100.0));

        let house_position = Vec3::new(1740.0, 0.0, 0.0);
        let house_door = SettlementBuildingKind::House.entrance_position(house_position, 0.0);
        let builder = app
            .world_mut()
            .spawn((
                CharacterName("Mara".into()),
                CharacterKind::Villager,
                CharacterActivity::Idle,
                PlayerPosition(house_door),
                PlayerRotation(0.0),
                RegionCoord::from_world_pos(house_door),
                VillagerIntent::Building {
                    settlement,
                    site: Entity::PLACEHOLDER,
                },
            ))
            .id();
        app.world_mut().spawn((
            SettlementBuilding {
                kind: SettlementBuildingKind::House,
                settlement: "Oakmead".into(),
                owner: Some("Mara".into()),
                quality: 0.7,
                workers: Vec::new(),
            },
            PlayerPosition(house_position),
            PlayerRotation(0.0),
            RoadRequest {
                builder,
                settlement,
                completed_site: Entity::PLACEHOLDER,
                attempt: 0,
            },
        ));
        let field_position = Vec3::new(1720.0, 0.0, -4.5);
        app.world_mut().spawn((
            FarmField {
                settlement: "Oakmead".into(),
                farmstead: Vec3::new(1720.0, 0.0, 4.5),
                plot_index: 0,
                quality: 0.7,
            },
            PlayerPosition(field_position),
            PlayerRotation(0.0),
        ));

        let mut saw_building_animation = false;
        for _ in 0..180 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(Duration::from_secs_f32(
                    1.0 / shared::protocol::FIXED_TIMESTEP_HZ as f32,
                ));
            app.update();
            saw_building_animation |= app
                .world()
                .get::<CharacterActivity>(builder)
                .is_some_and(|activity| *activity == CharacterActivity::Building);
            if app.world().get::<RoadBuilderRoutine>(builder).is_none()
                && app
                    .world()
                    .iter_entities()
                    .any(|entity| entity.contains::<VillageRoad>())
            {
                break;
            }
        }

        let mut roads = app.world_mut().query::<&VillageRoad>();
        let road = roads.single(app.world()).unwrap();
        let hall_door = SettlementBuildingKind::Hall.entrance_position(hall_position, 0.0);
        assert_eq!(road.builder, "Mara");
        assert!(
            road.is_complete(),
            "road stopped at {}/{}",
            road.built_through,
            road.points.len()
        );
        assert!(road.points[0].distance(Vec2::new(house_door.x, house_door.z)) < 0.01);
        assert!(
            road.points
                .last()
                .unwrap()
                .distance(Vec2::new(hall_door.x, hall_door.z))
                < 0.01
        );
        let hall_footprint = SettlementBuildingKind::Hall.art().definition().footprint;
        let hall_blocker = BuildingBlocker {
            center: Vec2::new(hall_position.x, hall_position.z),
            half: hall_footprint * 0.5,
            rotation: 0.0,
        };
        assert!(
            road.points.windows(2).all(|pair| {
                let steps = (pair[0].distance(pair[1]) / 0.2).ceil().max(1.0) as usize;
                (0..=steps).all(|step| {
                    !hall_blocker.contains(pair[0].lerp(pair[1], step as f32 / steps as f32))
                })
            }),
            "the road crossed the hall footprint: {:?}",
            road.points
        );
        assert!(
            !road.intersects_rotated_rect(
                Vec2::new(field_position.x, field_position.z),
                SettlementBuildingKind::Farmstead
                    .field_half_extents()
                    .unwrap(),
                0.0,
                shared::components::FARM_FIELD_EDGE_CLEARANCE,
            ),
            "the road crossed the planted wheat field: {:?}",
            road.points
        );
        assert!(
            saw_building_animation,
            "road work never exposed its build animation"
        );
        assert!(matches!(
            app.world().get::<VillagerIntent>(builder),
            Some(VillagerIntent::Resident { settlement: home }) if *home == settlement
        ));
    }

    #[test]
    fn a_building_beside_the_network_still_gets_its_own_short_road() {
        let mut app = App::new();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(Update, plan_requested_roads);

        let hall_position = Vec3::new(1700.0, 0.0, 0.0);
        let settlement = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Oakmead".into(),
                    tier: SettlementTier::Hamlet,
                    residents: 1,
                    treasury: 0,
                },
                PlayerPosition(hall_position),
                PlayerRotation(0.0),
            ))
            .id();

        let house_position = Vec3::new(1740.0, 0.0, 0.0);
        let house_door = SettlementBuildingKind::House.entrance_position(house_position, 0.0);
        let nearby_network = Vec2::new(house_door.x, house_door.z - 1.6);
        let hall_door3 = SettlementBuildingKind::Hall.entrance_position(hall_position, 0.0);
        let hall_door = Vec2::new(hall_door3.x, hall_door3.z);
        assert!(nearby_network.distance(Vec2::new(house_door.x, house_door.z)) < 2.0);
        app.world_mut().spawn(VillageRoad {
            settlement: "Oakmead".into(),
            builder: "EarlierBuilder".into(),
            points: vec![nearby_network, hall_door],
            built_through: 2,
            width: VILLAGE_ROAD_WIDTH,
            reserved_width: RoadClass::Lane.initial_reserved_width(),
            surface: RoadSurface::Dirt,
            class: RoadClass::Lane,
            stone_committed: 0,
        });

        let builder = app
            .world_mut()
            .spawn((
                CharacterName("NearBuilder".into()),
                CharacterKind::Villager,
                CharacterActivity::Idle,
                PlayerPosition(house_door),
                VillagerIntent::Building {
                    settlement,
                    site: Entity::PLACEHOLDER,
                },
            ))
            .id();
        let house = app
            .world_mut()
            .spawn((
                SettlementBuilding {
                    kind: SettlementBuildingKind::House,
                    settlement: "Oakmead".into(),
                    owner: Some("NearBuilder".into()),
                    quality: 0.7,
                    workers: Vec::new(),
                },
                PlayerPosition(house_position),
                PlayerRotation(0.0),
                PlacedBuilding {
                    building_type: BuildingType::LogCabin,
                    rotation: 0.0,
                },
                BuildingPosition(house_position),
                RoadRequest {
                    builder,
                    settlement,
                    completed_site: Entity::PLACEHOLDER,
                    attempt: 0,
                },
            ))
            .id();

        app.update();

        let connector = app
            .world_mut()
            .query::<&VillageRoad>()
            .iter(app.world())
            .find(|road| road.builder == "NearBuilder")
            .expect("the nearby house must receive its own connector road");
        assert!(connector.points.len() >= 2);
        assert!(connector.points[0].distance(Vec2::new(house_door.x, house_door.z)) < 0.01);
        assert!(connector.points.last().unwrap().distance(nearby_network) < 0.01);
        assert!(app.world().get::<RoadRequest>(house).is_none());
        assert!(app.world().get::<RoadBuilderRoutine>(builder).is_some());
    }

    #[test]
    fn detached_and_unfinished_roads_are_not_public_network_anchors() {
        let hall_door = Vec2::new(1_700.0, -3.0);
        let connected = VillageRoad {
            settlement: "Oakmead".into(),
            builder: "First".into(),
            points: vec![hall_door, hall_door + Vec2::X * 8.0],
            built_through: 2,
            width: VILLAGE_ROAD_WIDTH,
            reserved_width: RoadClass::Lane.initial_reserved_width(),
            surface: RoadSurface::Dirt,
            class: RoadClass::Lane,
            stone_committed: 0,
        };
        let detached = VillageRoad {
            settlement: "Oakmead".into(),
            builder: "Second".into(),
            points: vec![Vec2::new(1_740.0, 0.0), Vec2::new(1_745.0, 0.0)],
            built_through: 2,
            width: VILLAGE_ROAD_WIDTH,
            reserved_width: RoadClass::Lane.initial_reserved_width(),
            surface: RoadSurface::Dirt,
            class: RoadClass::Lane,
            stone_committed: 0,
        };
        let unfinished = VillageRoad {
            settlement: "Oakmead".into(),
            builder: "Third".into(),
            points: vec![Vec2::new(1_730.0, 0.0), hall_door],
            built_through: 1,
            width: VILLAGE_ROAD_WIDTH,
            reserved_width: RoadClass::Lane.initial_reserved_width(),
            surface: RoadSurface::Dirt,
            class: RoadClass::Lane,
            stone_committed: 0,
        };
        let roads = [&connected, &detached, &unfinished];

        let network = hall_road_network("Oakmead", hall_door, &roads);

        assert_eq!(network.disconnected_components, 1);
        assert!(network.connected_keys.contains(&graph_key(hall_door)));
        assert!(network
            .connected_keys
            .contains(&graph_key(hall_door + Vec2::X * 8.0)));
        assert!(!network
            .connected_keys
            .contains(&graph_key(detached.points[0])));
        assert!(!network
            .connected_keys
            .contains(&graph_key(unfinished.points[0])));
    }

    #[test]
    fn road_steward_audits_repairs_and_is_paid_from_the_treasury() {
        let mut app = App::new();
        app.add_systems(
            Update,
            (
                ensure_moot_administrations,
                staff_and_pay_road_stewards,
                audit_village_roads,
            )
                .chain(),
        );
        let clock = app.world_mut().spawn(WorldTime::new_default()).id();
        let hall_position = Vec3::new(1_700.0, 0.0, 0.0);
        let settlement = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Stewardham".into(),
                    tier: SettlementTier::Hamlet,
                    residents: 1,
                    treasury: 250,
                },
                PlayerPosition(hall_position),
                PlayerRotation(0.0),
            ))
            .id();
        let steward = app
            .world_mut()
            .spawn((
                CharacterName("Alda".into()),
                VillagerIntent::Resident { settlement },
                Occupation(None),
                Wallet::new(1_000),
            ))
            .id();
        let house = app
            .world_mut()
            .spawn((
                SettlementBuilding {
                    kind: SettlementBuildingKind::House,
                    settlement: "Stewardham".into(),
                    owner: Some("Alda".into()),
                    quality: 0.7,
                    workers: Vec::new(),
                },
                PlayerPosition(Vec3::new(1_730.0, 0.0, 0.0)),
                PlayerRotation(0.0),
            ))
            .id();

        app.update();

        let administration = app.world().get::<MootAdministration>(settlement).unwrap();
        assert_eq!(administration.road_steward.as_deref(), Some("Alda"));
        assert_eq!(administration.roadless_buildings, 1);
        assert_eq!(administration.disconnected_buildings, 0);
        let request = app.world().get::<RoadRequest>(house).unwrap();
        assert_eq!(request.builder, steward);
        assert_eq!(request.settlement, settlement);
        assert_eq!(
            app.world().get::<Occupation>(steward).unwrap().0.as_deref(),
            Some("Road Steward")
        );

        app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 1;
        app.update();

        assert_eq!(
            app.world().get::<Settlement>(settlement).unwrap().treasury,
            150
        );
        assert_eq!(app.world().get::<Wallet>(steward).unwrap().balance(), 1_100);
        assert_eq!(
            app.world()
                .get::<MootAdministration>(settlement)
                .unwrap()
                .wage_arrears,
            0
        );
    }

    #[test]
    fn road_steward_reclaims_an_abandoned_unfinished_connector() {
        let mut app = App::new();
        app.add_systems(
            Update,
            (
                ensure_moot_administrations,
                staff_and_pay_road_stewards,
                audit_village_roads,
            )
                .chain(),
        );
        app.world_mut().spawn(WorldTime::new_default());
        let hall_position = Vec3::new(1_700.0, 0.0, 0.0);
        let settlement = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Repairwick".into(),
                    tier: SettlementTier::Hamlet,
                    residents: 1,
                    treasury: 250,
                },
                PlayerPosition(hall_position),
                PlayerRotation(0.0),
            ))
            .id();
        let steward = app
            .world_mut()
            .spawn((
                CharacterName("Alda".into()),
                VillagerIntent::Resident { settlement },
                Occupation(None),
                Wallet::new(1_000),
            ))
            .id();
        let house_position = Vec3::new(1_730.0, 0.0, 0.0);
        let house = app
            .world_mut()
            .spawn((
                SettlementBuilding {
                    kind: SettlementBuildingKind::House,
                    settlement: "Repairwick".into(),
                    owner: Some("Alda".into()),
                    quality: 0.7,
                    workers: Vec::new(),
                },
                PlayerPosition(house_position),
                PlayerRotation(0.0),
            ))
            .id();
        let door3 = SettlementBuildingKind::House.entrance_position(house_position, 0.0);
        let door = Vec2::new(door3.x, door3.z);
        let abandoned = app
            .world_mut()
            .spawn(VillageRoad {
                settlement: "Repairwick".into(),
                builder: "Missing Builder".into(),
                points: vec![door, door + Vec2::X * 8.0],
                built_through: 1,
                width: VILLAGE_ROAD_WIDTH,
                reserved_width: RoadClass::Lane.initial_reserved_width(),
                surface: RoadSurface::Dirt,
                class: RoadClass::Lane,
                stone_committed: 0,
            })
            .id();

        app.update();

        assert!(app.world().get_entity(abandoned).is_err());
        let request = app.world().get::<RoadRequest>(house).unwrap();
        assert_eq!(request.builder, steward);
        assert_eq!(request.attempt, 0);
        let administration = app.world().get::<MootAdministration>(settlement).unwrap();
        assert_eq!(administration.roadless_buildings, 1);
        assert_eq!(administration.disconnected_buildings, 0);
        assert_eq!(administration.pending_road_buildings, 0);
    }

    #[test]
    fn road_steward_reclaims_a_live_connector_that_makes_no_daylight_progress() {
        let mut app = App::new();
        app.add_systems(
            Update,
            (
                ensure_moot_administrations,
                staff_and_pay_road_stewards,
                audit_village_roads,
            )
                .chain(),
        );
        let clock = app.world_mut().spawn(WorldTime::new_default()).id();
        let settlement = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Stallford".into(),
                    tier: SettlementTier::Hamlet,
                    residents: 2,
                    treasury: 250,
                },
                PlayerPosition(Vec3::new(1_700.0, 0.0, 0.0)),
                PlayerRotation(0.0),
            ))
            .id();
        let steward = app
            .world_mut()
            .spawn((
                CharacterName("Alda".into()),
                VillagerIntent::Resident { settlement },
                Occupation(None),
                Wallet::new(1_000),
            ))
            .id();
        let house_position = Vec3::new(1_730.0, 0.0, 0.0);
        let house = app
            .world_mut()
            .spawn((
                SettlementBuilding {
                    kind: SettlementBuildingKind::House,
                    settlement: "Stallford".into(),
                    owner: Some("Bryn".into()),
                    quality: 0.7,
                    workers: Vec::new(),
                },
                PlayerPosition(house_position),
                PlayerRotation(0.0),
            ))
            .id();
        let door3 = SettlementBuildingKind::House.entrance_position(house_position, 0.0);
        let door = Vec2::new(door3.x, door3.z);
        let road = app
            .world_mut()
            .spawn(VillageRoad {
                settlement: "Stallford".into(),
                builder: "Bryn".into(),
                points: vec![door, door + Vec2::X * 8.0],
                built_through: 1,
                width: VILLAGE_ROAD_WIDTH,
                reserved_width: RoadClass::Lane.initial_reserved_width(),
                surface: RoadSurface::Dirt,
                class: RoadClass::Lane,
                stone_committed: 0,
            })
            .id();
        let builder = app
            .world_mut()
            .spawn((
                CharacterName("Bryn".into()),
                CharacterKind::Villager,
                PlayerPosition(door3),
                VillagerIntent::RoadBuilding { settlement, road },
                Occupation(None),
                Wallet::new(1_000),
                RoadBuilderRoutine {
                    road,
                    settlement,
                    attempt: 0,
                    phase: RoadBuildPhase::GoingTo { point: 1 },
                },
            ))
            .id();

        app.update();
        assert!(app.world().get::<RoadRequest>(house).is_none());

        app.world_mut()
            .get_mut::<WorldTime>(clock)
            .unwrap()
            .seconds_in_cycle += (ROAD_BUILDER_STALL_SECONDS + ROAD_AUDIT_INTERVAL_SECONDS) as f32;
        app.update();

        assert!(app.world().get_entity(road).is_err());
        assert!(app.world().get::<RoadBuilderRoutine>(builder).is_none());
        assert!(matches!(
            app.world().get::<VillagerIntent>(builder),
            Some(VillagerIntent::Resident { settlement: owner }) if *owner == settlement
        ));
        let request = app.world().get::<RoadRequest>(house).unwrap();
        assert_eq!(request.builder, steward);
        let administration = app.world().get::<MootAdministration>(settlement).unwrap();
        assert_eq!(administration.roadless_buildings, 1);
        assert_eq!(administration.pending_road_buildings, 0);
    }

    #[test]
    fn active_road_builder_is_not_hired_for_a_production_job() {
        let mut app = App::new();
        app.add_systems(Update, crate::world::village::fill_vacancies);
        let settlement = app
            .world_mut()
            .spawn(Settlement {
                name: "OneJob".into(),
                tier: SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            })
            .id();
        let road = app.world_mut().spawn_empty().id();
        let worker = app
            .world_mut()
            .spawn((
                CharacterName("Bryn".into()),
                VillagerIntent::RoadBuilding { settlement, road },
                PlayerPosition(Vec3::ZERO),
                Occupation(None),
                WorkStatus::LookingForWork,
                RoadBuilderRoutine {
                    road,
                    settlement,
                    attempt: 0,
                    phase: RoadBuildPhase::GoingTo { point: 0 },
                },
            ))
            .id();
        let farm = app
            .world_mut()
            .spawn((
                SettlementBuilding {
                    kind: SettlementBuildingKind::Farmstead,
                    settlement: "OneJob".into(),
                    owner: Some("Someone Else".into()),
                    quality: 1.0,
                    workers: Vec::new(),
                },
                PlayerPosition(Vec3::X * 10.0),
            ))
            .id();

        app.update();

        assert!(app
            .world()
            .get::<SettlementBuilding>(farm)
            .unwrap()
            .workers
            .is_empty());
        assert_eq!(
            *app.world().get::<WorkStatus>(worker).unwrap(),
            WorkStatus::LookingForWork
        );
        assert!(app.world().get::<Occupation>(worker).unwrap().0.is_none());
    }

    #[test]
    fn road_steward_cannot_also_be_hired_as_a_farmer() {
        let mut app = App::new();
        app.add_systems(Update, crate::world::village::fill_vacancies);
        let settlement = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Singletrade".into(),
                    tier: SettlementTier::Hamlet,
                    residents: 1,
                    treasury: 0,
                },
                MootAdministration {
                    road_steward: Some("Alda".into()),
                    ..default()
                },
            ))
            .id();
        let steward = app
            .world_mut()
            .spawn((
                CharacterName("Alda".into()),
                VillagerIntent::Resident { settlement },
                PlayerPosition(Vec3::ZERO),
                Occupation(Some("Road Steward".into())),
                WorkStatus::Employed,
                RoadSteward { settlement },
            ))
            .id();
        let farm = app
            .world_mut()
            .spawn((
                SettlementBuilding {
                    kind: SettlementBuildingKind::Farmstead,
                    settlement: "Singletrade".into(),
                    owner: Some("Someone Else".into()),
                    quality: 1.0,
                    // Reproduce a save written by the old recruiter, which
                    // could leave the Road Steward in a farm roster too.
                    workers: vec!["Alda".into()],
                },
                PlayerPosition(Vec3::X * 10.0),
            ))
            .id();

        app.update();

        assert!(app
            .world()
            .get::<SettlementBuilding>(farm)
            .unwrap()
            .workers
            .is_empty());
        assert_eq!(
            app.world().get::<Occupation>(steward).unwrap().0.as_deref(),
            Some("Road Steward")
        );
        assert_eq!(
            *app.world().get::<WorkStatus>(steward).unwrap(),
            WorkStatus::Employed
        );
    }

    #[test]
    fn road_steward_reclaims_a_stale_request_from_a_builder_with_a_new_permit() {
        let mut app = App::new();
        app.add_systems(
            Update,
            (
                ensure_moot_administrations,
                staff_and_pay_road_stewards,
                audit_village_roads,
            )
                .chain(),
        );
        app.world_mut().spawn(WorldTime::new_default());
        let settlement = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Permitford".into(),
                    tier: SettlementTier::Hamlet,
                    residents: 2,
                    treasury: 250,
                },
                PlayerPosition(Vec3::new(1_700.0, 0.0, 0.0)),
                PlayerRotation(0.0),
            ))
            .id();
        let steward = app
            .world_mut()
            .spawn((
                CharacterName("Alda".into()),
                VillagerIntent::Resident { settlement },
                Occupation(None),
                Wallet::new(1_000),
            ))
            .id();
        let newer_site = app.world_mut().spawn_empty().id();
        let original_builder = app
            .world_mut()
            .spawn((
                CharacterName("Bryn".into()),
                VillagerIntent::Building {
                    settlement,
                    site: newer_site,
                },
                Occupation(None),
                Wallet::new(1_000),
            ))
            .id();
        let house = app
            .world_mut()
            .spawn((
                SettlementBuilding {
                    kind: SettlementBuildingKind::House,
                    settlement: "Permitford".into(),
                    owner: Some("Bryn".into()),
                    quality: 0.7,
                    workers: Vec::new(),
                },
                PlayerPosition(Vec3::new(1_730.0, 0.0, 0.0)),
                PlayerRotation(0.0),
            ))
            .id();
        app.world_mut().entity_mut(house).insert(RoadRequest {
            builder: original_builder,
            settlement,
            completed_site: house,
            attempt: 0,
        });

        app.update();

        let request = app.world().get::<RoadRequest>(house).unwrap();
        assert_eq!(request.builder, steward);
        let administration = app.world().get::<MootAdministration>(settlement).unwrap();
        assert_eq!(administration.roadless_buildings, 1);
        assert_eq!(administration.pending_road_buildings, 0);
    }

    #[test]
    fn road_builder_skips_a_failed_already_built_door_anchor() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(Update, build_village_roads);
        app.world_mut()
            .spawn(shared::components::TimeWarp::clamped(100.0));
        let settlement = app.world_mut().spawn_empty().id();
        let terrain = app.world().resource::<WorldTerrain>();
        let first = Vec2::new(1_700.0, 0.0);
        let second = first + Vec2::X * 5.0;
        let first3 = Vec3::new(first.x, terrain.get_height(first.x, first.y), first.y);
        let road = app
            .world_mut()
            .spawn(VillageRoad {
                settlement: "Anchorham".into(),
                builder: "Aud".into(),
                points: vec![first, second],
                built_through: 1,
                width: VILLAGE_ROAD_WIDTH,
                reserved_width: RoadClass::Lane.initial_reserved_width(),
                surface: RoadSurface::Dirt,
                class: RoadClass::Lane,
                stone_committed: 0,
            })
            .id();
        let builder = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(first3 + Vec3::X * 12.0),
                PlayerRotation(0.0),
                CharacterActivity::Idle,
                VillagerIntent::RoadBuilding { settlement, road },
                MoveTarget(first3),
                NavigationRouteFailed { goal: first3 },
                RoadBuilderRoutine {
                    road,
                    settlement,
                    attempt: 0,
                    phase: RoadBuildPhase::GoingTo { point: 0 },
                },
            ))
            .id();

        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs_f32(1.0 / 60.0));
        app.update();

        let routine = app.world().get::<RoadBuilderRoutine>(builder).unwrap();
        assert!(matches!(
            routine.phase,
            RoadBuildPhase::GoingTo { point: 1 }
        ));
        assert!(app.world().get::<NavigationRouteFailed>(builder).is_none());
        let target = app.world().get::<MoveTarget>(builder).unwrap().0;
        assert!(Vec2::new(target.x, target.z).distance(second) < 0.01);
        assert!(app.world().get_entity(road).is_ok());
    }

    #[test]
    fn road_builder_discards_a_failed_route_from_an_older_destination() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(Update, build_village_roads);
        let settlement = app.world_mut().spawn_empty().id();
        let terrain = app.world().resource::<WorldTerrain>();
        let first = Vec2::new(1_700.0, 0.0);
        let second = first + Vec2::X * 5.0;
        let first3 = Vec3::new(first.x, terrain.get_height(first.x, first.y), first.y);
        let second3 = Vec3::new(second.x, terrain.get_height(second.x, second.y), second.y);
        let road = app
            .world_mut()
            .spawn(VillageRoad {
                settlement: "Freshroad".into(),
                builder: "Bryn".into(),
                points: vec![first, second],
                built_through: 1,
                width: VILLAGE_ROAD_WIDTH,
                reserved_width: RoadClass::Lane.initial_reserved_width(),
                surface: RoadSurface::Dirt,
                class: RoadClass::Lane,
                stone_committed: 0,
            })
            .id();
        let builder = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(first3),
                PlayerRotation(0.0),
                CharacterActivity::Idle,
                VillagerIntent::RoadBuilding { settlement, road },
                MoveTarget(second3),
                NavigationRouteFailed {
                    goal: first3 + Vec3::Z * 20.0,
                },
                RoadBuilderRoutine {
                    road,
                    settlement,
                    attempt: 0,
                    phase: RoadBuildPhase::GoingTo { point: 1 },
                },
            ))
            .id();

        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs_f32(1.0 / 60.0));
        app.update();

        assert!(app.world().get::<NavigationRouteFailed>(builder).is_none());
        assert_eq!(app.world().get::<MoveTarget>(builder).unwrap().0, second3);
        assert!(app.world().get::<RoadBuilderRoutine>(builder).is_some());
        assert!(app.world().get_entity(road).is_ok());
    }

    #[test]
    fn a_retained_road_request_cannot_steal_a_builder_from_a_new_site() {
        let mut app = App::new();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(Update, plan_requested_roads);

        let settlement = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Twojobs".into(),
                    tier: SettlementTier::Hamlet,
                    residents: 1,
                    treasury: 0,
                },
                PlayerPosition(Vec3::new(1700.0, 0.0, 0.0)),
                PlayerRotation(0.0),
            ))
            .id();
        let completed_site = app.world_mut().spawn_empty().id();
        let newer_site = app.world_mut().spawn_empty().id();
        let builder = app
            .world_mut()
            .spawn((
                CharacterName("Mara".into()),
                CharacterKind::Villager,
                CharacterActivity::Idle,
                PlayerPosition(Vec3::new(1740.0, 0.0, 0.0)),
                VillagerIntent::Building {
                    settlement,
                    site: newer_site,
                },
            ))
            .id();
        let building = app
            .world_mut()
            .spawn((
                SettlementBuilding {
                    kind: SettlementBuildingKind::House,
                    settlement: "Twojobs".into(),
                    owner: Some("Mara".into()),
                    quality: 0.5,
                    workers: Vec::new(),
                },
                PlayerPosition(Vec3::new(1740.0, 0.0, 0.0)),
                PlayerRotation(0.0),
                RoadRequest {
                    builder,
                    settlement,
                    completed_site,
                    attempt: 0,
                },
            ))
            .id();

        app.update();

        assert!(matches!(
            app.world().get::<VillagerIntent>(builder),
            Some(VillagerIntent::Building { settlement: home, site })
                if *home == settlement && *site == newer_site
        ));
        assert!(app.world().get::<RoadBuilderRoutine>(builder).is_none());
        assert!(
            app.world().get::<RoadRequest>(building).is_some(),
            "the old road should wait until its builder is free"
        );
    }
}
