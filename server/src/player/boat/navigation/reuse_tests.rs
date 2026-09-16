use super::*;

#[test]
fn a_new_start_reuses_a_same_hull_destination_without_astar_but_with_fresh_proof() {
    let terrain = terrain(128., 129, |_, _| -4.);
    let original_start = Vec2::new(-80., 0.);
    let goal = Vec2::new(80., 0.);
    let mut cache = WaterRouteCache::default();
    let mut original = cache.begin(&terrain, original_start, goal);
    finish(&mut original, &terrain, &mut cache).expect("first sea lane");
    let start = Vec2::new(-80., 12.);
    let mut reuse = cache.begin(&terrain, start, goal);
    assert!(matches!(reuse.phase, Phase::Certify { .. }));
    assert!(
        reuse.known.is_empty(),
        "the old start's hull memo is not authority"
    );
    let route = finish(&mut reuse, &terrain, &mut cache).expect("new clear connector");
    assert_eq!(
        reuse.expanded, 0,
        "warm shared destination should not repeat A*"
    );
    assert!(!reuse.known.is_empty(), "full hull proof must still run");
    assert_eq!(route.first(), Some(&original_start));
    assert_full_hull_route(&terrain, start, goal, &route);
}

#[test]
fn a_changed_cached_sea_lane_cannot_cross_a_new_shoal() {
    let mut terrain = terrain(256., 257, |_, _| -4.);
    let original_start = Vec2::new(-120., 0.);
    let goal = Vec2::new(120., 0.);
    let mut cache = WaterRouteCache::default();
    let mut original = cache.begin(&terrain, original_start, goal);
    finish(&mut original, &terrain, &mut cache).expect("original direct ocean lane");
    terrain.apply_flatten_rect(Vec3::new(0., 4., 0.), Vec2::splat(8.), 0., 0.);
    assert!(!cache.geometry.segment_clear(
        &terrain,
        original_start,
        goal,
        WatercraftClearance::DINGHY
    ));
    let start = Vec2::new(-120., 16.);
    let mut reuse = cache.begin(&terrain, start, goal);
    assert!(
        matches!(reuse.phase, Phase::Certify { .. }),
        "sparse edits retain only a proposal"
    );
    assert_eq!(
        reuse.cell_size, COARSE_CELL,
        "new-start distance chooses the fallback grid"
    );
    let route =
        finish(&mut reuse, &terrain, &mut cache).expect("fresh route around the real shoal");
    assert!(
        reuse.expanded > 0,
        "invalid cached suffix must fall back to real A*"
    );
    assert_full_hull_route(&terrain, start, goal, &route);
}

#[test]
fn a_blocked_nearest_connector_falls_back_instead_of_rejecting_a_reachable_town() {
    let terrain = terrain(128., 129, |x, z| {
        if (-90. ..=-70.).contains(&x) && z.abs() <= 5. {
            4.
        } else {
            -4.
        }
    });
    let original_start = Vec2::new(-80., -20.);
    let goal = Vec2::new(80., -20.);
    let mut cache = WaterRouteCache::default();
    let mut original = cache.begin(&terrain, original_start, goal);
    finish(&mut original, &terrain, &mut cache).expect("clear lane below the island");
    let start = Vec2::new(-80., 20.);
    assert!(!cache.geometry.segment_clear(
        &terrain,
        start,
        original_start,
        WatercraftClearance::DINGHY
    ));
    assert!(
        cache
            .geometry
            .segment_clear(&terrain, start, goal, WatercraftClearance::DINGHY),
        "the new start has its own clear lane past the island"
    );
    let mut reuse = cache.begin(&terrain, start, goal);
    assert!(matches!(reuse.phase, Phase::Certify { .. }));
    let route =
        finish(&mut reuse, &terrain, &mut cache).expect("fresh direct lane remains reachable");
    assert_eq!(
        route,
        vec![goal],
        "the rejected cached connector must be replaced with a freshly certified lane"
    );
    assert_eq!(reuse.expanded, 0, "a clear fallback does not need A*");
    assert_full_hull_route(&terrain, start, goal, &route);
}

#[test]
fn proposals_never_borrow_another_destination_or_hull_profile() {
    let terrain = terrain(128., 129, |_, _| -4.);
    let original_start = Vec2::new(-80., 0.);
    let goal = Vec2::new(80., 0.);
    let mut cache = WaterRouteCache::default();
    let mut original = cache.begin(&terrain, original_start, goal);
    finish(&mut original, &terrain, &mut cache).expect("cached dinghy lane");
    let start = Vec2::new(-80., 12.);
    for kind in [ShipKind::Coaster, ShipKind::Cog] {
        let other = cache.begin_for(&terrain, start, goal, WatercraftClearance::for_ship(kind));
        assert!(
            matches!(other.phase, Phase::Validate),
            "{kind:?} borrowed a dinghy proof"
        );
    }
    let other_goal = cache.begin(&terrain, start, goal + Vec2::Y);
    assert!(matches!(other_goal.phase, Phase::Validate));
    let same_goal = cache.begin(&terrain, start, goal);
    assert!(matches!(same_goal.phase, Phase::Certify { .. }));
}

#[test]
fn replacing_the_base_map_discards_old_destination_proposals() {
    let mut terrain = terrain(128., 129, |_, _| -4.);
    let original_start = Vec2::new(-80., 0.);
    let goal = Vec2::new(80., 0.);
    let mut cache = WaterRouteCache::default();
    let mut original = cache.begin(&terrain, original_start, goal);
    finish(&mut original, &terrain, &mut cache).expect("cached sea lane");
    let mut replacement = terrain.generator.loaded_map().clone();
    replacement.content_hash = replacement.content_hash.wrapping_add(1);
    terrain.replace_loaded_map(replacement);
    let search = cache.begin(&terrain, Vec2::new(-80., 12.), goal);
    assert!(cache.entries.is_empty());
    assert!(matches!(search.phase, Phase::Validate));
}
