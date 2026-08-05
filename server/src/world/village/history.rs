//! Bounded, session-only settlement and market history.
//!
//! Runtime entities are good enough for the current restart-heavy test loop.
//! The archive therefore lives only in memory and is sent on request instead
//! of being replicated with every ordinary settlement update.

use std::collections::VecDeque;

use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::{MessageReceiver, MessageSender};

use shared::components::{
    MootAdministration, Nutrition, Occupation, Residence, Settlement, SettlementBuilding,
    SettlementBuildingKind, WorldTime,
};
use shared::economy::{
    BusinessAccount, Good, GoodsInventory, HouseholdEconomy, MarketGoodHistoryDay, MootMarket,
    SettlementEconomy, SettlementHistoryArchive, SettlementHistoryDay, Wallet, WorldHistoryArchive,
    WorldHistoryDay, SETTLEMENT_HISTORY_DAYS,
};
use shared::protocol::{
    ReliableChannel, RequestSettlementHistory, RequestWorldHistory, SettlementHistoryResponse,
    WorldHistoryResponse,
};

use super::{MarketCollectionRoutine, PendingMarketPayment, SettlementEconomyRuntime};

#[derive(Default)]
struct SettlementAggregate {
    resident_wallets: u64,
    hungry: u32,
    employed: u32,
    stock: [u32; Good::COUNT],
    buildings: u16,
    productive_buildings: u16,
    work_positions: u16,
    filled_jobs: u16,
}

/// Authoritative archive. At thirty settlements the bounded data remains only
/// a few megabytes, while clients pay its network cost only for the place they
/// actively inspect.
#[derive(Resource, Default)]
pub struct SettlementHistoryRuntime {
    days: HashMap<Entity, VecDeque<SettlementHistoryDay>>,
    world_days: VecDeque<WorldHistoryDay>,
    last_world_day: HashMap<Entity, u32>,
}

impl SettlementHistoryRuntime {
    fn push(&mut self, settlement: Entity, day: SettlementHistoryDay) {
        let days = self.days.entry(settlement).or_default();
        if days.len() == SETTLEMENT_HISTORY_DAYS {
            days.pop_front();
        }
        days.push_back(day);
    }

    pub fn archive(&self, settlement: Entity, name: &str) -> SettlementHistoryArchive {
        SettlementHistoryArchive {
            settlement: name.to_string(),
            days: self
                .days
                .get(&settlement)
                .map(|days| days.iter().cloned().collect())
                .unwrap_or_default(),
        }
    }

    pub fn world_archive(&self) -> WorldHistoryArchive {
        WorldHistoryArchive {
            days: self.world_days.iter().cloned().collect(),
        }
    }

    fn world_snapshot(&self, day: u32) -> Option<WorldHistoryDay> {
        let snapshots: Vec<&SettlementHistoryDay> = self
            .days
            .values()
            .filter_map(|days| days.iter().rev().find(|snapshot| snapshot.day == day))
            .collect();
        if snapshots.is_empty() {
            return None;
        }
        let mut world = WorldHistoryDay {
            day,
            settlements: snapshots.len().min(u32::MAX as usize) as u32,
            population: 0,
            employed: 0,
            hungry: 0,
            civic_treasury: 0,
            market_cash: 0,
            resident_wallet_money: 0,
            pending_payments: 0,
            total_local_coin: 0,
            stock_liquidation_value: 0,
            physical_stock: [0; Good::COUNT],
            food_reserves: 0,
            food_produced: 0,
            food_consumed: 0,
            buildings: 0,
            productive_buildings: 0,
            prosperity: 0.0,
        };
        let mut prosperity_weight = 0u32;
        let mut prosperity_total = 0.0f64;
        for snapshot in snapshots {
            world.population = world.population.saturating_add(snapshot.population);
            world.employed = world.employed.saturating_add(snapshot.employed);
            world.hungry = world.hungry.saturating_add(snapshot.hungry);
            world.civic_treasury = world.civic_treasury.saturating_add(snapshot.civic_treasury);
            world.market_cash = world.market_cash.saturating_add(
                snapshot
                    .market_cash
                    .iter()
                    .copied()
                    .fold(0, u64::saturating_add),
            );
            world.resident_wallet_money = world
                .resident_wallet_money
                .saturating_add(snapshot.resident_wallet_money);
            world.pending_payments = world
                .pending_payments
                .saturating_add(snapshot.pending_payments);
            world.total_local_coin = world
                .total_local_coin
                .saturating_add(snapshot.total_local_coin);
            world.stock_liquidation_value = world
                .stock_liquidation_value
                .saturating_add(snapshot.stock_liquidation_value);
            world.food_reserves = world.food_reserves.saturating_add(snapshot.food_reserves);
            world.food_produced = world.food_produced.saturating_add(snapshot.food_produced);
            world.food_consumed = world.food_consumed.saturating_add(snapshot.food_consumed);
            world.buildings = world
                .buildings
                .saturating_add(u32::from(snapshot.buildings));
            world.productive_buildings = world
                .productive_buildings
                .saturating_add(u32::from(snapshot.productive_buildings));
            for good in Good::ALL {
                world.physical_stock[good.index()] = world.physical_stock[good.index()]
                    .saturating_add(snapshot.physical_stock[good.index()]);
            }
            let weight = snapshot.population.max(1);
            prosperity_weight = prosperity_weight.saturating_add(weight);
            prosperity_total += f64::from(snapshot.prosperity) * f64::from(weight);
        }
        world.prosperity = if prosperity_weight == 0 {
            0.0
        } else {
            (prosperity_total / f64::from(prosperity_weight)) as f32
        };
        Some(world)
    }

    fn push_world(&mut self, day: WorldHistoryDay) {
        if self
            .world_days
            .back()
            .is_some_and(|existing| existing.day == day.day)
        {
            self.world_days.pop_back();
        }
        if self.world_days.len() == SETTLEMENT_HISTORY_DAYS {
            self.world_days.pop_front();
        }
        self.world_days.push_back(day);
    }
}

/// Close daily market counters and record a conservation-oriented settlement
/// snapshot after food consumption and prosperity have been updated.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn capture_settlement_history(
    world_time: Query<&WorldTime>,
    economy_runtime: Res<SettlementEconomyRuntime>,
    mut history: ResMut<SettlementHistoryRuntime>,
    mut halls: Query<(
        Entity,
        &Settlement,
        &GoodsInventory,
        &mut MootMarket,
        &SettlementEconomy,
        Option<&MootAdministration>,
    )>,
    buildings: Query<(
        &SettlementBuilding,
        Option<&GoodsInventory>,
        Option<&BusinessAccount>,
        Option<&HouseholdEconomy>,
    )>,
    residents: Query<(
        &Residence,
        Option<&Occupation>,
        Option<&Wallet>,
        Option<&Nutrition>,
        Option<&GoodsInventory>,
    )>,
    pending: Query<&PendingMarketPayment>,
    collections: Query<&MarketCollectionRoutine>,
) {
    let Some(day) = world_time.iter().next().map(|time| time.day) else {
        return;
    };

    let mut aggregates: HashMap<String, SettlementAggregate> = HashMap::new();
    for (building, inventory, business_account, household_economy) in buildings.iter() {
        let aggregate = aggregates.entry(building.settlement.clone()).or_default();
        aggregate.buildings = aggregate.buildings.saturating_add(1);
        if matches!(
            building.kind,
            SettlementBuildingKind::Farmstead
                | SettlementBuildingKind::LumberjackHut
                | SettlementBuildingKind::FishermansHut
        ) {
            aggregate.productive_buildings = aggregate.productive_buildings.saturating_add(1);
        }
        aggregate.work_positions = aggregate
            .work_positions
            .saturating_add(u16::from(building.kind.positions()));
        aggregate.filled_jobs = aggregate
            .filled_jobs
            .saturating_add(building.workers.len().min(u16::MAX as usize) as u16);
        add_inventory(&mut aggregate.stock, inventory);
        // Keep the existing private-money band conservation-complete as coin
        // moves from personal wallets into household and business ledgers.
        aggregate.resident_wallets = aggregate
            .resident_wallets
            .saturating_add(business_account.map_or(0, |account| account.cash))
            .saturating_add(household_economy.map_or(0, |economy| economy.pennies));
    }
    for (residence, occupation, wallet, nutrition, inventory) in residents.iter() {
        let aggregate = aggregates.entry(residence.0.clone()).or_default();
        aggregate.resident_wallets = aggregate
            .resident_wallets
            .saturating_add(wallet.copied().map_or(0, Wallet::balance));
        aggregate.hungry = aggregate
            .hungry
            .saturating_add(u32::from(nutrition.is_some_and(|value| value.is_hungry())));
        aggregate.employed = aggregate.employed.saturating_add(u32::from(
            occupation.is_some_and(|occupation| occupation.0.is_some()),
        ));
        add_inventory(&mut aggregate.stock, inventory);
    }
    let mut pending_by_settlement: HashMap<Entity, u64> = HashMap::new();
    for payment in pending.iter() {
        let total = pending_by_settlement.entry(payment.settlement).or_default();
        *total = total.saturating_add(payment.pennies);
    }
    for collection in collections.iter() {
        let total = pending_by_settlement.entry(collection.hall).or_default();
        *total = total.saturating_add(collection.reserved_pennies);
    }

    let mut completed_days = HashSet::new();
    for (entity, settlement, hall_inventory, mut market, economy, administration) in
        halls.iter_mut()
    {
        let previous = history.last_world_day.insert(entity, day);
        let Some(previous) = previous else {
            continue;
        };
        let elapsed = day.saturating_sub(previous);
        if elapsed == 0 {
            continue;
        }

        let aggregate = aggregates.remove(&settlement.name).unwrap_or_default();
        let mut physical_stock = aggregate.stock;
        add_inventory(&mut physical_stock, Some(hall_inventory));
        let pending_payments = pending_by_settlement
            .get(&entity)
            .copied()
            .unwrap_or(0)
            .saturating_add(administration.map_or(0, |office| office.wage_arrears));

        for offset in 0..elapsed {
            let is_latest = offset + 1 == elapsed;
            let completed_day = previous.saturating_add(offset).saturating_add(1);
            let mut market_days = [MarketGoodHistoryDay::default(); Good::COUNT];
            let mut market_cash = [0u64; Good::COUNT];
            let mut liquidation_value = 0u64;
            for good in Good::ALL {
                let pool = market.pool(good);
                let flow = if is_latest {
                    pool.day
                } else {
                    Default::default()
                };
                market_days[good.index()] = MarketGoodHistoryDay {
                    opening_bid: if is_latest {
                        flow.opening_bid
                    } else {
                        pool.bid
                    },
                    opening_ask: if is_latest {
                        flow.opening_ask
                    } else {
                        pool.ask
                    },
                    closing_bid: pool.bid,
                    closing_ask: pool.ask,
                    high_bid: if is_latest { flow.high_bid } else { pool.bid },
                    low_bid: if is_latest { flow.low_bid } else { pool.bid },
                    high_ask: if is_latest { flow.high_ask } else { pool.ask },
                    low_ask: if is_latest { flow.low_ask } else { pool.ask },
                    producer_units: flow.producer_units,
                    producer_coin: flow.producer_coin,
                    consumer_units: flow.consumer_units,
                    consumer_coin: flow.consumer_coin,
                    closing_stock: hall_inventory.amount(good),
                    target_stock: pool.target_stock,
                    pool_cash: pool.cash,
                };
                market_cash[good.index()] = pool.cash;
                liquidation_value = liquidation_value.saturating_add(
                    u64::from(physical_stock[good.index()]).saturating_mul(pool.bid),
                );
            }

            let history_index = elapsed.saturating_sub(offset).saturating_sub(1) as usize;
            let (food_produced, food_consumed) = economy_runtime
                .by_settlement
                .get(&entity)
                .map(|state| {
                    (
                        state
                            .production_history
                            .get(history_index)
                            .copied()
                            .unwrap_or(0),
                        state
                            .consumption_history
                            .get(history_index)
                            .copied()
                            .unwrap_or(0),
                    )
                })
                .unwrap_or_default();
            let market_total = market_cash.into_iter().fold(0u64, u64::saturating_add);
            let total_local_coin = settlement
                .treasury
                .saturating_add(market_total)
                .saturating_add(aggregate.resident_wallets)
                .saturating_add(pending_payments);

            history.push(
                entity,
                SettlementHistoryDay {
                    day: completed_day,
                    market: market_days,
                    civic_treasury: settlement.treasury,
                    market_cash,
                    resident_wallet_money: aggregate.resident_wallets,
                    pending_payments,
                    physical_stock,
                    stock_liquidation_value: liquidation_value,
                    total_local_coin,
                    population: settlement.residents,
                    employed: aggregate.employed,
                    hungry: aggregate.hungry,
                    food_reserves: economy.edible_stock,
                    food_produced,
                    food_consumed,
                    buildings: aggregate.buildings.saturating_add(1),
                    productive_buildings: aggregate.productive_buildings,
                    work_positions: aggregate.work_positions,
                    filled_jobs: aggregate.filled_jobs,
                    prosperity: economy.prosperity,
                    reserve_prosperity: economy.reserve_prosperity,
                    production_prosperity: economy.production_prosperity,
                    housing_prosperity: economy.housing_prosperity,
                    employment_prosperity: economy.employment_prosperity,
                    hunger_penalty: economy.hunger_penalty,
                },
            );
            completed_days.insert(completed_day);
        }
        market.begin_new_day();
    }
    let mut completed_days: Vec<u32> = completed_days.into_iter().collect();
    completed_days.sort_unstable();
    let world_days: Vec<WorldHistoryDay> = completed_days
        .into_iter()
        .filter_map(|day| history.world_snapshot(day))
        .collect();
    for day in world_days {
        history.push_world(day);
    }
}

fn add_inventory(target: &mut [u32; Good::COUNT], inventory: Option<&GoodsInventory>) {
    let Some(inventory) = inventory else {
        return;
    };
    for good in Good::ALL {
        target[good.index()] = target[good.index()].saturating_add(inventory.amount(good));
    }
}

/// Serve history only when a client opens a relevant page.
pub fn handle_settlement_history_requests(
    history: Res<SettlementHistoryRuntime>,
    settlements: Query<&Settlement>,
    mut clients: Query<
        (
            &mut MessageReceiver<RequestSettlementHistory>,
            &mut MessageSender<SettlementHistoryResponse>,
        ),
        With<ClientOf>,
    >,
) {
    for (mut receiver, mut sender) in clients.iter_mut() {
        for request in receiver.receive() {
            let Ok(settlement) = settlements.get(request.settlement) else {
                continue;
            };
            sender.send::<ReliableChannel>(SettlementHistoryResponse {
                settlement: request.settlement,
                archive: history.archive(request.settlement, &settlement.name),
            });
        }
    }
}

pub fn handle_world_history_requests(
    history: Res<SettlementHistoryRuntime>,
    mut clients: Query<
        (
            &mut MessageReceiver<RequestWorldHistory>,
            &mut MessageSender<WorldHistoryResponse>,
        ),
        With<ClientOf>,
    >,
) {
    for (mut receiver, mut sender) in clients.iter_mut() {
        for _ in receiver.receive() {
            sender.send::<ReliableChannel>(WorldHistoryResponse {
                archive: history.world_archive(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_day(day: u32) -> SettlementHistoryDay {
        SettlementHistoryDay {
            day,
            market: [MarketGoodHistoryDay::default(); Good::COUNT],
            civic_treasury: 0,
            market_cash: [0; Good::COUNT],
            resident_wallet_money: 0,
            pending_payments: 0,
            physical_stock: [0; Good::COUNT],
            stock_liquidation_value: 0,
            total_local_coin: 0,
            population: 0,
            employed: 0,
            hungry: 0,
            food_reserves: 0,
            food_produced: 0,
            food_consumed: 0,
            buildings: 0,
            productive_buildings: 0,
            work_positions: 0,
            filled_jobs: 0,
            prosperity: 0.0,
            reserve_prosperity: 0.0,
            production_prosperity: 0.0,
            housing_prosperity: 0.0,
            employment_prosperity: 0.0,
            hunger_penalty: 0.0,
        }
    }

    #[test]
    fn archive_is_a_365_day_circular_window() {
        let settlement = Entity::from_bits(7);
        let mut history = SettlementHistoryRuntime::default();
        for day in 1..=400 {
            history.push(settlement, empty_day(day));
        }
        let archive = history.archive(settlement, "Brackwater");
        assert_eq!(archive.days.len(), 365);
        assert_eq!(archive.days.first().unwrap().day, 36);
        assert_eq!(archive.days.last().unwrap().day, 400);
    }

    #[test]
    fn world_rollup_can_recover_each_day_after_a_multi_day_jump() {
        let first = Entity::from_bits(7);
        let second = Entity::from_bits(8);
        let mut history = SettlementHistoryRuntime::default();
        for day in 1..=3 {
            let mut first_day = empty_day(day);
            first_day.population = day;
            history.push(first, first_day);

            let mut second_day = empty_day(day);
            second_day.population = day * 10;
            history.push(second, second_day);
        }

        let day_one = history.world_snapshot(1).unwrap();
        let day_three = history.world_snapshot(3).unwrap();
        assert_eq!(day_one.settlements, 2);
        assert_eq!(day_one.population, 11);
        assert_eq!(day_three.population, 33);
    }
}
