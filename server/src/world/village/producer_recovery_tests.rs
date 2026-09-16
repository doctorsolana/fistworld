//! Physical work must survive diversions without creating output remotely.

use super::*;
use shared::components::{
    AttachedTo, BuildingId, BuildingOf, EmployedAt, HouseholdId, SettlementId,
};

#[derive(Clone, Copy)]
enum Producer {
    Farmer,
    Lumberjack,
}

impl Producer {
    fn good(self) -> Good {
        match self {
            Self::Farmer => Good::Wheat,
            Self::Lumberjack => Good::Wood,
        }
    }

    fn batch(self) -> u32 {
        match self {
            Self::Farmer => FARM_CARRY_BATCH_UNITS,
            Self::Lumberjack => lumber_tree_yield(1.0),
        }
    }
}

struct Fixture {
    app: App,
    producer: Producer,
    hall: Entity,
    workplace: Entity,
    worker: Entity,
    entrance: Vec3,
    stand: Vec3,
}

impl Fixture {
    fn new(producer: Producer) -> Self {
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<SettlementEconomyRuntime>()
            .init_resource::<BusinessEventQueue>()
            .insert_resource(WorldTerrain::default());
        match producer {
            Producer::Farmer => app.add_systems(Update, run_farmer_routines),
            Producer::Lumberjack => app.add_systems(Update, run_lumberjack_routines),
        };
        app.world_mut().spawn(WorldTime::new_default());
        let settlement = SettlementId(1);
        let building = BuildingId(2);
        let hall = app.world_mut().spawn(settlement).id();
        let position = Vec3::new(20.0, 4.0, 0.0);
        let stand = Vec3::new(32.0, 4.0, 0.0);
        let kind = match producer {
            Producer::Farmer => SettlementBuildingKind::Farmstead,
            Producer::Lumberjack => SettlementBuildingKind::LumberjackHut,
        };
        let workplace = app
            .world_mut()
            .spawn((
                building,
                BuildingOf(settlement),
                PlayerPosition(position),
                PlayerRotation(0.0),
                SettlementBuilding {
                    kind,
                    settlement: "Test".into(),
                    owner: None,
                    quality: 1.0,
                    workers: vec!["Worker".into()],
                },
                GoodsInventory::new(100),
                BusinessStaffingForecast {
                    day: 0,
                    expected_sales_units: 0,
                    produced_output_units: 0,
                    optimal_positions: 1,
                    marginal_daily_profit: 0,
                },
            ))
            .id();
        let worker = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                CharacterName("Worker".into()),
                EmployedAt(building),
                PlayerPosition(stand),
                PlayerRotation(0.0),
                VillagerIntent::Resident { settlement: hall },
                CharacterActivity::Idle,
                GoodsInventory::new(shared::economy::capacity::VILLAGER),
            ))
            .id();
        match producer {
            Producer::Farmer => {
                let field = app
                    .world_mut()
                    .spawn((
                        FarmField {
                            shape: None,
                            plot_index: 0,
                            settlement: "Test".into(),
                            farmstead: position,
                            layout_version: 0,
                            quality: 1.0,
                        },
                        AttachedTo(building),
                    ))
                    .id();
                app.world_mut().entity_mut(worker).insert(FarmerRoutine {
                    farmstead: workplace,
                    field,
                    hall,
                    work_stand: stand,
                    harvest_seconds: farmer_seconds_per_wheat(1.0) * producer.batch() as f32,
                    failed_workplace_routes: 0,
                    production_day: 0,
                    produced_today: 0,
                    phase: FarmerPhase::Farming,
                });
            }
            Producer::Lumberjack => {
                app.world_mut()
                    .entity_mut(worker)
                    .insert(LumberjackRoutine {
                        hut: workplace,
                        hall,
                        cycle: 0,
                        failed_tree_routes: 0,
                        failed_hut_routes: 0,
                        chop_seconds: lumber_seconds_per_tree(1.0),
                        production_day: 0,
                        produced_today: 0,
                        phase: LumberjackPhase::Chopping {
                            tree: stand - Vec3::Z,
                            stand,
                        },
                    });
            }
        }
        Self {
            app,
            producer,
            hall,
            workplace,
            worker,
            entrance: kind.entrance_position(position, 0.0),
            stand,
        }
    }

    fn carried(&self) -> u32 {
        self.app
            .world()
            .get::<GoodsInventory>(self.worker)
            .unwrap()
            .amount(self.producer.good())
    }

    fn progress(&self) -> f32 {
        match self.producer {
            Producer::Farmer => {
                self.app
                    .world()
                    .get::<FarmerRoutine>(self.worker)
                    .unwrap()
                    .harvest_seconds
            }
            Producer::Lumberjack => {
                self.app
                    .world()
                    .get::<LumberjackRoutine>(self.worker)
                    .unwrap()
                    .chop_seconds
            }
        }
    }

    fn set_inside(&mut self) {
        match self.producer {
            Producer::Farmer => {
                self.app
                    .world_mut()
                    .get_mut::<FarmerRoutine>(self.worker)
                    .unwrap()
                    .phase = FarmerPhase::Inside { seconds_left: 0.0 }
            }
            Producer::Lumberjack => {
                self.app
                    .world_mut()
                    .get_mut::<LumberjackRoutine>(self.worker)
                    .unwrap()
                    .phase = LumberjackPhase::Inside { seconds_left: 0.0 }
            }
        }
    }
}

#[test]
fn farmers_and_woodcutters_work_past_a_zero_output_forecast() {
    for producer in [Producer::Farmer, Producer::Lumberjack] {
        let mut f = Fixture::new(producer);
        f.app.update();
        assert_eq!(f.carried(), producer.batch());
        assert_eq!(
            f.app
                .world()
                .get::<BusinessStaffingForecast>(f.workplace)
                .unwrap()
                .produced_output_units,
            producer.batch()
        );
        assert!(f.app.world().get::<WorkerOffDuty>(f.worker).is_none());
    }
}

#[test]
fn a_completed_producer_load_cannot_bank_accelerated_time_for_the_next_trip() {
    for producer in [Producer::Farmer, Producer::Lumberjack] {
        let mut f = Fixture::new(producer);
        f.app
            .world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs(600));
        f.app.update();
        assert_eq!(f.carried(), producer.batch());
        assert_eq!(f.progress(), 0.0);

        // Arrive, deposit, and begin a fresh field/tree visit without any
        // additional labour. Only the previously carried load may exist.
        f.app
            .world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::ZERO);
        f.app
            .world_mut()
            .get_mut::<PlayerPosition>(f.worker)
            .unwrap()
            .0 = f.entrance;
        f.app.update();
        assert_eq!(f.carried(), 0);
        f.app
            .world_mut()
            .entity_mut(f.worker)
            .remove::<(WorkplaceDoorTransit, BuildingDoorUse, MoveTarget)>();
        f.app
            .world_mut()
            .get_mut::<PlayerPosition>(f.worker)
            .unwrap()
            .0 = f.stand;
        match producer {
            Producer::Farmer => {
                f.app
                    .world_mut()
                    .get_mut::<FarmerRoutine>(f.worker)
                    .unwrap()
                    .phase = FarmerPhase::Farming
            }
            Producer::Lumberjack => {
                f.app
                    .world_mut()
                    .get_mut::<LumberjackRoutine>(f.worker)
                    .unwrap()
                    .phase = LumberjackPhase::Chopping {
                    tree: f.stand - Vec3::Z,
                    stand: f.stand,
                }
            }
        }
        f.app.update();
        assert_eq!(f.carried(), 0);
        assert_eq!(
            f.app
                .world()
                .get::<GoodsInventory>(f.workplace)
                .unwrap()
                .amount(producer.good()),
            producer.batch()
        );
    }
}

#[test]
fn a_displaced_producer_must_reach_the_work_stand_before_producing() {
    for producer in [Producer::Farmer, Producer::Lumberjack] {
        let mut f = Fixture::new(producer);
        let progress = f.progress();
        f.app
            .world_mut()
            .get_mut::<PlayerPosition>(f.worker)
            .unwrap()
            .0 = Vec3::ZERO;
        f.app.update();
        assert_eq!(f.carried(), 0);
        assert_eq!(f.progress(), progress);
        assert_eq!(
            f.app.world().get::<MoveTarget>(f.worker).unwrap().0,
            f.stand
        );
        assert_eq!(
            *f.app.world().get::<CharacterActivity>(f.worker).unwrap(),
            CharacterActivity::Idle
        );
        f.app
            .world_mut()
            .get_mut::<PlayerPosition>(f.worker)
            .unwrap()
            .0 = f.stand;
        f.app.update();
        f.app.update();
        assert_eq!(f.carried(), producer.batch());
    }
}

#[test]
fn household_shopping_resumes_through_the_workplace_without_losing_progress() {
    for producer in [Producer::Farmer, Producer::Lumberjack] {
        let mut f = Fixture::new(producer);
        let progress = f.progress();
        let shopping_destination = Vec3::new(-40.0, 4.0, -20.0);
        f.app.world_mut().entity_mut(f.worker).insert((
            PlayerPosition(shopping_destination),
            MoveTarget(shopping_destination),
            HouseholdShoppingRoutine {
                account: f.hall,
                household: HouseholdId(7),
                home: f.hall,
                hall: f.hall,
                counter: shopping_destination,
                phase: HouseholdShoppingPhase::GoingToMarket,
                cargo: [0; Good::COUNT],
            },
        ));
        f.app.update();
        assert_eq!(f.carried(), 0);
        assert_eq!(f.progress(), progress);
        assert_eq!(
            f.app.world().get::<MoveTarget>(f.worker).unwrap().0,
            shopping_destination
        );
        f.app
            .world_mut()
            .entity_mut(f.worker)
            .remove::<HouseholdShoppingRoutine>();
        f.app.update();
        assert_eq!(f.carried(), 0);
        assert_eq!(f.progress(), progress);
        assert_eq!(
            f.app.world().get::<MoveTarget>(f.worker).unwrap().0,
            f.entrance
        );
        assert_eq!(
            f.app.world().get::<PlayerPosition>(f.worker).unwrap().0,
            shopping_destination
        );
    }
}

#[test]
fn a_full_store_pauses_empty_workers_without_repeated_field_or_tree_trips() {
    for producer in [Producer::Farmer, Producer::Lumberjack] {
        let mut f = Fixture::new(producer);
        f.app
            .world_mut()
            .get_mut::<GoodsInventory>(f.workplace)
            .unwrap()
            .add(producer.good(), u32::MAX);
        f.set_inside();
        let progress = f.progress();
        for _ in 0..3 {
            f.app.update();
            assert_eq!(f.carried(), 0);
            assert_eq!(f.progress(), progress);
            assert!(f.app.world().get::<MoveTarget>(f.worker).is_none());
            assert!(
                f.app
                    .world()
                    .get::<WorkplaceDoorTransit>(f.worker)
                    .is_none()
            );
            assert!(f.app.world().get::<WorkerOffDuty>(f.worker).is_none());
        }
    }
}
