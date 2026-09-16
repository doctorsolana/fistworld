//! Full workplace backpressure exercises real production and titled freight.

use super::*;
use bevy::ecs::system::RunSystemOnce;
use shared::components::{
    BuildingId, BuildingOf, CompanyId, EmployedAt, OperatedBy, PersonId, RoadClass, RoadOf,
    RoadSurface, SettlementId, TimeWarp,
};
use shared::economy::{CompanyAccount, CompanyBranchPolicies, CompanyResourcePolicy, MarketSeller};

struct Fixture {
    app: App,
    worker: Entity,
    business: Entity,
    hall: Entity,
    company: Entity,
    entrance: Vec3,
    counter: Vec3,
}

impl Fixture {
    fn produce(warp: f32) -> Self {
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<BusinessEventQueue>()
            .init_resource::<SettlementEconomyRuntime>();
        app.world_mut()
            .spawn((WorldTime::new_default(), TimeWarp(warp)));
        let company = app
            .world_mut()
            .spawn((
                CompanyId(3),
                CompanyAccount {
                    cash: 500,
                    ..default()
                },
            ))
            .id();
        let hall_at = Vec3::ZERO;
        let at = Vec3::new(30.0, 0.0, 0.0);
        let kind = SettlementBuildingKind::LivestockFarm;
        let entrance = kind.entrance_position(at, 0.0);
        let counter = SettlementBuildingKind::Hall.entrance_position(hall_at, 0.0);
        let hall = app
            .world_mut()
            .spawn((
                SettlementId(1),
                PlayerPosition(hall_at),
                PlayerRotation(0.0),
                GoodsInventory::new(shared::economy::capacity::HALL),
                MootMarket::founding(),
            ))
            .id();
        let business = app
            .world_mut()
            .spawn((
                BuildingId(2),
                BuildingOf(SettlementId(1)),
                OperatedBy(CompanyId(3)),
                SettlementBuilding {
                    kind,
                    settlement: "Fullstore".into(),
                    owner: None,
                    quality: 1.0,
                    workers: vec![],
                },
                PlayerPosition(at),
                PlayerRotation(0.0),
                GoodsInventory::new(kind.storage_bulk_capacity()),
                BusinessSalePolicy {
                    collection_enabled: true,
                    max_units_per_collection: 8,
                    ..BusinessSalePolicy::for_good(Good::Meat)
                },
                BusinessAccount::default(),
                BusinessWagePolicy::default(),
                BusinessCondition::default(),
                BusinessStaffingPolicy::new(1),
            ))
            .id();
        app.world_mut().spawn((
            VillageRoad {
                settlement: "Fullstore".into(),
                builder: "Crew".into(),
                points: vec![entrance.xz(), counter.xz()],
                built_through: 2,
                width: crate::world::village_roads::VILLAGE_ROAD_WIDTH,
                reserved_width: RoadClass::Lane.initial_reserved_width(),
                surface: RoadSurface::Dirt,
                class: RoadClass::Lane,
                stone_committed: 0,
            },
            RoadOf(SettlementId(1)),
        ));
        let worker = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PersonId(4),
                CharacterName("Herder".into()),
                VillagerIntent::Resident { settlement: hall },
                EmployedAt(BuildingId(2)),
                Occupation::default(),
                WorkStatus::Employed,
                PlayerPosition(entrance),
                PlayerRotation(0.0),
                CharacterActivity::Idle,
                GoodsInventory::new(shared::economy::capacity::VILLAGER),
            ))
            .id();
        app.world_mut()
            .run_system_once(super::super::quarry::assign_quarry_routines)
            .unwrap();
        let face = app
            .world()
            .get::<MoveTarget>(worker)
            .expect("real assignment selects pasture")
            .0;
        app.world_mut().get_mut::<PlayerPosition>(worker).unwrap().0 = face;
        app.world_mut()
            .run_system_once(super::super::quarry::run_quarry_routines)
            .unwrap();
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(
                2.0 * super::super::production::livestock_seconds_per_meat(1.0) / warp,
            ));
        app.world_mut()
            .run_system_once(super::super::quarry::run_quarry_routines)
            .unwrap();
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::ZERO);
        let carrier = app.world().get::<GoodsInventory>(worker).unwrap();
        assert_eq!(carrier.amount(Good::Meat), 2);
        assert_eq!(carrier.amount(Good::Wool), 2);
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(business)
                .unwrap()
                .used_bulk(),
            0
        );
        Self {
            app,
            worker,
            business,
            hall,
            company,
            entrance,
            counter,
        }
    }

    fn fill_store(&mut self) {
        let mut store = self
            .app
            .world_mut()
            .get_mut::<GoodsInventory>(self.business)
            .unwrap();
        let units = store.free_units(Good::Wheat);
        assert_eq!(store.add(Good::Wheat, units), units);
        assert_eq!(store.free_bulk(), 0);
    }

    fn arrive_at_store(&mut self) {
        self.app
            .world_mut()
            .get_mut::<PlayerPosition>(self.worker)
            .unwrap()
            .0 = self.entrance;
        self.app
            .world_mut()
            .run_system_once(super::super::quarry::run_quarry_routines)
            .unwrap();
    }

    fn dispatch(&mut self) {
        self.app
            .world_mut()
            .run_system_once(run_market_collections)
            .unwrap();
    }

    fn deliver(&mut self) {
        self.app
            .world_mut()
            .get_mut::<PlayerPosition>(self.worker)
            .unwrap()
            .0 = self.counter;
        self.dispatch();
    }

    fn release(&mut self, state: BusinessState) {
        self.app
            .world_mut()
            .get_mut::<BusinessCondition>(self.business)
            .unwrap()
            .state = state;
        self.app
            .world_mut()
            .entity_mut(self.business)
            .insert(BusinessStaffingPolicy::new(0));
        self.app
            .world_mut()
            .run_system_once(super::super::employment::enforce_staffing_targets)
            .unwrap();
        assert!(
            self.app
                .world()
                .get::<super::super::worker_activity::EmploymentReleaseRequested>(self.worker)
                .is_some()
        );
    }
}

#[test]
fn full_store_self_haul_keeps_both_real_outputs_and_finishes_requested_release() {
    for warp in [1.0, 25.0] {
        for state in [
            BusinessState::Operating,
            BusinessState::Mothballed,
            BusinessState::Liquidating,
        ] {
            let mut f = Fixture::produce(warp);
            f.fill_store();
            let initial_store = f
                .app
                .world()
                .get::<GoodsInventory>(f.business)
                .unwrap()
                .clone();
            let released = state != BusinessState::Operating;
            if released {
                f.release(state);
            }
            f.arrive_at_store();
            for good in [Good::Meat, Good::Wool] {
                assert!(f.app.world().get::<MoveTarget>(f.worker).is_none());
                f.dispatch();
                let trip = f
                    .app
                    .world()
                    .get::<MarketCollectionRoutine>(f.worker)
                    .expect("full-store producer delivers its existing output");
                assert_eq!(trip.good, good);
                assert_eq!(trip.reserved_units, 2);
                assert_eq!(trip.phase, MarketCollectionPhase::ReturningToHall);
                assert_eq!(trip.seller, BuildingId(2));
                assert!(
                    f.app.world().get::<QuarryRoutine>(f.worker).is_some(),
                    "producer still owns remaining cargo and progress"
                );
                assert_eq!(
                    f.app
                        .world()
                        .get::<GoodsInventory>(f.worker)
                        .unwrap()
                        .amount(good),
                    2
                );
                f.deliver();
                assert!(
                    f.app
                        .world()
                        .get::<MarketCollectionRoutine>(f.worker)
                        .is_none()
                );
                assert_eq!(
                    f.app
                        .world()
                        .get::<MootMarket>(f.hall)
                        .unwrap()
                        .seller_listed_units(MarketSeller::Business(BuildingId(2)), good),
                    2
                );
                assert_eq!(
                    f.app
                        .world()
                        .get::<MootMarket>(f.hall)
                        .unwrap()
                        .seller_listed_units(MarketSeller::Treasury(SettlementId(1)), good),
                    0
                );
                f.arrive_at_store();
            }
            assert!(
                f.app
                    .world()
                    .get::<GoodsInventory>(f.worker)
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(
                f.app.world().get::<GoodsInventory>(f.business).unwrap(),
                &initial_store,
                "no stock is pulled from the full workplace or discarded"
            );
            assert_eq!(
                f.app.world().get::<CompanyAccount>(f.company).unwrap().cash,
                500,
                "consignment is not a sale"
            );
            if released {
                f.app
                    .world_mut()
                    .run_system_once(super::super::quarry::run_quarry_routines)
                    .unwrap();
                f.app
                    .world_mut()
                    .run_system_once(super::super::employment::enforce_staffing_targets)
                    .unwrap();
                assert!(f.app.world().get::<QuarryRoutine>(f.worker).is_none());
                assert!(f.app.world().get::<EmployedAt>(f.worker).is_none());
                assert!(
                    f.app
                        .world()
                        .get::<super::super::worker_activity::EmploymentReleaseRequested>(f.worker)
                        .is_none()
                );
            }
        }
    }
}

#[test]
fn ordinary_deposit_and_specialist_logistics_take_precedence_over_overflow_self_haul() {
    let mut f = Fixture::produce(1.0);
    f.arrive_at_store();
    assert!(
        f.app
            .world()
            .get::<GoodsInventory>(f.worker)
            .unwrap()
            .is_empty()
    );
    for good in [Good::Meat, Good::Wool] {
        assert_eq!(
            f.app
                .world()
                .get::<GoodsInventory>(f.business)
                .unwrap()
                .amount(good),
            2
        );
    }
    let mut f = Fixture::produce(1.0);
    f.fill_store();
    f.arrive_at_store();
    f.app.world_mut().spawn((
        CharacterKind::Villager,
        MootSteward { settlement: f.hall },
        PlayerPosition(f.counter),
        CharacterActivity::Idle,
        GoodsInventory::new(24),
    ));
    f.dispatch();
    assert!(
        f.app
            .world()
            .get::<MarketCollectionRoutine>(f.worker)
            .is_none()
    );
    assert_eq!(
        f.app
            .world()
            .get::<GoodsInventory>(f.worker)
            .unwrap()
            .amount(Good::Meat),
        2
    );
}

#[test]
fn output_overflow_retains_title_when_the_market_fills_mid_trip() {
    let mut f = Fixture::produce(1.0);
    f.fill_store();
    f.release(BusinessState::Mothballed);
    f.arrive_at_store();
    f.dispatch();
    f.app
        .world_mut()
        .get_mut::<GoodsInventory>(f.hall)
        .unwrap()
        .resize_bulk_capacity(Good::Meat.bulk_per_unit());
    f.deliver();
    assert_eq!(
        f.app
            .world()
            .get::<MarketCollectionRoutine>(f.worker)
            .unwrap()
            .reserved_units,
        1
    );
    assert_eq!(
        f.app
            .world()
            .get::<GoodsInventory>(f.worker)
            .unwrap()
            .amount(Good::Meat),
        1
    );
    assert_eq!(
        f.app
            .world()
            .get::<GoodsInventory>(f.worker)
            .unwrap()
            .amount(Good::Wool),
        2
    );
    f.dispatch();
    assert_eq!(
        f.app
            .world()
            .get::<MarketCollectionRoutine>(f.worker)
            .unwrap()
            .reserved_units,
        1
    );
    f.app
        .world_mut()
        .get_mut::<GoodsInventory>(f.hall)
        .unwrap()
        .resize_bulk_capacity(24);
    f.dispatch();
    assert!(
        f.app
            .world()
            .get::<MarketCollectionRoutine>(f.worker)
            .is_none()
    );
    assert_eq!(
        f.app
            .world()
            .get::<MootMarket>(f.hall)
            .unwrap()
            .seller_listed_units(MarketSeller::Business(BuildingId(2)), Good::Meat),
        2
    );
    assert_eq!(
        f.app
            .world()
            .get::<GoodsInventory>(f.worker)
            .unwrap()
            .amount(Good::Wool),
        2
    );
}

#[test]
fn final_output_haul_respects_private_reserves_and_unsafe_workplace_ownership() {
    let mut f = Fixture::produce(1.0);
    f.fill_store();
    f.release(BusinessState::Mothballed);
    f.arrive_at_store();
    let mut policies = CompanyBranchPolicies::default();
    for good in [Good::Meat, Good::Wool] {
        policies.set_resource(
            SettlementId(1),
            good,
            CompanyResourcePolicy {
                retain_units: 0,
                sell_excess: false,
            },
        );
    }
    f.app.world_mut().entity_mut(f.company).insert(policies);
    f.dispatch();
    assert!(
        f.app
            .world()
            .get::<MarketCollectionRoutine>(f.worker)
            .is_none()
    );
    f.app
        .world_mut()
        .get_mut::<CompanyBranchPolicies>(f.company)
        .unwrap()
        .set_resource(
            SettlementId(1),
            Good::Meat,
            CompanyResourcePolicy {
                retain_units: 1,
                sell_excess: true,
            },
        );
    f.app
        .world_mut()
        .entity_mut(f.worker)
        .insert(WorkplaceInterior {
            building: Vec3::new(30.0, 0.0, 0.0),
            door: f.entrance,
            inside: f.entrance + Vec3::Z,
        });
    f.dispatch();
    assert!(
        f.app
            .world()
            .get::<MarketCollectionRoutine>(f.worker)
            .is_none()
    );
    f.app
        .world_mut()
        .entity_mut(f.worker)
        .remove::<WorkplaceInterior>();
    f.dispatch();
    assert_eq!(
        f.app
            .world()
            .get::<MarketCollectionRoutine>(f.worker)
            .unwrap()
            .reserved_units,
        1
    );
    f.deliver();
    assert_eq!(
        f.app
            .world()
            .get::<GoodsInventory>(f.worker)
            .unwrap()
            .amount(Good::Meat),
        1
    );
    assert_eq!(
        f.app
            .world()
            .get::<GoodsInventory>(f.worker)
            .unwrap()
            .amount(Good::Wool),
        2
    );
    f.arrive_at_store();
    f.dispatch();
    assert!(
        f.app
            .world()
            .get::<MarketCollectionRoutine>(f.worker)
            .is_none(),
        "reserved company stock cannot be sold to unblock a release"
    );
}

#[test]
fn purchased_input_delivery_completes_after_multiple_partial_unloads() {
    let mut f = Fixture::produce(1.0);
    f.app
        .world_mut()
        .get_mut::<GoodsInventory>(f.business)
        .unwrap()
        .resize_bulk_capacity(2 * Good::Wheat.bulk_per_unit());
    let mut carrier = GoodsInventory::new(24);
    assert_eq!(carrier.add(Good::Wheat, 5), 5);
    let porter = f
        .app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            PlayerPosition(f.entrance),
            CharacterActivity::Idle,
            carrier,
            MarketCollectionRoutine {
                business: f.business,
                seller: BuildingId(2),
                hall: f.hall,
                counter: f.counter,
                good: Good::Wheat,
                reserved_units: 5,
                unit_price: 0,
                phase: MarketCollectionPhase::DeliveringInput,
                fallback_counter_attempted: false,
            },
        ))
        .id();
    f.dispatch();
    assert_eq!(
        f.app
            .world()
            .get::<GoodsInventory>(f.business)
            .unwrap()
            .amount(Good::Wheat),
        2
    );
    assert_eq!(
        f.app
            .world()
            .get::<GoodsInventory>(porter)
            .unwrap()
            .amount(Good::Wheat),
        3
    );
    assert_eq!(
        f.app
            .world()
            .get::<MarketCollectionRoutine>(porter)
            .unwrap()
            .reserved_units,
        3
    );
    f.dispatch();
    assert_eq!(
        f.app
            .world()
            .get::<MarketCollectionRoutine>(porter)
            .unwrap()
            .reserved_units,
        3
    );
    f.app
        .world_mut()
        .get_mut::<GoodsInventory>(f.business)
        .unwrap()
        .resize_bulk_capacity(5 * Good::Wheat.bulk_per_unit());
    f.dispatch();
    assert_eq!(
        f.app
            .world()
            .get::<GoodsInventory>(f.business)
            .unwrap()
            .amount(Good::Wheat),
        5
    );
    assert!(
        f.app
            .world()
            .get::<GoodsInventory>(porter)
            .unwrap()
            .is_empty()
    );
    assert!(
        f.app
            .world()
            .get::<MarketCollectionRoutine>(porter)
            .is_none()
    );
    assert_eq!(
        f.app
            .world()
            .get::<MootMarket>(f.hall)
            .unwrap()
            .listed_units(Good::Wheat),
        0
    );
    assert_eq!(
        f.app.world().get::<CompanyAccount>(f.company).unwrap().cash,
        500
    );
}
