//! Physical goods, fixed-point money and the local Moot market.
//!
//! Goods remain physical and capacity-bounded. Coin is deliberately a ledger:
//! it consumes no cargo space and every transfer has two sides. The market is
//! an inventory-aware dealer rather than a global order book. Its bid and ask
//! react to the settlement's desired stock, while a widening spread protects a
//! pool that is running short of buying liquidity.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// The first five bulk goods in the settlement economy.
///
/// Coin is intentionally absent. It has no cargo bulk, is not consumed by a
/// building, and belongs in a wallet/ledger rather than in the same arithmetic
/// as logs and grain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Good {
    Food,
    Wheat,
    Wood,
    Stone,
    Iron,
}

/// One displayed coin is one hundred internal pennies.
///
/// Money never uses floating point. Prices may be derived with floating-point
/// tuning curves on the authoritative server, but every debit, credit and
/// conservation check is exact integer arithmetic.
pub const PENNIES_PER_COIN: u64 = 100;
pub const STARTING_VILLAGER_COINS: u64 = 10;
pub const STARTING_VILLAGER_MONEY: u64 = STARTING_VILLAGER_COINS * PENNIES_PER_COIN;
pub const STARTING_TREASURY_MONEY: u64 = 20 * PENNIES_PER_COIN;
/// Public daily wage for the Moot Hall's single road-steward position.
pub const ROAD_STEWARD_DAILY_SALARY: u64 = PENNIES_PER_COIN;
/// Ordinary business wage. Kept equal across founding trades until skills and
/// a labour market exist; importantly it is paid for holding a real position,
/// not for each animation loop completed at high time warp.
pub const FOUNDING_DAILY_WAGE: u64 = PENNIES_PER_COIN;
/// Bounds and review cadence for an NPC owner's automatic wage offer.
pub const MINIMUM_BUSINESS_DAILY_WAGE: u64 = PENNIES_PER_COIN / 2;
pub const MAXIMUM_BUSINESS_DAILY_WAGE: u64 = 3 * PENNIES_PER_COIN;
pub const BUSINESS_WAGE_REVIEW_STEP: u64 = PENNIES_PER_COIN / 10;
pub const VACANCY_DAYS_BEFORE_RAISE: u16 = 2;
pub const PAYROLL_STRESS_DAYS_BEFORE_CUT: u16 = 2;
pub const FULLY_STAFFED_DAYS_BEFORE_REVIEW: u16 = 7;
/// A productive owner may leave the worker roster only after reaching this
/// personal liquid cushion and finding a replacement worker.
pub const WEALTHY_OWNER_MONEY: u64 = 30 * PENNIES_PER_COIN;

/// Format internal pennies for player-facing panels without losing pennies.
pub fn format_money(pennies: u64) -> String {
    format!(
        "{}.{:02}",
        pennies / PENNIES_PER_COIN,
        pennies % PENNIES_PER_COIN
    )
}

/// Personal liquid money. Coin is not a [`Good`] and has no physical bulk.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Wallet {
    pennies: u64,
}

impl Wallet {
    pub const fn new(pennies: u64) -> Self {
        Self { pennies }
    }

    pub const fn founding_villager() -> Self {
        Self::new(STARTING_VILLAGER_MONEY)
    }

    pub const fn balance(self) -> u64 {
        self.pennies
    }

    pub fn can_afford(self, pennies: u64) -> bool {
        self.pennies >= pennies
    }

    pub fn debit(&mut self, pennies: u64) -> bool {
        if self.pennies < pennies {
            return false;
        }
        self.pennies -= pennies;
        true
    }

    pub fn credit(&mut self, pennies: u64) {
        self.pennies = self.pennies.saturating_add(pennies);
    }
}

/// Shared necessities budget attached to a completed house.
///
/// Personal wallets still exist for permits and discretionary purchases. A
/// household contributes only enough for its pantry target, so one resident
/// can shop without pretending every dependant owns an equal share of cash.
#[derive(Component, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HouseholdEconomy {
    pub pennies: u64,
    pub pantry_target_days: u8,
    pub shopper: Option<String>,
    pub last_budget_day: u32,
}

impl Default for HouseholdEconomy {
    fn default() -> Self {
        Self {
            pennies: 0,
            pantry_target_days: 3,
            shopper: None,
            last_budget_day: u32::MAX,
        }
    }
}

/// Cash and payroll belonging to a business rather than its owner personally.
/// Sale receipts land here; wages and a conservative owner draw leave it.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessAccount {
    pub cash: u64,
    pub wage_arrears: u64,
    pub last_payroll_day: u32,
}

impl Default for BusinessAccount {
    fn default() -> Self {
        Self {
            cash: 0,
            wage_arrears: 0,
            last_payroll_day: u32::MAX,
        }
    }
}

/// The wage offered by one workplace, controlled by its owner.
///
/// NPC owners begin in automatic mode. Persistent vacancies push the offer up
/// when the business can cover a full staffed day; persistent arrears push it
/// down. A future player-business panel can turn `automatic` off and edit the
/// same replicated `daily_wage` rather than introducing a second salary path.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessWagePolicy {
    pub daily_wage: u64,
    pub automatic: bool,
    pub vacancy_days: u16,
    pub payroll_stress_days: u16,
    pub fully_staffed_days: u16,
}

impl Default for BusinessWagePolicy {
    fn default() -> Self {
        Self {
            daily_wage: FOUNDING_DAILY_WAGE,
            automatic: true,
            vacancy_days: 0,
            payroll_stress_days: 0,
            fully_staffed_days: 0,
        }
    }
}

/// Optional minimum aptitudes for a workplace. Founding businesses deliberately
/// do not carry this component, so farms, fishing huts and lumber huts remain
/// open to every resident while the first labour market is being established.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WorkforceRequirements {
    pub minimum_physique: u8,
    pub minimum_intelligence: u8,
    pub minimum_charm: u8,
}

impl WorkforceRequirements {
    pub fn is_met_by(self, attributes: crate::components::CharacterAttributes) -> bool {
        attributes.physique() >= self.minimum_physique.min(100)
            && attributes.intelligence() >= self.minimum_intelligence.min(100)
            && attributes.charm() >= self.minimum_charm.min(100)
    }
}

/// Owner-selected offer settings. All early businesses use these defaults;
/// the component makes them inspectable and ready for future player control.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessSalePolicy {
    pub collection_enabled: bool,
    pub keep_units: u32,
    pub max_units_per_collection: u32,
    pub minimum_unit_price: u64,
}

impl Default for BusinessSalePolicy {
    fn default() -> Self {
        Self {
            collection_enabled: true,
            keep_units: 2,
            max_units_per_collection: 8,
            minimum_unit_price: 1,
        }
    }
}

/// The authored object shown in a character's arms.
///
/// This is deliberately separate from [`Good`]. Accounting can keep one Food
/// category while a fisherman carries a fish basket and a future baker carries
/// bread. Only the five appearances with shipped assets exist today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CarriedAppearance {
    FishBasket,
    WheatSheaf,
    WoodBundle,
    StoneBundle,
    IronBundle,
}

impl CarriedAppearance {
    /// Default appearance when the producing routine has no more specific
    /// presentation. Food currently comes only from fishing, so its honest
    /// first appearance is a fish basket rather than a generic crate.
    pub const fn default_for(good: Good) -> Self {
        match good {
            Good::Food => Self::FishBasket,
            Good::Wheat => Self::WheatSheaf,
            Good::Wood => Self::WoodBundle,
            Good::Stone => Self::StoneBundle,
            Good::Iron => Self::IronBundle,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::FishBasket => "Fish basket",
            Self::WheatSheaf => "Wheat sheaf",
            Self::WoodBundle => "Wood bundle",
            Self::StoneBundle => "Stone bundle",
            Self::IronBundle => "Iron bundle",
        }
    }
}

/// Small replicated summary of what is visibly in a character's arms.
///
/// The authoritative quantities remain in [`GoodsInventory`]. This component
/// exists so a client can select a carry animation and prop without receiving
/// every private inventory slot whenever one amount changes.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CarriedLoad {
    pub good: Option<Good>,
    pub amount: u32,
    /// Visual presentation is not the accounting category. `None` remains
    /// valid for old saves/packets and falls back through `default_for`.
    #[serde(default)]
    pub appearance: Option<CarriedAppearance>,
}

impl CarriedLoad {
    pub fn from_inventory(inventory: &GoodsInventory) -> Self {
        let good = Good::ALL
            .into_iter()
            .find(|good| inventory.amount(*good) > 0);
        Self {
            good,
            amount: good.map(|good| inventory.amount(good)).unwrap_or(0),
            appearance: good.map(CarriedAppearance::default_for),
        }
    }

    pub const fn is_empty(self) -> bool {
        self.amount == 0 || self.good.is_none()
    }

    pub const fn visible_appearance(self) -> Option<CarriedAppearance> {
        if self.is_empty() {
            None
        } else if let Some(appearance) = self.appearance {
            Some(appearance)
        } else if let Some(good) = self.good {
            Some(CarriedAppearance::default_for(good))
        } else {
            None
        }
    }
}

impl Good {
    pub const ALL: [Self; 5] = [Self::Food, Self::Wheat, Self::Wood, Self::Stone, Self::Iron];
    pub const COUNT: usize = Self::ALL.len();

    pub const fn label(self) -> &'static str {
        match self {
            Self::Food => "Food",
            Self::Wheat => "Wheat",
            Self::Wood => "Wood",
            Self::Stone => "Stone",
            Self::Iron => "Iron",
        }
    }

    /// Whether one unit can satisfy one resident's daily food ration.
    ///
    /// Wheat is deliberately edible in the first economy slice. A mill and
    /// bakery can later turn it into a more efficient or valuable Food good,
    /// but villages must be able to eat what their first Farmstead grows now.
    pub const fn is_edible(self) -> bool {
        matches!(self, Self::Food | Self::Wheat)
    }

    /// How much physical capacity one unit occupies.
    ///
    /// Quantities are gameplay units (a ration, a tied wood bundle, a dressed
    /// stone block, an iron billet), not kilograms. The relative bulk is what
    /// makes a person able to carry many meals but only a few bundles of wood.
    pub const fn bulk_per_unit(self) -> u32 {
        match self {
            Self::Food => 1,
            Self::Wheat => 2,
            Self::Wood => 4,
            Self::Stone => 6,
            Self::Iron => 3,
        }
    }

    /// Reference value before local stock pressure and liquidity risk.
    pub const fn base_price(self) -> u64 {
        match self {
            Self::Food => 100,
            Self::Wheat => 80,
            // A founding villager's ten coins must be enough to buy the ten
            // bundles for a modest cabin on a reasonably stocked market. The
            // scarcity curve can still make timber dear in a true shortage.
            Self::Wood => 50,
            Self::Stone => 250,
            Self::Iron => 400,
        }
    }

    pub const fn index(self) -> usize {
        match self {
            Self::Food => 0,
            Self::Wheat => 1,
            Self::Wood => 2,
            Self::Stone => 3,
            Self::Iron => 4,
        }
    }
}

/// Maximum number of completed in-game days retained in session memory.
///
/// History is deliberately not persistent yet: settlements still use runtime
/// entities as identity and the test workflow regularly starts a fresh world.
pub const SETTLEMENT_HISTORY_DAYS: usize = 365;

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
        }
    }

    fn observe_quote(&mut self, bid: u64, ask: u64) {
        self.high_bid = self.high_bid.max(bid);
        self.low_bid = self.low_bid.min(bid);
        self.high_ask = self.high_ask.max(ask);
        self.low_ask = self.low_ask.min(ask);
    }

    fn record_producer_sale(&mut self, unit_price: u64) {
        self.producer_units = self.producer_units.saturating_add(1);
        self.producer_coin = self.producer_coin.saturating_add(unit_price);
    }

    fn record_consumer_purchase(&mut self, unit_price: u64) {
        self.consumer_units = self.consumer_units.saturating_add(1);
        self.consumer_coin = self.consumer_coin.saturating_add(unit_price);
    }
}

impl Default for MarketDayFlow {
    fn default() -> Self {
        Self::opening(0, 0)
    }
}

/// One good's earmarked buying liquidity and current public quote.
///
/// Physical stock remains in the hall's [`GoodsInventory`]. Duplicating it in
/// this component would eventually disagree, so trade functions receive the
/// real stock amount and update only cash, quotes and cumulative flow here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketPool {
    pub cash: u64,
    pub target_cash: u64,
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
}

impl MarketPool {
    const fn new(cash: u64, target_stock: u32, base_price: u64) -> Self {
        let bid = base_price.saturating_mul(92) / 100;
        let ask = base_price.saturating_mul(108).div_ceil(100);
        Self {
            cash,
            target_cash: cash,
            target_stock,
            bid,
            ask,
            units_bought: 0,
            units_sold: 0,
            coin_volume: 0,
            day: MarketDayFlow::opening(bid, ask),
        }
    }
}

/// Result of one atomic market-side trade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MarketTrade {
    pub units: u32,
    pub pennies: u64,
}

/// The local dealer operated from a settlement's Moot Hall.
///
/// Each good has its own cash budget. This prevents an abundant Wheat harvest
/// from draining the money needed to buy scarce construction Wood. The general
/// settlement treasury is separate and can later recapitalise a pool through
/// an explicit transfer rather than silently paying every seller.
#[derive(Component, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MootMarket {
    pools: [MarketPool; Good::COUNT],
}

impl MootMarket {
    pub const fn founding() -> Self {
        Self {
            pools: [
                MarketPool::new(3_000, 4, Good::Food.base_price()),
                MarketPool::new(3_000, 4, Good::Wheat.base_price()),
                MarketPool::new(4_000, 10, Good::Wood.base_price()),
                MarketPool::new(1_000, 0, Good::Stone.base_price()),
                MarketPool::new(1_000, 0, Good::Iron.base_price()),
            ],
        }
    }

    pub fn pool(&self, good: Good) -> &MarketPool {
        &self.pools[good.index()]
    }

    fn pool_mut(&mut self, good: Good) -> &mut MarketPool {
        &mut self.pools[good.index()]
    }

    /// Pools remain useful price/accounting bands, but their pennies form one
    /// operating fund. Move idle cash deterministically when the good being
    /// collected cannot cover its next bid; no coin is created or destroyed.
    fn share_liquidity_for(&mut self, good: Good, wanted: u64) {
        let mut deficit = wanted.saturating_sub(self.pool(good).cash);
        if deficit == 0 {
            return;
        }
        for donor in Good::ALL {
            if donor == good || deficit == 0 {
                continue;
            }
            let available = self.pool(donor).cash;
            let moved = available.min(deficit);
            self.pool_mut(donor).cash -= moved;
            let receiving_cash = self.pool(good).cash;
            self.pool_mut(good).cash = receiving_cash.saturating_add(moved);
            deficit -= moved;
        }
    }

    pub fn total_liquidity(&self) -> u64 {
        self.pools
            .iter()
            .map(|pool| pool.cash)
            .fold(0u64, u64::saturating_add)
    }

    pub fn total_volume(&self) -> u64 {
        self.pools
            .iter()
            .map(|pool| pool.coin_volume)
            .fold(0u64, u64::saturating_add)
    }

    /// Update desired reserves from current population and construction demand.
    pub fn set_targets(&mut self, residents: u32, outstanding_wood: u32) {
        let edible_target = residents.saturating_mul(3).max(4);
        self.pool_mut(Good::Food).target_stock = edible_target;
        self.pool_mut(Good::Wheat).target_stock = edible_target;
        self.pool_mut(Good::Wood).target_stock = outstanding_wood.saturating_add(10);
    }

    /// Refresh the visible one-unit quote against the real hall inventory.
    pub fn refresh_quote(&mut self, good: Good, stock: u32) {
        let (bid, ask) = quote(self.pool(good), good, stock);
        let pool = self.pool_mut(good);
        pool.day.observe_quote(bid, ask);
        pool.bid = bid;
        pool.ask = ask;
    }

    /// Begin a fresh market-day accumulator from the currently displayed
    /// quote. Called only after the previous accumulator has been archived.
    pub fn begin_new_day(&mut self) {
        for pool in &mut self.pools {
            pool.day = MarketDayFlow::opening(pool.bid, pool.ask);
        }
    }

    pub fn refresh_all(&mut self, inventory: &GoodsInventory) {
        for good in Good::ALL {
            self.refresh_quote(good, inventory.amount(good));
        }
    }

    /// Buy up to `offered` physical units from a producer.
    ///
    /// Every unit moves the curve before the next one is priced, producing
    /// deterministic slippage. The market never promises money it does not
    /// actually have.
    pub fn buy_from_producer(&mut self, good: Good, stock: u32, offered: u32) -> MarketTrade {
        let mut trade = MarketTrade::default();
        let mut simulated_stock = stock;
        for _ in 0..offered {
            let (initial_bid, _) = quote(self.pool(good), good, simulated_stock);
            self.share_liquidity_for(good, initial_bid);
            let (bid, ask) = quote(self.pool(good), good, simulated_stock);
            if bid == 0 || self.pool(good).cash < bid {
                break;
            }
            let pool = self.pool_mut(good);
            pool.day.observe_quote(bid, ask);
            pool.day.record_producer_sale(bid);
            pool.cash -= bid;
            pool.units_bought = pool.units_bought.saturating_add(1);
            pool.coin_volume = pool.coin_volume.saturating_add(bid);
            trade.units += 1;
            trade.pennies = trade.pennies.saturating_add(bid);
            simulated_stock = simulated_stock.saturating_add(1);
        }
        self.refresh_quote(good, simulated_stock);
        trade
    }

    /// Undo an undeliverable collection reservation. Reservations normally
    /// become real producer sales at the hall; this path exists for the rare
    /// case where construction consumed a reserved log before the porter
    /// reached the business.
    pub fn cancel_producer_purchase(&mut self, good: Good, stock: u32, trade: MarketTrade) {
        if trade.units == 0 && trade.pennies == 0 {
            return;
        }
        let pool = self.pool_mut(good);
        pool.cash = pool.cash.saturating_add(trade.pennies);
        pool.units_bought = pool.units_bought.saturating_sub(u64::from(trade.units));
        pool.coin_volume = pool.coin_volume.saturating_sub(trade.pennies);
        pool.day.producer_units = pool
            .day
            .producer_units
            .saturating_sub(u64::from(trade.units));
        pool.day.producer_coin = pool.day.producer_coin.saturating_sub(trade.pennies);
        self.refresh_quote(good, stock);
    }

    /// Sell up to `requested` physical units to a consumer with `budget`.
    pub fn sell_to_consumer(
        &mut self,
        good: Good,
        stock: u32,
        requested: u32,
        budget: u64,
    ) -> MarketTrade {
        let mut trade = MarketTrade::default();
        let mut simulated_stock = stock;
        for _ in 0..requested.min(stock) {
            let (bid, ask) = quote(self.pool(good), good, simulated_stock);
            if ask == 0 || trade.pennies.saturating_add(ask) > budget {
                break;
            }
            let pool = self.pool_mut(good);
            pool.day.observe_quote(bid, ask);
            pool.day.record_consumer_purchase(ask);
            pool.cash = pool.cash.saturating_add(ask);
            pool.units_sold = pool.units_sold.saturating_add(1);
            pool.coin_volume = pool.coin_volume.saturating_add(ask);
            trade.units += 1;
            trade.pennies = trade.pennies.saturating_add(ask);
            simulated_stock = simulated_stock.saturating_sub(1);
        }
        self.refresh_quote(good, simulated_stock);
        trade
    }
}

impl Default for MootMarket {
    fn default() -> Self {
        Self::founding()
    }
}

/// One completed good-day sent to clients as part of an on-demand archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MarketGoodHistoryDay {
    pub opening_bid: u64,
    pub opening_ask: u64,
    pub closing_bid: u64,
    pub closing_ask: u64,
    pub high_bid: u64,
    pub low_bid: u64,
    pub high_ask: u64,
    pub low_ask: u64,
    pub producer_units: u64,
    pub producer_coin: u64,
    pub consumer_units: u64,
    pub consumer_coin: u64,
    pub closing_stock: u32,
    pub target_stock: u32,
    pub pool_cash: u64,
}

impl MarketGoodHistoryDay {
    pub fn average_producer_price(self) -> Option<u64> {
        (self.producer_units > 0).then(|| self.producer_coin / self.producer_units)
    }

    pub fn average_consumer_price(self) -> Option<u64> {
        (self.consumer_units > 0).then(|| self.consumer_coin / self.consumer_units)
    }

    pub const fn coin_volume(self) -> u64 {
        self.producer_coin.saturating_add(self.consumer_coin)
    }
}

/// One completed daily reading of a settlement's money, goods and welfare.
///
/// `total_local_coin` is the conservation-oriented measure: treasury + market
/// pools + resident wallets + payments already removed from a pool but still
/// waiting for their named recipient. Produced goods are tracked separately
/// and valued at that day's closing bid in `stock_liquidation_value`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettlementHistoryDay {
    pub day: u32,
    pub market: [MarketGoodHistoryDay; Good::COUNT],
    pub civic_treasury: u64,
    pub market_cash: [u64; Good::COUNT],
    pub resident_wallet_money: u64,
    pub pending_payments: u64,
    pub physical_stock: [u32; Good::COUNT],
    pub stock_liquidation_value: u64,
    pub total_local_coin: u64,
    pub population: u32,
    pub employed: u32,
    pub hungry: u32,
    pub food_reserves: u32,
    pub food_produced: u32,
    pub food_consumed: u32,
    pub buildings: u16,
    pub productive_buildings: u16,
    pub work_positions: u16,
    pub filled_jobs: u16,
    pub prosperity: f32,
    pub reserve_prosperity: f32,
    pub production_prosperity: f32,
    pub housing_prosperity: f32,
    pub employment_prosperity: f32,
    pub hunger_penalty: f32,
}

/// Up to one in-game year of session history for a single settlement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SettlementHistoryArchive {
    pub settlement: String,
    /// Oldest to newest, never longer than [`SETTLEMENT_HISTORY_DAYS`].
    pub days: Vec<SettlementHistoryDay>,
}

/// One completed world-wide daily rollup across every settlement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldHistoryDay {
    pub day: u32,
    pub settlements: u32,
    pub population: u32,
    pub employed: u32,
    pub hungry: u32,
    pub civic_treasury: u64,
    pub market_cash: u64,
    pub resident_wallet_money: u64,
    pub pending_payments: u64,
    pub total_local_coin: u64,
    pub stock_liquidation_value: u64,
    pub physical_stock: [u32; Good::COUNT],
    pub food_reserves: u32,
    pub food_produced: u32,
    pub food_consumed: u32,
    pub buildings: u32,
    pub productive_buildings: u32,
    /// Population-weighted mean of settlement prosperity. Empty foundations
    /// receive equal weight only when the whole world has zero residents.
    pub prosperity: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct WorldHistoryArchive {
    /// Oldest to newest, never longer than [`SETTLEMENT_HISTORY_DAYS`].
    pub days: Vec<WorldHistoryDay>,
}

/// Inventory-aware midpoint with a low-cash risk spread.
///
/// The stock buffer keeps an empty new market finite. Low stock moves both
/// quotes upward. Low cash widens the spread: the market pays sellers less and
/// charges buyers more until purchases restore its buying liquidity.
fn quote(pool: &MarketPool, good: Good, stock: u32) -> (u64, u64) {
    let target = pool.target_stock.max(1) as f64;
    let stock_buffer = (target * 0.5).max(4.0);
    let stock_pressure = ((target + stock_buffer) / (stock as f64 + stock_buffer))
        .powf(0.68)
        .clamp(0.25, 8.0);
    let mid = good.base_price() as f64 * stock_pressure;

    let cash_buffer = (pool.target_cash / 4).max(PENNIES_PER_COIN) as f64;
    let cash_ratio = (pool.target_cash as f64 + cash_buffer) / (pool.cash as f64 + cash_buffer);
    let low_cash_risk = (cash_ratio - 1.0).max(0.0);
    let spread = (0.08 + low_cash_risk * 0.14).clamp(0.08, 0.65);
    let bid = (mid * (1.0 - spread)).floor().max(1.0) as u64;
    let ask = (mid * (1.0 + spread)).ceil().max(bid as f64 + 1.0) as u64;
    (bid, ask)
}

/// Automatic permit price. Needed housing is civic approval and remains free;
/// every business permit has a positive floor.
pub fn permit_price(
    kind: crate::components::SettlementBuildingKind,
    applicant_holdings: usize,
    needed_by_settlement: bool,
) -> u64 {
    use crate::components::SettlementBuildingKind;
    if kind == SettlementBuildingKind::House {
        return 0;
    }
    if kind == SettlementBuildingKind::Hall {
        return u64::MAX;
    }
    let base: u64 = match kind {
        SettlementBuildingKind::Farmstead | SettlementBuildingKind::FishermansHut => 300,
        SettlementBuildingKind::LumberjackHut => 250,
        // Civic blockouts are settlement-requested progression infrastructure.
        // Their physical wood still has to be supplied; pricing can be revisited
        // with the wider ownership model without creating a progression lock.
        SettlementBuildingKind::Market
        | SettlementBuildingKind::Tavern
        | SettlementBuildingKind::Church => return 0,
        SettlementBuildingKind::House | SettlementBuildingKind::Hall => unreachable!(),
    };
    let holdings_multiplier = 100u64.saturating_add(applicant_holdings as u64 * 50);
    let need_multiplier = if needed_by_settlement { 55 } else { 100 };
    base.saturating_mul(holdings_multiplier)
        .saturating_mul(need_multiplier)
        .div_ceil(10_000)
        .max(PENNIES_PER_COIN)
}

/// Server-owned bounded storage for bulk goods.
///
/// One representation serves carried loads, workplace stores, houses and the
/// settlement hall. Different capacities make them different without giving
/// each location subtly different transfer rules. The fixed array keeps the
/// strategic tick cheap and makes serialization stable and predictable.
#[derive(Component, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoodsInventory {
    amounts: [u32; Good::COUNT],
    bulk_capacity: u32,
}

impl GoodsInventory {
    pub const fn new(bulk_capacity: u32) -> Self {
        Self {
            amounts: [0; Good::COUNT],
            bulk_capacity,
        }
    }

    pub const fn bulk_capacity(&self) -> u32 {
        self.bulk_capacity
    }

    pub fn amount(&self, good: Good) -> u32 {
        self.amounts[good.index()]
    }

    pub fn used_bulk(&self) -> u32 {
        Good::ALL
            .iter()
            .map(|good| self.amount(*good).saturating_mul(good.bulk_per_unit()))
            .fold(0, u32::saturating_add)
    }

    pub fn free_bulk(&self) -> u32 {
        self.bulk_capacity.saturating_sub(self.used_bulk())
    }

    pub fn is_empty(&self) -> bool {
        self.amounts.iter().all(|amount| *amount == 0)
    }

    /// Edible portions currently stored here. One fish Food or one Wheat is
    /// one resident-day in the prototype economy.
    pub fn edible_amount(&self) -> u32 {
        self.amount(Good::Food)
            .saturating_add(self.amount(Good::Wheat))
    }

    /// Consume up to `requested` portions, prepared Food before raw Wheat.
    pub fn remove_edible(&mut self, requested: u32) -> u32 {
        let food = self.remove(Good::Food, requested);
        food.saturating_add(self.remove(Good::Wheat, requested.saturating_sub(food)))
    }

    /// Add up to `requested` units and return the amount accepted.
    pub fn add(&mut self, good: Good, requested: u32) -> u32 {
        let accepted = requested.min(self.free_bulk() / good.bulk_per_unit());
        self.amounts[good.index()] = self.amount(good).saturating_add(accepted);
        accepted
    }

    /// Remove up to `requested` units and return the amount removed.
    pub fn remove(&mut self, good: Good, requested: u32) -> u32 {
        let removed = requested.min(self.amount(good));
        self.amounts[good.index()] -= removed;
        removed
    }

    /// Move goods into another inventory, partially if the destination fills.
    ///
    /// Returns the amount moved. Goods are removed only after the destination
    /// accepts them, so a full store can never destroy a worker's carried load.
    pub fn transfer_to(&mut self, destination: &mut Self, good: Good, requested: u32) -> u32 {
        let available = requested.min(self.amount(good));
        let accepted = destination.add(good, available);
        let removed = self.remove(good, accepted);
        debug_assert_eq!(accepted, removed);
        accepted
    }
}

impl Default for GoodsInventory {
    fn default() -> Self {
        Self::new(0)
    }
}

/// Replicated settlement-level reading of the physical food loop.
///
/// Stocks remain in [`GoodsInventory`]; this is the small derived summary used
/// by autonomous planning, tier advancement and the settlement panel. Recent
/// rates are rolling daily averages rather than lifetime totals.
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettlementEconomy {
    pub edible_stock: u32,
    pub reserve_days: f32,
    pub recent_food_production: f32,
    pub recent_food_consumption: f32,
    pub unmet_food: u32,
    pub observed_days: u16,
    pub food_secure_days: u16,
    pub reserve_prosperity: f32,
    pub production_prosperity: f32,
    pub housing_prosperity: f32,
    pub employment_prosperity: f32,
    pub hunger_penalty: f32,
    pub prosperity: f32,
}

impl Default for SettlementEconomy {
    fn default() -> Self {
        Self {
            edible_stock: 0,
            reserve_days: 0.0,
            recent_food_production: 0.0,
            recent_food_consumption: 0.0,
            unmet_food: 0,
            observed_days: 0,
            food_secure_days: 0,
            reserve_prosperity: 0.0,
            production_prosperity: 0.0,
            housing_prosperity: 0.0,
            employment_prosperity: 0.0,
            hunger_penalty: 0.0,
            prosperity: 0.0,
        }
    }
}

/// A settlement is secure once it can survive this many days without another
/// harvest. Production reliability remains a separate promotion requirement.
pub const FOOD_SECURITY_TARGET_DAYS: f32 = 3.0;
pub const VILLAGE_MIN_RESIDENTS: u32 = 4;
pub const VILLAGE_REQUIRED_SECURE_DAYS: u16 = 3;
pub const VILLAGE_MIN_PROSPERITY: f32 = 65.0;
pub const TOWN_MIN_RESIDENTS: u32 = 12;
pub const TOWN_MIN_PROSPERITY: f32 = 70.0;
pub const TOWN_MIN_MARKET_VOLUME: u64 = 5_000;
pub const TOWN_REQUIRED_DAYS: u16 = 3;
pub const CITY_MIN_RESIDENTS: u32 = 24;
pub const CITY_MIN_PROSPERITY: f32 = 75.0;
pub const CITY_REQUIRED_DAYS: u16 = 5;

/// The next building justified by measurable settlement shortages.
///
/// Kept in shared code because the server makes the decision while clients
/// explain it in civic records. Two copies would eventually make the hall ask
/// for one permit while its encyclopedia confidently named another.
pub fn next_settlement_building(
    farms: usize,
    fishers: usize,
    lumber_huts: usize,
    houses: usize,
    residents: u32,
    economy: Option<&SettlementEconomy>,
) -> Option<crate::components::SettlementBuildingKind> {
    use crate::components::SettlementBuildingKind as Kind;

    if farms + fishers == 0 {
        return Some(Kind::Farmstead);
    }
    if lumber_huts == 0 {
        return Some(Kind::LumberjackHut);
    }
    let beds = houses.saturating_mul(Kind::House.housing_capacity() as usize);
    if beds < residents as usize {
        return Some(Kind::House);
    }
    if fishers > 0 && farms == 0 {
        return Some(Kind::Farmstead);
    }
    let food_shortage = economy.is_some_and(|economy| {
        economy.observed_days > 0
            && (economy.reserve_days < FOOD_SECURITY_TARGET_DAYS
                || economy.recent_food_production < residents as f32)
    });
    let supported_farms = residents.div_ceil(4).max(1) as usize;
    if food_shortage && farms < supported_farms {
        return Some(Kind::Farmstead);
    }
    None
}

/// Initial physical capacities, measured in [`Good::bulk_per_unit`] units.
///
/// These are tuning, not economic values. They say how much can physically be
/// present before somebody must haul it elsewhere; they say nothing about who
/// owns the contents or what they are worth.
pub mod capacity {
    pub const VILLAGER: u32 = 12;
    pub const HOUSE: u32 = 80;
    pub const FARMSTEAD: u32 = 240;
    pub const LUMBERJACK_HUT: u32 = 240;
    pub const FISHERMANS_HUT: u32 = 240;
    pub const MARKET: u32 = 600;
    pub const TAVERN: u32 = 180;
    pub const CHURCH: u32 = 120;
    pub const HALL: u32 = 1_200;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlike_goods_compete_for_the_same_physical_space() {
        let mut inventory = GoodsInventory::new(12);

        assert_eq!(inventory.add(Good::Food, 4), 4);
        assert_eq!(inventory.add(Good::Wood, 9), 2);
        assert_eq!(inventory.amount(Good::Food), 4);
        assert_eq!(inventory.amount(Good::Wood), 2);
        assert_eq!(inventory.used_bulk(), 12);
        assert_eq!(inventory.free_bulk(), 0);
    }

    #[test]
    fn a_partial_transfer_is_lossless() {
        let mut carrier = GoodsInventory::new(40);
        let mut nearly_full_hut = GoodsInventory::new(12);
        assert_eq!(carrier.add(Good::Wood, 8), 8);
        assert_eq!(nearly_full_hut.add(Good::Food, 8), 8);

        let before = carrier.amount(Good::Wood) + nearly_full_hut.amount(Good::Wood);
        let moved = carrier.transfer_to(&mut nearly_full_hut, Good::Wood, 8);
        let after = carrier.amount(Good::Wood) + nearly_full_hut.amount(Good::Wood);

        assert_eq!(moved, 1, "only one wood bundle fits in four bulk");
        assert_eq!(
            before, after,
            "a full destination must not eat the overflow"
        );
        assert_eq!(carrier.amount(Good::Wood), 7);
        assert_eq!(nearly_full_hut.amount(Good::Wood), 1);
    }

    #[test]
    fn removal_never_underflows() {
        let mut inventory = GoodsInventory::new(12);
        inventory.add(Good::Iron, 2);
        assert_eq!(inventory.remove(Good::Iron, 99), 2);
        assert!(inventory.is_empty());
    }

    #[test]
    fn wheat_and_food_are_both_edible_without_destroying_other_goods() {
        let mut inventory = GoodsInventory::new(40);
        inventory.add(Good::Food, 2);
        inventory.add(Good::Wheat, 3);
        inventory.add(Good::Wood, 2);

        assert_eq!(inventory.edible_amount(), 5);
        assert_eq!(inventory.remove_edible(4), 4);
        assert_eq!(inventory.amount(Good::Food), 0);
        assert_eq!(inventory.amount(Good::Wheat), 1);
        assert_eq!(inventory.amount(Good::Wood), 2);
    }

    #[test]
    fn accounting_good_and_carried_appearance_are_separate() {
        let mut inventory = GoodsInventory::new(12);
        inventory.add(Good::Food, 3);
        let load = CarriedLoad::from_inventory(&inventory);

        assert_eq!(load.good, Some(Good::Food));
        assert_eq!(
            load.visible_appearance(),
            Some(CarriedAppearance::FishBasket)
        );

        let presentation_override = CarriedLoad {
            appearance: Some(CarriedAppearance::StoneBundle),
            ..load
        };
        assert_eq!(
            presentation_override.good,
            Some(Good::Food),
            "changing presentation must not change accounting"
        );
        assert_eq!(
            presentation_override.visible_appearance(),
            Some(CarriedAppearance::StoneBundle),
            "an explicit producer-specific appearance must override the fallback"
        );
    }

    #[test]
    fn market_trade_conserves_coin_and_applies_slippage() {
        let mut market = MootMarket::founding();
        market.set_targets(8, 0);
        let initial_cash = market.pool(Good::Wheat).cash;

        let sale = market.buy_from_producer(Good::Wheat, 0, 6);
        assert_eq!(sale.units, 6);
        assert_eq!(
            market.pool(Good::Wheat).cash + sale.pennies,
            initial_cash,
            "producer proceeds must come from real pool cash"
        );

        let one = {
            let mut copy = MootMarket::founding();
            copy.set_targets(8, 0);
            copy.buy_from_producer(Good::Wheat, 0, 1).pennies
        };
        assert!(
            sale.pennies < one * 6,
            "each added unit must push the bid down rather than clearing at a stale quote"
        );

        let cash_before_purchase = market.pool(Good::Wheat).cash;
        let purchase = market.sell_to_consumer(Good::Wheat, 6, 3, u64::MAX);
        assert_eq!(purchase.units, 3);
        assert_eq!(
            market.pool(Good::Wheat).cash,
            cash_before_purchase + purchase.pennies,
            "buyer spending must replenish pool cash exactly"
        );
    }

    #[test]
    fn daily_market_flow_keeps_executed_prices_separate_and_resets() {
        let mut market = MootMarket::founding();
        market.set_targets(8, 0);

        let producer_sale = market.buy_from_producer(Good::Wheat, 0, 4);
        let consumer_purchase = market.sell_to_consumer(Good::Wheat, 4, 2, u64::MAX);
        let pool = market.pool(Good::Wheat);
        assert_eq!(pool.day.producer_units, u64::from(producer_sale.units));
        assert_eq!(pool.day.producer_coin, producer_sale.pennies);
        assert_eq!(pool.day.consumer_units, u64::from(consumer_purchase.units));
        assert_eq!(pool.day.consumer_coin, consumer_purchase.pennies);
        assert!(pool.day.high_bid >= pool.day.low_bid);
        assert!(pool.day.high_ask >= pool.day.low_ask);

        let closing_bid = pool.bid;
        let closing_ask = pool.ask;
        market.begin_new_day();
        let next_day = market.pool(Good::Wheat).day;
        assert_eq!(next_day.opening_bid, closing_bid);
        assert_eq!(next_day.opening_ask, closing_ask);
        assert_eq!(next_day.producer_units, 0);
        assert_eq!(next_day.consumer_units, 0);
    }

    #[test]
    fn scarcity_and_low_liquidity_move_quotes_in_the_safe_direction() {
        let mut market = MootMarket::founding();
        market.set_targets(8, 0);
        market.refresh_quote(Good::Food, 24);
        let balanced = *market.pool(Good::Food);
        market.refresh_quote(Good::Food, 0);
        let empty = *market.pool(Good::Food);
        assert!(empty.bid > balanced.bid);
        assert!(empty.ask > balanced.ask);

        market.pool_mut(Good::Food).cash = PENNIES_PER_COIN;
        market.refresh_quote(Good::Food, 24);
        let cash_poor = *market.pool(Good::Food);
        assert!(cash_poor.bid < balanced.bid);
        assert!(cash_poor.ask > balanced.ask);
    }

    #[test]
    fn only_needed_housing_is_free_while_businesses_have_a_floor() {
        use crate::components::SettlementBuildingKind;
        assert_eq!(permit_price(SettlementBuildingKind::House, 4, true), 0);
        assert!(permit_price(SettlementBuildingKind::Farmstead, 0, true) >= PENNIES_PER_COIN);
        assert!(
            permit_price(SettlementBuildingKind::Farmstead, 2, false)
                > permit_price(SettlementBuildingKind::Farmstead, 0, true)
        );
    }
}
