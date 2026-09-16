use super::*;
use shared::components::{
    BuildingId, BuildingOf, EmployedAt, PersonId, RoadClass, RoadOf, RoadSurface, SettlementId,
    TimeWarp,
};

fn fixture(warp: f32) -> (App, Entity, Entity, Entity, Vec3, Vec3) {
    let mut app = App::new();
    app.init_resource::<Time>()
        .init_resource::<BusinessEventQueue>()
        .init_resource::<SettlementEconomyRuntime>()
        .add_systems(
            Update,
            (fill_vacancies, assign_quarry_routines, run_quarry_routines).chain(),
        );
    let clock = app
        .world_mut()
        .spawn((WorldTime::new_default(), TimeWarp(warp)))
        .id();
    let hall_at = Vec3::new(1700.0, 0.0, 0.0);
    let farm_at = hall_at + Vec3::X * 30.0;
    let town_id = SettlementId(1);
    let hall = app
        .world_mut()
        .spawn((
            town_id,
            PlayerPosition(hall_at),
            PlayerRotation(0.0),
            Settlement {
                name: "Pasturetest".into(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            },
        ))
        .id();
    let kind = SettlementBuildingKind::LivestockFarm;
    let store = kind.entrance_position(farm_at, 0.0);
    let face = outdoor_work_point(kind, farm_at, 0.0, None);
    let farm = app
        .world_mut()
        .spawn((
            BuildingId(2),
            BuildingOf(town_id),
            PlayerPosition(farm_at),
            PlayerRotation(0.0),
            SettlementBuilding {
                kind,
                settlement: "Pasturetest".into(),
                owner: None,
                quality: 1.0,
                workers: vec![],
            },
            GoodsInventory::new(kind.storage_bulk_capacity()),
            BusinessStaffingPolicy::new(1),
            BusinessStaffingForecast {
                day: 0,
                expected_sales_units: 0,
                produced_output_units: 0,
                optimal_positions: 1,
                marginal_daily_profit: 0,
            },
        ))
        .id();
    app.world_mut().spawn((
        VillageRoad {
            settlement: "Pasturetest".into(),
            builder: "Crew".into(),
            points: vec![
                store.xz(),
                SettlementBuildingKind::Hall
                    .entrance_position(hall_at, 0.0)
                    .xz(),
            ],
            built_through: 2,
            width: crate::world::village_roads::VILLAGE_ROAD_WIDTH,
            reserved_width: RoadClass::Lane.initial_reserved_width(),
            surface: RoadSurface::Dirt,
            class: RoadClass::Lane,
            stone_committed: 0,
        },
        RoadOf(town_id),
    ));
    let worker = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            PersonId(3),
            CharacterName("New hand".into()),
            VillagerIntent::Resident { settlement: hall },
            PlayerPosition(hall_at),
            PlayerRotation(0.0),
            Occupation::default(),
            WorkStatus::LookingForWork,
            CharacterActivity::Idle,
            GoodsInventory::new(shared::economy::capacity::VILLAGER),
            WorkerOffDuty { day: 0 },
        ))
        .id();
    (app, clock, farm, worker, face, store)
}

#[test]
fn an_unhoused_herder_keeps_the_work_destination_and_animation() {
    for warp in [1.0, 25.0] {
        let (mut app, _, _, worker, face, _) = fixture(warp);
        app.init_resource::<super::super::ambient::AmbientClock>()
            .init_resource::<super::super::ambient::AmbientSpotCache>()
            .insert_resource(WorldTerrain::default())
            .add_systems(
                Update,
                super::super::ambient::run_ambient_routines.after(run_quarry_routines),
            );
        app.world_mut()
            .entity_mut(worker)
            .insert(shared::region::RegionCoord::default());

        // A newly hired resident may still lack a house. Cosmetic roadside
        // walks must not replace the commute or interrupt tending the herd.
        for _ in 0..3 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(0.3));
            app.update();
            assert!(
                app.world()
                    .get::<super::super::ambient::AmbientRoutine>(worker)
                    .is_none()
            );
            let target = app.world().get::<MoveTarget>(worker).unwrap().0;
            assert_eq!(target.xz(), face.xz());
        }
        app.world_mut().get_mut::<PlayerPosition>(worker).unwrap().0 = face;
        for _ in 0..3 {
            app.update();
            assert_eq!(
                app.world().get::<CharacterActivity>(worker),
                Some(&CharacterActivity::Farming)
            );
            assert!(app.world().get::<MoveTarget>(worker).is_none());
        }
    }
}

#[test]
fn hired_livestock_worker_starts_now_and_delivers_real_output_at_both_speeds() {
    for warp in [1.0, 25.0] {
        let (mut app, clock, farm, worker, face, store) = fixture(warp);
        app.update();
        assert_eq!(
            app.world().get::<EmployedAt>(worker),
            Some(&EmployedAt(BuildingId(2)))
        );
        assert!(app.world().get::<WorkerOffDuty>(worker).is_none());
        assert_eq!(
            app.world().get::<QuarryRoutine>(worker).unwrap().phase,
            QuarryPhase::GoingToFace
        );
        assert_eq!(app.world().get::<MoveTarget>(worker).unwrap().0, face);
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(farm)
                .unwrap()
                .amount(Good::Meat),
            0
        );

        // Test the production boundary separately from navigation: physically
        // arriving enables work; neither a job title nor elapsed days does.
        app.world_mut().get_mut::<PlayerPosition>(worker).unwrap().0 = face;
        app.update();
        assert_eq!(
            app.world().get::<QuarryRoutine>(worker).unwrap().phase,
            QuarryPhase::Mining
        );
        let step = livestock_seconds_per_meat(1.0) * 2.01 / warp;
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(step));
        app.update();
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(worker)
                .unwrap()
                .amount(Good::Meat),
            2
        );
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(worker)
                .unwrap()
                .amount(Good::Wool),
            2
        );
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(farm)
                .unwrap()
                .amount(Good::Meat),
            0
        );
        assert_eq!(app.world().get::<MoveTarget>(worker).unwrap().0, store);

        app.world_mut().get_mut::<PlayerPosition>(worker).unwrap().0 = store;
        app.update();
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(farm)
                .unwrap()
                .amount(Good::Meat),
            2
        );
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(farm)
                .unwrap()
                .amount(Good::Wool),
            2
        );
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(worker)
                .unwrap()
                .used_bulk(),
            0
        );
        assert_eq!(
            app.world().get::<QuarryRoutine>(worker).unwrap().phase,
            QuarryPhase::GoingToFace,
            "an employee returns to work despite the old zero-output forecast"
        );
        assert_eq!(app.world().get::<WorldTime>(clock).unwrap().day, 0);
    }
}

#[test]
fn livestock_labour_cannot_continue_at_the_hall_after_a_diversion() {
    let (mut app, _, farm, worker, face, _) = fixture(25.0);
    app.update();
    app.world_mut().get_mut::<PlayerPosition>(worker).unwrap().0 = face;
    app.update();
    app.world_mut()
        .get_mut::<QuarryRoutine>(worker)
        .unwrap()
        .work_seconds = livestock_seconds_per_meat(1.0);
    app.world_mut().get_mut::<PlayerPosition>(worker).unwrap().0 = Vec3::new(1700.0, 0.0, 0.0);
    app.update();
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(worker)
            .unwrap()
            .used_bulk(),
        0
    );
    assert_eq!(
        app.world().get::<GoodsInventory>(farm).unwrap().used_bulk(),
        0
    );
    assert_eq!(
        app.world().get::<QuarryRoutine>(worker).unwrap().phase,
        QuarryPhase::GoingToFace
    );
    assert_eq!(app.world().get::<MoveTarget>(worker).unwrap().0, face);
    assert_eq!(
        *app.world().get::<CharacterActivity>(worker).unwrap(),
        CharacterActivity::Idle
    );
}

#[test]
fn nightfall_waits_for_meat_and_wool_to_reach_the_farm() {
    let (mut app, clock, _, worker, face, _) = fixture(1.0);
    app.update();
    app.world_mut().get_mut::<PlayerPosition>(worker).unwrap().0 = face;
    app.update();
    app.world_mut()
        .get_mut::<QuarryRoutine>(worker)
        .unwrap()
        .work_seconds = livestock_seconds_per_meat(1.0) * 2.0;
    app.update();
    let home = app
        .world_mut()
        .spawn((
            SettlementBuilding {
                kind: SettlementBuildingKind::House,
                settlement: "Pasturetest".into(),
                owner: None,
                quality: 1.0,
                workers: vec![],
            },
            PlayerPosition(Vec3::new(1690.0, 0.0, 0.0)),
            PlayerRotation(0.0),
            Household {
                resident_ids: vec![PersonId(3)],
                residents: vec!["New hand".into()],
            },
        ))
        .id();
    app.world_mut()
        .entity_mut(worker)
        .insert(HomeAssignment::new(home));
    app.add_systems(Update, run_household_schedules.before(run_quarry_routines));
    {
        let mut time = app.world_mut().get_mut::<WorldTime>(clock).unwrap();
        time.seconds_in_cycle = time.day_duration + 1.0;
    }
    app.update();
    assert!(
        app.world().get::<HomeRoutine>(worker).is_none(),
        "nightfall must not redirect a farmer still carrying the workplace's Meat or Wool"
    );
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(worker)
            .unwrap()
            .amount(Good::Meat),
        2
    );
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(worker)
            .unwrap()
            .amount(Good::Wool),
        2
    );
}

#[test]
fn a_full_load_cannot_bank_extra_work_for_the_next_trip() {
    let (mut app, _, _, worker, face, _) = fixture(25.0);
    app.update();
    app.world_mut().get_mut::<PlayerPosition>(worker).unwrap().0 = face;
    app.update();
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs(120));
    app.update();
    assert!(
        app.world()
            .get::<GoodsInventory>(worker)
            .unwrap()
            .amount(Good::Meat)
            > 0
    );
    assert_eq!(
        app.world()
            .get::<QuarryRoutine>(worker)
            .unwrap()
            .work_seconds,
        0.0
    );
    assert_eq!(
        app.world().get::<QuarryRoutine>(worker).unwrap().phase,
        QuarryPhase::ReturningToStore
    );
}

#[test]
fn full_store_retains_livestock_cargo_then_accepts_it_before_shift_end() {
    let (mut app, clock, farm, worker, face, store) = fixture(1.0);
    app.update();
    app.world_mut().get_mut::<PlayerPosition>(worker).unwrap().0 = face;
    app.update();
    app.world_mut()
        .get_mut::<QuarryRoutine>(worker)
        .unwrap()
        .work_seconds = livestock_seconds_per_meat(1.0) * 2.0;
    app.update();
    let capacity = app.world().get::<GoodsInventory>(farm).unwrap().free_bulk();
    app.world_mut()
        .get_mut::<GoodsInventory>(farm)
        .unwrap()
        .add(Good::Meat, capacity);
    app.world_mut().get_mut::<PlayerPosition>(worker).unwrap().0 = store;
    {
        let mut time = app.world_mut().get_mut::<WorldTime>(clock).unwrap();
        time.seconds_in_cycle = time.day_duration * 0.8;
    }
    app.update();
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(worker)
            .unwrap()
            .amount(Good::Meat),
        2
    );
    assert!(app.world().get::<WorkerOffDuty>(worker).is_none());
    app.world_mut()
        .get_mut::<GoodsInventory>(farm)
        .unwrap()
        .remove(Good::Meat, capacity);
    app.update();
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(worker)
            .unwrap()
            .used_bulk(),
        0
    );
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(farm)
            .unwrap()
            .amount(Good::Meat),
        2
    );
    app.update();
    assert!(app.world().get::<QuarryRoutine>(worker).is_none());
    assert!(app.world().get::<WorkerOffDuty>(worker).is_some());
}

#[test]
fn rich_stone_ground_extracts_faster_without_creating_free_units() {
    assert!(quarry_seconds_per_stone(1.0) < quarry_seconds_per_stone(0.0));
    assert_eq!(Good::Stone.bulk_per_unit(), 6);
    let inventory = GoodsInventory::new(shared::economy::capacity::VILLAGER);
    assert_eq!(inventory.free_bulk() / Good::Stone.bulk_per_unit(), 4);
}

#[test]
fn livestock_pause_preserves_labour_cargo_and_once_daily_training() {
    use bevy::ecs::system::RunSystemOnce;

    let (mut app, clock, farm, worker, face, store) = fixture(1.0);
    app.world_mut()
        .entity_mut(worker)
        .insert(CharacterAttributes::default());
    app.update();
    app.world_mut().get_mut::<PlayerPosition>(worker).unwrap().0 = face;
    app.update();
    let cycle = livestock_seconds_per_meat(1.0);
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f32(cycle + 37.0));
    app.update();
    let trained = app
        .world()
        .get::<CharacterAttributes>(worker)
        .unwrap()
        .physique();
    assert!(trained > 10);
    let hall = app.world().get::<QuarryRoutine>(worker).unwrap().hall;
    app.world_mut()
        .run_system_once(
            |mut commands: Commands,
             mut workers: Query<(Entity, &QuarryRoutine, &mut CharacterActivity)>| {
                for (worker, routine, mut activity) in &mut workers {
                    super::super::worker_activity::lifecycle::pause(
                        &mut commands,
                        worker,
                        routine,
                        &mut activity,
                    );
                }
            },
        )
        .unwrap();
    app.world_mut()
        .entity_mut(worker)
        .insert(MarketCollectionRoutine {
            business: farm,
            seller: BuildingId(2),
            hall,
            counter: store,
            good: Good::Meat,
            reserved_units: 0,
            unit_price: 0,
            phase: MarketCollectionPhase::GoingToBusiness,
            fallback_counter_attempted: false,
        });
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::ZERO);
    app.update();
    let progress = app.world().get::<QuarryWorkProgress>(worker).unwrap();
    assert!((progress.seconds - 37.0).abs() < 0.01);
    assert_eq!(progress.produced_today, 1);
    assert_eq!(progress.production_day, 0);
    assert!(app.world().get::<QuarryRoutine>(worker).is_none());
    assert!(app.world().get::<WorkerOffDuty>(worker).is_none());
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(worker)
            .unwrap()
            .amount(Good::Meat),
        1
    );
    assert_eq!(
        app.world().get::<EmployedAt>(worker),
        Some(&EmployedAt(BuildingId(2)))
    );

    app.world_mut()
        .entity_mut(worker)
        .remove::<MarketCollectionRoutine>();
    app.update();
    assert!(app.world().get::<QuarryWorkProgress>(worker).is_none());
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f32(cycle - 37.0));
    app.update();
    assert_eq!(
        app.world()
            .get::<QuarryRoutine>(worker)
            .unwrap()
            .produced_today,
        2
    );
    assert_eq!(
        app.world()
            .get::<CharacterAttributes>(worker)
            .unwrap()
            .physique(),
        trained,
        "resuming a partial shift must not award another first-output training gain"
    );
    app.world_mut().get_mut::<PlayerPosition>(worker).unwrap().0 = store;
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::ZERO);
    app.update();
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(farm)
            .unwrap()
            .amount(Good::Meat),
        2
    );
    app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 1;
    app.world_mut().get_mut::<PlayerPosition>(worker).unwrap().0 = face;
    app.update();
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f32(cycle));
    app.update();
    assert_eq!(
        app.world()
            .get::<QuarryRoutine>(worker)
            .unwrap()
            .produced_today,
        1
    );
    assert!(
        app.world()
            .get::<CharacterAttributes>(worker)
            .unwrap()
            .physique()
            > trained
    );
}

#[test]
fn a_partial_livestock_load_stops_work_animation_before_the_dusk_return() {
    for warp in [1.0, 25.0] {
        let (mut app, clock, _, worker, face, store) = fixture(warp);
        app.update();
        app.world_mut().get_mut::<PlayerPosition>(worker).unwrap().0 = face;
        app.update();
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(
                livestock_seconds_per_meat(1.0) / warp,
            ));
        app.update();
        assert_eq!(
            app.world().get::<CharacterActivity>(worker),
            Some(&CharacterActivity::Farming)
        );
        app.world_mut()
            .get_mut::<WorldTime>(clock)
            .unwrap()
            .set_normalized_time(19.0 / 24.0);
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::ZERO);
        app.update();
        assert_eq!(
            app.world().get::<QuarryRoutine>(worker).unwrap().phase,
            QuarryPhase::ReturningToStore
        );
        assert_eq!(app.world().get::<MoveTarget>(worker).unwrap().0, store);
        assert_eq!(
            app.world().get::<CharacterActivity>(worker),
            Some(&CharacterActivity::Idle)
        );
        assert!(app.world().get::<WorkerOffDuty>(worker).is_none());
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(worker)
                .unwrap()
                .amount(Good::Meat),
            1
        );
    }
}
