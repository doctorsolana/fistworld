//! Bounded synthetic ledger histories for the real history UI.

use bevy::prelude::*;

pub(super) fn synthetic_settlement_history(
    name: &str,
) -> shared::economy::SettlementHistoryArchive {
    use shared::economy::{Good, MarketGoodHistoryDay, SettlementHistoryDay};

    let mut days = Vec::with_capacity(shared::economy::SETTLEMENT_HISTORY_DAYS);
    for day in 1..=shared::economy::SETTLEMENT_HISTORY_DAYS as u32 {
        let mut market = [MarketGoodHistoryDay::default(); Good::COUNT];
        let mut physical_stock = [0u32; Good::COUNT];
        for good in Good::ALL {
            let wave =
                ((day as f32 * 0.071 + good.index() as f32).sin() * 0.18 + 1.0).clamp(0.6, 1.4);
            let midpoint = (good.base_price() as f32 * wave) as u64;
            let producer_units = if (day + good.index() as u32).is_multiple_of(3) {
                0
            } else {
                2 + u64::from(day % 5)
            };
            let consumer_units = 1 + u64::from((day + good.index() as u32) % 4);
            let producer_price = midpoint.saturating_mul(92) / 100;
            let consumer_price = midpoint.saturating_mul(108).div_ceil(100);
            let stock = 5 + ((day * (good.index() as u32 + 2)) % 24);
            let target = match good {
                Good::Food | Good::Flour | Good::Bread | Good::Meat => 14,
                Good::Wheat => 10,
                Good::Wood => 20,
                Good::Stone | Good::Iron | Good::Wool => 5,
            };
            market[good.index()] = MarketGoodHistoryDay {
                opening_bid: producer_price.saturating_sub(3),
                opening_ask: consumer_price.saturating_sub(2),
                closing_bid: producer_price,
                closing_ask: consumer_price,
                high_bid: producer_price.saturating_add(8),
                low_bid: producer_price.saturating_sub(9),
                high_ask: consumer_price.saturating_add(10),
                low_ask: consumer_price.saturating_sub(7),
                producer_units,
                producer_coin: producer_price.saturating_mul(producer_units),
                consumer_units,
                consumer_coin: consumer_price.saturating_mul(consumer_units),
                unavailable_units: u64::from((day + good.index() as u32) % 3),
                unaffordable_units: u64::from((day + good.index() as u32 + 1) % 2),
                funded_unmet_units: u64::from((day + good.index() as u32) % 2),
                closing_stock: stock,
                target_stock: target,
                listed_units: stock.saturating_sub(2),
            };
            physical_stock[good.index()] = stock + 3;
        }
        let population = 3 + day / 38;
        let employed = population.saturating_sub(if day % 47 < 8 { 2 } else { 1 });
        let hungry = u32::from(day % 53 < 5);
        let prosperity =
            (55.0 + day as f32 * 0.085 + (day as f32 * 0.12).sin() * 8.0).clamp(0.0, 100.0);
        let resident_wallets = u64::from(population) * (900 + u64::from(day) * 4);
        let business_cash = 8_000 + u64::from(day) * 27;
        let household_cash = u64::from(population) * 160;
        let treasury = 2_000 + u64::from(day) * 12;
        let liquidation = Good::ALL
            .into_iter()
            .map(|good| u64::from(physical_stock[good.index()]) * market[good.index()].closing_bid)
            .sum();
        days.push(SettlementHistoryDay {
            day,
            market,
            civic_treasury: treasury,
            resident_wallet_money: resident_wallets,
            household_cash,
            business_cash,
            business_wage_arrears: if day % 29 == 0 { 150 } else { 0 },
            business_tax_arrears: if day % 37 == 0 { 80 } else { 0 },
            civic_wage_arrears: if day % 11 == 0 { 125 } else { 0 },
            civic: shared::economy::CivicHistoryDay::default(),
            physical_stock,
            stock_liquidation_value: liquidation,
            total_local_coin: treasury + resident_wallets + household_cash + business_cash,
            population,
            employed,
            hungry,
            job_seekers: population.saturating_sub(employed),
            homeless: u32::from(day % 71 < 4),
            unpaid_workers: u32::from(day % 29 == 0),
            unrest: (18.0 + (day as f32 * 0.07).sin() * 10.0).clamp(0.0, 100.0),
            unrest_target: (20.0 + (day as f32 * 0.05).sin() * 12.0).clamp(0.0, 100.0),
            food_reserves: physical_stock[Good::Food.index()]
                + physical_stock[Good::Flour.index()]
                + physical_stock[Good::Bread.index()],
            purchasable_food: market[Good::Food.index()].listed_units
                + market[Good::Flour.index()].listed_units
                + market[Good::Bread.index()].listed_units,
            unlisted_business_food: day % 9,
            food_produced: 3 + day % 7,
            food_consumed: population,
            buildings: (4 + day / 55) as u16,
            productive_buildings: (2 + day / 100) as u16,
            work_positions: (4 + day / 45) as u16,
            filled_jobs: employed.min(u16::MAX as u32) as u16,
            prosperity,
            reserve_prosperity: (prosperity * 0.38).min(40.0),
            production_prosperity: (prosperity * 0.29).min(30.0),
            housing_prosperity: (prosperity * 0.2).min(20.0),
            employment_prosperity: (prosperity * 0.1).min(10.0),
            hunger_penalty: -(hungry as f32 * 4.0),
        });
    }
    shared::economy::SettlementHistoryArchive {
        settlement: name.to_string(),
        days,
        businesses: vec![
            synthetic_business_history(
                shared::components::BuildingId(100),
                shared::components::SettlementBuildingKind::Farmstead,
                shared::economy::Good::Wheat,
                shared::names::person_name(7_000),
            ),
            synthetic_business_history(
                shared::components::BuildingId(101),
                shared::components::SettlementBuildingKind::LumberjackHut,
                shared::economy::Good::Wood,
                shared::names::person_name(7_001),
            ),
        ],
    }
}

pub(super) fn synthetic_business_history(
    id: shared::components::BuildingId,
    kind: shared::components::SettlementBuildingKind,
    output: shared::economy::Good,
    owner: String,
) -> shared::economy::BusinessHistoryArchive {
    use shared::economy::{BusinessHistoryDay, BusinessState, BusinessStrategy, Good};

    let days = (1..=shared::economy::SETTLEMENT_HISTORY_DAYS as u32)
        .map(|day| {
            let produced = 2 + (day % 5);
            let sold = produced.saturating_sub(u32::from(day % 7 == 0));
            let asking = output.base_price().saturating_mul(90 + u64::from(day % 24)) / 100;
            let revenue = asking.saturating_mul(u64::from(sold));
            let wages = 100 + u64::from(day % 3) * 25;
            let fees = revenue * 5 / 100;
            let levy = revenue.saturating_sub(wages + fees) / 10;
            let costs = wages.saturating_add(fees).saturating_add(levy);
            let mut stock = [0; Good::COUNT];
            stock[output.index()] = 3 + day % 14;
            BusinessHistoryDay {
                day,
                observed: true,
                cash: 1_500 + u64::from(day) * 9,
                protected_working_capital: 600,
                withdrawable_profit: 300 + u64::from(day),
                wage_arrears: if day % 61 == 0 { 100 } else { 0 },
                tax_arrears: if day % 79 == 0 { 75 } else { 0 },
                gross_revenue: revenue,
                internal_revenue: if matches!(
                    kind,
                    shared::components::SettlementBuildingKind::Windmill
                        | shared::components::SettlementBuildingKind::Bakery
                ) {
                    revenue / 6
                } else {
                    0
                },
                wage_expense: wages,
                input_expense: 0,
                internal_input_expense: 0,
                market_fees: fees,
                delivery_fees: 0,
                profit_taxes: levy,
                owner_withdrawals: if day % 4 == 0 { 125 } else { 0 },
                capital_expenditures: if day == 1 { 450 } else { 0 },
                book_value: 450,
                profit: revenue as i64 - costs as i64,
                produced_units: produced,
                sold_units: sold,
                purchased_input_units: 0,
                workplace_stock: stock,
                listed_output_units: stock[output.index()].saturating_sub(2),
                asking_unit_price: asking,
                daily_wage: wages,
                strategy: BusinessStrategy::Balanced,
                autopilot: true,
                state: if day % 61 == 0 {
                    BusinessState::CashTight
                } else {
                    BusinessState::Operating
                },
            }
        })
        .collect();

    shared::economy::BusinessHistoryArchive {
        id,
        settlement: shared::components::SettlementId::UNASSIGNED,
        company_id: Some(shared::components::CompanyId(100 + id.0)),
        kind,
        owner_id: None,
        owner_name: Some(owner),
        output_good: Some(output),
        days,
    }
}

pub(super) fn synthetic_company_history(
    company: shared::components::CompanyId,
) -> shared::economy::CompanyHistoryArchive {
    let mut farm = synthetic_business_history(
        shared::components::BuildingId(601),
        shared::components::SettlementBuildingKind::Farmstead,
        shared::economy::Good::Wheat,
        "Aldric".to_string(),
    );
    farm.company_id = Some(company);
    farm.settlement = shared::components::SettlementId(41);
    let mut mill = synthetic_business_history(
        shared::components::BuildingId(602),
        shared::components::SettlementBuildingKind::Windmill,
        shared::economy::Good::Flour,
        "Aldric".to_string(),
    );
    mill.company_id = Some(company);
    mill.settlement = shared::components::SettlementId(41);
    let mut bakery = synthetic_business_history(
        shared::components::BuildingId(603),
        shared::components::SettlementBuildingKind::Bakery,
        shared::economy::Good::Bread,
        "Aldric".to_string(),
    );
    bakery.company_id = Some(company);
    bakery.settlement = shared::components::SettlementId(52);
    shared::economy::CompanyHistoryArchive {
        company,
        businesses: vec![farm, mill, bakery],
    }
}

pub(super) fn synthetic_world_history() -> shared::economy::WorldHistoryArchive {
    use shared::economy::{Good, WorldHistoryDay};

    let days = (1..=shared::economy::SETTLEMENT_HISTORY_DAYS as u32)
        .map(|day| {
            let settlements = 2 + day / 90;
            let population = 18 + day / 8 + (day / 70) * 5;
            let employed = population.saturating_sub(3 + day % 4);
            let hungry = if day % 61 < 8 { 2 + day % 3 } else { day % 2 };
            let mut physical_stock = [0u32; Good::COUNT];
            for good in Good::ALL {
                physical_stock[good.index()] = 15 + day / 5 + good.index() as u32 * 9 + day % 13;
            }
            let business_cash = 28_000 + u64::from(day) * 37;
            let household_cash = u64::from(population) * 175;
            let wallets = u64::from(population) * (850 + u64::from(day) * 3);
            let treasury = u64::from(settlements) * 2_500 + u64::from(day) * 18;
            WorldHistoryDay {
                day,
                settlements,
                population,
                employed,
                hungry,
                civic_treasury: treasury,
                resident_wallet_money: wallets,
                household_cash,
                business_cash,
                business_wage_arrears: if day % 29 == 0 { 300 } else { 0 },
                business_tax_arrears: if day % 41 == 0 { 175 } else { 0 },
                civic_wage_arrears: if day % 17 == 0 { 220 } else { 0 },
                total_local_coin: treasury + business_cash + household_cash + wallets,
                stock_liquidation_value: 18_000 + u64::from(day) * 91,
                physical_stock,
                food_reserves: physical_stock[Good::Food.index()]
                    + physical_stock[Good::Flour.index()]
                    + physical_stock[Good::Bread.index()],
                purchasable_food: physical_stock[Good::Food.index()]
                    + physical_stock[Good::Bread.index()],
                unlisted_business_food: day % 13,
                food_produced: population + 5 + day % 12,
                food_consumed: population.saturating_sub(hungry),
                buildings: 8 + day / 17,
                productive_buildings: 4 + day / 43,
                prosperity: (48.0 + day as f32 * 0.1 + (day as f32 * 0.085).sin() * 6.0)
                    .clamp(0.0, 100.0),
            }
        })
        .collect();
    shared::economy::WorldHistoryArchive { days }
}
