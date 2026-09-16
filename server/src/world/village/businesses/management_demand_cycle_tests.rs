//! Controlled policy/conservation integration on the canonical physical quarry
//! and self-haul routines. A finite customer wallet submits real orders; the
//! worker walks, works, carries, deposits and consigns on a dry authored floor.
//! Payroll, household demand and taxes are outside this focused fixture; its
//! existing tax claim deliberately remains outstanding.

use super::*;
use crate::world::village::commerce::run_market_collections;
use crate::world::village::quarry::{assign_quarry_routines, run_quarry_routines};
use shared::components::{
    BuildingId, BuildingOf, CompanyId, EmployedAt, OperatedBy, PersonId, SettlementId,
};
use shared::economy::{BusinessStrategy, CompanyAccount};
use shared::region::RegionCoord;

struct DemandCycle {
    app: App,
    clock: Entity,
    hall: Entity,
    site: Entity,
    company: Entity,
    customer: Entity,
    worker: Entity,
    settlement: SettlementId,
    building: BuildingId,
}

impl DemandCycle {
    fn new(strategy: BusinessStrategy) -> Self {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.insert_resource(crate::world::village::tests::fixtures::dry_test_terrain());
        app.init_resource::<SettlementEconomyRuntime>();
        app.init_resource::<BusinessEventQueue>();
        app.add_systems(
            Update,
            (
                review_business_management,
                review_automatic_staffing,
                enforce_staffing_targets,
                fill_vacancies,
                assign_quarry_routines,
                run_quarry_routines,
                run_market_collections,
                crate::player::hero::step_units,
                apply_business_events,
            )
                .chain(),
        );
        let mut time = WorldTime::new_default();
        time.day = 4;
        let clock = app.world_mut().spawn(time).id();
        let settlement = SettlementId(981);
        let hall = app
            .world_mut()
            .spawn((
                settlement,
                Settlement {
                    name: "Cycleford".into(),
                    tier: shared::components::SettlementTier::Village,
                    residents: 1,
                    treasury: 0,
                },
                PlayerPosition(Vec3::ZERO),
                GoodsInventory::new(shared::economy::capacity::HALL),
                MootMarket::founding(),
            ))
            .id();
        let company_id = CompanyId(982);
        let company = app
            .world_mut()
            .spawn((
                company_id,
                CompanyAccount {
                    cash: 5_000,
                    tax_arrears: 37,
                    ..default()
                },
            ))
            .id();
        let building = BuildingId(983);
        let site = app
            .world_mut()
            .spawn((
                building,
                BuildingOf(settlement),
                OperatedBy(company_id),
                SettlementBuilding {
                    kind: SettlementBuildingKind::StoneQuarry,
                    settlement: "Cycleford".into(),
                    owner: None,
                    quality: 1.0,
                    workers: Vec::new(),
                },
                PlayerPosition(Vec3::new(30.0, 0.0, 0.0)),
                PlayerRotation(0.0),
                GoodsInventory::new(SettlementBuildingKind::StoneQuarry.storage_bulk_capacity()),
                BusinessAccount {
                    tax_arrears: 37,
                    ..default()
                },
                BusinessSalePolicy {
                    asking_unit_price: Good::Stone.base_price(),
                    ..BusinessSalePolicy::for_good(Good::Stone)
                },
                BusinessWagePolicy {
                    daily_wage: 100,
                    automatic: false,
                    ..default()
                },
                BusinessManagementPolicy {
                    rescue_with_personal_savings: false,
                    ..BusinessManagementPolicy::for_strategy(strategy)
                },
                BusinessCondition {
                    state: BusinessState::Operating,
                    opened_day: 0,
                    ..default()
                },
                BusinessStaffingPolicy::new(1),
            ))
            .id();
        let entrance =
            SettlementBuildingKind::StoneQuarry.entrance_position(Vec3::new(30.0, 0.0, 0.0), 0.0);
        let counter = SettlementBuildingKind::Hall.entrance_position(Vec3::ZERO, 0.0);
        app.world_mut().spawn((
            VillageRoad {
                settlement: "Cycleford".into(),
                builder: "Fixture".into(),
                points: vec![entrance.xz(), counter.xz()],
                built_through: 2,
                width: 2.6,
                reserved_width: 2.6,
                surface: shared::components::RoadSurface::Dirt,
                class: shared::components::RoadClass::Lane,
                stone_committed: 0,
            },
            shared::components::RoadOf(settlement),
        ));
        let worker = app
            .world_mut()
            .spawn((
                PersonId(984),
                CharacterName("Rowan".into()),
                CharacterKind::Villager,
                VillagerIntent::Resident { settlement: hall },
                PlayerPosition(Vec3::new(30.0, 0.0, -8.0)),
                PlayerRotation(0.0),
                RegionCoord::new(0, 0),
                CharacterActivity::Idle,
                Occupation(None),
                WorkStatus::LookingForWork,
                GoodsInventory::new(Good::Stone.bulk_per_unit()),
                Wallet::new(0),
            ))
            .id();
        let customer = app
            .world_mut()
            .spawn((PersonId(985), Wallet::new(5_000)))
            .id();
        Self {
            app,
            clock,
            hall,
            site,
            company,
            customer,
            worker,
            settlement,
            building,
        }
    }

    fn funded_order(&mut self, units: u32) {
        let cash = self
            .app
            .world()
            .get::<Wallet>(self.customer)
            .unwrap()
            .balance();
        // Failed purchase records an actual affordable bid, with no cash or
        // goods created. Successful fills are settled exactly like live buys.
        let purchase = self
            .app
            .world_mut()
            .get_mut::<MootMarket>(self.hall)
            .unwrap()
            .purchase_recording_demand(
                Good::Stone,
                units,
                cash.min(u64::from(units) * Good::Stone.base_price()),
                Some(Good::Stone.base_price()),
                None,
            );
        self.settle_purchase(purchase);
    }

    fn buy_listed_stock(&mut self) {
        let cash = self
            .app
            .world()
            .get::<Wallet>(self.customer)
            .unwrap()
            .balance();
        let units = self
            .app
            .world()
            .get::<MootMarket>(self.hall)
            .unwrap()
            .listed_units(Good::Stone);
        assert!(units > 0, "a customer must buy real produced stock");
        let purchase = self
            .app
            .world_mut()
            .get_mut::<MootMarket>(self.hall)
            .unwrap()
            .purchase(Good::Stone, units, cash, None, None);
        assert_eq!(purchase.trade.units, units);
        self.settle_purchase(purchase);
    }

    fn settle_purchase(&mut self, purchase: shared::economy::MarketPurchase) {
        assert!(
            self.app
                .world_mut()
                .get_mut::<Wallet>(self.customer)
                .unwrap()
                .debit(purchase.trade.pennies)
        );
        assert_eq!(
            self.app
                .world_mut()
                .get_mut::<GoodsInventory>(self.hall)
                .unwrap()
                .remove(Good::Stone, purchase.trade.units),
            purchase.trade.units
        );
        let day = self.app.world().get::<WorldTime>(self.clock).unwrap().day;
        self.app
            .world_mut()
            .resource_mut::<BusinessEventQueue>()
            .record_market_purchase(day, self.settlement, purchase.fills);
    }

    fn work(&mut self, seconds: f64) {
        let dt = 0.25;
        for _ in 0..(seconds / dt).round() as usize {
            self.app
                .world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f64(dt));
            self.app
                .world_mut()
                .get_mut::<WorldTime>(self.clock)
                .unwrap()
                .advance(dt as f32, 0.0);
            self.app.update();
        }
    }

    fn next_review(&mut self) {
        {
            let mut clock = self
                .app
                .world_mut()
                .get_mut::<WorldTime>(self.clock)
                .unwrap();
            clock.day += 1;
            clock.seconds_in_cycle = WorldTime::new_default().seconds_in_cycle;
        }
        self.app
            .world_mut()
            .get_mut::<MootMarket>(self.hall)
            .unwrap()
            .begin_new_day();
        // A calendar review has no elapsed work or travel time.
        self.app
            .world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::ZERO);
        self.app.update();
    }

    fn unsold_stock(&self) -> u32 {
        self.app
            .world()
            .get::<GoodsInventory>(self.site)
            .unwrap()
            .amount(Good::Stone)
            + self
                .app
                .world()
                .get::<GoodsInventory>(self.hall)
                .unwrap()
                .amount(Good::Stone)
    }

    fn assert_accounts(&self) {
        let world = self.app.world();
        assert_eq!(
            world.get::<CompanyAccount>(self.company).unwrap().cash
                + world.get::<Wallet>(self.customer).unwrap().balance()
                + world.get::<Wallet>(self.worker).unwrap().balance()
                + world.get::<Settlement>(self.hall).unwrap().treasury,
            10_000
        );
        assert_eq!(
            world.get::<BusinessAccount>(self.site).unwrap().tax_arrears,
            37,
            "pause/restart does not silently discharge the retained tax claim"
        );
        assert_eq!(
            world
                .get::<BusinessAccount>(self.site)
                .unwrap()
                .wage_arrears,
            0
        );
    }
}

#[test]
fn all_three_managers_contract_then_reopen_the_same_plant_on_returning_funded_orders() {
    for strategy in BusinessStrategy::ALL {
        let mut run = DemandCycle::new(strategy);
        run.funded_order(24);
        run.app.update();
        assert_eq!(
            run.app.world().get::<EmployedAt>(run.worker),
            Some(&EmployedAt(run.building)),
            "{strategy:?}"
        );
        run.work(180.0);
        run.work(180.0);
        run.buy_listed_stock();
        run.app.update();
        let account = run.app.world().get::<BusinessAccount>(run.site).unwrap();
        assert!(account.current_day.produced_units > 0);
        assert!(account.current_day.sold_units > 0);
        assert!(account.gross_revenue > 0);
        run.assert_accounts();

        // The customer now stops ordering and buying. Allow only bounded
        // daily changes, with actual work still offered to employed workers.
        let mut mothballed = false;
        for _ in 0..6 {
            let previous = run
                .app
                .world()
                .get::<BusinessStaffingPolicy>(run.site)
                .unwrap()
                .enabled_positions;
            run.next_review();
            let current = run
                .app
                .world()
                .get::<BusinessStaffingPolicy>(run.site)
                .unwrap()
                .enabled_positions;
            assert!(
                previous.abs_diff(current) <= 1,
                "{strategy:?}: abrupt staffing change"
            );
            run.work(180.0);
            if run
                .app
                .world()
                .get::<BusinessCondition>(run.site)
                .unwrap()
                .state
                == BusinessState::Mothballed
            {
                mothballed = true;
                break;
            }
        }
        assert!(
            mothballed,
            "{strategy:?}: empty order book never caused a pause"
        );
        assert!(run.app.world().get::<EmployedAt>(run.worker).is_none());
        assert_eq!(
            run.app
                .world()
                .get::<BusinessStaffingPolicy>(run.site)
                .unwrap()
                .enabled_positions,
            0
        );
        let held = run.unsold_stock();
        assert!(held > 0);
        let produced_before = run
            .app
            .world()
            .get::<BusinessAccount>(run.site)
            .unwrap()
            .current_day
            .produced_units;
        run.work(180.0);
        assert_eq!(
            run.unsold_stock(),
            held,
            "mothball preserves the existing stock"
        );
        assert_eq!(
            run.app
                .world()
                .get::<BusinessAccount>(run.site)
                .unwrap()
                .current_day
                .produced_units,
            produced_before
        );
        run.assert_accounts();

        // Real affordable demand comes back. The customer first pays for
        // offered old stock; the remaining funded order exceeds the plant's
        // finite unsold inventory. No lifecycle state or job is assigned here.
        run.funded_order(held + 24);
        run.app.update();
        run.next_review();
        assert!(
            run.app
                .world()
                .get::<BusinessCondition>(run.site)
                .unwrap()
                .state
                .can_operate(),
            "{strategy:?}"
        );
        assert!(
            run.app
                .world()
                .get::<BusinessStaffingPolicy>(run.site)
                .unwrap()
                .enabled_positions
                > 0
        );
        assert_eq!(
            run.app.world().get::<EmployedAt>(run.worker),
            Some(&EmployedAt(run.building))
        );
        run.work(180.0);
        assert!(
            run.app
                .world()
                .get::<BusinessAccount>(run.site)
                .unwrap()
                .current_day
                .produced_units
                > produced_before
        );
        assert!(run.app.world().get::<BusinessForSale>(run.site).is_none());
        assert_eq!(
            run.app
                .world_mut()
                .query::<&SettlementBuilding>()
                .iter(run.app.world())
                .count(),
            1
        );
        assert_eq!(
            run.app
                .world_mut()
                .query::<&UnderConstruction>()
                .iter(run.app.world())
                .count(),
            0
        );
        run.assert_accounts();
    }
}
