//! Actual street-fitted entrances must remain usable by the production route survey.
use super::*;
use shared::components::{HouseAppearance, HouseLevel, HouseLine, HouseholdYardLand};

#[test]
fn fitted_street_gates_support_actual_house_routes_at_rotated_lattice_offsets() {
    let base = Vec3::new(1700., 80., 0.);
    let mut terrain = WorldTerrain::default();
    terrain.apply_flatten_rect(base, Vec2::splat(65.), 0., 4.);
    let props = PropBlockers::default();
    let mut scratch = SurveyScratch::default();
    let mut clipped = 0;
    let mut detours = 0;
    for line in [HouseLine::Cabin, HouseLine::LongCabin] {
        for level in [HouseLevel::Ground, HouseLevel::UpperStorey] {
            let appearance = HouseAppearance { line, level };
            let art = appearance.building_type();
            let definition = art.definition();
            let hi = definition.footprint_center + definition.footprint * 0.5;
            for step in [0, 1, 2, 5] {
                let yaw = step as f32 * std::f32::consts::FRAC_PI_8;
                for offset in [Vec2::ZERO, Vec2::new(0.37, 0.61)] {
                    let at = base + Vec3::new(offset.x, 0., offset.y);
                    let world = |p| at.xz() + shared::rotation::local_to_world_xz(p, yaw);
                    let road = VillageRoad {
                        settlement: "Route fixture".into(),
                        builder: "Builder".into(),
                        points: [Vec2::new(hi.x + 2.9, -9.), Vec2::new(hi.x + 5.2, 10.)]
                            .map(world)
                            .to_vec(),
                        built_through: 2,
                        width: 1.1,
                        reserved_width: 1.1,
                        surface: RoadSurface::Dirt,
                        class: RoadClass::Lane,
                        stone_committed: 0,
                    };
                    let mut land = HouseholdYardLand::default();
                    land.reserve_house(appearance, at, yaw);
                    land.reserve_road(&road);
                    let yard = land.fit_yard(appearance, at, yaw, 0, |p, _| shared::rotation::world_to_local_xz(p - at.xz(), yaw).x > hi.x, |_| Some(at.y))
                        .unwrap_or_else(|| panic!("no street-fitted yard for {appearance:?}, yaw={yaw}, offset={offset:?}"));
                    assert_eq!(yard.house, Some(appearance));
                    let entry = world(yard.entry.expect("new fitted yard has an actual entrance"));
                    let approach = world(
                        yard.approach
                            .expect("new fitted yard records the built-road endpoint"),
                    );
                    assert!(road.contains_built_point(approach, 0.05));
                    let size = yard.maximum - yard.minimum;
                    if yard.area() < size.x * size.y - 0.1 {
                        clipped += 1;
                    }
                    let building = PlacedBuilding {
                        building_type: art,
                        rotation: yaw,
                    };
                    let position = BuildingPosition(at);
                    let player_position = PlayerPosition(at);
                    let rotation = PlayerRotation(yaw);
                    let mut cache = NavigationBuildingCache::default();
                    cache.rebuild(
                        std::iter::once((&building, &position)),
                        std::iter::empty(),
                        std::iter::once((&yard, &player_position, &rotation)),
                    );
                    let center = world(yard.center());
                    let mut points = vec![(center, entry), (entry, approach), (approach, center)];
                    // Also certify useful inner points away from the
                    // centreline, including the broad open gate approach.
                    for corner in yard.boundary_points() {
                        let local = yard.center().lerp(corner, 0.58);
                        let start = world(local);
                        if yard.contains_local_point(local, -0.65)
                            && !cache.spatial.point_blocked(start)
                        {
                            points.push((start, approach));
                        }
                    }
                    // A wide gate can make every crop-to-road ray clear.
                    // Aim outside a real closed span instead: the direct ray
                    // must cross the fence, while A* must find its opening.
                    let mut spans = yard.fence_segments();
                    spans.sort_by(|(a, b), (c, d)| {
                        c.distance_squared(*d).total_cmp(&a.distance_squared(*b))
                    });
                    let outside = spans.into_iter().find_map(|(a, b)| {
                        let edge = b - a;
                        let outward = Vec2::new(edge.y, -edge.x).normalize_or_zero();
                        let goal = world((a + b) * 0.5 + outward * 1.2);
                        (!cache.spatial.point_blocked(goal)
                            && !yard.contains_world_point(goal, at, yaw, 0.2)
                            && cache.spatial.segment_blocked(center, goal))
                        .then_some(goal)
                    }).unwrap_or_else(|| panic!("no solid-span detour fixture for {appearance:?}, yaw={yaw}, offset={offset:?}"));
                    points.push((center, outside));
                    points.push((outside, center));
                    for (start, goal) in points {
                        if cache.spatial.segment_blocked(start, goal) {
                            detours += 1;
                        }
                        let route = survey_agent_route(
                            &terrain,
                            start,
                            goal,
                            &cache.blockers,
                            Some(&cache.spatial),
                            &props,
                            &mut scratch,
                            AGENT_SURVEY_MAX_NODES,
                        );
                        assert!(
                            !route.is_empty(),
                            "street gate trapped {appearance:?}, yaw={yaw}, offset={offset:?}, start={start:?}, goal={goal:?}"
                        );
                        assert!(route.first().unwrap().distance_squared(start) < 0.01);
                        assert!(route.last().unwrap().distance_squared(goal) < 0.01);
                        assert!(
                            route
                                .windows(2)
                                .all(|leg| !cache.spatial.segment_blocked(leg[0], leg[1])),
                            "a route crossed an actual fence or house"
                        );
                    }
                }
            }
        }
    }
    assert!(
        clipped > 0,
        "the matrix must include genuine diagonal street clipping"
    );
    assert!(
        detours > 0,
        "the matrix must exercise A* around solid spans, not just straight clear rays"
    );
}
