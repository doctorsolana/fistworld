use super::*;
use shared::components::{TimeWarp, WorldTime};
use shared::region::RegionCoord;

#[test]
fn embodied_collision_seeds_one_real_time_backoff_before_retry_at_both_warps() {
    let mut observed_delays = Vec::new();
    for warp in [1.0, 25.0] {
        let mut app = App::new();
        let mut terrain = WorldTerrain::default();
        terrain.apply_flatten_rect(Vec3::new(0.0, 80.0, 0.0), Vec2::splat(32.0), 0.0, 4.0);
        app.init_resource::<Time>()
            .init_resource::<VillageRoadGraph>()
            .insert_resource(terrain)
            .init_resource::<SpatialObstacleGrid>()
            .add_systems(
                Update,
                (
                    retry_failed_routes_after_obstacle_change,
                    crate::player::hero::step_units,
                )
                    .chain(),
            );
        app.world_mut()
            .spawn((WorldTime::new_default(), TimeWarp::clamped(warp)));
        let start = Vec3::new(0.0, 80.0, 0.0);
        let goal = start + Vec3::X * 10.0;
        let worker = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(start),
                PlayerRotation(0.0),
                RegionCoord::from_world_pos(start),
                MoveTarget(goal),
                TravelRoute {
                    goal,
                    waypoints: vec![RouteWaypoint {
                        position: goal,
                        on_road: false,
                    }],
                    next: 0,
                    geometry_version: 0,
                },
            ))
            .id();
        // A once-certified route now meets newly occupied ground. The real
        // mover generates this failure, rather than the planner's rejection.
        app.world_mut()
            .resource_mut::<SpatialObstacleGrid>()
            .insert(shared::spatial::ObstacleEntry {
                center: Vec2::new(1.0, 0.0),
                half_extents: Vec2::new(0.1, 2.0),
                rotation: 0.0,
                obstacle_type: 0,
            });
        let tick = |app: &mut App| {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(Duration::from_secs_f64(1.0 / 60.0));
            app.update();
        };
        for _ in 0..120 {
            tick(&mut app);
            if app.world().get::<NavigationRouteFailed>(worker).is_some() {
                break;
            }
        }
        assert!(app.world().get::<NavigationRouteFailed>(worker).is_some());
        assert!(app.world().get::<NavigationRouteBackoff>(worker).is_none());
        let stopped = app.world().get::<PlayerPosition>(worker).unwrap().0;
        tick(&mut app);
        let initial = *app.world().get::<NavigationRouteBackoff>(worker).unwrap();
        let seeded_at = app.world().resource::<Time>().elapsed_secs_f64();
        assert!(initial.retry_after > seeded_at);
        assert_eq!(initial.failures, 1);
        observed_delays.push(initial.retry_after - seeded_at);
        app.world_mut()
            .resource_mut::<SpatialObstacleGrid>()
            .clear();
        while app.world().resource::<Time>().elapsed_secs_f64() + 1.0 / 60.0 < initial.retry_after {
            tick(&mut app);
            let retained = app.world().get::<NavigationRouteBackoff>(worker).unwrap();
            assert_eq!(
                retained.retry_after, initial.retry_after,
                "waiting must not restart its timer"
            );
            assert_eq!(retained.failures, 1);
            assert!(app.world().get::<NavigationRouteFailed>(worker).is_some());
            assert!(app.world().get::<NavigationRoutePending>(worker).is_none());
            assert_eq!(
                app.world().get::<PlayerPosition>(worker).unwrap().0,
                stopped
            );
        }
        tick(&mut app);
        assert!(app.world().get::<NavigationRoutePending>(worker).is_some());
        assert!(app.world().get::<NavigationRouteFailed>(worker).is_none());
        assert_eq!(
            app.world().get::<PlayerPosition>(worker).unwrap().0,
            stopped,
            "retry admission still waits for a certified route"
        );
        assert_eq!(
            app.world()
                .get::<NavigationRouteBackoff>(worker)
                .unwrap()
                .retry_after,
            initial.retry_after
        );
    }
    assert!(
        (observed_delays[0] - observed_delays[1]).abs() < 1e-6,
        "warp must not multiply or divide the real-time retry delay"
    );
}
