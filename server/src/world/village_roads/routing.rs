//! Built-road graph maintenance, bounded tactical route planning and caching.

use super::*;

#[derive(Default)]
pub(super) struct RoadGraphNode {
    pub(super) point: Vec2,
    pub(super) edges: Vec<(usize, f32)>,
}

#[derive(Resource, Default)]
pub struct VillageRoadGraph {
    pub(super) nodes: Vec<RoadGraphNode>,
    /// Connected-component label for each node, rebuilt with the graph.
    /// This lets route selection discard dead local spurs before launching
    /// Dijkstra and makes it safe to inspect a wider geometric candidate pool.
    components: Vec<usize>,
    /// Current collision can make a durable road edge unusable. This shared
    /// set is rebuilt once per geometry revision rather than during every
    /// villager's Dijkstra search.
    blocked_edges: HashSet<((i32, i32), (i32, i32))>,
    checked_edges: HashSet<((i32, i32), (i32, i32))>,
    walkability_road_revision: u64,
    walkability_obstacle_version: u64,
    pub(super) routes: HashMap<(usize, usize), Vec<usize>>,
    pub(super) tactical_routes: HashMap<TacticalRouteKey, Vec<(Vec2, bool)>>,
    pub(super) tactical_order: VecDeque<TacticalRouteKey>,
    /// A few certified approaches per exact destination. Immigrant cohorts
    /// usually share one hall door but begin at slightly different points;
    /// joining a nearby certified approach turns hundreds of full A* jobs
    /// into one route plus cheap local connectors.
    cohort_routes: HashMap<TacticalRoutePoint, VecDeque<CohortRoute>>,
    /// Monotonic, destination-specific proof that a new certified approach
    /// has become available. Migration cooldowns remember this value when a
    /// route fails, so a later successful immigrant can wake the stranded
    /// cohort immediately instead of making them wait through an exponential
    /// real-time cooldown despite the world now containing a usable route.
    cohort_opportunity_revisions: HashMap<TacticalRoutePoint, u64>,
    cohort_opportunity_order: VecDeque<TacticalRoutePoint>,
    next_cohort_opportunity_revision: u64,
    pub(super) road_revision: u64,
    pub(super) built_point_keys: HashSet<(i32, i32)>,
    pub(super) road_opportunity_revisions: HashMap<(i32, i32), u64>,
    pub(super) next_road_opportunity_revision: u64,
    pub(super) initialized: bool,
}

const MAX_TACTICAL_ROUTE_CACHE_ENTRIES: usize = 8_192;
const MAX_COHORT_OPPORTUNITY_GOALS: usize = MAX_TACTICAL_ROUTE_CACHE_ENTRIES;
const MAX_COHORT_ROUTES_PER_GOAL: usize = 8;
const COHORT_ROUTE_JOIN_DISTANCE: f32 = 64.0;
const ROAD_OPPORTUNITY_CELL: f32 = ROAD_ROUTE_JOIN_DISTANCE;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct TacticalRoutePoint(u32, u32);

impl From<Vec2> for TacticalRoutePoint {
    fn from(point: Vec2) -> Self {
        // Exact endpoints preserve the planner's collision proof. Work and
        // household destinations are stable authored points, so repeated
        // commutes still hit without joining an approximate cached start.
        Self(point.x.to_bits(), point.y.to_bits())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct TacticalRouteKey {
    start: TacticalRoutePoint,
    goal: TacticalRoutePoint,
}

#[derive(Clone, Debug)]
struct CohortRoute {
    start: Vec2,
    tagged: Vec<(Vec2, bool)>,
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

pub(super) fn graph_key(point: Vec2) -> (i32, i32) {
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
    let previous_points = std::mem::take(&mut graph.built_point_keys);
    graph.nodes.clear();
    graph.routes.clear();
    graph.road_revision = graph.road_revision.wrapping_add(1);
    let mut by_point = HashMap::<(i32, i32), usize>::new();
    let mut current_points = HashSet::new();
    let mut new_points = Vec::new();
    for road in roads.iter() {
        let mut previous: Option<usize> = None;
        for point in road.built_points() {
            let key = graph_key(*point);
            current_points.insert(key);
            if !previous_points.contains(&key) {
                new_points.push(*point);
            }
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
    graph.built_point_keys = current_points;
    for point in new_points {
        graph.note_road_opportunity(point);
    }
    graph.rebuild_components();
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
    fn rebuild_components(&mut self) {
        self.components.clear();
        self.components.resize(self.nodes.len(), usize::MAX);
        let mut frontier = Vec::new();
        for root in 0..self.nodes.len() {
            if self.components[root] != usize::MAX {
                continue;
            }
            self.components[root] = root;
            frontier.push(root);
            while let Some(node) = frontier.pop() {
                for &(next, _) in &self.nodes[node].edges {
                    if !self.edge_is_blocked(node, next) && self.components[next] == usize::MAX {
                        self.components[next] = root;
                        frontier.push(next);
                    }
                }
            }
        }
    }

    fn same_component(&self, start: usize, goal: usize) -> bool {
        // Hand-built unit-test graphs predate component labels. They remain
        // valid inputs to shortest_path; runtime graphs are always labelled by
        // rebuild_village_road_graph.
        self.components
            .get(start)
            .zip(self.components.get(goal))
            .is_none_or(|(start, goal)| start == goal)
    }

    fn component_of(&self, node: usize) -> usize {
        self.components.get(node).copied().unwrap_or(0)
    }

    fn edge_key(&self, a: usize, b: usize) -> ((i32, i32), (i32, i32)) {
        let a = graph_key(self.nodes[a].point);
        let b = graph_key(self.nodes[b].point);
        if a < b {
            (a, b)
        } else {
            (b, a)
        }
    }

    fn edge_is_blocked(&self, a: usize, b: usize) -> bool {
        self.blocked_edges.contains(&self.edge_key(a, b))
    }

    fn refresh_walkable_edges(
        &mut self,
        obstacle_version: u64,
        buildings: Option<&SpatialObstacleGrid>,
        colliders: Option<&StaticColliders>,
        derived: Option<&DerivedColliderLibrary>,
    ) {
        if self.walkability_road_revision == self.road_revision
            && self.walkability_obstacle_version == obstacle_version
        {
            return;
        }
        if self.walkability_obstacle_version != obstacle_version {
            self.blocked_edges.clear();
            self.checked_edges.clear();
        }
        for (node, road_node) in self.nodes.iter().enumerate() {
            for &(next, _) in &road_node.edges {
                if node >= next {
                    continue;
                }
                let edge = self.edge_key(node, next);
                if !self.checked_edges.insert(edge) {
                    continue;
                }
                if !crate::player::hero::navigation_segment_clear(
                    road_node.point,
                    self.nodes[next].point,
                    buildings,
                    colliders,
                    derived,
                ) {
                    self.blocked_edges.insert(edge);
                }
            }
        }
        self.routes.clear();
        self.rebuild_components();
        self.walkability_road_revision = self.road_revision;
        self.walkability_obstacle_version = obstacle_version;
    }

    fn clear_tactical_routes(&mut self) {
        self.tactical_routes.clear();
        self.tactical_order.clear();
        self.cohort_routes.clear();
    }

    /// Embodied movement discovered that at least one cached geometry proof
    /// is older than the collision world it is walking through. This should be
    /// rare (normally targeted revision invalidation catches the change), so
    /// conservatively discard the shared tactical cache and make any critical
    /// same-goal retry survey current geometry once.
    pub(crate) fn invalidate_tactical_routes_after_embodied_rejection(&mut self) {
        self.clear_tactical_routes();
    }

    fn rebuild_cohort_routes(&mut self) {
        self.cohort_routes.clear();
        let retained: Vec<_> = self.tactical_routes.values().cloned().collect();
        for route in retained {
            let (Some((start, _)), Some((goal, _))) = (route.first(), route.last()) else {
                continue;
            };
            self.cache_cohort_route(*goal, *start, &route);
        }
    }

    pub(super) fn invalidate_tactical_routes_near_buildings(
        &mut self,
        changed: &[BuildingBlocker],
    ) {
        if changed.is_empty() || self.tactical_routes.is_empty() {
            return;
        }
        self.tactical_routes.retain(|_, route| {
            !changed.iter().any(|blocker| {
                route.iter().any(|(point, _)| blocker.contains(*point))
                    || route
                        .windows(2)
                        .any(|pair| blocker.blocks_segment(pair[0].0, pair[1].0))
            })
        });
        self.tactical_order
            .retain(|key| self.tactical_routes.contains_key(key));
        self.rebuild_cohort_routes();
    }

    fn invalidate_tactical_routes_in_chunks(&mut self, changed: &HashSet<ChunkCoord>) {
        if changed.is_empty() || self.tactical_routes.is_empty() {
            return;
        }
        self.tactical_routes.retain(|_, route| {
            !route.windows(2).any(|pair| {
                let start = pair[0].0;
                let end = pair[1].0;
                let min_x = (start.x.min(end.x) / CHUNK_SIZE).floor() as i32;
                let max_x = (start.x.max(end.x) / CHUNK_SIZE).floor() as i32;
                let min_z = (start.y.min(end.y) / CHUNK_SIZE).floor() as i32;
                let max_z = (start.y.max(end.y) / CHUNK_SIZE).floor() as i32;
                (min_x..=max_x)
                    .any(|x| (min_z..=max_z).any(|z| changed.contains(&ChunkCoord::new(x, z))))
            })
        });
        self.tactical_order
            .retain(|key| self.tactical_routes.contains_key(key));
        self.rebuild_cohort_routes();
    }

    fn road_opportunity_cell(point: Vec2) -> (i32, i32) {
        (
            (point.x / ROAD_OPPORTUNITY_CELL).floor() as i32,
            (point.y / ROAD_OPPORTUNITY_CELL).floor() as i32,
        )
    }

    pub(super) fn note_road_opportunity(&mut self, point: Vec2) {
        self.next_road_opportunity_revision =
            self.next_road_opportunity_revision.wrapping_add(1).max(1);
        self.road_opportunity_revisions.insert(
            Self::road_opportunity_cell(point),
            self.next_road_opportunity_revision,
        );
    }

    fn nearby_road_opportunity_revision(&self, point: Vec2) -> u64 {
        let cell = Self::road_opportunity_cell(point);
        let mut revision = 0;
        for dx in -1..=1 {
            for dz in -1..=1 {
                revision = revision.max(
                    self.road_opportunity_revisions
                        .get(&(cell.0 + dx, cell.1 + dz))
                        .copied()
                        .unwrap_or(0),
                );
            }
        }
        revision
    }

    pub(super) fn route_opportunity_version(&self, start: Vec2, goal: Vec2) -> u64 {
        self.nearby_road_opportunity_revision(start)
            .max(self.nearby_road_opportunity_revision(goal))
    }

    /// Version of the latest newly-certified embodied approach to an exact
    /// destination. Unlike road opportunity revisions this also advances when
    /// a nearby migrant proves a useful off-road connector through otherwise
    /// unchanged geometry.
    pub(crate) fn cohort_route_opportunity_version(&self, goal: Vec2) -> u64 {
        let goal = TacticalRoutePoint::from(goal);
        self.cohort_opportunity_revisions
            .get(&goal)
            .copied()
            .unwrap_or(0)
    }

    fn note_cohort_route_opportunity(&mut self, goal: Vec2) {
        self.next_cohort_opportunity_revision =
            self.next_cohort_opportunity_revision.wrapping_add(1).max(1);
        let goal = TacticalRoutePoint::from(goal);
        if self
            .cohort_opportunity_revisions
            .insert(goal, self.next_cohort_opportunity_revision)
            .is_none()
        {
            self.cohort_opportunity_order.push_back(goal);
        }
        while self.cohort_opportunity_revisions.len() > MAX_COHORT_OPPORTUNITY_GOALS {
            let Some(oldest) = self.cohort_opportunity_order.pop_front() else {
                break;
            };
            self.cohort_opportunity_revisions.remove(&oldest);
        }
    }

    pub(super) fn tactical_route(&self, start: Vec2, goal: Vec2) -> Option<&[(Vec2, bool)]> {
        self.tactical_routes
            .get(&TacticalRouteKey::new(start, goal))
            .map(Vec::as_slice)
    }

    pub(crate) fn cache_tactical_route(
        &mut self,
        start: Vec2,
        goal: Vec2,
        route: &[(Vec2, bool)],
        reverse_is_certified: bool,
    ) {
        let key = TacticalRouteKey::new(start, goal);
        if self.cache_tactical_route_one(key, route.to_vec()) {
            self.note_cohort_route_opportunity(goal);
        }
        self.cache_cohort_route(goal, start, route);

        if reverse_is_certified {
            let mut reverse = route.to_vec();
            reverse.reverse();
            self.cache_cohort_route(start, goal, &reverse);
            if self.cache_tactical_route_one(key.reversed(), reverse) {
                self.note_cohort_route_opportunity(start);
            }
        }
    }

    fn cache_cohort_route(&mut self, goal: Vec2, start: Vec2, route: &[(Vec2, bool)]) {
        let routes = self.cohort_routes.entry(goal.into()).or_default();
        if let Some(existing) = routes
            .iter_mut()
            .find(|existing| existing.start.distance_squared(start) <= 0.01)
        {
            existing.tagged = route.to_vec();
            return;
        }
        routes.push_back(CohortRoute {
            start,
            tagged: route.to_vec(),
        });
        while routes.len() > MAX_COHORT_ROUTES_PER_GOAL {
            routes.pop_front();
        }
    }

    fn cohort_route_candidates(&self, start: Vec2, goal: Vec2) -> Vec<CohortRoute> {
        let goal_key = TacticalRoutePoint::from(goal);
        let mut candidates: Vec<_> = self
            .cohort_routes
            .get(&goal_key)
            .into_iter()
            .flatten()
            .filter(|route| route.start.distance(start) <= COHORT_ROUTE_JOIN_DISTANCE)
            .cloned()
            .collect();
        candidates.sort_by(|a, b| {
            a.start
                .distance_squared(start)
                .total_cmp(&b.start.distance_squared(start))
        });
        candidates
    }

    fn cache_tactical_route_one(
        &mut self,
        key: TacticalRouteKey,
        route: Vec<(Vec2, bool)>,
    ) -> bool {
        let inserted = self.tactical_routes.insert(key, route).is_none();
        if inserted {
            self.tactical_order.push_back(key);
        }
        while self.tactical_routes.len() > MAX_TACTICAL_ROUTE_CACHE_ENTRIES {
            let Some(oldest) = self.tactical_order.pop_front() else {
                break;
            };
            self.tactical_routes.remove(&oldest);
        }
        inserted
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

    pub(super) fn shortest_path(&mut self, start: usize, goal: usize) -> Option<Vec<usize>> {
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
                if self.edge_is_blocked(node, next) {
                    continue;
                }
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

/// Record changed villager destinations before movement, and recover any
/// authoritative target whose route state was orphaned during a handoff.
/// Route work is split into a bounded second system, so a crowd receiving jobs
/// on one tick cannot create an unbounded A* spike.
pub fn queue_villager_travel_routes(
    simulation_time: crate::world::simulation_time::SimulationTime,
    mut commands: Commands,
    movers: Query<
        (
            Entity,
            &CharacterKind,
            &MoveTarget,
            Option<&NavigationRouteFailed>,
            Option<&NavigationRouteBackoff>,
        ),
        (
            Or<(
                Changed<MoveTarget>,
                (
                    Without<TravelRoute>,
                    Without<NavigationRoutePending>,
                    Without<NavigationRouteFailed>,
                    Without<NavigationRouteBackoff>,
                ),
            )>,
            Without<BuildingDoorUse>,
            Without<PierTraversal>,
            // Only an active forecourt step owns movement. A stale transit
            // marker must not strand someone after their permit/meal/arrival
            // ticket has been consumed and another routine takes over.
            Or<(
                Without<crate::world::village::MootQueueTransit>,
                Without<crate::world::village::MootQueueTicket>,
            )>,
        ),
    >,
) {
    let now = simulation_time.elapsed_real_seconds_f64();
    for (entity, kind, target, failed, backoff) in movers.iter() {
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
        let destination_changed = backoff.is_some_and(|backoff| !backoff.matches(target.0));
        if let Some(backoff) = backoff.copied() {
            if !destination_changed && now < backoff.retry_after {
                continue;
            }
        }
        let mut entity_commands = commands.entity(entity);
        entity_commands
            .remove::<TravelRoute>()
            .remove::<NavigationRouteFailed>()
            .insert(NavigationRoutePending::new(target.0));
        if destination_changed {
            entity_commands.remove::<NavigationRouteBackoff>();
        }
    }
}

/// A static failure sleeps until the AI chooses a genuinely new destination.
///
/// A global obstacle version is intentionally not a retry trigger: completing
/// any house changes that version, even if the failed point is on the opposite
/// side of the village. At high time warp that used to wake every blocked
/// villager repeatedly while a settlement was expanding.
pub fn retry_failed_routes_after_obstacle_change(
    simulation_time: crate::world::simulation_time::SimulationTime,
    mut commands: Commands,
    graph: Res<VillageRoadGraph>,
    failed: Query<
        (
            Entity,
            &PlayerPosition,
            &MoveTarget,
            Option<&NavigationRouteFailed>,
            Option<&NavigationRouteBackoff>,
            Has<NavigationRoutePending>,
            Has<TravelRoute>,
        ),
        Or<(With<NavigationRouteFailed>, With<NavigationRouteBackoff>)>,
    >,
) {
    let now = simulation_time.elapsed_real_seconds_f64();
    for (entity, position, target, failed, backoff, pending, travelling) in failed.iter() {
        let destination_changed = failed
            .is_some_and(|failed| failed.goal.distance_squared(target.0) > 0.01)
            || backoff.is_some_and(|backoff| !backoff.matches(target.0));
        if destination_changed {
            commands
                .entity(entity)
                .remove::<NavigationRouteFailed>()
                .remove::<NavigationRouteBackoff>()
                .insert(NavigationRoutePending::new(target.0));
            continue;
        }
        let relevant_road_changed = backoff.is_some_and(|backoff| {
            graph.route_opportunity_version(
                Vec2::new(position.0.x, position.0.z),
                Vec2::new(target.0.x, target.0.z),
            ) > backoff.road_opportunity_version
        });
        let retry_due = backoff.is_some_and(|backoff| now >= backoff.retry_after);
        if !pending && !travelling && (retry_due || relevant_road_changed) {
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

struct IncrementalRouteJob {
    target: Vec3,
    start: NavigationEndpoint,
    goal: NavigationEndpoint,
    start_apron: Vec<Vec2>,
    goal_apron: Vec<Vec2>,
    props: PropBlockers,
    scratch: SurveyScratch,
    search: SurveySearchState,
    geometry_version: u64,
    road_opportunity_version: u64,
}

#[derive(Default)]
pub(crate) struct RouteRequestState {
    next_request_by_priority: [usize; ROUTE_PRIORITY_COUNT],
    incremental_jobs: HashMap<Entity, IncrementalRouteJob>,
    collision_snapshot: RouteCollisionSnapshot,
    warning_limiter: RouteWarningLimiter,
}

#[derive(Default)]
struct RouteCollisionSnapshot {
    initialized: bool,
    collider_version: u64,
    chunk_versions: std::collections::HashMap<ChunkCoord, u64>,
}

#[derive(Default)]
struct RouteWarningLimiter {
    last_by_goal: HashMap<(i32, i32), f64>,
}

const ROUTE_WARNING_GOAL_CELL: f32 = 2.0;
const ROUTE_WARNING_INTERVAL_SECONDS: f64 = 5.0;
const MAX_ROUTE_WARNING_GOALS: usize = 4_096;

impl RouteWarningLimiter {
    fn allow(&mut self, target: Vec3, now: f64) -> bool {
        if self.last_by_goal.len() >= MAX_ROUTE_WARNING_GOALS {
            self.last_by_goal
                .retain(|_, last| now - *last < ROUTE_WARNING_INTERVAL_SECONDS);
        }
        let key = (
            (target.x / ROUTE_WARNING_GOAL_CELL).round() as i32,
            (target.z / ROUTE_WARNING_GOAL_CELL).round() as i32,
        );
        if self
            .last_by_goal
            .get(&key)
            .is_some_and(|last| now - *last < ROUTE_WARNING_INTERVAL_SECONDS)
        {
            return false;
        }
        self.last_by_goal.insert(key, now);
        true
    }
}

const ROUTE_PRIORITY_COUNT: usize = 2;
const ROUTE_PRIORITY_COMMITTED: usize = 0;
const ROUTE_PRIORITY_AMBIENT: usize = 1;

/// Embodied work, construction, migration, shopping and home journeys must
/// not queue behind cosmetic roadside wandering during a population burst.
/// AmbientRoutine is the authoritative marker for the latter; every other
/// destination was selected by a committed simulation state machine.
fn route_request_priority(intent: Option<&VillagerIntent>, ambient: bool) -> usize {
    if ambient
        && !matches!(
            intent,
            Some(VillagerIntent::Building { .. } | VillagerIntent::RoadBuilding { .. })
        )
    {
        ROUTE_PRIORITY_AMBIENT
    } else {
        ROUTE_PRIORITY_COMMITTED
    }
}

enum IncrementalRouteResult {
    Pending,
    Found(Vec<Vec2>),
    Failed,
}

fn resume_incremental_route(
    terrain: &WorldTerrain,
    building_cache: &NavigationBuildingCache,
    job: &mut IncrementalRouteJob,
    deadline: Instant,
    telemetry: &mut RoutePlannerTelemetry,
) -> IncrementalRouteResult {
    let survey = RoadSurvey {
        terrain,
        buildings: &building_cache.blockers,
        live_buildings: Some(&building_cache.spatial),
        props: &job.props,
        start: job.start.survey,
        goal: job.goal.survey,
        min: job.start.survey.min(job.goal.survey) - Vec2::splat(SURVEY_PADDING),
        max: job.start.survey.max(job.goal.survey) + Vec2::splat(SURVEY_PADDING),
        max_nodes: EXTENDED_LOCAL_SURVEY_MAX_NODES,
    };
    let metrics_before = job.scratch.metrics;
    // Road-first requests only arrive here after no useful graph connector
    // was found. Open ground is still extremely common, so certify the one
    // straight segment before creating a 2,400-cell retained A* search. The
    // former incremental fallback skipped this fast path even though every
    // ordinary short survey already uses it.
    if !job.search.initialized {
        job.scratch.begin_search();
        if survey.line_clear(job.start.survey, job.goal.survey, &mut job.scratch) {
            telemetry.record_surveys(job.scratch.metrics - metrics_before);
            return IncrementalRouteResult::Found(complete_agent_route(
                job.start,
                job.goal,
                &job.start_apron,
                vec![job.start.survey, job.goal.survey],
                &job.goal_apron,
            ));
        }
    }
    let result = resume_survey_a_star(&survey, &mut job.scratch, &mut job.search, Some(deadline));
    telemetry.record_surveys(job.scratch.metrics - metrics_before);
    match result {
        SurveySearchResult::Pending => IncrementalRouteResult::Pending,
        SurveySearchResult::Failed => IncrementalRouteResult::Failed,
        SurveySearchResult::Found(raw) => {
            let simple = simplify_visible(&raw, &survey, &mut job.scratch);
            let resampled = resample_path(&simple, 1.5);
            let middle = if polyline_clear_live_buildings(&resampled, &building_cache.spatial) {
                resampled
            } else if polyline_clear_live_buildings(&raw, &building_cache.spatial) {
                raw
            } else {
                Vec::new()
            };
            if middle.is_empty() {
                IncrementalRouteResult::Failed
            } else {
                IncrementalRouteResult::Found(complete_agent_route(
                    job.start,
                    job.goal,
                    &job.start_apron,
                    middle,
                    &job.goal_apron,
                ))
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn install_completed_direct_route(
    commands: &mut Commands,
    graph: &mut VillageRoadGraph,
    telemetry: &mut RoutePlannerTelemetry,
    entity: Entity,
    target: Vec3,
    start: NavigationEndpoint,
    goal: NavigationEndpoint,
    direct: &[Vec2],
    props: &PropBlockers,
    terrain: &WorldTerrain,
    obstacles: Option<&SpatialObstacleGrid>,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
) -> bool {
    if direct.is_empty() {
        return false;
    }
    let tagged: Vec<_> = direct.iter().copied().map(|point| (point, false)).collect();
    let certification_started = Instant::now();
    let clear = polyline_clear_live_world_with_start_escape(
        direct,
        obstacles,
        colliders,
        derived,
        start.escaping_building,
    );
    telemetry.certification_time += certification_started.elapsed();
    if !clear {
        return false;
    }
    let reverse_is_certified =
        !start.escaping_building && reverse_route_clears_goal_prop_exemption(&tagged, props);
    graph.cache_tactical_route(start.actual, goal.actual, &tagged, reverse_is_certified);
    telemetry.routes_installed = telemetry.routes_installed.saturating_add(1);
    install_tactical_route(
        commands,
        entity,
        target,
        goal.actual,
        terrain,
        &tagged,
        start.escaping_building,
    );
    true
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
pub(super) fn reverse_route_clears_goal_prop_exemption(
    route: &[(Vec2, bool)],
    props: &PropBlockers,
) -> bool {
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
    escaping_building: bool,
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
    // Cached graph routes are reusable middle corridors. A connector can end
    // several metres before the interaction point; treating that last graph
    // node as arrival made a work routine reissue the same MoveTarget and walk
    // the same short loop forever. The route's authoritative goal must always
    // be its final waypoint. Movement-time collision certification will reject
    // and replan this last leg if the live world changed after the survey.
    append_missing_route_goal(&mut waypoints, target);
    let mut entity_commands = commands.entity(entity);
    entity_commands
        .insert(TravelRoute {
            goal: target,
            waypoints,
            next: 0,
        })
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteBackoff>()
        .remove::<NavigationRouteFailed>();
    if escaping_building {
        entity_commands.insert(NavigationObstacleEscape);
    } else {
        entity_commands.remove::<NavigationObstacleEscape>();
    }
}

fn append_missing_route_goal(waypoints: &mut Vec<RouteWaypoint>, target: Vec3) {
    if waypoints.last().is_none_or(|waypoint| {
        Vec2::new(waypoint.position.x, waypoint.position.z)
            .distance_squared(Vec2::new(target.x, target.z))
            > 0.01
    }) {
        waypoints.push(RouteWaypoint {
            position: target,
            on_road: false,
        });
    }
}

fn reject_navigation_route(
    commands: &mut Commands,
    entity: Entity,
    target: Vec3,
    previous: Option<NavigationRouteBackoff>,
    geometry_version: u64,
    road_opportunity_version: u64,
    now: f64,
    reason: &'static str,
    warning_limiter: &mut RouteWarningLimiter,
) {
    let backoff = NavigationRouteBackoff::after_failure(
        previous,
        target,
        geometry_version,
        road_opportunity_version,
        now,
        entity,
    );
    if backoff.should_warn() && warning_limiter.allow(target, now) {
        warn!(
            "Villager route to {:.1},{:.1} {reason}; retry {} no earlier than {:.1}s real time",
            target.x,
            target.z,
            backoff.failures,
            backoff.retry_after - now,
        );
    } else {
        debug!(
            "Villager route to {:.1},{:.1} remains blocked; retry {} deferred {:.1}s",
            target.x,
            target.z,
            backoff.failures,
            backoff.retry_after - now,
        );
    }
    commands
        .entity(entity)
        .remove::<TravelRoute>()
        .remove::<NavigationRoutePending>()
        .remove::<NavigationObstacleEscape>()
        .insert((NavigationRouteFailed { goal: target }, backoff));
}

#[allow(clippy::too_many_arguments)]
pub fn plan_villager_travel_routes(
    simulation_time: crate::world::simulation_time::SimulationTime,
    terrain: Option<Res<WorldTerrain>>,
    collision: TravelCollisionResources,
    budget: Res<PathfindingBudgetSettings>,
    mut request_state: Local<RouteRequestState>,
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
            Option<&NavigationRouteBackoff>,
            Option<&VillagerIntent>,
            Has<AmbientRoutine>,
        ),
        (
            With<CharacterKind>,
            Without<BuildingDoorUse>,
            Without<PierTraversal>,
        ),
    >,
    stale: Query<
        Entity,
        (
            Without<MoveTarget>,
            Or<(With<NavigationRoutePending>, With<NavigationObstacleEscape>)>,
        ),
    >,
) {
    let planner_started = Instant::now();
    let now = simulation_time.elapsed_real_seconds_f64();
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
    if let Some(colliders) = colliders {
        let collision_snapshot = &mut request_state.collision_snapshot;
        if !collision_snapshot.initialized {
            collision_snapshot.initialized = true;
            collision_snapshot.collider_version = colliders.version;
            collision_snapshot.chunk_versions = colliders.chunk_versions.clone();
        } else if collision_snapshot.collider_version != colliders.version {
            let mut changed_chunks = HashSet::new();
            for (chunk, revision) in &colliders.chunk_versions {
                if collision_snapshot.chunk_versions.get(chunk) != Some(revision) {
                    changed_chunks.insert(*chunk);
                }
            }
            for chunk in collision_snapshot.chunk_versions.keys() {
                if !colliders.chunk_versions.contains_key(chunk) {
                    changed_chunks.insert(*chunk);
                }
            }
            if changed_chunks.is_empty() {
                // Lightweight tests or legacy resources without per-chunk
                // revisions still retain the conservative old behavior.
                graph.clear_tactical_routes();
            } else {
                graph.invalidate_tactical_routes_in_chunks(&changed_chunks);
            }
            collision_snapshot.collider_version = colliders.version;
            collision_snapshot.chunk_versions = colliders.chunk_versions.clone();
        }
    }

    let removed_any = removed_buildings.read().next().is_some();
    if !building_cache.initialized || !changed_buildings.is_empty() || removed_any {
        let rebuild_started = Instant::now();
        let changed_blockers = building_cache.rebuild(placed_buildings.iter());
        // A cabin on the east side of town cannot invalidate a certified
        // migration corridor on the west. Remove only cached polylines that
        // actually touch an added, moved or removed building shell.
        graph.invalidate_tactical_routes_near_buildings(&changed_blockers);
        telemetry.blocker_rebuilds = telemetry.blocker_rebuilds.saturating_add(1);
        telemetry.blocker_time += rebuild_started.elapsed();
    }
    // Roads remain durable, but a later building or a still-live prop can
    // cover an old edge. Check every edge once when collision truth changes;
    // all villager requests then share the filtered components and route
    // cache instead of repeating collision work inside Dijkstra.
    graph.refresh_walkable_edges(
        obstacle_version,
        Some(&building_cache.spatial),
        colliders,
        derived,
    );
    for entity in stale.iter() {
        commands
            .entity(entity)
            .remove::<NavigationRoutePending>()
            .remove::<NavigationObstacleEscape>();
    }
    if movers.is_empty() {
        request_state.incremental_jobs.clear();
        telemetry.finish_invocation(planner_started);
        telemetry.maybe_report(&graph);
        return;
    }

    // A single fair queue lets hundreds of cosmetic idle walks delay a
    // builder or farmer for an entire world shift at high warp. Keep fairness
    // within each priority class, but spend the bounded budget on committed
    // simulation journeys before ambient animation.
    let mut request_buckets: [Vec<Entity>; ROUTE_PRIORITY_COUNT] =
        std::array::from_fn(|_| Vec::new());
    for (entity, _, _, _, _, intent, ambient) in movers.iter_mut() {
        request_buckets[route_request_priority(intent, ambient)].push(entity);
    }
    let active_requests: HashSet<_> = request_buckets
        .iter()
        .flat_map(|bucket| bucket.iter().copied())
        .collect();
    request_state
        .incremental_jobs
        .retain(|entity, _| active_requests.contains(entity));
    let request_count = active_requests.len();
    telemetry.pending_peak = telemetry.pending_peak.max(request_count);
    let mut bucket_starts = [0usize; ROUTE_PRIORITY_COUNT];
    let mut bucket_counts = [0usize; ROUTE_PRIORITY_COUNT];
    let mut request_order = Vec::with_capacity(request_count);
    for (priority, bucket) in request_buckets.iter_mut().enumerate() {
        bucket.sort_unstable_by_key(|entity| entity.to_bits());
        bucket_counts[priority] = bucket.len();
        if !bucket.is_empty() {
            let start = request_state.next_request_by_priority[priority] % bucket.len();
            bucket_starts[priority] = start;
            bucket.rotate_left(start);
        }
        request_order.extend(bucket.iter().copied().map(|entity| (priority, entity)));
    }
    let mut processed = 0usize;
    let mut visited_by_priority = [0usize; ROUTE_PRIORITY_COUNT];
    'requests: for (priority, entity) in request_order {
        visited_by_priority[priority] += 1;
        let Ok((entity, position, target, mut pending, route_backoff, intent, _)) =
            movers.get_mut(entity)
        else {
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
            // This is still an AI result. Silently erasing an invalid target
            // leaves the owning work routine unaware that it must choose a
            // different interaction point, so it can reissue the same order
            // forever. Preserve the destination in a failure component while
            // cancelling all planner-owned state; construction and trade
            // routines already know how to recover from that signal.
            commands
                .entity(entity)
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationObstacleEscape>()
                .insert(NavigationRouteFailed { goal: target.0 });
            continue;
        }
        let geometry_version = obstacle_version;
        let road_opportunity_version = graph.route_opportunity_version(
            Vec2::new(position.0.x, position.0.z),
            Vec2::new(target.0.x, target.0.z),
        );
        if processed >= budget.max_requests_per_tick
            || (processed > 0 && planner_started.elapsed() >= budget.max_duration())
        {
            // This request has not had its turn yet; begin with it next tick.
            telemetry.budget_yields = telemetry.budget_yields.saturating_add(1);
            visited_by_priority[priority] -= 1;
            break;
        }
        processed += 1;
        telemetry.requests = telemetry.requests.saturating_add(1);

        // The previous full A* already proved this exact route impossible for
        // the current geometry. High-level AI may have consumed its failure
        // result to preserve a work transaction, but must not make the server
        // pay for the same deterministic search again.
        if route_backoff.is_some_and(|backoff| {
            backoff.matches(target.0)
                && backoff.geometry_version == geometry_version
                && backoff.road_opportunity_version == road_opportunity_version
        }) {
            telemetry.failed_attempts = telemetry.failed_attempts.saturating_add(1);
            reject_navigation_route(
                &mut commands,
                entity,
                target.0,
                route_backoff.copied(),
                geometry_version,
                road_opportunity_version,
                now,
                "is still blocked by unchanged geometry",
                &mut request_state.warning_limiter,
            );
            continue;
        }

        if let Some(mut job) = request_state.incremental_jobs.remove(&entity) {
            let job_is_current = job.target.distance_squared(target.0) <= 0.01
                && job.geometry_version == geometry_version
                && job.road_opportunity_version == road_opportunity_version
                && job
                    .start
                    .actual
                    .distance_squared(Vec2::new(position.0.x, position.0.z))
                    <= 0.01;
            if job_is_current {
                let direct_started = Instant::now();
                let result = resume_incremental_route(
                    &terrain,
                    &building_cache,
                    &mut job,
                    planner_started + budget.max_duration(),
                    &mut telemetry,
                );
                telemetry.direct_survey_time += direct_started.elapsed();
                match result {
                    IncrementalRouteResult::Pending => {
                        request_state.incremental_jobs.insert(entity, job);
                        telemetry.budget_yields = telemetry.budget_yields.saturating_add(1);
                        // Retain this frontier, but advance the priority
                        // cursor. Pinning the cursor here can starve every
                        // worker behind a difficult route if preparation has
                        // already consumed this tick's time budget and the
                        // retained search repeatedly yields with zero nodes.
                        break;
                    }
                    IncrementalRouteResult::Found(direct) => {
                        if !install_completed_direct_route(
                            &mut commands,
                            &mut graph,
                            &mut telemetry,
                            entity,
                            target.0,
                            job.start,
                            job.goal,
                            &direct,
                            &job.props,
                            &terrain,
                            obstacles,
                            colliders,
                            derived,
                        ) {
                            telemetry.failed_attempts = telemetry.failed_attempts.saturating_add(1);
                            reject_navigation_route(
                                &mut commands,
                                entity,
                                target.0,
                                route_backoff.copied(),
                                geometry_version,
                                road_opportunity_version,
                                now,
                                "failed final live-obstacle certification",
                                &mut request_state.warning_limiter,
                            );
                        }
                        continue;
                    }
                    IncrementalRouteResult::Failed => {
                        telemetry.failed_attempts = telemetry.failed_attempts.saturating_add(1);
                        reject_navigation_route(
                            &mut commands,
                            entity,
                            target.0,
                            route_backoff.copied(),
                            geometry_version,
                            road_opportunity_version,
                            now,
                            "is blocked",
                            &mut request_state.warning_limiter,
                        );
                        continue;
                    }
                }
            }
        }

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
                start.escaping_building,
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
            &building_cache.spatial,
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
        let start_apron = if start.escaping_building {
            // This short segment is the authored doorway recovery itself.
            // Building collision is intentionally exempt until clear ground;
            // terrain and prop collision remain fully certified here and in
            // movement.
            survey_agent_route(
                &terrain,
                start.actual,
                start.survey,
                &[],
                None,
                &prop_blockers,
                &mut survey_scratch,
                AGENT_SURVEY_MAX_NODES,
            )
        } else {
            survey_agent_route(
                &terrain,
                start.actual,
                start.survey,
                &building_cache.blockers,
                Some(&building_cache.spatial),
                &prop_blockers,
                &mut survey_scratch,
                AGENT_SURVEY_MAX_NODES,
            )
        };
        let goal_apron = survey_agent_route(
            &terrain,
            goal.survey,
            goal.actual,
            &building_cache.blockers,
            Some(&building_cache.spatial),
            &prop_blockers,
            &mut survey_scratch,
            AGENT_SURVEY_MAX_NODES,
        );

        // A migration wave is one journey, not hundreds of unrelated route
        // searches. Reuse a certified approach to this exact hall door when
        // the new migrant can cheaply join it from nearby clear ground. The
        // connector is surveyed against this person's live prop/building
        // blockers and the composed route is certified again before use.
        if matches!(intent, Some(VillagerIntent::Travelling { .. }))
            && !start.escaping_building
            && !start_apron.is_empty()
            && !goal_apron.is_empty()
        {
            let candidates = graph.cohort_route_candidates(start.actual, goal.actual);
            for candidate in candidates {
                let connector = survey_agent_route(
                    &terrain,
                    start.survey,
                    candidate.start,
                    &building_cache.blockers,
                    Some(&building_cache.spatial),
                    &prop_blockers,
                    &mut survey_scratch,
                    AGENT_SURVEY_MAX_NODES,
                );
                if connector.is_empty() {
                    continue;
                }
                let mut tagged = Vec::new();
                for point in start_apron.iter().copied() {
                    append_tagged(&mut tagged, point, false);
                }
                for point in connector {
                    append_tagged(&mut tagged, point, false);
                }
                for (point, on_road) in candidate.tagged {
                    append_tagged(&mut tagged, point, on_road);
                }
                let points: Vec<_> = tagged.iter().map(|(point, _)| *point).collect();
                if !polyline_clear_live_world_with_start_escape(
                    &points, obstacles, colliders, derived, false,
                ) {
                    continue;
                }
                telemetry.cache_hits = telemetry.cache_hits.saturating_add(1);
                telemetry.routes_installed = telemetry.routes_installed.saturating_add(1);
                telemetry.direct_survey_time += direct_started.elapsed();
                telemetry.record_surveys(survey_scratch.metrics - survey_metrics_before);
                graph.cache_tactical_route(start.actual, goal.actual, &tagged, false);
                install_tactical_route(
                    &mut commands,
                    entity,
                    target.0,
                    goal.actual,
                    &terrain,
                    &tagged,
                    false,
                );
                continue 'requests;
            }
        }
        // Beyond a short local walk, try the already-built road network before
        // asking A* to search the whole start-to-goal rectangle. The previous
        // direct-first order made every outer-farm commute pay for a large A*
        // even when a cheap graph route was available. Direct A* remains the
        // bounded fallback when no road connector can be certified.
        let prefer_road_first = local_distance >= EXTENDED_LOCAL_SURVEY_MIN_DISTANCE;
        let mut direct = Vec::new();
        if !prefer_road_first {
            let direct_middle = survey_agent_route(
                &terrain,
                start.survey,
                goal.survey,
                &building_cache.blockers,
                Some(&building_cache.spatial),
                &prop_blockers,
                &mut survey_scratch,
                survey_max_nodes,
            );
            direct = complete_agent_route(start, goal, &start_apron, direct_middle, &goal_apron);
        }
        telemetry.direct_survey_time += direct_started.elapsed();
        telemetry.record_surveys(survey_scratch.metrics - survey_metrics_before);
        let direct_length = (!direct.is_empty()).then(|| polyline_length(&direct));

        let graph_started = Instant::now();
        // Every connector begins at an authored door, and the road's first
        // node is consequently often inside that building's inflated shell.
        // It is a valid road-construction anchor but not a legal place for an
        // ordinary commuter to join. Filter such nodes before the bounded
        // pair ranking, or both connector attempts can be wasted on the same
        // impossible doorway while a clear second road point sits nearby.
        let mut start_candidates: Vec<_> = graph
            .nearest_candidates(
                start.survey,
                ROAD_ROUTE_JOIN_DISTANCE,
                ROAD_ROUTE_CANDIDATE_POOL * 3,
            )
            .into_iter()
            .filter(|(node, _)| {
                !building_cache
                    .spatial
                    .point_blocked(graph.nodes[*node].point)
            })
            .take(ROAD_ROUTE_CANDIDATE_POOL)
            .collect();
        let mut goal_candidates: Vec<_> = graph
            .nearest_candidates(
                goal.survey,
                ROAD_ROUTE_JOIN_DISTANCE,
                ROAD_ROUTE_CANDIDATE_POOL * 3,
            )
            .into_iter()
            .filter(|(node, _)| {
                !building_cache
                    .spatial
                    .point_blocked(graph.nodes[*node].point)
            })
            .take(ROAD_ROUTE_CANDIDATE_POOL)
            .collect();
        // The wider pool exists only to see past nearby disconnected door
        // spurs. Once live collision components are known, retain the four
        // nearest candidates which can actually share a component with the
        // opposite endpoint. This keeps pair ranking at 4x4 rather than
        // paying for 16x16 Dijkstra searches while roads are growing.
        let goal_components: HashSet<_> = goal_candidates
            .iter()
            .map(|(node, _)| graph.component_of(*node))
            .collect();
        start_candidates.retain(|(node, _)| goal_components.contains(&graph.component_of(*node)));
        start_candidates.truncate(ROAD_ROUTE_CANDIDATES);
        let start_components: HashSet<_> = start_candidates
            .iter()
            .map(|(node, _)| graph.component_of(*node))
            .collect();
        goal_candidates.retain(|(node, _)| start_components.contains(&graph.component_of(*node)));
        goal_candidates.truncate(ROAD_ROUTE_CANDIDATES);
        let mut road_candidates = Vec::new();
        for &(start_node, start_distance) in &start_candidates {
            for &(goal_node, goal_distance) in &goal_candidates {
                if start_node == goal_node || !graph.same_component(start_node, goal_node) {
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
        let road_candidate_count = road_candidates.len();
        telemetry.graph_time += graph_started.elapsed();

        let mut chosen_road: Option<(Vec<(Vec2, bool)>, f32)> = None;
        let mut connector_attempts = 0usize;
        let mut connector_failures = 0usize;
        let mut road_certification_failures = 0usize;
        for candidate in road_candidates
            .into_iter()
            .take(AGENT_ROAD_CANDIDATES_TO_SURVEY)
        {
            connector_attempts += 1;
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
                Some(&building_cache.spatial),
                &prop_blockers,
                &mut survey_scratch,
                AGENT_SURVEY_MAX_NODES,
            );
            let goal_connector = survey_agent_route(
                &terrain,
                last,
                goal.survey,
                &building_cache.blockers,
                Some(&building_cache.spatial),
                &prop_blockers,
                &mut survey_scratch,
                AGENT_SURVEY_MAX_NODES,
            );
            telemetry.connector_survey_time += connectors_started.elapsed();
            telemetry.record_surveys(survey_scratch.metrics - survey_metrics_before);
            if start_connector.is_empty() || goal_connector.is_empty() {
                connector_failures += 1;
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
            let points: Vec<_> = route.iter().map(|(point, _)| *point).collect();
            if !polyline_clear_live_world_with_start_escape(
                &points,
                obstacles,
                colliders,
                derived,
                start.escaping_building,
            ) {
                road_certification_failures += 1;
                if road_certification_failures == 1
                    && std::env::var_os("FISTWORLD_LAB_ROUTE_DIAGNOSTICS").is_some()
                {
                    let blocked_segment =
                        points.windows(2).enumerate().find_map(|(index, pair)| {
                            (!crate::player::hero::navigation_segment_clear(
                                pair[0], pair[1], obstacles, colliders, derived,
                            ))
                            .then_some((
                                index,
                                pair[0],
                                pair[1],
                                obstacles
                                    .is_some_and(|grid| grid.segment_blocked(pair[0], pair[1])),
                            ))
                        });
                    let blocking_obstacles: Vec<_> = blocked_segment
                        .and_then(|(_, start, end, obstacle_blocked)| {
                            obstacle_blocked.then_some((start, end))
                        })
                        .into_iter()
                        .flat_map(|(start, end)| {
                            obstacles.into_iter().flat_map(move |grid| {
                                grid.get_nearby((start + end) * 0.5)
                                    .filter_map(move |entry| {
                                        let local_start = shared::rotation::world_to_local_xz(
                                            start - entry.center,
                                            entry.rotation,
                                        );
                                        let local_end = shared::rotation::world_to_local_xz(
                                            end - entry.center,
                                            entry.rotation,
                                        );
                                        shared::spatial::segment_intersects_box_after_start(
                                            local_start,
                                            local_end,
                                            entry.half_extents,
                                        )
                                        .then_some((
                                            entry.center,
                                            entry.half_extents,
                                            entry.rotation,
                                            entry.obstacle_type,
                                        ))
                                    })
                            })
                        })
                        .collect();
                    eprintln!(
                        "LAB road certification diagnostic entity={entity:?} points={} blocked_segment={blocked_segment:?} obstacles={blocking_obstacles:?}",
                        points.len(),
                    );
                }
                continue;
            }
            chosen_road = Some((route, weighted));
            break;
        }

        // A road-first request only reaches this fallback if the graph has no
        // connected/certified pair of nearby nodes. This makes the expensive
        // search exceptional. Unlike the short 400-node surveys, this larger
        // search is retained and resumed across ticks so no single request can
        // consume an entire server frame.
        if chosen_road.is_none() && direct.is_empty() && prefer_road_first {
            if std::env::var_os("FISTWORLD_LAB_ROUTE_DIAGNOSTICS").is_some() {
                eprintln!(
                    "LAB route diagnostic entity={entity:?} from={:.1},{:.1} to={:.1},{:.1} distance={local_distance:.1} graph_nodes={} start_candidates={} goal_candidates={} road_pairs={road_candidate_count} attempted={connector_attempts} connector_failures={connector_failures} certification_failures={road_certification_failures}",
                    start.survey.x,
                    start.survey.y,
                    goal.survey.x,
                    goal.survey.y,
                    graph.nodes.len(),
                    start_candidates.len(),
                    goal_candidates.len(),
                );
            }
            request_state.incremental_jobs.insert(
                entity,
                IncrementalRouteJob {
                    target: target.0,
                    start,
                    goal,
                    start_apron,
                    goal_apron,
                    props: prop_blockers,
                    scratch: SurveyScratch::default(),
                    search: SurveySearchState::default(),
                    geometry_version,
                    road_opportunity_version,
                },
            );
            telemetry.budget_yields = telemetry.budget_yields.saturating_add(1);
            // Preserve the frontier without pinning the fair queue to this
            // actor. It receives another bounded slice after its peers have
            // had a turn.
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
            reject_navigation_route(
                &mut commands,
                entity,
                target.0,
                route_backoff.copied(),
                geometry_version,
                road_opportunity_version,
                now,
                "is blocked",
                &mut request_state.warning_limiter,
            );
            continue;
        };

        // The live spatial grid is movement's final authority. Keep this
        // inexpensive last certification even though every surveyed connector
        // already reads the grid: graph composition and endpoint stitching
        // must never be able to hand movement a segment it will immediately
        // reject and re-request forever.
        let certification_started = Instant::now();
        let tagged_points: Vec<_> = tagged.iter().map(|(point, _)| *point).collect();
        let live_route_clear = polyline_clear_live_world_with_start_escape(
            &tagged_points,
            obstacles,
            colliders,
            derived,
            start.escaping_building,
        );
        telemetry.certification_time += certification_started.elapsed();
        if !live_route_clear {
            let certification_started = Instant::now();
            let direct_clear = polyline_clear_live_world_with_start_escape(
                &direct,
                obstacles,
                colliders,
                derived,
                start.escaping_building,
            );
            telemetry.certification_time += certification_started.elapsed();
            if direct_clear && !direct.is_empty() {
                tagged = direct.iter().copied().map(|point| (point, false)).collect();
            } else {
                telemetry.failed_attempts = telemetry.failed_attempts.saturating_add(1);
                reject_navigation_route(
                    &mut commands,
                    entity,
                    target.0,
                    route_backoff.copied(),
                    geometry_version,
                    road_opportunity_version,
                    now,
                    "failed final live-obstacle certification",
                    &mut request_state.warning_limiter,
                );
                continue;
            }
        }

        let reverse_is_certified = !start.escaping_building
            && reverse_route_clears_goal_prop_exemption(&tagged, &prop_blockers);
        graph.cache_tactical_route(start.actual, goal.actual, &tagged, reverse_is_certified);
        telemetry.routes_installed = telemetry.routes_installed.saturating_add(1);
        install_tactical_route(
            &mut commands,
            entity,
            target.0,
            goal.actual,
            &terrain,
            &tagged,
            start.escaping_building,
        );
    }
    for priority in 0..ROUTE_PRIORITY_COUNT {
        if bucket_counts[priority] != 0 {
            request_state.next_request_by_priority[priority] =
                (bucket_starts[priority] + visited_by_priority[priority]) % bucket_counts[priority];
        }
    }
    telemetry.finish_invocation(planner_started);
    telemetry.maybe_report(&graph);
}

#[cfg(test)]
mod local_tests {
    use super::*;

    #[test]
    fn nearby_migrants_can_join_one_certified_destination_route() {
        let mut graph = VillageRoadGraph::default();
        let start = Vec2::new(100.0, 20.0);
        let goal = Vec2::new(0.0, 0.0);
        let route = vec![
            (start, false),
            (Vec2::new(50.0, 10.0), false),
            (goal, false),
        ];
        graph.cache_tactical_route(start, goal, &route, false);

        let nearby = graph.cohort_route_candidates(start + Vec2::new(4.0, -3.0), goal);
        assert_eq!(nearby.len(), 1);
        assert_eq!(nearby[0].tagged, route);
        assert!(graph
            .cohort_route_candidates(start + Vec2::X * (COHORT_ROUTE_JOIN_DISTANCE + 1.0), goal)
            .is_empty());
    }

    #[test]
    fn each_new_certified_start_advances_that_destinations_opportunity() {
        let mut graph = VillageRoadGraph::default();
        let first_start = Vec2::new(100.0, 20.0);
        let second_start = Vec2::new(96.0, 18.0);
        let goal = Vec2::new(0.0, 0.0);
        let other_goal = Vec2::new(20.0, 30.0);
        let first_route = vec![(first_start, false), (goal, false)];

        assert_eq!(graph.cohort_route_opportunity_version(goal), 0);
        graph.cache_tactical_route(first_start, goal, &first_route, false);
        let first_version = graph.cohort_route_opportunity_version(goal);
        assert!(first_version > 0);
        assert_eq!(graph.cohort_route_opportunity_version(other_goal), 0);

        graph.cache_tactical_route(first_start, goal, &first_route, false);
        assert_eq!(
            graph.cohort_route_opportunity_version(goal),
            first_version,
            "refreshing the same proof must not wake failed migrants repeatedly"
        );

        graph.cache_tactical_route(
            second_start,
            goal,
            &[(second_start, false), (goal, false)],
            false,
        );
        assert!(graph.cohort_route_opportunity_version(goal) > first_version);
    }

    #[test]
    fn disconnected_door_spurs_cannot_hide_a_nearby_connected_street() {
        let mut graph = VillageRoadGraph::default();
        for x in [1.0, 2.0, 3.0, 4.0, 6.0, 80.0] {
            graph.nodes.push(RoadGraphNode {
                point: Vec2::new(x, 0.0),
                edges: Vec::new(),
            });
        }
        let distance = graph.nodes[4].point.distance(graph.nodes[5].point);
        graph.nodes[4].edges.push((5, distance));
        graph.nodes[5].edges.push((4, distance));
        graph.rebuild_components();

        let start_candidates = graph.nearest_candidates(
            Vec2::ZERO,
            ROAD_ROUTE_JOIN_DISTANCE,
            ROAD_ROUTE_CANDIDATE_POOL,
        );
        let goal_candidates = graph.nearest_candidates(
            Vec2::new(80.0, 0.0),
            ROAD_ROUTE_JOIN_DISTANCE,
            ROAD_ROUTE_CANDIDATE_POOL,
        );

        assert!(start_candidates
            .iter()
            .take(4)
            .all(|(start, _)| goal_candidates
                .iter()
                .all(|(goal, _)| !graph.same_component(*start, *goal))));
        let connected = start_candidates
            .iter()
            .find_map(|(start, _)| {
                goal_candidates
                    .iter()
                    .find(|(goal, _)| graph.same_component(*start, *goal))
                    .map(|(goal, _)| (*start, *goal))
            })
            .expect("the wider pool must reach the completed street");
        assert_eq!(
            graph.shortest_path(connected.0, connected.1),
            Some(vec![4, 5])
        );
    }

    #[test]
    fn graph_routing_avoids_a_legacy_road_edge_covered_by_a_building() {
        let mut graph = VillageRoadGraph::default();
        for point in [
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(0.0, 10.0),
            Vec2::new(10.0, 10.0),
        ] {
            graph.nodes.push(RoadGraphNode {
                point,
                edges: Vec::new(),
            });
        }
        for (a, b) in [(0, 1), (0, 2), (2, 3), (3, 1)] {
            let distance = graph.nodes[a].point.distance(graph.nodes[b].point);
            graph.nodes[a].edges.push((b, distance));
            graph.nodes[b].edges.push((a, distance));
        }
        let mut buildings = SpatialObstacleGrid::default();
        buildings.insert(shared::spatial::ObstacleEntry {
            center: Vec2::new(5.0, 0.0),
            half_extents: Vec2::splat(1.0),
            rotation: 0.0,
            obstacle_type: 0,
        });
        graph.refresh_walkable_edges(1, Some(&buildings), None, None);

        assert_eq!(graph.shortest_path(0, 1), Some(vec![0, 2, 3, 1]));
    }

    #[test]
    fn route_warnings_are_coalesced_by_destination() {
        let mut limiter = RouteWarningLimiter::default();
        let hall = Vec3::new(200.0, 0.0, -80.0);
        assert!(limiter.allow(hall, 10.0));
        assert!(!limiter.allow(hall + Vec3::new(0.4, 0.0, 0.4), 11.0));
        assert!(limiter.allow(hall, 10.0 + ROUTE_WARNING_INTERVAL_SECONDS));
    }

    #[test]
    fn a_cached_middle_corridor_cannot_finish_before_the_move_target() {
        let connector = Vec3::new(59.8, 0.0, -194.2);
        let target = Vec3::new(55.9, 0.0, -199.6);
        let mut waypoints = vec![RouteWaypoint {
            position: connector,
            on_road: true,
        }];

        append_missing_route_goal(&mut waypoints, target);

        assert_eq!(waypoints.len(), 2);
        assert_eq!(waypoints.last().unwrap().position, target);
        append_missing_route_goal(&mut waypoints, target);
        assert_eq!(waypoints.len(), 2, "the target must not be duplicated");
    }

    #[test]
    fn streamed_prop_change_invalidates_only_routes_crossing_that_chunk() {
        let mut graph = VillageRoadGraph::default();
        let start = Vec2::new(8.0, 8.0);
        let goal = Vec2::new(24.0, 8.0);
        let route = vec![(start, false), (goal, false)];
        graph.cache_tactical_route(start, goal, &route, false);

        graph.invalidate_tactical_routes_in_chunks(&HashSet::from([ChunkCoord::new(4, 4)]));
        assert!(graph.tactical_route(start, goal).is_some());

        graph.invalidate_tactical_routes_in_chunks(&HashSet::from([ChunkCoord::new(0, 0)]));
        assert!(graph.tactical_route(start, goal).is_none());
    }
}
