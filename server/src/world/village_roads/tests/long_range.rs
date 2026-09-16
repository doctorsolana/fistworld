use super::*;
use shared::{
    components::RoadBridge,
    terrain::{CHUNK_RESOLUTION, TerrainDeltaData, VERTEX_SPACING},
};

fn sloped_terrain() -> WorldTerrain {
    let mut terrain = WorldTerrain::default();
    for x in -1..=3 {
        for z in -1..=1 {
            let coord = ChunkCoord::new(x, z);
            let origin = coord.world_pos();
            let mut data = TerrainDeltaData::default();
            for zi in 0..CHUNK_RESOLUTION {
                for xi in 0..CHUNK_RESOLUTION {
                    let px = origin.x + xi as f32 * VERTEX_SPACING;
                    let pz = origin.z + zi as f32 * VERTEX_SPACING;
                    data.add_vertex(
                        xi,
                        zi,
                        100.0 + px * 0.25 - terrain.generator.get_height(px, pz),
                    );
                }
            }
            terrain.set_delta_chunk(coord, data);
        }
    }
    terrain
}

#[test]
fn regional_stride_crosses_the_same_gentle_hill_as_fine_navigation() {
    let terrain = sloped_terrain();
    let props = PropBlockers::default();
    for stride in [1, 4, 8] {
        let survey = RoadSurvey {
            terrain: &terrain,
            decks: None,
            buildings: &[],
            live_buildings: None,
            props: &props,
            start: Vec2::new(0., 0.),
            goal: Vec2::new(180., 0.),
            min: Vec2::new(-10., -10.),
            max: Vec2::new(190., 10.),
            max_nodes: 2_400,
            cell_size: SURVEY_CELL,
            coarse_stride: stride,
            fine_endpoint_radius: 12.,
        };
        let mut scratch = SurveyScratch::default();
        // Deliberately exercise A*: the old straight-line fast path masked the
        // absolute-rise defect until one obstruction required a real search.
        let path = survey_a_star(&survey, &mut scratch);
        assert!(!path.is_empty(), "stride {stride} rejected a 25% grade");
        assert!(
            path.windows(2)
                .all(|pair| survey.line_clear(pair[0], pair[1], &mut scratch))
        );
        assert!(scratch.metrics.expanded_nodes <= survey.max_nodes as u64);
    }
}

fn bridge_fixture() -> (WorldTerrain, RoadBridge) {
    let mut terrain = WorldTerrain::default();
    let water = terrain
        .water_surface_height(1700., 0.)
        .expect("water-capable fixture");
    terrain.apply_flatten_rect(
        Vec3::new(1700., water + 1., 0.),
        Vec2::new(60., 80.),
        0.,
        0.,
    );
    terrain.apply_flatten_rect(
        Vec3::new(1700., water - 8., 0.),
        Vec2::new(10., 80.),
        0.,
        0.,
    );
    let bridge = RoadBridge {
        start: Vec3::new(1680., water + 1., 0.),
        end: Vec3::new(1720., water + 1., 0.),
        deck_height: water + 4.,
        ramp_length: 8.,
        width: 3.6,
        built: true,
    };
    assert!(bridge.valid());
    assert!(terrain.get_water_height(1700., 0.).is_some());
    (terrain, bridge)
}

#[test]
fn only_a_completed_bridge_connects_road_graph_and_actual_walking_surface() {
    let (terrain, mut bridge) = bridge_fixture();
    let start = bridge.start;
    let end = bridge.end;
    let mut app = App::new();
    app.insert_resource(terrain)
        .init_resource::<BridgeDecks>()
        .init_resource::<VillageRoadGraph>();
    app.add_systems(
        Update,
        (
            crate::world::bridges::rebuild_bridge_decks,
            rebuild_village_road_graph,
        )
            .chain(),
    );
    bridge.built = false;
    let bridge_entity = app.world_mut().spawn(bridge).id();
    app.update();
    assert!(app.world().resource::<VillageRoadGraph>().nodes.is_empty());
    assert!(!crate::world::bridges::segment_walkable(
        app.world().resource::<WorldTerrain>(),
        Some(app.world().resource::<BridgeDecks>()),
        start.xz(),
        end.xz(),
        CHARACTER_NAV_RADIUS
    ));
    app.world_mut()
        .get_mut::<RoadBridge>(bridge_entity)
        .unwrap()
        .built = true;
    app.update();
    assert_eq!(
        app.world_mut()
            .resource_mut::<VillageRoadGraph>()
            .shortest_path(0, 1),
        Some(vec![0, 1])
    );
    let terrain = app.world().resource::<WorldTerrain>();
    let decks = app.world().resource::<BridgeDecks>();
    assert!(crate::world::bridges::segment_walkable(
        terrain,
        Some(decks),
        start.xz(),
        end.xz(),
        CHARACTER_NAV_RADIUS
    ));
    assert!(
        !crate::world::bridges::segment_walkable(
            terrain,
            Some(decks),
            start.xz() + Vec2::Y * 1.6,
            end.xz() + Vec2::Y * 1.6,
            CHARACTER_NAV_RADIUS
        ),
        "water beside a bridge remains water"
    );
    let props = PropBlockers::default();
    let survey = RoadSurvey {
        terrain,
        decks: Some(decks),
        buildings: &[],
        live_buildings: None,
        props: &props,
        start: start.xz(),
        goal: end.xz(),
        min: start.xz() - Vec2::splat(5.),
        max: end.xz() + Vec2::splat(5.),
        max_nodes: 2_400,
        cell_size: SURVEY_CELL,
        coarse_stride: 4,
        fine_endpoint_radius: 8.,
    };
    let mut scratch = SurveyScratch::default();
    assert!(
        !survey_a_star(&survey, &mut scratch).is_empty(),
        "the live fine bridge approach must fit the existing budget"
    );
    // Exact route proof must disappear with the actual deck.
    app.world_mut()
        .resource_mut::<VillageRoadGraph>()
        .cache_tactical_route(
            start.xz(),
            end.xz(),
            &[(start.xz(), true), (end.xz(), true)],
            true,
        );
    app.world_mut().despawn(bridge_entity);
    app.update();
    assert!(app.world().resource::<VillageRoadGraph>().nodes.is_empty());
    assert!(
        app.world()
            .resource::<VillageRoadGraph>()
            .tactical_route(start.xz(), end.xz())
            .is_none()
    );
}

#[test]
fn warped_villager_uses_deck_height_and_cannot_cross_after_its_removal() {
    let (terrain, bridge) = bridge_fixture();
    let mut app = App::new();
    app.insert_resource(terrain).init_resource::<BridgeDecks>();
    app.add_systems(
        Update,
        (
            crate::world::bridges::rebuild_bridge_decks,
            crate::player::hero::step_units,
        )
            .chain(),
    );
    app.world_mut()
        .spawn(shared::components::TimeWarp::clamped(100.));
    let bridge_entity = app.world_mut().spawn(bridge.clone()).id();
    let actor = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            PlayerPosition(bridge.start),
            PlayerRotation(0.),
            shared::region::RegionCoord::default(),
            MoveTarget(bridge.end),
            TravelRoute {
                goal: bridge.end,
                waypoints: vec![RouteWaypoint {
                    position: bridge.end,
                    on_road: true,
                }],
                next: 0,
                geometry_version: 0,
            },
        ))
        .id();
    let mut saw_plateau = false;
    for _ in 0..30 {
        app.update();
        let p = app.world().get::<PlayerPosition>(actor).unwrap().0;
        if p.x > 1690. && p.x < 1710. {
            assert!((p.y - bridge.deck_height).abs() < 0.01);
            saw_plateau = true;
        }
        if app.world().get::<MoveTarget>(actor).is_none() {
            break;
        }
    }
    assert!(saw_plateau);
    assert!(
        app.world()
            .get::<PlayerPosition>(actor)
            .unwrap()
            .0
            .distance(bridge.end)
            < 0.01
    );
    app.world_mut().despawn(bridge_entity);
    app.world_mut().entity_mut(actor).insert((
        PlayerPosition(bridge.start),
        MoveTarget(bridge.end),
        TravelRoute {
            goal: bridge.end,
            waypoints: vec![RouteWaypoint {
                position: bridge.end,
                on_road: true,
            }],
            next: 0,
            geometry_version: 0,
        },
    ));
    for _ in 0..30 {
        app.update();
    }
    assert!(
        app.world().get::<PlayerPosition>(actor).unwrap().0.x < 1690.,
        "missing bridge allowed a full warped crossing"
    );
    assert!(app.world().get::<NavigationRouteFailed>(actor).is_some());
}

#[test]
fn completed_decks_on_both_sides_do_not_cover_a_gap_between_them() {
    let (terrain, bridge) = bridge_fixture();
    let mut left = bridge.clone();
    left.end.x = 1698.;
    let mut right = bridge.clone();
    right.start.x = 1702.;
    assert!(left.valid() && right.valid());
    let mut app = App::new();
    app.init_resource::<BridgeDecks>()
        .add_systems(Update, crate::world::bridges::rebuild_bridge_decks);
    app.world_mut().spawn(left);
    app.world_mut().spawn(right);
    app.update();
    let decks = app.world().resource::<BridgeDecks>();
    assert!(
        decks
            .height_at(bridge.start.xz(), CHARACTER_NAV_RADIUS)
            .is_some()
    );
    assert!(
        decks
            .height_at(bridge.end.xz(), CHARACTER_NAV_RADIUS)
            .is_some()
    );
    assert!(
        decks
            .height_at(Vec2::new(1700., 0.), CHARACTER_NAV_RADIUS)
            .is_none()
    );
    assert!(!crate::world::bridges::segment_walkable(
        &terrain,
        Some(decks),
        bridge.start.xz(),
        bridge.end.xz(),
        CHARACTER_NAV_RADIUS
    ));
}

#[test]
fn bridge_hero_keeps_land_speed_while_swimmer_below_stays_in_water() {
    let (terrain, bridge) = bridge_fixture();
    let water_y = shared::character::locomotion::swimming_surface(
        terrain.get_height(1700., 0.),
        terrain.get_water_height(1700., 0.),
    )
    .unwrap();
    let mut app = App::new();
    app.insert_resource(terrain)
        .init_resource::<BridgeDecks>()
        .add_systems(
            Update,
            (
                crate::world::bridges::rebuild_bridge_decks,
                crate::player::hero::step_units,
            )
                .chain(),
        );
    app.world_mut().spawn(bridge.clone());
    let mut actors = Vec::new();
    for y in [bridge.deck_height, water_y] {
        actors.push(
            app.world_mut()
                .spawn((
                    CharacterKind::Hero,
                    PlayerPosition(Vec3::new(1700., y, 0.)),
                    PlayerRotation(0.),
                    shared::region::RegionCoord::default(),
                    shared::components::CharacterMotion::STATIONARY,
                    MoveTarget(Vec3::new(1702., y, 0.)),
                ))
                .id(),
        );
    }
    app.update();
    for (actor, expected_speed, expected_y) in [
        (
            actors[0],
            shared::player::HERO_MOVE_SPEED,
            bridge.deck_height,
        ),
        (
            actors[1],
            shared::character::locomotion::SWIM_SPEED,
            water_y,
        ),
    ] {
        let position = app.world().get::<PlayerPosition>(actor).unwrap().0;
        let speed = app
            .world()
            .get::<shared::components::CharacterMotion>(actor)
            .unwrap()
            .velocity
            .length();
        assert!(
            (speed - expected_speed).abs() < 0.02,
            "actual movement speed {speed}, expected {expected_speed}"
        );
        assert!(
            (position.y - expected_y).abs() < 0.01,
            "walking and swimming must keep separate vertical surfaces"
        );
        assert!(position.x > 1700.);
    }
}

#[test]
fn a_large_warped_swim_cannot_snap_up_through_a_bridge() {
    let (terrain, bridge) = bridge_fixture();
    let water_y = shared::character::locomotion::swimming_surface(
        terrain.get_height(1700., 0.),
        terrain.get_water_height(1700., 0.),
    )
    .unwrap();
    let mut app = App::new();
    app.insert_resource(terrain)
        .init_resource::<BridgeDecks>()
        .add_systems(
            Update,
            (
                crate::world::bridges::rebuild_bridge_decks,
                crate::player::hero::step_units,
            )
                .chain(),
        );
    app.world_mut().spawn(bridge);
    app.world_mut()
        .spawn(shared::components::TimeWarp::clamped(500.));
    let swimmer = app
        .world_mut()
        .spawn((
            CharacterKind::Hero,
            PlayerPosition(Vec3::new(1697., water_y, 0.)),
            PlayerRotation(0.),
            shared::region::RegionCoord::default(),
            MoveTarget(Vec3::new(1703., water_y, 0.)),
        ))
        .id();
    app.update();
    let actual = app.world().get::<PlayerPosition>(swimmer).unwrap().0;
    assert!(
        actual.x > 1702.9,
        "fixture must exercise a large crossing within one tick"
    );
    assert!(
        (actual.y - water_y).abs() < 0.01,
        "swimming below a completed deck must retain the water surface"
    );
}
