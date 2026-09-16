//! Recorded tree/apron geometry, exercised through real material and movement systems.
use super::*;
use crate::collision::library::{DerivedCollider, StaticColliderInstance};
use crate::world::pathfinding::PathfindingBudgetSettings;
use crate::world::village_roads::{
    VillageRoadGraph, plan_villager_travel_routes, queue_villager_travel_routes,
    retry_failed_routes_after_obstacle_change,
};
use shared::components::{PersonId, SettlementId, TimeWarp};
use shared::props::PropKind;
use shared::region::RegionCoord;

struct Fixture {
    app: App,
    clock: Entity,
    builder: Entity,
    site: Entity,
    apron: Vec3,
    start: Vec3,
    stand: Option<Vec3>,
    required: u32,
}

fn fixture(kind: PropKind, offset: Vec2, inflated_radius: f32, wall: bool, warp: f32) -> Fixture {
    let apron = Vec3::new(1700., 80., 0.);
    let start = apron - Vec3::X * 8.;
    let mut terrain = WorldTerrain::default();
    terrain.apply_flatten_rect(apron, Vec2::splat(80.), 0., 4.);
    let access = PlannedRoadAccess {
        settlement_id: SettlementId(1),
        points: vec![
            apron.xz() - Vec2::Y * 2.25,
            apron.xz(),
            apron.xz() + Vec2::Y * 8.,
            apron.xz() + Vec2::new(-120., 8.),
        ],
        half_width: 1.,
    };
    let tree = apron + Vec3::new(offset.x, 0., offset.y);
    let cell = ((tree.x / 16.).floor() as i32, (tree.z / 16.).floor() as i32);
    let mut colliders = StaticColliders::default();
    colliders.instances.insert(
        1,
        StaticColliderInstance {
            kind,
            position: tree,
            rotation: Quat::IDENTITY,
            scale: 1.,
            cell,
        },
    );
    colliders.cells.insert(cell, vec![1]);
    let centre = shared::terrain::ChunkCoord::new(
        (apron.x / shared::terrain::CHUNK_SIZE).floor() as i32,
        (apron.z / shared::terrain::CHUNK_SIZE).floor() as i32,
    );
    for x in -3..=3 {
        for z in -3..=3 {
            colliders
                .loaded_chunks
                .insert(shared::terrain::ChunkCoord::new(centre.x + x, centre.z + z));
        }
    }
    let derived = DerivedColliderLibrary {
        by_kind: std::collections::HashMap::from([(
            kind,
            DerivedCollider {
                horizontal_radius: inflated_radius - crate::world::navgrid::VILLAGER_PROP_RADIUS,
            },
        )]),
    };
    let mut obstacles = SpatialObstacleGrid::default();
    if wall {
        obstacles.insert(shared::spatial::ObstacleEntry {
            center: apron.xz(),
            half_extents: Vec2::splat(3.),
            rotation: 0.,
            obstacle_type: 0,
        });
    }
    let routes = MaterialRouteContext {
        terrain: &terrain,
        obstacles: Some(&obstacles),
        colliders: Some(&colliders),
        derived: Some(&derived),
        now: 0.,
    };
    assert!(!routes.plausible_local_join(apron.xz(), apron.xz()));
    let stand = routes.delivery_stand(&access);
    let mut app = App::new();
    app.init_resource::<Time>()
        .init_resource::<BusinessEventQueue>()
        .init_resource::<VillageRoadGraph>()
        .init_resource::<PathfindingBudgetSettings>();
    let clock = app
        .world_mut()
        .spawn((WorldTime::new_default(), TimeWarp(warp)))
        .id();
    let hall = app
        .world_mut()
        .spawn((
            SettlementId(1),
            Settlement {
                name: "Frontage test".into(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            },
            PlayerPosition(apron - Vec3::X * 120.),
        ))
        .id();
    let mut cargo = GoodsInventory::new(shared::economy::capacity::VILLAGER);
    assert_eq!(cargo.add(Good::Wood, 4), 4);
    assert_eq!(cargo.add(Good::Wool, 1), 1);
    let builder = app
        .world_mut()
        .spawn((
            PersonId(1),
            CharacterName("Frontage builder".into()),
            CharacterKind::Villager,
            PlayerPosition(start),
            PlayerRotation(0.),
            RegionCoord::from_world_pos(start),
            CharacterActivity::Idle,
            cargo,
        ))
        .id();
    let building = SettlementBuildingKind::House;
    let required = building.construction_wood_required();
    let mut store = GoodsInventory::new(building.construction_storage_bulk());
    store.add(Good::Wood, required - 4);
    let plot = apron - Vec3::Z * 2.25;
    let site = app
        .world_mut()
        .spawn((
            UnderConstruction {
                kind: building,
                position: plot,
                rotation: 0.,
                owner: Some("Frontage builder".into()),
                owner_id: Some(PersonId(1)),
                builder: Some(builder),
                settlement: hall,
                settlement_id: SettlementId(1),
                stand: apron,
                failed_stand_routes: 0,
                stage: BuildStage::Supplying,
                quality: 0.5,
            },
            PlayerPosition(plot),
            store,
            access.clone(),
        ))
        .id();
    let mut routine = ConstructionMaterialRoutine::new(site);
    routine.phase = begin_material_delivery(
        &mut app.world_mut().commands(),
        builder,
        start,
        apron,
        &routes,
        Some(&access),
        false,
    );
    app.world_mut().flush();
    app.world_mut().entity_mut(builder).insert((
        routine,
        VillagerIntent::Building {
            settlement: hall,
            site,
        },
    ));
    app.insert_resource(terrain)
        .insert_resource(colliders)
        .insert_resource(derived)
        .insert_resource(obstacles)
        .add_systems(
            Update,
            (
                run_construction_material_logistics,
                retry_failed_routes_after_obstacle_change,
                queue_villager_travel_routes,
                plan_villager_travel_routes,
                crate::player::hero::step_units,
            )
                .chain(),
        );
    Fixture {
        app,
        clock,
        builder,
        site,
        apron,
        start,
        stand,
        required,
    }
}

fn tick(f: &mut Fixture, warp: f32) {
    f.app
        .world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f32(1. / 60.));
    f.app
        .world_mut()
        .get_mut::<WorldTime>(f.clock)
        .unwrap()
        .advance(1. / 60., warp);
    f.app.update();
}

#[test]
fn recorded_tree_aprons_deliver_only_after_real_arrival_at_a_clear_local_stand() {
    for warp in [1., 25.] {
        for (kind, offset, radius) in [
            (PropKind::PineB, Vec2::new(1.598, 0.789), 2.33984),
            (PropKind::PineYoungB, Vec2::new(-1.189, 0.2233), 1.5440257),
        ] {
            let mut f = fixture(kind, offset, radius, false, warp);
            let stand = f
                .stand
                .expect("recorded apron must have a close legal alternative");
            assert!(ground_distance(stand, f.apron) <= WORK_REACH);
            assert!(ground_distance(stand, f.apron) > 0.1);
            assert_eq!(f.app.world().get::<MoveTarget>(f.builder).unwrap().0, stand);
            let mut actually_arrived = false;
            for _ in 0..600 {
                let before = f.app.world().get::<PlayerPosition>(f.builder).unwrap().0;
                tick(&mut f, warp);
                let body = f.app.world().get::<PlayerPosition>(f.builder).unwrap().0;
                actually_arrived |= ground_distance(body, stand) <= 0.1;
                let held = f.app.world().get::<GoodsInventory>(f.builder).unwrap();
                assert_eq!(held.amount(Good::Wool), 1);
                if ground_distance(before, stand) > 0.1 {
                    assert_eq!(
                        held.amount(Good::Wood),
                        4,
                        "cargo may not deposit before exact movement arrival"
                    );
                    assert_eq!(
                        f.app
                            .world()
                            .get::<GoodsInventory>(f.site)
                            .unwrap()
                            .amount(Good::Wood),
                        f.required - 4
                    );
                }
                assert!(
                    body.xz().distance(f.apron.xz()) < 20.,
                    "local timber must never return to the Hall"
                );
                if let Some(target) = f.app.world().get::<MoveTarget>(f.builder) {
                    assert!(target.0.xz().distance(f.apron.xz()) < 20.);
                }
                assert!(
                    f.app
                        .world()
                        .get::<NavigationRouteFailed>(f.builder)
                        .is_none()
                );
                if f.app
                    .world()
                    .get::<ConstructionMaterialRoutine>(f.builder)
                    .is_none()
                {
                    break;
                }
            }
            assert!(actually_arrived);
            assert!(
                f.app
                    .world()
                    .get::<ConstructionMaterialRoutine>(f.builder)
                    .is_none()
            );
            assert_eq!(
                f.app
                    .world()
                    .get::<GoodsInventory>(f.builder)
                    .unwrap()
                    .amount(Good::Wood),
                0
            );
            assert_eq!(
                f.app
                    .world()
                    .get::<GoodsInventory>(f.site)
                    .unwrap()
                    .amount(Good::Wood),
                f.required
            );
            assert!(
                ground_distance(
                    f.app
                        .world()
                        .get::<UnderConstruction>(f.site)
                        .unwrap()
                        .stand,
                    stand
                ) <= 0.1
            );
        }
    }
}

#[test]
fn fully_blocked_frontage_waits_with_cargo_and_without_an_invalid_target() {
    for warp in [1., 25.] {
        for wall in [false, true] {
            let mut f = fixture(
                PropKind::PineB,
                Vec2::ZERO,
                if wall { 0.6 } else { 5. },
                wall,
                warp,
            );
            assert!(f.stand.is_none());
            for _ in 0..100 {
                tick(&mut f, warp);
                assert_eq!(
                    f.app.world().get::<PlayerPosition>(f.builder).unwrap().0,
                    f.start
                );
                assert_eq!(
                    f.app
                        .world()
                        .get::<GoodsInventory>(f.builder)
                        .unwrap()
                        .amount(Good::Wood),
                    4
                );
                assert_eq!(
                    f.app
                        .world()
                        .get::<GoodsInventory>(f.builder)
                        .unwrap()
                        .amount(Good::Wool),
                    1
                );
                assert_eq!(
                    f.app
                        .world()
                        .get::<GoodsInventory>(f.site)
                        .unwrap()
                        .amount(Good::Wood),
                    f.required - 4
                );
                assert!(f.app.world().get::<MoveTarget>(f.builder).is_none());
                assert!(f.app.world().get::<TravelRoute>(f.builder).is_none());
                assert!(matches!(
                    f.app
                        .world()
                        .get::<ConstructionMaterialRoutine>(f.builder)
                        .unwrap()
                        .phase,
                    ConstructionMaterialPhase::WaitingForDeliveryAccess { .. }
                ));
            }
        }
    }
}

#[test]
fn hall_pickup_releases_its_ready_ticket_while_frontage_waits_then_delivers_once_clear() {
    for warp in [1., 25.] {
        let mut f = fixture(PropKind::PineB, Vec2::ZERO, 0.6, true, warp);
        let hall = f
            .app
            .world()
            .get::<UnderConstruction>(f.site)
            .unwrap()
            .settlement;
        // Stage the ordinary counter at this fixture's initial body position;
        // after simulation begins no position or cargo is assigned by the test.
        let entrance_offset = SettlementBuildingKind::Hall.entrance_position(Vec3::ZERO, 0.);
        f.app.world_mut().entity_mut(hall).insert((
            PlayerPosition(f.start - entrance_offset),
            PlayerRotation(0.),
        ));
        f.app
            .world_mut()
            .get_mut::<GoodsInventory>(f.builder)
            .unwrap()
            .remove(Good::Wood, 4);
        f.app
            .world_mut()
            .entity_mut(f.builder)
            .insert(Wallet::new(100));
        let mut stock = GoodsInventory::new(shared::economy::capacity::HALL);
        stock.add(Good::Wood, 4);
        let mut market = MootMarket::founding();
        market.consign(
            shared::economy::MarketSeller::Treasury(SettlementId(1)),
            Good::Wood,
            4,
            20,
        );
        f.app.world_mut().entity_mut(hall).insert((stock, market));
        f.app
            .world_mut()
            .get_mut::<ConstructionMaterialRoutine>(f.builder)
            .unwrap()
            .phase = ConstructionMaterialPhase::CollectingFromStore {
            source: hall,
            entrance: f.start,
            reserved_units: 4,
        };
        f.app.init_resource::<MootQueueClock>().add_systems(
            Update,
            advance_moot_service_queues.before(run_construction_material_logistics),
        );
        f.app
            .world_mut()
            .resource_scope(|world, mut clock: Mut<MootQueueClock>| {
                enqueue_moot_service(
                    &mut world.commands(),
                    &mut clock,
                    f.builder,
                    hall,
                    MootServiceKind::ConstructionMaterial,
                );
            });
        f.app.world_mut().flush();
        for _ in 0..1200 {
            tick(&mut f, warp);
            if f.app
                .world()
                .get::<GoodsInventory>(f.builder)
                .unwrap()
                .amount(Good::Wood)
                == 4
            {
                break;
            }
        }
        assert_eq!(
            f.app
                .world()
                .get::<GoodsInventory>(f.builder)
                .unwrap()
                .amount(Good::Wood),
            4
        );
        assert_eq!(
            f.app
                .world()
                .get::<GoodsInventory>(hall)
                .unwrap()
                .amount(Good::Wood),
            0
        );
        assert_eq!(
            f.app
                .world()
                .get::<MootMarket>(hall)
                .unwrap()
                .listed_units(Good::Wood),
            0
        );
        assert_eq!(
            f.app.world().get::<Wallet>(f.builder).unwrap().balance(),
            20
        );
        assert_eq!(
            f.app
                .world()
                .resource::<BusinessEventQueue>()
                .pending_sale_gross(),
            80
        );
        assert!(
            f.app
                .world()
                .get::<MootQueueTicket>(f.builder)
                .unwrap()
                .is_ready(),
            "the test must cover actual successful service, not a fabricated wait marker"
        );
        let retry_after = match f
            .app
            .world()
            .get::<ConstructionMaterialRoutine>(f.builder)
            .unwrap()
            .phase
        {
            ConstructionMaterialPhase::WaitingForDeliveryAccess { retry_after, .. } => retry_after,
            ref phase => panic!("blocked frontage did not preserve the purchased load: {phase:?}"),
        };
        tick(&mut f, warp);
        assert!(
            f.app.world().get::<MootQueueTicket>(f.builder).is_none(),
            "a safely waiting carrier must release the Ready freight queue head"
        );
        assert!(f.app.world().get::<MootQueueTransit>(f.builder).is_none());
        assert!(f.app.world().get::<MoveTarget>(f.builder).is_none());
        assert_eq!(
            f.app
                .world()
                .get::<GoodsInventory>(f.site)
                .unwrap()
                .amount(Good::Wood),
            f.required - 4
        );

        f.app
            .world_mut()
            .resource_mut::<SpatialObstacleGrid>()
            .clear();
        // Advance only the wait deadline. The following route and all transfer
        // acknowledgements still execute through the normal fixed-size steps.
        f.app
            .world_mut()
            .get_mut::<WorldTime>(f.clock)
            .unwrap()
            .seconds_in_cycle = retry_after as f32 + 0.01;
        for _ in 0..600 {
            tick(&mut f, warp);
            assert!(f.app.world().get::<MootQueueTicket>(f.builder).is_none());
            assert_eq!(
                f.app.world().get::<Wallet>(f.builder).unwrap().balance(),
                20
            );
            assert_eq!(
                f.app
                    .world()
                    .resource::<BusinessEventQueue>()
                    .pending_sale_gross(),
                80
            );
            if f.app
                .world()
                .get::<ConstructionMaterialRoutine>(f.builder)
                .is_none()
            {
                break;
            }
        }
        assert!(
            f.app
                .world()
                .get::<ConstructionMaterialRoutine>(f.builder)
                .is_none()
        );
        assert_eq!(
            f.app
                .world()
                .get::<GoodsInventory>(f.builder)
                .unwrap()
                .amount(Good::Wood),
            0
        );
        assert_eq!(
            f.app
                .world()
                .get::<GoodsInventory>(f.builder)
                .unwrap()
                .amount(Good::Wool),
            1
        );
        assert_eq!(
            f.app
                .world()
                .get::<GoodsInventory>(f.site)
                .unwrap()
                .amount(Good::Wood),
            f.required
        );
        assert!(
            ground_distance(
                f.app.world().get::<PlayerPosition>(f.builder).unwrap().0,
                f.apron
            ) <= WORK_REACH
        );
    }
}
