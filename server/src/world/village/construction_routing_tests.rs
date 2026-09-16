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
        now: 0.,
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
fn uncleared_tree_on_reserved_access_requests_a_real_detour_without_moving_cargo() {
    use crate::collision::library::{DerivedCollider, StaticColliderInstance};
    use shared::props::PropKind;
    let (terrain, access, _) = fixture();
    let tree = (access.points[1] + access.points[2]) * 0.5;
    let cell = (
        (tree.x / 16.0).floor() as i32,
        (tree.y / 16.0).floor() as i32,
    );
    let kind = PropKind::BroadleafNarrowA;
    let mut colliders = StaticColliders::default();
    colliders.cells.insert(cell, vec![1]);
    colliders.instances.insert(
        1,
        StaticColliderInstance {
            kind,
            position: Vec3::new(tree.x, 80.0, tree.y),
            rotation: Quat::IDENTITY,
            scale: 1.0,
            cell,
        },
    );
    let derived = DerivedColliderLibrary {
        by_kind: [(
            kind,
            DerivedCollider {
                horizontal_radius: 0.8,
            },
        )]
        .into_iter()
        .collect(),
    };
    let routes = MaterialRouteContext {
        colliders: Some(&colliders),
        derived: Some(&derived),
        ..context(&terrain)
    };
    let entry = routes.point(access.points[2]);
    let apron = routes.point(access.points[1]);
    let mut world = World::new();
    let mut cargo = GoodsInventory::new(shared::economy::capacity::VILLAGER);
    cargo.add(Good::Wood, 6);
    let builder = world.spawn(cargo).id();
    let phase = finish_material_approach(
        &mut world.commands(),
        builder,
        entry,
        &routes,
        Some(&access),
    );
    world.flush();
    assert!(
        matches!(phase, ConstructionMaterialPhase::Delivering { destination } if destination == apron)
    );
    assert_eq!(world.get::<MoveTarget>(builder).unwrap().0, apron);
    assert!(
        world.get::<TravelRoute>(builder).is_none(),
        "uncleared access is not an authorized route"
    );
    assert_eq!(
        world
            .get::<GoodsInventory>(builder)
            .unwrap()
            .amount(Good::Wood),
        6
    );

    let phase = begin_material_egress(
        &mut world.commands(),
        builder,
        apron,
        &routes,
        Some(&access),
        Some(entry),
    );
    world.flush();
    assert!(
        matches!(phase, ConstructionMaterialPhase::LeavingDeliveryAccess { exit } if exit == entry)
    );
    assert_eq!(world.get::<MoveTarget>(builder).unwrap().0, entry);
    assert!(
        world.get::<TravelRoute>(builder).is_none(),
        "return journey also needs a live detour"
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

#[test]
fn material_runner_waits_for_real_arrival_then_uses_corridor_despite_changed_ground_height() {
    use bevy::ecs::system::RunSystemOnce;
    use shared::components::{PersonId, SettlementId, TimeWarp};
    use shared::region::RegionCoord;

    for warp in [1.0, 25.0] {
        let (terrain, access, _) = fixture();
        let routes = context(&terrain);
        let grounded_entry = routes.point(access.points[2]);
        // The route was authored before nearby earthworks changed height.
        // Movement owns XZ arrival and re-grounds the body on current terrain.
        let stale_entry = grounded_entry + Vec3::Y * 7.0;
        let apron = routes.point(access.points[1]);
        let site_position = routes.point(access.points[0]);
        let mut app = App::new();
        app.insert_resource(terrain)
            .init_resource::<Time>()
            .init_resource::<BusinessEventQueue>();
        app.world_mut()
            .spawn((WorldTime::new_default(), TimeWarp(warp)));
        let hall = app
            .world_mut()
            .spawn((
                SettlementId(1),
                Settlement {
                    name: "Arrival".into(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 1,
                    treasury: 0,
                },
                PlayerPosition(grounded_entry + Vec3::X * 20.0),
            ))
            .id();
        let mut cargo = GoodsInventory::new(shared::economy::capacity::VILLAGER);
        assert_eq!(cargo.add(Good::Wood, 4), 4);
        let builder = app
            .world_mut()
            .spawn((
                CharacterName("Grounded builder".into()),
                CharacterKind::Villager,
                PersonId(1),
                PlayerPosition(grounded_entry),
                PlayerRotation(0.0),
                RegionCoord::from_world_pos(grounded_entry),
                CharacterActivity::Idle,
                cargo,
                MoveTarget(stale_entry),
                ConstructionDeliveryAccess { entry: stale_entry },
                TravelRoute {
                    goal: stale_entry,
                    waypoints: vec![RouteWaypoint {
                        position: stale_entry,
                        on_road: false,
                    }],
                    next: 0,
                    geometry_version: 0,
                },
            ))
            .id();
        let kind = SettlementBuildingKind::House;
        let mut site_store = GoodsInventory::new(kind.construction_storage_bulk());
        let required = kind.construction_wood_required();
        assert_eq!(site_store.add(Good::Wood, required - 4), required - 4);
        let site = app
            .world_mut()
            .spawn((
                UnderConstruction {
                    kind,
                    position: site_position,
                    rotation: 0.0,
                    owner: Some("Grounded builder".into()),
                    owner_id: Some(PersonId(1)),
                    builder: Some(builder),
                    settlement: hall,
                    settlement_id: SettlementId(1),
                    stand: apron,
                    failed_stand_routes: 0,
                    stage: BuildStage::Supplying,
                    quality: 0.5,
                },
                PlayerPosition(site_position),
                site_store,
                access.clone(),
            ))
            .id();
        let mut routine = ConstructionMaterialRoutine::new(site);
        routine.phase = ConstructionMaterialPhase::ApproachingDeliveryAccess { entry: stale_entry };
        app.world_mut().entity_mut(builder).insert((
            VillagerIntent::Building {
                settlement: hall,
                site,
            },
            routine,
        ));
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(1.0 / 60.0));

        app.world_mut()
            .run_system_once(run_construction_material_logistics)
            .unwrap();
        assert!(
            matches!(
                app.world()
                    .get::<ConstructionMaterialRoutine>(builder)
                    .unwrap()
                    .phase,
                ConstructionMaterialPhase::ApproachingDeliveryAccess { .. }
            ),
            "the material runner cannot claim movement's unfinished target"
        );
        app.world_mut()
            .run_system_once(crate::player::hero::step_units)
            .unwrap();
        assert!(
            app.world().get::<MoveTarget>(builder).is_none(),
            "real movement must acknowledge XZ arrival despite stale route height"
        );
        assert!(
            (app.world().get::<PlayerPosition>(builder).unwrap().0.y - grounded_entry.y).abs()
                < 0.01
        );
        app.world_mut()
            .run_system_once(run_construction_material_logistics)
            .unwrap();
        assert!(matches!(
            app.world()
                .get::<ConstructionMaterialRoutine>(builder)
                .unwrap()
                .phase,
            ConstructionMaterialPhase::Delivering { .. }
        ));
        let retained = app.world().get::<TravelRoute>(builder).unwrap();
        assert_eq!(
            retained
                .waypoints
                .iter()
                .map(|point| point.position.xz())
                .collect::<Vec<_>>(),
            vec![access.points[2], access.points[1]],
            "arrival authorizes only the already reserved corridor suffix"
        );
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(builder)
                .unwrap()
                .amount(Good::Wood),
            4
        );
        app.add_systems(
            Update,
            (
                run_construction_material_logistics,
                crate::player::hero::step_units,
            )
                .chain(),
        );
        for _ in 0..400 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(1.0 / 60.0));
            app.update();
            if app
                .world()
                .get::<ConstructionMaterialRoutine>(builder)
                .is_none()
            {
                break;
            }
        }
        assert!(
            app.world()
                .get::<ConstructionMaterialRoutine>(builder)
                .is_none()
        );
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(site)
                .unwrap()
                .amount(Good::Wood),
            required
        );
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(builder)
                .unwrap()
                .amount(Good::Wood),
            0
        );
        assert!(
            ground_distance(app.world().get::<PlayerPosition>(builder).unwrap().0, apron)
                <= WORK_REACH
        );
    }
}

#[test]
fn paid_meals_and_material_deliveries_finish_without_competing_targets_or_lost_cargo() {
    use bevy::ecs::system::RunSystemOnce;
    use shared::components::{CharacterObjective, PersonId, ResidentOf, SettlementId, TimeWarp};
    use shared::region::RegionCoord;

    fn tick(app: &mut App, warp: f32) {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(0.1 / warp));
        app.update();
    }

    for warp in [1.0, 25.0] {
        for (already_carrying_meal, unloading_other_goods) in
            [(false, false), (true, false), (true, true)]
        {
            let (terrain, mut access, _) = fixture();
            let routes = context(&terrain);
            let initial_entry = routes.point(access.points[2]);
            let apron = routes.point(access.points[1]);
            let site_position = routes.point(access.points[0]);
            let mut app = App::new();
            app.insert_resource(terrain)
                .init_resource::<Time>()
                .init_resource::<BusinessEventQueue>()
                .init_resource::<SettlementEconomyRuntime>()
                .init_resource::<MootQueueClock>()
                .init_resource::<PublishedTerrainDeltas>();
            let clock = app
                .world_mut()
                .spawn((WorldTime::new_default(), TimeWarp(warp)))
                .id();
            let mut food = GoodsInventory::new(shared::economy::capacity::HALL);
            food.add(Good::Bread, 1);
            let mut market = MootMarket::founding();
            market.consign(MarketSeller::Treasury(SettlementId(1)), Good::Bread, 1, 20);
            let hall = app
                .world_mut()
                .spawn((
                    SettlementId(1),
                    Settlement {
                        name: "Construction meals".into(),
                        tier: shared::components::SettlementTier::Hamlet,
                        residents: 1,
                        treasury: 0,
                    },
                    SettlementEconomy::default(),
                    PlayerPosition(initial_entry + Vec3::X * 20.0),
                    PlayerRotation(0.0),
                    food,
                    market,
                ))
                .id();
            let mut cargo = GoodsInventory::new(shared::economy::capacity::VILLAGER);
            cargo.add(Good::Wood, 2);
            cargo.add(Good::Wool, 1);
            let builder = app
                .world_mut()
                .spawn((
                    CharacterName("Meal-carrying builder".into()),
                    CharacterKind::Villager,
                    PersonId(1),
                    ResidentOf(SettlementId(1)),
                    VillagerIntent::Resident { settlement: hall },
                    PlayerPosition(initial_entry),
                    PlayerRotation(0.0),
                    RegionCoord::from_world_pos(initial_entry),
                    CharacterActivity::Idle,
                    Wallet::new(100),
                    Nutrition::default(),
                    cargo,
                ))
                .id();
            app.add_systems(
                Update,
                (
                    advance_moot_service_queues,
                    run_moot_meal_collections,
                    run_construction_material_logistics,
                    crate::player::hero::step_units,
                )
                    .chain(),
            );

            // The real daily purchase removes stock and pays its seller; no
            // test-only ration or queue grant bypasses the transaction.
            app.world_mut()
                .run_system_once(update_settlement_economies)
                .unwrap();
            app.world_mut().get_mut::<WorldTime>(clock).unwrap().day += 1;
            app.world_mut()
                .run_system_once(update_settlement_economies)
                .unwrap();
            app.world_mut()
                .run_system_once(apply_business_events)
                .unwrap();
            if already_carrying_meal {
                for _ in 0..1000 {
                    if app
                        .world()
                        .get::<MootMealRoutine>(builder)
                        .is_some_and(|meal| meal.objective() == CharacterObjective::CollectingFood)
                    {
                        break;
                    }
                    tick(&mut app, warp);
                }
                assert_eq!(
                    app.world()
                        .get::<GoodsInventory>(builder)
                        .unwrap()
                        .amount(Good::Bread),
                    1
                );
                assert!(app.world().get::<MootQueueTicket>(builder).is_none());
            } else {
                assert!(app.world().get::<MootQueueTicket>(builder).is_some());
            }
            assert_eq!(app.world().get::<Wallet>(builder).unwrap().balance(), 80);
            assert_eq!(app.world().get::<Settlement>(hall).unwrap().treasury, 20);
            assert_eq!(
                app.world()
                    .get::<GoodsInventory>(hall)
                    .unwrap()
                    .amount(Good::Bread),
                0
            );
            assert_eq!(
                app.world().get::<Nutrition>(builder).unwrap().total_meals,
                0
            );

            // Reproduce both the newly reserved meal and the already-carried
            // ration seen in the long lab. The body is never teleported: its
            // current position is the construction approach's retained entry.
            let entry = app.world().get::<PlayerPosition>(builder).unwrap().0;
            access.points[2] = entry.xz();
            let kind = SettlementBuildingKind::House;
            let required = kind.construction_wood_required();
            let mut stock = GoodsInventory::new(kind.construction_storage_bulk());
            stock.add(Good::Wood, required - 2);
            let site = app
                .world_mut()
                .spawn((
                    UnderConstruction {
                        kind,
                        position: site_position,
                        rotation: 0.0,
                        owner: Some("Meal-carrying builder".into()),
                        owner_id: Some(PersonId(1)),
                        builder: Some(builder),
                        settlement: hall,
                        settlement_id: SettlementId(1),
                        stand: apron,
                        failed_stand_routes: 0,
                        stage: BuildStage::Supplying,
                        quality: 0.5,
                    },
                    PlayerPosition(site_position),
                    stock,
                    access.clone(),
                ))
                .id();
            let mut routine = ConstructionMaterialRoutine::new(site);
            routine.phase = if unloading_other_goods {
                ConstructionMaterialPhase::UnloadingAtHall {
                    hall,
                    entrance: SettlementBuildingKind::Hall
                        .entrance_position(app.world().get::<PlayerPosition>(hall).unwrap().0, 0.0),
                }
            } else {
                ConstructionMaterialPhase::ApproachingDeliveryAccess { entry }
            };
            app.world_mut().entity_mut(builder).insert((
                VillagerIntent::Building {
                    settlement: hall,
                    site,
                },
                routine,
                ConstructionDeliveryAccess { entry },
            ));
            for _ in 0..2000 {
                tick(&mut app, warp);
                if app
                    .world()
                    .get::<ConstructionMaterialRoutine>(builder)
                    .is_none()
                    && app.world().get::<MootMealRoutine>(builder).is_some()
                {
                    app.world_mut()
                        .run_system_once(advance_construction)
                        .unwrap();
                    assert_eq!(
                        app.world().get::<UnderConstruction>(site).unwrap().stage,
                        BuildStage::Supplying,
                        "finishing the material load does not let construction steal the meal's next trip"
                    );
                }
                let world = app.world();
                let carrier = world.get::<GoodsInventory>(builder).unwrap();
                let nutrition = world.get::<Nutrition>(builder).unwrap();
                let reserved =
                    u32::from(world.get::<MootMealRoutine>(builder).is_some_and(|meal| {
                        meal.objective() == CharacterObjective::QueuedForPersonalFood
                    }));
                assert_eq!(
                    carrier.amount(Good::Wood)
                        + world
                            .get::<GoodsInventory>(site)
                            .unwrap()
                            .amount(Good::Wood),
                    required
                );
                assert_eq!(
                    carrier.amount(Good::Wool),
                    1,
                    "unrelated personal cargo stays owned"
                );
                assert_eq!(
                    u64::from(carrier.amount(Good::Bread) + reserved)
                        + u64::from(nutrition.total_meals),
                    1,
                    "the paid ration is reserved, carried, or eaten exactly once"
                );
                if !unloading_other_goods
                    && world.get::<ConstructionMaterialRoutine>(builder).is_some()
                {
                    assert_eq!(
                        nutrition.total_meals, 0,
                        "the committed delivery finishes before its waiting meal"
                    );
                }
                if (unloading_other_goods
                    || world.get::<ConstructionMaterialRoutine>(builder).is_none())
                    && world.get::<MootMealRoutine>(builder).is_none()
                {
                    break;
                }
            }
            if unloading_other_goods {
                // Unloading is an interruptible counter approach. In
                // particular it cannot sell a ration that another routine
                // already paid for and holds as carried personal food.
                assert!(app.world().get::<MootMealRoutine>(builder).is_none());
                assert!(app.world().get::<MootQueueTicket>(builder).is_none());
                assert_eq!(
                    app.world().get::<Nutrition>(builder).unwrap().total_meals,
                    1
                );
                assert_eq!(
                    app.world()
                        .get::<GoodsInventory>(builder)
                        .unwrap()
                        .amount(Good::Wood),
                    2
                );
                assert_eq!(
                    app.world()
                        .get::<GoodsInventory>(builder)
                        .unwrap()
                        .amount(Good::Wool),
                    1
                );
                assert_eq!(
                    app.world()
                        .get::<GoodsInventory>(builder)
                        .unwrap()
                        .amount(Good::Bread),
                    0
                );
                assert_eq!(
                    app.world()
                        .get::<GoodsInventory>(hall)
                        .unwrap()
                        .amount(Good::Bread),
                    0
                );
                assert_eq!(app.world().get::<Wallet>(builder).unwrap().balance(), 80);
                assert_eq!(app.world().get::<Settlement>(hall).unwrap().treasury, 20);
                assert!(matches!(
                    app.world()
                        .get::<ConstructionMaterialRoutine>(builder)
                        .unwrap()
                        .phase,
                    ConstructionMaterialPhase::UnloadingAtHall { .. }
                ));
                continue;
            }
            assert!(
                app.world()
                    .get::<ConstructionMaterialRoutine>(builder)
                    .is_none()
            );
            assert!(app.world().get::<MootMealRoutine>(builder).is_none());
            assert!(app.world().get::<MootQueueTicket>(builder).is_none());
            assert_eq!(
                app.world()
                    .get::<GoodsInventory>(site)
                    .unwrap()
                    .amount(Good::Wood),
                required
            );
            assert_eq!(
                app.world()
                    .get::<GoodsInventory>(builder)
                    .unwrap()
                    .amount(Good::Wood),
                0
            );
            assert_eq!(
                app.world().get::<Nutrition>(builder).unwrap().total_meals,
                1
            );
            assert_eq!(app.world().get::<Wallet>(builder).unwrap().balance(), 80);
            assert_eq!(app.world().get::<Settlement>(hall).unwrap().treasury, 20);

            // Resuming an already started shell after the meal must walk
            // back without resetting or remotely advancing its saved work.
            let saved_stand = app.world().get::<UnderConstruction>(site).unwrap().stand;
            app.world_mut()
                .get_mut::<UnderConstruction>(site)
                .unwrap()
                .stage = BuildStage::Raising { seconds_left: 60.0 };
            assert!(
                ground_distance(
                    app.world().get::<PlayerPosition>(builder).unwrap().0,
                    saved_stand
                ) > BUILD_REACH
            );
            app.world_mut()
                .run_system_once(advance_construction)
                .unwrap();
            assert_eq!(
                app.world().get::<UnderConstruction>(site).unwrap().stage,
                BuildStage::Raising { seconds_left: 60.0 }
            );
            assert_eq!(
                app.world().get::<MoveTarget>(builder).unwrap().0,
                saved_stand
            );
            for _ in 0..1000 {
                tick(&mut app, warp);
                app.world_mut()
                    .run_system_once(advance_construction)
                    .unwrap();
                if matches!(app.world().get::<UnderConstruction>(site).unwrap().stage, BuildStage::Raising { seconds_left } if seconds_left < 60.0)
                {
                    break;
                }
            }
            assert!(
                ground_distance(
                    app.world().get::<PlayerPosition>(builder).unwrap().0,
                    saved_stand
                ) <= BUILD_REACH
            );
            assert!(
                matches!(app.world().get::<UnderConstruction>(site).unwrap().stage, BuildStage::Raising { seconds_left } if seconds_left < 60.0 && seconds_left > 59.0)
            );
        }
    }
}

#[test]
fn repeated_blocked_deliveries_wait_with_real_cargo_and_never_move_the_worksite() {
    use bevy::ecs::system::RunSystemOnce;
    use shared::components::{PersonId, SettlementId, TimeWarp};
    for warp in [1., 25.] {
        let (terrain, access, initial) = fixture();
        let routes = context(&terrain);
        let position = routes.point(initial.xz());
        let apron = routes.point(access.points[1]);
        let plot = routes.point(access.points[0]);
        assert!(ground_distance(position, apron) > WORK_REACH);
        let mut app = App::new();
        app.insert_resource(terrain)
            .init_resource::<Time>()
            .init_resource::<BusinessEventQueue>();
        let clock = app
            .world_mut()
            .spawn((WorldTime::new_default(), TimeWarp(warp)))
            .id();
        let hall = app
            .world_mut()
            .spawn((
                SettlementId(1),
                Settlement {
                    name: "Blocked delivery".into(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 1,
                    treasury: 0,
                },
                PlayerPosition(position + Vec3::X * 20.),
            ))
            .id();
        let mut cargo = GoodsInventory::new(shared::economy::capacity::VILLAGER);
        cargo.add(Good::Wood, 4);
        cargo.add(Good::Wool, 1);
        let builder = app
            .world_mut()
            .spawn((
                PersonId(1),
                CharacterKind::Villager,
                CharacterName("Waiting builder".into()),
                PlayerPosition(position),
                PlayerRotation(0.),
                CharacterActivity::Building,
                cargo,
            ))
            .id();
        let kind = SettlementBuildingKind::House;
        let required = kind.construction_wood_required();
        let mut stock = GoodsInventory::new(kind.construction_storage_bulk());
        stock.add(Good::Wood, required - 4);
        let site = app
            .world_mut()
            .spawn((
                UnderConstruction {
                    kind,
                    position: plot,
                    rotation: 0.,
                    owner: Some("Waiting builder".into()),
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
                stock,
                access,
            ))
            .id();
        let mut routine = ConstructionMaterialRoutine::new(site);
        routine.phase = ConstructionMaterialPhase::Delivering { destination: apron };
        app.world_mut().entity_mut(builder).insert((
            routine,
            VillagerIntent::Building {
                settlement: hall,
                site,
            },
            MoveTarget(apron),
        ));
        for _ in 0..MAX_CERTIFIED_DELIVERY_FAILURES {
            let failed = app
                .world()
                .get::<MoveTarget>(builder)
                .map_or(apron, |target| target.0);
            app.world_mut()
                .entity_mut(builder)
                .insert(NavigationRouteFailed { goal: failed });
            app.world_mut()
                .run_system_once(run_construction_material_logistics)
                .unwrap();
        }
        let retry_after = match app
            .world()
            .get::<ConstructionMaterialRoutine>(builder)
            .unwrap()
            .phase
        {
            ConstructionMaterialPhase::WaitingForDeliveryAccess {
                destination,
                retry_after,
            } => {
                assert_eq!(destination, apron);
                retry_after
            }
            ref other => panic!("blocked delivery did not pause: {other:?}"),
        };
        assert!(
            !app.world()
                .get::<ConstructionMaterialRoutine>(builder)
                .unwrap()
                .finishes_before_personal_needs(),
            "an impossible delivery must not hold a paid meal hostage"
        );
        // More simulation passes cannot reinterpret the carrier's remote
        // position as the site or spend the retained load while paused.
        for _ in 0..20 {
            app.world_mut()
                .run_system_once(run_construction_material_logistics)
                .unwrap();
            assert_eq!(
                app.world()
                    .get::<GoodsInventory>(builder)
                    .unwrap()
                    .amount(Good::Wood),
                4
            );
            assert_eq!(
                app.world()
                    .get::<GoodsInventory>(builder)
                    .unwrap()
                    .amount(Good::Wool),
                1
            );
            assert_eq!(
                app.world()
                    .get::<GoodsInventory>(site)
                    .unwrap()
                    .amount(Good::Wood),
                required - 4
            );
            let worksite = app.world().get::<UnderConstruction>(site).unwrap();
            assert_eq!(worksite.stand, apron);
            assert_eq!(worksite.stage, BuildStage::Supplying);
            assert_eq!(worksite.builder, Some(builder));
            assert_eq!(
                app.world().get::<PlayerPosition>(builder).unwrap().0,
                position
            );
            assert_eq!(
                *app.world().get::<CharacterActivity>(builder).unwrap(),
                CharacterActivity::Idle
            );
            assert!(app.world().get::<MoveTarget>(builder).is_none());
            assert!(app.world().get::<TravelRoute>(builder).is_none());
        }
        app.world_mut()
            .get_mut::<WorldTime>(clock)
            .unwrap()
            .seconds_in_cycle = retry_after as f32 + 0.01;
        app.world_mut()
            .run_system_once(run_construction_material_logistics)
            .unwrap();
        assert_eq!(
            app.world().get::<MoveTarget>(builder).unwrap().0,
            apron,
            "retry must ask the real navigator for the same apron"
        );
        assert!(app.world().get::<TravelRoute>(builder).is_none());
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(builder)
                .unwrap()
                .amount(Good::Wood),
            4
        );
        // Once the initial attempts are exhausted, each later failed route
        // pauses again instead of starting another immediate twelve-route burst.
        app.world_mut()
            .entity_mut(builder)
            .insert(NavigationRouteFailed { goal: apron });
        app.world_mut()
            .run_system_once(run_construction_material_logistics)
            .unwrap();
        assert!(
            matches!(app.world().get::<ConstructionMaterialRoutine>(builder).unwrap().phase,
            ConstructionMaterialPhase::WaitingForDeliveryAccess { retry_after:next,.. } if next>retry_after)
        );
        assert_eq!(
            app.world().get::<UnderConstruction>(site).unwrap().stand,
            apron
        );
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(builder)
                .unwrap()
                .amount(Good::Wood),
            4
        );
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(site)
                .unwrap()
                .amount(Good::Wood),
            required - 4
        );
    }
}
