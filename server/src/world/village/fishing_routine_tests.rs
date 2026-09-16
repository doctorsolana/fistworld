//! Physical fishing remains valid after other routines borrow the actor.

use super::*;
use shared::components::{AttachedTo, BuildingId, BuildingOf, EmployedAt, SettlementId, TimeWarp};

struct Fixture {
    app: App,
    worker: Entity,
    hut: Entity,
    hall: Entity,
    fish_spot: Vec3,
    deck_start: Vec3,
    staging: Vec3,
    entrance: Vec3,
}

impl Fixture {
    fn new(warp: f32) -> Self {
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<SettlementEconomyRuntime>()
            .init_resource::<BusinessEventQueue>()
            .insert_resource(WorldTerrain::default())
            .add_systems(Update, run_fishing_routines);
        let mut clock = WorldTime::new_default();
        clock.set_normalized_time(12.0 / 24.0);
        app.world_mut().spawn((clock, TimeWarp::clamped(warp)));
        let hall = app
            .world_mut()
            .spawn((SettlementId(1), PlayerPosition(Vec3::ZERO)))
            .id();
        let hut_position = Vec3::new(20.0, 2.0, 0.0);
        let building_id = BuildingId(2);
        let hut = app
            .world_mut()
            .spawn((
                building_id,
                BuildingOf(SettlementId(1)),
                SettlementBuilding {
                    kind: SettlementBuildingKind::FishermansHut,
                    settlement: "Fishing test".into(),
                    owner: None,
                    quality: 1.0,
                    workers: vec![],
                },
                PlayerPosition(hut_position),
                PlayerRotation(0.0),
                GoodsInventory::new(shared::economy::capacity::FISHERMANS_HUT),
                BusinessStaffingForecast {
                    expected_sales_units: 0,
                    ..BusinessStaffingForecast::manual(0, 1)
                },
            ))
            .id();
        let pier_position = Vec3::new(20.0, 0.0, 8.0);
        let pier = app
            .world_mut()
            .spawn((
                FishingPier {
                    settlement: "Fishing test".into(),
                    fishermans_hut: hut_position,
                    quality: 1.0,
                },
                AttachedTo(building_id),
                PlayerPosition(pier_position),
                PlayerRotation(0.0),
            ))
            .id();
        let (deck_start, fish_spot) = fishing_deck_points(pier_position, 0.0);
        let worker = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                CharacterName("Fisher".into()),
                EmployedAt(building_id),
                VillagerIntent::Resident { settlement: hall },
                PlayerPosition(fish_spot),
                PlayerRotation(0.0),
                CharacterActivity::Fishing,
                GoodsInventory::new(shared::economy::capacity::VILLAGER),
                FishingRoutine {
                    hut,
                    pier,
                    hall,
                    catch_seconds: 0.0,
                    failed_workplace_routes: 0,
                    production_day: 0,
                    produced_today: 0,
                    phase: FishingPhase::Fishing,
                },
            ))
            .id();
        let staging = fishing_land_point(
            app.world().resource::<WorldTerrain>(),
            hut_position,
            0.0,
            Vec2::new(-4.15, -4.45),
        );
        let entrance = SettlementBuildingKind::FishermansHut.entrance_position(hut_position, 0.0);
        Self {
            app,
            worker,
            hut,
            hall,
            fish_spot,
            deck_start,
            staging,
            entrance,
        }
    }

    fn tick(&mut self, real_seconds: f32) {
        self.app
            .world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(real_seconds));
        self.app.update();
    }

    fn place(&mut self, position: Vec3) {
        self.app
            .world_mut()
            .get_mut::<PlayerPosition>(self.worker)
            .unwrap()
            .0 = position;
        self.app
            .world_mut()
            .entity_mut(self.worker)
            .remove::<(MoveTarget, TravelRoute, PierTraversal)>();
    }

    fn phase(&self) -> FishingPhase {
        self.app
            .world()
            .get::<FishingRoutine>(self.worker)
            .unwrap()
            .phase
    }

    fn fish(&self, entity: Entity) -> u32 {
        self.app
            .world()
            .get::<GoodsInventory>(entity)
            .unwrap()
            .amount(Good::Food)
    }
}

#[test]
fn resumed_fishing_cannot_animate_or_produce_at_the_hall_at_normal_or_fast_time() {
    for warp in [1.0, 25.0] {
        let mut fixture = Fixture::new(warp);
        fixture.place(Vec3::ZERO);
        fixture
            .app
            .world_mut()
            .get_mut::<FishingRoutine>(fixture.worker)
            .unwrap()
            .catch_seconds = 240.0;
        fixture.tick(1.0);
        assert_eq!(fixture.fish(fixture.worker), 0);
        assert_eq!(fixture.fish(fixture.hut), 0);
        assert_eq!(
            *fixture
                .app
                .world()
                .get::<CharacterActivity>(fixture.worker)
                .unwrap(),
            CharacterActivity::Idle
        );
        assert!(matches!(fixture.phase(), FishingPhase::GoingToHut));
        assert_eq!(
            fixture
                .app
                .world()
                .get::<MoveTarget>(fixture.worker)
                .unwrap()
                .0,
            fixture.entrance
        );
        for _ in 0..5 {
            fixture.tick(1.0);
        }
        assert_eq!(
            fixture
                .app
                .world()
                .get::<FishingRoutine>(fixture.worker)
                .unwrap()
                .catch_seconds,
            240.0
        );
        assert_eq!(fixture.fish(fixture.worker), 0);
    }
}

#[test]
fn a_diversion_resets_fishing_phase_without_stealing_its_movement_or_presentation() {
    let mut fixture = Fixture::new(1.0);
    fixture.app.world_mut().entity_mut(fixture.worker).insert((
        HomeRoutine {
            home: fixture.hall,
            phase: HomePhase::Sleeping,
            failed_routes: 0,
        },
        MoveTarget(Vec3::ZERO),
        CharacterActivity::Sitting,
    ));
    fixture.tick(1.0);
    assert!(matches!(fixture.phase(), FishingPhase::GoingToHut));
    assert_eq!(
        fixture
            .app
            .world()
            .get::<MoveTarget>(fixture.worker)
            .unwrap()
            .0,
        Vec3::ZERO
    );
    assert_eq!(
        *fixture
            .app
            .world()
            .get::<CharacterActivity>(fixture.worker)
            .unwrap(),
        CharacterActivity::Sitting
    );
    fixture
        .app
        .world_mut()
        .entity_mut(fixture.worker)
        .remove::<HomeRoutine>();
    fixture.place(Vec3::ZERO);
    fixture.tick(1.0);
    assert_eq!(
        fixture
            .app
            .world()
            .get::<MoveTarget>(fixture.worker)
            .unwrap()
            .0,
        fixture.entrance
    );
    assert_eq!(fixture.fish(fixture.worker), 0);
}

#[test]
fn fishing_assignment_waits_for_an_existing_home_trip() {
    let mut fixture = Fixture::new(1.0);
    fixture.app.add_systems(Update, assign_fishing_routines);
    fixture
        .app
        .world_mut()
        .entity_mut(fixture.worker)
        .remove::<FishingRoutine>()
        .insert((
            HomeRoutine {
                home: fixture.hall,
                phase: HomePhase::GoingToDoor,
                failed_routes: 0,
            },
            MoveTarget(Vec3::ZERO),
        ));
    fixture.tick(0.0);
    assert!(
        fixture
            .app
            .world()
            .get::<FishingRoutine>(fixture.worker)
            .is_none()
    );
    assert_eq!(
        fixture
            .app
            .world()
            .get::<MoveTarget>(fixture.worker)
            .unwrap()
            .0,
        Vec3::ZERO
    );
    fixture
        .app
        .world_mut()
        .entity_mut(fixture.worker)
        .remove::<HomeRoutine>();
    fixture.tick(0.0);
    assert!(
        fixture
            .app
            .world()
            .get::<FishingRoutine>(fixture.worker)
            .is_some()
    );
    assert_eq!(
        fixture
            .app
            .world()
            .get::<MoveTarget>(fixture.worker)
            .unwrap()
            .0,
        fixture.entrance
    );
}

#[test]
fn a_resumed_fisher_unloads_existing_catch_before_starting_another_pier_trip() {
    let mut fixture = Fixture::new(1.0);
    fixture
        .app
        .world_mut()
        .get_mut::<FishingRoutine>(fixture.worker)
        .unwrap()
        .phase = FishingPhase::GoingToHut;
    fixture
        .app
        .world_mut()
        .get_mut::<GoodsInventory>(fixture.worker)
        .unwrap()
        .add(Good::Food, 1);
    fixture.place(fixture.entrance);
    fixture.tick(0.0);
    assert!(matches!(fixture.phase(), FishingPhase::ReturningToHut));
    fixture.tick(0.0);
    assert_eq!(fixture.fish(fixture.worker), 0);
    assert_eq!(fixture.fish(fixture.hut), 1);
    assert!(matches!(fixture.phase(), FishingPhase::Inside { .. }));
}

#[test]
fn a_lost_pier_route_recovers_over_the_deck_without_fishing_midway() {
    for warp in [1.0, 25.0] {
        let mut fixture = Fixture::new(warp);
        fixture.place(fixture.deck_start.lerp(fixture.fish_spot, 0.5));
        fixture
            .app
            .world_mut()
            .get_mut::<FishingRoutine>(fixture.worker)
            .unwrap()
            .phase = FishingPhase::WalkingToPier;
        fixture.tick(1.0);
        assert!(matches!(
            fixture.phase(),
            FishingPhase::ReturningFromPier { .. }
        ));
        let route = fixture
            .app
            .world()
            .get::<TravelRoute>(fixture.worker)
            .unwrap();
        assert_eq!(
            route.waypoints.first().unwrap().position,
            fixture.deck_start
        );
        assert_eq!(route.goal, fixture.staging);
        assert!(
            fixture
                .app
                .world()
                .get::<PierTraversal>(fixture.worker)
                .is_some()
        );
        assert_eq!(fixture.fish(fixture.worker), 0);
        assert_eq!(
            *fixture
                .app
                .world()
                .get::<CharacterActivity>(fixture.worker)
                .unwrap(),
            CharacterActivity::Idle
        );
    }
}

#[test]
fn a_fisher_two_metres_short_must_finish_walking_before_working() {
    for warp in [1.0, 25.0] {
        let mut fixture = Fixture::new(warp);
        fixture.place(fixture.fish_spot - Vec3::Z * 2.0);
        fixture
            .app
            .world_mut()
            .entity_mut(fixture.worker)
            .insert((MoveTarget(fixture.fish_spot), CharacterActivity::Idle));
        {
            let mut routine = fixture
                .app
                .world_mut()
                .get_mut::<FishingRoutine>(fixture.worker)
                .unwrap();
            routine.phase = FishingPhase::WalkingToPier;
            routine.catch_seconds = 240.0;
        }
        fixture.tick(1.0);
        assert!(matches!(fixture.phase(), FishingPhase::WalkingToPier));
        assert_eq!(fixture.fish(fixture.worker), 0);
        assert_eq!(
            *fixture
                .app
                .world()
                .get::<CharacterActivity>(fixture.worker)
                .unwrap(),
            CharacterActivity::Idle
        );
        assert_eq!(
            fixture
                .app
                .world()
                .get::<MoveTarget>(fixture.worker)
                .unwrap()
                .0,
            fixture.fish_spot
        );
    }
}

#[test]
fn interrupted_fishing_two_metres_short_cannot_animate_or_spend_banked_progress() {
    for warp in [1.0, 25.0] {
        let mut fixture = Fixture::new(warp);
        fixture.place(fixture.fish_spot - Vec3::Z * 2.0);
        fixture
            .app
            .world_mut()
            .get_mut::<FishingRoutine>(fixture.worker)
            .unwrap()
            .catch_seconds = 240.0;
        fixture.tick(1.0);
        assert!(matches!(
            fixture.phase(),
            FishingPhase::ReturningFromPier { .. }
        ));
        assert_eq!(fixture.fish(fixture.worker), 0);
        assert_eq!(
            fixture
                .app
                .world()
                .get::<FishingRoutine>(fixture.worker)
                .unwrap()
                .catch_seconds,
            240.0
        );
        assert_eq!(
            *fixture
                .app
                .world()
                .get::<CharacterActivity>(fixture.worker)
                .unwrap(),
            CharacterActivity::Idle
        );
    }
}

#[test]
fn a_fisher_at_the_authored_work_point_can_arrive_and_finish_a_catch() {
    for warp in [1.0, 25.0] {
        let mut fixture = Fixture::new(warp);
        fixture.place(fixture.fish_spot - Vec3::Z * 0.3);
        fixture
            .app
            .world_mut()
            .entity_mut(fixture.worker)
            .insert((MoveTarget(fixture.fish_spot), CharacterActivity::Idle));
        fixture
            .app
            .world_mut()
            .get_mut::<FishingRoutine>(fixture.worker)
            .unwrap()
            .phase = FishingPhase::WalkingToPier;
        fixture.tick(0.0);
        assert!(matches!(fixture.phase(), FishingPhase::Fishing));
        assert!(
            fixture
                .app
                .world()
                .get::<PierTraversal>(fixture.worker)
                .is_some()
        );
        assert_eq!(
            *fixture
                .app
                .world()
                .get::<CharacterActivity>(fixture.worker)
                .unwrap(),
            CharacterActivity::Fishing
        );
        fixture.tick(2.0 * fisher_seconds_per_food(1.0) / warp);
        assert_eq!(fixture.fish(fixture.worker), FISH_CARRY_BATCH_UNITS);
    }
}

#[test]
fn fish_require_real_work_and_deposit_then_resume_even_with_a_zero_forecast() {
    for warp in [1.0, 25.0] {
        let mut fixture = Fixture::new(warp);
        let half_batch = fisher_seconds_per_food(1.0);
        fixture.tick(half_batch / warp);
        assert_eq!(fixture.fish(fixture.worker), 0);
        assert!(matches!(fixture.phase(), FishingPhase::Fishing));
        fixture.tick(half_batch / warp);
        assert_eq!(fixture.fish(fixture.worker), 2);
        assert_eq!(fixture.fish(fixture.hut), 0);
        assert_eq!(
            fixture
                .app
                .world()
                .get::<BusinessStaffingForecast>(fixture.hut)
                .unwrap()
                .produced_output_units,
            2
        );
        assert!(matches!(
            fixture.phase(),
            FishingPhase::ReturningFromPier { .. }
        ));
        // Movement is a separate system: exercise the real arrival boundaries.
        fixture.place(fixture.staging);
        fixture.tick(0.0);
        assert!(matches!(fixture.phase(), FishingPhase::ReturningToHut));
        assert_eq!(
            fixture
                .app
                .world()
                .get::<MoveTarget>(fixture.worker)
                .unwrap()
                .0,
            fixture.entrance
        );
        fixture.place(fixture.entrance);
        fixture.tick(0.0);
        assert_eq!(fixture.fish(fixture.worker), 0);
        assert_eq!(fixture.fish(fixture.hut), 2);
        assert!(matches!(fixture.phase(), FishingPhase::Inside { .. }));
        assert!(
            fixture
                .app
                .world()
                .get::<WorkerOffDuty>(fixture.worker)
                .is_none()
        );
    }
}

#[test]
fn full_hut_storage_keeps_the_catch_and_waits_instead_of_an_empty_pier_loop() {
    let mut fixture = Fixture::new(1.0);
    fixture
        .app
        .world_mut()
        .get_mut::<GoodsInventory>(fixture.hut)
        .unwrap()
        .add(Good::Food, u32::MAX);
    fixture
        .app
        .world_mut()
        .get_mut::<GoodsInventory>(fixture.worker)
        .unwrap()
        .add(Good::Food, 2);
    fixture
        .app
        .world_mut()
        .get_mut::<FishingRoutine>(fixture.worker)
        .unwrap()
        .phase = FishingPhase::ReturningToHut;
    fixture.place(fixture.entrance);
    for _ in 0..5 {
        fixture.tick(1.0);
        assert!(matches!(fixture.phase(), FishingPhase::ReturningToHut));
        assert_eq!(fixture.fish(fixture.worker), 2);
        assert!(
            fixture
                .app
                .world()
                .get::<MoveTarget>(fixture.worker)
                .is_none()
        );
    }
    fixture
        .app
        .world_mut()
        .get_mut::<GoodsInventory>(fixture.hut)
        .unwrap()
        .remove(Good::Food, 2);
    fixture.tick(1.0);
    assert_eq!(fixture.fish(fixture.worker), 0);
    assert!(matches!(fixture.phase(), FishingPhase::Inside { .. }));
}

#[test]
fn a_completed_catch_cannot_bank_accelerated_time_for_the_next_pier_trip() {
    let mut fixture = Fixture::new(25.0);
    fixture.tick(600.0 / 25.0);
    assert_eq!(fixture.fish(fixture.worker), FISH_CARRY_BATCH_UNITS);
    assert_eq!(
        fixture
            .app
            .world()
            .get::<FishingRoutine>(fixture.worker)
            .unwrap()
            .catch_seconds,
        0.0
    );
    fixture.place(fixture.staging);
    fixture.tick(0.0);
    fixture.place(fixture.entrance);
    fixture.tick(0.0);
    assert_eq!(fixture.fish(fixture.worker), 0);
    fixture
        .app
        .world_mut()
        .entity_mut(fixture.worker)
        .remove::<(WorkplaceDoorTransit, BuildingDoorUse)>();
    fixture.place(fixture.fish_spot);
    fixture
        .app
        .world_mut()
        .get_mut::<FishingRoutine>(fixture.worker)
        .unwrap()
        .phase = FishingPhase::Fishing;
    fixture.tick(0.0);
    assert_eq!(fixture.fish(fixture.worker), 0);
    assert_eq!(fixture.fish(fixture.hut), FISH_CARRY_BATCH_UNITS);
}

#[test]
fn paid_meal_waits_for_physical_pier_exit_and_preserves_the_ration_at_both_warps() {
    for warp in [1.0, 25.0] {
        let mut fixture = Fixture::new(warp);
        // This test drives the authored phase boundaries explicitly; the public
        // counter still needs real dry support. Procedural origin terrain is
        // not a valid forecourt merely because the Hall fixture sits there.
        let hall_position = Vec3::new(0.0, 80.0, 0.0);
        fixture
            .app
            .world_mut()
            .resource_mut::<WorldTerrain>()
            .apply_flatten_rect(hall_position, Vec2::splat(10.0), 0.0, 4.0);
        fixture
            .app
            .world_mut()
            .get_mut::<PlayerPosition>(fixture.hall)
            .unwrap()
            .0 = hall_position;
        fixture
            .app
            .world_mut()
            .get_mut::<FishingRoutine>(fixture.worker)
            .unwrap()
            .phase = FishingPhase::WalkingToPier;
        fixture.tick(0.0);
        assert!(
            fixture
                .app
                .world()
                .get::<PierTraversal>(fixture.worker)
                .is_some()
        );
        let region = shared::region::RegionCoord::new(0, 0);
        fixture
            .app
            .init_resource::<MootQueueClock>()
            .init_resource::<RegionRegistry>();
        fixture
            .app
            .world_mut()
            .resource_mut::<RegionRegistry>()
            .set_observers_for_test(region, 1);
        let mut food = GoodsInventory::new(shared::economy::capacity::HALL);
        food.add(Good::Bread, 1);
        let mut market = MootMarket::founding();
        market.consign(MarketSeller::Treasury(SettlementId(1)), Good::Bread, 1, 20);
        fixture.app.world_mut().entity_mut(fixture.hall).insert((
            Settlement {
                name: "Fishing test".into(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            },
            SettlementEconomy::default(),
            PlayerRotation(0.0),
            food,
            market,
        ));
        fixture.app.world_mut().entity_mut(fixture.worker).insert((
            shared::components::PersonId(1),
            shared::components::ResidentOf(SettlementId(1)),
            Wallet::new(100),
            Nutrition::default(),
            region,
        ));
        fixture.app.add_systems(
            Update,
            (
                update_settlement_economies,
                apply_business_events,
                advance_moot_service_queues,
                run_moot_meal_collections,
            )
                .chain()
                .before(run_fishing_routines),
        );
        fixture.tick(0.0); // establish the initial economy observation
        {
            let world = fixture.app.world_mut();
            world
                .query::<&mut WorldTime>()
                .single_mut(world)
                .unwrap()
                .day += 1;
        }
        fixture.tick(0.0);
        assert!(matches!(
            fixture.phase(),
            FishingPhase::ReturningFromPier { .. }
        ));
        assert!(
            fixture
                .app
                .world()
                .get::<MootMealRoutine>(fixture.worker)
                .is_some()
        );
        assert!(
            fixture
                .app
                .world()
                .get::<MootQueueTicket>(fixture.worker)
                .is_some()
        );
        assert_eq!(
            fixture
                .app
                .world()
                .get::<MoveTarget>(fixture.worker)
                .unwrap()
                .0,
            fixture.staging
        );
        assert_eq!(
            fixture
                .app
                .world()
                .get::<Wallet>(fixture.worker)
                .unwrap()
                .balance(),
            80
        );
        assert_eq!(
            fixture
                .app
                .world()
                .get::<Settlement>(fixture.hall)
                .unwrap()
                .treasury,
            20
        );
        assert_eq!(
            fixture
                .app
                .world()
                .get::<GoodsInventory>(fixture.hall)
                .unwrap()
                .amount(Good::Bread),
            0
        );
        assert_eq!(
            fixture
                .app
                .world()
                .get::<Nutrition>(fixture.worker)
                .unwrap()
                .total_meals,
            0
        );
        let route = fixture
            .app
            .world()
            .get::<TravelRoute>(fixture.worker)
            .unwrap()
            .clone();
        assert_eq!(
            route.waypoints.first().unwrap().position,
            fixture.deck_start
        );
        // Movement is tested separately. Drive each actual authored arrival
        // boundary without deleting the ownership marker as Fixture::place does.
        for waypoint in &route.waypoints {
            assert!(
                fixture
                    .app
                    .world()
                    .get::<PierTraversal>(fixture.worker)
                    .is_some()
            );
            fixture
                .app
                .world_mut()
                .get_mut::<PlayerPosition>(fixture.worker)
                .unwrap()
                .0 = waypoint.position;
            fixture.tick(0.1 / warp);
            assert_eq!(fixture.fish(fixture.worker), 0);
            assert_eq!(
                fixture
                    .app
                    .world()
                    .get::<Nutrition>(fixture.worker)
                    .unwrap()
                    .total_meals,
                0
            );
        }
        assert!(
            fixture
                .app
                .world()
                .get::<PierTraversal>(fixture.worker)
                .is_none()
        );
        assert!(
            fixture
                .app
                .world()
                .get::<MootQueueTicket>(fixture.worker)
                .is_some()
        );
        fixture.tick(0.0); // admit the waiting ticket after the dry-land handoff
        fixture.tick(0.0); // acquire the public counter destination
        let counter = fixture
            .app
            .world()
            .get::<MoveTarget>(fixture.worker)
            .unwrap()
            .0;
        assert_ne!(counter, fixture.staging);
        assert!(crate::world::bridges::segment_walkable(
            fixture.app.world().resource::<WorldTerrain>(),
            None,
            counter.xz(),
            counter.xz(),
            shared::physics::CHARACTER_NAV_RADIUS,
        ));
        fixture
            .app
            .world_mut()
            .get_mut::<PlayerPosition>(fixture.worker)
            .unwrap()
            .0 = counter;
        fixture.tick(0.0);
        fixture.tick(3.0 / warp);
        assert!(
            fixture
                .app
                .world()
                .get::<MootQueueTicket>(fixture.worker)
                .is_none()
        );
        let commons = fixture
            .app
            .world()
            .get::<MoveTarget>(fixture.worker)
            .unwrap()
            .0;
        fixture
            .app
            .world_mut()
            .get_mut::<PlayerPosition>(fixture.worker)
            .unwrap()
            .0 = commons;
        fixture.tick(0.0);
        fixture.tick(60.0 / warp);
        assert!(
            fixture
                .app
                .world()
                .get::<MootMealRoutine>(fixture.worker)
                .is_none()
        );
        assert_eq!(
            fixture
                .app
                .world()
                .get::<Nutrition>(fixture.worker)
                .unwrap()
                .total_meals,
            1
        );
        assert_eq!(
            fixture
                .app
                .world()
                .get::<GoodsInventory>(fixture.worker)
                .unwrap()
                .amount(Good::Bread),
            0
        );
        assert_eq!(
            fixture
                .app
                .world()
                .get::<Wallet>(fixture.worker)
                .unwrap()
                .balance()
                + fixture
                    .app
                    .world()
                    .get::<Settlement>(fixture.hall)
                    .unwrap()
                    .treasury,
            100
        );
        assert_eq!(
            fixture
                .app
                .world()
                .get::<MoveTarget>(fixture.worker)
                .unwrap()
                .0,
            fixture.entrance
        );
    }
}

#[test]
fn sleep_waits_for_an_empty_handed_fisher_to_finish_the_authored_pier_exit() {
    for warp in [1.0, 25.0] {
        let mut fixture = Fixture::new(warp);
        fixture
            .app
            .world_mut()
            .get_mut::<FishingRoutine>(fixture.worker)
            .unwrap()
            .phase = FishingPhase::WalkingToPier;
        fixture.tick(0.0);
        let person = shared::components::PersonId(1);
        let home = fixture
            .app
            .world_mut()
            .spawn((
                SettlementBuilding {
                    kind: SettlementBuildingKind::House,
                    settlement: "Fishing test".into(),
                    owner: None,
                    quality: 1.0,
                    workers: vec![],
                },
                PlayerPosition(Vec3::new(80.0, 3.0, 0.0)),
                PlayerRotation(0.0),
                Household {
                    residents: vec!["Fisher".into()],
                    resident_ids: vec![person],
                },
            ))
            .id();
        fixture
            .app
            .world_mut()
            .entity_mut(fixture.worker)
            .insert((person, HomeAssignment { home }));
        {
            let world = fixture.app.world_mut();
            world
                .query::<&mut WorldTime>()
                .single_mut(world)
                .unwrap()
                .set_normalized_time(23.5 / 24.0);
        }
        fixture
            .app
            .add_systems(Update, run_household_schedules.before(run_fishing_routines));
        fixture.tick(0.0);
        assert!(
            fixture
                .app
                .world()
                .get::<HomeRoutine>(fixture.worker)
                .is_none()
        );
        assert_eq!(
            fixture
                .app
                .world()
                .get::<MoveTarget>(fixture.worker)
                .unwrap()
                .0,
            fixture.staging
        );
        assert!(matches!(
            fixture.phase(),
            FishingPhase::ReturningFromPier { .. }
        ));
        fixture
            .app
            .world_mut()
            .get_mut::<PlayerPosition>(fixture.worker)
            .unwrap()
            .0 = fixture.deck_start;
        fixture.tick(0.0);
        assert!(
            fixture
                .app
                .world()
                .get::<HomeRoutine>(fixture.worker)
                .is_none()
        );
        fixture
            .app
            .world_mut()
            .get_mut::<PlayerPosition>(fixture.worker)
            .unwrap()
            .0 = fixture.staging;
        fixture.tick(0.0);
        assert!(
            fixture
                .app
                .world()
                .get::<PierTraversal>(fixture.worker)
                .is_none()
        );
        fixture.tick(0.0);
        assert!(
            fixture
                .app
                .world()
                .get::<HomeRoutine>(fixture.worker)
                .is_some()
        );
        assert_eq!(fixture.fish(fixture.worker), 0);
    }
}
