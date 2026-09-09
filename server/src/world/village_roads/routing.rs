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
    /// Durable road topology before temporary/legacy collision cuts. This is
    /// a safe fallback candidate source only: every resulting polyline still
    /// receives the ordinary live-world certification before installation.
    topology_components: Vec<usize>,
    /// Current collision can make a durable road edge unusable. This shared
    /// set is rebuilt once per geometry revision rather than during every
    /// villager's Dijkstra search.
    blocked_edges: HashSet<((i32, i32), (i32, i32))>,
    checked_edges: HashSet<((i32, i32), (i32, i32))>,
    walkability_road_revision: u64,
    walkability_obstacle_version: u64,
    pub(super) routes: HashMap<(usize, usize), Vec<usize>>,
    topology_routes: HashMap<(usize, usize), Vec<usize>>,
    /// One reverse shortest-path tree per popular destination road node.
    /// A commute wave to a hall, market or workplace now pays for Dijkstra
    /// once and every different origin walks the same immutable tree.
    pub(super) destination_trees: HashMap<usize, Vec<Option<usize>>>,
    destination_tree_order: VecDeque<usize>,
    topology_destination_trees: HashMap<usize, Vec<Option<usize>>>,
    topology_destination_tree_order: VecDeque<usize>,
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
const MAX_DESTINATION_TREES: usize = 64;
const MAX_COHORT_OPPORTUNITY_GOALS: usize = MAX_TACTICAL_ROUTE_CACHE_ENTRIES;
const MAX_COHORT_ROUTES_PER_GOAL: usize = 8;
/// Try only the nearest couple of reusable approaches before falling back to
/// the ordinary road/direct planner. Every connector attempt is a bounded A*
/// search, so probing all eight cached approaches can turn one hostile start
/// position into a visible server hitch without improving the eventual route.
const COHORT_ROUTE_CANDIDATES_TO_SURVEY: usize = 2;
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
    graph.topology_routes.clear();
    graph.destination_trees.clear();
    graph.destination_tree_order.clear();
    graph.topology_destination_trees.clear();
    graph.topology_destination_tree_order.clear();
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
    graph.rebuild_topology_components();
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
    fn rebuild_topology_components(&mut self) {
        self.topology_components.clear();
        self.topology_components
            .resize(self.nodes.len(), usize::MAX);
        let mut frontier = Vec::new();
        for root in 0..self.nodes.len() {
            if self.topology_components[root] != usize::MAX {
                continue;
            }
            self.topology_components[root] = root;
            frontier.push(root);
            while let Some(node) = frontier.pop() {
                for &(next, _) in &self.nodes[node].edges {
                    if self.topology_components[next] == usize::MAX {
                        self.topology_components[next] = root;
                        frontier.push(next);
                    }
                }
            }
        }
    }

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

    fn same_topology_component(&self, start: usize, goal: usize) -> bool {
        self.topology_components
            .get(start)
            .zip(self.topology_components.get(goal))
            .is_none_or(|(start, goal)| start == goal)
    }

    fn topology_component_of(&self, node: usize) -> usize {
        self.topology_components.get(node).copied().unwrap_or(0)
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
        self.destination_trees.clear();
        self.destination_tree_order.clear();
        self.rebuild_components();
        self.walkability_road_revision = self.road_revision;
        self.walkability_obstacle_version = obstacle_version;
    }

    fn clear_tactical_routes(&mut self) {
        self.tactical_routes.clear();
        self.tactical_order.clear();
        self.cohort_routes.clear();
    }

    /// Embodied movement found one stale segment. Discard only cached routes
    /// which pass through that segment; a tree appearing beside one cabin
    /// must not erase every certified commute on the other side of town.
    pub(crate) fn invalidate_tactical_routes_after_embodied_rejection(
        &mut self,
        rejected_start: Vec2,
        rejected_end: Vec2,
    ) {
        const REJECTION_INFLUENCE: f32 = 0.9;
        let threshold_squared = REJECTION_INFLUENCE * REJECTION_INFLUENCE;
        self.tactical_routes.retain(|_, route| {
            !route.windows(2).any(|pair| {
                segments_are_near(
                    pair[0].0,
                    pair[1].0,
                    rejected_start,
                    rejected_end,
                    threshold_squared,
                )
            })
        });
        self.tactical_order
            .retain(|key| self.tactical_routes.contains_key(key));
        self.rebuild_cohort_routes();
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
        if !self.same_component(start, goal) {
            return None;
        }
        if !self.destination_trees.contains_key(&goal) {
            let tree = self.build_destination_tree(goal, false);
            self.destination_trees.insert(goal, tree);
            self.destination_tree_order.push_back(goal);
            while self.destination_trees.len() > MAX_DESTINATION_TREES {
                let Some(oldest) = self.destination_tree_order.pop_front() else {
                    break;
                };
                self.destination_trees.remove(&oldest);
            }
        }
        let tree = self.destination_trees.get(&goal)?;
        let mut path = vec![start];
        let mut cursor = start;
        while cursor != goal {
            cursor = tree.get(cursor).copied().flatten()?;
            path.push(cursor);
            if path.len() > self.nodes.len() {
                return None;
            }
        }
        self.routes.insert((start, goal), path.clone());
        let mut reverse = path.clone();
        reverse.reverse();
        self.routes.insert((goal, start), reverse);
        Some(path)
    }

    fn shortest_topology_path(&mut self, start: usize, goal: usize) -> Option<Vec<usize>> {
        if let Some(path) = self.topology_routes.get(&(start, goal)) {
            return Some(path.clone());
        }
        if !self.same_topology_component(start, goal) {
            return None;
        }
        if !self.topology_destination_trees.contains_key(&goal) {
            let tree = self.build_destination_tree(goal, true);
            self.topology_destination_trees.insert(goal, tree);
            self.topology_destination_tree_order.push_back(goal);
            while self.topology_destination_trees.len() > MAX_DESTINATION_TREES {
                let Some(oldest) = self.topology_destination_tree_order.pop_front() else {
                    break;
                };
                self.topology_destination_trees.remove(&oldest);
            }
        }
        let tree = self.topology_destination_trees.get(&goal)?;
        let mut path = vec![start];
        let mut cursor = start;
        while cursor != goal {
            cursor = tree.get(cursor).copied().flatten()?;
            path.push(cursor);
            if path.len() > self.nodes.len() {
                return None;
            }
        }
        self.topology_routes.insert((start, goal), path.clone());
        let mut reverse = path.clone();
        reverse.reverse();
        self.topology_routes.insert((goal, start), reverse);
        Some(path)
    }

    fn build_destination_tree(
        &self,
        goal: usize,
        include_collision_blocked_edges: bool,
    ) -> Vec<Option<usize>> {
        let mut open = BinaryHeap::new();
        let mut score = vec![f32::INFINITY; self.nodes.len()];
        // `next[node]` points one edge closer to the destination. Since village
        // roads are undirected, expanding outwards from the goal constructs a
        // reusable reverse tree for every possible origin.
        let mut next_toward_goal = vec![None; self.nodes.len()];
        score[goal] = 0.0;
        open.push(GraphOpen {
            cost: 0,
            node: goal,
        });
        while let Some(GraphOpen { cost, node }) = open.pop() {
            if cost > (score[node] * 1000.0) as i32 {
                continue;
            }
            for &(next, edge) in &self.nodes[node].edges {
                if !include_collision_blocked_edges && self.edge_is_blocked(node, next) {
                    continue;
                }
                let tentative = score[node] + edge;
                if tentative >= score[next] {
                    continue;
                }
                score[next] = tentative;
                next_toward_goal[next] = Some(node);
                open.push(GraphOpen {
                    cost: (tentative * 1000.0) as i32,
                    node: next,
                });
            }
        }
        next_toward_goal
    }
}

fn segments_are_near(a0: Vec2, a1: Vec2, b0: Vec2, b1: Vec2, threshold_squared: f32) -> bool {
    if segments_intersect(a0, a1, b0, b1) {
        return true;
    }
    point_segment_distance_squared(a0, b0, b1) <= threshold_squared
        || point_segment_distance_squared(a1, b0, b1) <= threshold_squared
        || point_segment_distance_squared(b0, a0, a1) <= threshold_squared
        || point_segment_distance_squared(b1, a0, a1) <= threshold_squared
}

fn point_segment_distance_squared(point: Vec2, start: Vec2, end: Vec2) -> f32 {
    let segment = end - start;
    let length_squared = segment.length_squared();
    if length_squared <= 1.0e-6 {
        return point.distance_squared(start);
    }
    let t = ((point - start).dot(segment) / length_squared).clamp(0.0, 1.0);
    point.distance_squared(start + segment * t)
}

fn segments_intersect(a0: Vec2, a1: Vec2, b0: Vec2, b1: Vec2) -> bool {
    fn cross(a: Vec2, b: Vec2) -> f32 {
        a.x * b.y - a.y * b.x
    }
    let a = a1 - a0;
    let b = b1 - b0;
    let denominator = cross(a, b);
    if denominator.abs() <= 1.0e-6 {
        return false;
    }
    let offset = b0 - a0;
    let t = cross(offset, b) / denominator;
    let u = cross(offset, a) / denominator;
    (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)
}

/// Record changed villager destinations before movement, and recover any
/// authoritative target whose route state was orphaned during a handoff.
/// Route work is split into a bounded second system, so a crowd receiving jobs
/// on one tick cannot create an unbounded A* spike.
type RouteMoverData = (
    Entity,
    &'static CharacterKind,
    &'static MoveTarget,
    Option<&'static NavigationRouteFailed>,
    Option<&'static NavigationRouteBackoff>,
);

type RouteMoverFilter = (
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
    Without<crate::world::village::ambient::AmbientDirectTransit>,
    // Combatants move like heroes - directly, collision-gated - and never
    // enter the road planner: step_units freezes a villager whose route is
    // pending or failed, and a soldier or raider on raw battlefield ground
    // must walk, not stand paralyzed waiting for A*.
    Without<shared::components::CommandedBy>,
    Without<crate::player::combat::WarParty>,
    // Only an active forecourt step owns movement. A stale transit marker must
    // not strand someone after their ticket is consumed and another routine
    // takes over.
    Or<(
        Without<crate::world::village::MootQueueTransit>,
        Without<crate::world::village::MootQueueTicket>,
    )>,
);

type RouteMoverQuery<'w, 's> = Query<'w, 's, RouteMoverData, RouteMoverFilter>;

pub fn queue_villager_travel_routes(
    simulation_time: crate::world::simulation_time::SimulationTime,
    mut commands: Commands,
    movers: RouteMoverQuery,
    formations: Query<
        (),
        Or<(
            With<crate::player::combat::fronts::FormationMember>,
            With<crate::player::combat::DirectCombatApproach>,
        )>,
    >,
) {
    let now = simulation_time.elapsed_real_seconds_f64();
    for (entity, kind, target, failed, backoff) in movers.iter() {
        if *kind != CharacterKind::Villager || formations.contains(entity) {
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
    max_nodes: usize,
    scratch: SurveyScratch,
    search: SurveySearchState,
    geometry_version: u64,
    road_opportunity_version: u64,
    /// The search owns an immutable blocker snapshot. Geometry may change
    /// while a long local or regional route is spread across ticks; the
    /// completed result is always certified against the live world before it
    /// can move an actor. A stale failure/result simply starts again from the
    /// current snapshot, so harmless town growth must not reset useful A*
    /// progress every frame.
    allow_geometry_drift: bool,
    /// Regional routes use a coarser middle corridor but exact endpoint
    /// searches. This is independent of geometry-drift safety: long local
    /// porter/work routes also retain progress, but keep full local fidelity.
    regional_corridor: bool,
}

#[derive(Default)]
pub(crate) struct RouteRequestState {
    // A stable entity cursor survives insertions and removals ahead of it in
    // the sorted queue. An index cursor can skip the same request forever when
    // a busy town continuously changes the bucket's membership between ticks.
    last_request_bits_by_priority: [Option<u64>; ROUTE_PRIORITY_COUNT],
    queued_since_real_seconds: HashMap<Entity, f64>,
    schedule_slot: usize,
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

const ROUTE_PRIORITY_COUNT: usize = 5;
const ROUTE_PRIORITY_CARAVAN: usize = 0;
const ROUTE_PRIORITY_ESSENTIAL: usize = 1;
const ROUTE_PRIORITY_COMMITTED: usize = 2;
const ROUTE_PRIORITY_LEISURE: usize = 3;
const ROUTE_PRIORITY_AMBIENT: usize = 4;
/// Weighted round-robin admission. This preserves a little visual life under
/// load without allowing cosmetic walks to form an unbounded head-of-line
/// block in front of food, cargo, work or migration.
const ROUTE_PRIORITY_SCHEDULE: [usize; 11] = [0, 1, 2, 1, 2, 1, 3, 2, 1, 2, 4];
/// A retained multi-tick search gets a tiny slice measured from the moment it
/// actually resumes. Shared road/collision preparation may legitimately use
/// the ordinary 2 ms budget first; without this reservation a growing town
/// can leave a porter, worker or caravan expanding zero useful nodes forever.
const RETAINED_INCREMENTAL_SLICE: Duration = Duration::from_millis(1);

/// Embodied work, construction, migration, shopping and home journeys must
/// not queue behind cosmetic roadside wandering during a population burst.
/// AmbientRoutine is the authoritative marker for the latter; every other
/// destination was selected by a committed simulation state machine.
fn route_request_priority(
    intent: Option<&VillagerIntent>,
    ambient: bool,
    objective: Option<&CharacterObjective>,
) -> usize {
    if is_intersettlement_route_objective(objective) {
        return ROUTE_PRIORITY_CARAVAN;
    }
    if is_essential_route_objective(objective) {
        return ROUTE_PRIORITY_ESSENTIAL;
    }
    // AmbientRoutine can survive a handoff into a real work routine until the
    // ambient system next owns the actor. CharacterObjective is synchronized
    // after every activity system and therefore tells us why this particular
    // MoveTarget exists. Without it, a woodcutter carrying the stale marker
    // can sit forever behind a continuously replenished committed queue.
    let genuinely_ambient = matches!(
        objective,
        Some(CharacterObjective::WalkingAroundTown | CharacterObjective::Resting)
    ) || (objective.is_none() && ambient);
    if genuinely_ambient
        && !matches!(
            intent,
            Some(VillagerIntent::Building { .. } | VillagerIntent::RoadBuilding { .. })
        )
    {
        return ROUTE_PRIORITY_AMBIENT;
    }
    if matches!(
        objective,
        Some(
            CharacterObjective::GoingToTavern
                | CharacterObjective::WaitingForTavernService
                | CharacterObjective::LeavingTavern
        )
    ) {
        ROUTE_PRIORITY_LEISURE
    } else {
        ROUTE_PRIORITY_COMMITTED
    }
}

fn is_essential_route_objective(objective: Option<&CharacterObjective>) -> bool {
    matches!(
        objective,
        Some(
            CharacterObjective::CarryingConstructionWood
                // A served wood collector holds the freight counter (and the
                // whole line behind it) until this route is installed; the
                // held ticket reports Collecting, not Carrying.
                | CharacterObjective::CollectingConstructionWood
                | CharacterObjective::ReturningWithHouseholdFood
                | CharacterObjective::CollectingMarketGoods
                | CharacterObjective::DeliveringMarketGoods
                | CharacterObjective::ReturningHarvest
                | CharacterObjective::ReturningCatch
                | CharacterObjective::ReturningTimber
                | CharacterObjective::ReturningStone
                | CharacterObjective::ReturningLivestockProducts
                | CharacterObjective::CollectingCompanyInputs
                | CharacterObjective::DeliveringCompanyInputs
                | CharacterObjective::GoingHome
                | CharacterObjective::ShelteringAtMoot
        )
    )
}

/// Previous navigation-frame pressure used by ambient admission. Counts are
/// deliberately grouped by settlement so several visible towns can each feel
/// alive while one large city cannot enqueue a thousand optional strolls.
#[derive(Resource, Default)]
pub struct NavigationLoad {
    pub(crate) pending_total: usize,
    pub(crate) ambient_by_settlement: HashMap<Entity, usize>,
}

#[derive(SystemParam)]
pub(crate) struct RoutePlannerAux<'w, 's> {
    navigation_load: Option<ResMut<'w, NavigationLoad>>,
    placed_buildings: Query<'w, 's, (&'static PlacedBuilding, &'static BuildingPosition)>,
    changed_buildings: Query<'w, 's, (), Or<(Changed<PlacedBuilding>, Changed<BuildingPosition>)>>,
    removed_buildings: RemovedComponents<'w, 's, PlacedBuilding>,
    defenses: Query<'w, 's, &'static shared::components::FortificationSegment>,
    changed_defenses: Query<'w, 's, (), Changed<shared::components::FortificationSegment>>,
    removed_defenses: RemovedComponents<'w, 's, shared::components::FortificationSegment>,
    active_ambient_routes: Query<
        'w,
        's,
        &'static AmbientRoutine,
        (
            Or<(
                With<TravelRoute>,
                With<crate::world::village::ambient::AmbientDirectTransit>,
            )>,
            Without<NavigationRoutePending>,
        ),
    >,
}

fn is_intersettlement_route_objective(objective: Option<&CharacterObjective>) -> bool {
    matches!(
        objective,
        Some(
            CharacterObjective::HaulingInterSettlementCargo
                | CharacterObjective::ReturningFromTradeRoute
        )
    )
}

fn needs_regional_corridor(objective: Option<&CharacterObjective>, local_distance: f32) -> bool {
    // Migration approaches were CERTIFIED with the intersettlement tier (its
    // node budget and 192 m detour window), so the live walk must plan with
    // the same tier or a certified approach can be unroutable in practice: a
    // boat immigrant landing 48-512 m up the coast used to fall into the
    // extended-local tier, whose 20 m padded window cannot express a headland
    // detour — and then stood at the landfall "waiting to retry" forever.
    is_intersettlement_route_objective(objective)
        || (local_distance > EXTENDED_LOCAL_SURVEY_MIN_DISTANCE
            && matches!(objective, Some(CharacterObjective::TravellingToSettlement)))
}

fn agent_survey_max_nodes(local_distance: f32, objective: Option<&CharacterObjective>) -> usize {
    if needs_regional_corridor(objective, local_distance) {
        INTERSETTLEMENT_TRADE_SURVEY_MAX_NODES
    } else if (EXTENDED_LOCAL_SURVEY_MIN_DISTANCE..=EXTENDED_LOCAL_SURVEY_MAX_DISTANCE)
        .contains(&local_distance)
    {
        EXTENDED_LOCAL_SURVEY_MAX_NODES
    } else {
        AGENT_SURVEY_MAX_NODES
    }
}

fn incremental_versions_compatible(
    allow_geometry_drift: bool,
    job_geometry_version: u64,
    current_geometry_version: u64,
    job_road_opportunity_version: u64,
    current_road_opportunity_version: u64,
) -> bool {
    allow_geometry_drift
        || (job_geometry_version == current_geometry_version
            && job_road_opportunity_version == current_road_opportunity_version)
}

fn rotate_request_bucket_after(bucket: &mut [Entity], last: Option<u64>) {
    bucket.sort_unstable_by_key(|entity| entity.to_bits());
    if let Some(last) = last {
        let start = bucket
            .iter()
            .position(|entity| entity.to_bits() > last)
            .unwrap_or(0);
        bucket.rotate_left(start);
    }
}

fn first_committed_schedule_request(
    request_buckets: &[Vec<Entity>; ROUTE_PRIORITY_COUNT],
    schedule_slot: usize,
) -> Option<(usize, usize, Entity)> {
    (0..ROUTE_PRIORITY_SCHEDULE.len()).find_map(|offset| {
        let schedule_index = (schedule_slot + offset) % ROUTE_PRIORITY_SCHEDULE.len();
        let priority = ROUTE_PRIORITY_SCHEDULE[schedule_index];
        (priority <= ROUTE_PRIORITY_COMMITTED)
            .then(|| request_buckets[priority].first().copied())
            .flatten()
            .map(|entity| (schedule_index, priority, entity))
    })
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
    let extended_local = !job.regional_corridor && job.max_nodes > AGENT_SURVEY_MAX_NODES;
    let survey_padding = if job.regional_corridor {
        INTERSETTLEMENT_SURVEY_PADDING
    } else {
        SURVEY_PADDING
    };
    let survey = RoadSurvey {
        terrain,
        buildings: &building_cache.blockers,
        live_buildings: Some(&building_cache.spatial),
        props: &job.props,
        start: job.start.survey,
        goal: job.goal.survey,
        min: job.start.survey.min(job.goal.survey) - Vec2::splat(survey_padding),
        max: job.start.survey.max(job.goal.survey) + Vec2::splat(survey_padding),
        max_nodes: job.max_nodes,
        cell_size: SURVEY_CELL,
        coarse_stride: if job.regional_corridor {
            INTERSETTLEMENT_SURVEY_STRIDE
        } else if extended_local {
            EXTENDED_LOCAL_SURVEY_STRIDE
        } else {
            1
        },
        fine_endpoint_radius: if job.regional_corridor {
            INTERSETTLEMENT_FINE_ENDPOINT_RADIUS
        } else if extended_local {
            EXTENDED_LOCAL_FINE_ENDPOINT_RADIUS
        } else {
            0.0
        },
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
    geometry_version: u64,
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
        geometry_version,
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
    pending_peak_by_priority: [usize; ROUTE_PRIORITY_COUNT],
    planner_budget_peak: Duration,
    oldest_committed_wait_seconds: f64,
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
            pending_peak_by_priority: [0; ROUTE_PRIORITY_COUNT],
            planner_budget_peak: Duration::ZERO,
            oldest_committed_wait_seconds: 0.0,
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
            "VillageRoutePerf calls={} requests={} installed={} failed_attempts={} pending_peak={} priority_peak={:?} planner_budget_peak_ms={:.1} oldest_committed_wait_s={:.2} budget_yields={} cache={}/{} ({:.1}%) entries={} destination_trees={} road_rev={} blockers_rebuilt={} surveys={} expanded={} memo_blocked={:.1}% memo_lines={:.1}% time_ms blockers={:.2} props={:.2} direct={:.2} graph={:.2} connectors={:.2} certify={:.2} total={:.2} max_call={:.2}",
            self.invocations,
            self.requests,
            self.routes_installed,
            self.failed_attempts,
            self.pending_peak,
            self.pending_peak_by_priority,
            self.planner_budget_peak.as_secs_f64() * 1_000.0,
            self.oldest_committed_wait_seconds,
            self.budget_yields,
            self.cache_hits,
            total_lookups,
            cache_hit_percent,
            graph.tactical_routes.len(),
            graph.destination_trees.len(),
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
    geometry_version: u64,
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
            geometry_version,
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
    mut aux: RoutePlannerAux,
    mut movers: Query<
        (
            Entity,
            &PlayerPosition,
            &MoveTarget,
            &mut NavigationRoutePending,
            Option<&NavigationRouteBackoff>,
            Option<&VillagerIntent>,
            Option<&AmbientRoutine>,
            Option<&CharacterObjective>,
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
    // Tactical cache entries are certified against both kinds of collision.
    // A streamed or newly cleared prop must invalidate them just as surely as
    // a newly completed building.
    let obstacle_version = navigation_geometry_version(obstacles, colliders);
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

    let removed_any = !aux.removed_buildings.is_empty() || !aux.removed_defenses.is_empty();
    aux.removed_buildings.clear();
    aux.removed_defenses.clear();
    if !building_cache.initialized
        || !aux.changed_buildings.is_empty()
        || !aux.changed_defenses.is_empty()
        || removed_any
    {
        let rebuild_started = Instant::now();
        let changed_blockers =
            building_cache.rebuild(aux.placed_buildings.iter(), aux.defenses.iter());
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
        if let Some(load) = aux.navigation_load.as_deref_mut() {
            load.pending_total = 0;
            load.ambient_by_settlement.clear();
            for routine in aux.active_ambient_routes.iter() {
                *load
                    .ambient_by_settlement
                    .entry(routine.settlement())
                    .or_default() += 1;
            }
        }
        request_state.incremental_jobs.clear();
        request_state.queued_since_real_seconds.clear();
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
    let mut pending_ambient_by_settlement = HashMap::<Entity, usize>::new();
    let mut oldest_committed_wait_seconds = 0.0_f64;
    for (entity, _, _, _, _, intent, ambient, objective) in movers.iter_mut() {
        let base_priority = route_request_priority(intent, ambient.is_some(), objective);
        let queued_since = request_state
            .queued_since_real_seconds
            .entry(entity)
            .or_insert(now);
        let age = (now - *queued_since).max(0.0);
        // Age only committed/leisure work. Optional ambience stays optional;
        // otherwise a saturated cosmetic queue would eventually promote
        // itself above the very simulation work it was bounded to protect.
        let priority = if base_priority == ROUTE_PRIORITY_COMMITTED && age >= 4.0 {
            ROUTE_PRIORITY_ESSENTIAL
        } else if base_priority == ROUTE_PRIORITY_LEISURE && age >= 8.0 {
            ROUTE_PRIORITY_COMMITTED
        } else {
            base_priority
        };
        if priority <= ROUTE_PRIORITY_COMMITTED {
            oldest_committed_wait_seconds = oldest_committed_wait_seconds.max(age);
        }
        request_buckets[priority].push(entity);
        if base_priority == ROUTE_PRIORITY_AMBIENT {
            if let Some(routine) = ambient {
                *pending_ambient_by_settlement
                    .entry(routine.settlement())
                    .or_default() += 1;
            }
        }
    }
    let active_requests: HashSet<_> = request_buckets
        .iter()
        .flat_map(|bucket| bucket.iter().copied())
        .collect();
    request_state
        .incremental_jobs
        .retain(|entity, _| active_requests.contains(entity));
    request_state
        .queued_since_real_seconds
        .retain(|entity, _| active_requests.contains(entity));
    let request_count = active_requests.len();
    telemetry.pending_peak = telemetry.pending_peak.max(request_count);
    for (priority, bucket) in request_buckets.iter().enumerate() {
        telemetry.pending_peak_by_priority[priority] =
            telemetry.pending_peak_by_priority[priority].max(bucket.len());
    }
    let committed_pressure = request_buckets[..=ROUTE_PRIORITY_COMMITTED]
        .iter()
        .map(Vec::len)
        .sum();
    let planner_duration =
        budget.max_duration_for_pressure(committed_pressure, oldest_committed_wait_seconds);
    telemetry.planner_budget_peak = telemetry.planner_budget_peak.max(planner_duration);
    telemetry.oldest_committed_wait_seconds = telemetry
        .oldest_committed_wait_seconds
        .max(oldest_committed_wait_seconds);
    if let Some(load) = aux.navigation_load.as_deref_mut() {
        load.pending_total = request_count;
        load.ambient_by_settlement = pending_ambient_by_settlement;
        for routine in aux.active_ambient_routes.iter() {
            *load
                .ambient_by_settlement
                .entry(routine.settlement())
                .or_default() += 1;
        }
    }
    let mut request_order = Vec::with_capacity(request_count);
    for (priority, bucket) in request_buckets.iter_mut().enumerate() {
        rotate_request_bucket_after(
            bucket,
            request_state.last_request_bits_by_priority[priority],
        );
    }
    let mut next_by_priority = [0usize; ROUTE_PRIORITY_COUNT];
    // A single difficult survey can consume the complete wall-clock budget.
    // Guarantee the first slot to real simulation work whenever any exists;
    // weighted round-robin then lets cache hits and remaining headroom animate
    // leisure and ambience. Without this slot, an optional stroll could own
    // an entire tick while an already-waiting porter made no progress.
    if let Some((schedule_index, priority, entity)) =
        first_committed_schedule_request(&request_buckets, request_state.schedule_slot)
    {
        request_order.push((priority, entity));
        next_by_priority[priority] = 1;
        request_state.schedule_slot = (schedule_index + 1) % ROUTE_PRIORITY_SCHEDULE.len();
    }
    while request_order.len() < request_count {
        let mut emitted = false;
        for offset in 0..ROUTE_PRIORITY_SCHEDULE.len() {
            let schedule_index =
                (request_state.schedule_slot + offset) % ROUTE_PRIORITY_SCHEDULE.len();
            let priority = ROUTE_PRIORITY_SCHEDULE[schedule_index];
            if let Some(&entity) = request_buckets[priority].get(next_by_priority[priority]) {
                next_by_priority[priority] += 1;
                request_order.push((priority, entity));
                emitted = true;
            }
        }
        if !emitted {
            break;
        }
        request_state.schedule_slot =
            (request_state.schedule_slot + 1) % ROUTE_PRIORITY_SCHEDULE.len();
    }
    let mut processed = 0usize;
    'requests: for (priority, entity) in request_order {
        let Ok((entity, position, target, mut pending, route_backoff, _intent, _, objective)) =
            movers.get_mut(entity)
        else {
            continue;
        };
        let local_distance = position.0.distance(target.0);
        let survey_max_nodes = agent_survey_max_nodes(local_distance, objective);
        if pending.goal.distance_squared(target.0) > 0.01 {
            *pending = NavigationRoutePending::new(target.0);
            // A changed destination is a new request: reset its wait age so a
            // goal-churning NPC cannot ratchet into the essential class and
            // pin the emergency planner budget open with fresh requests.
            request_state.queued_since_real_seconds.insert(entity, now);
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
            || (processed > 0 && planner_started.elapsed() >= planner_duration)
        {
            // This request has not had its turn yet; begin with it next tick.
            telemetry.budget_yields = telemetry.budget_yields.saturating_add(1);
            break;
        }
        processed += 1;
        request_state.last_request_bits_by_priority[priority] = Some(entity.to_bits());
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
            let versions_are_current = job.geometry_version == geometry_version
                && job.road_opportunity_version == road_opportunity_version;
            let job_is_current = job.target.distance_squared(target.0) <= 0.01
                && incremental_versions_compatible(
                    job.allow_geometry_drift,
                    job.geometry_version,
                    geometry_version,
                    job.road_opportunity_version,
                    road_opportunity_version,
                )
                && job
                    .start
                    .actual
                    .distance_squared(Vec2::new(position.0.x, position.0.z))
                    <= 0.01;
            if job_is_current {
                let direct_started = Instant::now();
                let ordinary_deadline = planner_started + planner_duration;
                let resume_deadline = if job.allow_geometry_drift {
                    ordinary_deadline.max(Instant::now() + RETAINED_INCREMENTAL_SLICE)
                } else {
                    ordinary_deadline
                };
                let result = resume_incremental_route(
                    &terrain,
                    &building_cache,
                    &mut job,
                    resume_deadline,
                    &mut telemetry,
                );
                telemetry.direct_survey_time += direct_started.elapsed();
                match result {
                    IncrementalRouteResult::Pending => {
                        let used_reserved_slice = job.allow_geometry_drift;
                        request_state.incremental_jobs.insert(entity, job);
                        telemetry.budget_yields = telemetry.budget_yields.saturating_add(1);
                        // Retain this frontier, but advance the priority
                        // cursor. Pinning the cursor here can starve every
                        // worker behind a difficult route if preparation has
                        // already consumed this tick's time budget and the
                        // retained search repeatedly gets only its minimum
                        // minimum slice.
                        if used_reserved_slice {
                            // The retained job owns its small reserved slice, not
                            // the rest of the planner. Continue so another request
                            // can use any part of the normal budget that remains.
                            continue;
                        }
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
                            geometry_version,
                        ) {
                            if job.allow_geometry_drift && !versions_are_current {
                                // The retained frontier crossed geometry that
                                // changed while it was being solved. Its final
                                // live-world proof correctly rejected it; keep
                                // the authoritative request pending so the
                                // next slice starts from the current snapshot.
                                *pending = NavigationRoutePending::new(target.0);
                            } else {
                                telemetry.failed_attempts =
                                    telemetry.failed_attempts.saturating_add(1);
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
                        }
                        continue;
                    }
                    IncrementalRouteResult::Failed => {
                        if job.allow_geometry_drift && !versions_are_current {
                            *pending = NavigationRoutePending::new(target.0);
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
                        }
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
                geometry_version,
            );
            continue;
        }
        telemetry.cache_misses = telemetry.cache_misses.saturating_add(1);

        // Scatter deterministic collidable props once for this order, covering
        // the direct path and every possible road connector.
        let prop_started = Instant::now();
        // A newcomer may have several kilometres of inland travel after a
        // coastal landing. Give that committed migration the same bounded,
        // coarse, incremental corridor mechanics as a caravan, while keeping
        // it in the ordinary committed priority bucket so a wave cannot starve
        // established work. Local migration retains the cheap town planner.
        let intersettlement_route = needs_regional_corridor(objective, local_distance);
        let prop_blockers = blockers_for_agent_route(
            &terrain,
            start.survey,
            goal.survey,
            &building_cache.spatial,
            derived,
            colliders,
            if intersettlement_route {
                INTERSETTLEMENT_SURVEY_PADDING
            } else {
                ROAD_ROUTE_JOIN_DISTANCE
            },
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

        // A commute wave is one shared corridor, not hundreds of unrelated
        // route searches. Reuse a certified approach to an exact hall,
        // workplace, home or delivery door when this actor can cheaply join
        // it from nearby clear ground. Optional ambient/leisure targets are
        // excluded because their many one-off destinations only pollute the
        // small approach cache.
        if priority <= ROUTE_PRIORITY_COMMITTED
            && !start.escaping_building
            && !start_apron.is_empty()
            && !goal_apron.is_empty()
        {
            let candidates = graph.cohort_route_candidates(start.actual, goal.actual);
            for candidate in candidates
                .into_iter()
                .take(COHORT_ROUTE_CANDIDATES_TO_SURVEY)
            {
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
                    geometry_version,
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
        let start_pool: Vec<_> = graph
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
        let goal_pool: Vec<_> = graph
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
        let mut start_candidates = start_pool.clone();
        let mut goal_candidates = goal_pool.clone();
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
        // A road endpoint may legitimately touch the authored building shell,
        // which can split the collision-filtered graph even though the durable
        // street topology remains connected. Before launching a 2,400-cell
        // direct A*, try that topology as a candidate source. This does not
        // weaken collision: the complete composed road is certified below and
        // any legacy street genuinely covered by a building is rejected.
        let mut used_topology_fallback = false;
        let mut topology_start_count = 0usize;
        let mut topology_goal_count = 0usize;
        if road_candidates.is_empty() {
            let goal_components: HashSet<_> = goal_pool
                .iter()
                .map(|(node, _)| graph.topology_component_of(*node))
                .collect();
            let mut topology_start = start_pool;
            topology_start
                .retain(|(node, _)| goal_components.contains(&graph.topology_component_of(*node)));
            topology_start.truncate(ROAD_ROUTE_CANDIDATES);
            let start_components: HashSet<_> = topology_start
                .iter()
                .map(|(node, _)| graph.topology_component_of(*node))
                .collect();
            let mut topology_goal = goal_pool;
            topology_goal
                .retain(|(node, _)| start_components.contains(&graph.topology_component_of(*node)));
            topology_goal.truncate(ROAD_ROUTE_CANDIDATES);
            topology_start_count = topology_start.len();
            topology_goal_count = topology_goal.len();
            for &(start_node, start_distance) in &topology_start {
                for &(goal_node, goal_distance) in &topology_goal {
                    if start_node == goal_node
                        || !graph.same_topology_component(start_node, goal_node)
                    {
                        continue;
                    }
                    let Some(nodes) = graph.shortest_topology_path(start_node, goal_node) else {
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
                        estimate: start_distance
                            + goal_distance
                            + road_length / ROAD_SPEED_MULTIPLIER,
                    });
                }
            }
            used_topology_fallback = !road_candidates.is_empty();
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
                    "LAB route diagnostic entity={entity:?} from={:.1},{:.1} to={:.1},{:.1} distance={local_distance:.1} graph_nodes={} start_candidates={} goal_candidates={} topology_start={} topology_goal={} topology_used={} road_pairs={road_candidate_count} attempted={connector_attempts} connector_failures={connector_failures} certification_failures={road_certification_failures}",
                    start.survey.x,
                    start.survey.y,
                    goal.survey.x,
                    goal.survey.y,
                    graph.nodes.len(),
                    start_candidates.len(),
                    goal_candidates.len(),
                    topology_start_count,
                    topology_goal_count,
                    used_topology_fallback,
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
                    max_nodes: survey_max_nodes,
                    scratch: SurveyScratch::default(),
                    search: SurveySearchState::default(),
                    geometry_version,
                    road_opportunity_version,
                    // Every completed route receives a final live-world
                    // certification. Retaining the snapshot across unrelated
                    // building, road or prop revisions prevents a growing
                    // town from perpetually restarting long porter/work trips.
                    allow_geometry_drift: true,
                    regional_corridor: intersettlement_route,
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
            geometry_version,
        );
    }
    telemetry.finish_invocation(planner_started);
    telemetry.maybe_report(&graph);
}

#[cfg(test)]
mod local_tests {
    use super::*;

    #[test]
    fn embodied_caravan_gets_a_bounded_intersettlement_search() {
        assert_eq!(
            agent_survey_max_nodes(
                600.0,
                Some(&CharacterObjective::HaulingInterSettlementCargo),
            ),
            INTERSETTLEMENT_TRADE_SURVEY_MAX_NODES,
        );
        let terrain = WorldTerrain::default();
        let props = PropBlockers::default();
        let survey = RoadSurvey {
            terrain: &terrain,
            buildings: &[],
            live_buildings: None,
            props: &props,
            start: Vec2::ZERO,
            goal: Vec2::new(600.0, 0.0),
            min: Vec2::splat(-SURVEY_PADDING),
            max: Vec2::new(600.0 + SURVEY_PADDING, SURVEY_PADDING),
            max_nodes: INTERSETTLEMENT_TRADE_SURVEY_MAX_NODES,
            cell_size: SURVEY_CELL,
            coarse_stride: INTERSETTLEMENT_SURVEY_STRIDE,
            fine_endpoint_radius: INTERSETTLEMENT_FINE_ENDPOINT_RADIUS,
        };
        assert_eq!(survey.stride_at(survey.start), 1);
        assert_eq!(survey.stride_at(Vec2::new(300.0, 0.0)), 4);
        assert_eq!(survey.stride_at(survey.goal), 1);
        assert_eq!(
            agent_survey_max_nodes(600.0, Some(&CharacterObjective::ReturningFromTradeRoute)),
            INTERSETTLEMENT_TRADE_SURVEY_MAX_NODES,
            "the empty return wagon still needs the inter-settlement corridor",
        );
        assert_eq!(
            agent_survey_max_nodes(600.0, Some(&CharacterObjective::GoingToFarm)),
            AGENT_SURVEY_MAX_NODES,
            "ordinary villagers must not inherit the expensive caravan budget",
        );
        assert_eq!(
            agent_survey_max_nodes(600.0, Some(&CharacterObjective::TravellingToSettlement),),
            INTERSETTLEMENT_TRADE_SURVEY_MAX_NODES,
            "a coastal immigrant must not be stranded by the local commute cap",
        );
        assert_eq!(
            agent_survey_max_nodes(200.0, Some(&CharacterObjective::TravellingToSettlement),),
            INTERSETTLEMENT_TRADE_SURVEY_MAX_NODES,
            "a mid-distance landfall walk must plan with the same tier that \
             certified the approach, or a headland detour beyond the local \
             window strands the immigrant at the coast",
        );
        assert_eq!(
            agent_survey_max_nodes(30.0, Some(&CharacterObjective::TravellingToSettlement),),
            AGENT_SURVEY_MAX_NODES,
            "a migrant already at the hall's doorstep keeps the cheap local search",
        );
        assert_eq!(
            route_request_priority(
                Some(&VillagerIntent::Travelling {
                    settlement: Entity::from_bits(1),
                }),
                false,
                Some(&CharacterObjective::TravellingToSettlement),
            ),
            ROUTE_PRIORITY_COMMITTED,
            "a migration wave must not outrank every established work journey",
        );
    }

    #[test]
    fn retained_frontier_can_survive_unrelated_world_growth() {
        assert!(incremental_versions_compatible(true, 10, 11, 20, 21,));
        assert!(incremental_versions_compatible(false, 10, 10, 20, 20,));
        assert!(
            !incremental_versions_compatible(false, 10, 11, 20, 20),
            "strict snapshot jobs must restart when their collision snapshot changes",
        );
        assert_eq!(
            route_request_priority(
                None,
                false,
                Some(&CharacterObjective::HaulingInterSettlementCargo),
            ),
            ROUTE_PRIORITY_CARAVAN,
        );
        assert_eq!(
            route_request_priority(
                None,
                false,
                Some(&CharacterObjective::ReturningFromTradeRoute),
            ),
            ROUTE_PRIORITY_CARAVAN,
        );
    }

    #[test]
    fn a_work_objective_overrides_a_stale_ambient_marker() {
        let resident = VillagerIntent::Resident {
            settlement: Entity::PLACEHOLDER,
        };
        assert_eq!(
            route_request_priority(
                Some(&resident),
                true,
                Some(&CharacterObjective::GoingToLumberWork),
            ),
            ROUTE_PRIORITY_COMMITTED,
        );
        assert_eq!(
            route_request_priority(
                Some(&resident),
                true,
                Some(&CharacterObjective::WalkingAroundTown),
            ),
            ROUTE_PRIORITY_AMBIENT,
        );
        assert_eq!(
            route_request_priority(
                Some(&resident),
                false,
                Some(&CharacterObjective::CollectingMarketGoods),
            ),
            ROUTE_PRIORITY_ESSENTIAL,
            "an empty porter going to collect cargo is still essential logistics",
        );
        assert_eq!(
            route_request_priority(
                Some(&resident),
                false,
                Some(&CharacterObjective::CollectingCompanyInputs),
            ),
            ROUTE_PRIORITY_ESSENTIAL,
        );
    }

    #[test]
    fn request_cursor_survives_membership_changes_without_skipping_the_successor() {
        let one = Entity::from_bits(1);
        let two = Entity::from_bits(2);
        let three = Entity::from_bits(3);
        let four = Entity::from_bits(4);

        let mut first = vec![four, one, three, two];
        rotate_request_bucket_after(&mut first, Some(two.to_bits()));
        assert_eq!(first, vec![three, four, one, two]);

        // Entity #3 remains the next request even though an older entity was
        // removed and a new one appeared before the cursor in sorted order.
        let mut changed = vec![four, one, three];
        rotate_request_bucket_after(&mut changed, Some(two.to_bits()));
        assert_eq!(changed, vec![three, four, one]);
    }

    #[test]
    fn committed_work_owns_the_first_planner_slot_even_when_ambient_is_scheduled() {
        let committed = Entity::from_bits(1);
        let ambient = Entity::from_bits(2);
        let mut buckets: [Vec<Entity>; ROUTE_PRIORITY_COUNT] = std::array::from_fn(|_| Vec::new());
        buckets[ROUTE_PRIORITY_COMMITTED].push(committed);
        buckets[ROUTE_PRIORITY_AMBIENT].push(ambient);

        let ambient_schedule_slot = ROUTE_PRIORITY_SCHEDULE
            .iter()
            .position(|priority| *priority == ROUTE_PRIORITY_AMBIENT)
            .unwrap();
        let (_, priority, entity) =
            first_committed_schedule_request(&buckets, ambient_schedule_slot).unwrap();

        assert_eq!(priority, ROUTE_PRIORITY_COMMITTED);
        assert_eq!(entity, committed);
    }

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

        assert!(start_candidates.iter().take(4).all(|(start, _)| {
            goal_candidates
                .iter()
                .all(|(goal, _)| !graph.same_component(*start, *goal))
        }));
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
