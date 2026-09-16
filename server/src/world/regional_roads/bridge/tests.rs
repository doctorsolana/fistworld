use super::*;

#[test]
fn bridge_survey_accepts_one_short_channel_and_rejects_wide_or_multiple_channels() {
    let sample = |p: Vec2| {
        if p.x.abs() < 10.0 {
            (-2.0, Some(0.0))
        } else {
            (0.5, None)
        }
    };
    let bridge =
        planning::survey_bridge(Vec2::new(-25.0, 0.0), Vec2::new(25.0, 0.0), sample).unwrap();
    assert!(!bridge.built);
    assert!(bridge.valid());
    assert!(
        bridge.deck_height - shared::components::ROAD_BRIDGE_UNDERDECK_DEPTH > 4.0,
        "the side trusses must clear the real dinghy mast and gentle swell"
    );
    assert!(bridge.height_at(Vec2::ZERO, 0.0).is_none());
    assert!(
        planning::survey_bridge(Vec2::new(-50.0, 0.0), Vec2::new(50.0, 0.0), |p| {
            if p.x.abs() < 35.0 {
                (-2.0, Some(0.0))
            } else {
                (0.5, None)
            }
        })
        .is_none()
    );
    assert!(
        planning::survey_bridge(Vec2::new(-40.0, 0.0), Vec2::new(40.0, 0.0), |p| {
            if p.x.abs() > 3.0 && p.x.abs() < 14.0 {
                (-2.0, Some(0.0))
            } else {
                (0.5, None)
            }
        })
        .is_none()
    );
}

#[test]
fn construction_cannot_consume_remote_materials_or_work_from_the_hall() {
    let mut app = App::new();
    let mut clock = WorldTime::new_default();
    clock.set_normalized_time(0.5);
    let clock_entity = app.world_mut().spawn(clock).id();
    let source = app.world_mut().spawn(GoodsInventory::new(1000)).id();
    let hall = app.world_mut().spawn_empty().id();
    let bridge = RoadBridge {
        start: Vec3::new(30.0, 0.0, 0.0),
        end: Vec3::new(70.0, 0.0, 0.0),
        deck_height: 3.0,
        ramp_length: 8.0,
        width: 3.6,
        built: false,
    };
    let wood = bridge.wood_required();
    let stone = bridge.stone_required();
    app.world_mut()
        .get_mut::<GoodsInventory>(source)
        .unwrap()
        .add(Good::Wood, wood);
    app.world_mut()
        .get_mut::<GoodsInventory>(source)
        .unwrap()
        .add(Good::Stone, stone);
    let site = app
        .world_mut()
        .spawn((bridge, GoodsInventory::new(1000)))
        .id();
    let worker = app
        .world_mut()
        .spawn((
            PlayerPosition(Vec3::new(-20.0, 0.0, 0.0)),
            CharacterActivity::Idle,
            GoodsInventory::new(10),
        ))
        .id();
    start_bridge_work(app.world_mut(), worker, hall, source, site, Vec3::ZERO);
    app.add_systems(Update, run_bridge_work);
    app.update();
    let advance = |app: &mut App| {
        app.world_mut()
            .get_mut::<WorldTime>(clock_entity)
            .unwrap()
            .advance(1.0, 1.0);
        app.update();
    };
    advance(&mut app);
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(source)
            .unwrap()
            .amount(Good::Wood),
        wood
    );
    assert_eq!(app.world().get::<MoveTarget>(worker).unwrap().0, Vec3::ZERO);
    app.world_mut().get_mut::<PlayerPosition>(worker).unwrap().0 = Vec3::ZERO;
    advance(&mut app);
    assert!(
        app.world()
            .get::<GoodsInventory>(worker)
            .unwrap()
            .amount(Good::Wood)
            > 0
    );
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(site)
            .unwrap()
            .amount(Good::Wood),
        0
    );
    advance(&mut app);
    assert_eq!(
        app.world().get::<MoveTarget>(worker).unwrap().0,
        Vec3::new(30.0, 0.0, 0.0)
    );
    assert!(!app.world().get::<RoadBridge>(site).unwrap().built);
}

#[test]
fn cancelling_bridge_keeps_an_existing_personal_meal_route() {
    use crate::world::village::moot_services::{MootQueueClock, MootServiceKind, reserve_meal};
    let mut world = World::new();
    let source = world.spawn(GoodsInventory::new(100)).id();
    let site = world.spawn_empty().id();
    let worker = world
        .spawn((PlayerPosition(Vec3::ZERO), CharacterActivity::Idle))
        .id();
    start_bridge_work(&mut world, worker, source, source, site, Vec3::ZERO);
    let mut queue_clock = MootQueueClock::default();
    reserve_meal(
        &mut world.commands(),
        &mut queue_clock,
        worker,
        source,
        MootServiceKind::PersonalMeal,
        Good::Food,
        1,
    );
    world.flush();
    let meal_target = Vec3::new(10., 0., 20.);
    world.entity_mut(worker).insert((
        MoveTarget(meal_target),
        TravelRoute {
            goal: meal_target,
            waypoints: vec![crate::world::village_roads::RouteWaypoint {
                position: meal_target,
                on_road: false,
            }],
            next: 0,
            geometry_version: 0,
        },
    ));
    assert!(world.get::<MootMealRoutine>(worker).is_some());
    assert!(cancel_bridge_work(&mut world, worker));
    assert!(world.get::<BridgeBuilder>(worker).is_none());
    assert!(world.get::<RoadBuilderRoutine>(worker).is_none());
    assert_eq!(world.get::<MoveTarget>(worker).unwrap().0, meal_target);
    assert_eq!(world.get::<TravelRoute>(worker).unwrap().goal, meal_target);
    assert!(world.get::<MootMealRoutine>(worker).is_some());
}

#[test]
fn partial_bridge_work_claim_is_exact_and_belongs_to_its_site() {
    let mut world = World::new();
    let bridge = RoadBridge {
        start: Vec3::ZERO,
        end: Vec3::new(40., 0., 0.),
        deck_height: 3.,
        ramp_length: 8.,
        width: 3.6,
        built: false,
    };
    let total = required_work(&bridge);
    let site = world.spawn(bridge).id();
    let other_site = world.spawn_empty().id();
    let worker = world.spawn_empty().id();
    start_bridge_work(&mut world, worker, other_site, other_site, site, Vec3::ZERO);
    world.get_mut::<BridgeBuilder>(worker).unwrap().worked = 15.75;
    assert_eq!(work_progress(&world, worker, site), 15);
    assert_eq!(work_progress(&world, worker, other_site), 0);
    world.get_mut::<BridgeBuilder>(worker).unwrap().worked = total as f32 + 100.;
    assert_eq!(
        work_progress(&world, worker, site),
        total - 1,
        "a pending footprint revalidation cannot earn completion before the deck exists"
    );
}

#[test]
fn borrowed_bridge_cart_reduces_real_loads_and_retires_without_losing_personal_goods() {
    use crate::world::village::{sync_porter_cargo_capacity, sync_porter_cart_state};
    let mut app = App::new();
    let mut clock = WorldTime::new_default();
    clock.set_normalized_time(0.5);
    let clock_entity = app.world_mut().spawn(clock).id();
    let deck = RoadBridge {
        start: Vec3::X * 30.0,
        end: Vec3::X * 70.0,
        deck_height: 3.0,
        ramp_length: 8.0,
        width: 3.6,
        built: false,
    };
    assert_eq!(
        required_work(&deck),
        144,
        "bank labour uses one paid second per square metre"
    );
    let wood = deck.wood_required();
    let stone = deck.stone_required();
    let mut source_stock = GoodsInventory::new(1_000);
    source_stock.add(Good::Wood, wood);
    source_stock.add(Good::Stone, stone);
    let source = app.world_mut().spawn(source_stock).id();
    let hall = app.world_mut().spawn_empty().id();
    let site = app
        .world_mut()
        .spawn((deck, GoodsInventory::new(1_000)))
        .id();
    let mut personal = GoodsInventory::new(shared::economy::capacity::VILLAGER);
    personal.add(Good::Bread, 1);
    let worker = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            PlayerPosition(Vec3::ZERO),
            CharacterActivity::Idle,
            personal,
        ))
        .id();
    start_bridge_work(app.world_mut(), worker, hall, source, site, Vec3::ZERO);
    app.add_systems(
        Update,
        (
            sync_porter_cargo_capacity,
            run_bridge_work,
            sync_porter_cart_state,
        )
            .chain(),
    );
    app.update();
    let mut loads = 0;
    for _ in 0..32 {
        let before = app
            .world()
            .get::<GoodsInventory>(source)
            .unwrap()
            .used_bulk();
        // This unit fixture supplies arrival only. The connected bridge lab
        // separately proves navigation and terrain-supported cart movement.
        if let Some(target) = app.world().get::<MoveTarget>(worker).map(|target| target.0) {
            app.world_mut().get_mut::<PlayerPosition>(worker).unwrap().0 = target;
        }
        app.world_mut()
            .get_mut::<WorldTime>(clock_entity)
            .unwrap()
            .advance(1.0, 1.0);
        app.update();
        let after = app
            .world()
            .get::<GoodsInventory>(source)
            .unwrap()
            .used_bulk();
        loads += usize::from(after < before);
        let total = [source, site, worker]
            .into_iter()
            .map(|entity| {
                app.world()
                    .get::<GoodsInventory>(entity)
                    .unwrap()
                    .amount(Good::Wood)
            })
            .sum::<u32>();
        assert_eq!(total, wood);
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(worker)
                .unwrap()
                .amount(Good::Bread),
            1
        );
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(worker)
                .unwrap()
                .bulk_capacity(),
            shared::economy::capacity::PORTER
        );
        assert!(
            app.world()
                .get::<shared::economy::PorterCartState>(worker)
                .is_some()
        );
        let deposited = app.world().get::<GoodsInventory>(site).unwrap();
        if deposited.amount(Good::Wood) == wood && deposited.amount(Good::Stone) == stone {
            break;
        }
    }
    assert_eq!(
        loads, 3,
        "two timber loads and one stone load, with actual personal bread space reserved"
    );
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(site)
            .unwrap()
            .amount(Good::Wood),
        wood
    );
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(site)
            .unwrap()
            .amount(Good::Stone),
        stone
    );
    assert!(cancel_bridge_work(app.world_mut(), worker));
    app.update();
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(worker)
            .unwrap()
            .bulk_capacity(),
        shared::economy::capacity::VILLAGER
    );
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(worker)
            .unwrap()
            .amount(Good::Bread),
        1
    );
    assert!(
        app.world()
            .get::<shared::economy::PorterCartState>(worker)
            .is_none()
    );
}
