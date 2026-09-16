use super::*;

#[test]
fn a_clear_goal_beside_a_tree_detours_before_live_certification() {
    let trunk = Vec3::new(1708., 80., 0.);
    let start = trunk.xz() - Vec2::X * 8.;
    let goal = trunk.xz() + Vec2::X * 1.05;
    let mut terrain = WorldTerrain::default();
    terrain.apply_flatten_rect(trunk, Vec2::splat(40.), 0., 4.);
    let trunk_radius = 0.55;
    let (colliders, derived) = one_static_prop(PropKind::PineB, trunk, trunk_radius);
    let mut props = PropBlockers::default();
    props.insert_radius(trunk.xz(), trunk_radius + VILLAGER_PROP_RADIUS);
    assert!(!props.blocks(goal));
    assert!(
        goal.distance(trunk.xz()) + trunk_radius + VILLAGER_PROP_RADIUS < 2.,
        "the entire blocked disc must lie within the former goal exemption"
    );
    assert!(crate::player::hero::navigation_segment_clear(
        goal,
        goal,
        None,
        Some(&colliders),
        Some(&derived),
    ));
    assert!(!crate::player::hero::navigation_segment_clear(
        start,
        goal,
        None,
        Some(&colliders),
        Some(&derived),
    ));

    let route = survey_agent_route(
        &terrain,
        start,
        goal,
        &[],
        None,
        &props,
        &mut SurveyScratch::default(),
        AGENT_SURVEY_MAX_NODES,
    );
    assert!(
        route.len() > 2,
        "a clear endpoint cannot authorize cutting through the nearby trunk"
    );
    assert_eq!(route.first(), Some(&start));
    assert_eq!(route.last(), Some(&goal));
    assert!(
        polyline_clear_live_world(&route, None, Some(&colliders), Some(&derived)),
        "the planned detour must also pass the actual movement collision predicate: {route:?}"
    );

    let inside_trunk = survey_agent_route(
        &terrain,
        start,
        trunk.xz(),
        &[],
        None,
        &props,
        &mut SurveyScratch::default(),
        AGENT_SURVEY_MAX_NODES,
    );
    assert!(
        inside_trunk.is_empty(),
        "an occupied endpoint must fail the survey instead of looping on final certification"
    );
}
