//! Daily, read-only investment evidence. Price ladders are indexed once per
//! settlement; evaluating another property never rescans its market listings.

use shared::components::SettlementBuildingKind;
use shared::economy::{
    sustainable_unit_price, Good, GoodsInventory, MarketDayFlow, MootMarket, BASIS_POINTS,
    DEFAULT_TARGET_MARGIN_BPS,
};

#[derive(Clone, Copy)]
struct SupplyStep {
    price: u64,
    units: u32,
    pennies: u64,
}

#[cfg(test)]
#[path = "investment_tests.rs"]
mod tests;

pub(crate) struct InvestmentMarket {
    supply: [Vec<SupplyStep>; Good::COUNT],
    current: [MarketDayFlow; Good::COUNT],
    previous: [MarketDayFlow; Good::COUNT],
    quotes: [u64; Good::COUNT],
    fee_bps: u16,
    claimed_output: [u32; Good::COUNT],
    hiring_wage: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RestartPlan {
    pub output_units: u32,
    pub daily_profit: u64,
    pub working_cash: u64,
    pub asking_price: u64,
}

impl InvestmentMarket {
    pub(crate) fn new(market: &MootMarket) -> Self {
        let mut supply: [Vec<SupplyStep>; Good::COUNT] = std::array::from_fn(|_| Vec::new());
        // MootMarket keeps listings sorted by price. Each good's subsequence
        // therefore forms a cumulative cheapest-fill curve without sorting.
        for listing in market.listings() {
            if listing.units == 0 || !market.can_trade(listing.good) {
                continue;
            }
            let ladder = &mut supply[listing.good.index()];
            let (units, pennies) = ladder
                .last()
                .map_or((0, 0), |step| (step.units, step.pennies));
            ladder.push(SupplyStep {
                price: listing.unit_price,
                units: units.saturating_add(listing.units),
                pennies: pennies
                    .saturating_add(u64::from(listing.units).saturating_mul(listing.unit_price)),
            });
        }
        Self {
            supply,
            current: std::array::from_fn(|i| market.pool(Good::ALL[i]).day),
            previous: std::array::from_fn(|i| market.pool(Good::ALL[i]).previous_day),
            quotes: std::array::from_fn(|i| market.suggested_price(Good::ALL[i]).max(1)),
            fee_bps: market.market_fee_bps(),
            claimed_output: [0; Good::COUNT],
            hiring_wage: shared::economy::FOUNDING_DAILY_WAGE,
        }
    }

    pub(crate) fn set_hiring_wage(&mut self, wage: u64) {
        self.hiring_wage = wage.max(shared::economy::MINIMUM_BUSINESS_DAILY_WAGE);
    }

    pub(crate) fn hiring_wage(&self) -> u64 {
        self.hiring_wage
    }

    pub(crate) fn available(&self, good: Good) -> u32 {
        self.supply[good.index()]
            .last()
            .map_or(0, |step| step.units)
    }

    fn available_at(&self, good: Good, quote: u64) -> u32 {
        let ladder = &self.supply[good.index()];
        let end = ladder.partition_point(|step| step.price <= quote);
        end.checked_sub(1).map_or(0, |i| ladder[i].units)
    }

    pub(crate) fn purchase_cost(&self, good: Good, units: u32) -> Option<u64> {
        if units == 0 {
            return Some(0);
        }
        let ladder = &self.supply[good.index()];
        let i = ladder.partition_point(|step| step.units < units);
        let step = ladder.get(i)?;
        let (prior_units, prior_cost) = i
            .checked_sub(1)
            .map_or((0, 0), |i| (ladder[i].units, ladder[i].pennies));
        Some(prior_cost.saturating_add(u64::from(units - prior_units).saturating_mul(step.price)))
    }

    pub(crate) fn personal_reserve(&self) -> u64 {
        let meal = Good::ALL
            .into_iter()
            .filter(|good| good.is_edible())
            .filter(|good| self.available(*good) > 0)
            .map(|good| self.quotes[good.index()])
            .min()
            .unwrap_or(100);
        meal.saturating_mul(3)
            .saturating_add(self.quotes[Good::Wood.index()])
            .max(2 * shared::economy::PENNIES_PER_COIN)
    }

    pub(crate) fn reserve_restart(&mut self, kind: SettlementBuildingKind, plan: RestartPlan) {
        if let Some(capacity) = super::super::rated_daily_production(kind, 1.0) {
            self.claimed_output[capacity.output.index()] =
                self.claimed_output[capacity.output.index()].saturating_add(plan.output_units);
        }
    }

    /// A complete profitable shift backed by observed buyers and physical
    /// inputs. Unpriced hunger and a high advertised ask are not sales.
    /// Owned stock is costed at replacement value, so inheritance cannot make
    /// a structurally loss-making processor look profitable.
    pub(crate) fn restart_plan(
        &self,
        kind: SettlementBuildingKind,
        quality: f32,
        daily_wage: u64,
        own_sales: u32,
        inventory: Option<&GoodsInventory>,
        fixed_price: Option<u64>,
    ) -> Option<RestartPlan> {
        let capacity = super::super::rated_daily_production(kind, quality)?;
        let output = capacity.output;
        let current = self.current[output.index()];
        let previous = self.previous[output.index()];
        let own_stock = inventory.map_or(0, |stock| stock.amount(output));
        let mut best = RestartPlan::default();
        for workers in 1..=kind.positions() {
            let rated = capacity.output_units.saturating_mul(u32::from(workers))
                / u32::from(kind.positions().max(1));
            let wage = daily_wage.saturating_mul(u64::from(workers));
            let input_unit_cost = capacity.input.map_or(0, |(input, units)| {
                u64::from(units)
                    .saturating_mul(self.quotes[input.index()])
                    .div_ceil(u64::from(capacity.output_units.max(1)))
            });
            let viable_quote = sustainable_unit_price(
                wage.div_ceil(u64::from(rated.max(1)))
                    .saturating_add(input_unit_cost),
                self.fee_bps,
                DEFAULT_TARGET_MARGIN_BPS,
            );
            let historic_quote = |flow: MarketDayFlow| {
                if flow.consumer_units > 0 {
                    flow.consumer_coin / flow.consumer_units
                } else {
                    0
                }
            };
            let ordinary_quotes = [
                self.quotes[output.index()],
                viable_quote,
                historic_quote(current),
                historic_quote(previous),
                fixed_price.unwrap_or_default(),
            ];
            for quote in ordinary_quotes
                .into_iter()
                .chain(current.funded_demand.price_levels())
                .chain(previous.funded_demand.price_levels())
                .filter(|price| *price > 0 && fixed_price.is_none_or(|fixed| fixed == *price))
            {
                let funded = current
                    .funded_unmet_at(quote)
                    .max(previous.funded_unmet_at(quote))
                    .min(u64::from(u32::MAX)) as u32;
                let demonstrated = own_sales
                    .saturating_add(funded)
                    .saturating_sub(self.claimed_output[output.index()]);
                let mut units = rated.min(
                    demonstrated
                        .saturating_sub(self.available_at(output, quote))
                        .saturating_sub(own_stock),
                );
                let mut input_cost = 0;
                if let Some(recipe) = super::super::processing_recipe(kind) {
                    let held = inventory.map_or(0, |stock| stock.amount(recipe.input));
                    units = units.min(
                        held.saturating_add(self.available(recipe.input)) / recipe.input_units
                            * recipe.output_units,
                    );
                    units = units / recipe.output_units * recipe.output_units;
                    let input_units = units / recipe.output_units * recipe.input_units;
                    let purchased = input_units.saturating_sub(held);
                    input_cost = self.purchase_cost(recipe.input, purchased)?.saturating_add(
                        u64::from(input_units.min(held))
                            .saturating_mul(self.quotes[recipe.input.index()]),
                    );
                }
                let revenue = u64::from(units)
                    .saturating_mul(quote)
                    .saturating_mul(BASIS_POINTS - u64::from(self.fee_bps))
                    / BASIS_POINTS;
                let profit = revenue.saturating_sub(wage.saturating_add(input_cost));
                if profit > best.daily_profit {
                    best = RestartPlan {
                        output_units: units,
                        daily_profit: profit,
                        working_cash: wage.saturating_add(input_cost).saturating_mul(2),
                        asking_price: quote,
                    };
                }
            }
        }
        (best.daily_profit > 0).then_some(best)
    }
}
