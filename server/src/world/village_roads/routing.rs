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
    pub(super) routes: HashMap<(usize, usize), Vec<usize>>,
    pub(super) tactical_routes: HashMap<TacticalRouteKey, Vec<(Vec2, bool)>>,
    pub(super) tactical_order: VecDeque<TacticalRouteKey>,
    pub(super) tactical_obstacle_version: Option<u64>,
    pub(super) road_revision: u64,
    pub(super) initialized: bool,
}

const MAX_TACTICAL_ROUTE_CACHE_ENTRIES: usize = 8_192;

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

    pub(super) fn sync_tactical_obstacle_version(&mut self, obstacle_version: u64) {
        if self.tactical_obstacle_version != Some(obstacle_version) {
            self.clear_tactical_routes();
            self.tactical_obstacle_version = Some(obstacle_version);
        }
    }

    pub(super) fn tactical_route(&self, start: Vec2, goal: Vec2) -> Option<&[(Vec2, bool)]> {
        self.tactical_routes
            .get(&TacticalRouteKey::new(start, goal))
            .map(Vec::as_slice)
    }

    pub(super) fn cache_tactical_route(
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
