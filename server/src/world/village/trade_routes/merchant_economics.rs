//! Bounded bid-aware valuation and markdowns for NPC merchant stock.

use super::*;

impl CompanyTradeObservation {
    fn funded_at(self, price: u64) -> u64 {
        self.funded_demand
            .units_at(price)
            .max(self.substitute_demand.units_at(price).div_ceil(4))
    }
}

pub(super) fn merchant_journey_cost(daily_wage: u64, distance: f32) -> u64 {
    daily_wage.saturating_add(
        ((distance.max(0.0) * 2.0 / 100.0).ceil() as u64)
            .saturating_mul(JOURNEY_PENNIES_PER_100_METRES),
    )
}

/// There are at most two bounded bid curves and two observed price references.
/// Evaluate their breakpoints rather than inventing revenue from an empty shelf.
pub(super) fn evaluate_merchant_opportunity(
    source: CompanyTradeObservation,
    destination: CompanyTradeObservation,
    source_position: Vec3,
    destination_position: Vec3,
    strategy: shared::economy::BusinessStrategy,
    daily_wage: u64,
    incoming_units: u32,
) -> Option<MerchantOpportunity> {
    if source.settlement == destination.settlement
        || source.good != destination.good
        || source.listed_units == 0
    {
        return None;
    }
    let (minimum_return_bps, minimum_confidence, minimum_profit) = strategy_trade_terms(strategy);
    let confidence = source.confidence.min(destination.confidence);
    if confidence < minimum_confidence {
        return None;
    }
    let distance = Vec2::new(
        destination_position.x - source_position.x,
        destination_position.z - source_position.z,
    )
    .length();
    let journey_cost = merchant_journey_cost(daily_wage, distance);
    let unit_capacity = shared::economy::capacity::PORTER / source.good.bulk_per_unit().max(1);
    let competing_price =
        (destination.listed_units > 0).then_some(destination.asking_price.saturating_sub(1).max(1));
    let candidates = competing_price
        .into_iter()
        .chain((destination.recent_units_sold > 0).then_some(destination.recent_sale_price))
        .chain(destination.funded_demand.price_levels())
        .chain(destination.substitute_demand.price_levels());
    let mut best: Option<MerchantOpportunity> = None;
    for candidate in candidates {
        let sale_price = competing_price.map_or(candidate, |ceiling| candidate.min(ceiling));
        if sale_price == 0 || sale_price <= source.asking_price {
            continue;
        }
        // Completed sales only support another modest trial at a price those
        // buyers actually paid. A funded shortfall is valued at its own bid.
        let repeat_sales = if sale_price <= destination.recent_sale_price {
            destination.recent_units_sold.div_ceil(2)
        } else {
            0
        };
        let demand = destination
            .funded_at(sale_price)
            .saturating_add(u64::from(repeat_sales))
            .saturating_sub(u64::from(incoming_units));
        let cargo_units = source
            .listed_units
            .min(demand.min(u64::from(u32::MAX)) as u32)
            .min(unit_capacity);
        if cargo_units == 0 {
            continue;
        }
        let revenue = sale_price.saturating_mul(u64::from(cargo_units));
        let destination_fee = revenue
            .saturating_mul(u64::from(destination.market_fee_bps))
            .div_ceil(BASIS_POINTS);
        let economics = |purchase_price: u64| {
            let purchase = purchase_price.saturating_mul(u64::from(cargo_units));
            let uncertainty =
                purchase.saturating_mul(u64::from(100_u8.saturating_sub(confidence))) / 500;
            let cost = purchase
                .saturating_add(destination_fee)
                .saturating_add(journey_cost)
                .saturating_add(uncertainty);
            let profit = i128::from(revenue) - i128::from(cost);
            let return_bps = if profit <= 0 {
                0
            } else {
                (profit as u128).saturating_mul(u128::from(BASIS_POINTS))
                    / u128::from(purchase.max(1))
            };
            (profit, return_bps)
        };
        let acceptable = |profit: i128, return_bps: u128| {
            profit >= i128::from(minimum_profit) && return_bps >= u128::from(minimum_return_bps)
        };
        let (profit, return_bps) = economics(source.asking_price);
        if !acceptable(profit, return_bps) {
            continue;
        }
        let mut maximum_purchase_price = source.asking_price;
        let mut high = uncertain_purchase_limit(source.asking_price, confidence);
        while maximum_purchase_price < high {
            let candidate = maximum_purchase_price + (high - maximum_purchase_price).div_ceil(2);
            let (profit, returns) = economics(candidate);
            if acceptable(profit, returns) {
                maximum_purchase_price = candidate;
            } else {
                high = candidate.saturating_sub(1);
            }
        }
        let opportunity = MerchantOpportunity {
            origin: source.settlement,
            destination: destination.settlement,
            good: source.good,
            cargo_units,
            maximum_purchase_price,
            minimum_sale_price: sale_price,
            expected_profit: profit.min(i128::from(i64::MAX)) as i64,
            confidence,
            minimum_cargo_net: journey_cost.saturating_add(minimum_profit),
            freight_cost: journey_cost,
            minimum_return_bps,
            destination_fee_bps: destination.market_fee_bps,
        };
        if best.is_none_or(|current| {
            (
                opportunity.expected_profit,
                opportunity.cargo_units,
                std::cmp::Reverse(sale_price),
            ) > (
                current.expected_profit,
                current.cargo_units,
                std::cmp::Reverse(current.minimum_sale_price),
            )
        }) {
            best = Some(opportunity);
        }
    }
    best
}

/// Recover working capital from an old consignment. New shipments still have
/// to pass the separate profitable-dispatch test, so liquidation never turns
/// into an instruction to keep buying goods at a loss.
pub(super) fn reviewed_consignment_price(
    current: u64,
    competitor: Option<u64>,
    landed_unit_cost: u64,
    fee_bps: u16,
    age_days: u32,
) -> u64 {
    if age_days < 2 {
        return current.max(1);
    }
    let step = current.saturating_mul(1_000).div_ceil(BASIS_POINTS).max(1);
    let bounded = current.saturating_sub(step).max(1);
    let target = competitor
        .filter(|price| *price < current)
        .map_or(bounded, |price| price.saturating_sub(1).max(1));
    let floor = if age_days >= AUTONOMOUS_ROUTE_RETRY_DAYS {
        1
    } else {
        shared::economy::sustainable_unit_price(landed_unit_cost, fee_bps, 0)
    };
    // A revised estimate may not raise already stranded stock's price.
    bounded
        .max(target.min(current))
        .max(floor)
        .min(current)
        .max(1)
}

pub(super) fn viable_pickup(
    preview: shared::economy::MarketTrade,
    sale_price: u64,
    terms: &AutonomousMerchantRoute,
) -> bool {
    if preview.units == 0 {
        return false;
    }
    let revenue = sale_price.saturating_mul(u64::from(preview.units));
    let fee = revenue
        .saturating_mul(u64::from(terms.destination_fee_bps))
        .div_ceil(BASIS_POINTS);
    let Some(cargo_net) = revenue
        .checked_sub(fee)
        .and_then(|net| net.checked_sub(preview.pennies))
    else {
        return false;
    };
    // The absolute net requirement includes freight and minimum trip profit.
    // Preserve the original return requirement conservatively as well.
    cargo_net >= terms.minimum_cargo_net
        && u128::from(cargo_net.saturating_sub(terms.freight_cost)) * u128::from(BASIS_POINTS)
            >= u128::from(preview.pennies) * u128::from(terms.minimum_return_bps)
}

pub(super) fn review_merchant_consignment(
    route: &CompanyTradeRoute,
    autonomous: &mut AutonomousMerchantRoute,
    history: &TradeRouteHistory,
    market: &mut MootMarket,
    daily_wage: u64,
    cycle_duration: f32,
    distance: f32,
    day: u32,
    shared_with_manual_route: bool,
) -> bool {
    let seller = MarketSeller::Business(route.warehouse);
    let unsold = market.seller_listed_units(seller, route.good);
    if route.completed_trips > autonomous.last_completed_trips {
        let failed = history.trips().last().is_none_or(|trip| trip.units == 0);
        autonomous.disappointing_reviews = if failed {
            autonomous.disappointing_reviews.saturating_add(1)
        } else {
            0
        };
        autonomous.last_completed_trips = route.completed_trips;
    }
    if unsold == 0 {
        autonomous.unsold_since_day = None;
        return false;
    }
    let since = *autonomous.unsold_since_day.get_or_insert_with(|| {
        history
            .trips()
            .iter()
            .rev()
            .find(|trip| trip.units > 0)
            .map_or(day, |trip| trip.completed_day)
    });
    if shared_with_manual_route {
        return true;
    }
    let Some(trip) = history.trips().iter().rev().find(|trip| trip.units > 0) else {
        return true;
    };
    let Some(current) = market
        .listings()
        .iter()
        .filter(|listing| listing.seller == seller && listing.good == route.good)
        .map(|listing| listing.unit_price)
        .min()
    else {
        return true;
    };
    let wage_days = (f64::from(trip.travel_world_seconds) / f64::from(cycle_duration.max(1.0)))
        .ceil()
        .max(1.0) as u64;
    let landed_cost = trip
        .source_purchase_cost
        .saturating_add(merchant_journey_cost(
            daily_wage.saturating_mul(wage_days),
            distance,
        ))
        .div_ceil(u64::from(trip.units));
    let price = reviewed_consignment_price(
        current,
        market.best_competing_price(seller, route.good),
        landed_cost,
        market.market_fee_bps(),
        day.saturating_sub(since),
    );
    if price < current {
        market.reprice(seller, route.good, price);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(settlement: u64, ask: u64, stock: u32) -> CompanyTradeObservation {
        CompanyTradeObservation {
            settlement: SettlementId(settlement),
            good: Good::Bread,
            observed_day: 1,
            confidence: 100,
            asking_price: ask,
            listed_units: stock,
            recent_units_sold: 0,
            recent_sale_price: 0,
            funded_unmet_units: 0,
            funded_demand: default(),
            substitute_demand: default(),
            market_fee_bps: 500,
        }
    }

    fn evaluate(
        source: CompanyTradeObservation,
        destination: CompanyTradeObservation,
        wage: u64,
    ) -> Option<MerchantOpportunity> {
        evaluate_merchant_opportunity(
            source,
            destination,
            Vec3::ZERO,
            Vec3::new(40.0, 0.0, 0.0),
            shared::economy::BusinessStrategy::Balanced,
            wage,
            0,
        )
    }

    #[test]
    fn an_empty_shelf_does_not_turn_a_cheap_bid_into_a_base_price_premium() {
        let source = observation(1, 20, 10);
        let mut destination = observation(2, 1_000, 0);
        destination.funded_demand.add(10, 25);
        destination.funded_unmet_units = 10;
        assert!(evaluate(source, destination, 100).is_none());
        destination.funded_demand.add(10, 80);
        let trip = evaluate(source, destination, 100).unwrap();
        assert_eq!(trip.minimum_sale_price, 80);
        assert_eq!(trip.cargo_units, 10);
        assert!(trip.minimum_sale_price < Good::Bread.base_price());
    }

    #[test]
    fn a_rich_minor_order_cannot_price_the_entire_poor_order_at_its_bid() {
        let source = observation(1, 20, 144);
        let mut destination = observation(2, 200, 0);
        destination.funded_demand.add(100, 10);
        destination.funded_demand.add(4, 100);
        let trip = evaluate(source, destination, 100).unwrap();
        assert_eq!((trip.minimum_sale_price, trip.cargo_units), (100, 4));
        assert!(
            evaluate(source, destination, 500).is_none(),
            "the actual wage must enter freight economics"
        );
    }

    #[test]
    fn historical_purchases_support_a_trial_only_at_the_price_actually_paid() {
        let source = observation(1, 10, 50);
        let mut destination = observation(2, 1_000, 0);
        destination.recent_units_sold = 10;
        destination.recent_sale_price = 80;
        let trip = evaluate(source, destination, 100).unwrap();
        assert_eq!((trip.minimum_sale_price, trip.cargo_units), (80, 5));
    }

    #[test]
    fn old_stock_cuts_price_with_bounded_cost_protection_then_can_liquidate() {
        assert_eq!(reviewed_consignment_price(100, Some(60), 75, 500, 1), 100);
        assert_eq!(reviewed_consignment_price(100, Some(60), 75, 500, 2), 90);
        assert_eq!(reviewed_consignment_price(100, Some(150), 75, 500, 2), 90);
        assert_eq!(reviewed_consignment_price(80, Some(60), 75, 500, 3), 79);
        assert_eq!(reviewed_consignment_price(80, Some(60), 75, 500, 7), 72);
    }

    #[test]
    fn unsold_imports_reprice_but_manual_coowned_listings_are_untouched() {
        let route = CompanyTradeRoute {
            company: shared::components::CompanyId(1),
            warehouse: shared::components::BuildingId(1),
            mode: TradeRouteMode::Merchant,
            origin: SettlementId(1),
            destination: SettlementId(2),
            good: Good::Bread,
            cargo_target: 10,
            maximum_purchase_price: 20,
            minimum_destination_price: 100,
            automatic: false,
            autonomous_management: true,
            expected_trip_profit: 400,
            decision_confidence: 100,
            active_contract: None,
            assigned_caravaner: None,
            current_stop: 0,
            status: TradeRouteStatus::Idle,
            completed_trips: 1,
            lifetime_units: 10,
            lifetime_delivery_revenue: 0,
            lifetime_purchase_cost: 200,
            lifetime_consigned_value: 1_000,
        };
        let mut autonomous = AutonomousMerchantRoute {
            last_review_day: 0,
            mothballed_day: None,
            last_completed_trips: 0,
            disappointing_reviews: 0,
            expected_trip_profit: 400,
            confidence: 100,
            unsold_since_day: None,
            minimum_cargo_net: 200,
            freight_cost: 100,
            minimum_return_bps: 1_500,
            destination_fee_bps: 500,
        };
        let mut history = TradeRouteHistory::default();
        assert!(
            !viable_pickup(
                shared::economy::MarketTrade {
                    units: 2,
                    pennies: 40
                },
                100,
                &autonomous
            ),
            "a partial cart cannot recover the fixed freight and profit requirement"
        );
        assert!(viable_pickup(
            shared::economy::MarketTrade {
                units: 10,
                pennies: 200
            },
            100,
            &autonomous
        ));
        history.record(TradeRouteTrip {
            departed_day: 0,
            completed_day: 1,
            units: 10,
            source_purchase_cost: 200,
            source_market_fees: 10,
            delivery_revenue: 0,
            consigned_value: 1_000,
            stops_visited: 2,
            travel_world_seconds: 20,
        });
        let mut market = MootMarket::founding();
        let seller = MarketSeller::Business(route.warehouse);
        market.consign(seller, Good::Bread, 10, 100);
        assert!(review_merchant_consignment(
            &route,
            &mut autonomous,
            &history,
            &mut market,
            100,
            600.0,
            40.0,
            4,
            true
        ));
        assert_eq!(market.suggested_price(Good::Bread), 100);
        assert!(review_merchant_consignment(
            &route,
            &mut autonomous,
            &history,
            &mut market,
            100,
            600.0,
            40.0,
            4,
            false
        ));
        assert_eq!(market.suggested_price(Good::Bread), 90);
        assert_eq!(market.seller_listed_units(seller, Good::Bread), 10);
        market.purchase(Good::Bread, 10, 1_000, None, None);
        assert!(!review_merchant_consignment(
            &route,
            &mut autonomous,
            &history,
            &mut market,
            100,
            600.0,
            40.0,
            5,
            false
        ));
        assert_eq!(autonomous.unsold_since_day, None);
    }
}
