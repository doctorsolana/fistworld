//! Service ownership must outlive the worker's final doorway crossing.

use super::super::worker_activity::EmploymentReleaseRequested;
use super::*;
use bevy::ecs::system::RunSystemOnce;
use shared::components::{BuildingId, EmployedAt, PersonId, TimeWarp};

struct Fixture {
    app: App,
    tavern: Entity,
    worker: Entity,
    inside: Vec3,
    outside: Vec3,
    warp: f32,
}

impl Fixture {
    fn new(warp: f32, entering: bool) -> Self {
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<moot_services::MootQueueClock>()
            .add_systems(
                Update,
                (
                    enforce_staffing_targets,
                    moot_services::advance_moot_service_queues,
                    run_workplace_service_handoffs,
                    run_tavern_routines,
                    run_workplace_door_transits,
                )
                    .chain(),
            );
        let mut clock = WorldTime::new_default();
        clock.set_normalized_time(18.5 / 24.0);
        app.world_mut().spawn((clock, TimeWarp::clamped(warp)));
        let center = Vec3::ZERO;
        let kind = SettlementBuildingKind::Tavern;
        let entrance = kind.entrance_position(center, 0.0);
        let inside = kind.interior_door_position(center, 0.0);
        let outside = exterior_door_clearance_position(center, entrance);
        let tavern = app
            .world_mut()
            .spawn((
                BuildingId(7),
                SettlementBuilding {
                    kind,
                    settlement: "Serviceford".into(),
                    owner: None,
                    quality: 1.0,
                    workers: vec!["Innkeeper".into()],
                },
                PlayerPosition(center),
                PlayerRotation(0.0),
                GoodsInventory::new(shared::economy::capacity::TAVERN),
                BusinessSalePolicy::default(),
                BusinessAccount::default(),
                BusinessCondition {
                    state: BusinessState::Operating,
                    ..default()
                },
                BusinessStaffingPolicy::new(1),
                shared::components::OperatedBy(shared::components::CompanyId(1)),
                TavernService::default(),
            ))
            .id();
        let worker = app
            .world_mut()
            .spawn((
                PersonId(1),
                CharacterKind::Villager,
                PlayerPosition(if entering { entrance } else { inside }),
                PlayerRotation(0.0),
                VillagerIntent::Resident { settlement: tavern },
                EmployedAt(BuildingId(7)),
                Occupation(Some("Innkeeper".into())),
                WorkStatus::Employed,
                Wallet::new(80),
                GoodsInventory::new(shared::economy::capacity::VILLAGER),
                CharacterActivity::Indoors,
                TavernWorkerRoutine {
                    tavern,
                    phase: if entering {
                        TavernWorkerPhase::Entering
                    } else {
                        TavernWorkerPhase::Serving
                    },
                },
            ))
            .id();
        app.world_mut()
            .entity_mut(worker)
            .insert(WorkplaceInterior {
                building: center,
                door: entrance,
                inside,
            });
        if entering {
            app.world_mut()
                .entity_mut(worker)
                .insert(WorkplaceDoorTransit {
                    building: center,
                    door: entrance,
                    inside,
                    direction: WorkplaceDoorDirection::Entering,
                    phase: WorkplaceDoorPhase::Opening {
                        seconds_left: DOOR_OPEN_SECONDS,
                    },
                    destination_after_exit: None,
                });
        }
        Self {
            app,
            tavern,
            worker,
            inside,
            outside,
            warp,
        }
    }

    fn tick(&mut self, world_seconds: f32) {
        self.app
            .world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(
                world_seconds / self.warp,
            ));
        self.app.update();
    }

    fn arrive(&mut self, position: Vec3) {
        // This fixture supplies physical arrivals; the real door systems decide
        // whether those arrivals allow a handoff. It never advances a phase.
        self.app
            .world_mut()
            .get_mut::<PlayerPosition>(self.worker)
            .unwrap()
            .0 = position;
        self.tick(0.0);
    }

    fn finish_entry_if_needed(&mut self, entering: bool) {
        self.tick(2.0);
        if entering {
            assert_eq!(
                self.app
                    .world()
                    .get::<WorkplaceDoorTransit>(self.worker)
                    .unwrap()
                    .direction,
                WorkplaceDoorDirection::Entering
            );
            self.arrive(self.inside);
            self.tick(0.0);
        }
        assert_eq!(
            self.app
                .world()
                .get::<WorkplaceDoorTransit>(self.worker)
                .unwrap()
                .direction,
            WorkplaceDoorDirection::Leaving
        );
    }

    fn finish_exit(&mut self) {
        self.tick(2.0);
        assert_eq!(
            self.app.world().get::<MoveTarget>(self.worker).unwrap().0,
            self.outside
        );
        // Time alone must not release a worker who is still behind the wall.
        self.tick(60.0);
        assert!(
            self.app
                .world()
                .get::<TavernWorkerRoutine>(self.worker)
                .is_some()
        );
        assert!(self.app.world().get::<EmployedAt>(self.worker).is_some());
        self.arrive(self.outside);
        self.tick(0.0);
        assert!(
            self.app
                .world()
                .get::<TavernWorkerRoutine>(self.worker)
                .is_none()
        );
        assert!(
            self.app
                .world()
                .get::<WorkplaceDoorTransit>(self.worker)
                .is_none()
        );
        assert!(
            self.app
                .world()
                .get::<BuildingDoorUse>(self.worker)
                .is_none()
        );
        assert_eq!(
            *self
                .app
                .world()
                .get::<CharacterActivity>(self.worker)
                .unwrap(),
            CharacterActivity::Idle
        );
    }
}

#[test]
fn dismissed_innkeeper_keeps_the_job_until_the_real_door_exit_finishes() {
    for warp in [1.0, 25.0] {
        for entering in [false, true] {
            let mut fixture = Fixture::new(warp, entering);
            fixture
                .app
                .world_mut()
                .get_mut::<BusinessStaffingPolicy>(fixture.tavern)
                .unwrap()
                .enabled_positions = 0;
            fixture.tick(0.0);
            assert!(
                fixture
                    .app
                    .world()
                    .get::<EmploymentReleaseRequested>(fixture.worker)
                    .is_some()
            );
            assert!(
                fixture
                    .app
                    .world()
                    .get::<EmployedAt>(fixture.worker)
                    .is_some()
            );
            fixture.finish_entry_if_needed(entering);
            fixture.finish_exit();
            fixture.tick(0.0);
            assert!(
                fixture
                    .app
                    .world()
                    .get::<EmployedAt>(fixture.worker)
                    .is_none()
            );
            assert!(
                fixture
                    .app
                    .world()
                    .get::<EmploymentReleaseRequested>(fixture.worker)
                    .is_none()
            );
            assert_eq!(
                *fixture
                    .app
                    .world()
                    .get::<WorkStatus>(fixture.worker)
                    .unwrap(),
                WorkStatus::LookingForWork
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
        }
    }
}

#[test]
fn closed_tavern_finishes_entry_then_exits_without_waiting_for_the_shift_end() {
    for entering in [false, true] {
        let mut fixture = Fixture::new(1.0, entering);
        fixture
            .app
            .world_mut()
            .get_mut::<BusinessCondition>(fixture.tavern)
            .unwrap()
            .state = BusinessState::Closed;
        fixture.tick(0.0);
        fixture.finish_entry_if_needed(entering);
        fixture.finish_exit();
        assert!(
            fixture
                .app
                .world()
                .get::<WorkerOffDuty>(fixture.worker)
                .is_some()
        );
    }
}

#[test]
fn pending_paid_service_waits_for_the_innkeepers_exit_without_ending_the_shift() {
    for warp in [1.0, 25.0] {
        let mut fixture = Fixture::new(warp, true);
        let worker = fixture.worker;
        let hall = fixture
            .app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Serviceford".into(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 1,
                    treasury: 20,
                },
                PlayerPosition(Vec3::new(50.0, 0.0, 50.0)),
                PlayerRotation(0.0),
            ))
            .id();
        fixture
            .app
            .world_mut()
            .run_system_once(
                move |mut commands: Commands, mut clock: ResMut<moot_services::MootQueueClock>| {
                    moot_services::reserve_meal(
                        &mut commands,
                        &mut clock,
                        worker,
                        hall,
                        moot_services::MootServiceKind::PersonalMeal,
                        Good::Bread,
                        0,
                    );
                },
            )
            .unwrap();
        fixture.tick(0.0);
        fixture.finish_entry_if_needed(true);
        fixture.finish_exit();
        // Service owns its reserved ration throughout the safe handoff; this
        // temporary interruption permits normal innkeeper admission afterward.
        assert!(fixture.app.world().get::<WorkerOffDuty>(worker).is_none());
        assert!(fixture.app.world().get::<EmployedAt>(worker).is_some());
        assert_eq!(
            fixture
                .app
                .world()
                .get::<MootMealRoutine>(worker)
                .unwrap()
                .good,
            Good::Bread
        );
        assert_eq!(
            fixture.app.world().get::<Wallet>(worker).unwrap().balance(),
            80
        );
        fixture.tick(0.0);
        fixture.tick(0.0);
        assert!(fixture.app.world().get::<MootQueueTicket>(worker).is_some());
        assert!(
            ground_distance(
                fixture.app.world().get::<MoveTarget>(worker).unwrap().0,
                Vec3::new(50.0, 0.0, 50.0)
            ) < 15.0
        );
    }
}
