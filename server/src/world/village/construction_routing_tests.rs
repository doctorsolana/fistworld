//! Local timber must reach its own plot without a compulsory Hall round trip.

use super::*;

fn fixture() -> (WorldTerrain, PlannedRoadAccess, Vec3) {
    let origin = Vec3::new(1700.0, 80.0, 0.0);
    let mut terrain = WorldTerrain::default();
    terrain.apply_flatten_rect(origin, Vec2::splat(40.0), 0.0, 4.0);
    let access = PlannedRoadAccess {
        settlement_id: shared::components::SettlementId(1),
        points: [
            Vec2::new(0.0, 0.0),
            Vec2::new(0.0, 8.0),
            Vec2::new(0.0, 16.0),
            Vec2::new(-200.0, 16.0),
        ]
        .map(|point| point + origin.xz())
        .into(),
        half_width: 1.0,
    };
    (terrain, access, origin + Vec3::new(8.0, 0.0, 8.0))
}

fn context(terrain: &WorldTerrain) -> MaterialRouteContext<'_> {
    MaterialRouteContext {
        terrain,
        obstacles: None,
        colliders: None,
        derived: None,
    }
}

#[test]
fn timber_beside_the_site_requests_a_local_route_and_keeps_its_cargo() {
    let (terrain, access, position) = fixture();
    let routes = context(&terrain);
    let mut world = World::new();
    let mut cargo = GoodsInventory::new(shared::economy::capacity::VILLAGER);
    cargo.add(Good::Wood, 6);
    let builder = world.spawn(cargo).id();
    let phase = begin_material_delivery(
        &mut world.commands(),
        builder,
        position,
        position,
        &routes,
        Some(&access),
        false,
    );
    world.flush();
    let ConstructionMaterialPhase::ApproachingDeliveryAccess { entry } = phase else {
        panic!("a local join must first pass through the ordinary navigation queue");
    };
    assert_eq!(entry.xz(), access.points[1]);
    assert_ne!(entry.xz(), *access.points.last().unwrap());
    assert_eq!(world.get::<MoveTarget>(builder).unwrap().0, entry);
    assert!(
        world.get::<TravelRoute>(builder).is_none(),
        "no unchecked shortcut"
    );
    assert_eq!(
        world
            .get::<GoodsInventory>(builder)
            .unwrap()
            .amount(Good::Wood),
        6
    );
}

#[test]
fn even_a_nearby_apron_must_be_reached_by_normal_navigation() {
    let (terrain, access, _) = fixture();
    let routes = context(&terrain);
    let mut world = World::new();
    let builder = world.spawn_empty().id();
    let position = routes.point(access.points[1]) + Vec3::X * 0.2;
    let phase = begin_material_delivery(
        &mut world.commands(),
        builder,
        position,
        position,
        &routes,
        Some(&access),
        false,
    );
    world.flush();
    assert!(matches!(
        phase,
        ConstructionMaterialPhase::ApproachingDeliveryAccess { .. }
    ));
    assert!(world.get::<TravelRoute>(builder).is_none());
    assert_eq!(
        world.get::<MoveTarget>(builder).unwrap().0.xz(),
        access.points[1]
    );
}

#[test]
fn obstructed_apron_uses_a_clear_nearby_entry_and_only_its_retained_prefix() {
    let (terrain, access, position) = fixture();
    let mut obstacles = SpatialObstacleGrid::default();
    obstacles.insert(shared::spatial::ObstacleEntry {
        center: position.xz() - Vec2::X * 4.0,
        half_extents: Vec2::new(1.0, 1.0),
        rotation: 0.0,
        obstacle_type: 0,
    });
    let routes = MaterialRouteContext {
        obstacles: Some(&obstacles),
        ..context(&terrain)
    };
    assert!(!routes.plausible_local_join(position.xz(), access.points[1]));
    assert!(routes.plausible_local_join(position.xz(), access.points[2]));
    let mut world = World::new();
    let builder = world.spawn_empty().id();
    let phase = begin_material_delivery(
        &mut world.commands(),
        builder,
        position,
        position,
        &routes,
        Some(&access),
        false,
    );
    world.flush();
    let ConstructionMaterialPhase::ApproachingDeliveryAccess { entry } = phase else {
        panic!()
    };
    assert_eq!(entry.xz(), access.points[2]);
    assert!(world.get::<TravelRoute>(builder).is_none());

    // The production phase waits for movement to remove its target on arrival.
    world.entity_mut(builder).remove::<MoveTarget>();
    let phase = finish_material_approach(
        &mut world.commands(),
        builder,
        entry,
        &routes,
        Some(&access),
    );
    world.flush();
    let ConstructionMaterialPhase::Delivering { destination } = phase else {
        panic!()
    };
    assert_eq!(destination.xz(), access.points[1]);
    let route = world.get::<TravelRoute>(builder).unwrap();
    assert_eq!(
        route.geometry_version, 0,
        "retain live collision validation"
    );
    assert_eq!(
        route
            .waypoints
            .iter()
            .map(|p| p.position.xz())
            .collect::<Vec<_>>(),
        vec![access.points[2], access.points[1]]
    );
}

#[test]
fn failed_local_approach_immediately_reuses_the_full_certified_corridor() {
    let (_, access, position) = fixture();
    let mut checks = 0;
    let selected = material_delivery_entry(position.xz(), &access, true, |_, _| {
        checks += 1;
        true
    });
    assert_eq!(selected, Some(access.points.len() - 1));
    assert_eq!(checks, 0, "do not retry the same failed local join");
}

#[test]
fn blocked_local_joins_fall_back_without_more_than_four_probes() {
    let (_, mut access, position) = fixture();
    let hall = *access.points.last().unwrap();
    access.points.pop();
    access
        .points
        .extend((1..=200).map(|step| position.xz() + Vec2::X * step as f32 * 0.1));
    access.points.push(hall);
    let mut checks = 0;
    let selected = material_delivery_entry(position.xz(), &access, false, |_, _| {
        checks += 1;
        false
    });
    assert_eq!(selected, Some(access.points.len() - 1));
    assert_eq!(checks, MATERIAL_LOCAL_JOIN_CANDIDATES);
}

#[test]
fn a_partial_delivery_exits_near_the_site_instead_of_walking_to_the_hall() {
    let (terrain, access, _) = fixture();
    let (exit, waypoints) = delivery_egress_points(
        &terrain,
        Some(&access),
        Some(context(&terrain).point(access.points[2])),
    )
    .unwrap();
    assert_eq!(exit.xz(), access.points[2]);
    assert_eq!(waypoints.len(), 1);
    assert_eq!(exit.xz().distance(access.points[1]), 8.0);
    let (fallback, full_route) = delivery_egress_points(&terrain, Some(&access), None).unwrap();
    assert_eq!(fallback.xz(), *access.points.last().unwrap());
    assert_eq!(full_route.len(), 2);
    assert_eq!(full_route[0].position.xz(), access.points[2]);
}

#[test]
fn a_direct_apron_delivery_can_seek_again_without_any_egress_walk() {
    let (terrain, access, _) = fixture();
    let apron = context(&terrain).point(access.points[1]);
    let (exit, waypoints) = delivery_egress_points(&terrain, Some(&access), Some(apron)).unwrap();
    assert_eq!(exit, apron);
    assert!(waypoints.is_empty());
}

#[test]
fn timber_a_hundred_metres_from_the_plot_still_tries_its_apron_before_the_hall() {
    let (mut terrain, access, _) = fixture();
    let position = Vec3::new(access.points[1].x + 100.0, 80.0, access.points[1].y);
    terrain.apply_flatten_rect(position, Vec2::splat(10.0), 0.0, 4.0);
    let routes = context(&terrain);
    let mut world = World::new();
    let builder = world.spawn_empty().id();
    let phase = begin_material_delivery(
        &mut world.commands(),
        builder,
        position,
        position,
        &routes,
        Some(&access),
        false,
    );
    world.flush();
    assert!(matches!(
        phase,
        ConstructionMaterialPhase::ApproachingDeliveryAccess { .. }
    ));
    assert_eq!(
        world.get::<MoveTarget>(builder).unwrap().0.xz(),
        access.points[1]
    );
    assert!(
        world.get::<TravelRoute>(builder).is_none(),
        "long joins also require ordinary proof"
    );
    assert_eq!(
        world
            .get::<ConstructionDeliveryAccess>(builder)
            .unwrap()
            .entry
            .xz(),
        access.points[1]
    );
}

#[test]
fn a_failed_join_remembers_the_full_return_corridor() {
    let (terrain, access, position) = fixture();
    let routes = context(&terrain);
    let mut world = World::new();
    let builder = world.spawn_empty().id();
    begin_material_delivery(
        &mut world.commands(),
        builder,
        position,
        position,
        &routes,
        Some(&access),
        true,
    );
    world.flush();
    let entry = world
        .get::<ConstructionDeliveryAccess>(builder)
        .unwrap()
        .entry;
    assert_eq!(entry.xz(), *access.points.last().unwrap());
    let (exit, waypoints) = delivery_egress_points(&terrain, Some(&access), Some(entry)).unwrap();
    assert_eq!(exit, entry);
    assert_eq!(waypoints.len(), access.points.len() - 2);
}

#[test]
fn a_removed_access_reservation_clears_its_return_marker_without_delivering() {
    let (terrain, access, _) = fixture();
    let routes = context(&terrain);
    let entry = routes.point(access.points[1]);
    let mut world = World::new();
    let builder = world
        .spawn((MoveTarget(entry), ConstructionDeliveryAccess { entry }))
        .id();
    let phase = finish_material_approach(&mut world.commands(), builder, entry, &routes, None);
    world.flush();
    assert!(matches!(phase, ConstructionMaterialPhase::Seeking));
    assert!(world.get::<ConstructionDeliveryAccess>(builder).is_none());
    assert!(world.get::<MoveTarget>(builder).is_none());
    assert!(world.get::<TravelRoute>(builder).is_none());
}
