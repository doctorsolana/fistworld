//! Retained, terrain-versioned water planning. Every runtime caller yields work.
use super::clearance::{depth_clear, WaterNavigationGeometry, WatercraftClearance, SAMPLE_STEP};
use super::*;
use std::sync::Arc;
use std::time::{Duration, Instant};

mod terrain_watch;
pub(crate) use terrain_watch::TerrainDependencies;

const COARSE_CELL: f32 = 24.0;
const COARSE_ROUTE_DISTANCE: f32 = 192.0;

const SLICE: Duration = Duration::from_micros(500);
// The wall-clock deadline bounds tick cost. The independent unit ceiling
// still guarantees a yield when many cached probes are cheap. The seed-91
// cold-route probe exhausted 8,192 units after only ~193 us in an optimized
// build, wasting most of its existing 500 us slice. This ceiling lets fast
// probes use that same deadline without enlarging it or the search bounds.
const SAMPLE_BUDGET: usize = 32_768;
const CACHE_ROUTES: usize = 64;
const MAX_ACTIVE_SEARCHES: usize = 4;
const CACHE_WAYPOINTS: usize = 2_048;
const SIMPLIFY_LOOKAHEAD: usize = 32;

#[derive(Debug)]
pub(crate) enum WaterPlanResult {
    Pending,
    Complete(Option<Vec<Vec2>>),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct RouteKey([u32; 7]);
impl RouteKey {
    fn new(start: Vec2, goal: Vec2, hull: WatercraftClearance) -> Self {
        let [radius, draft, air] = hull.key();
        Self([
            start.x.to_bits(),
            start.y.to_bits(),
            goal.x.to_bits(),
            goal.y.to_bits(),
            radius,
            draft,
            air,
        ])
    }

    fn start(self) -> Vec2 {
        Vec2::new(f32::from_bits(self.0[0]), f32::from_bits(self.0[1]))
    }

    fn same_goal_and_hull(self, other: Self) -> bool {
        self.0[2..] == other.0[2..]
    }
}

#[derive(Debug)]
struct CachedWaterRoute {
    key: RouteKey,
    certified_revision: (u64, u32, u64),
    route: Option<Vec<Vec2>>,
}

#[derive(Default, Debug)]
pub(crate) struct WaterRouteCache {
    version: Option<(u64, u32, u64)>,
    pub(crate) geometry: WaterNavigationGeometry,
    entries: VecDeque<CachedWaterRoute>,
}
impl WaterRouteCache {
    pub(crate) fn begin(&mut self, terrain: &WorldTerrain, start: Vec2, goal: Vec2) -> WaterSearch {
        self.begin_for(terrain, start, goal, WatercraftClearance::DINGHY)
    }

    pub(crate) fn begin_for(
        &mut self,
        terrain: &WorldTerrain,
        start: Vec2,
        goal: Vec2,
        hull: WatercraftClearance,
    ) -> WaterSearch {
        self.reconcile(terrain);
        let revision = self.geometry.revision(terrain);
        let key = RouteKey::new(start, goal, hull);
        if let Some(index) = self
            .entries
            .iter()
            .position(|stored| stored.key == key && stored.certified_revision == revision)
        {
            let cached = self.entries.remove(index).unwrap();
            let result = cached.route.clone();
            self.entries.push_back(cached);
            return WaterSearch {
                version: Some(revision),
                phase: Phase::Ready(result),
                ..WaterSearch::with_clearance(start, goal, hull)
            };
        }
        // One recent proposal bounds this admission work to 64 keys and at
        // most 2,049 points. Its geometry is not trusted: even a warm same-goal
        // route must freshly certify the new connector and its entire suffix.
        if let Some(index) = self
            .entries
            .iter()
            .rposition(|stored| stored.key.same_goal_and_hull(key) && stored.route.is_some())
        {
            let cached = self.entries.remove(index).unwrap();
            let route = cached.route.as_deref().unwrap();
            let nearest = std::iter::once(cached.key.start())
                .chain(route.iter().copied())
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    a.distance_squared(start)
                        .total_cmp(&b.distance_squared(start))
                })
                .map(|(index, _)| index)
                .unwrap();
            let proposal = if nearest == 0 {
                std::iter::once(cached.key.start())
                    .chain(route.iter().copied())
                    .collect()
            } else {
                route[nearest - 1..].to_vec()
            };
            self.entries.push_back(cached);
            let mut search = WaterSearch::with_clearance(start, goal, hull);
            search.version = Some(revision);
            search.begin_certification(proposal, terrain);
            return search;
        }
        WaterSearch::with_clearance(start, goal, hull)
    }

    fn reconcile(&mut self, terrain: &WorldTerrain) {
        let revision = self.geometry.revision(terrain);
        if self.version != Some(revision) {
            if self.version.is_some_and(|old| old.0 != revision.0) {
                self.entries.clear();
            } else {
                // Successful routes remain useful geometric proposals through
                // sparse edits. Old failures cannot veto newly opened water.
                self.entries.retain(|entry| entry.route.is_some());
            }
            self.version = Some(revision);
        }
    }

    pub(crate) fn advance(
        &mut self,
        search: &mut WaterSearch,
        terrain: &WorldTerrain,
    ) -> WaterPlanResult {
        self.reconcile(terrain);
        let result = search.advance_in(terrain, &self.geometry);
        if let WaterPlanResult::Complete(route) = &result {
            let key = RouteKey::new(search.start, search.goal, search.hull);
            if route
                .as_ref()
                .is_none_or(|points| points.len() <= CACHE_WAYPOINTS)
            {
                self.entries.retain(|stored| stored.key != key);
                self.entries.push_back(CachedWaterRoute {
                    key,
                    certified_revision: self.geometry.revision(terrain),
                    route: route.clone(),
                });
                while self.entries.len() > CACHE_ROUTES {
                    self.entries.pop_front();
                }
            }
        }
        result
    }
}

/// Every depth probe, including A* edges and exact endpoint connectors, yields.
#[derive(Debug)]
struct Segment {
    start: Vec2,
    end: Vec2,
    samples: usize,
    next: usize,
    probe: usize,
}
impl Segment {
    fn new(start: Vec2, end: Vec2) -> Self {
        Self {
            start,
            end,
            samples: (start.distance(end) / SAMPLE_STEP).ceil().max(1.) as usize,
            next: 0,
            probe: 0,
        }
    }
    fn advance(
        &mut self,
        terrain: &WorldTerrain,
        geometry: &WaterNavigationGeometry,
        hull: WatercraftClearance,
        offsets: &[Vec2],
        known: &mut HashMap<[u32; 2], bool>,
        terrain_reads: &mut TerrainDependencies,
        budget: &mut usize,
        until: Instant,
    ) -> Option<bool> {
        while *budget > 0 && Instant::now() < until {
            let point = self
                .start
                .lerp(self.end, self.next as f32 / self.samples as f32);
            let key = [point.x.to_bits(), point.y.to_bits()];
            if self.probe == 0 {
                *budget -= 1;
                terrain_reads.observe(terrain, point);
                if let Some(clear) = known.get(&key).copied() {
                    if !clear {
                        return Some(false);
                    }
                    self.next += 1;
                    if self.next > self.samples {
                        return Some(true);
                    }
                    continue;
                }
                if !water_at(terrain, point)
                    .is_some_and(|water| geometry.structure_clear(point, water, hull))
                {
                    if known.len() < 65_536 {
                        known.insert(key, false);
                    }
                    return Some(false);
                }
                self.probe = 1;
            } else {
                *budget -= 1;
                let sample = point + offsets[self.probe - 1];
                terrain_reads.observe(terrain, sample);
                if !depth_clear(terrain, sample, hull.draft) {
                    if known.len() < 65_536 {
                        known.insert(key, false);
                    }
                    return Some(false);
                }
                self.probe += 1;
                if self.probe > offsets.len() {
                    if known.len() < 65_536 {
                        known.insert(key, true);
                    }
                    self.probe = 0;
                    self.next += 1;
                    if self.next > self.samples {
                        return Some(true);
                    }
                }
            }
        }
        None
    }
}
fn connector_candidates(point: Vec2, cell_size: f32) -> Vec<WaterCell> {
    let center = water_cell(point, cell_size);
    let mut cells: Vec<_> = (-2..=2)
        .flat_map(|x| {
            (-2..=2).map(move |z| WaterCell {
                x: center.x + x,
                z: center.z + z,
            })
        })
        .collect();
    cells.sort_by(|a, b| {
        point
            .distance_squared(water_point(*a, cell_size))
            .total_cmp(&point.distance_squared(water_point(*b, cell_size)))
    });
    cells
}
fn water_cell(point: Vec2, cell_size: f32) -> WaterCell {
    WaterCell {
        x: (point.x / cell_size).round() as i32,
        z: (point.y / cell_size).round() as i32,
    }
}
fn water_point(cell: WaterCell, cell_size: f32) -> Vec2 {
    Vec2::new(cell.x as f32 * cell_size, cell.z as f32 * cell_size)
}
const NEIGHBORS: [(i32, i32); 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

#[derive(Debug)]
enum Phase {
    Validate,
    Direct(Segment),
    Connect {
        start_cell: Option<WaterCell>,
        candidates: Vec<WaterCell>,
        index: usize,
        segment: Segment,
    },
    Search,
    Expand {
        cell: WaterCell,
        cost: f32,
        next: usize,
        segment: Option<Segment>,
    },
    Reconstruct {
        cursor: WaterCell,
        reverse: Vec<Vec2>,
    },
    Simplify {
        raw: Vec<Vec2>,
        route: Vec<Vec2>,
        index: usize,
        candidate: usize,
        segment: Segment,
    },
    /// A changed search is only a proposal. This fresh swept proof owns a
    /// cleared memo and current terrain dependencies before any route is used.
    Certify {
        route: Vec<Vec2>,
        index: usize,
        segment: Segment,
    },
    Ready(Option<Vec<Vec2>>),
}

#[derive(Debug)]
pub(crate) struct WaterSearch {
    pub(crate) start: Vec2,
    goal: Vec2,
    version: Option<(u64, u32, u64)>,
    hull: WatercraftClearance,
    cell_size: f32,
    terrain_reads: TerrainDependencies,
    proposal_stale: bool,
    offsets: Arc<Vec<Vec2>>,
    known: HashMap<[u32; 2], bool>,
    phase: Phase,
    goal_cell: Option<WaterCell>,
    open: BinaryHeap<OpenWaterCell>,
    came_from: HashMap<WaterCell, WaterCell>,
    score: HashMap<WaterCell, f32>,
    closed: HashSet<WaterCell>,
    expanded: usize,
}
impl WaterSearch {
    #[cfg(test)]
    pub(crate) fn new(start: Vec2, goal: Vec2) -> Self {
        Self::with_clearance(start, goal, WatercraftClearance::DINGHY)
    }
    fn with_clearance(start: Vec2, goal: Vec2, hull: WatercraftClearance) -> Self {
        Self {
            start,
            goal,
            version: None,
            hull,
            cell_size: if start.distance(goal) >= COARSE_ROUTE_DISTANCE {
                COARSE_CELL
            } else {
                NAV_CELL
            },
            terrain_reads: default(),
            proposal_stale: false,
            offsets: hull.samples(),
            known: default(),
            phase: Phase::Validate,
            goal_cell: None,
            open: default(),
            came_from: default(),
            score: default(),
            closed: default(),
            expanded: 0,
        }
    }

    pub(crate) fn grid_metres(&self) -> f32 {
        self.cell_size
    }

    /// Compact read-only state for opt-in connected acceptance diagnostics.
    pub(crate) fn progress(&self) -> (&'static str, usize, usize, usize, Option<(u64, u32, u64)>) {
        let phase = match &self.phase {
            Phase::Validate => "validate",
            Phase::Direct(_) => "direct",
            Phase::Connect { .. } => "connect",
            Phase::Search => "search",
            Phase::Expand { .. } => "expand",
            Phase::Reconstruct { .. } => "reconstruct",
            Phase::Simplify { .. } => "simplify",
            Phase::Certify { .. } => "certify",
            Phase::Ready(_) => "ready",
        };
        (
            phase,
            self.expanded,
            self.open.len(),
            self.known.len(),
            self.version,
        )
    }

    #[cfg(test)]
    pub(crate) fn advance(&mut self, terrain: &WorldTerrain) -> WaterPlanResult {
        self.advance_in(terrain, &WaterNavigationGeometry::default())
    }
    fn advance_in(
        &mut self,
        terrain: &WorldTerrain,
        geometry: &WaterNavigationGeometry,
    ) -> WaterPlanResult {
        let revision = geometry.revision(terrain);
        let previous_revision = self.version;
        let map_changed = previous_revision.is_some_and(|old| old.0 != revision.0);
        let terminal_cache_changed = matches!(self.phase, Phase::Ready(_))
            && previous_revision.is_some_and(|old| old != revision);
        let structure_changed = previous_revision.is_some_and(|old| old.2 != revision.2);
        let terrain_changed = self.terrain_reads.changed(terrain);
        if map_changed {
            *self = Self::with_clearance(self.start, self.goal, self.hull);
            self.terrain_reads.changed(terrain);
        } else if terrain_changed || structure_changed || terminal_cache_changed {
            // Sparse edits may invalidate explored dead ends without changing
            // the eventual route. Keep that expensive frontier, but never use
            // its old samples as authority for the route we return.
            self.proposal_stale = true;
            if matches!(self.phase, Phase::Certify { .. }) {
                let Phase::Certify { route, .. } =
                    std::mem::replace(&mut self.phase, Phase::Ready(None))
                else {
                    unreachable!()
                };
                self.begin_certification(route, terrain);
            }
        }
        self.version = Some(revision);
        let certifying = matches!(self.phase, Phase::Certify { .. });
        let result = self.advance_slice(terrain, geometry);
        match result {
            WaterPlanResult::Complete(Some(route)) if self.proposal_stale => {
                self.begin_certification(route, terrain);
                WaterPlanResult::Pending
            }
            WaterPlanResult::Complete(None) if self.proposal_stale || certifying => {
                // An obsolete rejection is no more authoritative than an old
                // successful edge. Retry on current terrain, retaining a fine
                // fallback's resolution instead of repeating the coarse pass.
                self.restart_current_grid(terrain, revision);
                WaterPlanResult::Pending
            }
            WaterPlanResult::Complete(None) if self.cell_size > NAV_CELL => {
                // A coarse grid may miss a narrow navigable channel. Keep the
                // exact endpoint/hull rules and retry on the original grid.
                self.cell_size = NAV_CELL;
                self.restart_current_grid(terrain, revision);
                WaterPlanResult::Pending
            }
            result => result,
        }
    }

    fn restart_current_grid(&mut self, terrain: &WorldTerrain, revision: (u64, u32, u64)) {
        let cell_size = self.cell_size;
        *self = Self {
            cell_size,
            ..Self::with_clearance(self.start, self.goal, self.hull)
        };
        self.version = Some(revision);
        self.terrain_reads.changed(terrain);
    }

    fn begin_certification(&mut self, route: Vec<Vec2>, terrain: &WorldTerrain) {
        self.known.clear();
        self.terrain_reads = TerrainDependencies::default();
        self.terrain_reads.changed(terrain);
        self.proposal_stale = false;
        self.phase = match route.first().copied() {
            Some(first) => Phase::Certify {
                route,
                index: 0,
                segment: Segment::new(self.start, first),
            },
            None => Phase::Ready(None),
        };
    }

    fn advance_slice(
        &mut self,
        terrain: &WorldTerrain,
        geometry: &WaterNavigationGeometry,
    ) -> WaterPlanResult {
        let until = Instant::now() + SLICE;
        let mut budget = SAMPLE_BUDGET;
        while budget > 0 && Instant::now() < until {
            let phase = std::mem::replace(&mut self.phase, Phase::Ready(None));
            match phase {
                Phase::Validate => {
                    self.terrain_reads.observe(terrain, self.start);
                    self.terrain_reads.observe(terrain, self.goal);
                    if !self.start.is_finite()
                        || !self.goal.is_finite()
                        || water_at(terrain, self.start).is_none()
                        || water_at(terrain, self.goal).is_none()
                    {
                        return WaterPlanResult::Complete(None);
                    }
                    self.phase = Phase::Direct(Segment::new(self.start, self.goal));
                }
                Phase::Direct(mut segment) => match segment.advance(
                    terrain,
                    geometry,
                    self.hull,
                    &self.offsets,
                    &mut self.known,
                    &mut self.terrain_reads,
                    &mut budget,
                    until,
                ) {
                    None => self.phase = Phase::Direct(segment),
                    Some(true) => return WaterPlanResult::Complete(Some(vec![self.goal])),
                    Some(false) => {
                        let candidates = connector_candidates(self.start, self.cell_size);
                        let segment =
                            Segment::new(self.start, water_point(candidates[0], self.cell_size));
                        self.phase = Phase::Connect {
                            start_cell: None,
                            candidates,
                            index: 0,
                            segment,
                        };
                    }
                },
                Phase::Connect {
                    start_cell,
                    candidates,
                    mut index,
                    mut segment,
                } => {
                    match segment.advance(
                        terrain,
                        geometry,
                        self.hull,
                        &self.offsets,
                        &mut self.known,
                        &mut self.terrain_reads,
                        &mut budget,
                        until,
                    ) {
                        None => {
                            self.phase = Phase::Connect {
                                start_cell,
                                candidates,
                                index,
                                segment,
                            }
                        }
                        Some(false) => {
                            index += 1;
                            if index == candidates.len() {
                                return WaterPlanResult::Complete(None);
                            }
                            let point = if start_cell.is_some() {
                                self.goal
                            } else {
                                self.start
                            };
                            segment =
                                Segment::new(point, water_point(candidates[index], self.cell_size));
                            self.phase = Phase::Connect {
                                start_cell,
                                candidates,
                                index,
                                segment,
                            };
                        }
                        Some(true) => {
                            if let Some(start) = start_cell {
                                let goal = candidates[index];
                                self.goal_cell = Some(goal);
                                self.score.insert(start, 0.);
                                self.open.push(OpenWaterCell {
                                    cost: (water_heuristic(start, goal) * 1000.) as i32,
                                    cell: start,
                                });
                                self.phase = Phase::Search;
                            } else {
                                let start_cell = Some(candidates[index]);
                                let candidates = connector_candidates(self.goal, self.cell_size);
                                let segment = Segment::new(
                                    self.goal,
                                    water_point(candidates[0], self.cell_size),
                                );
                                self.phase = Phase::Connect {
                                    start_cell,
                                    candidates,
                                    index: 0,
                                    segment,
                                };
                            }
                        }
                    }
                }
                Phase::Search => {
                    let Some(OpenWaterCell { cell, .. }) = self.open.pop() else {
                        return WaterPlanResult::Complete(None);
                    };
                    budget -= 1; // Heap duplicates count toward admission work as well.
                    self.phase = Phase::Search;
                    if !self.closed.insert(cell) {
                        continue;
                    }
                    self.expanded += 1;
                    // Both resolutions share the same bounded frontier cap.
                    // Dividing it by the cell-area ratio prematurely rejected
                    // long ocean detours, then repeated the larger search on
                    // the fine grid. Time slicing already limits per-tick cost.
                    if self.expanded > NAV_MAX_EXPANDED {
                        return WaterPlanResult::Complete(None);
                    }
                    let goal = self.goal_cell.unwrap();
                    if cell == goal {
                        self.phase = Phase::Reconstruct {
                            cursor: cell,
                            reverse: vec![self.goal],
                        };
                        continue;
                    }
                    self.phase = Phase::Expand {
                        cell,
                        cost: self.score[&cell],
                        next: 0,
                        segment: None,
                    };
                }
                Phase::Expand {
                    cell,
                    cost,
                    mut next,
                    mut segment,
                } => {
                    if next == NEIGHBORS.len() {
                        self.phase = Phase::Search;
                        continue;
                    }
                    let (dx, dz) = NEIGHBORS[next];
                    let target = WaterCell {
                        x: cell.x + dx,
                        z: cell.z + dz,
                    };
                    let tentative = cost
                        + if dx != 0 && dz != 0 {
                            std::f32::consts::SQRT_2
                        } else {
                            1.
                        };
                    if self.closed.contains(&target)
                        || tentative >= self.score.get(&target).copied().unwrap_or(f32::INFINITY)
                    {
                        budget -= 1;
                        next += 1;
                        segment = None;
                    } else {
                        let edge = segment.get_or_insert_with(|| {
                            Segment::new(
                                water_point(cell, self.cell_size),
                                water_point(target, self.cell_size),
                            )
                        });
                        match edge.advance(
                            terrain,
                            geometry,
                            self.hull,
                            &self.offsets,
                            &mut self.known,
                            &mut self.terrain_reads,
                            &mut budget,
                            until,
                        ) {
                            None => {}
                            Some(clear) => {
                                if clear {
                                    self.came_from.insert(target, cell);
                                    self.score.insert(target, tentative);
                                    self.open.push(OpenWaterCell {
                                        cost: ((tentative
                                            + water_heuristic(target, self.goal_cell.unwrap()))
                                            * 1000.)
                                            as i32,
                                        cell: target,
                                    });
                                }
                                next += 1;
                                segment = None;
                            }
                        }
                    }
                    self.phase = Phase::Expand {
                        cell,
                        cost,
                        next,
                        segment,
                    };
                }
                Phase::Reconstruct {
                    cursor,
                    mut reverse,
                } => {
                    budget -= 1;
                    reverse.push(water_point(cursor, self.cell_size));
                    if let Some(previous) = self.came_from.get(&cursor).copied() {
                        self.phase = Phase::Reconstruct {
                            cursor: previous,
                            reverse,
                        };
                    } else {
                        // Include the start cell: only this connector was certified
                        // against the exact shoreline start. Its neighbour may be occluded.
                        reverse.reverse();
                        let candidate = (reverse.len() - 1).min(SIMPLIFY_LOOKAHEAD);
                        let segment = Segment::new(self.start, reverse[candidate]);
                        self.phase = Phase::Simplify {
                            raw: reverse,
                            route: Vec::new(),
                            index: 0,
                            candidate,
                            segment,
                        };
                    }
                }
                Phase::Simplify {
                    raw,
                    mut route,
                    mut index,
                    mut candidate,
                    mut segment,
                } => {
                    match segment.advance(
                        terrain,
                        geometry,
                        self.hull,
                        &self.offsets,
                        &mut self.known,
                        &mut self.terrain_reads,
                        &mut budget,
                        until,
                    ) {
                        None => {}
                        Some(false) if candidate > index => {
                            candidate -= 1;
                            segment = Segment::new(
                                route.last().copied().unwrap_or(self.start),
                                raw[candidate],
                            );
                        }
                        Some(false) => return WaterPlanResult::Complete(None),
                        Some(true) => {
                            let anchor = raw[candidate];
                            route.push(anchor);
                            index = candidate + 1;
                            if index >= raw.len() {
                                return WaterPlanResult::Complete(Some(route));
                            }
                            candidate = (index + SIMPLIFY_LOOKAHEAD).min(raw.len() - 1);
                            segment = Segment::new(anchor, raw[candidate]);
                        }
                    }
                    self.phase = Phase::Simplify {
                        raw,
                        route,
                        index,
                        candidate,
                        segment,
                    };
                }
                Phase::Certify {
                    route,
                    mut index,
                    mut segment,
                } => {
                    match segment.advance(
                        terrain,
                        geometry,
                        self.hull,
                        &self.offsets,
                        &mut self.known,
                        &mut self.terrain_reads,
                        &mut budget,
                        until,
                    ) {
                        None => {}
                        Some(false) => return WaterPlanResult::Complete(None),
                        Some(true) => {
                            index += 1;
                            if index == route.len() {
                                return WaterPlanResult::Complete(Some(route));
                            }
                            segment = Segment::new(route[index - 1], route[index]);
                        }
                    }
                    self.phase = Phase::Certify {
                        route,
                        index,
                        segment,
                    };
                }
                Phase::Ready(route) => return WaterPlanResult::Complete(route),
            }
        }
        WaterPlanResult::Pending
    }
}

/// Coast discovery preserves its cursor and tries at most four real water routes.
#[derive(Debug)]
struct LandingSearch {
    start: Vec2,
    click: Vec2,
    direction: Vec2,
    distance: f32,
    steps: usize,
    next: usize,
    last_dry: Option<Vec2>,
    attempts: usize,
    route: Option<(Vec2, Vec2, WaterSearch)>,
}
impl LandingSearch {
    fn new(start: Vec2, click: Vec2) -> Self {
        let delta = start - click;
        let distance = delta.length();
        Self {
            start,
            click,
            direction: delta.normalize_or_zero(),
            distance,
            steps: (distance / LANDING_SCAN_STEP).ceil() as usize,
            next: 0,
            last_dry: None,
            attempts: 0,
            route: None,
        }
    }

    fn advance(
        &mut self,
        terrain: &WorldTerrain,
        cache: &mut WaterRouteCache,
    ) -> Option<Option<(Vec2, Vec2, Vec<Vec2>)>> {
        if !self.start.is_finite()
            || !self.click.is_finite()
            || !self.distance.is_finite()
            || self.distance < 1.0e-3
        {
            return Some(None);
        }
        if let Some((mooring, landing, search)) = &mut self.route {
            return match cache.advance(search, terrain) {
                WaterPlanResult::Pending => None,
                WaterPlanResult::Complete(Some(route)) => Some(Some((*mooring, *landing, route))),
                WaterPlanResult::Complete(None) => {
                    self.route = None;
                    self.last_dry = None;
                    None
                }
            };
        }
        if self.attempts >= LANDING_ROUTE_ATTEMPTS {
            return Some(None);
        }
        let until = Instant::now() + SLICE;
        let mut samples = 0;
        while self.next <= self.steps && samples < SAMPLE_BUDGET && Instant::now() < until {
            samples += 1;
            let point = self.click
                + self.direction * (self.next as f32 * LANDING_SCAN_STEP).min(self.distance);
            self.next += 1;
            if water_at(terrain, point).is_none() {
                self.last_dry = dry_at(terrain, point).then_some(point);
                continue;
            }
            let Some(landing) = self.last_dry.take() else {
                continue;
            };
            let Some(mooring) = (0..=MAX_DISEMBARK_DISTANCE as usize)
                .map(|i| point + self.direction * i as f32)
                .find(|p| {
                    landing.distance(*p) <= MAX_DISEMBARK_DISTANCE
                        && cache
                            .geometry
                            .point_clear(terrain, *p, WatercraftClearance::DINGHY)
                })
            else {
                continue;
            };
            self.attempts += 1;
            self.route = Some((mooring, landing, cache.begin(terrain, self.start, mooring)));
            return None;
        }
        (self.next > self.steps).then_some(None)
    }
}

#[derive(Debug)]
enum VesselSearch {
    Sail(WaterSearch),
    Land(LandingSearch),
}
#[derive(Debug)]
pub(super) struct ActiveVesselSearch {
    start: Vec2,
    search: VesselSearch,
}

pub fn plan(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    mut queue: ResMut<VesselNavigationQueue>,
    geometry: Option<Res<WaterNavigationGeometry>>,
    vessels: Query<
        (&PlayerPosition, Option<&VesselNavigation>),
        (With<Vessel>, Without<PausedPlayerVoyage>),
    >,
) {
    if let Some(geometry) = geometry {
        queue.cache.geometry = geometry.clone();
    }
    for _ in 0..NAV_ROUTES_PER_TICK {
        // Bound retained frontiers as well as tick work: a fleet must not
        // allocate one 60k-node A* for every hull while waiting its turn.
        let index = if queue.active.len() >= MAX_ACTIVE_SEARCHES {
            queue
                .pending
                .iter()
                .position(|(vessel, _)| queue.active.contains_key(vessel))
        } else {
            (!queue.pending.is_empty()).then_some(0)
        };
        let Some(index) = index else {
            break;
        };
        let (vessel, goal) = queue.pending.remove(index).unwrap();
        let Ok((position, navigation)) = vessels.get(vessel) else {
            queue.active.remove(&vessel);
            continue;
        };
        let mut active = queue.active.remove(&vessel).unwrap_or_else(|| {
            commands
                .entity(vessel)
                .remove::<VesselRoute>()
                .remove::<PendingLanding>()
                .remove::<VesselRouteFailed>()
                .remove::<VesselRouteCertification>()
                .insert(CharacterMotion::STATIONARY);
            let start = position.0.xz();
            let search = match goal {
                VesselGoal::Sail(goal) => VesselSearch::Sail(queue.cache.begin_for(
                    &terrain,
                    start,
                    goal,
                    navigation.unwrap_or(&VesselNavigation::DINGHY).clearance,
                )),
                VesselGoal::Land { click } => VesselSearch::Land(LandingSearch::new(start, click)),
            };
            ActiveVesselSearch { start, search }
        });
        // A teleport/external owner invalidates the authored starting connector.
        if active.start.distance_squared(position.0.xz()) > 0.01 {
            queue.request(vessel, goal);
            continue;
        }
        let result = match &mut active.search {
            VesselSearch::Sail(search) => match queue.cache.advance(search, &terrain) {
                WaterPlanResult::Pending => None,
                WaterPlanResult::Complete(route) => Some(route.map(|route| (route, None))),
            },
            VesselSearch::Land(search) => {
                search.advance(&terrain, &mut queue.cache).map(|result| {
                    result.map(|(mooring, landing, route)| {
                        (
                            route,
                            Some(PendingLanding {
                                mooring,
                                landing,
                                walk_to: search.click,
                            }),
                        )
                    })
                })
            }
        };
        match result {
            None => {
                queue.active.insert(vessel, active);
                queue.pending.push_back((vessel, goal));
            }
            Some(Some((waypoints, landing))) => {
                let proof = VesselRouteCertification::new(
                    &terrain,
                    queue.cache.geometry.revision(&terrain),
                    navigation.unwrap_or(&VesselNavigation::DINGHY).clearance,
                    position.0.xz(),
                    &waypoints,
                );
                commands
                    .entity(vessel)
                    .insert((VesselRoute { waypoints, next: 0 }, proof));
                if let Some(landing) = landing {
                    commands.entity(vessel).insert(landing);
                }
            }
            Some(None) => {
                let destination = match goal {
                    VesselGoal::Sail(p) => p,
                    VesselGoal::Land { click } => click,
                };
                commands
                    .entity(vessel)
                    .insert(VesselRouteFailed { goal: destination });
            }
        }
    }
}

#[cfg(test)]
pub(super) fn complete_route(terrain: &WorldTerrain, start: Vec2, goal: Vec2) -> Option<Vec<Vec2>> {
    let mut search = WaterSearch::new(start, goal);
    loop {
        if let WaterPlanResult::Complete(route) = search.advance(terrain) {
            return route;
        }
    }
}

#[cfg(test)]
pub(super) fn complete_landing(
    terrain: &WorldTerrain,
    start: Vec2,
    click: Vec2,
) -> Option<(Vec2, Vec2, Vec<Vec2>)> {
    let mut search = LandingSearch::new(start, click);
    let mut cache = WaterRouteCache::default();
    loop {
        if let Some(result) = search.advance(terrain, &mut cache) {
            return result;
        }
    }
}

#[cfg(test)]
mod tests;
