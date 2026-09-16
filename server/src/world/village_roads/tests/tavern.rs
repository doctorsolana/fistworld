use super::*;
use crate::collision::building_index::{BuildingSpatialIndex, sync_building_spatial_index};
use crate::world::navgrid::{ObstacleGridState, sync_obstacle_grid};

#[test]
fn tavern_route_cache_matches_rotated_live_courtyard_collision() {
    let mut app = App::new();
    app.init_resource::<BuildingSpatialIndex>()
        .init_resource::<SpatialObstacleGrid>()
        .init_resource::<ObstacleGridState>()
        .add_systems(
            Update,
            (sync_building_spatial_index, sync_obstacle_grid).chain(),
        );
    let position = BuildingPosition(Vec3::new(1700.37, 80.0, 0.61));
    let entity = app.world_mut().spawn_empty().id();
    let mut cache = NavigationBuildingCache::default();
    for step in 0..16 {
        let yaw = step as f32 * std::f32::consts::TAU / 16.0;
        let building = PlacedBuilding {
            building_type: BuildingType::Tavern,
            rotation: yaw,
        };
        app.world_mut().entity_mut(entity).insert((
            PlacedBuilding {
                building_type: BuildingType::Tavern,
                rotation: yaw,
            },
            BuildingPosition(position.0),
        ));
        app.update();
        let changed = cache.rebuild(
            std::iter::once((&building, &position)),
            std::iter::empty(),
            std::iter::empty(),
        );
        assert!(!changed.is_empty());
        assert_eq!(cache.blockers.len(), 3);
        assert_eq!(cache.buildings.len(), 1, "tables are not doorway shells");
        let live = app.world().resource::<SpatialObstacleGrid>();
        for table in shared::building::tavern::table_obstacles(position.0, yaw) {
            assert!(cache.blockers.iter().any(|blocker| {
                blocker.center == table.center
                    && blocker.half == table.half_extents
                    && blocker.rotation == table.rotation
            }));
            assert!(changed.iter().any(|blocker| blocker.center == table.center));
            assert!(live.point_blocked(table.center));
            assert!(cache.spatial.point_blocked(table.center));
        }
        // Sample both sides of the inn, tables and open central aisle against
        // the actual movement grid, rather than a second hand-built fixture.
        for x in -28..=28 {
            for z in -40..=24 {
                let p = position.0.xz()
                    + shared::rotation::local_to_world_xz(
                        Vec2::new(x as f32 * 0.25, z as f32 * 0.25),
                        yaw,
                    );
                assert_eq!(cache.spatial.point_blocked(p), live.point_blocked(p));
            }
        }
        assert!(
            cache
                .rebuild(
                    std::iter::once((&building, &position)),
                    std::iter::empty(),
                    std::iter::empty(),
                )
                .is_empty()
        );
    }
    assert_eq!(
        cache
            .rebuild(std::iter::empty(), std::iter::empty(), std::iter::empty())
            .len(),
        3,
        "demolition invalidates both tables as well as the inn"
    );
    assert!(cache.spatial.is_empty());
}

#[test]
fn tavern_courtyard_routes_detour_around_tables_before_live_certification() {
    let origin = Vec3::new(1700.37, 80.0, 0.61);
    let mut terrain = WorldTerrain::default();
    terrain.apply_flatten_rect(origin, Vec2::splat(45.0), 0.0, 4.0);
    let position = BuildingPosition(origin);
    let mut cache = NavigationBuildingCache::default();
    let props = PropBlockers::default();
    let mut scratch = SurveyScratch::default();
    for step in 0..16 {
        let yaw = step as f32 * std::f32::consts::TAU / 16.0;
        let building = PlacedBuilding {
            building_type: BuildingType::Tavern,
            rotation: yaw,
        };
        cache.rebuild(
            std::iter::once((&building, &position)),
            std::iter::empty(),
            std::iter::empty(),
        );
        for table in shared::building::tavern::table_obstacles(origin, yaw) {
            let across = shared::rotation::local_to_world_xz(Vec2::X * 2.2, yaw);
            let start = table.center - across;
            let goal = table.center + across;
            assert!(!cache.spatial.point_blocked(start));
            assert!(!cache.spatial.point_blocked(goal));
            assert!(cache.spatial.segment_blocked(start, goal));
            let route = survey_agent_route(
                &terrain,
                start,
                goal,
                &cache.blockers,
                Some(&cache.spatial),
                &props,
                &mut scratch,
                AGENT_SURVEY_MAX_NODES,
            );
            assert!(route.len() > 2, "missing courtyard detour at yaw {yaw}");
            assert_eq!(route.first(), Some(&start));
            assert_eq!(route.last(), Some(&goal));
            assert!(
                route
                    .windows(2)
                    .all(|leg| !cache.spatial.segment_blocked(leg[0], leg[1]))
            );
        }
    }
}
