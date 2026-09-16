use super::*;
use shared::components::TimeWarp;

#[test]
fn failed_home_routes_preserve_body_and_cargo_without_remote_sleep_at_both_warps() {
    for warp in [1.0, 25.0] {
        let mut app = App::new();
        app.init_resource::<Time>()
            .add_systems(Update, run_household_schedules);
        let mut clock = WorldTime::new_default();
        clock.set_normalized_time(23.0 / 24.0);
        assert!(!clock.is_day());
        let clock_entity = app.world_mut().spawn((clock, TimeWarp::clamped(warp))).id();
        let person_id = shared::components::PersonId(1);
        let at = Vec3::new(12.0, 5.0, 10.0);
        let home = app
            .world_mut()
            .spawn((
                SettlementBuilding {
                    kind: SettlementBuildingKind::House,
                    settlement: "Home route test".into(),
                    owner: None,
                    quality: 0.5,
                    workers: Vec::new(),
                },
                PlayerPosition(at),
                PlayerRotation(0.0),
                Household {
                    resident_ids: vec![person_id],
                    residents: vec!["Walker".into()],
                },
            ))
            .id();
        let door = SettlementBuildingKind::House.entrance_position(at, 0.0);
        let stranded_at = at + Vec3::X * 80.0;
        let mut cargo = GoodsInventory::new(shared::economy::capacity::VILLAGER);
        cargo.add(Good::Wood, 2);
        cargo.add(Good::Bread, 1);
        let person = app
            .world_mut()
            .spawn((
                person_id,
                CharacterKind::Villager,
                PlayerPosition(stranded_at),
                PlayerRotation(0.0),
                CharacterActivity::Idle,
                cargo,
                HomeAssignment { home },
                HomeRoutine {
                    home,
                    phase: HomePhase::GoingToDoor,
                    failed_routes: 0,
                },
                MoveTarget(door),
            ))
            .id();

        // Exercise the actual household failure path at a remote stand. The
        // navigator's failed certificate is input; this test does not invent
        // an arrival or replace the movement system with a positional update.
        for failure in 1..=3 {
            app.world_mut()
                .entity_mut(person)
                .insert(NavigationRouteFailed { goal: door });
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(1.0 / 60.0));
            app.update();
            assert_eq!(
                app.world()
                    .get::<HomeRoutine>(person)
                    .unwrap()
                    .failed_routes,
                failure
            );
            assert_eq!(
                app.world().get::<PlayerPosition>(person).unwrap().0,
                stranded_at
            );
            assert_eq!(app.world().get::<MoveTarget>(person).is_some(), failure < 3);
        }
        for _ in 0..120 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(1.0 / 60.0));
            app.update();
            assert_eq!(
                app.world().get::<PlayerPosition>(person).unwrap().0,
                stranded_at
            );
            assert_eq!(
                *app.world().get::<CharacterActivity>(person).unwrap(),
                CharacterActivity::Idle
            );
            assert!(matches!(
                app.world().get::<HomeRoutine>(person).unwrap().phase,
                HomePhase::GoingToDoor
            ));
            assert!(
                app.world().get::<MoveTarget>(person).is_none(),
                "failed night must not requeue every tick at {warp}x"
            );
            assert!(app.world().get::<BuildingDoorUse>(person).is_none());
            let carried = app.world().get::<GoodsInventory>(person).unwrap();
            assert_eq!(carried.amount(Good::Wood), 2);
            assert_eq!(carried.amount(Good::Bread), 1);
        }

        app.world_mut()
            .get_mut::<WorldTime>(clock_entity)
            .unwrap()
            .set_normalized_time(8.0 / 24.0);
        app.update();
        assert!(
            app.world().get::<HomeRoutine>(person).is_none(),
            "morning must release the failed night owner"
        );
        assert_eq!(
            app.world().get::<PlayerPosition>(person).unwrap().0,
            stranded_at
        );
        app.world_mut()
            .get_mut::<WorldTime>(clock_entity)
            .unwrap()
            .set_normalized_time(23.0 / 24.0);
        app.update();
        assert_eq!(
            app.world()
                .get::<HomeRoutine>(person)
                .unwrap()
                .failed_routes,
            0
        );
        assert_eq!(app.world().get::<MoveTarget>(person).unwrap().0, door);
    }
}
