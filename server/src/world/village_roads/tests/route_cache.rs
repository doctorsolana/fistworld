use super::*;

#[test]
fn moving_work_and_cohort_destinations_do_not_grow_route_storage_forever() {
    let mut graph = VillageRoadGraph::default();
    for i in 0..MAX_COHORT_ROUTE_GOALS * 3 {
        let start = Vec2::new(i as f32, 0.);
        let goal = start + Vec2::Y * 20.;
        graph.cache_tactical_route(start, goal, &[(start, false), (goal, false)], true);
    }
    assert!(graph.cohort_routes.len() <= MAX_COHORT_ROUTE_GOALS);
    assert_eq!(graph.cohort_goal_order.len(), graph.cohort_routes.len());
    assert_eq!(
        graph.cohort_points,
        graph
            .cohort_routes
            .values()
            .flatten()
            .map(|route| route.tagged.len())
            .sum::<usize>()
    );
    let latest = Vec2::new((MAX_COHORT_ROUTE_GOALS * 3 - 1) as f32, 0.);
    assert!(
        graph
            .tactical_route(latest, latest + Vec2::Y * 20.)
            .is_some()
    );
    assert!(
        !graph
            .cohort_route_candidates(latest, latest + Vec2::Y * 20.)
            .is_empty()
    );
}

#[test]
fn long_corridors_have_a_geometry_budget_and_evicted_destinations_remain_recomputable() {
    let mut graph = VillageRoadGraph::default();
    for i in 0..600 {
        let start = Vec2::new(i as f32, 0.);
        let route: Vec<_> = (0..1000)
            .map(|j| (start + Vec2::Y * j as f32, false))
            .collect();
        graph.cache_tactical_route(start, route.last().unwrap().0, &route, false);
    }
    assert!(graph.tactical_points <= MAX_CACHED_ROUTE_POINTS);
    assert!(graph.cohort_points <= MAX_CACHED_ROUTE_POINTS);
    assert_eq!(
        graph.tactical_points,
        graph.tactical_routes.values().map(Vec::len).sum::<usize>()
    );
    assert!(graph.tactical_route(Vec2::ZERO, Vec2::Y * 999.).is_none());
    graph.cache_tactical_route(
        Vec2::ZERO,
        Vec2::Y * 999.,
        &[(Vec2::ZERO, false), (Vec2::Y * 999., false)],
        true,
    );
    assert_eq!(
        graph
            .tactical_route(Vec2::ZERO, Vec2::Y * 999.)
            .unwrap()
            .len(),
        2
    );
    // A geometry invalidation must update accounting as well as keys.
    graph.invalidate_tactical_routes_after_embodied_rejection(Vec2::ZERO, Vec2::Y * 999.);
    assert_eq!(
        graph.tactical_points,
        graph.tactical_routes.values().map(Vec::len).sum::<usize>()
    );
    assert_eq!(
        graph.cohort_points,
        graph
            .cohort_routes
            .values()
            .flatten()
            .map(|route| route.tagged.len())
            .sum::<usize>()
    );
}

#[test]
fn graph_pair_cache_is_bounded_while_destination_tree_reuse_remains_available() {
    let mut graph = VillageRoadGraph::default();
    graph.nodes = (0..100)
        .map(|i| RoadGraphNode {
            point: Vec2::X * i as f32,
            edges: Vec::new(),
        })
        .collect();
    for i in 0..99 {
        graph.nodes[i].edges.push((i + 1, 1.));
        graph.nodes[i + 1].edges.push((i, 1.));
    }
    for goal in 0..100 {
        for start in 0..100 {
            let route = graph.shortest_path(start, goal).unwrap();
            assert_eq!(route.first(), Some(&start));
            assert_eq!(route.last(), Some(&goal));
        }
    }
    assert!(graph.routes.len() <= MAX_GRAPH_ROUTE_PAIRS);
    assert!(graph.route_points <= MAX_CACHED_ROUTE_POINTS);
    assert!(graph.destination_trees.len() <= MAX_DESTINATION_TREES);
    let result = graph.shortest_path(0, 99).unwrap();
    assert_eq!(result, (0..100).collect::<Vec<_>>());
}

#[test]
fn distant_committed_jobs_use_regional_budget_without_a_512m_failure_cliff() {
    for objective in [
        CharacterObjective::GoingHome,
        CharacterObjective::CarryingConstructionWood,
        CharacterObjective::TravellingToSettlement,
    ] {
        assert!(needs_regional_corridor(Some(&objective), 600.));
        assert_eq!(
            agent_survey_max_nodes(600., Some(&objective)),
            INTERSETTLEMENT_TRADE_SURVEY_MAX_NODES
        );
    }
    assert_eq!(
        agent_survey_max_nodes(20., Some(&CharacterObjective::GoingHome)),
        AGENT_SURVEY_MAX_NODES
    );
}
