//! Daily assignment of observed market shortfalls to capable suppliers.
//!
//! Existing staff share the signal. Closed competitors do not each receive a
//! tiny fraction which makes every restart unprofitable: one viable additional
//! supplier may respond when their combined capacity cannot cover the order.
//! This is a forecast, not reserved sales, funding, jobs or manufactured stock.

use super::super::*;
use shared::components::{BuildingId, SettlementId};
use shared::economy::sustainable_unit_price;

#[derive(Clone, Copy)]
pub(crate) struct MarketResponseSite {
    pub id: BuildingId,
    pub settlement: SettlementId,
    pub kind: SettlementBuildingKind,
    pub quality: f32,
    pub state: BusinessState,
    pub assigned_workers: u8,
    pub daily_wage: u64,
    pub sale: BusinessSalePolicy,
    pub autopilot: bool,
}

impl MarketResponseSite {
    /// Quote the actual allocated batch, including the whole daily wage that
    /// payroll charges. A buyer's observed ceiling must cover its cost/margin.
    pub fn funded_restart_quote(
        &self,
        market: &MootMarket,
        share: MarketResponseShare,
    ) -> Option<(u64, u64)> {
        if share.participants == 0 {
            return None;
        }
        let capacity = rated_daily_production(self.kind, self.quality)?;
        let worker_capacity = capacity.output_units / u32::from(self.kind.positions().max(1));
        let pool = market.pool(capacity.output);
        [pool.day, pool.previous_day]
            .into_iter()
            .filter_map(|flow| {
                let mut units = share
                    .units(flow.funded_unmet_units)
                    .min(u64::from(worker_capacity)) as u32;
                let input_cost = if let Some(recipe) = processing_recipe(self.kind) {
                    let cycles = units / recipe.output_units.max(1);
                    units = cycles.saturating_mul(recipe.output_units);
                    u64::from(cycles)
                        .saturating_mul(u64::from(recipe.input_units))
                        .saturating_mul(market.suggested_price(recipe.input))
                } else {
                    0
                };
                if units == 0 {
                    return None;
                }
                let cost = self.daily_wage.saturating_add(input_cost);
                let mut quote = sustainable_unit_price(
                    cost.div_ceil(u64::from(units)),
                    market.market_fee_bps(),
                    self.sale.target_margin_bps,
                )
                .max(self.sale.minimum_unit_price);
                let net = quote.saturating_mul(
                    BASIS_POINTS.saturating_sub(u64::from(market.market_fee_bps())),
                ) / BASIS_POINTS;
                if net.saturating_mul(u64::from(units)) <= cost {
                    quote = quote.saturating_add(1);
                }
                if !self.sale.automatic_pricing {
                    let fixed = self
                        .sale
                        .asking_unit_price
                        .max(self.sale.minimum_unit_price);
                    let fixed_net = fixed.saturating_mul(
                        BASIS_POINTS.saturating_sub(u64::from(market.market_fee_bps())),
                    ) / BASIS_POINTS;
                    if fixed_net.saturating_mul(u64::from(units)) <= cost {
                        return None;
                    }
                    quote = fixed;
                }
                (flow.funded_unmet_at(quote) > 0).then_some((quote, flow.funded_unmet_unit_price))
            })
            .min()
    }
}

/// Exact, stable remainder allocation. Several firms may forecast one order,
/// but the sum of their shares never exceeds its recorded quantity.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct MarketResponseShare {
    rank: usize,
    participants: usize,
}

impl MarketResponseShare {
    pub const SOLO: Self = Self {
        rank: 0,
        participants: 1,
    };

    pub fn units(self, total: u64) -> u64 {
        if self.participants == 0 {
            return 0;
        }
        let count = self.participants as u64;
        total / count + u64::from((self.rank as u64) < total % count)
    }
}

#[derive(Default)]
pub(crate) struct MarketResponses {
    shares: HashMap<BuildingId, MarketResponseShare>,
    quotes: HashMap<BuildingId, (u64, u64)>,
}

impl MarketResponses {
    pub fn share_for(&self, site: BuildingId) -> MarketResponseShare {
        self.shares.get(&site).copied().unwrap_or_default()
    }

    pub fn restart_quote_for(&self, site: BuildingId) -> Option<(u64, u64)> {
        self.quotes.get(&site).copied()
    }

    pub fn build<'a>(
        sites: impl IntoIterator<Item = MarketResponseSite>,
        market_for: impl Fn(SettlementId) -> Option<&'a MootMarket>,
    ) -> Self {
        let mut groups = HashMap::<(SettlementId, Good), Vec<MarketResponseSite>>::new();
        for site in sites {
            if !site.sale.collection_enabled
                || !(site.state.accepts_new_workers() || site.state == BusinessState::Mothballed)
            {
                continue;
            }
            if let Some(capacity) = rated_daily_production(site.kind, site.quality) {
                groups
                    .entry((site.settlement, capacity.output))
                    .or_default()
                    .push(site);
            }
        }
        let mut result = Self::default();
        for ((settlement, good), mut sites) in groups {
            let Some(market) = market_for(settlement) else {
                continue;
            };
            sites.sort_unstable_by_key(|site| site.id);
            let pool = market.pool(good);
            let funded = pool
                .day
                .funded_unmet_units
                .max(pool.previous_day.funded_unmet_units);
            // Empty advertised jobs do not constitute supply. Nor can an
            // unaffordable fixed ask block a cheaper willing competitor.
            let active = |site: &&MarketResponseSite| {
                let affordable = if site.autopilot && site.sale.automatic_pricing {
                    site.funded_restart_quote(market, MarketResponseShare::SOLO)
                        .is_some()
                } else {
                    let price = site
                        .sale
                        .asking_unit_price
                        .max(site.sale.minimum_unit_price)
                        .max(1);
                    pool.day
                        .funded_unmet_at(price)
                        .max(pool.previous_day.funded_unmet_at(price))
                        > 0
                };
                site.assigned_workers > 0
                    && site.state != BusinessState::Mothballed
                    && (funded == 0 || affordable)
            };
            let active_sites: Vec<_> = sites.iter().filter(active).collect();
            let active_count = active_sites.len();
            let active_capacity = active_sites.iter().fold(0u64, |total, site| {
                let capacity = rated_daily_production(site.kind, site.quality).expect("rated site");
                total.saturating_add(
                    u64::from(capacity.output_units)
                        * u64::from(site.assigned_workers.min(site.kind.positions()))
                        / u64::from(site.kind.positions().max(1)),
                )
            });
            let candidate = if funded > active_capacity {
                sites
                    .iter()
                    .filter(|site| !active(site))
                    .filter_map(|site| {
                        if !site.autopilot {
                            return None;
                        }
                        let share = MarketResponseShare {
                            rank: active_sites.partition_point(|active| active.id < site.id),
                            participants: active_count + 1,
                        };
                        let (quote, _) = site.funded_restart_quote(market, share)?;
                        Some((quote, site.id))
                    })
                    .min()
                    .map(|(_, id)| id)
            } else {
                None
            };
            let participants = active_count + usize::from(candidate.is_some());
            for (rank, site) in sites
                .iter()
                .filter(|site| active(site) || candidate == Some(site.id))
                .enumerate()
            {
                let share = MarketResponseShare { rank, participants };
                result.shares.insert(site.id, share);
                if let Some(quote) = site.funded_restart_quote(market, share) {
                    result.quotes.insert(site.id, quote);
                }
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lumber(id: u64, wage: u64) -> MarketResponseSite {
        MarketResponseSite {
            id: BuildingId(id),
            settlement: SettlementId(1),
            kind: SettlementBuildingKind::LumberjackHut,
            quality: 0.25,
            state: BusinessState::Mothballed,
            assigned_workers: 0,
            daily_wage: wage,
            sale: BusinessSalePolicy::for_good(Good::Wood),
            autopilot: true,
        }
    }

    #[test]
    fn five_idle_suppliers_do_not_fragment_one_viable_order() {
        let mut market = MootMarket::founding();
        market.record_unmet_demand(Good::Wood, 6, 0, 6, 50);
        for sites in [
            (1..=5).map(|id| lumber(id, 100)).collect::<Vec<_>>(),
            (1..=5).rev().map(|id| lumber(id, 100)).collect(),
        ] {
            let allocation = MarketResponses::build(sites, |_| Some(&market));
            assert_eq!(allocation.shares.len(), 1);
            assert_eq!(allocation.share_for(BuildingId(1)).participants, 1);
        }
    }

    #[test]
    fn idle_response_chooses_a_cheaper_viable_offer_and_rejects_unfunded_work() {
        let mut market = MootMarket::founding();
        market.record_unmet_demand(Good::Wood, 6, 0, 0, 50);
        let sites = [lumber(1, 100), lumber(2, 80), lumber(3, 400)];
        assert!(MarketResponses::build(sites, |_| Some(&market))
            .shares
            .is_empty());
        market.record_unmet_demand(Good::Wood, 0, 0, 6, 50);
        let result = MarketResponses::build(sites, |_| Some(&market));
        assert_eq!(result.share_for(BuildingId(2)).participants, 1);
        assert_eq!(result.shares.len(), 1);

        let mut cheap_buyer = MootMarket::founding();
        cheap_buyer.record_unmet_demand(Good::Wood, 6, 0, 6, 4);
        assert!(MarketResponses::build(sites, |_| Some(&cheap_buyer))
            .shares
            .is_empty());
    }

    #[test]
    fn existing_capacity_serves_small_orders_and_admits_one_entrant_for_overflow() {
        let mut market = MootMarket::founding();
        market.record_unmet_demand(Good::Wood, 6, 0, 6, 50);
        let mut active = lumber(5, 100);
        active.state = BusinessState::Operating;
        active.assigned_workers = 1;
        let sites = [lumber(1, 80), lumber(2, 100), active];
        let result = MarketResponses::build(sites, |_| Some(&market));
        assert_eq!(result.share_for(BuildingId(5)).participants, 1);
        assert_eq!(result.shares.len(), 1);
        market.record_unmet_demand(Good::Wood, 500, 0, 500, 50);
        let result = MarketResponses::build(sites, |_| Some(&market));
        assert_eq!(result.share_for(BuildingId(5)).participants, 2);
        assert_eq!(result.share_for(BuildingId(1)).participants, 2);
        assert_eq!(result.shares.len(), 2);
    }

    #[test]
    fn exact_shares_do_not_multiply_small_orders_and_are_stable() {
        let mut market = MootMarket::founding();
        market.record_unmet_demand(Good::Wood, 6, 0, 6, 50);
        for reverse in [false, true] {
            let mut sites: Vec<_> = (1..=5)
                .map(|id| {
                    let mut site = lumber(id, 50);
                    site.state = BusinessState::Operating;
                    site.assigned_workers = 1;
                    site
                })
                .collect();
            if reverse {
                sites.reverse();
            }
            let result = MarketResponses::build(sites, |_| Some(&market));
            let shares: Vec<_> = (1..=5)
                .map(|id| result.share_for(BuildingId(id)).units(6))
                .collect();
            assert_eq!(shares, [2, 1, 1, 1, 1]);
            assert_eq!(shares.iter().sum::<u64>(), 6);
            for units in 0..=30 {
                assert_eq!(
                    (1..=5)
                        .map(|id| result.share_for(BuildingId(id)).units(units))
                        .sum::<u64>(),
                    units
                );
            }
        }
    }

    #[test]
    fn unaffordable_incumbent_cannot_block_a_willing_cheaper_supplier() {
        let mut market = MootMarket::founding();
        market.record_unmet_demand(Good::Wood, 6, 0, 6, 50);
        let mut incumbent = lumber(1, 100);
        incumbent.state = BusinessState::Operating;
        incumbent.assigned_workers = 1;
        incumbent.autopilot = false;
        incumbent.sale.automatic_pricing = false;
        incumbent.sale.asking_unit_price = 1_000;
        let result = MarketResponses::build([incumbent, lumber(2, 80)], |_| Some(&market));
        assert_eq!(result.share_for(BuildingId(1)).units(6), 0);
        assert_eq!(result.share_for(BuildingId(2)).units(6), 6);
    }

    #[test]
    fn affordable_fixed_price_can_restart_without_repricing() {
        let mut market = MootMarket::founding();
        market.record_unmet_demand(Good::Wood, 6, 0, 6, 50);
        let mut site = lumber(1, 80);
        site.sale.automatic_pricing = false;
        site.sale.asking_unit_price = 15;
        let result = MarketResponses::build([site], |_| Some(&market));
        assert_eq!(result.share_for(site.id).units(6), 6);
        assert_eq!(result.restart_quote_for(site.id), Some((15, 50)));
        site.sale.asking_unit_price = 4;
        assert!(MarketResponses::build([site], |_| Some(&market))
            .shares
            .is_empty());
    }
}
