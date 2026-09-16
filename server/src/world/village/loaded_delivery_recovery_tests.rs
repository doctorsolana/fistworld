//! Exhausted route retries cannot stand in for a physical goods delivery.

use super::*;
use crate::world::pathfinding::PathfindingBudgetSettings;
use crate::world::village_roads::{
    NavigationRouteBackoff, VillageRoadGraph, plan_villager_travel_routes,
    queue_villager_travel_routes, retry_failed_routes_after_obstacle_change,
};
use shared::components::{AttachedTo, BuildingId, BuildingOf, EmployedAt, SettlementId, TimeWarp};
use shared::economy::Wallet;
use shared::spatial::{ObstacleEntry, SpatialObstacleGrid};
use std::time::Duration;

#[derive(Clone, Copy, Debug)]
enum Trade {
    Farmer,
    Fisher,
    Lumberjack,
}

impl Trade {
    fn kind(self) -> SettlementBuildingKind {
        match self {
            Self::Farmer => SettlementBuildingKind::Farmstead,
            Self::Fisher => SettlementBuildingKind::FishermansHut,
            Self::Lumberjack => SettlementBuildingKind::LumberjackHut,
        }
    }

    fn good(self) -> Good {
        match self {
            Self::Farmer => Good::Wheat,
            Self::Fisher => Good::Food,
            Self::Lumberjack => Good::Wood,
        }
    }

    fn still_returning(self, world: &World, worker: Entity) -> bool {
        match self {
            Self::Farmer => world.get::<FarmerRoutine>(worker).is_some_and(|r| {
                matches!(r.phase, FarmerPhase::ReturningToFarmstead)
                    && r.failed_workplace_routes == MAX_WORKPLACE_ROUTE_FAILURES
                    && r.harvest_seconds == 7.0
                    && r.produced_today == 5
            }),
            Self::Fisher => world.get::<FishingRoutine>(worker).is_some_and(|r| {
                matches!(r.phase, FishingPhase::ReturningToHut)
                    && r.failed_workplace_routes == MAX_WORKPLACE_ROUTE_FAILURES
                    && r.catch_seconds == 7.0
                    && r.produced_today == 5
            }),
            Self::Lumberjack => world.get::<LumberjackRoutine>(worker).is_some_and(|r| {
                matches!(r.phase, LumberjackPhase::ReturningToHut)
                    && r.failed_hut_routes == MAX_WORKPLACE_ROUTE_FAILURES
                    && r.chop_seconds == 7.0
                    && r.produced_today == 5
            }),
        }
    }
}

struct Fixture {
    app: App,
    trade: Trade,
    worker: Entity,
    workplace: Entity,
    entrance: Vec3,
    start: Vec3,
}

impl Fixture {
    fn new(trade: Trade, warp: f32) -> Self {
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<SettlementEconomyRuntime>()
            .init_resource::<BusinessEventQueue>()
            .init_resource::<VillageRoadGraph>()
            .init_resource::<PathfindingBudgetSettings>()
            .init_resource::<SpatialObstacleGrid>()
            .insert_resource(super::super::tests::fixtures::dry_test_terrain())
            .add_systems(
                Update,
                (
                    run_farmer_routines,
                    run_fishing_routines,
                    run_lumberjack_routines,
                    queue_villager_travel_routes,
                    retry_failed_routes_after_obstacle_change,
                    plan_villager_travel_routes,
                    crate::player::hero::step_units,
                )
                    .chain(),
            );
        // End of shift: after its one legitimate load arrives there is no new
        // production to obscure duplicate deposits or failure-created output.
        let mut clock = WorldTime::new_default();
        clock.set_normalized_time(20.0 / 24.0);
        app.world_mut().spawn((clock, TimeWarp::clamped(warp)));
        let hall = app.world_mut().spawn(SettlementId(1)).id();
        let building_id = BuildingId(2);
        let site = Vec3::new(20.0, 0.0, 0.0);
        let entrance = trade.kind().entrance_position(site, 0.0);
        let start = entrance + Vec3::X * 24.0;
        let workplace = app
            .world_mut()
            .spawn((
                building_id,
                BuildingOf(SettlementId(1)),
                SettlementBuilding {
                    kind: trade.kind(),
                    settlement: "Delivery test".into(),
                    owner: None,
                    quality: 1.0,
                    workers: vec![],
                },
                PlayerPosition(site),
                PlayerRotation(0.0),
                GoodsInventory::new(100),
                BusinessStaffingForecast::manual(0, 1),
            ))
            .id();
        let mut inventory = GoodsInventory::new(shared::economy::capacity::VILLAGER);
        assert_eq!(inventory.add(trade.good(), 5), 5);
        let worker = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                CharacterName("Carrier".into()),
                EmployedAt(building_id),
                VillagerIntent::Resident { settlement: hall },
                PlayerPosition(start),
                PlayerRotation(0.0),
                RegionCoord::from_world_pos(start),
                CharacterActivity::Idle,
                inventory,
                Wallet::new(73),
                MoveTarget(entrance),
            ))
            .id();
        match trade {
            Trade::Farmer => {
                let field = app
                    .world_mut()
                    .spawn((
                        FarmField {
                            shape: None,
                            plot_index: 0,
                            settlement: "Delivery test".into(),
                            farmstead: site,
                            layout_version: 0,
                            quality: 1.0,
                        },
                        AttachedTo(building_id),
                    ))
                    .id();
                app.world_mut().entity_mut(worker).insert(FarmerRoutine {
                    farmstead: workplace,
                    field,
                    hall,
                    work_stand: start,
                    harvest_seconds: 7.0,
                    failed_workplace_routes: MAX_WORKPLACE_ROUTE_FAILURES,
                    production_day: 0,
                    produced_today: 5,
                    phase: FarmerPhase::ReturningToFarmstead,
                });
            }
            Trade::Fisher => {
                let pier = app
                    .world_mut()
                    .spawn((
                        FishingPier {
                            settlement: "Delivery test".into(),
                            fishermans_hut: site,
                            quality: 1.0,
                        },
                        AttachedTo(building_id),
                        PlayerPosition(site + Vec3::Z * 8.0),
                        PlayerRotation(0.0),
                    ))
                    .id();
                app.world_mut().entity_mut(worker).insert(FishingRoutine {
                    hut: workplace,
                    pier,
                    hall,
                    catch_seconds: 7.0,
                    failed_workplace_routes: MAX_WORKPLACE_ROUTE_FAILURES,
                    production_day: 0,
                    produced_today: 5,
                    phase: FishingPhase::ReturningToHut,
                });
            }
            Trade::Lumberjack => {
                app.world_mut()
                    .entity_mut(worker)
                    .insert(LumberjackRoutine {
                        hut: workplace,
                        hall,
                        cycle: 0,
                        failed_tree_routes: 0,
                        failed_hut_routes: MAX_WORKPLACE_ROUTE_FAILURES,
                        chop_seconds: 7.0,
                        production_day: 0,
                        produced_today: 5,
                        phase: LumberjackPhase::ReturningToHut,
                    });
            }
        }
        // A real solid obstacle makes the requested store entrance impossible.
        // Navigation itself must create the retained failure/backoff state.
        app.world_mut()
            .resource_mut::<SpatialObstacleGrid>()
            .insert(ObstacleEntry {
                center: entrance.xz(),
                half_extents: Vec2::splat(5.0),
                rotation: 0.0,
                obstacle_type: 0,
            });
        Self {
            app,
            trade,
            worker,
            workplace,
            entrance,
            start,
        }
    }

    fn tick(&mut self) {
        self.app
            .world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs_f64(1.0 / 60.0));
        self.app.update();
    }

    fn amount(&self, actor: Entity) -> u32 {
        self.app
            .world()
            .get::<GoodsInventory>(actor)
            .unwrap()
            .amount(self.trade.good())
    }

    fn assert_retained(&self) {
        let world = self.app.world();
        assert_eq!(
            world.get::<PlayerPosition>(self.worker).unwrap().0,
            self.start
        );
        assert_eq!(self.amount(self.worker), 5);
        assert_eq!(self.amount(self.workplace), 0);
        assert_eq!(world.get::<Wallet>(self.worker).unwrap().balance(), 73);
        assert!(world.get::<WorkerOffDuty>(self.worker).is_none());
        assert!(self.trade.still_returning(world, self.worker));
        assert_eq!(
            world.get::<MoveTarget>(self.worker).unwrap().0,
            self.entrance
        );
        assert_eq!(
            world
                .get::<BusinessStaffingForecast>(self.workplace)
                .unwrap()
                .produced_output_units,
            0
        );
    }
}

#[test]
fn failed_loaded_deliveries_wait_then_walk_to_the_actual_store_at_both_warps() {
    for warp in [1.0, 25.0] {
        for trade in [Trade::Farmer, Trade::Fisher, Trade::Lumberjack] {
            let mut f = Fixture::new(trade, warp);
            for _ in 0..600 {
                f.tick();
                f.assert_retained();
                if f.app
                    .world()
                    .get::<NavigationRouteFailed>(f.worker)
                    .is_some()
                {
                    break;
                }
            }
            assert!(
                f.app
                    .world()
                    .get::<NavigationRouteFailed>(f.worker)
                    .is_some(),
                "{trade:?}, {warp}× did not exercise real route failure"
            );
            assert!(
                f.app
                    .world()
                    .get::<NavigationRouteBackoff>(f.worker)
                    .is_some()
            );
            // Producer updates cannot consume the failure and reward the load,
            // or bypass the navigator's real-time backoff at a higher warp.
            for _ in 0..12 {
                f.tick();
                f.assert_retained();
                assert!(
                    f.app
                        .world()
                        .get::<NavigationRouteFailed>(f.worker)
                        .is_some()
                );
                assert!(
                    f.app
                        .world()
                        .get::<NavigationRoutePending>(f.worker)
                        .is_none()
                );
            }
            // Remove only the obstruction. No actor, cargo, route or retry
            // deadline is edited; the existing planner must recover and walk.
            f.app
                .world_mut()
                .resource_mut::<SpatialObstacleGrid>()
                .clear();
            let mut saw_walk = false;
            for _ in 0..7_200 {
                f.tick();
                let position = f.app.world().get::<PlayerPosition>(f.worker).unwrap().0;
                saw_walk |= position.distance(f.start) > 0.1;
                assert_eq!(f.amount(f.worker) + f.amount(f.workplace), 5);
                if f.amount(f.workplace) > 0 {
                    assert!(ground_distance(position, f.entrance) <= DOOR_REACH);
                    break;
                }
            }
            assert!(saw_walk, "{trade:?}, {warp}× never resumed physical travel");
            assert_eq!(
                f.amount(f.worker),
                0,
                "{trade:?}, {warp}× failed to deliver"
            );
            assert_eq!(f.amount(f.workplace), 5);
            assert!(f.app.world().get::<WorkerOffDuty>(f.worker).is_some());
            for _ in 0..30 {
                f.tick();
            }
            assert_eq!(f.amount(f.workplace), 5, "delivery must occur exactly once");
            assert_eq!(f.app.world().get::<Wallet>(f.worker).unwrap().balance(), 73);
            assert_eq!(
                f.app
                    .world()
                    .get::<BusinessStaffingForecast>(f.workplace)
                    .unwrap()
                    .produced_output_units,
                0
            );
        }
    }
}
