use super::*;
use crate::collision::library::{DerivedCollider, StaticColliderInstance};
use shared::props::PropKind;

fn fixture() -> (WorldTerrain, StaticColliders, DerivedColliderLibrary, Vec3) {
    let start = Vec3::new(1700.2, 80.0, 0.0);
    let mut terrain = WorldTerrain::default();
    terrain.apply_flatten_rect(start, Vec2::splat(30.0), 0.0, 4.0);
    let mut colliders = StaticColliders::default();
    insert(&mut colliders, 0, start - Vec3::X * 0.2);
    let derived = DerivedColliderLibrary {
        by_kind: [(
            PropKind::PineA,
            DerivedCollider {
                horizontal_radius: 1.0,
            },
        )]
        .into(),
    };
    (terrain, colliders, derived, start)
}

fn insert(colliders: &mut StaticColliders, id: u32, position: Vec3) {
    let cell = (
        (position.x / 16.0).floor() as i32,
        (position.z / 16.0).floor() as i32,
    );
    colliders.cells.entry(cell).or_default().push(id);
    colliders.instances.insert(
        id,
        StaticColliderInstance {
            kind: PropKind::PineA,
            position,
            scale: 1.0,
            rotation: Quat::IDENTITY,
            cell,
        },
    );
}

#[test]
fn trapped_start_recovers_to_ground_that_can_resume_an_ordinary_solid_route() {
    let (terrain, colliders, derived, start) = fixture();
    let end = recover_prop_overlap(start, &terrain, None, &colliders, &derived).unwrap();
    assert!(start.distance(end) <= MAX_CORRECTION + 0.001);
    assert!(end.x > start.x);
    assert!(crate::player::hero::navigation_segment_clear(
        end.xz(),
        end.xz(),
        None,
        Some(&colliders),
        Some(&derived),
    ));
    assert!(recover_prop_overlap(end, &terrain, None, &colliders, &derived).is_none());
    let mut props = PropBlockers::default();
    add_static_collider_blockers(
        &mut props,
        Some(&colliders),
        Some(&derived),
        end.xz(),
        start.xz() + Vec2::X * 8.0,
        8.0,
        VILLAGER_PROP_RADIUS,
    );
    let route = survey_agent_route(
        &terrain,
        end.xz(),
        start.xz() + Vec2::X * 8.0,
        &[],
        None,
        &props,
        &mut SurveyScratch::default(),
        AGENT_SURVEY_MAX_NODES,
    );
    assert!(route.len() >= 2);
    assert!(polyline_clear_live_world(
        &route,
        None,
        Some(&colliders),
        Some(&derived)
    ));
}

#[test]
fn recovery_cannot_cross_a_second_prop_or_enclosing_wall() {
    let (terrain, mut colliders, derived, start) = fixture();
    insert(&mut colliders, 1, start + Vec3::X * 2.8);
    let end = recover_prop_overlap(start, &terrain, None, &colliders, &derived).unwrap();
    assert!(
        end.z.abs() > 0.1,
        "the second trunk requires a different escape angle"
    );
    let second = colliders.instances[&1].position.xz();
    assert!(
        point_segment_distance_squared(second, start.xz(), end.xz())
            >= (1.0 + VILLAGER_PROP_RADIUS).powi(2)
    );
    let mut walls = SpatialObstacleGrid::default();
    for (offset, half) in [
        (Vec2::X, Vec2::new(0.1, 1.1)),
        (-Vec2::X, Vec2::new(0.1, 1.1)),
        (Vec2::Y, Vec2::new(1.1, 0.1)),
        (-Vec2::Y, Vec2::new(1.1, 0.1)),
    ] {
        walls.insert(shared::spatial::ObstacleEntry {
            center: start.xz() + offset,
            half_extents: half,
            rotation: 0.0,
            obstacle_type: 0,
        });
    }
    assert!(recover_prop_overlap(start, &terrain, Some(&walls), &colliders, &derived).is_none());
}

#[test]
fn recovery_never_crosses_through_an_initial_prop_or_exceeds_its_local_limit() {
    let (terrain, colliders, mut derived, start) = fixture();
    let disc = PropDisc {
        kind: PropKind::PineA,
        center: start.xz() - Vec2::X * 0.2,
        radius: 1.35,
    };
    assert!(!clear_escape(
        start.xz(),
        start.xz() - Vec2::X * 2.0,
        &[disc]
    ));
    derived
        .by_kind
        .get_mut(&PropKind::PineA)
        .unwrap()
        .horizontal_radius = 6.0;
    assert!(recover_prop_overlap(start, &terrain, None, &colliders, &derived).is_none());
}

#[test]
fn recovery_does_not_relocate_a_body_through_flooded_ground() {
    let (mut terrain, colliders, derived, start) = fixture();
    let water = terrain.water_surface_height(start.x, start.z).unwrap();
    terrain.apply_flatten_rect(
        Vec3::new(start.x, water - 2.0, start.z),
        Vec2::splat(15.0),
        0.0,
        2.0,
    );
    assert!(!road_sample_is_dry(&terrain, start.xz()));
    assert!(recover_prop_overlap(start, &terrain, None, &colliders, &derived).is_none());
}

#[test]
fn recovery_leaves_multiple_initial_overlaps_without_entering_either_more_deeply() {
    let (terrain, mut colliders, derived, start) = fixture();
    insert(&mut colliders, 1, start + Vec3::X * 0.2);
    let end = recover_prop_overlap(start, &terrain, None, &colliders, &derived).unwrap();
    assert!((end.x - start.x).abs() < 0.001);
    assert!(end.z.abs() > 1.35);
    assert!(crate::player::hero::navigation_segment_clear(
        end.xz(),
        end.xz(),
        None,
        Some(&colliders),
        Some(&derived),
    ));
}
