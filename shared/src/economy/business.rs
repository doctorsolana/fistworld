//! Site staffing, supply, pricing, solvency and working-capital policies.

use super::{BusinessAccount, Good, MootMarket, PENNIES_PER_COIN};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

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

/// Automatic prices move slowly enough that a one-day shortage cannot produce
/// a wild oscillation when the simulation is observed at high time warp.
pub const DEFAULT_DAILY_PRICE_STEP_BPS: u16 = 500;

pub const DEFAULT_TARGET_MARGIN_BPS: u16 = 1_500;

/// Operating reserves contributed by one site to its company's consolidated
/// dividend and expansion protection. Site liabilities remain itemised so the
/// company can sum them exactly once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct BusinessWorkingCapital {
    pub payroll: u64,
    pub inputs: u64,
    pub operating_buffer: u64,
}

impl BusinessWorkingCapital {
    pub const fn total(self) -> u64 {
        self.payroll
            .saturating_add(self.inputs)
            .saturating_add(self.operating_buffer)
    }

    pub const fn total_with_liabilities(self, account: &BusinessAccount) -> u64 {
        self.total()
            .saturating_add(account.wage_arrears)
            .saturating_add(account.tax_arrears)
    }
}

/// The wage offered by one workplace, controlled by its owner.
///
/// NPC owners begin in automatic mode. Persistent vacancies push the offer up
/// when the business can cover a full staffed day; persistent arrears push it
/// down. The player-business panel can turn `automatic` off and edits this same
/// replicated `daily_wage`, so there is no second salary path.
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
    /// Owner-facing safety stock, expressed as days of this site's rated
    /// output. Company input requests always take priority over this reserve;
    /// only public market collection is held back by it.
    #[serde(default)]
    pub company_reserve_days: u8,
    /// Server-derived unit equivalent of `company_reserve_days`. Kept on the
    /// replicated policy so the client can explain the physical allocation
    /// without duplicating production-rate formulas.
    #[serde(default, alias = "keep_units")]
    pub company_reserve_units: u32,
    pub max_units_per_collection: u32,
    /// Absolute owner floor. Automatic management normally stays above its
    /// estimated sustainable price, but distress may liquidate down to this.
    pub minimum_unit_price: u64,
    #[serde(default = "default_asking_unit_price")]
    pub asking_unit_price: u64,
    #[serde(default = "default_true")]
    pub automatic_pricing: bool,
    #[serde(default = "default_target_margin_bps")]
    pub target_margin_bps: u16,
    #[serde(default = "default_daily_price_step_bps")]
    pub max_daily_price_change_bps: u16,
    #[serde(default)]
    pub days_without_sales: u16,
    #[serde(default = "default_unreviewed_day")]
    pub last_review_day: u32,
}

/// How many of a building's physical work positions the operator currently
/// advertises. The building kind remains the hard architectural maximum.
/// Keeping the target separate from the readable worker roster makes hiring
/// idempotent and lets a struggling site close positions without pretending
/// the building itself became smaller.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessStaffingPolicy {
    pub enabled_positions: u8,
}

impl BusinessStaffingPolicy {
    pub const fn new(enabled_positions: u8) -> Self {
        Self { enabled_positions }
    }

    pub fn target_for(self, kind: crate::components::SettlementBuildingKind) -> u8 {
        if self.enabled_positions < kind.positions() {
            self.enabled_positions
        } else {
            kind.positions()
        }
    }
}

impl Default for BusinessStaffingPolicy {
    fn default() -> Self {
        Self {
            enabled_positions: u8::MAX,
        }
    }
}

const fn default_true() -> bool {
    true
}

const fn default_asking_unit_price() -> u64 {
    1
}

const fn default_target_margin_bps() -> u16 {
    DEFAULT_TARGET_MARGIN_BPS
}

const fn default_daily_price_step_bps() -> u16 {
    DEFAULT_DAILY_PRICE_STEP_BPS
}

const fn default_unreviewed_day() -> u32 {
    u32::MAX
}

impl Default for BusinessSalePolicy {
    fn default() -> Self {
        Self {
            collection_enabled: true,
            company_reserve_days: 0,
            company_reserve_units: 0,
            // A cart is primarily bounded by physical bulk: this upper limit
            // lets light goods use most of it while Wood and Stone naturally
            // stop earlier. Owners can still choose a smaller dispatch.
            max_units_per_collection: 64,
            minimum_unit_price: 1,
            asking_unit_price: default_asking_unit_price(),
            automatic_pricing: true,
            target_margin_bps: DEFAULT_TARGET_MARGIN_BPS,
            max_daily_price_change_bps: DEFAULT_DAILY_PRICE_STEP_BPS,
            days_without_sales: 0,
            last_review_day: u32::MAX,
        }
    }
}

impl BusinessSalePolicy {
    pub fn for_good(good: Good) -> Self {
        Self {
            asking_unit_price: good.base_price(),
            // The authored base price seeds an unobserved market; it is not a
            // legal price floor. Automatic firms protect their live unit cost
            // in the business review, while a manual owner may deliberately
            // clear stock below cost. Keeping this at one penny lets genuine
            // competition discover a price instead of preserving 35% of an
            // arbitrary founding quote forever.
            minimum_unit_price: 1,
            ..Self::default()
        }
    }
}

/// One input which automatic management may purchase from the local market.
/// The rule is deliberately about stock and price rather than a named building
/// kind: a tavern, bakery, brewery or smithy can all use the same decision path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessInputRule {
    pub enabled: bool,
    /// Desired physical input coverage. This is the only owner-authored stock
    /// quantity; the server derives the unit target and reorder threshold from
    /// real recipe capacity and current staffing.
    #[serde(default = "default_input_coverage_days")]
    pub coverage_days: u8,
    /// Derived hysteresis threshold. It is replicated for explanation and
    /// compatibility but is never directly edited by an owner.
    pub reorder_below: u32,
    /// Derived unit equivalent of `coverage_days`.
    pub target_units: u32,
    pub maximum_unit_price: u64,
}

pub const DEFAULT_INPUT_COVERAGE_DAYS: u8 = 2;

pub const MAXIMUM_STOCK_COVERAGE_DAYS: u8 = 7;

const fn default_input_coverage_days() -> u8 {
    DEFAULT_INPUT_COVERAGE_DAYS
}

impl BusinessInputRule {
    pub fn set_coverage_days(&mut self, days: u8) {
        self.coverage_days = days.min(MAXIMUM_STOCK_COVERAGE_DAYS);
    }
}

impl Default for BusinessInputRule {
    fn default() -> Self {
        Self {
            enabled: false,
            coverage_days: DEFAULT_INPUT_COVERAGE_DAYS,
            reorder_below: 0,
            target_units: 0,
            maximum_unit_price: 0,
        }
    }
}

/// Where an input-consuming site looks first. Private supply is a sourcing
/// preference, never a compulsory market intervention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BusinessSourcingMode {
    /// Use an owned supplier when its landed value is acceptable, otherwise
    /// fall back to the public Moot market.
    #[default]
    PreferOwned,
    /// Compare owned supply and the public market before dispatching.
    CheapestAvailable,
    /// Never buy this input from an outside seller.
    OwnedOnly,
}

impl BusinessSourcingMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::PreferOwned => "Company first",
            Self::CheapestAvailable => "Best value",
            Self::OwnedOnly => "Company only",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessPrivateInputRule {
    pub enabled: bool,
    pub sourcing: BusinessSourcingMode,
    pub preferred_supplier: Option<crate::components::BuildingId>,
}

impl Default for BusinessPrivateInputRule {
    fn default() -> Self {
        Self {
            enabled: false,
            sourcing: BusinessSourcingMode::PreferOwned,
            preferred_supplier: None,
        }
    }
}

/// Private-company complement to public procurement. Keeping it separate from
/// `BusinessProcurementPolicy` preserves old saves and makes it impossible for
/// a private commitment to become a public listing accidentally.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessSupplyPolicy {
    pub automatic: bool,
    rules: [BusinessPrivateInputRule; Good::COUNT],
}

impl BusinessSupplyPolicy {
    pub const fn none() -> Self {
        Self {
            automatic: true,
            rules: [BusinessPrivateInputRule {
                enabled: false,
                sourcing: BusinessSourcingMode::PreferOwned,
                preferred_supplier: None,
            }; Good::COUNT],
        }
    }

    pub fn rule(&self, good: Good) -> BusinessPrivateInputRule {
        self.rules[good.index()]
    }

    pub fn set_rule(&mut self, good: Good, rule: BusinessPrivateInputRule) {
        self.rules[good.index()] = rule;
    }

    pub fn with_rule(mut self, good: Good, rule: BusinessPrivateInputRule) -> Self {
        self.set_rule(good, rule);
        self
    }
}

impl Default for BusinessSupplyPolicy {
    fn default() -> Self {
        Self::none()
    }
}

/// Owner policy for purchasing production inputs. No founding extractor has
/// an input requirement, but the live porter and accounting systems honour
/// these rules now so later transforming businesses need no parallel economy.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessProcurementPolicy {
    pub automatic: bool,
    rules: [BusinessInputRule; Good::COUNT],
}

impl BusinessProcurementPolicy {
    pub const fn none() -> Self {
        Self {
            automatic: true,
            rules: [BusinessInputRule {
                enabled: false,
                coverage_days: DEFAULT_INPUT_COVERAGE_DAYS,
                reorder_below: 0,
                target_units: 0,
                maximum_unit_price: 0,
            }; Good::COUNT],
        }
    }

    pub fn rule(&self, good: Good) -> BusinessInputRule {
        self.rules[good.index()]
    }

    pub fn set_rule(&mut self, good: Good, rule: BusinessInputRule) {
        self.rules[good.index()] = rule;
    }

    pub fn with_rule(mut self, good: Good, rule: BusinessInputRule) -> Self {
        self.set_rule(good, rule);
        self
    }

    pub fn needs_anything(&self) -> bool {
        self.automatic && self.rules.iter().any(|rule| rule.enabled)
    }
}

impl Default for BusinessProcurementPolicy {
    fn default() -> Self {
        Self::none()
    }
}

/// Owner decisions which are independent from a particular product.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BusinessStrategy {
    /// Ordinary margins and a three-day payroll reserve.
    #[default]
    Balanced,
    /// Lower margins, quicker price cuts and a smaller reserve to win volume.
    Growth,
    /// Protect a large margin and accept slower sales.
    HighMargin,
    /// Hold extra working cash and rescue the firm readily.
    Cautious,
    /// React strongly to shortages and accept more cash-flow risk.
    Opportunistic,
}

impl BusinessStrategy {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Balanced => "Balanced",
            Self::Growth => "Low-price growth",
            Self::HighMargin => "High margin",
            Self::Cautious => "Cautious reserves",
            Self::Opportunistic => "Opportunistic",
        }
    }

    pub const fn target_margin_bps(self) -> u16 {
        match self {
            Self::Balanced => 1_500,
            Self::Growth => 750,
            Self::HighMargin => 3_000,
            Self::Cautious => 2_000,
            Self::Opportunistic => 2_500,
        }
    }

    pub const fn payroll_reserve_days(self) -> u8 {
        match self {
            Self::Growth => 2,
            Self::Balanced | Self::HighMargin | Self::Opportunistic => 3,
            Self::Cautious => 5,
        }
    }

    /// Normal input coverage selected by an NPC Company Master. The value is
    /// deliberately small: physical deliveries and company-first reservation
    /// prevent stockouts without turning every processor into a warehouse.
    pub const fn input_coverage_days(self) -> u8 {
        match self {
            Self::Balanced | Self::Growth | Self::HighMargin => 2,
            Self::Cautious => 4,
            Self::Opportunistic => 1,
        }
    }

    pub const fn daily_price_step_bps(self) -> u16 {
        match self {
            Self::Cautious => 300,
            Self::Balanced | Self::HighMargin => 500,
            Self::Growth => 700,
            Self::Opportunistic => 900,
        }
    }

    /// How this owner initially positions a new firm's price against the
    /// observed local market. This is a private strategy choice, not a Hall
    /// discount or price control. Growth owners seek volume, balanced and
    /// cautious owners broadly match the market, high-margin owners ask more,
    /// and opportunists exploit shortages but discount a well-supplied market.
    pub const fn opening_market_position_bps(self, scarce: bool) -> u16 {
        match self {
            Self::Growth => 9_000,
            Self::Balanced | Self::Cautious => 10_000,
            Self::HighMargin => 11_500,
            Self::Opportunistic if scarce => 12_000,
            Self::Opportunistic => 9_000,
        }
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessManagementPolicy {
    /// Automatic management and player management write the same policies.
    /// Turning this off leaves asking prices, wages, procurement and draws at
    /// their player-selected values rather than creating a parallel code path.
    pub autopilot: bool,
    pub strategy: BusinessStrategy,
    pub automatic_withdrawals: bool,
    pub payroll_reserve_days: u8,
    pub max_daily_withdrawal: u64,
    pub rescue_with_personal_savings: bool,
}

impl Default for BusinessManagementPolicy {
    fn default() -> Self {
        Self {
            autopilot: true,
            strategy: BusinessStrategy::Balanced,
            automatic_withdrawals: true,
            payroll_reserve_days: 3,
            max_daily_withdrawal: 5 * PENNIES_PER_COIN,
            rescue_with_personal_savings: true,
        }
    }
}

impl BusinessManagementPolicy {
    pub fn for_strategy(strategy: BusinessStrategy) -> Self {
        Self {
            strategy,
            payroll_reserve_days: strategy.payroll_reserve_days(),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BusinessState {
    Operating,
    CashTight,
    Distressed,
    Insolvent,
    Closed,
    #[default]
    New,
    Liquidating,
    ForSale,
    /// A solvent workplace whose owner has temporarily withdrawn its labour
    /// and production. The building, stock, listings and company ownership
    /// remain intact so observed demand can reopen it without constructing a
    /// duplicate plant.
    Mothballed,
}

impl BusinessState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::New => "New",
            Self::Operating => "Operating",
            Self::CashTight => "Cash tight",
            Self::Distressed => "Distressed",
            Self::Insolvent => "Insolvent",
            Self::Liquidating => "Liquidating",
            Self::ForSale => "For sale",
            Self::Closed => "Closed",
            Self::Mothballed => "Mothballed",
        }
    }

    pub const fn can_operate(self) -> bool {
        matches!(
            self,
            Self::New | Self::Operating | Self::CashTight | Self::Distressed | Self::Insolvent
        )
    }

    pub const fn accepts_new_workers(self) -> bool {
        matches!(
            self,
            Self::New | Self::Operating | Self::CashTight | Self::Distressed
        )
    }

    pub const fn counts_as_active_capacity(self) -> bool {
        // An insolvent firm may finish or liquidate stock during its grace
        // period, but it cannot hire a worker to answer new demand. Treating
        // it as capacity makes planning and restart selection hide a genuine
        // vacancy behind an unusable shell.
        self.accepts_new_workers()
    }

    /// Existing physical capacity which can answer future demand without a
    /// fresh permit and construction project.
    pub const fn counts_as_recoverable_capacity(self) -> bool {
        matches!(self, Self::Mothballed | Self::Liquidating | Self::ForSale)
    }

    pub const fn blocks_owner_expansion(self) -> bool {
        matches!(
            self,
            Self::New
                | Self::Distressed
                | Self::Insolvent
                | Self::Liquidating
                | Self::ForSale
                | Self::Closed
        )
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessCondition {
    pub state: BusinessState,
    pub cash_tight_days: u16,
    pub insolvent_days: u16,
    pub last_review_day: u32,
    #[serde(default = "default_unreviewed_day")]
    pub opened_day: u32,
    #[serde(default)]
    pub operating_days: u16,
    #[serde(default)]
    pub liquidation_days: u16,
}

impl Default for BusinessCondition {
    fn default() -> Self {
        Self {
            state: BusinessState::New,
            cash_tight_days: 0,
            insolvent_days: 0,
            last_review_day: u32::MAX,
            opened_day: u32::MAX,
            operating_days: 0,
            liquidation_days: 0,
        }
    }
}

/// Why ownership of an existing private workplace is being offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BusinessSaleReason {
    OwnerDied,
    Insolvent,
    VoluntaryClosure,
}

impl BusinessSaleReason {
    pub const fn label(self) -> &'static str {
        match self {
            Self::OwnerDied => "Owner died",
            Self::Insolvent => "Insolvent",
            Self::VoluntaryClosure => "Owner closed business",
        }
    }
}

/// One former employee's senior claim on liquidation receipts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessWageClaim {
    pub worker: crate::components::PersonId,
    pub pennies: u64,
}

/// Bankruptcy keeps its stock, creditor claims and property alive. Goods are
/// physically carried to the local market, then receipts pay these wage claims
/// before tax debt and any owner residual.
#[derive(Component, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessLiquidation {
    pub started_day: u32,
    pub last_review_day: u32,
    pub empty_days: u16,
    pub reason: BusinessSaleReason,
    #[serde(default)]
    pub wage_claims: Vec<BusinessWageClaim>,
    /// Insolvency starts after management has already released the staff.
    /// Succession liquidation starts in the mortality pass and lets the next
    /// management review preserve claims before releasing them.
    #[serde(default = "liquidation_staff_already_released")]
    pub staff_released: bool,
}

const fn liquidation_staff_already_released() -> bool {
    true
}

impl BusinessLiquidation {
    pub fn insolvency(day: u32, wage_claims: Vec<BusinessWageClaim>) -> Self {
        Self::with_reason(day, BusinessSaleReason::Insolvent, wage_claims)
    }

    pub fn voluntary_closure(day: u32, wage_claims: Vec<BusinessWageClaim>) -> Self {
        Self::with_reason(day, BusinessSaleReason::VoluntaryClosure, wage_claims)
    }

    fn with_reason(
        day: u32,
        reason: BusinessSaleReason,
        wage_claims: Vec<BusinessWageClaim>,
    ) -> Self {
        Self {
            started_day: day,
            last_review_day: day,
            empty_days: 0,
            reason,
            wage_claims,
            staff_released: true,
        }
    }

    pub fn owner_died(day: u32) -> Self {
        Self {
            started_day: day,
            last_review_day: day,
            empty_days: 0,
            reason: BusinessSaleReason::OwnerDied,
            wage_claims: Vec::new(),
            staff_released: false,
        }
    }

    pub fn outstanding_wages(&self) -> u64 {
        self.wage_claims
            .iter()
            .map(|claim| claim.pennies)
            .fold(0, u64::saturating_add)
    }
}

/// An existing business or unfinished private worksite awaiting a new owner.
///
/// The purchase price is paid into the business as takeover capital. There is
/// no ghost seller account after a death, and a sale therefore both transfers
/// durable ownership and gives the inherited workplace a chance to meet its
/// payroll and input obligations.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessForSale {
    pub previous_owner: crate::components::PersonId,
    pub asking_price: u64,
    pub listed_day: u32,
    pub reason: BusinessSaleReason,
}

/// Newly listed property remains publicly visible for this many complete world
/// days before the automatic resident-investor system may acquire it.
pub const PROPERTY_MARKET_EXPOSURE_DAYS: u32 = 1;

/// Compute the reserve used by both NPC autopilot and business UI. Input
/// targets are priced at the current cheapest local offer, falling back to the
/// good's base value when a market has no quote yet.
pub fn business_working_capital(
    staffed_positions: u8,
    wage: &BusinessWagePolicy,
    management: &BusinessManagementPolicy,
    procurement: &BusinessProcurementPolicy,
    held_stock: Option<&[u32; Good::COUNT]>,
    market: Option<&MootMarket>,
) -> BusinessWorkingCapital {
    let payroll = wage
        .daily_wage
        .saturating_mul(u64::from(staffed_positions))
        .saturating_mul(u64::from(management.payroll_reserve_days.max(2)));
    let inputs = Good::ALL
        .into_iter()
        .filter_map(|good| {
            let rule = procurement.rule(good);
            rule.enabled.then(|| {
                let price = market.map_or(good.base_price(), |market| market.suggested_price(good));
                let held = held_stock.map_or(0, |stock| stock[good.index()]);
                u64::from(rule.target_units.saturating_sub(held)).saturating_mul(price)
            })
        })
        .fold(0u64, u64::saturating_add);
    BusinessWorkingCapital {
        payroll,
        inputs,
        operating_buffer: 2 * PENNIES_PER_COIN,
    }
}
