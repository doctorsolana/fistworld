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
    BuildingId, BuildingOf, CivicEmployment, EmployedAt, MootAdministration, Nutrition, OwnedBy,
    PersonId, ResidentOf, Settlement, SettlementBuilding, SettlementBuildingKind, SettlementId,
    SettlementPolicies, WorldTime,
};
use shared::economy::{
    business_working_capital, BusinessAccount, BusinessCondition, BusinessDayLedger,
    BusinessHistoryArchive, BusinessHistoryDay, BusinessManagementPolicy,
    BusinessProcurementPolicy, BusinessSalePolicy, BusinessWagePolicy, CivicAccount,
    CivicHistoryDay, Good, GoodsInventory, HouseholdEconomy, MarketGoodHistoryDay, MarketSeller,
    MootMarket, SettlementEconomy, SettlementHistoryArchive, SettlementHistoryDay, Wallet,
    WorldHistoryArchive, WorldHistoryDay, SETTLEMENT_HISTORY_DAYS,
};
use shared::protocol::{
    ReliableChannel, RequestSettlementHistory, RequestWorldHistory, SettlementHistoryResponse,
    WorldHistoryResponse,
};

use super::SettlementEconomyRuntime;

#[derive(Default)]
struct SettlementAggregate {
    resident_wallets: u64,
    household_cash: u64,
    business_cash: u64,
    business_wage_arrears: u64,
    business_tax_arrears: u64,
    unlisted_business_food: u32,
    hungry: u32,
    employed: u32,
    stock: [u32; Good::COUNT],
    buildings: u16,
    productive_buildings: u16,
    work_positions: u16,
    filled_jobs: u16,
}

#[derive(Clone)]
struct BusinessSnapshot {
    id: BuildingId,
    settlement: SettlementId,
    kind: SettlementBuildingKind,
    owner_id: Option<PersonId>,
    owner_name: Option<String>,
    output_good: Option<Good>,
    account: BusinessAccount,
    sale: BusinessSalePolicy,
    wage: BusinessWagePolicy,
    management: BusinessManagementPolicy,
    procurement: BusinessProcurementPolicy,
    condition: BusinessCondition,
    stock: [u32; Good::COUNT],
}

struct BusinessHistoryRecord {
    settlement: SettlementId,
    kind: SettlementBuildingKind,
    owner_id: Option<PersonId>,
    owner_name: Option<String>,
    output_good: Option<Good>,
    days: VecDeque<BusinessHistoryDay>,
    last_seen_day: u32,
}

/// Authoritative bounded archive. It is sampled only at world-day boundaries,
/// and clients pay its network cost only for the place they actively inspect.
#[derive(Resource, Default)]
pub struct SettlementHistoryRuntime {
    days: HashMap<Entity, VecDeque<SettlementHistoryDay>>,
    business_days: HashMap<BuildingId, BusinessHistoryRecord>,
    world_days: VecDeque<WorldHistoryDay>,
    last_world_day: HashMap<Entity, u32>,
    last_capture_day: Option<u32>,
}

impl SettlementHistoryRuntime {
    fn push(&mut self, settlement: Entity, day: SettlementHistoryDay) {
        let days = self.days.entry(settlement).or_default();
        if days.len() == SETTLEMENT_HISTORY_DAYS {
            days.pop_front();
        }
        days.push_back(day);
    }

    fn push_business(&mut self, snapshot: &BusinessSnapshot, day: BusinessHistoryDay) {
        let record =
            self.business_days
                .entry(snapshot.id)
                .or_insert_with(|| BusinessHistoryRecord {
                    settlement: snapshot.settlement,
                    kind: snapshot.kind,
                    owner_id: snapshot.owner_id,
                    owner_name: snapshot.owner_name.clone(),
                    output_good: snapshot.output_good,
                    days: VecDeque::new(),
                    last_seen_day: day.day,
                });
        // A takeover keeps the firm's stable history but updates its current
        // owner metadata for future UI and lab reports.
        record.settlement = snapshot.settlement;
        record.kind = snapshot.kind;
        record.owner_id = snapshot.owner_id;
        record.owner_name.clone_from(&snapshot.owner_name);
        record.output_good = snapshot.output_good;
        record.last_seen_day = day.day;
        if record.days.len() == SETTLEMENT_HISTORY_DAYS {
            record.days.pop_front();
        }
        record.days.push_back(day);
    }

    pub fn archive(
        &self,
        settlement: Entity,
        settlement_id: SettlementId,
        name: &str,
    ) -> SettlementHistoryArchive {
        let mut businesses: Vec<_> = self
            .business_days
            .iter()
            .filter(|(_, record)| record.settlement == settlement_id)
            .map(|(id, record)| BusinessHistoryArchive {
                id: *id,
                settlement: record.settlement,
                kind: record.kind,
                owner_id: record.owner_id,
                owner_name: record.owner_name.clone(),
                output_good: record.output_good,
                days: record.days.iter().copied().collect(),
            })
            .collect();
        businesses.sort_unstable_by_key(|business| business.id);
        SettlementHistoryArchive {
            settlement: name.to_string(),
            days: self
                .days
                .get(&settlement)
                .map(|days| days.iter().cloned().collect())
                .unwrap_or_default(),
            businesses,
        }
    }

    #[cfg(test)]
    pub fn all_business_archives(&self) -> Vec<BusinessHistoryArchive> {
        let mut businesses: Vec<_> = self
            .business_days
            .iter()
            .map(|(id, record)| BusinessHistoryArchive {
                id: *id,
                settlement: record.settlement,
                kind: record.kind,
                owner_id: record.owner_id,
                owner_name: record.owner_name.clone(),
                output_good: record.output_good,
                days: record.days.iter().copied().collect(),
            })
            .collect();
        businesses.sort_unstable_by_key(|business| business.id);
        businesses
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
            resident_wallet_money: 0,
            household_cash: 0,
            business_cash: 0,
            business_wage_arrears: 0,
            business_tax_arrears: 0,
            civic_wage_arrears: 0,
            total_local_coin: 0,
            stock_liquidation_value: 0,
            physical_stock: [0; Good::COUNT],
            food_reserves: 0,
            purchasable_food: 0,
            unlisted_business_food: 0,
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
            world.resident_wallet_money = world
                .resident_wallet_money
                .saturating_add(snapshot.resident_wallet_money);
            world.household_cash = world.household_cash.saturating_add(snapshot.household_cash);
            world.business_cash = world.business_cash.saturating_add(snapshot.business_cash);
            world.business_wage_arrears = world
                .business_wage_arrears
                .saturating_add(snapshot.business_wage_arrears);
            world.business_tax_arrears = world
                .business_tax_arrears
                .saturating_add(snapshot.business_tax_arrears);
            world.civic_wage_arrears = world
                .civic_wage_arrears
                .saturating_add(snapshot.civic_wage_arrears);
            world.total_local_coin = world
                .total_local_coin
                .saturating_add(snapshot.total_local_coin);
            world.stock_liquidation_value = world
                .stock_liquidation_value
                .saturating_add(snapshot.stock_liquidation_value);
            world.food_reserves = world.food_reserves.saturating_add(snapshot.food_reserves);
            world.purchasable_food = world
                .purchasable_food
                .saturating_add(snapshot.purchasable_food);
            world.unlisted_business_food = world
                .unlisted_business_food
                .saturating_add(snapshot.unlisted_business_food);
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

    fn prune_stale_businesses(&mut self, current_day: u32) {
        // Preserve closed firms for a full in-game year so their history
        // remains useful, but do not retain every demolished or bankrupt
        // business identity for the lifetime of a persistent server.
        self.business_days.retain(|_, record| {
            current_day.saturating_sub(record.last_seen_day)
                <= SETTLEMENT_HISTORY_DAYS.min(u32::MAX as usize) as u32
        });
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
        &SettlementId,
        &Settlement,
        &GoodsInventory,
        &mut MootMarket,
        &SettlementEconomy,
        Option<&MootAdministration>,
        Option<&CivicAccount>,
        Option<&SettlementPolicies>,
    )>,
    buildings: Query<(
        &BuildingId,
        &BuildingOf,
        &SettlementBuilding,
        Option<&GoodsInventory>,
        Option<&BusinessAccount>,
        Option<&HouseholdEconomy>,
        Option<&BusinessSalePolicy>,
        Option<&BusinessWagePolicy>,
        Option<&BusinessManagementPolicy>,
        Option<&BusinessProcurementPolicy>,
        Option<&BusinessCondition>,
        Option<&OwnedBy>,
    )>,
    residents: Query<(
        &ResidentOf,
        Option<&EmployedAt>,
        Option<&CivicEmployment>,
        Option<&Wallet>,
        Option<&Nutrition>,
        Option<&GoodsInventory>,
    )>,
    employment: Query<&EmployedAt>,
) {
    let Some(day) = world_time.iter().next().map(|time| time.day) else {
        return;
    };
    // History is daily data. Avoid walking every resident, building and firm
    // on all the simulation ticks between two day boundaries.
    if history.last_capture_day == Some(day) {
        return;
    }
    history.last_capture_day = Some(day);

    let mut filled_jobs: HashMap<BuildingId, u16> = HashMap::new();
    for employed_at in employment.iter() {
        let count = filled_jobs.entry(employed_at.0).or_default();
        *count = count.saturating_add(1);
    }
    let mut aggregates: HashMap<SettlementId, SettlementAggregate> = HashMap::new();
    let mut business_snapshots: HashMap<SettlementId, Vec<BusinessSnapshot>> = HashMap::new();
    for (
        building_id,
        building_of,
        building,
        inventory,
        business_account,
        household_economy,
        sale,
        wage,
        management,
        procurement,
        condition,
        owner,
    ) in buildings.iter()
    {
        let aggregate = aggregates.entry(building_of.0).or_default();
        aggregate.buildings = aggregate.buildings.saturating_add(1);
        if matches!(
            building.kind,
            SettlementBuildingKind::Farmstead
                | SettlementBuildingKind::LumberjackHut
                | SettlementBuildingKind::FishermansHut
                | SettlementBuildingKind::Windmill
                | SettlementBuildingKind::Bakery
        ) {
            aggregate.productive_buildings = aggregate.productive_buildings.saturating_add(1);
        }
        aggregate.work_positions = aggregate
            .work_positions
            .saturating_add(u16::from(building.kind.positions()));
        aggregate.filled_jobs = aggregate
            .filled_jobs
            .saturating_add(filled_jobs.get(building_id).copied().unwrap_or(0));
        add_inventory(&mut aggregate.stock, inventory);
        aggregate.business_cash = aggregate
            .business_cash
            .saturating_add(business_account.map_or(0, |account| account.cash));
        aggregate.business_wage_arrears = aggregate
            .business_wage_arrears
            .saturating_add(business_account.map_or(0, |account| account.wage_arrears));
        aggregate.business_tax_arrears = aggregate
            .business_tax_arrears
            .saturating_add(business_account.map_or(0, |account| account.tax_arrears));
        if business_account.is_some() {
            aggregate.unlisted_business_food = aggregate
                .unlisted_business_food
                .saturating_add(inventory.map_or(0, GoodsInventory::edible_amount));
        }
        aggregate.household_cash = aggregate
            .household_cash
            .saturating_add(household_economy.map_or(0, |economy| economy.pennies));
        if let Some(account) = business_account.copied() {
            let output_good = super::business_output(building.kind);
            let mut stock = [0; Good::COUNT];
            add_inventory(&mut stock, inventory);
            business_snapshots
                .entry(building_of.0)
                .or_default()
                .push(BusinessSnapshot {
                    id: *building_id,
                    settlement: building_of.0,
                    kind: building.kind,
                    owner_id: owner.map(|owner| owner.0),
                    owner_name: building.owner.clone(),
                    output_good,
                    account,
                    sale: sale.copied().unwrap_or_else(|| {
                        output_good
                            .map_or_else(BusinessSalePolicy::default, BusinessSalePolicy::for_good)
                    }),
                    wage: wage.copied().unwrap_or_default(),
                    management: management.copied().unwrap_or_default(),
                    procurement: procurement.copied().unwrap_or_default(),
                    condition: condition.copied().unwrap_or_default(),
                    stock,
                });
        }
    }
    for (resident_of, employed_at, civic_job, wallet, nutrition, inventory) in residents.iter() {
        let aggregate = aggregates.entry(resident_of.0).or_default();
        aggregate.resident_wallets = aggregate
            .resident_wallets
            .saturating_add(wallet.copied().map_or(0, Wallet::balance));
        aggregate.hungry = aggregate
            .hungry
            .saturating_add(u32::from(nutrition.is_some_and(|value| value.is_hungry())));
        aggregate.employed = aggregate
            .employed
            .saturating_add(u32::from(employed_at.is_some() || civic_job.is_some()));
        add_inventory(&mut aggregate.stock, inventory);
    }
    let mut completed_days = HashSet::new();
    for (
        entity,
        settlement_id,
        settlement,
        hall_inventory,
        mut market,
        economy,
        administration,
        civic_account,
        policies,
    ) in halls.iter_mut()
    {
        let previous = history.last_world_day.insert(entity, day);
        let Some(previous) = previous else {
            continue;
        };
        let elapsed = day.saturating_sub(previous);
        if elapsed == 0 {
            continue;
        }

        let aggregate = aggregates.remove(settlement_id).unwrap_or_default();
        let snapshots = business_snapshots.remove(settlement_id).unwrap_or_default();
        let mut physical_stock = aggregate.stock;
        add_inventory(&mut physical_stock, Some(hall_inventory));
        let civic_wage_arrears = administration.map_or(0, |office| office.wage_arrears);

        for offset in 0..elapsed {
            let is_latest = offset + 1 == elapsed;
            let completed_day = previous.saturating_add(offset).saturating_add(1);
            let mut market_days = [MarketGoodHistoryDay::default(); Good::COUNT];
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
                    unavailable_units: flow.unavailable_units,
                    unaffordable_units: flow.unaffordable_units,
                    closing_stock: hall_inventory.amount(good),
                    target_stock: pool.target_stock,
                    listed_units: market.listed_units(good),
                };
                liquidation_value = liquidation_value.saturating_add(
                    u64::from(physical_stock[good.index()]).saturating_mul(pool.bid),
                );
            }

            let history_index = elapsed.saturating_sub(offset).saturating_sub(1) as usize;
            let (food_produced, food_consumed) =
                economy_runtime.historical_food(entity, history_index);
            let total_local_coin = settlement
                .treasury
                .saturating_add(aggregate.resident_wallets)
                .saturating_add(aggregate.household_cash)
                .saturating_add(aggregate.business_cash);
            let civic_ledger = civic_account
                .and_then(|account| account.ledger_for_day(completed_day))
                .unwrap_or_else(|| shared::economy::CivicDayLedger::empty(completed_day));
            let civic_observed = civic_account
                .and_then(|account| account.ledger_for_day(completed_day))
                .is_some();
            let filled_positions = administration.map_or(0, |office| {
                crate::world::village::civic::filled_civic_positions(office)
                    .min(usize::from(u16::MAX)) as u16
            });
            let policy = policies.copied().unwrap_or_default();
            let desired_positions = crate::world::village::civic::desired_civic_positions(
                settlement.tier,
                policy.staffing_posture,
            )
            .min(usize::from(u16::MAX)) as u16;

            history.push(
                entity,
                SettlementHistoryDay {
                    day: completed_day,
                    market: market_days,
                    civic_treasury: settlement.treasury,
                    resident_wallet_money: aggregate.resident_wallets,
                    household_cash: aggregate.household_cash,
                    business_cash: aggregate.business_cash,
                    business_wage_arrears: aggregate.business_wage_arrears,
                    business_tax_arrears: aggregate.business_tax_arrears,
                    civic_wage_arrears,
                    civic: CivicHistoryDay {
                        observed: civic_observed,
                        permit_income: civic_ledger.permit_income,
                        market_fee_income: civic_ledger.market_fee_income,
                        profit_tax_income: civic_ledger.profit_tax_income,
                        public_sale_income: civic_ledger.public_sale_income,
                        wage_expense: civic_ledger.wage_expense,
                        poor_relief_expense: civic_ledger.poor_relief_expense,
                        material_expense: civic_ledger.material_expense,
                        filled_positions,
                        vacant_positions: desired_positions.saturating_sub(filled_positions),
                        market_fee_bps: policy.market_fee_bps,
                        business_profit_tax_bps: policy.business_profit_tax_bps,
                        poor_relief: policy.poor_relief,
                        food_reserve_target_days: policy.food_reserve_target_days,
                        civic_payroll_reserve_days: policy.civic_payroll_reserve_days,
                        staffing_posture: policy.staffing_posture,
                        business_permit_subsidy_bps: policy.business_permit_subsidy_bps,
                        strategy: policy.strategy,
                        autopilot: policy.autopilot,
                        adjustment: (policy.last_change_day == completed_day)
                            .then_some(policy.last_adjustment)
                            .unwrap_or_default(),
                        reason: (policy.last_change_day == completed_day)
                            .then_some(policy.last_reason)
                            .unwrap_or_default(),
                    },
                    physical_stock,
                    stock_liquidation_value: liquidation_value,
                    total_local_coin,
                    population: settlement.residents,
                    employed: aggregate.employed,
                    hungry: aggregate.hungry,
                    food_reserves: economy.edible_stock,
                    purchasable_food: market.listed_edible_units(),
                    unlisted_business_food: aggregate.unlisted_business_food,
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
            for snapshot in &snapshots {
                let ledger = if is_latest {
                    snapshot.account.previous_day
                } else {
                    BusinessDayLedger::empty(completed_day)
                };
                let observed = is_latest && ledger.day != u32::MAX;
                let listed_output_units = snapshot.output_good.map_or(0, |good| {
                    market.seller_listed_units(MarketSeller::Business(snapshot.id), good)
                });
                history.push_business(
                    snapshot,
                    BusinessHistoryDay {
                        day: completed_day,
                        observed,
                        cash: snapshot.account.cash,
                        protected_working_capital: business_working_capital(
                            snapshot.kind.positions(),
                            &snapshot.wage,
                            &snapshot.management,
                            &snapshot.procurement,
                            Some(&market),
                        )
                        .total_with_liabilities(&snapshot.account),
                        withdrawable_profit: snapshot.account.withdrawable_profit(
                            business_working_capital(
                                snapshot.kind.positions(),
                                &snapshot.wage,
                                &snapshot.management,
                                &snapshot.procurement,
                                Some(&market),
                            )
                            .total(),
                        ),
                        wage_arrears: snapshot.account.wage_arrears,
                        tax_arrears: snapshot.account.tax_arrears,
                        gross_revenue: if observed { ledger.gross_revenue } else { 0 },
                        wage_expense: if observed { ledger.wage_expense } else { 0 },
                        input_expense: if observed { ledger.input_expense } else { 0 },
                        market_fees: if observed { ledger.market_fees } else { 0 },
                        profit_taxes: if observed { ledger.profit_taxes } else { 0 },
                        owner_withdrawals: if observed {
                            ledger.owner_withdrawals
                        } else {
                            0
                        },
                        profit: if observed { ledger.profit() } else { 0 },
                        produced_units: if observed { ledger.produced_units } else { 0 },
                        sold_units: if observed { ledger.sold_units } else { 0 },
                        purchased_input_units: if observed {
                            ledger.purchased_input_units
                        } else {
                            0
                        },
                        workplace_stock: snapshot.stock,
                        listed_output_units,
                        asking_unit_price: snapshot.sale.asking_unit_price,
                        daily_wage: snapshot.wage.daily_wage,
                        strategy: snapshot.management.strategy,
                        autopilot: snapshot.management.autopilot,
                        state: snapshot.condition.state,
                    },
                );
            }
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
    history.prune_stale_businesses(day);
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
    settlements: Query<(&Settlement, &SettlementId)>,
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
            let Ok((settlement, settlement_id)) = settlements.get(request.settlement) else {
                continue;
            };
            sender.send::<ReliableChannel>(SettlementHistoryResponse {
                settlement: request.settlement,
                archive: history.archive(request.settlement, *settlement_id, &settlement.name),
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
            resident_wallet_money: 0,
            household_cash: 0,
            business_cash: 0,
            business_wage_arrears: 0,
            business_tax_arrears: 0,
            civic_wage_arrears: 0,
            civic: CivicHistoryDay::default(),
            physical_stock: [0; Good::COUNT],
            stock_liquidation_value: 0,
            total_local_coin: 0,
            population: 0,
            employed: 0,
            hungry: 0,
            food_reserves: 0,
            purchasable_food: 0,
            unlisted_business_food: 0,
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
        let archive = history.archive(settlement, SettlementId(7), "Brackwater");
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

    #[test]
    fn business_archive_is_bounded_and_scoped_by_stable_settlement_id() {
        let settlement_entity = Entity::from_bits(9);
        let settlement_id = SettlementId(17);
        let snapshot = BusinessSnapshot {
            id: BuildingId(42),
            settlement: settlement_id,
            kind: SettlementBuildingKind::Farmstead,
            owner_id: Some(PersonId(3)),
            owner_name: Some("Edric".to_string()),
            output_good: Some(Good::Wheat),
            account: BusinessAccount::default(),
            sale: BusinessSalePolicy::for_good(Good::Wheat),
            wage: BusinessWagePolicy::default(),
            management: BusinessManagementPolicy::default(),
            procurement: BusinessProcurementPolicy::default(),
            condition: BusinessCondition::default(),
            stock: [0; Good::COUNT],
        };
        let mut history = SettlementHistoryRuntime::default();
        for day in 1..=400 {
            history.push_business(
                &snapshot,
                BusinessHistoryDay {
                    day,
                    observed: true,
                    cash: u64::from(day),
                    protected_working_capital: 0,
                    withdrawable_profit: 0,
                    wage_arrears: 0,
                    tax_arrears: 0,
                    gross_revenue: 10,
                    wage_expense: 4,
                    input_expense: 0,
                    market_fees: 1,
                    profit_taxes: 0,
                    owner_withdrawals: 0,
                    profit: 5,
                    produced_units: 2,
                    sold_units: 1,
                    purchased_input_units: 0,
                    workplace_stock: [0; Good::COUNT],
                    listed_output_units: 1,
                    asking_unit_price: 80,
                    daily_wage: 100,
                    strategy: shared::economy::BusinessStrategy::Balanced,
                    autopilot: true,
                    state: shared::economy::BusinessState::Operating,
                },
            );
        }

        let archive = history.archive(settlement_entity, settlement_id, "Brackwater");
        assert_eq!(archive.businesses.len(), 1);
        let business = &archive.businesses[0];
        assert_eq!(business.id, BuildingId(42));
        assert_eq!(business.owner_id, Some(PersonId(3)));
        assert_eq!(business.days.len(), SETTLEMENT_HISTORY_DAYS);
        assert_eq!(business.days.first().unwrap().day, 36);
        assert_eq!(business.days.last().unwrap().day, 400);

        assert!(history
            .archive(settlement_entity, SettlementId(18), "Elsewhere")
            .businesses
            .is_empty());

        history.prune_stale_businesses(400 + SETTLEMENT_HISTORY_DAYS as u32);
        assert_eq!(history.business_days.len(), 1);
        history.prune_stale_businesses(401 + SETTLEMENT_HISTORY_DAYS as u32);
        assert!(
            history.business_days.is_empty(),
            "a firm remains inspectable for one year after closure, not forever"
        );
    }
}
