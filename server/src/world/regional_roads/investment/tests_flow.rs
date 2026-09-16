//! Actual public observations feed the retained approval pipeline; the accepted
//! resident then uses the production road routine and ordinary position mover.

use super::*;
use crate::world::regional_roads::projects::ProjectStatus;
use shared::region::RegionCoord;
use shared::terrain::WorldTerrain;
use std::{sync::Arc, time::Duration};

fn clear_authored_terrain() -> WorldTerrain {
    use shared::map::{HeightmapData, LoadedMap, MapBounds, MapDefinition, MapTerrain};
    let bounds = MapBounds {
        min: [-160.0, -160.0],
        max: [160.0, 160.0],
    };
    let map = LoadedMap {
        definition: MapDefinition {
            map_id: "regional-road-unit".into(),
            bounds,
            terrain: MapTerrain {
                heightmap: "unused-authored-fixture".into(),
                minimap: None,
                water_level: Some(0.0),
                height_min: 8.0,
                height_max: 8.0,
            },
            generated: None,
            player_spawn: None,
            objects: Vec::new(),
            blockers: Vec::new(),
        },
        heightmap: HeightmapData::new(bounds, 2, 2, vec![8.0; 4], Some(0.0)),
        edits: Default::default(),
        terrain_deltas_by_chunk: Default::default(),
        objects_by_chunk: Default::default(),
        biome_field: None,
        rivers: Arc::new(Vec::new()),
        river_segments_by_chunk: Default::default(),
        content_hash: 1,
        map_dir: Default::default(),
    };
    WorldTerrain::from_loaded_map(map)
}

fn fixture(treasury: u64, day: u32) -> (App, Entity, Entity) {
    let mut app = App::new();
    app.init_resource::<Time>()
        .init_resource::<RegionalRoadTraffic>()
        .init_resource::<RegionalInfrastructure>()
        .init_resource::<BusinessEventQueue>()
        .insert_resource(clear_authored_terrain());
    app.add_systems(
        Update,
        (
            review_regional_investment,
            crate::world::village_roads::build_village_roads,
            advance_regional_projects,
            crate::player::hero::step_units,
        )
            .chain(),
    );
    let mut clock = WorldTime::new_default();
    clock.day = day;
    clock.set_normalized_time(0.5);
    app.world_mut().spawn((clock, TimeWarp(1.0)));
    let hall = app
        .world_mut()
        .spawn((
            SettlementId(1),
            Settlement {
                name: "Northford".into(),
                tier: SettlementTier::Village,
                residents: 2,
                treasury,
            },
            PlayerPosition(Vec3::new(0.0, 8.0, -20.0)),
            PlayerRotation(0.0),
            MootMarket::founding(),
            GoodsInventory::new(100),
            CivicAccount::default(),
        ))
        .id();
    let position = Vec3::new(0.0, 8.0, 0.0);
    let worker = app
        .world_mut()
        .spawn((
            PersonId(10),
            CharacterName("Mara".into()),
            CharacterKind::Villager,
            CharacterActivity::Idle,
            PlayerPosition(position),
            PlayerRotation(0.0),
            RegionCoord::from_world_pos(position),
            VillagerIntent::Resident { settlement: hall },
            Occupation(None),
            Wallet::new(0),
            GoodsInventory::new(24),
        ))
        .id();
    (app, hall, worker)
}

fn observe(traffic: &mut RegionalRoadTraffic, cycle: u32, day: u32, delivered: bool) {
    let leg = TradeLeg {
        route: TradeRouteId(1),
        cycle,
        stop: 1,
    };
    for x in 0..=40 {
        traffic.observe(
            leg,
            SettlementId(1),
            SettlementId(2),
            Vec2::X * x as f32 * 2.0,
            day,
        );
    }
    if delivered {
        traffic.delivered(
            leg,
            SettlementId(1),
            SettlementId(2),
            Vec2::X * 80.0,
            3,
            day,
        );
    }
    traffic.finish_leg(leg);
}

fn tick(app: &mut App) {
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_millis(100));
    let mut clocks = app.world_mut().query::<&mut WorldTime>();
    for mut clock in clocks.iter_mut(app.world_mut()) {
        clock.advance(0.1, 0.1);
    }
    app.update();
}

#[test]
fn unfulfilled_expired_or_unaffordable_traffic_cannot_reserve_a_worker_or_treasury() {
    // Promised-but-unloaded journeys, a lone actual trip, expired actual trips,
    // and a treasury without spending money are separate rejected inputs.
    for (trips, delivered, observation_day, review_day, treasury) in [
        (3, false, 0, 0, 1000),
        (1, true, 0, 0, 1000),
        (3, true, 0, 20, 1000),
        (3, true, 0, 0, 0),
    ] {
        let (mut app, hall, worker) = fixture(treasury, review_day);
        for cycle in 0..trips {
            observe(
                &mut app.world_mut().resource_mut::<RegionalRoadTraffic>(),
                cycle,
                observation_day,
                delivered,
            );
        }
        for _ in 0..100 {
            tick(&mut app);
        }
        assert_eq!(
            app.world().get::<Settlement>(hall).unwrap().treasury,
            treasury
        );
        assert!(app.world().get::<RegionalRoadWorker>(worker).is_none());
        assert_eq!(
            app.world_mut()
                .query::<&RegionalProject>()
                .iter(app.world())
                .count(),
            0
        );
        assert_eq!(
            app.world_mut()
                .query::<&VillageRoad>()
                .iter(app.world())
                .count(),
            0
        );
    }
}

#[test]
fn road_piecework_does_not_cancel_an_existing_journey() {
    let (mut app, hall, worker) = fixture(1000, 0);
    app.world_mut().entity_mut(worker).insert((
        crate::player::hero::MoveTarget(Vec3::X * 10.),
        crate::world::village_roads::TravelRoute {
            goal: Vec3::X * 10.,
            waypoints: vec![crate::world::village_roads::RouteWaypoint {
                position: Vec3::X * 10.,
                on_road: false,
            }],
            next: 0,
            geometry_version: 0,
        },
    ));
    assert!(select_worker(app.world_mut(), hall, Vec3::ZERO, 100).is_none());
    assert!(
        app.world()
            .get::<crate::world::village_roads::TravelRoute>(worker)
            .is_some()
    );
    app.world_mut().entity_mut(worker).remove::<(
        crate::player::hero::MoveTarget,
        crate::world::village_roads::TravelRoute,
    )>();
    assert_eq!(
        select_worker(app.world_mut(), hall, Vec3::ZERO, 100).map(|choice| choice.0),
        Some(worker)
    );
}

#[test]
fn repeated_delivery_evidence_funds_a_surveyed_road_and_offscreen_resident_builds_it() {
    let (mut app, hall, worker) = fixture(1000, 0);
    for cycle in 0..3 {
        observe(
            &mut app.world_mut().resource_mut::<RegionalRoadTraffic>(),
            cycle,
            0,
            true,
        );
    }
    tick(&mut app);
    assert!(
        app.world()
            .resource::<RegionalInfrastructure>()
            .pending
            .is_some(),
        "daily review must retain a survey before spending"
    );
    assert_eq!(app.world().get::<Settlement>(hall).unwrap().treasury, 1000);
    assert!(app.world().get::<RegionalRoadWorker>(worker).is_none());
    let mut project = None;
    for _ in 0..100 {
        tick(&mut app);
        project = app
            .world()
            .get::<RegionalRoadWorker>(worker)
            .map(|owner| owner.project);
        if project.is_some() {
            break;
        }
    }
    let project =
        project.expect("positive real traffic on a clear certified corridor should be investible");

    assert!(
        app.world()
            .get::<RegionalProject>(project)
            .unwrap()
            .escrow_cash
            > 0
    );
    assert!(
        app.world().get::<CivicEmployment>(worker).is_none(),
        "piecework must not create a duplicate public salary"
    );
    let mut moving = false;
    let mut working = false;
    for _ in 0..4000 {
        tick(&mut app);
        moving |= app.world().get::<PlayerPosition>(worker).unwrap().0.x > 10.0;
        working |=
            app.world().get::<CharacterActivity>(worker) == Some(&CharacterActivity::Building);
        let contract = app.world().get::<RegionalProject>(project).unwrap();
        let cash = app.world().get::<Settlement>(hall).unwrap().treasury
            + app.world().get::<Wallet>(worker).unwrap().balance()
            + contract.escrow_cash;
        assert_eq!(
            cash, 1000,
            "approval and every physical progress payment preserve all pennies"
        );
        if contract.status == ProjectStatus::Completed {
            break;
        }
        assert_eq!(contract.status, ProjectStatus::Building);
    }
    assert!(
        moving && working,
        "the production actor must walk and perform road work"
    );
    assert_eq!(
        app.world().get::<RegionalProject>(project).unwrap().status,
        ProjectStatus::Completed
    );
    assert!(app.world().get::<RegionalRoadWorker>(worker).is_none());
    assert!(app.world().get::<Wallet>(worker).unwrap().balance() > 0);
    let mut roads = app.world_mut().query::<&VillageRoad>();
    let built: Vec<_> = roads.iter(app.world()).collect();
    assert!(!built.is_empty());
    assert!(built.iter().all(|road| road.is_complete()));
    assert!(built.iter().map(|road| road.total_length()).sum::<f32>() >= 79.0);
    assert!(
        app.world()
            .resource::<RegionalInfrastructure>()
            .completed
            .contains(&SettlementPair(SettlementId(1), SettlementId(2)))
    );
}
