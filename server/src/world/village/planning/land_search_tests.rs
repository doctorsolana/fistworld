//! Actual land surveys exercise the live permit entry point and resumable cursor.
use super::*;

#[test]
fn exhausted_service_permits_never_scan_the_whole_settlement_in_one_review() {
    let terrain = WorldTerrain::default();
    let hall = Vec3::new(1720.0, terrain.get_height(1720.0, 0.0), 0.0);
    let plan = shared::components::SettlementDevelopment::from_foundation("Budget", hall, 0);
    // Simulate a crowded mature town. These failed services formerly scanned
    // thousands of charter and fallback samples synchronously.
    let occupied = vec![OccupiedLand::block(hall, Vec2::splat(1000.0), 0.0); 80];
    for kind in [
        SettlementBuildingKind::House,
        SettlementBuildingKind::Market,
        SettlementBuildingKind::Tavern,
        SettlementBuildingKind::Church,
        SettlementBuildingKind::Bakery,
        SettlementBuildingKind::StorageHall,
        SettlementBuildingKind::LivestockFarm,
    ] {
        let mut rejected = SiteSearchRejections::default();
        assert!(
            find_permit_site(
                &terrain,
                hall,
                kind,
                &occupied,
                &[],
                &[],
                &[],
                &[],
                Some(&plan),
                None,
                None,
                None,
                Some(&mut rejected),
                None,
                None,
                &[],
            )
            .is_none()
        );
        assert!(rejected.sampled > 0);
        assert!(
            rejected.sampled <= 60,
            "{kind:?} exhausted more than one 12+48 bearing band: {rejected:?}"
        );
    }
}

#[test]
fn a_tavern_search_reaches_legal_land_beyond_its_filled_founding_ring() {
    let mut terrain = WorldTerrain::default();
    let hall = Vec3::new(1700.0, 80.0, 0.0);
    // Isolate cursor progression from this map's irregular shoreline/slope.
    terrain.apply_flatten_rect(hall, Vec2::splat(350.0), 0.0, 4.0);
    let kind = SettlementBuildingKind::Tavern;
    let settlement = Entity::from_bits(29);
    let mut clock = VillageClock::default();
    let occupied_radius = kind.preferred_ring().1 + 6.0;
    let occupied = [OccupiedLand::block(hall, Vec2::splat(occupied_radius), 0.0)];
    let mut accepted = None;
    for _ in 0..60 {
        let mut rejected = SiteSearchRejections::default();
        accepted = find_permit_site(
            &terrain,
            hall,
            kind,
            &occupied,
            &[],
            &[],
            &[],
            &[],
            None,
            None,
            None,
            clock.site_search_radii.get(&(settlement, kind)).copied(),
            Some(&mut rejected),
            None,
            None,
            &[],
        );
        assert!(rejected.sampled <= 48, "one open-land band per review");
        if accepted.is_some() {
            break;
        }
        assert!(
            advance_land_search(&mut clock, settlement, kind),
            "flat dry land outside the crowded founding core must remain discoverable: {rejected:?}"
        );
    }
    let (position, rotation) =
        accepted.expect("incremental survey eventually finds a real legal site");
    let shell = shared::components::footprint_claim(kind, position, rotation);
    assert!(!occupied[0].blocks(&shell));
    assert!(position.xz().distance(hall.xz()) > kind.preferred_ring().1);
}

#[test]
fn the_final_land_ring_is_searched_before_exhaustion_and_progress_is_per_kind() {
    let settlement = Entity::from_bits(31);
    let mut clock = VillageClock::default();
    let tavern = SettlementBuildingKind::Tavern;
    let church = SettlementBuildingKind::Church;
    clock
        .site_search_radii
        .insert((settlement, tavern), MAX_SETTLEMENT_SEARCH_RADIUS - 1.0);
    assert!(advance_land_search(&mut clock, settlement, tavern));
    assert_eq!(
        clock.site_search_radii[&(settlement, tavern)],
        MAX_SETTLEMENT_SEARCH_RADIUS
    );
    assert!(!advance_land_search(&mut clock, settlement, tavern));
    assert!(advance_land_search(&mut clock, settlement, church));
    assert_eq!(
        clock.site_search_radii[&(settlement, church)],
        church.preferred_ring().0 + SEARCH_RING_STEP
    );
    assert_eq!(
        clock.site_search_radii[&(settlement, tavern)],
        MAX_SETTLEMENT_SEARCH_RADIUS
    );
}
