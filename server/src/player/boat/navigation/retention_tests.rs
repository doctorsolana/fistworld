use super::*;

#[test]
fn touched_earthworks_keep_a_fine_frontier_then_recertify_the_actual_route() {
    let mut terrain = terrain(256., 257, |x, z| {
        if x.abs() < 45. && z.abs() < 90. {
            4.
        } else {
            -4.
        }
    });
    let start = Vec2::new(-180., 0.);
    let goal = Vec2::new(180., 0.);
    let mut cache = WaterRouteCache::default();
    let mut search = cache.begin(&terrain, start, goal);
    // Exercise the expensive fallback that formerly lost tens of thousands of
    // cells when a sampled coast chunk changed during construction.
    search.cell_size = NAV_CELL;
    for _ in 0..2_000 {
        assert!(matches!(
            cache.advance(&mut search, &terrain),
            WaterPlanResult::Pending
        ));
        if search.expanded >= 96 {
            break;
        }
    }
    assert!(search.expanded >= 96);
    let expanded = search.expanded;
    let touched = search
        .known
        .iter()
        .find_map(|(key, clear)| {
            if *clear {
                return None;
            }
            let center = Vec2::new(f32::from_bits(key[0]), f32::from_bits(key[1]));
            search
                .offsets
                .iter()
                .map(|offset| center + *offset)
                .find(|point| !depth_clear(&terrain, *point, search.hull.draft))
        })
        .expect("the island must have rejected an actual terrain sample");
    terrain.apply_flatten_rect(Vec3::new(touched.x, 8., touched.y), Vec2::splat(1.), 0., 0.);
    assert!(matches!(
        cache.advance(&mut search, &terrain),
        WaterPlanResult::Pending
    ));
    assert!(
        search.expanded >= expanded,
        "a sampled dead-end edit discarded the fine frontier"
    );
    assert_eq!(search.cell_size, NAV_CELL);
    assert!(search.proposal_stale || matches!(search.phase, Phase::Certify { .. }));
    let mut saw_certification = matches!(search.phase, Phase::Certify { .. });
    let mut slices = 0;
    let route = loop {
        slices += 1;
        assert!(slices <= 10_000, "retained fixture failed to terminate");
        let result = cache.advance(&mut search, &terrain);
        saw_certification |= matches!(search.phase, Phase::Certify { .. });
        if let WaterPlanResult::Complete(route) = result {
            break route.expect("the untouched ocean still surrounds the island");
        }
    };
    assert!(
        saw_certification,
        "stale A* must not be returned as current proof"
    );
    assert_full_hull_route(&terrain, start, goal, &route);
}

#[test]
fn a_changed_exhausted_proposal_retries_before_declaring_current_water_unreachable() {
    // Authored vertices share the 2 m edit grid. A 1 m cliff between edit
    // vertices would legitimately leave a shallow ridge after this flatten.
    let mut terrain = terrain(48., 49, |x, _| if x.abs() < 8. { 4. } else { -4. });
    let start = Vec2::new(-24., 0.);
    let goal = Vec2::new(24., 0.);
    let mut cache = WaterRouteCache::default();
    let mut search = cache.begin(&terrain, start, goal);
    search.version = Some(cache.geometry.revision(&terrain));
    search.terrain_reads.changed(&terrain);
    search.terrain_reads.observe(&terrain, Vec2::ZERO);
    // Stage the exact final empty-frontier boundary of an older rejected
    // search, then open a real channel before it emits Complete(None).
    search.phase = Phase::Search;
    search.expanded = 96;
    terrain.apply_flatten_rect(Vec3::new(0., -4., 0.), Vec2::new(12., 12.), 0., 0.);
    assert!(
        cache
            .geometry
            .segment_clear(&terrain, start, goal, WatercraftClearance::DINGHY),
        "fixture excavation must actually open the entire swept hull corridor"
    );
    assert!(matches!(
        cache.advance(&mut search, &terrain),
        WaterPlanResult::Pending
    ));
    assert_eq!(
        search.expanded, 0,
        "old rejection must schedule a fresh attempt"
    );
    let route =
        finish(&mut search, &terrain, &mut cache).expect("the newly opened channel is usable");
    assert_full_hull_route(&terrain, start, goal, &route);
}

fn partial_certification(
    terrain: &WorldTerrain,
    start: Vec2,
    goal: Vec2,
) -> (WaterRouteCache, WaterSearch) {
    let mut cache = WaterRouteCache::default();
    let mut search = cache.begin(terrain, start, goal);
    search.version = Some(cache.geometry.revision(terrain));
    search.begin_certification(vec![goal], terrain);
    assert!(matches!(
        cache.advance(&mut search, terrain),
        WaterPlanResult::Pending
    ));
    assert!(matches!(search.phase, Phase::Certify { .. }));
    assert!(
        !search.known.is_empty(),
        "the first part of the hull proof should have run"
    );
    (cache, search)
}

#[test]
fn edits_to_an_already_certified_hull_sample_restart_only_proof_before_use() {
    let mut terrain = terrain(1024., 2, |_, _| -4.);
    let start = Vec2::new(-500., 0.);
    let goal = Vec2::new(500., 0.);
    let (mut cache, mut search) = partial_certification(&terrain, start, goal);
    terrain.apply_flatten_rect(Vec3::new(start.x, 4., 0.), Vec2::splat(8.), 0., 0.);
    assert!(matches!(
        cache.advance(&mut search, &terrain),
        WaterPlanResult::Pending
    ));
    assert!(
        finish(&mut search, &terrain, &mut cache).is_none(),
        "a fresh bank under the already-checked hull cannot inherit its old clearance"
    );
}

#[test]
fn new_bridge_clearance_is_checked_before_a_retained_proposal_can_be_installed() {
    let terrain = terrain(1024., 2, |_, _| -4.);
    let start = Vec2::new(0., -500.);
    let goal = Vec2::new(0., 500.);
    let (mut cache, mut search) = partial_certification(&terrain, start, goal);
    let mut world = World::new();
    world.init_resource::<WaterNavigationGeometry>();
    world.spawn(shared::components::RoadBridge {
        start: Vec3::new(-16., 2., 500.),
        end: Vec3::new(16., 2., 500.),
        deck_height: 4.,
        ramp_length: 8.,
        width: 3.6,
        built: true,
    });
    world
        .run_system_once(crate::player::boat::clearance::rebuild_water_navigation_geometry)
        .unwrap();
    cache.geometry = world.resource::<WaterNavigationGeometry>().clone();
    assert!(!cache
        .geometry
        .point_clear(&terrain, goal, WatercraftClearance::DINGHY));
    assert!(
        finish(&mut search, &terrain, &mut cache).is_none(),
        "a completed low bridge must reject the old destination, even after partial proof"
    );
}
