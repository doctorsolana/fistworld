//! Household accounts, repeat provisioning and physical shopping integration.

use super::*;
use shared::components::{
    BuildingId, BuildingOf, HouseholdId, HouseholdMember, HouseholdMembers, LivesAt,
    OccupiedByHousehold, PersonId, ResidentOf, SettlementId,
};

const SETTLEMENT: SettlementId = SettlementId(920);
const HOUSEHOLD: HouseholdId = HouseholdId(921);
const HOME: BuildingId = BuildingId(922);

struct Fixture {
    app: App,
    clock: Entity,
    hall: Entity,
    home: Entity,
    account: Entity,
    people: Vec<(PersonId, Entity)>,
}

impl Fixture {
    fn new(wallets: &[(PersonId, u64)], tactical: bool, fuel_days: u8) -> Self {
        let mut app = village_test_app();
        app.add_systems(
            Update,
            (
                update_household_budgets_and_pantries,
                run_household_shopping,
                apply_business_events,
            )
                .chain(),
        );
        let region = RegionCoord::new(0, 0);
        if tactical {
            app.init_resource::<RegionRegistry>();
            app.world_mut()
                .resource_mut::<RegionRegistry>()
                .set_level_for_test(region, SimLevel::Tactical);
        }
        let clock = app.world_mut().spawn(WorldTime::new_default()).id();
        let hall = app
            .world_mut()
            .spawn((
                SETTLEMENT,
                Settlement {
                    name: "Provisionford".into(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: wallets.len() as u32,
                    treasury: 0,
                },
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
                GoodsInventory::new(shared::economy::capacity::HALL),
                MootMarket::founding(),
            ))
            .id();
        let resident_ids: Vec<_> = wallets.iter().map(|(id, _)| *id).collect();
        let account = app
            .world_mut()
            .spawn((
                HOUSEHOLD,
                HouseholdMembers {
                    resident_ids: resident_ids.clone(),
                    settlement: SETTLEMENT,
                    dwelling: Some(HOME),
                },
                HouseholdEconomy {
                    fuel_target_days: fuel_days,
                    ..default()
                },
            ))
            .id();
        let home_position = Vec3::new(20.0, 0.0, 0.0);
        let home = app
            .world_mut()
            .spawn((
                HOME,
                BuildingOf(SETTLEMENT),
                SettlementBuilding {
                    kind: SettlementBuildingKind::House,
                    settlement: "Provisionford".into(),
                    owner: None,
                    quality: 1.0,
                    workers: Vec::new(),
                },
                OccupiedByHousehold(HOUSEHOLD),
                Household {
                    resident_ids,
                    residents: wallets
                        .iter()
                        .map(|(id, _)| format!("Resident {}", id.0))
                        .collect(),
                },
                PlayerPosition(home_position),
                PlayerRotation(0.0),
                GoodsInventory::new(shared::economy::capacity::HOUSE),
            ))
            .id();
        let people = wallets
            .iter()
            .map(|(id, cash)| {
                let person = app
                    .world_mut()
                    .spawn((
                        *id,
                        CharacterKind::Villager,
                        CharacterName(format!("Resident {}", id.0)),
                        ResidentOf(SETTLEMENT),
                        VillagerIntent::Resident { settlement: hall },
                        HouseholdMember(HOUSEHOLD),
                        LivesAt(HOME),
                        HomeAssignment { home },
                        Wallet::new(*cash),
                        WorkStatus::LookingForWork,
                        PlayerPosition(home_position),
                        region,
                        CharacterActivity::Idle,
                        GoodsInventory::new(shared::economy::capacity::VILLAGER),
                    ))
                    .id();
                (*id, person)
            })
            .collect();
        Self {
            app,
            clock,
            hall,
            home,
            account,
            people,
        }
    }

    fn list(&mut self, good: Good, units: u32, price: u64) {
        assert_eq!(
            self.app
                .world_mut()
                .get_mut::<GoodsInventory>(self.hall)
                .unwrap()
                .add(good, units),
            units
        );
        self.app
            .world_mut()
            .get_mut::<MootMarket>(self.hall)
            .unwrap()
            .consign(MarketSeller::Treasury(SETTLEMENT), good, units, price);
    }

    fn at_minute(&mut self, minute: u32) {
        let mut clock = self
            .app
            .world_mut()
            .get_mut::<WorldTime>(self.clock)
            .unwrap();
        clock.seconds_in_cycle = minute as f32 * clock.cycle_duration() / 1440.0;
        drop(clock);
        self.app.update();
    }

    fn stock(&self, entity: Entity, good: Good) -> u32 {
        self.app
            .world()
            .get::<GoodsInventory>(entity)
            .unwrap()
            .amount(good)
    }

    fn money(&self) -> u64 {
        self.people
            .iter()
            .map(|(_, entity)| self.app.world().get::<Wallet>(*entity).unwrap().balance())
            .sum::<u64>()
            + self
                .app
                .world()
                .get::<HouseholdEconomy>(self.account)
                .unwrap()
                .pennies
            + self
                .app
                .world()
                .get::<Settlement>(self.hall)
                .unwrap()
                .treasury
    }

    fn shoppers(&self) -> Vec<Entity> {
        self.people
            .iter()
            .filter_map(|(_, entity)| {
                self.app
                    .world()
                    .get::<HouseholdShoppingRoutine>(*entity)
                    .map(|_| *entity)
            })
            .collect()
    }
}

#[test]
fn same_day_restock_reaches_the_pantry_without_repeating_the_shortage_claim() {
    let mut fixture = Fixture::new(&[(PersonId(1), 1_000)], false, 0);
    fixture.at_minute(0);
    let claim = fixture
        .app
        .world()
        .get::<MootMarket>(fixture.hall)
        .unwrap()
        .pool(Good::Bread)
        .day;
    assert_eq!(claim.unavailable_units, 3);
    assert_eq!(
        fixture
            .app
            .world()
            .get::<HouseholdEconomy>(fixture.account)
            .unwrap()
            .pennies,
        0
    );
    for minute in [20, 40, 60] {
        fixture.at_minute(minute);
        let current = fixture
            .app
            .world()
            .get::<MootMarket>(fixture.hall)
            .unwrap()
            .pool(Good::Bread)
            .day;
        assert_eq!(current.unavailable_units, claim.unavailable_units);
        assert_eq!(current.funded_unmet_units, claim.funded_unmet_units);
    }
    fixture.list(Good::Bread, 3, 10);
    fixture.at_minute(80);

    assert_eq!(
        fixture
            .app
            .world()
            .get::<WorldTime>(fixture.clock)
            .unwrap()
            .day,
        0
    );
    assert_eq!(fixture.stock(fixture.home, Good::Bread), 3);
    assert_eq!(fixture.stock(fixture.hall, Good::Bread), 0);
    assert_eq!(fixture.money(), 1_000);
    assert_eq!(
        fixture
            .app
            .world()
            .get::<MootMarket>(fixture.hall)
            .unwrap()
            .pool(Good::Bread)
            .day
            .consumer_units,
        3
    );
}

#[test]
fn a_household_buys_food_before_cheap_fuel_when_it_cannot_afford_both() {
    let mut fixture = Fixture::new(&[(PersonId(1), 50)], false, 4);
    fixture.list(Good::Bread, 3, 20);
    fixture.list(Good::Wood, 2, 5);
    fixture.at_minute(0);

    assert_eq!(fixture.stock(fixture.home, Good::Bread), 2);
    assert_eq!(fixture.stock(fixture.home, Good::Wood), 0);
    assert_eq!(fixture.stock(fixture.hall, Good::Bread), 1);
    assert_eq!(fixture.stock(fixture.hall, Good::Wood), 2);
    assert_eq!(fixture.money(), 50);
    assert_eq!(
        fixture
            .app
            .world()
            .get::<Wallet>(fixture.people[0].1)
            .unwrap()
            .balance(),
        10
    );
}

#[test]
fn pantry_capacity_blocks_claims_and_cash_until_storage_is_available() {
    let mut fixture = Fixture::new(&[(PersonId(1), 1_000)], false, 0);
    let mut pantry = GoodsInventory::new(3 * Good::Stone.bulk_per_unit());
    assert_eq!(pantry.add(Good::Stone, 3), 3);
    fixture
        .app
        .world_mut()
        .entity_mut(fixture.home)
        .insert(pantry);
    fixture.list(Good::Bread, 3, 10);

    for minute in [0, 20] {
        fixture.at_minute(minute);
        let market = fixture.app.world().get::<MootMarket>(fixture.hall).unwrap();
        for good in Good::HOUSEHOLD_FOOD_PRIORITY {
            let flow = market.pool(good).day;
            assert_eq!(flow.funded_unmet_units, 0);
            assert_eq!(flow.unavailable_units, 0);
            assert_eq!(flow.unaffordable_units, 0);
            assert_eq!(flow.consumer_units, 0);
        }
        assert_eq!(fixture.stock(fixture.hall, Good::Bread), 3);
        assert_eq!(fixture.stock(fixture.home, Good::Bread), 0);
        assert_eq!(
            fixture
                .app
                .world()
                .get::<Wallet>(fixture.people[0].1)
                .unwrap()
                .balance(),
            1_000
        );
        assert_eq!(
            fixture
                .app
                .world()
                .get::<HouseholdEconomy>(fixture.account)
                .unwrap()
                .pennies,
            0
        );
    }

    let removed = fixture
        .app
        .world_mut()
        .get_mut::<GoodsInventory>(fixture.home)
        .unwrap()
        .remove(Good::Stone, 3);
    let mut yard_store = GoodsInventory::new(3 * Good::Stone.bulk_per_unit());
    assert_eq!(yard_store.add(Good::Stone, removed), 3);
    fixture.app.world_mut().spawn(yard_store);
    fixture.at_minute(100);

    assert_eq!(fixture.stock(fixture.home, Good::Bread), 3);
    assert_eq!(fixture.stock(fixture.hall, Good::Bread), 0);
    assert_eq!(fixture.money(), 1_000);
    assert_eq!(
        fixture
            .app
            .world()
            .get::<MootMarket>(fixture.hall)
            .unwrap()
            .pool(Good::Bread)
            .day
            .consumer_units,
        3
    );
}

#[test]
fn real_pantry_purchases_share_contributions_proportionally_and_conserve_coin() {
    let mut outcomes = Vec::new();
    for wallets in [
        vec![(PersonId(1), 100), (PersonId(2), 300)],
        vec![(PersonId(2), 300), (PersonId(1), 100)],
    ] {
        let mut fixture = Fixture::new(&wallets, false, 0);
        fixture.list(Good::Bread, 6, 10);
        fixture.at_minute(0);
        let mut balances: Vec<_> = fixture
            .people
            .iter()
            .map(|(id, entity)| {
                (
                    *id,
                    fixture
                        .app
                        .world()
                        .get::<Wallet>(*entity)
                        .unwrap()
                        .balance(),
                )
            })
            .collect();
        balances.sort_by_key(|(id, _)| *id);
        assert_eq!(balances, vec![(PersonId(1), 85), (PersonId(2), 255)]);
        assert_eq!(fixture.stock(fixture.home, Good::Bread), 6);
        assert_eq!(fixture.stock(fixture.hall, Good::Bread), 0);
        assert_eq!(
            fixture
                .app
                .world()
                .get::<Settlement>(fixture.hall)
                .unwrap()
                .treasury,
            60
        );
        assert_eq!(fixture.money(), 400);
        fixture.at_minute(180);
        assert_eq!(fixture.money(), 400);
        assert_eq!(fixture.stock(fixture.home, Good::Bread), 6);
        outcomes.push(balances);
    }
    assert_eq!(outcomes[0], outcomes[1]);
}

#[test]
fn observed_household_keeps_one_trip_and_unloads_only_its_purchased_cargo() {
    let mut fixture = Fixture::new(&[(PersonId(1), 1_000), (PersonId(2), 1_000)], true, 0);
    fixture.list(Good::Bread, 6, 10);
    for (_, person) in &fixture.people {
        let mut inventory = fixture
            .app
            .world_mut()
            .get_mut::<GoodsInventory>(*person)
            .unwrap();
        inventory.add(Good::Bread, 2);
        inventory.add(Good::Wood, 1);
    }
    fixture.at_minute(0);
    let shoppers = fixture.shoppers();
    assert_eq!(shoppers.len(), 1);
    let shopper = shoppers[0];
    let starting_purse = fixture
        .app
        .world()
        .get::<HouseholdEconomy>(fixture.account)
        .unwrap()
        .pennies;
    assert_eq!(starting_purse, 60);
    for minute in [30, 60, 120] {
        fixture.at_minute(minute);
        assert_eq!(fixture.shoppers(), vec![shopper]);
        assert_eq!(
            fixture
                .app
                .world()
                .get::<HouseholdEconomy>(fixture.account)
                .unwrap()
                .pennies,
            starting_purse
        );
        assert_eq!(fixture.stock(fixture.hall, Good::Bread), 6);
    }
    let counter = fixture
        .app
        .world()
        .get::<HouseholdShoppingRoutine>(shopper)
        .unwrap()
        .counter;
    fixture
        .app
        .world_mut()
        .get_mut::<PlayerPosition>(shopper)
        .unwrap()
        .0 = counter;
    fixture.app.update();
    let routine = fixture
        .app
        .world()
        .get::<HouseholdShoppingRoutine>(shopper)
        .unwrap();
    assert_eq!(routine.phase, HouseholdShoppingPhase::ReturningHome);
    assert_eq!(routine.cargo[Good::Bread.index()], 6);
    assert_eq!(routine.cargo[Good::Wood.index()], 0);
    assert_eq!(fixture.stock(shopper, Good::Bread), 8);
    assert_eq!(fixture.stock(fixture.home, Good::Bread), 0);
    assert_eq!(fixture.money(), 2_000);

    let home_position = fixture
        .app
        .world()
        .get::<PlayerPosition>(fixture.home)
        .unwrap()
        .0;
    let entrance = SettlementBuildingKind::House.entrance_position(home_position, 0.0);
    fixture
        .app
        .world_mut()
        .get_mut::<PlayerPosition>(shopper)
        .unwrap()
        .0 = entrance;
    fixture.app.update();
    assert!(fixture.shoppers().is_empty());
    assert_eq!(fixture.stock(fixture.home, Good::Bread), 6);
    assert_eq!(fixture.stock(fixture.home, Good::Wood), 0);
    assert_eq!(fixture.stock(shopper, Good::Bread), 2);
    assert_eq!(fixture.stock(shopper, Good::Wood), 1);
    assert_eq!(fixture.money(), 2_000);
}

#[test]
fn returning_to_a_vacant_hearth_does_not_charge_fuel_for_the_empty_days() {
    let mut fixture = Fixture::new(
        &[
            (PersonId(1), 0),
            (PersonId(2), 0),
            (PersonId(3), 0),
            (PersonId(4), 0),
        ],
        false,
        0,
    );
    {
        let mut pantry = fixture
            .app
            .world_mut()
            .get_mut::<GoodsInventory>(fixture.home)
            .unwrap();
        pantry.add(Good::Bread, 12);
        pantry.add(Good::Wood, 2);
    }
    fixture.at_minute(0);
    fixture
        .app
        .world_mut()
        .get_mut::<WorldTime>(fixture.clock)
        .unwrap()
        .day = 1;
    fixture.at_minute(0);
    assert_eq!(fixture.stock(fixture.home, Good::Wood), 1);

    fixture
        .app
        .world_mut()
        .get_mut::<HouseholdMembers>(fixture.account)
        .unwrap()
        .dwelling = None;
    for (_, person) in &fixture.people {
        fixture
            .app
            .world_mut()
            .entity_mut(*person)
            .remove::<(HomeAssignment, LivesAt)>();
    }
    fixture
        .app
        .world_mut()
        .get_mut::<WorldTime>(fixture.clock)
        .unwrap()
        .day = 10;
    fixture.at_minute(0);
    assert_eq!(fixture.stock(fixture.home, Good::Wood), 1);

    fixture
        .app
        .world_mut()
        .get_mut::<HouseholdMembers>(fixture.account)
        .unwrap()
        .dwelling = Some(HOME);
    for (_, person) in &fixture.people {
        fixture
            .app
            .world_mut()
            .entity_mut(*person)
            .insert((HomeAssignment { home: fixture.home }, LivesAt(HOME)));
    }
    fixture.at_minute(100);
    assert_eq!(fixture.stock(fixture.home, Good::Wood), 1);
    fixture
        .app
        .world_mut()
        .get_mut::<WorldTime>(fixture.clock)
        .unwrap()
        .day = 11;
    fixture.at_minute(0);
    assert_eq!(
        fixture.stock(fixture.home, Good::Wood),
        1,
        "the existing half-bundle must supply the first occupied day"
    );
    let economy = fixture
        .app
        .world()
        .get::<HouseholdEconomy>(fixture.account)
        .unwrap();
    assert_eq!(economy.fuel_satisfaction, 100);
    assert_eq!(economy.fuel_shortage_days, 0);
}

#[test]
fn market_checkout_provisions_current_occupants_not_absent_household_members() {
    let mut fixture = Fixture::new(&[(PersonId(1), 0), (PersonId(2), 0)], true, 4);
    fixture.list(Good::Bread, 9, 10);
    fixture.list(Good::Wood, 3, 5);
    fixture
        .app
        .world_mut()
        .get_mut::<HouseholdEconomy>(fixture.account)
        .unwrap()
        .pennies = 1_000;
    {
        let mut roster = fixture
            .app
            .world_mut()
            .get_mut::<Household>(fixture.home)
            .unwrap();
        roster.resident_ids = vec![PersonId(1)];
        roster.residents = vec!["Resident 1".into()];
    }
    fixture
        .app
        .world_mut()
        .entity_mut(fixture.people[1].1)
        .remove::<(HomeAssignment, LivesAt)>();
    let counter = SettlementBuildingKind::Hall.entrance_position(Vec3::ZERO, 0.0);
    let shopper = fixture.people[0].1;
    fixture.app.world_mut().entity_mut(shopper).insert((
        PlayerPosition(counter),
        HouseholdShoppingRoutine {
            account: fixture.account,
            household: HOUSEHOLD,
            home: fixture.home,
            hall: fixture.hall,
            counter,
            phase: HouseholdShoppingPhase::GoingToMarket,
            cargo: [0; Good::COUNT],
        },
    ));

    fixture.at_minute(0);

    assert_eq!(
        fixture
            .app
            .world()
            .get::<HouseholdMembers>(fixture.account)
            .unwrap()
            .resident_ids
            .len(),
        2
    );
    let routine = fixture
        .app
        .world()
        .get::<HouseholdShoppingRoutine>(shopper)
        .unwrap();
    assert_eq!(routine.phase, HouseholdShoppingPhase::ReturningHome);
    assert_eq!(routine.cargo[Good::Bread.index()], 3);
    assert_eq!(routine.cargo[Good::Wood.index()], 2);
    assert_eq!(fixture.stock(fixture.hall, Good::Bread), 6);
    assert_eq!(fixture.money(), 1_000);
}

#[test]
fn displaced_shopper_returns_owned_goods_and_the_household_is_paid_only_after_resale() {
    let mut fixture = Fixture::new(&[(PersonId(1), 1_000)], true, 0);
    fixture.list(Good::Bread, 3, 10);
    let shopper = fixture.people[0].1;
    fixture
        .app
        .world_mut()
        .get_mut::<GoodsInventory>(shopper)
        .unwrap()
        .add(Good::Wood, 1);
    fixture.at_minute(0);
    let counter = fixture
        .app
        .world()
        .get::<HouseholdShoppingRoutine>(shopper)
        .unwrap()
        .counter;
    fixture
        .app
        .world_mut()
        .get_mut::<PlayerPosition>(shopper)
        .unwrap()
        .0 = counter;
    fixture.app.update();
    assert_eq!(fixture.stock(shopper, Good::Bread), 3);
    assert_eq!(fixture.stock(fixture.hall, Good::Bread), 0);
    assert_eq!(
        fixture
            .app
            .world()
            .get::<HouseholdEconomy>(fixture.account)
            .unwrap()
            .pennies,
        0
    );

    fixture
        .app
        .world_mut()
        .get_mut::<PlayerPosition>(shopper)
        .unwrap()
        .0 = Vec3::new(10.0, 0.0, 0.0);
    fixture
        .app
        .world_mut()
        .get_mut::<HouseholdMembers>(fixture.account)
        .unwrap()
        .dwelling = None;
    fixture
        .app
        .world_mut()
        .entity_mut(shopper)
        .remove::<(HomeAssignment, LivesAt)>();
    fixture.at_minute(30);
    let routine = fixture
        .app
        .world()
        .get::<HouseholdShoppingRoutine>(shopper)
        .unwrap();
    assert_eq!(routine.phase, HouseholdShoppingPhase::ReturningToMarket);
    assert_eq!(fixture.stock(shopper, Good::Bread), 3);
    assert_eq!(
        fixture.stock(fixture.hall, Good::Bread),
        0,
        "goods must reach the counter before being consigned"
    );
    assert!(fixture.app.world().get::<MoveTarget>(shopper).is_some());

    fixture
        .app
        .world_mut()
        .get_mut::<GoodsInventory>(fixture.hall)
        .unwrap()
        .resize_bulk_capacity(0);
    fixture
        .app
        .world_mut()
        .get_mut::<PlayerPosition>(shopper)
        .unwrap()
        .0 = counter;
    fixture.app.update();
    assert_eq!(fixture.shoppers(), vec![shopper]);
    assert_eq!(fixture.stock(shopper, Good::Bread), 3);
    assert_eq!(fixture.stock(fixture.hall, Good::Bread), 0);
    assert_eq!(fixture.money(), 1_000);
    assert_eq!(
        fixture
            .app
            .world()
            .get::<MootMarket>(fixture.hall)
            .unwrap()
            .seller_listed_units(MarketSeller::Household(HOUSEHOLD), Good::Bread),
        0,
        "a full Hall cannot discard or sell goods still with the carrier"
    );
    fixture
        .app
        .world_mut()
        .get_mut::<GoodsInventory>(fixture.hall)
        .unwrap()
        .resize_bulk_capacity(3 * Good::Bread.bulk_per_unit());
    fixture.app.update();
    assert!(fixture.shoppers().is_empty());
    assert_eq!(fixture.stock(shopper, Good::Bread), 0);
    assert_eq!(
        fixture.stock(shopper, Good::Wood),
        1,
        "personal cargo remains personal"
    );
    assert_eq!(fixture.stock(fixture.hall, Good::Bread), 3);
    assert_eq!(
        fixture
            .app
            .world()
            .get::<MootMarket>(fixture.hall)
            .unwrap()
            .seller_listed_units(MarketSeller::Household(HOUSEHOLD), Good::Bread),
        3
    );
    assert_eq!(
        fixture
            .app
            .world()
            .get::<HouseholdEconomy>(fixture.account)
            .unwrap()
            .pennies,
        0,
        "consignment must not create an automatic refund"
    );
    assert_eq!(fixture.money(), 1_000);

    let buyer = fixture
        .app
        .world_mut()
        .spawn((PersonId(99), Wallet::new(1_000), GoodsInventory::new(24)))
        .id();
    let purchase = fixture
        .app
        .world_mut()
        .get_mut::<MootMarket>(fixture.hall)
        .unwrap()
        .purchase(Good::Bread, 3, 1_000, None, None);
    let net: u64 = purchase
        .fills
        .iter()
        .map(|fill| fill.gross - fill.market_fee)
        .sum();
    assert_eq!(purchase.trade.units, 3);
    assert!(fixture
        .app
        .world_mut()
        .get_mut::<Wallet>(buyer)
        .unwrap()
        .debit(purchase.trade.pennies));
    let removed = fixture
        .app
        .world_mut()
        .get_mut::<GoodsInventory>(fixture.hall)
        .unwrap()
        .remove(Good::Bread, purchase.trade.units);
    assert_eq!(
        fixture
            .app
            .world_mut()
            .get_mut::<GoodsInventory>(buyer)
            .unwrap()
            .add(Good::Bread, removed),
        removed
    );
    fixture
        .app
        .world_mut()
        .resource_mut::<BusinessEventQueue>()
        .record_market_purchase(0, SETTLEMENT, purchase.fills);
    fixture.app.update();
    fixture.app.update();
    assert_eq!(
        fixture
            .app
            .world()
            .get::<HouseholdEconomy>(fixture.account)
            .unwrap()
            .pennies,
        net
    );
    assert_eq!(fixture.stock(buyer, Good::Bread), 3);
    assert_eq!(fixture.stock(fixture.hall, Good::Bread), 0);
    assert_eq!(
        fixture.money() + fixture.app.world().get::<Wallet>(buyer).unwrap().balance(),
        2_000
    );
}

#[test]
fn losing_a_dwelling_before_purchase_releases_the_empty_shopping_trip() {
    let mut fixture = Fixture::new(&[(PersonId(1), 1_000)], true, 0);
    fixture.list(Good::Bread, 3, 10);
    fixture.at_minute(0);
    let shopper = fixture.shoppers()[0];
    fixture
        .app
        .world_mut()
        .get_mut::<HouseholdMembers>(fixture.account)
        .unwrap()
        .dwelling = None;
    fixture.app.update();

    assert!(fixture.shoppers().is_empty());
    assert!(fixture.app.world().get::<MoveTarget>(shopper).is_none());
    assert!(fixture
        .app
        .world()
        .get::<NavigationRoutePending>(shopper)
        .is_none());
    assert_eq!(fixture.stock(shopper, Good::Bread), 0);
    assert_eq!(fixture.stock(fixture.hall, Good::Bread), 3);
    assert_eq!(fixture.money(), 1_000);
}
