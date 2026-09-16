//! The same doorway handoff protects every staffed workplace interior.

use super::*;
use bevy::ecs::system::RunSystemOnce;

fn tick(app: &mut App, world_seconds: f32, warp: f32) {
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f32(world_seconds / warp));
    app.update();
}

#[test]
fn paid_service_waits_for_a_retained_workplace_interior_to_physically_exit() {
    for warp in [1.0, 25.0] {
        for kind in [
            SettlementBuildingKind::Farmstead,
            SettlementBuildingKind::FishermansHut,
            SettlementBuildingKind::LumberjackHut,
            SettlementBuildingKind::Windmill,
            SettlementBuildingKind::Bakery,
        ] {
            let mut app = App::new();
            app.init_resource::<Time>()
                .init_resource::<MootQueueClock>()
                .add_systems(
                    Update,
                    (
                        advance_moot_service_queues,
                        run_workplace_service_handoffs,
                        run_workplace_door_transits,
                    )
                        .chain(),
                );
            app.world_mut().spawn((
                WorldTime::new_default(),
                shared::components::TimeWarp::clamped(warp),
            ));
            let hall = app
                .world_mut()
                .spawn((
                    Settlement {
                        name: "Doorford".into(),
                        tier: shared::components::SettlementTier::Hamlet,
                        residents: 1,
                        treasury: 20,
                    },
                    PlayerPosition(Vec3::new(80.0, 0.0, 80.0)),
                    PlayerRotation(0.0),
                ))
                .id();
            let building = Vec3::ZERO;
            let door = kind.entrance_position(building, 0.0);
            let inside = kind.interior_door_position(building, 0.0);
            let outside = exterior_door_clearance_position(building, door);
            let worker = app
                .world_mut()
                .spawn((
                    CharacterKind::Villager,
                    PlayerPosition(door),
                    PlayerRotation(0.0),
                    VillagerIntent::Resident { settlement: hall },
                    CharacterActivity::Idle,
                    Wallet::new(80),
                    GoodsInventory::new(shared::economy::capacity::VILLAGER),
                ))
                .id();
            app.world_mut()
                .run_system_once(move |mut commands: Commands| {
                    begin_workplace_entry(&mut commands, worker, building, door, inside);
                })
                .unwrap();
            tick(&mut app, 2.0, warp);
            assert_eq!(app.world().get::<MoveTarget>(worker).unwrap().0, inside);
            app.world_mut().get_mut::<PlayerPosition>(worker).unwrap().0 = inside;
            tick(&mut app, 0.0, warp);
            assert!(app.world().get::<WorkplaceDoorTransit>(worker).is_none());
            assert!(app.world().get::<WorkplaceInterior>(worker).is_some());
            assert_eq!(
                *app.world().get::<CharacterActivity>(worker).unwrap(),
                CharacterActivity::Indoors
            );

            app.world_mut()
                .run_system_once(
                    move |mut commands: Commands, mut clock: ResMut<MootQueueClock>| {
                        moot_services::reserve_meal(
                            &mut commands,
                            &mut clock,
                            worker,
                            hall,
                            MootServiceKind::PersonalMeal,
                            Good::Bread,
                            0,
                        );
                    },
                )
                .unwrap();
            tick(&mut app, 0.0, warp);
            assert!(app.world().get::<MootQueueTransit>(worker).is_none());
            assert_eq!(
                app.world()
                    .get::<WorkplaceDoorTransit>(worker)
                    .unwrap()
                    .direction,
                WorkplaceDoorDirection::Leaving
            );
            tick(&mut app, 2.0, warp);
            assert_eq!(app.world().get::<MoveTarget>(worker).unwrap().0, outside);
            tick(&mut app, 120.0, warp);
            assert!(app.world().get::<WorkplaceInterior>(worker).is_some());
            assert!(app.world().get::<MootQueueTransit>(worker).is_none());
            assert_eq!(app.world().get::<MoveTarget>(worker).unwrap().0, outside);

            // Supply only the body's arrival; the real threshold system must
            // release occupancy before the real queue may plan its next leg.
            app.world_mut().get_mut::<PlayerPosition>(worker).unwrap().0 = outside;
            tick(&mut app, 0.0, warp);
            assert!(app.world().get::<WorkplaceInterior>(worker).is_none());
            assert!(app.world().get::<WorkplaceDoorTransit>(worker).is_none());
            tick(&mut app, 0.0, warp);
            tick(&mut app, 0.0, warp);
            assert!(
                ground_distance(
                    app.world().get::<MoveTarget>(worker).unwrap().0,
                    Vec3::new(80.0, 0.0, 80.0)
                ) < 15.0
            );
            assert_eq!(
                app.world().get::<MootMealRoutine>(worker).unwrap().good,
                Good::Bread
            );
            assert_eq!(app.world().get::<Wallet>(worker).unwrap().balance(), 80);
            assert_eq!(
                app.world()
                    .get::<GoodsInventory>(worker)
                    .unwrap()
                    .used_bulk(),
                0
            );
            assert_eq!(app.world().get::<Settlement>(hall).unwrap().treasury, 20);
        }
    }
}
