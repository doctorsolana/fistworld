use super::super::plots::{advance_land_search, find_permit_site, SiteSearchRejections};
use super::*;

fn road_at(hall: Vec3) -> VillageRoad {
    let door = SettlementBuildingKind::Hall
        .entrance_position(hall, 0.0)
        .xz();
    VillageRoad {
        settlement: "Access test".into(),
        builder: "Builder".into(),
        points: vec![door, door + Vec2::new(45.0, 0.0)],
        built_through: 1,
        width: 2.0,
        reserved_width: 3.0,
        surface: shared::components::RoadSurface::Dirt,
        class: shared::components::RoadClass::Lane,
        stone_committed: 0,
    }
}

#[test]
fn completing_an_existing_lane_reopens_a_real_inner_service_site() {
    let mut terrain = WorldTerrain::default();
    let hall = Vec3::new(1700.0, 80.0, 0.0);
    terrain.apply_flatten_rect(hall, Vec2::splat(350.0), 0.0, 4.0);
    let settlement = Entity::from_bits(52);
    let kind = SettlementBuildingKind::Tavern;
    let mut road = road_at(hall);
    let mut clock = VillageClock::default();
    let initial = refresh_land_search_access(&mut clock, settlement, hall, &terrain, &[&road]);
    clock
        .site_search_radii
        .insert((settlement, kind), MAX_SETTLEMENT_SEARCH_RADIUS);
    clock.failed_site_searches.insert(
        (settlement, kind),
        FailedSiteSearch {
            kind,
            occupied_plots: 0,
            access_version: initial,
        },
    );
    // Same road entity/count: its built prefix is the changed physical fact.
    road.built_through = 2;
    let current = refresh_land_search_access(&mut clock, settlement, hall, &terrain, &[&road]);
    assert_ne!(current, initial);
    assert!(!clock
        .failed_site_searches
        .contains_key(&(settlement, kind)));
    assert!(!clock.site_search_radii.contains_key(&(settlement, kind)));
    // The first 18 m ring can legitimately be occupied by civic/road
    // clearance. Resume ordinary bounded reviews; never jump to a full scan.
    let mut accepted = None;
    for _ in 0..14 {
        let cursor = clock.site_search_radii.get(&(settlement, kind)).copied();
        assert!(cursor.unwrap_or(kind.preferred_ring().0) < 100.0);
        let mut rejected = SiteSearchRejections::default();
        accepted = find_permit_site(
            &terrain,
            hall,
            kind,
            &[],
            &[],
            &[&road],
            &[],
            &[],
            None,
            None,
            None,
            cursor,
            Some(&mut rejected),
            None,
            None,
            &[],
        );
        assert!(
            rejected.sampled <= 48,
            "one open-land ring per review: {rejected:?}"
        );
        if accepted.is_some() {
            break;
        }
        assert!(advance_land_search(&mut clock, settlement, kind));
    }
    let (position, _) = accepted.expect("the completed lane restores a real inner service site");
    assert!(position.xz().distance(hall.xz()) < 100.0);
}

#[test]
fn labels_and_unfinished_work_preserve_progress_but_local_geometry_rewinds_it() {
    let mut terrain = WorldTerrain::default();
    let hall = Vec3::new(1700.0, 80.0, 0.0);
    let settlement = Entity::from_bits(53);
    let other = Entity::from_bits(54);
    let kind = SettlementBuildingKind::Church;
    let fisher = SettlementBuildingKind::FishermansHut;
    let mut road = road_at(hall);
    road.points.push(road.points[1] + Vec2::X * 10.0);
    road.built_through = 2;
    let mut clock = VillageClock::default();
    let initial = refresh_land_search_access(&mut clock, settlement, hall, &terrain, &[&road]);
    for owner in [settlement, other] {
        clock.site_search_radii.insert((owner, kind), 160.0);
        clock.site_search_radii.insert((owner, fisher), 100.0);
    }
    road.builder = "Different builder".into();
    road.stone_committed = 12;
    road.points[2] += Vec2::X * 5.0;
    assert_eq!(
        refresh_land_search_access(&mut clock, settlement, hall, &terrain, &[&road]),
        initial
    );
    assert_eq!(clock.site_search_radii[&(settlement, kind)], 160.0);
    // An unrelated town's earthworks must not undo this town's bounded search.
    terrain.apply_flatten_rect(hall + Vec3::X * 1200.0, Vec2::splat(5.0), 0.0, 1.0);
    assert_eq!(
        refresh_land_search_access(&mut clock, settlement, hall, &terrain, &[&road]),
        initial
    );
    assert_eq!(clock.site_search_radii[&(settlement, kind)], 160.0);
    terrain.apply_flatten_rect(hall + Vec3::X * 30.0, Vec2::splat(5.0), 0.0, 1.0);
    assert_ne!(
        refresh_land_search_access(&mut clock, settlement, hall, &terrain, &[&road]),
        initial
    );
    assert!(!clock.site_search_radii.contains_key(&(settlement, kind)));
    assert_eq!(clock.site_search_radii[&(other, kind)], 160.0);
    assert_eq!(clock.site_search_radii[&(settlement, fisher)], 100.0);
}

#[test]
fn road_order_does_not_change_access_but_removal_and_widening_do() {
    let terrain = WorldTerrain::default();
    let hall = Vec3::new(1700.0, 80.0, 0.0);
    let mut a = road_at(hall);
    a.built_through = 2;
    let mut b = road_at(hall + Vec3::Z * 60.0);
    b.built_through = 2;
    let mut access = LandSearchAccess::default();
    assert!(!access.refresh(hall, &terrain, &[&a, &b]));
    assert!(!access.refresh(hall, &terrain, &[&b, &a]));
    b.width = 2.5;
    assert!(access.refresh(hall, &terrain, &[&a, &b]));
    assert!(access.refresh(hall, &terrain, &[&a]));
}
