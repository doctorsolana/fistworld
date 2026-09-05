//! Geometric proof that an approved plot has a dry, unobstructed road connector.

#[cfg(test)]
use super::demand::processing_upstream_is_complete;
#[cfg(test)]
use super::fishing::{advance_incremental_fishing_search, find_incremental_fishing_site};
#[cfg(test)]
use super::manual::validate_manual_plot;
#[cfg(test)]
use super::plots::{include_resumable_search_cursor, MAX_SETTLEMENT_SEARCH_RADIUS};
use crate::world::village::*;

pub(super) fn closest_point_on_segment(point: Vec2, start: Vec2, end: Vec2) -> Vec2 {
    let segment = end - start;
    let length_squared = segment.length_squared();
    if length_squared <= 1e-6 {
        return start;
    }
    let t = ((point - start).dot(segment) / length_squared).clamp(0.0, 1.0);
    start + segment * t
}

pub(super) fn nearest_completed_road_frontage(
    candidate: Vec2,
    roads: &[&VillageRoad],
) -> Option<Vec2> {
    roads
        .iter()
        .filter(|road| road.is_complete())
        .flat_map(|road| road.built_points().windows(2))
        .map(|pair| closest_point_on_segment(candidate, pair[0], pair[1]))
        .min_by(|a, b| {
            a.distance_squared(candidate)
                .total_cmp(&b.distance_squared(candidate))
        })
        .filter(|frontage| frontage.distance_squared(candidate) <= 48.0 * 48.0)
}

pub(super) fn nearest_completed_road_access_point(
    candidate: Vec2,
    roads: &[&VillageRoad],
    blockers: &[RoadAccessBlocker],
    connected_keys: &HashSet<(i32, i32)>,
) -> Option<Vec2> {
    let mut best = None;
    let mut best_distance = f32::INFINITY;
    for point in roads
        .iter()
        .filter(|road| road.is_complete())
        .flat_map(|road| road.built_points().iter().copied())
    {
        if !connected_keys.contains(&crate::world::village_roads::road_point_key(point)) {
            continue;
        }
        let distance = point.distance_squared(candidate);
        // Only a new nearest point can replace the result. Testing every
        // farther road node against every building turned this O(road nodes ×
        // buildings) in a mature village even though almost all nodes could
        // never win.
        if distance >= best_distance || blockers.iter().any(|blocker| blocker.contains(point)) {
            continue;
        }
        best = Some(point);
        best_distance = distance;
    }
    best
}

#[derive(Clone, Copy)]
pub(crate) struct RoadAccessBlocker {
    pub(super) center: Vec2,
    pub(super) half: Vec2,
    pub(super) rotation: f32,
}

impl RoadAccessBlocker {
    pub(super) fn contains(self, point: Vec2) -> bool {
        let local = shared::rotation::world_to_local_xz(point - self.center, self.rotation);
        local.x.abs() <= self.half.x && local.y.abs() <= self.half.y
    }

    pub(super) fn blocks_segment(self, start: Vec2, end: Vec2) -> bool {
        shared::spatial::segment_intersects_box_after_start(
            shared::rotation::world_to_local_xz(start - self.center, self.rotation),
            shared::rotation::world_to_local_xz(end - self.center, self.rotation),
            self.half,
        )
    }
}

pub(crate) fn road_access_blockers_for_plot(
    kind: SettlementBuildingKind,
    position: Vec3,
    rotation: f32,
) -> Vec<RoadAccessBlocker> {
    let road_margin = RoadClass::Lane.initial_reserved_width() * 0.5 + 0.45;
    let definition = kind.placement_definition();
    let mut blockers = vec![RoadAccessBlocker {
        center: definition.world_footprint_center(position, rotation),
        half: definition.footprint * 0.5 + Vec2::splat(road_margin),
        rotation,
    }];
    if let (Some(fields), Some(field_half)) = (
        kind.field_positions(position, rotation),
        kind.field_half_extents(),
    ) {
        blockers.extend(fields.into_iter().map(|field| RoadAccessBlocker {
            center: Vec2::new(field.x, field.z),
            half: field_half
                + Vec2::splat(road_margin + shared::components::FARM_FIELD_TERRACE_MARGIN),
            rotation,
        }));
    }
    if let (Some(pasture), Some(half)) = (
        kind.pasture_position(position, rotation),
        kind.pasture_half_extents(),
    ) {
        blockers.push(RoadAccessBlocker {
            center: Vec2::new(pasture.x, pasture.z),
            half: half + Vec2::splat(road_margin + 1.0),
            rotation,
        });
    }
    blockers
}

pub(in crate::world::village) fn planned_road_access_path(
    terrain: &WorldTerrain,
    hall: Vec3,
    kind: SettlementBuildingKind,
    position: Vec3,
    rotation: f32,
    roads: &[&VillageRoad],
    blockers: &[RoadAccessBlocker],
    connected_keys: &HashSet<(i32, i32)>,
) -> Option<Vec<Vec2>> {
    const CELL: f32 = 4.0;
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
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            other
                .estimate
                .cmp(&self.estimate)
                .then_with(|| self.cell.x.cmp(&other.cell.x))
                .then_with(|| self.cell.z.cmp(&other.cell.z))
        }
    }

    impl PartialOrd for Open {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }

    let (door, approach) = crate::world::village_roads::doorway_approach(kind, position, rotation);
    let (goal, hall_door) = if let Some(frontage) =
        nearest_completed_road_access_point(approach, roads, blockers, connected_keys)
    {
        (frontage, None)
    } else {
        let (hall_door, hall_approach) =
            crate::world::village_roads::doorway_approach(SettlementBuildingKind::Hall, hall, 0.0);
        (hall_approach, Some(hall_door))
    };
    let reserved_width = RoadClass::Lane.initial_reserved_width();
    let search_min = approach.min(goal) - Vec2::splat(PADDING);
    let search_max = approach.max(goal) + Vec2::splat(PADDING);
    let local_blockers: Vec<_> = blockers
        .iter()
        .copied()
        .filter(|blocker| {
            let radius = blocker.half.length();
            blocker.center.x + radius >= search_min.x
                && blocker.center.y + radius >= search_min.y
                && blocker.center.x - radius <= search_max.x
                && blocker.center.y - radius <= search_max.y
        })
        .collect();
    let mut local_blockers = local_blockers;
    // The permit is deciding where a future shell will stand. Existing plots
    // alone are not enough: without the proposed shell here, A* can leave the
    // authored apron and bend straight back through the cabin's future floor.
    // Construction then quite correctly sees that shell and rejects the same
    // reserved path forever. Match the road survey's source-building margin;
    // crop plots retain the wider permanent reservation below.
    let proposed_definition = kind.placement_definition();
    local_blockers.push(RoadAccessBlocker {
        center: proposed_definition.world_footprint_center(position, rotation),
        half: proposed_definition.footprint * 0.5
            + Vec2::splat(crate::world::village_roads::VILLAGE_ROAD_WIDTH * 0.5 + 0.45),
        rotation,
    });
    if let (Some(fields), Some(field_half)) = (
        kind.field_positions(position, rotation),
        kind.field_half_extents(),
    ) {
        let field_margin =
            reserved_width * 0.5 + 0.45 + shared::components::FARM_FIELD_TERRACE_MARGIN;
        local_blockers.extend(fields.into_iter().map(|field| RoadAccessBlocker {
            center: Vec2::new(field.x, field.z),
            half: field_half + Vec2::splat(field_margin),
            rotation,
        }));
    }
    // The hall is not a SettlementBuilding and therefore is absent from the
    // caller's ordinary blocker list. It must remain solid even when this plot
    // joins an existing street: the geometrically nearest street point can be
    // on the far side of the Hall, and a direct line to it would otherwise
    // reserve and later draw a road through the civic building. Only the
    // explicit hall-approach-to-door segment below may enter this margin.
    let non_hall_blockers = local_blockers.len();
    let road_margin = reserved_width * 0.5 + 0.45;
    local_blockers.push(RoadAccessBlocker {
        center: shared::components::CivicHallLevel::reserved_world_center(hall, 0.0),
        half: shared::components::CivicHallLevel::reserved_half_extents()
            + Vec2::splat(road_margin),
        rotation: 0.0,
    });
    let edge_has_walkable_slope = |start: Vec2, end: Vec2| {
        let steps = (start.distance(end) / crate::world::navgrid::NAVIGATION_SAMPLE_STEP)
            .ceil()
            .max(1.0) as usize;
        let mut previous_height = None;
        (0..=steps).all(|step| {
            let point = start.lerp(end, step as f32 / steps as f32);
            let height = terrain.get_height(point.x, point.y);
            let slope_clear =
                previous_height.is_none_or(|previous: f32| (height - previous).abs() <= 0.47);
            previous_height = Some(height);
            slope_clear
        })
    };
    let edge_is_coarsely_clear = |start: Vec2, end: Vec2| {
        crate::world::village_roads::road_segment_is_coarsely_dry_at_width(
            terrain,
            start,
            end,
            reserved_width,
        ) && local_blockers
            .iter()
            .all(|blocker| !blocker.blocks_segment(start, end))
            && edge_has_walkable_slope(start, end)
    };

    let mut route = if crate::world::village_roads::road_segment_is_dry_at_width(
        terrain,
        approach,
        goal,
        reserved_width,
    ) && local_blockers
        .iter()
        .all(|blocker| !blocker.blocks_segment(approach, goal))
        && edge_has_walkable_slope(approach, goal)
    {
        vec![approach, goal]
    } else {
        let min = search_min;
        let max = search_max;
        let cell_for = |point: Vec2| Cell {
            x: (point.x / CELL).round() as i32,
            z: (point.y / CELL).round() as i32,
        };
        let point_for = |cell: Cell| Vec2::new(cell.x as f32 * CELL, cell.z as f32 * CELL);
        let heuristic =
            |a: Cell, b: Cell| Vec2::new((a.x - b.x) as f32, (a.z - b.z) as f32).length();
        let start_cell = cell_for(approach);
        let goal_cell = cell_for(goal);
        let mut open = std::collections::BinaryHeap::new();
        let mut closed = HashSet::new();
        let mut scores = HashMap::new();
        let mut came_from = HashMap::new();
        scores.insert(start_cell, 0.0_f32);
        open.push(Open {
            estimate: (heuristic(start_cell, goal_cell) * 1_000.0) as i32,
            cell: start_cell,
        });
        let mut found = None;
        while let Some(Open { cell: current, .. }) = open.pop() {
            if !closed.insert(current) || closed.len() > MAX_NODES {
                continue;
            }
            let current_point = if current == start_cell {
                approach
            } else {
                point_for(current)
            };
            if current_point.distance(goal) <= CELL * 1.6
                && edge_is_coarsely_clear(current_point, goal)
            {
                found = Some(current);
                break;
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
                        || !edge_is_coarsely_clear(current_point, next_point)
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
                    came_from.insert(next, current);
                    open.push(Open {
                        estimate: ((tentative + heuristic(next, goal_cell)) * 1_000.0) as i32,
                        cell: next,
                    });
                }
            }
        }
        let mut cursor = found?;
        let mut cells = vec![cursor];
        while let Some(previous) = came_from.get(&cursor).copied() {
            cells.push(previous);
            cursor = previous;
        }
        cells.reverse();
        let mut path = vec![approach];
        path.extend(cells.into_iter().skip(1).map(point_for));
        if path
            .last()
            .is_none_or(|point| point.distance_squared(goal) > 0.01)
        {
            path.push(goal);
        }
        // Coarse terrain sampling belongs only inside the search. Approval is
        // authoritative: certify the chosen four-metre polyline at the same
        // width and 20 cm spacing that physical road construction uses.
        if !crate::world::village_roads::road_corridor_is_dry(terrain, &path, reserved_width) {
            return None;
        }
        path
    };

    route.insert(0, door);
    if let Some(hall_door) = hall_door {
        if !crate::world::village_roads::road_segment_is_dry_at_width(
            terrain,
            goal,
            hall_door,
            reserved_width,
        ) || local_blockers[..non_hall_blockers]
            .iter()
            .any(|blocker| blocker.blocks_segment(goal, hall_door))
        {
            return None;
        }
        route.push(hall_door);
    }
    route.dedup_by(|a, b| a.distance_squared(*b) <= 0.01);
    if !crate::world::village_roads::road_corridor_is_dry(terrain, &route, reserved_width) {
        return None;
    }
    if !route
        .windows(2)
        .all(|segment| edge_has_walkable_slope(segment[0], segment[1]))
    {
        return None;
    }
    Some(route)
}

pub(super) fn direct_road_access_is_coarsely_clear(
    terrain: &WorldTerrain,
    hall: Vec3,
    kind: SettlementBuildingKind,
    position: Vec3,
    rotation: f32,
    roads: &[&VillageRoad],
    blockers: &[RoadAccessBlocker],
    connected_keys: &HashSet<(i32, i32)>,
) -> bool {
    let (_, approach) = crate::world::village_roads::doorway_approach(kind, position, rotation);
    let goal = nearest_completed_road_access_point(approach, roads, blockers, connected_keys)
        .unwrap_or_else(|| {
            crate::world::village_roads::doorway_approach(SettlementBuildingKind::Hall, hall, 0.0).1
        });
    crate::world::village_roads::road_segment_is_coarsely_dry_at_width(
        terrain,
        approach,
        goal,
        RoadClass::Lane.initial_reserved_width(),
    ) && blockers
        .iter()
        .all(|blocker| !blocker.blocks_segment(approach, goal))
}

#[cfg(test)]
mod road_access_tests {
    use super::*;

    #[test]
    fn processing_permits_require_completed_upstream_industries() {
        let mut completed = HashMap::new();
        assert!(!processing_upstream_is_complete(
            SettlementBuildingKind::Windmill,
            &completed
        ));
        completed.insert(SettlementBuildingKind::Farmstead, 1);
        assert!(processing_upstream_is_complete(
            SettlementBuildingKind::Windmill,
            &completed
        ));
        assert!(!processing_upstream_is_complete(
            SettlementBuildingKind::Bakery,
            &completed
        ));
        completed.insert(SettlementBuildingKind::Windmill, 1);
        assert!(processing_upstream_is_complete(
            SettlementBuildingKind::Bakery,
            &completed
        ));
    }

    #[test]
    fn resource_search_cursor_expands_beyond_the_preferred_layout_band() {
        assert_eq!(
            include_resumable_search_cursor(54.0, 120.0, Some(246.0)),
            (246.0, 246.0),
            "an outward retry must inspect its real cursor rather than resampling 120 m"
        );
        assert_eq!(
            include_resumable_search_cursor(54.0, 120.0, Some(999.0)),
            (MAX_SETTLEMENT_SEARCH_RADIUS, MAX_SETTLEMENT_SEARCH_RADIUS),
            "physical settlement search remains bounded"
        );
    }

    #[test]
    fn permit_access_routes_around_the_future_town_hall_shell() {
        let terrain = WorldTerrain::default();
        let hall = Vec3::new(1700.0, terrain.get_height(1700.0, 0.0), 0.0);
        // This cabin is behind the civic centre. A direct connector to the
        // permanent front door would cross the rear half of the future Town
        // Hall even though only the smaller Moot Hall is visible today.
        let position = Vec3::new(1700.0, terrain.get_height(1700.0, 30.0), 30.0);
        let route = planned_road_access_path(
            &terrain,
            hall,
            SettlementBuildingKind::House,
            position,
            0.0,
            &[],
            &[],
            &HashSet::new(),
        )
        .expect("the connector should bend around reserved civic ground");

        let reserved = RoadAccessBlocker {
            center: shared::components::CivicHallLevel::reserved_world_center(hall, 0.0),
            half: shared::components::CivicHallLevel::reserved_half_extents(),
            rotation: 0.0,
        };
        assert!(route.len() > 4, "the direct road was not bent: {route:?}");
        assert!(
            route
                .windows(2)
                .all(|segment| !reserved.blocks_segment(segment[0], segment[1])),
            "a permit reserved road through the future Town Hall: {route:?}"
        );
    }

    #[test]
    fn permit_access_bends_around_an_existing_building_and_remains_authoritative() {
        let terrain = WorldTerrain::default();
        let hall = Vec3::new(1700.0, terrain.get_height(1700.0, 0.0), 0.0);
        let position = Vec3::new(1744.0, terrain.get_height(1744.0, 0.0), 0.0);
        let kind = SettlementBuildingKind::House;
        let rotation = 0.0;
        let (_, start) = crate::world::village_roads::doorway_approach(kind, position, rotation);
        let (_, goal) =
            crate::world::village_roads::doorway_approach(SettlementBuildingKind::Hall, hall, 0.0);
        let blocker = RoadAccessBlocker {
            center: start.lerp(goal, 0.5),
            half: Vec2::splat(5.0),
            rotation: 0.0,
        };
        let route = planned_road_access_path(
            &terrain,
            hall,
            kind,
            position,
            rotation,
            &[],
            &[blocker],
            &HashSet::new(),
        )
        .expect("a dry access reservation should bend around the occupied shell");

        assert!(
            route.len() > 4,
            "the blocked direct line must become a bend"
        );
        assert!(route
            .windows(2)
            .all(|segment| !blocker.blocks_segment(segment[0], segment[1])));
        let future_definition = kind.placement_definition();
        let future_shell = RoadAccessBlocker {
            center: future_definition.world_footprint_center(position, rotation),
            half: future_definition.footprint * 0.5
                + Vec2::splat(crate::world::village_roads::VILLAGE_ROAD_WIDTH * 0.5 + 0.45),
            rotation,
        };
        assert!(
            route
                .windows(2)
                .skip(1)
                .all(|segment| !future_shell.blocks_segment(segment[0], segment[1])),
            "only the authored door apron may touch the proposed shell: {route:?}",
        );
        assert!(crate::world::village_roads::road_corridor_is_dry(
            &terrain,
            &route,
            RoadClass::Lane.initial_reserved_width(),
        ));
    }

    #[test]
    fn permit_access_never_anchors_on_a_detached_road_island() {
        let terrain = WorldTerrain::default();
        let hall = Vec3::new(1700.0, terrain.get_height(1700.0, 0.0), 0.0);
        let position = Vec3::new(1744.0, terrain.get_height(1744.0, 0.0), 0.0);
        let kind = SettlementBuildingKind::House;
        let rotation = 0.0;
        let (_, approach) = crate::world::village_roads::doorway_approach(kind, position, rotation);
        let detached = VillageRoad {
            settlement: "Island".into(),
            builder: "Old builder".into(),
            points: vec![approach + Vec2::X * 4.0, approach + Vec2::X * 12.0],
            built_through: 2,
            width: crate::world::village_roads::VILLAGE_ROAD_WIDTH,
            reserved_width: RoadClass::Lane.initial_reserved_width(),
            surface: shared::components::RoadSurface::Dirt,
            class: RoadClass::Lane,
            stone_committed: 0,
        };
        let roads = [&detached];
        let hall_door3 = SettlementBuildingKind::Hall.entrance_position(hall, 0.0);
        let hall_door = Vec2::new(hall_door3.x, hall_door3.z);
        let connected = crate::world::village_roads::hall_connected_road_keys(hall_door, &roads);

        let route = planned_road_access_path(
            &terrain,
            hall,
            kind,
            position,
            rotation,
            &roads,
            &[],
            &connected,
        )
        .expect("the plot should fall back to the Moot Hall component");

        assert!(route.last().unwrap().distance_squared(hall_door) <= 0.01);
        assert!(route.last().unwrap().distance_squared(detached.points[0]) > 0.01);
    }

    #[test]
    fn manual_house_plot_uses_the_authoritative_access_and_charter_rules() {
        let terrain = WorldTerrain::default();
        let hall = Vec3::new(1700.0, terrain.get_height(1700.0, 0.0), 0.0);
        let position = Vec3::new(1744.0, terrain.get_height(1744.0, 0.0), 0.0);
        let approval = validate_manual_plot(
            &terrain,
            hall,
            SettlementBuildingKind::House,
            position,
            0.0,
            &[(hall, SettlementBuildingKind::Hall.clearance())],
            &[],
            &[],
            &[],
            None,
            None,
        )
        .expect("a dry nearby player plot should reserve a real Hall connector");
        assert!(approval.road_access.len() >= 2);
        assert_eq!(
            approval.position.y,
            terrain.get_height(position.x, position.z)
        );

        let outside = Vec3::new(hall.x + MAX_SETTLEMENT_SEARCH_RADIUS + 1.0, 0.0, hall.z);
        let rejection = validate_manual_plot(
            &terrain,
            hall,
            SettlementBuildingKind::House,
            outside,
            0.0,
            &[(hall, SettlementBuildingKind::Hall.clearance())],
            &[],
            &[],
            &[],
            None,
            None,
        )
        .unwrap_err();
        assert!(rejection.contains("charter"));
    }

    #[test]
    fn exhausted_fishing_coast_advances_one_ring_then_sleeps_until_terrain_changes() {
        let terrain = WorldTerrain::default();
        assert!(terrain.water_level().is_some());
        let hall = Vec3::new(1700.0, terrain.get_height(1700.0, 0.0), 0.0);
        // One deliberately enormous occupied plot makes every shoreline
        // candidate fail before its facing checks. The assertion here is the
        // live cursor contract, independent of a particular generated coast.
        let occupied = vec![(hall, 1_000.0)];
        let settlement = Entity::from_bits(9);
        let kind = SettlementBuildingKind::FishermansHut;
        let (minimum, maximum) = kind.preferred_ring();
        let rings = ((maximum - minimum) / 4.0).floor() as usize + 1;
        let mut clock = VillageClock::default();

        for _ in 0..rings {
            assert!(find_incremental_fishing_site(
                &terrain,
                hall,
                &occupied,
                &[],
                settlement,
                &mut clock,
            )
            .is_none());
        }

        assert_eq!(
            clock.site_search_radii.get(&(settlement, kind)).copied(),
            Some(maximum),
        );
        assert_eq!(
            clock
                .failed_fishing_terrain_versions
                .get(&settlement)
                .copied(),
            Some(terrain.modification_version()),
        );
        // The next permit decision returns from the exhausted-coast cache and
        // does not restart at the founding ring.
        assert!(find_incremental_fishing_site(
            &terrain,
            hall,
            &occupied,
            &[],
            settlement,
            &mut clock,
        )
        .is_none());
        assert_eq!(
            clock.site_search_radii.get(&(settlement, kind)).copied(),
            Some(maximum),
        );
    }

    #[test]
    fn rejected_fishing_access_advances_instead_of_poisoning_the_coast() {
        let settlement = Entity::from_bits(10);
        let kind = SettlementBuildingKind::FishermansHut;
        let (minimum, maximum) = kind.preferred_ring();
        let mut clock = VillageClock::default();
        clock.site_search_radii.insert((settlement, kind), minimum);
        clock.failed_fishing_terrain_versions.insert(settlement, 7);

        assert!(advance_incremental_fishing_search(&mut clock, settlement));
        assert_eq!(
            clock.site_search_radii.get(&(settlement, kind)).copied(),
            Some((minimum + 4.0).min(maximum))
        );
        assert!(!clock
            .failed_fishing_terrain_versions
            .contains_key(&settlement));

        clock.site_search_radii.insert((settlement, kind), maximum);
        assert!(!advance_incremental_fishing_search(&mut clock, settlement));
    }
}
