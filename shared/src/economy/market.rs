//! Local consignment order book, purchase settlement and daily market flow.

use super::{Good, GoodsInventory, MarketTradeTier, BASIS_POINTS, DEFAULT_MARKET_FEE_BPS};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Exact activity accumulated inside one market day.
///
/// Displayed quotes and executed prices are separate. A producer can sell
/// several units through a moving bid curve, so `producer_coin / producer_units`
/// is the realised average rather than an approximation from the closing bid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketDayFlow {
    pub opening_bid: u64,
    pub opening_ask: u64,
    pub high_bid: u64,
    pub low_bid: u64,
    pub high_ask: u64,
    pub low_ask: u64,
    pub producer_units: u64,
    pub producer_coin: u64,
    pub consumer_units: u64,
    pub consumer_coin: u64,
    /// Units buyers wanted but no eligible physical listing could supply.
    #[serde(default)]
    pub unavailable_units: u64,
    /// Units which existed, but the buyer's cash or maximum bid could not buy.
    #[serde(default)]
    pub unaffordable_units: u64,
    /// Unfilled units for which the buyer demonstrably had enough cash at the
    /// market's observed reference price. This is deliberately narrower than
    /// `unavailable_units`: hunger without purchasing power is a social need,
    /// not guaranteed merchant revenue.
    #[serde(default)]
    pub funded_unmet_units: u64,
}

impl MarketDayFlow {
    const fn opening(bid: u64, ask: u64) -> Self {
        Self {
            opening_bid: bid,
            opening_ask: ask,
            high_bid: bid,
            low_bid: bid,
            high_ask: ask,
            low_ask: ask,
            producer_units: 0,
            producer_coin: 0,
            consumer_units: 0,
            consumer_coin: 0,
            unavailable_units: 0,
            unaffordable_units: 0,
            funded_unmet_units: 0,
        }
    }

    fn observe_quote(&mut self, bid: u64, ask: u64) {
        self.high_bid = self.high_bid.max(bid);
        self.low_bid = self.low_bid.min(bid);
        self.high_ask = self.high_ask.max(ask);
        self.low_ask = self.low_ask.min(ask);
    }

    fn record_consignment_sale(
        &mut self,
        units: u32,
        gross: u64,
        seller_net: u64,
        records_end_demand: bool,
    ) {
        self.producer_units = self.producer_units.saturating_add(u64::from(units));
        self.producer_coin = self.producer_coin.saturating_add(seller_net);
        if records_end_demand {
            self.consumer_units = self.consumer_units.saturating_add(u64::from(units));
            self.consumer_coin = self.consumer_coin.saturating_add(gross);
        }
    }

    fn record_unmet_demand(&mut self, unavailable: u32, unaffordable: u32, funded_unmet: u32) {
        self.unavailable_units = self
            .unavailable_units
            .saturating_add(u64::from(unavailable));
        self.unaffordable_units = self
            .unaffordable_units
            .saturating_add(u64::from(unaffordable));
        self.funded_unmet_units = self
            .funded_unmet_units
            .saturating_add(u64::from(funded_unmet));
    }

    pub const fn unmet_units(self) -> u64 {
        self.unavailable_units
            .saturating_add(self.unaffordable_units)
    }

    pub const fn requested_units(self) -> u64 {
        self.consumer_units.saturating_add(self.unmet_units())
    }
}

impl Default for MarketDayFlow {
    fn default() -> Self {
        Self::opening(0, 0)
    }
}

/// One good's demand target, most recent sale and cheapest current offer.
/// Physical stock remains in the hall's [`GoodsInventory`]; private ownership
/// of that stock is retained by [`MarketListing`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketPool {
    pub target_stock: u32,
    pub bid: u64,
    pub ask: u64,
    pub units_bought: u64,
    pub units_sold: u64,
    pub coin_volume: u64,
    /// Session-only daily accumulator. The server closes and resets it at the
    /// calendar boundary; clients normally receive the archived result only
    /// when a history page is requested.
    #[serde(default)]
    pub day: MarketDayFlow,
    /// The immediately preceding completed day. Permit reviews need a small
    /// durable window so one failed breakfast does not create a factory, while
    /// two days of rejected buyers remain visible after history rolls the day.
    #[serde(default)]
    pub previous_day: MarketDayFlow,
}

impl MarketPool {
    const fn new(target_stock: u32, base_price: u64) -> Self {
        Self {
            target_stock,
            // In the consignment market BID is the last completed unit price
            // and ASK is the cheapest current listing. No public dealer cash
            // or synthetic buy quote exists at founding.
            bid: 0,
            ask: base_price,
            units_bought: 0,
            units_sold: 0,
            coin_volume: 0,
            day: MarketDayFlow::opening(0, base_price),
            previous_day: MarketDayFlow::opening(0, 0),
        }
    }
}

/// Result of one atomic market-side trade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MarketTrade {
    pub units: u32,
    pub pennies: u64,
}

/// The durable claimant on stock physically stored in a marketplace.
///
/// Businesses are the normal seller. Individuals can consign incidental
/// output, while Treasury stock covers old worlds and future public
/// procurement without pretending all goods in the hall are privately owned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum MarketSeller {
    Business(crate::components::BuildingId),
    Person(crate::components::PersonId),
    Treasury(crate::components::SettlementId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketListing {
    pub seller: MarketSeller,
    pub good: Good,
    pub units: u32,
    pub unit_price: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarketFill {
    pub seller: MarketSeller,
    pub good: Good,
    pub units: u32,
    pub unit_price: u64,
    pub gross: u64,
    pub market_fee: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MarketPurchase {
    pub trade: MarketTrade,
    pub fills: Vec<MarketFill>,
}

/// The local consignment exchange operated from a settlement's Moot Hall.
///
/// The Moot begins with no inventory and no buying money. A porter moves
/// privately owned goods into the hall, the listing retains its seller, and
/// payment happens only when a real consumer buys it. Public procurement can
/// later create Treasury-owned listings through the same book.
#[derive(Component, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MootMarket {
    pools: [MarketPool; Good::COUNT],
    #[serde(default)]
    listings: Vec<MarketListing>,
    #[serde(default = "default_market_fee_bps")]
    market_fee_bps: u16,
    #[serde(default)]
    trade_tier: MarketTradeTier,
}

const fn default_market_fee_bps() -> u16 {
    DEFAULT_MARKET_FEE_BPS
}

impl MootMarket {
    pub const fn founding() -> Self {
        Self {
            pools: [
                MarketPool::new(4, Good::Food.base_price()),
                MarketPool::new(4, Good::Wheat.base_price()),
                MarketPool::new(10, Good::Wood.base_price()),
                MarketPool::new(0, Good::Stone.base_price()),
                MarketPool::new(0, Good::Iron.base_price()),
                MarketPool::new(4, Good::Flour.base_price()),
                MarketPool::new(4, Good::Bread.base_price()),
                MarketPool::new(4, Good::Meat.base_price()),
                MarketPool::new(6, Good::Wool.base_price()),
            ],
            listings: Vec::new(),
            market_fee_bps: DEFAULT_MARKET_FEE_BPS,
            trade_tier: MarketTradeTier::Moot,
        }
    }

    pub const fn trade_tier(&self) -> MarketTradeTier {
        self.trade_tier
    }

    /// Formal inter-settlement commerce begins only after the settlement has
    /// built a physical Marketplace. The founding Moot exchange remains a
    /// local order book for residents and businesses, but caravans cannot use
    /// it as a regional endpoint.
    pub const fn supports_regional_trade(&self) -> bool {
        self.trade_tier as u8 >= MarketTradeTier::Marketplace as u8
    }

    /// Settlement development is monotonic, so a temporarily missing streamed
    /// Marketplace must never hide or strand goods which were already legal.
    pub fn unlock_trade_tier(&mut self, tier: MarketTradeTier) {
        self.trade_tier = self.trade_tier.max(tier);
    }

    pub const fn can_trade(&self, good: Good) -> bool {
        self.trade_tier as u8 >= good.minimum_market_tier() as u8
    }

    pub fn pool(&self, good: Good) -> &MarketPool {
        &self.pools[good.index()]
    }

    fn pool_mut(&mut self, good: Good) -> &mut MarketPool {
        &mut self.pools[good.index()]
    }

    pub const fn market_fee_bps(&self) -> u16 {
        self.market_fee_bps
    }

    pub fn set_market_fee_bps(&mut self, basis_points: u16) {
        self.market_fee_bps = basis_points.min(BASIS_POINTS as u16);
    }

    pub fn listings(&self) -> &[MarketListing] {
        &self.listings
    }

    pub fn listed_units(&self, good: Good) -> u32 {
        self.listings
            .iter()
            .filter(|listing| listing.good == good)
            .map(|listing| listing.units)
            .fold(0u32, u32::saturating_add)
    }

    /// How far current listings sit below the planning target. This is a
    /// shortage signal, not a storage or consignment limit: competing sellers
    /// must remain free to post cheaper asks until physical storage fills.
    pub fn target_shortfall(&self, good: Good) -> u32 {
        if !self.can_trade(good) {
            return 0;
        }
        self.pool(good)
            .target_stock
            .max(4)
            .saturating_sub(self.listed_units(good))
    }

    pub fn seller_listed_units(&self, seller: MarketSeller, good: Good) -> u32 {
        self.listings
            .iter()
            .filter(|listing| listing.seller == seller && listing.good == good)
            .map(|listing| listing.units)
            .fold(0u32, u32::saturating_add)
    }

    /// Cheapest live offer from somebody other than `seller`.
    ///
    /// Listings are already sorted by good, price and stable seller identity,
    /// so this is the actual rival an autonomous owner must beat rather than a
    /// synthetic settlement-wide quote or yesterday's highest asking price.
    pub fn best_competing_price(&self, seller: MarketSeller, good: Good) -> Option<u64> {
        self.listings
            .iter()
            .find(|listing| listing.good == good && listing.seller != seller && listing.units > 0)
            .map(|listing| listing.unit_price)
    }

    pub fn seller_total_listed_units(&self, seller: MarketSeller) -> u32 {
        self.listings
            .iter()
            .filter(|listing| listing.seller == seller)
            .map(|listing| listing.units)
            .fold(0u32, u32::saturating_add)
    }

    /// Transfer already-consigned stock without moving the physical goods.
    /// Used by estate handling so listings cannot keep paying a deceased
    /// `PersonId` after that person has left the simulation.
    pub fn transfer_seller(&mut self, from: MarketSeller, to: MarketSeller) -> u32 {
        if from == to {
            return 0;
        }
        let mut transferred = 0u32;
        for listing in &mut self.listings {
            if listing.seller == from {
                transferred = transferred.saturating_add(listing.units);
                listing.seller = to;
            }
        }
        self.sort_listings();
        transferred
    }

    pub fn listed_edible_units(&self) -> u32 {
        self.listings
            .iter()
            .filter(|listing| listing.good.is_edible())
            .map(|listing| listing.units)
            .fold(0u32, u32::saturating_add)
    }

    pub fn suggested_price(&self, good: Good) -> u64 {
        self.listings
            .iter()
            .find(|listing| listing.good == good && listing.units > 0)
            .map_or_else(
                || {
                    let last = self.pool(good).bid;
                    if last > 0 {
                        last
                    } else {
                        good.base_price()
                    }
                },
                |listing| listing.unit_price,
            )
    }

    fn sort_listings(&mut self) {
        self.listings.sort_unstable_by_key(|listing| {
            (listing.good.index(), listing.unit_price, listing.seller)
        });
    }

    pub fn total_volume(&self) -> u64 {
        self.pools
            .iter()
            .map(|pool| pool.coin_volume)
            .fold(0u64, u64::saturating_add)
    }

    /// Update desired reserves for a founding Hall from current population and
    /// construction demand.
    pub fn set_targets(&mut self, residents: u32, outstanding_wood: u32) {
        self.set_targets_with_marketplace(residents, outstanding_wood, false);
    }

    /// Update desired public shelves. A completed Marketplace does not create
    /// a second order book: it expands the same settlement exchange and gives
    /// it enough depth to serve processors and export-oriented producers.
    pub fn set_targets_with_marketplace(
        &mut self,
        residents: u32,
        outstanding_wood: u32,
        has_marketplace: bool,
    ) {
        let edible_target = residents.saturating_mul(3).max(4);
        let edible_target = if has_marketplace {
            edible_target.saturating_mul(2).max(24)
        } else {
            edible_target
        };
        self.pool_mut(Good::Food).target_stock = edible_target;
        self.pool_mut(Good::Flour).target_stock = edible_target;
        self.pool_mut(Good::Bread).target_stock = edible_target;
        // Wheat is both mill working stock and the settlement's main tradable
        // crop. One unit per resident made a 16-person town stop accepting at
        // 16 Wheat despite an otherwise empty Hall. The founding shelf now
        // supports two local processing turns; a Marketplace supports a much
        // deeper regional-producer shelf.
        self.pool_mut(Good::Wheat).target_stock = if has_marketplace {
            residents.saturating_mul(6).max(96)
        } else {
            residents.saturating_mul(2).max(32)
        };
        self.pool_mut(Good::Wood).target_stock =
            outstanding_wood.saturating_add(if has_marketplace { 40 } else { 10 });
    }

    /// Refresh the visible last-sale and best-offer quote from the order book.
    pub fn refresh_quote(&mut self, good: Good, _stock: u32) {
        let ask = self
            .listings
            .iter()
            .find(|listing| listing.good == good && listing.units > 0)
            .map_or(good.base_price(), |listing| listing.unit_price);
        let bid = self.pool(good).bid;
        let pool = self.pool_mut(good);
        pool.day.observe_quote(bid, ask);
        pool.ask = ask;
    }

    /// Begin a fresh market-day accumulator from the currently displayed
    /// quote. Called only after the previous accumulator has been archived.
    pub fn begin_new_day(&mut self) {
        for pool in &mut self.pools {
            pool.previous_day = pool.day;
            pool.day = MarketDayFlow::opening(pool.bid, pool.ask);
        }
    }

    pub fn refresh_all(&mut self, inventory: &GoodsInventory) {
        for good in Good::ALL {
            self.refresh_quote(good, inventory.amount(good));
        }
    }

    /// Put physically delivered goods on sale without paying the seller early.
    pub fn consign(&mut self, seller: MarketSeller, good: Good, units: u32, unit_price: u64) {
        if units == 0 || !self.can_trade(good) {
            return;
        }
        let unit_price = unit_price.max(1);
        if let Some(existing) = self.listings.iter_mut().find(|listing| {
            listing.seller == seller && listing.good == good && listing.unit_price == unit_price
        }) {
            existing.units = existing.units.saturating_add(units);
        } else {
            self.listings.push(MarketListing {
                seller,
                good,
                units,
                unit_price,
            });
        }
        self.pool_mut(good).units_bought = self
            .pool(good)
            .units_bought
            .saturating_add(u64::from(units));
        self.sort_listings();
        self.refresh_quote(good, self.listed_units(good));
    }

    pub fn reprice(&mut self, seller: MarketSeller, good: Good, unit_price: u64) {
        let unit_price = unit_price.max(1);
        for listing in &mut self.listings {
            if listing.seller == seller && listing.good == good {
                listing.unit_price = unit_price;
            }
        }
        self.sort_listings();
        self.refresh_quote(good, self.listed_units(good));
    }

    /// Mark down every offer from one liquidating seller while retaining a
    /// per-good floor. This is deliberately applied once per world day by the
    /// business manager, never once per render or simulation tick.
    pub fn markdown_seller(&mut self, seller: MarketSeller, basis_points: u16) {
        let basis_points = u64::from(basis_points).min(BASIS_POINTS);
        for listing in &mut self.listings {
            if listing.seller != seller {
                continue;
            }
            let movement = listing
                .unit_price
                .saturating_mul(basis_points)
                .div_ceil(BASIS_POINTS)
                .max(1);
            listing.unit_price = listing.unit_price.saturating_sub(movement).max(1);
        }
        self.sort_listings();
        for good in Good::ALL {
            self.refresh_quote(good, self.listed_units(good));
        }
    }

    /// Buy the cheapest consigned units which meet the caller's budget and
    /// optional price ceiling. The returned fills retain every seller so the
    /// authoritative server can credit businesses, people or the treasury.
    pub fn purchase(
        &mut self,
        good: Good,
        requested: u32,
        budget: u64,
        maximum_unit_price: Option<u64>,
        excluded_seller: Option<MarketSeller>,
    ) -> MarketPurchase {
        self.purchase_filtered(
            good,
            requested,
            budget,
            maximum_unit_price,
            None,
            excluded_seller,
            true,
        )
    }

    /// Buy inventory for resale elsewhere. The source producer still records
    /// a real sale and receives real cash, but this wholesale collection is
    /// not end demand from source-town households or processors. Keeping the
    /// two flows distinct prevents one caravan from making its supplier town
    /// look hungry for substitute goods.
    pub fn purchase_for_resale(
        &mut self,
        good: Good,
        requested: u32,
        budget: u64,
        maximum_unit_price: Option<u64>,
        excluded_seller: Option<MarketSeller>,
    ) -> MarketPurchase {
        self.purchase_filtered(
            good,
            requested,
            budget,
            maximum_unit_price,
            None,
            excluded_seller,
            false,
        )
    }

    /// Fill only one named seller's consignment. Buyer-funded caravan
    /// contracts use this after reserving cash against a specific public
    /// offer, so collection cannot silently switch ownership or price while
    /// the carrier is standing at the source market.
    pub fn purchase_from_seller(
        &mut self,
        seller: MarketSeller,
        good: Good,
        requested: u32,
        budget: u64,
        maximum_unit_price: Option<u64>,
    ) -> MarketPurchase {
        self.purchase_filtered(
            good,
            requested,
            budget,
            maximum_unit_price,
            Some(seller),
            None,
            true,
        )
    }

    /// Named-seller version of [`Self::purchase_for_resale`] used by a
    /// buyer-funded inter-settlement contract.
    pub fn purchase_from_seller_for_resale(
        &mut self,
        seller: MarketSeller,
        good: Good,
        requested: u32,
        budget: u64,
        maximum_unit_price: Option<u64>,
    ) -> MarketPurchase {
        self.purchase_filtered(
            good,
            requested,
            budget,
            maximum_unit_price,
            Some(seller),
            None,
            false,
        )
    }

    fn purchase_filtered(
        &mut self,
        good: Good,
        requested: u32,
        budget: u64,
        maximum_unit_price: Option<u64>,
        required_seller: Option<MarketSeller>,
        excluded_seller: Option<MarketSeller>,
        records_end_demand: bool,
    ) -> MarketPurchase {
        if !self.can_trade(good) {
            return MarketPurchase::default();
        }
        let mut purchase = MarketPurchase::default();
        let mut remaining = requested;
        for listing in &mut self.listings {
            if remaining == 0 || purchase.trade.pennies >= budget {
                break;
            }
            if listing.good != good
                || listing.units == 0
                || required_seller.is_some_and(|seller| seller != listing.seller)
                || excluded_seller == Some(listing.seller)
                || maximum_unit_price.is_some_and(|maximum| listing.unit_price > maximum)
            {
                continue;
            }
            let affordable = budget.saturating_sub(purchase.trade.pennies) / listing.unit_price;
            let units = remaining
                .min(listing.units)
                .min(affordable.min(u64::from(u32::MAX)) as u32);
            if units == 0 {
                break;
            }
            let gross = listing.unit_price.saturating_mul(u64::from(units));
            let fee = gross
                .saturating_mul(u64::from(self.market_fee_bps))
                .div_ceil(BASIS_POINTS)
                .min(gross);
            purchase.fills.push(MarketFill {
                seller: listing.seller,
                good,
                units,
                unit_price: listing.unit_price,
                gross,
                market_fee: fee,
            });
            listing.units -= units;
            remaining -= units;
            purchase.trade.units = purchase.trade.units.saturating_add(units);
            purchase.trade.pennies = purchase.trade.pennies.saturating_add(gross);
        }
        self.listings.retain(|listing| listing.units > 0);
        if purchase.trade.units > 0 {
            let fee = purchase
                .fills
                .iter()
                .map(|fill| fill.market_fee)
                .fold(0u64, u64::saturating_add);
            let last_price = purchase.fills.last().map_or(0, |fill| fill.unit_price);
            let pool = self.pool_mut(good);
            pool.bid = last_price;
            pool.units_sold = pool
                .units_sold
                .saturating_add(u64::from(purchase.trade.units));
            pool.coin_volume = pool.coin_volume.saturating_add(purchase.trade.pennies);
            pool.day.record_consignment_sale(
                purchase.trade.units,
                purchase.trade.pennies,
                purchase.trade.pennies.saturating_sub(fee),
                records_end_demand,
            );
        }
        self.sort_listings();
        self.refresh_quote(good, self.listed_units(good));
        purchase
    }

    /// Buy from the order book and retain the part of the request which the
    /// exchange could not satisfy. Ordinary `purchase` remains useful for
    /// already-previewed physical transfers; household and other once-per-day
    /// decisions use this method so unsuccessful demand is not economically
    /// invisible.
    pub fn purchase_recording_demand(
        &mut self,
        good: Good,
        requested: u32,
        budget: u64,
        maximum_unit_price: Option<u64>,
        excluded_seller: Option<MarketSeller>,
    ) -> MarketPurchase {
        // Locked goods do not create false scarcity signals. Demand becomes
        // economically visible only once this settlement can legally trade it.
        if !self.can_trade(good) {
            return MarketPurchase::default();
        }
        let available = self
            .listings
            .iter()
            .filter(|listing| {
                listing.good == good && listing.units > 0 && excluded_seller != Some(listing.seller)
            })
            .map(|listing| listing.units)
            .fold(0u32, u32::saturating_add);
        let price_eligible = self
            .listings
            .iter()
            .filter(|listing| {
                listing.good == good
                    && listing.units > 0
                    && excluded_seller != Some(listing.seller)
                    && maximum_unit_price.is_none_or(|maximum| listing.unit_price <= maximum)
            })
            .map(|listing| listing.units)
            .fold(0u32, u32::saturating_add);
        let preview =
            self.preview_purchase(good, requested, budget, maximum_unit_price, excluded_seller);
        let physically_possible = requested.min(available);
        let price_possible = requested.min(price_eligible);
        let unavailable = requested.saturating_sub(physically_possible);
        // A seller above the buyer's ceiling and a cheapest offer above their
        // remaining cash are both real price rejection, not missing stock.
        let unaffordable = physically_possible
            .saturating_sub(price_possible)
            .saturating_add(price_possible.saturating_sub(preview.units));
        // A remote merchant needs to distinguish an empty shelf from a buyer
        // who can actually pay. Use the caller's explicit ceiling when one
        // exists; ordinary household demand uses the stable reference price.
        // That lets a cheaper entrant recognize funded demand hidden behind
        // an incumbent's unaffordable asking price.
        let reference_price = maximum_unit_price
            .unwrap_or_else(|| good.base_price())
            .max(1);
        let funded_request =
            requested.min((budget / reference_price).min(u64::from(u32::MAX)) as u32);
        let funded_unmet = funded_request.saturating_sub(preview.units.min(funded_request));
        let purchase = self.purchase(good, requested, budget, maximum_unit_price, excluded_seller);
        debug_assert_eq!(purchase.trade, preview);
        self.pool_mut(good)
            .day
            .record_unmet_demand(unavailable, unaffordable, funded_unmet);
        purchase
    }

    /// Read the exact cheapest-fill result without changing listings or daily
    /// flow. This lets a buyer reserve real cash before the order book mutates.
    pub fn preview_purchase(
        &self,
        good: Good,
        requested: u32,
        budget: u64,
        maximum_unit_price: Option<u64>,
        excluded_seller: Option<MarketSeller>,
    ) -> MarketTrade {
        if !self.can_trade(good) {
            return MarketTrade::default();
        }
        let mut trade = MarketTrade::default();
        let mut remaining = requested;
        for listing in &self.listings {
            if remaining == 0 || trade.pennies >= budget {
                break;
            }
            if listing.good != good
                || listing.units == 0
                || excluded_seller == Some(listing.seller)
                || maximum_unit_price.is_some_and(|maximum| listing.unit_price > maximum)
            {
                continue;
            }
            let affordable = budget.saturating_sub(trade.pennies) / listing.unit_price;
            let units = remaining
                .min(listing.units)
                .min(affordable.min(u64::from(u32::MAX)) as u32);
            if units == 0 {
                break;
            }
            trade.units = trade.units.saturating_add(units);
            trade.pennies = trade
                .pennies
                .saturating_add(listing.unit_price.saturating_mul(u64::from(units)));
            remaining -= units;
        }
        trade
    }

    /// Adopt physical stock which predates ownership-aware listings as public
    /// Treasury stock. This is a save/test migration path, not founding stock.
    pub fn reconcile_inventory(
        &mut self,
        settlement: crate::components::SettlementId,
        inventory: &GoodsInventory,
    ) {
        for good in Good::ALL {
            // Locked physical stock may remain safely stored. It becomes a
            // Treasury listing on the first review after the tier unlocks.
            if !self.can_trade(good) {
                continue;
            }
            let physical = inventory.amount(good);
            let listed = self.listed_units(good);
            if physical > listed {
                self.consign(
                    MarketSeller::Treasury(settlement),
                    good,
                    physical - listed,
                    self.suggested_price(good),
                );
            } else if listed > physical {
                let mut excess = listed - physical;
                // Prefer discarding migration-created Treasury claims. If a
                // damaged save is still short, trim the most recently sorted
                // private offers deterministically rather than selling goods
                // which do not physically exist in the hall.
                for treasury_only in [true, false] {
                    for listing in self.listings.iter_mut().rev() {
                        if excess == 0 {
                            break;
                        }
                        if listing.good != good
                            || (treasury_only
                                && listing.seller != MarketSeller::Treasury(settlement))
                            || (!treasury_only
                                && listing.seller == MarketSeller::Treasury(settlement))
                        {
                            continue;
                        }
                        let removed = listing.units.min(excess);
                        listing.units -= removed;
                        excess -= removed;
                    }
                }
                self.listings.retain(|listing| listing.units > 0);
                self.sort_listings();
                self.refresh_quote(good, physical);
            }
        }
    }
}

impl Default for MootMarket {
    fn default() -> Self {
        Self::founding()
    }
}
