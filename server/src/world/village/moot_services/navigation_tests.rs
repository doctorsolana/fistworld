use super::*;
use crate::player::hero::step_units;
use crate::world::village_roads::RouteWaypoint;
use shared::components::SettlementTier;
use shared::spatial::ObstacleEntry;
use std::time::Duration;

#[test]
fn a_counter_approach_follows_its_detour_until_it_reaches_the_forecourt() {
    counter_detour(0.);
    counter_detour(2.);
}

fn counter_detour(previous_ground_height: f32) {
    let mut app = App::new();
    app.init_resource::<Time>();
    app.init_resource::<MootQueueClock>();
    let hall_position = Vec3::new(1700., 80., 0.);
    let mut terrain = WorldTerrain::default();
    terrain.apply_flatten_rect(hall_position, Vec2::splat(50.), 0., 4.);
    let target = world_slot(
        hall_position,
        0.,
        MootServiceLane::Resident,
        0,
        Some(&terrain),
    );
    let start = hall_position + Vec3::new(12., 0., -12.);
    let corner = Vec3::new(start.x, target.y, target.z);
    // Leveling nearby building ground can change Y while the same counter
    // destination remains active. The mover resolves height from live terrain.
    let previous_target = target + Vec3::Y * previous_ground_height;
    let mut obstacles = SpatialObstacleGrid::default();
    obstacles.insert(ObstacleEntry {
        center: hall_position.xz() + Vec2::new(6.5, -9.),
        half_extents: Vec2::new(1.3, 1.5),
        rotation: 0.,
        obstacle_type: 0,
    });
    assert!(obstacles.segment_blocked(start.xz(), target.xz()));
    assert!(!obstacles.segment_blocked(start.xz(), corner.xz()));
    assert!(!obstacles.segment_blocked(corner.xz(), target.xz()));
    app.insert_resource(terrain);
    app.insert_resource(obstacles);
    app.add_systems(Update, (advance_moot_service_queues, step_units).chain());
    let hall = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Detourford".into(),
                tier: SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            },
            PlayerPosition(hall_position),
            PlayerRotation(0.),
        ))
        .id();
    let person = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            CharacterActivity::Idle,
            PlayerPosition(start),
            PlayerRotation(0.),
            RegionCoord::from_world_pos(start),
            MoveTarget(previous_target),
            TravelRoute {
                goal: previous_target,
                next: 0,
                geometry_version: 0,
                waypoints: vec![
                    RouteWaypoint {
                        position: corner,
                        on_road: false,
                    },
                    RouteWaypoint {
                        position: target,
                        on_road: false,
                    },
                ],
            },
            MootQueueTicket {
                hall,
                serial: 1,
                kind: MootServiceKind::Permit,
                state: MootQueueState::Queued,
                failed_routes: 0,
                head_wait_seconds: 0.,
                head_best_distance: f32::INFINITY,
                head_route_progress: None,
            },
        ))
        .id();
    // Real movement for ten seconds: long enough to walk the detour and be
    // served, shorter than the fifteen-second no-progress retry threshold.
    for _ in 0..600 {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs_f32(1. / 60.));
        app.update();
        assert!(
            app.world().get::<NavigationRoutePending>(person).is_none(),
            "a certified counter approach must not be restarted on the next queue tick"
        );
        let at = app.world().get::<PlayerPosition>(person).unwrap().0;
        assert!(
            !app.world()
                .resource::<SpatialObstacleGrid>()
                .point_blocked(at.xz())
        );
    }
    let at = app.world().get::<PlayerPosition>(person).unwrap().0;
    assert!(
        ground_distance(at, target) <= QUEUE_REACH,
        "arrival must be physical, not elapsed service time: {at:?}"
    );
    assert!(
        app.world()
            .get::<MootQueueTicket>(person)
            .unwrap()
            .is_ready()
    );
}
