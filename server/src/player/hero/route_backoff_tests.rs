//! Route admission remains authoritative even if a high-level owner clears a
//! failure result. These use the actual queue, retry, planner and mover.

use super::*;
use crate::world::pathfinding::PathfindingBudgetSettings;
use crate::world::village::moot_services::{
    self, MootQueueClock, MootQueueTransit, MootServiceKind,
};
use crate::world::village_roads::{
    plan_villager_travel_routes, queue_villager_travel_routes,
    retry_failed_routes_after_obstacle_change,
};
use shared::components::TimeWarp;
use std::time::Duration;

#[derive(Resource, Default)]
struct CertifiedGoals(Vec<Vec3>);

fn note_certified_routes(mut goals: ResMut<CertifiedGoals>, routes: Query<&TravelRoute>) {
    for route in &routes {
        if !goals.0.contains(&route.goal) {
            goals.0.push(route.goal);
        }
    }
}

fn terrain(barrier: bool) -> WorldTerrain {
    let mut terrain = WorldTerrain::default();
    let mut map = terrain.generator.loaded_map().clone();
    let bounds = shared::map::MapBounds {
        min: [-40.; 2],
        max: [40.; 2],
    };
    map.definition.bounds = bounds;
    map.definition.generated = None;
    map.definition.objects.clear();
    map.objects_by_chunk.clear();
    // A water strip spans the complete local map. Both endpoints are dry but
    // the genuine land planner cannot find a route around this barrier.
    let row = [4., 4., if barrier { -8. } else { 4. }, 4., 4.];
    map.heightmap = shared::map::HeightmapData::new(
        bounds,
        5,
        2,
        row.into_iter().chain(row).collect(),
        Some(0.),
    );
    map.rivers = default();
    map.river_segments_by_chunk.clear();
    map.terrain_deltas_by_chunk.clear();
    terrain.generator = shared::terrain::TerrainGenerator::from_loaded_map(map);
    terrain
}

fn fixture(warp: f32) -> (App, Entity, Vec3, Vec3) {
    let mut app = App::new();
    app.init_resource::<Time>()
        .init_resource::<VillageRoadGraph>()
        .init_resource::<PathfindingBudgetSettings>()
        .init_resource::<CertifiedGoals>()
        .insert_resource(terrain(true))
        .add_systems(
            Update,
            (
                queue_villager_travel_routes,
                retry_failed_routes_after_obstacle_change,
                plan_villager_travel_routes,
                note_certified_routes,
                step_units,
            )
                .chain(),
        );
    app.world_mut().spawn(TimeWarp(warp));
    let start = Vec3::new(-20., 4., 0.);
    let goal = Vec3::new(20., 4., 0.);
    let person = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            PlayerPosition(start),
            PlayerRotation(0.),
            RegionCoord::from_world_pos(start),
            CharacterMotion::STATIONARY,
            MoveTarget(goal),
        ))
        .id();
    // Fixed simulation steps may finish bounded planning in different numbers
    // of slices. There is no wall-clock timing assertion or forged failure.
    for _ in 0..512 {
        app.update();
        if app.world().get::<NavigationRouteFailed>(person).is_some() {
            break;
        }
    }
    assert!(
        app.world().get::<NavigationRouteFailed>(person).is_some(),
        "the real planner must reject the impassable water strip"
    );
    assert!(app.world().get::<NavigationRouteBackoff>(person).is_some());
    assert_eq!(app.world().get::<PlayerPosition>(person).unwrap().0, start);
    assert!(app.world().resource::<CertifiedGoals>().0.is_empty());
    (app, person, start, goal)
}

fn open_dry_corridor(app: &mut App, start: Vec3, goal: Vec3) {
    let mut terrain = app.world_mut().resource_mut::<WorldTerrain>();
    let before = terrain.modification_version();
    // Edit the live terrain through its authoritative API: replacing the
    // generator under an unchanged map hash/revision would leave cached
    // rejection results valid for what appears to be the same terrain.
    terrain.apply_flatten_rect(Vec3::new(0., 4., 0.), Vec2::new(30., 8.), 0., 4.);
    assert_ne!(terrain.modification_version(), before);
    assert!(crate::world::village_roads::road_segment_is_dry(
        &terrain,
        start.xz(),
        goal.xz(),
    ));
    assert!(crate::world::bridges::segment_walkable(
        &terrain,
        None,
        start.xz(),
        goal.xz(),
        shared::physics::CHARACTER_NAV_RADIUS,
    ));
}

fn tick(app: &mut App, seconds: f32) {
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs_f32(seconds));
    app.update();
}

#[test]
fn cleared_failure_cannot_walk_during_backoff_and_expiry_uses_a_real_replacement_route() {
    for warp in [1., 25.] {
        for reassert_target in [false, true] {
            let (mut app, person, start, goal) = fixture(warp);
            app.world_mut()
                .entity_mut(person)
                .remove::<NavigationRouteFailed>();
            for _ in 0..20 {
                if reassert_target {
                    app.world_mut().entity_mut(person).insert(MoveTarget(goal));
                }
                tick(&mut app, 0.01);
                assert_eq!(
                    app.world().get::<PlayerPosition>(person).unwrap().0,
                    start,
                    "clearing Failed cannot authorize a direct step at {warp}x"
                );
                assert!(
                    app.world().get::<NavigationRouteFailed>(person).is_none(),
                    "movement must not fabricate new planner failure events"
                );
                assert!(app.world().get::<NavigationRoutePending>(person).is_none());
                assert!(app.world().get::<NavigationRouteBackoff>(person).is_some());
            }
            // Geometry now permits a route, but only the scheduled retry can
            // certify it. The body is never relocated to manufacture success.
            open_dry_corridor(&mut app, start, goal);
            tick(&mut app, 2.);
            for _ in 0..2_000 {
                if app.world().get::<MoveTarget>(person).is_none() {
                    break;
                }
                tick(&mut app, 1. / 60.);
            }
            assert!(
                app.world().resource::<CertifiedGoals>().0.contains(&goal),
                "the real planner must certify the edited corridor at {warp}x (reasserted: {reassert_target}); position={:?}, failure={:?}, backoff={:?}",
                app.world().get::<PlayerPosition>(person),
                app.world().get::<NavigationRouteFailed>(person),
                app.world().get::<NavigationRouteBackoff>(person),
            );
            assert!(app.world().get::<NavigationRouteBackoff>(person).is_none());
            assert!(app.world().get::<NavigationRouteFailed>(person).is_none());
            assert!(app.world().get::<MoveTarget>(person).is_none());
            assert!(
                app.world()
                    .get::<PlayerPosition>(person)
                    .unwrap()
                    .0
                    .distance(goal)
                    < 0.2
            );
        }
    }
}

#[test]
fn a_different_destination_can_be_certified_before_the_old_backoff_expires() {
    for warp in [1., 25.] {
        let (mut app, person, start, _) = fixture(warp);
        let safe_goal = Vec3::new(-20., 4., 20.);
        app.world_mut()
            .entity_mut(person)
            .remove::<NavigationRouteFailed>()
            .insert(MoveTarget(safe_goal));
        tick(&mut app, 1. / 60.);
        for _ in 0..128 {
            if app
                .world()
                .resource::<CertifiedGoals>()
                .0
                .contains(&safe_goal)
            {
                break;
            }
            // Preserve the same simulation instant while a bounded planner
            // finishes: a new goal must not wait for the old real-time delay.
            app.update();
        }
        assert!(
            app.world()
                .resource::<CertifiedGoals>()
                .0
                .contains(&safe_goal)
        );
        assert!(app.world().get::<NavigationRouteBackoff>(person).is_none());
        assert_ne!(app.world().get::<PlayerPosition>(person).unwrap().0, start);
    }
}

#[test]
fn stale_civilian_backoff_does_not_capture_explicit_direct_motion_owners() {
    for warp in [1., 25.] {
        let (mut app, original, start, goal) = fixture(warp);
        let backoff = *app.world().get::<NavigationRouteBackoff>(original).unwrap();
        app.world_mut().despawn(original);
        // The route had failed against the old water geometry. Direct owners
        // below operate on genuinely clear ground; existing integration tests
        // separately exercise their physical doorway/queue/ambient admissions.
        open_dry_corridor(&mut app, start, goal);
        for mode in [
            "door",
            "obstacle_exit",
            "ambient",
            "queue",
            "commanded",
            "war_party",
            "formation",
            "combat_approach",
            "hero",
            "orphan_queue_transit",
        ] {
            let person = app
                .world_mut()
                .spawn((
                    if mode == "hero" {
                        CharacterKind::Hero
                    } else {
                        CharacterKind::Villager
                    },
                    PlayerPosition(start),
                    PlayerRotation(0.),
                    RegionCoord::from_world_pos(start),
                    CharacterMotion::STATIONARY,
                    MoveTarget(goal),
                    backoff,
                ))
                .id();
            match mode {
                "door" => {
                    app.world_mut()
                        .entity_mut(person)
                        .insert(BuildingDoorUse { building: goal });
                }
                "obstacle_exit" => {
                    app.world_mut()
                        .entity_mut(person)
                        .insert(NavigationObstacleEscape);
                }
                "ambient" => {
                    app.world_mut()
                        .entity_mut(person)
                        .insert(crate::world::village::ambient::AmbientDirectTransit);
                }
                "queue" => {
                    let hall = app.world_mut().spawn_empty().id();
                    let mut clock = MootQueueClock::default();
                    moot_services::enqueue_moot_service(
                        &mut app.world_mut().commands(),
                        &mut clock,
                        person,
                        hall,
                        MootServiceKind::Immigration,
                    );
                    app.world_mut().flush();
                    app.world_mut().entity_mut(person).insert(MootQueueTransit);
                }
                "commanded" => {
                    app.world_mut()
                        .entity_mut(person)
                        .insert(CommandedBy("player".into()));
                }
                "war_party" => {
                    app.world_mut()
                        .entity_mut(person)
                        .insert(super::super::combat::WarParty { banner: 1 });
                }
                "formation" => {
                    app.world_mut().entity_mut(person).insert(
                        super::super::combat::fronts::FormationMember {
                            group: 1,
                            battalion: None,
                        },
                    );
                }
                "combat_approach" => {
                    app.world_mut()
                        .entity_mut(person)
                        .insert(super::super::combat::DirectCombatApproach);
                }
                "orphan_queue_transit" => {
                    app.world_mut().entity_mut(person).insert(MootQueueTransit);
                }
                "hero" => {}
                _ => unreachable!(),
            }
            tick(&mut app, 0.01);
            let at = app.world().get::<PlayerPosition>(person).unwrap().0;
            if mode == "orphan_queue_transit" {
                assert_eq!(
                    at, start,
                    "a leftover queue marker is not a current motion owner"
                );
            } else {
                assert_ne!(at, start, "{mode} still owns direct motion at {warp}x");
            }
            assert!(app.world().get::<NavigationRouteFailed>(person).is_none());
            app.world_mut().despawn(person);
        }
    }
}
