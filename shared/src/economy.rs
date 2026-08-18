//! Physical goods, fixed-point money and the local Moot market.
//!
//! Goods remain physical and capacity-bounded. Coin is deliberately a ledger:
//! it consumes no cargo space and every transfer has two sides. The Moot is a
//! local consignment exchange: owners set offers, buyers pay those owners at
//! the moment of purchase, and the settlement receives only its market fee.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Physical bulk goods in the settlement economy.
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
    /// Wheat milled into a household-ready baking ingredient. One Flour can
    /// become one basic ration in a cabin, but is not served directly at the
    /// Moot commons.
    Flour,
    /// Prepared bakery bread. Two Flour become four Bread, making this the
    /// first higher-efficiency (tier-two) food rather than a cosmetic rename.
    Bread,
    /// Ready-to-cook livestock food. One unit is one household ration; taverns
    /// may later turn it into a higher-value prepared meal.
    Meat,
    /// Raw fleece from livestock. It is deliberately not edible and is the
    /// first input reserved for the future spinner/weaver clothing chain.
    Wool,
}

/// Civic infrastructure required before a good may enter a settlement's
/// public order book. Physical ownership and private company transfers remain
/// legal at every tier; this controls only Hall/Marketplace trade.
///
/// The empty level-one rung is intentional with today's resource set. Future
/// crafted goods can opt into the ordinary Marketplace without changing save
/// data or scattering building checks throughout the economy.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[repr(u8)]
pub enum MarketTradeTier {
    #[default]
    Moot,
    Marketplace,
    PavedMarketplace,
}

impl MarketTradeTier {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Moot => "MOOT EXCHANGE",
            Self::Marketplace => "MARKETPLACE LEVEL 1",
            Self::PavedMarketplace => "MARKETPLACE LEVEL 2",
        }
    }

    pub const fn requirement_label(self) -> &'static str {
        match self {
            Self::Moot => "the Moot",
            Self::Marketplace => "a level 1 Marketplace",
            Self::PavedMarketplace => "a level 2 paved Marketplace",
        }
    }
}

/// One displayed coin is one hundred internal pennies.
///
/// Money never uses floating point. Prices may be derived with floating-point
/// tuning curves on the authoritative server, but every debit, credit and
/// conservation check is exact integer arithmetic.
pub const PENNIES_PER_COIN: u64 = 100;
pub const STARTING_VILLAGER_COINS: u64 = 10;
pub const STARTING_VILLAGER_MONEY: u64 = STARTING_VILLAGER_COINS * PENNIES_PER_COIN;
/// A newly created player hero gets a slightly larger testable foothold than
/// an ordinary immigrant. This is granted once with the body, never when the
/// same live hero is re-adopted after reconnecting.
pub const STARTING_HERO_COINS: u64 = 20;
pub const STARTING_HERO_MONEY: u64 = STARTING_HERO_COINS * PENNIES_PER_COIN;
pub const STARTING_TREASURY_MONEY: u64 = 20 * PENNIES_PER_COIN;
/// Public daily wage for each combined Moot Steward position.
pub const MOOT_STEWARD_DAILY_SALARY: u64 = PENNIES_PER_COIN;
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
/// Portion of a consignment sale retained by the settlement operating the
/// marketplace. Five percent is meaningful without making food taxation the
/// dominant part of its price; the enacted civic policy may move it within
/// the bounded 2–10% range.
pub const DEFAULT_MARKET_FEE_BPS: u16 = 500;
/// Balanced settlements levy only positive operating profit. Loss-making
/// firms and contributed working capital are never taxed.
pub const DEFAULT_BUSINESS_PROFIT_TAX_BPS: u16 = 1_000;
pub const DEFAULT_CIVIC_PAYROLL_RESERVE_DAYS: u8 = 7;
pub const MINIMUM_FOOD_RESERVE_TARGET_DAYS: u8 = 1;
pub const MAXIMUM_FOOD_RESERVE_TARGET_DAYS: u8 = 30;
pub const MAXIMUM_CIVIC_PAYROLL_RESERVE_DAYS: u8 = 30;
/// Balanced foundations discount a requested business permit by 45%, matching
/// the previous demand discount while making the choice explicit policy.
pub const DEFAULT_BUSINESS_PERMIT_SUBSIDY_BPS: u16 = 4_500;
pub const CIVIC_POLICY_REVIEW_DAYS: u32 = 7;
pub const MINIMUM_MARKET_FEE_BPS: u16 = 200;
pub const MAXIMUM_MARKET_FEE_BPS: u16 = 1_000;
pub const MARKET_FEE_REVIEW_STEP_BPS: u16 = 100;
pub const MAXIMUM_BUSINESS_PROFIT_TAX_BPS: u16 = 1_500;
pub const PROFIT_TAX_REVIEW_STEP_BPS: u16 = 250;
pub const MAXIMUM_BUSINESS_PERMIT_SUBSIDY_BPS: u16 = 7_500;
pub const PERMIT_SUBSIDY_REVIEW_STEP_BPS: u16 = 1_000;
pub const BASIS_POINTS: u64 = 10_000;
/// Automatic prices move slowly enough that a one-day shortage cannot produce
/// a wild oscillation when the simulation is observed at high time warp.
pub const DEFAULT_DAILY_PRICE_STEP_BPS: u16 = 500;
pub const DEFAULT_TARGET_MARGIN_BPS: u16 = 1_500;

/// Convert an observed per-unit cost into the asking price needed to retain a
/// target markup after the local market fee. Keeping this arithmetic beside the
/// shared money constants lets operating firms and prospective investors use
/// exactly the same definition of a sustainable price.
pub fn sustainable_unit_price(
    estimated_unit_cost: u64,
    market_fee_bps: u16,
    target_margin_bps: u16,
) -> u64 {
    if estimated_unit_cost == 0 {
        return 1;
    }
    let after_fee = BASIS_POINTS
        .saturating_sub(u64::from(market_fee_bps))
        .max(1);
    estimated_unit_cost
        .saturating_mul(BASIS_POINTS)
        .div_ceil(after_fee)
        .saturating_mul(BASIS_POINTS.saturating_add(u64::from(target_margin_bps)))
        .div_ceil(BASIS_POINTS)
        .max(1)
}

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

    pub const fn founding_hero() -> Self {
        Self::new(STARTING_HERO_MONEY)
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
    /// Stable identity of the resident currently responsible for provisions.
    /// Character names are presentation and are never used to find a wallet.
    #[serde(default)]
    pub shopper: Option<crate::components::PersonId>,
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

/// Exact public income and spending attributed to one world day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CivicDayLedger {
    pub day: u32,
    pub permit_income: u64,
    pub market_fee_income: u64,
    #[serde(default)]
    pub delivery_fee_income: u64,
    pub profit_tax_income: u64,
    pub public_sale_income: u64,
    pub wage_expense: u64,
    pub poor_relief_expense: u64,
    pub material_expense: u64,
    #[serde(default)]
    pub freight_expense: u64,
}

impl CivicDayLedger {
    pub const fn empty(day: u32) -> Self {
        Self {
            day,
            permit_income: 0,
            market_fee_income: 0,
            delivery_fee_income: 0,
            profit_tax_income: 0,
            public_sale_income: 0,
            wage_expense: 0,
            poor_relief_expense: 0,
            material_expense: 0,
            freight_expense: 0,
        }
    }

    pub const fn income(self) -> u64 {
        self.permit_income
            .saturating_add(self.market_fee_income)
            .saturating_add(self.delivery_fee_income)
            .saturating_add(self.profit_tax_income)
            .saturating_add(self.public_sale_income)
    }

    pub const fn spending(self) -> u64 {
        self.wage_expense
            .saturating_add(self.poor_relief_expense)
            .saturating_add(self.material_expense)
            .saturating_add(self.freight_expense)
    }
}

impl Default for CivicDayLedger {
    fn default() -> Self {
        Self::empty(u32::MAX)
    }
}

/// The treasury's auditable operating ledger. `Settlement::treasury` remains
/// the authoritative cash balance; this component explains every change and
/// supplies the Reeve's bounded review window.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CivicAccount {
    #[serde(default)]
    pub current_day: CivicDayLedger,
    #[serde(default)]
    pub previous_day: CivicDayLedger,
    #[serde(default)]
    pub income_since_review: u64,
    #[serde(default)]
    pub spending_since_review: u64,
    #[serde(default)]
    pub lifetime_income: u64,
    #[serde(default)]
    pub lifetime_spending: u64,
}

impl CivicAccount {
    pub fn roll_to_day(&mut self, day: u32) {
        if self.current_day.day == day {
            return;
        }
        if self.current_day.day != u32::MAX {
            self.previous_day = self.current_day;
        }
        self.current_day = CivicDayLedger::empty(day);
    }

    fn record_income(&mut self, pennies: u64) {
        self.income_since_review = self.income_since_review.saturating_add(pennies);
        self.lifetime_income = self.lifetime_income.saturating_add(pennies);
    }

    fn record_spending(&mut self, pennies: u64) {
        self.spending_since_review = self.spending_since_review.saturating_add(pennies);
        self.lifetime_spending = self.lifetime_spending.saturating_add(pennies);
    }

    pub fn record_permit_income(&mut self, day: u32, pennies: u64) {
        self.roll_to_day(day);
        self.current_day.permit_income = self.current_day.permit_income.saturating_add(pennies);
        self.record_income(pennies);
    }

    pub fn record_market_fee_income(&mut self, day: u32, pennies: u64) {
        self.roll_to_day(day);
        self.current_day.market_fee_income =
            self.current_day.market_fee_income.saturating_add(pennies);
        self.record_income(pennies);
    }

    pub fn record_delivery_fee_income(&mut self, day: u32, pennies: u64) {
        self.roll_to_day(day);
        self.current_day.delivery_fee_income =
            self.current_day.delivery_fee_income.saturating_add(pennies);
        self.record_income(pennies);
    }

    pub fn record_profit_tax_income(&mut self, day: u32, pennies: u64) {
        self.roll_to_day(day);
        self.current_day.profit_tax_income =
            self.current_day.profit_tax_income.saturating_add(pennies);
        self.record_income(pennies);
    }

    pub fn record_public_sale_income(&mut self, day: u32, pennies: u64) {
        self.roll_to_day(day);
        self.current_day.public_sale_income =
            self.current_day.public_sale_income.saturating_add(pennies);
        self.record_income(pennies);
    }

    pub fn record_wage_expense(&mut self, day: u32, pennies: u64) {
        self.roll_to_day(day);
        self.current_day.wage_expense = self.current_day.wage_expense.saturating_add(pennies);
        self.record_spending(pennies);
    }

    pub fn record_poor_relief_expense(&mut self, day: u32, pennies: u64) {
        self.roll_to_day(day);
        self.current_day.poor_relief_expense =
            self.current_day.poor_relief_expense.saturating_add(pennies);
        self.record_spending(pennies);
    }

    pub fn record_freight_expense(&mut self, day: u32, pennies: u64) {
        self.roll_to_day(day);
        self.current_day.freight_expense = self.current_day.freight_expense.saturating_add(pennies);
        self.record_spending(pennies);
    }

    pub fn record_material_expense(&mut self, day: u32, pennies: u64) {
        self.roll_to_day(day);
        self.current_day.material_expense =
            self.current_day.material_expense.saturating_add(pennies);
        self.record_spending(pennies);
    }

    pub fn ledger_for_day(self, day: u32) -> Option<CivicDayLedger> {
        if self.current_day.day == day {
            Some(self.current_day)
        } else if self.previous_day.day == day {
            Some(self.previous_day)
        } else {
            None
        }
    }

    pub fn close_review_window(&mut self) -> (u64, u64) {
        let result = (self.income_since_review, self.spending_since_review);
        self.income_since_review = 0;
        self.spending_since_review = 0;
        result
    }
}

/// Exact cash-basis activity in one business day.
///
/// Buying an input is an expense when the cash leaves in the first accounting
/// model. A future manufacturing-cost layer may capitalise input inventory,
/// but this honest small-business ledger already guarantees that opening
/// capital and unpaid wages can never be mistaken for owner profit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessDayLedger {
    pub day: u32,
    /// Revenue settled through the public market or another external buyer.
    pub gross_revenue: u64,
    /// Bookkeeping credit for output delivered to another site in the same
    /// company. No coin moves and consolidation eliminates this value.
    #[serde(default)]
    pub internal_revenue: u64,
    pub wage_expense: u64,
    /// Inputs purchased from an external seller for real coin.
    pub input_expense: u64,
    /// Matching bookkeeping charge for inputs received from another site in
    /// the same company.
    #[serde(default)]
    pub internal_input_expense: u64,
    pub market_fees: u64,
    #[serde(default)]
    pub delivery_fees: u64,
    #[serde(default)]
    pub profit_taxes: u64,
    pub owner_withdrawals: u64,
    /// Permit, acquisition and other long-lived asset spending. It changes
    /// cash and book value but is deliberately excluded from operating profit.
    #[serde(default)]
    pub capital_expenditures: u64,
    pub produced_units: u32,
    pub sold_units: u32,
    pub purchased_input_units: u32,
}

impl BusinessDayLedger {
    pub const fn empty(day: u32) -> Self {
        Self {
            day,
            gross_revenue: 0,
            internal_revenue: 0,
            wage_expense: 0,
            input_expense: 0,
            internal_input_expense: 0,
            market_fees: 0,
            delivery_fees: 0,
            profit_taxes: 0,
            owner_withdrawals: 0,
            capital_expenditures: 0,
            produced_units: 0,
            sold_units: 0,
            purchased_input_units: 0,
        }
    }

    pub const fn operating_expenses(self) -> u64 {
        self.wage_expense
            .saturating_add(self.input_expense)
            .saturating_add(self.internal_input_expense)
            .saturating_add(self.market_fees)
            .saturating_add(self.delivery_fees)
            .saturating_add(self.profit_taxes)
    }

    pub const fn pre_tax_expenses(self) -> u64 {
        self.wage_expense
            .saturating_add(self.input_expense)
            .saturating_add(self.internal_input_expense)
            .saturating_add(self.market_fees)
            .saturating_add(self.delivery_fees)
    }

    pub fn pre_tax_profit(self) -> u64 {
        self.gross_revenue
            .saturating_add(self.internal_revenue)
            .saturating_sub(self.pre_tax_expenses())
    }

    pub fn profit(self) -> i64 {
        signed_difference(
            self.gross_revenue.saturating_add(self.internal_revenue),
            self.operating_expenses(),
        )
    }

    /// Company-consolidated cash-basis contribution. Internal charges and
    /// credits are deliberately absent because they cancel across owned sites.
    pub fn consolidated_profit(self) -> i64 {
        signed_difference(
            self.gross_revenue,
            self.wage_expense
                .saturating_add(self.input_expense)
                .saturating_add(self.market_fees)
                .saturating_add(self.delivery_fees)
                .saturating_add(self.profit_taxes),
        )
    }
}

impl Default for BusinessDayLedger {
    fn default() -> Self {
        Self::empty(u32::MAX)
    }
}

fn signed_difference(income: u64, expense: u64) -> i64 {
    if income >= expense {
        income.saturating_sub(expense).min(i64::MAX as u64) as i64
    } else {
        -(expense.saturating_sub(income).min(i64::MAX as u64) as i64)
    }
}

/// The cost-centre ledger for one operating site.
///
/// A site records the revenue, costs and liabilities it creates so its own
/// performance remains inspectable. Spendable money belongs exclusively to
/// the operating [`CompanyAccount`], not to this building.
/// `unposted_company_capital` is a transient construction, acquisition and
/// save-migration posting field; the server sweeps it into the company
/// treasury before economic activity.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessAccount {
    #[serde(default, alias = "cash")]
    pub unposted_company_capital: u64,
    pub wage_arrears: u64,
    #[serde(default)]
    pub tax_arrears: u64,
    pub last_payroll_day: u32,
    #[serde(default)]
    pub contributed_capital: u64,
    /// Cumulative cash invested in permits/acquisitions rather than consumed
    /// as a current operating cost.
    #[serde(default)]
    pub capital_expenditures: u64,
    /// First-pass historical cost of the site's capital assets. Depreciation,
    /// land revaluation and construction-material capitalisation can extend
    /// this seam later without corrupting operating profit.
    #[serde(default)]
    pub book_value: u64,
    #[serde(default)]
    pub gross_revenue: u64,
    #[serde(default)]
    pub internal_revenue: u64,
    #[serde(default)]
    pub operating_expenses: u64,
    #[serde(default)]
    pub internal_input_expenses: u64,
    #[serde(default)]
    pub delivery_expenses: u64,
    #[serde(default)]
    pub owner_withdrawals: u64,
    #[serde(default)]
    pub defaulted_wages: u64,
    #[serde(default)]
    pub estimated_unit_cost: u64,
    #[serde(default)]
    pub current_day: BusinessDayLedger,
    #[serde(default)]
    pub previous_day: BusinessDayLedger,
    /// Tax claims that could not be recovered before a failed firm finished
    /// liquidation. This makes municipal bankruptcy losses auditable.
    #[serde(default)]
    pub defaulted_taxes: u64,
}

impl Default for BusinessAccount {
    fn default() -> Self {
        Self {
            unposted_company_capital: 0,
            wage_arrears: 0,
            tax_arrears: 0,
            last_payroll_day: u32::MAX,
            contributed_capital: 0,
            capital_expenditures: 0,
            book_value: 0,
            gross_revenue: 0,
            internal_revenue: 0,
            operating_expenses: 0,
            internal_input_expenses: 0,
            delivery_expenses: 0,
            owner_withdrawals: 0,
            defaulted_wages: 0,
            estimated_unit_cost: 0,
            current_day: BusinessDayLedger::default(),
            previous_day: BusinessDayLedger::default(),
            defaulted_taxes: 0,
        }
    }
}

impl BusinessAccount {
    pub fn with_capital(pennies: u64) -> Self {
        Self {
            unposted_company_capital: pennies,
            contributed_capital: pennies,
            ..Self::default()
        }
    }

    pub fn with_project_funding(
        opening_cash: u64,
        contributed_capital: u64,
        capital_expenditure: u64,
        funded_day: u32,
    ) -> Self {
        let mut account = Self {
            unposted_company_capital: opening_cash,
            contributed_capital,
            capital_expenditures: capital_expenditure,
            book_value: capital_expenditure,
            ..Self::default()
        };
        if capital_expenditure > 0 {
            account.roll_to_day(funded_day);
            account.current_day.capital_expenditures = capital_expenditure;
        }
        account
    }

    pub fn roll_to_day(&mut self, day: u32) {
        if self.current_day.day == day {
            return;
        }
        if self.current_day.day != u32::MAX {
            self.previous_day = self.current_day;
        }
        self.current_day = BusinessDayLedger::empty(day);
    }

    pub fn contribute_capital(&mut self, pennies: u64) {
        self.unposted_company_capital = self.unposted_company_capital.saturating_add(pennies);
        self.contributed_capital = self.contributed_capital.saturating_add(pennies);
    }

    pub fn record_production(&mut self, day: u32, units: u32) {
        self.roll_to_day(day);
        self.current_day.produced_units = self.current_day.produced_units.saturating_add(units);
    }

    /// Attribute one external consignment sale to this site. The caller moves
    /// the net proceeds into the company's single treasury.
    pub fn record_sale(&mut self, day: u32, gross: u64, fee: u64, units: u32) {
        self.roll_to_day(day);
        let fee = fee.min(gross);
        self.gross_revenue = self.gross_revenue.saturating_add(gross);
        self.operating_expenses = self.operating_expenses.saturating_add(fee);
        self.current_day.gross_revenue = self.current_day.gross_revenue.saturating_add(gross);
        self.current_day.market_fees = self.current_day.market_fees.saturating_add(fee);
        self.current_day.sold_units = self.current_day.sold_units.saturating_add(units);
    }

    /// Attribute external service income, such as a completed caravan
    /// contract, without pretending the warehouse produced or sold the cargo.
    pub fn record_service_revenue(&mut self, day: u32, pennies: u64) {
        self.roll_to_day(day);
        self.gross_revenue = self.gross_revenue.saturating_add(pennies);
        self.current_day.gross_revenue = self.current_day.gross_revenue.saturating_add(pennies);
    }

    /// Attribute a direct service sale at this site. Unlike a market
    /// consignment there is no municipal fee, but the served unit remains
    /// visible in the site's sales history and automatic management evidence.
    pub fn record_service_sale(&mut self, day: u32, pennies: u64, units: u32) {
        self.record_service_revenue(day, pennies);
        self.current_day.sold_units = self.current_day.sold_units.saturating_add(units);
    }

    /// Attribute a company-funded external input purchase to this site.
    pub fn record_input_purchase(&mut self, day: u32, pennies: u64, units: u32) {
        self.roll_to_day(day);
        self.operating_expenses = self.operating_expenses.saturating_add(pennies);
        self.current_day.input_expense = self.current_day.input_expense.saturating_add(pennies);
        self.current_day.purchased_input_units =
            self.current_day.purchased_input_units.saturating_add(units);
    }

    /// Record a same-company output transfer without changing cash. This is a
    /// site-performance credit only; the company ledger eliminates it.
    pub fn record_internal_output(&mut self, day: u32, value: u64, units: u32) {
        self.roll_to_day(day);
        self.internal_revenue = self.internal_revenue.saturating_add(value);
        self.current_day.internal_revenue = self.current_day.internal_revenue.saturating_add(value);
        self.current_day.sold_units = self.current_day.sold_units.saturating_add(units);
    }

    /// Record the receiving half of an internal goods transfer. No cash moves.
    pub fn record_internal_input(&mut self, day: u32, value: u64, units: u32) {
        self.roll_to_day(day);
        self.internal_input_expenses = self.internal_input_expenses.saturating_add(value);
        self.current_day.internal_input_expense = self
            .current_day
            .internal_input_expense
            .saturating_add(value);
        self.current_day.purchased_input_units =
            self.current_day.purchased_input_units.saturating_add(units);
    }

    /// Attribute a company-funded municipal freight charge to this site.
    pub fn record_delivery_fee(&mut self, day: u32, pennies: u64) {
        self.roll_to_day(day);
        self.operating_expenses = self.operating_expenses.saturating_add(pennies);
        self.delivery_expenses = self.delivery_expenses.saturating_add(pennies);
        self.current_day.delivery_fees = self.current_day.delivery_fees.saturating_add(pennies);
    }

    /// Wages are expenses when earned, not only if enough cash exists to pay
    /// them. This prevents an insolvent owner from withdrawing an apparent
    /// profit while employees hold unpaid claims.
    pub fn incur_wages(&mut self, day: u32, pennies: u64) {
        self.roll_to_day(day);
        self.wage_arrears = self.wage_arrears.saturating_add(pennies);
        self.operating_expenses = self.operating_expenses.saturating_add(pennies);
        self.current_day.wage_expense = self.current_day.wage_expense.saturating_add(pennies);
    }

    /// Attribute payroll to a completed shift without ever rolling an account
    /// backwards. At dawn, another system may already have opened the new
    /// trading day; in that case the earned wage belongs in `previous_day`.
    pub fn incur_completed_day_wages(&mut self, day: u32, pennies: u64) {
        if pennies == 0 {
            return;
        }
        self.wage_arrears = self.wage_arrears.saturating_add(pennies);
        self.operating_expenses = self.operating_expenses.saturating_add(pennies);
        if self.current_day.day == day {
            self.current_day.wage_expense = self.current_day.wage_expense.saturating_add(pennies);
        } else if self.previous_day.day == day {
            self.previous_day.wage_expense = self.previous_day.wage_expense.saturating_add(pennies);
        } else if self.current_day.day == u32::MAX || self.current_day.day < day {
            self.roll_to_day(day);
            self.current_day.wage_expense = self.current_day.wage_expense.saturating_add(pennies);
        } else {
            // Only two daily ledgers are retained. If simulation resumed after
            // a gap, keep today's ledger intact and use the bounded completed-
            // day slot for the catch-up payroll.
            self.previous_day = BusinessDayLedger::empty(day);
            self.previous_day.wage_expense = pennies;
        }
    }

    pub fn settle_wage_claim(&mut self, pennies: u64) -> u64 {
        let paid = pennies.min(self.wage_arrears);
        self.wage_arrears -= paid;
        paid
    }

    /// Close an unpayable wage claim without pretending cash changed hands.
    /// Defaults remove the liability and remain visible in business history;
    /// only a real company-treasury payment may settle the claim.
    pub fn write_off_wage_claim(&mut self, pennies: u64) -> u64 {
        let written_off = pennies.min(self.wage_arrears);
        self.wage_arrears -= written_off;
        self.defaulted_wages = self.defaulted_wages.saturating_add(written_off);
        written_off
    }

    /// Profit tax is assessed only after the completed day's wages, input
    /// purchases and market charges are known. It becomes a real liability
    /// before owner withdrawals are considered.
    pub fn incur_profit_tax(&mut self, day: u32, pennies: u64) {
        if pennies == 0 {
            return;
        }
        self.roll_to_day(day);
        self.tax_arrears = self.tax_arrears.saturating_add(pennies);
        self.operating_expenses = self.operating_expenses.saturating_add(pennies);
        self.current_day.profit_taxes = self.current_day.profit_taxes.saturating_add(pennies);
    }

    /// Assess a levy against a day which has already rolled into the bounded
    /// previous-day slot. This avoids rolling an account backwards when dawn
    /// payroll has already opened the new business day.
    pub fn incur_completed_day_profit_tax(&mut self, completed_day: u32, pennies: u64) {
        if pennies == 0 {
            return;
        }
        self.tax_arrears = self.tax_arrears.saturating_add(pennies);
        self.operating_expenses = self.operating_expenses.saturating_add(pennies);
        if self.current_day.day == completed_day {
            self.current_day.profit_taxes = self.current_day.profit_taxes.saturating_add(pennies);
        } else if self.previous_day.day == completed_day {
            self.previous_day.profit_taxes = self.previous_day.profit_taxes.saturating_add(pennies);
        }
    }

    pub const fn ledger_for_day(self, day: u32) -> Option<BusinessDayLedger> {
        if self.current_day.day == day {
            Some(self.current_day)
        } else if self.previous_day.day == day {
            Some(self.previous_day)
        } else {
            None
        }
    }

    pub fn settle_tax_claim(&mut self, pennies: u64) -> u64 {
        let paid = pennies.min(self.tax_arrears);
        self.tax_arrears -= paid;
        paid
    }

    pub fn lifetime_profit(self) -> i64 {
        signed_difference(self.gross_revenue, self.operating_expenses)
    }

    pub fn lifetime_site_profit(self) -> i64 {
        signed_difference(
            self.gross_revenue.saturating_add(self.internal_revenue),
            self.operating_expenses
                .saturating_add(self.internal_input_expenses),
        )
    }

    pub fn retained_profit(self) -> u64 {
        self.gross_revenue
            .saturating_sub(self.operating_expenses)
            .saturating_sub(self.owner_withdrawals)
    }

    /// Attribute a dividend to the company's books without pretending a
    /// particular building paid it. Cash is debited on [`CompanyAccount`].
    pub fn record_company_dividend(&mut self, day: u32, pennies: u64) {
        if pennies == 0 {
            return;
        }
        self.roll_to_day(day);
        self.owner_withdrawals = self.owner_withdrawals.saturating_add(pennies);
        self.current_day.owner_withdrawals =
            self.current_day.owner_withdrawals.saturating_add(pennies);
    }
}

/// Consolidated cash-basis activity for one company day. Site-internal credits
/// and charges are retained as an auditable memorandum but excluded from
/// operating profit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompanyDayLedger {
    pub day: u32,
    pub external_revenue: u64,
    pub wage_expense: u64,
    pub external_input_expense: u64,
    pub market_fees: u64,
    pub delivery_fees: u64,
    pub profit_taxes: u64,
    pub owner_withdrawals: u64,
    pub capital_expenditures: u64,
    pub internal_revenue: u64,
    pub internal_input_expense: u64,
}

impl CompanyDayLedger {
    pub const fn empty(day: u32) -> Self {
        Self {
            day,
            external_revenue: 0,
            wage_expense: 0,
            external_input_expense: 0,
            market_fees: 0,
            delivery_fees: 0,
            profit_taxes: 0,
            owner_withdrawals: 0,
            capital_expenditures: 0,
            internal_revenue: 0,
            internal_input_expense: 0,
        }
    }

    pub const fn operating_costs(self) -> u64 {
        self.wage_expense
            .saturating_add(self.external_input_expense)
            .saturating_add(self.market_fees)
            .saturating_add(self.delivery_fees)
            .saturating_add(self.profit_taxes)
    }

    pub fn profit(self) -> i64 {
        signed_difference(self.external_revenue, self.operating_costs())
    }

    pub fn pre_tax_profit(self) -> u64 {
        self.external_revenue.saturating_sub(
            self.wage_expense
                .saturating_add(self.external_input_expense)
                .saturating_add(self.market_fees)
                .saturating_add(self.delivery_fees),
        )
    }
}

impl Default for CompanyDayLedger {
    fn default() -> Self {
        Self::empty(u32::MAX)
    }
}

/// Consolidated company finance and liabilities. `cash` is the one spendable
/// treasury used by every site. Buildings retain only cost-centre ledgers.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CompanyAccount {
    pub cash: u64,
    pub wage_arrears: u64,
    pub tax_arrears: u64,
    pub contributed_capital: u64,
    pub capital_expenditures: u64,
    pub book_value: u64,
    pub owner_withdrawals: u64,
    pub current_day: CompanyDayLedger,
    pub previous_day: CompanyDayLedger,
}

impl CompanyAccount {
    pub fn credit(&mut self, pennies: u64) {
        self.cash = self.cash.saturating_add(pennies);
    }

    pub fn debit(&mut self, pennies: u64) -> bool {
        if self.cash < pennies {
            return false;
        }
        self.cash -= pennies;
        true
    }

    pub fn roll_to_day(&mut self, day: u32) {
        if self.current_day.day == day {
            return;
        }
        if self.current_day.day != u32::MAX {
            self.previous_day = self.current_day;
        }
        self.current_day = CompanyDayLedger::empty(day);
    }

    pub fn refresh_from_sites(
        &mut self,
        day: u32,
        wage_arrears: u64,
        tax_arrears: u64,
        contributed_capital: u64,
        capital_expenditures: u64,
        book_value: u64,
        owner_withdrawals: u64,
        current_ledger: CompanyDayLedger,
        completed_ledger: Option<CompanyDayLedger>,
    ) {
        self.roll_to_day(day);
        self.wage_arrears = wage_arrears;
        self.tax_arrears = tax_arrears;
        // Company formation and later shareholder contributions are posted
        // directly to the legal treasury before a site may exist. Site books
        // still carry legacy/project attribution, so consolidation may raise
        // this lifetime total but must never erase already-recorded capital.
        self.contributed_capital = self.contributed_capital.max(contributed_capital);
        self.capital_expenditures = capital_expenditures;
        self.book_value = book_value;
        self.owner_withdrawals = owner_withdrawals;
        self.current_day = current_ledger;
        if let Some(completed_ledger) = completed_ledger {
            self.previous_day = completed_ledger;
        }
    }
}

/// Decisions applying to the whole legal company rather than one operating
/// site. Individual sites still control their own product, price, wage offer,
/// stock retention and input route.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompanyManagementPolicy {
    pub strategy: BusinessStrategy,
    pub autopilot: bool,
    pub automatic_dividends: bool,
    pub max_daily_dividend: u64,
    pub payroll_reserve_days: u8,
    pub last_review_day: u32,
    pub last_dividend_day: u32,
}

/// One resource decision for one company's operations in one settlement.
///
/// The quantity is intentionally an absolute physical unit count. A company
/// may own sites in many settlements, but goods never teleport between them:
/// each local branch protects its own stock before making any of that stock
/// available to the local public market.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompanyResourcePolicy {
    pub retain_units: u32,
    pub sell_excess: bool,
}

impl Default for CompanyResourcePolicy {
    fn default() -> Self {
        Self {
            retain_units: 0,
            sell_excess: true,
        }
    }
}

impl CompanyResourcePolicy {
    /// Units that may enter the public market after protecting the branch's
    /// absolute reserve and shipments already promised to a porter.
    pub const fn public_surplus(self, physically_held: u32, reserved: u32) -> u32 {
        if self.sell_excess {
            physically_held
                .saturating_sub(self.retain_units)
                .saturating_sub(reserved)
        } else {
            0
        }
    }
}

/// The local operating policy of a company in one settlement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompanyBranchPolicy {
    pub settlement: crate::components::SettlementId,
    resources: [CompanyResourcePolicy; Good::COUNT],
}

impl CompanyBranchPolicy {
    pub const fn new(settlement: crate::components::SettlementId) -> Self {
        Self {
            settlement,
            resources: [CompanyResourcePolicy {
                retain_units: 0,
                sell_excess: true,
            }; Good::COUNT],
        }
    }

    pub fn resource(&self, good: Good) -> CompanyResourcePolicy {
        self.resources[good.index()]
    }

    pub fn set_resource(&mut self, good: Good, policy: CompanyResourcePolicy) {
        self.resources[good.index()] = policy;
    }
}

/// Sparse company-wide directory of local branches. Cash and accounting live
/// on [`CompanyAccount`]; this component governs only physical stock located
/// in a particular settlement.
#[derive(Component, Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CompanyBranchPolicies {
    branches: Vec<CompanyBranchPolicy>,
}

impl CompanyBranchPolicies {
    pub fn branches(&self) -> &[CompanyBranchPolicy] {
        &self.branches
    }

    pub fn branch(
        &self,
        settlement: crate::components::SettlementId,
    ) -> Option<&CompanyBranchPolicy> {
        self.branches
            .iter()
            .find(|branch| branch.settlement == settlement)
    }

    pub fn resource(
        &self,
        settlement: crate::components::SettlementId,
        good: Good,
    ) -> CompanyResourcePolicy {
        self.branch(settlement)
            .map_or_else(CompanyResourcePolicy::default, |branch| {
                branch.resource(good)
            })
    }

    /// Record that the company operates in this settlement even when every
    /// resource still uses its default policy. The legal/economic simulation
    /// also uses this sparse directory as the company's last known local seat
    /// when winding up an ownerless empty shell.
    pub fn ensure_branch(&mut self, settlement: crate::components::SettlementId) {
        if self.branch(settlement).is_some() {
            return;
        }
        self.branches.push(CompanyBranchPolicy::new(settlement));
        self.branches
            .sort_unstable_by_key(|branch| branch.settlement);
    }

    pub fn set_resource(
        &mut self,
        settlement: crate::components::SettlementId,
        good: Good,
        policy: CompanyResourcePolicy,
    ) {
        self.ensure_branch(settlement);
        let index = self
            .branches
            .iter()
            .position(|branch| branch.settlement == settlement)
            .expect("branch was ensured above");
        self.branches[index].set_resource(good, policy);
    }
}

impl Default for CompanyManagementPolicy {
    fn default() -> Self {
        let strategy = BusinessStrategy::Balanced;
        Self {
            strategy,
            autopilot: true,
            automatic_dividends: true,
            max_daily_dividend: 2 * PENNIES_PER_COIN,
            payroll_reserve_days: strategy.payroll_reserve_days(),
            last_review_day: u32::MAX,
            last_dividend_day: u32::MAX,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompanyDecisionReason {
    FinancialStress,
    ProfitableExpansion,
    StrongMarketPosition,
    NormalisedOperations,
}

impl CompanyDecisionReason {
    pub const fn label(self) -> &'static str {
        match self {
            Self::FinancialStress => "liabilities or distressed sites required larger reserves",
            Self::ProfitableExpansion => "strong consolidated profit supported lower-margin growth",
            Self::StrongMarketPosition => {
                "strong sales and the Master's aptitude supported firmer margins"
            }
            Self::NormalisedOperations => "ordinary trading conditions favoured a balanced posture",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompanyDecisionRecord {
    pub day: u32,
    pub master: crate::components::PersonId,
    pub from: BusinessStrategy,
    pub to: BusinessStrategy,
    pub reason: CompanyDecisionReason,
}

/// Bounded executive audit trail. It records infrequent daily decisions, not
/// per-tick thought state, so thousands of companies remain cheap.
#[derive(Component, Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CompanyDecisionHistory {
    entries: Vec<CompanyDecisionRecord>,
}

impl CompanyDecisionHistory {
    pub const CAPACITY: usize = 32;

    pub fn entries(&self) -> &[CompanyDecisionRecord] {
        &self.entries
    }

    pub fn push(&mut self, record: CompanyDecisionRecord) {
        if self.entries.len() == Self::CAPACITY {
            self.entries.remove(0);
        }
        self.entries.push(record);
    }
}

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

/// Simultaneous guest places in the founding Tavern blockout. Throughput is
/// separately limited by Innkeepers and ingredients, so this is a physical
/// presentation bound rather than free production capacity.
pub const TAVERN_GUEST_CAPACITY: u8 = 8;
/// One Innkeeper can serve this many paid meals in an ordinary day. The value
/// is deliberately modest: another busy Tavern or another hired position can
/// emerge from demand instead of one counter serving an entire Town.
pub const TAVERN_MEALS_PER_INNKEEPER_DAY: u32 = 8;

/// Inspectable demand and sales evidence for one Tavern day.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TavernServiceDay {
    pub day: u32,
    pub planned_visits: u32,
    pub served_meals: u32,
    pub bread_used: u32,
    pub meat_used: u32,
    pub unaffordable_visits: u32,
    pub unavailable_visits: u32,
    pub route_failures: u32,
    pub revenue: u64,
}

impl TavernServiceDay {
    pub const fn empty(day: u32) -> Self {
        Self {
            day,
            planned_visits: 0,
            served_meals: 0,
            bread_used: 0,
            meat_used: 0,
            unaffordable_visits: 0,
            unavailable_visits: 0,
            route_failures: 0,
            revenue: 0,
        }
    }

    pub const fn unmet_visits(self) -> u32 {
        self.unaffordable_visits
            .saturating_add(self.unavailable_visits)
            .saturating_add(self.route_failures)
    }
}

/// The small replicated service board for a private Tavern.
///
/// Meal price remains in [`BusinessSalePolicy`], meaning player and NPC
/// Company Masters use the same manual/automatic pricing path. This component
/// records capacity and evidence; it never mints goods or money.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TavernService {
    pub guest_capacity: u8,
    pub innkeepers_on_duty: u8,
    pub current_guests: u8,
    pub current_day: TavernServiceDay,
    pub previous_day: TavernServiceDay,
}

impl Default for TavernService {
    fn default() -> Self {
        Self {
            guest_capacity: TAVERN_GUEST_CAPACITY,
            innkeepers_on_duty: 0,
            current_guests: 0,
            current_day: TavernServiceDay::empty(u32::MAX),
            previous_day: TavernServiceDay::empty(u32::MAX),
        }
    }
}

impl TavernService {
    pub fn roll_to_day(&mut self, day: u32) {
        if self.current_day.day == day {
            return;
        }
        if self.current_day.day != u32::MAX {
            self.previous_day = self.current_day;
        }
        self.current_day = TavernServiceDay::empty(day);
        self.current_guests = 0;
    }

    pub fn record_planned_visit(&mut self, day: u32) {
        self.roll_to_day(day);
        self.current_day.planned_visits = self.current_day.planned_visits.saturating_add(1);
    }

    pub fn record_meal(&mut self, day: u32, ingredient: Good, price: u64) {
        self.roll_to_day(day);
        self.current_day.served_meals = self.current_day.served_meals.saturating_add(1);
        self.current_day.revenue = self.current_day.revenue.saturating_add(price);
        match ingredient {
            Good::Bread => {
                self.current_day.bread_used = self.current_day.bread_used.saturating_add(1)
            }
            Good::Meat => self.current_day.meat_used = self.current_day.meat_used.saturating_add(1),
            _ => {}
        }
    }

    pub const fn daily_capacity(self) -> u32 {
        self.innkeepers_on_duty as u32 * TAVERN_MEALS_PER_INNKEEPER_DAY
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

/// The authored object shown in a character's arms.
///
/// This is deliberately separate from [`Good`]. Accounting can keep one Food
/// category while a fisherman carries a fish basket and a baker carries bread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CarriedAppearance {
    FishBasket,
    WheatSheaf,
    WoodBundle,
    StoneBundle,
    IronBundle,
    FlourSack,
    BreadBasket,
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
            Good::Flour => Self::FlourSack,
            Good::Bread => Self::BreadBasket,
            // Dedicated carried art can replace these founding placeholders
            // without changing the inventory or trade contract.
            Good::Meat => Self::FishBasket,
            Good::Wool => Self::FlourSack,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::FishBasket => "Fish basket",
            Self::WheatSheaf => "Wheat sheaf",
            Self::WoodBundle => "Wood bundle",
            Self::StoneBundle => "Stone bundle",
            Self::IronBundle => "Iron bundle",
            Self::FlourSack => "Flour sack",
            Self::BreadBasket => "Bread basket",
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

/// Replicated presentation state for an embodied porter trip.
///
/// The server's [`GoodsInventory`] remains the cargo authority. Presence says
/// the character is currently hauling the handcart (including an empty
/// outbound leg); `load_slots` is the bounded 0..=2 visual summary consumed by
/// the two authored `Anchor_Load.*` nodes.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PorterCartState {
    pub load_slots: u8,
}

impl PorterCartState {
    pub const fn for_used_bulk(used_bulk: u32) -> Self {
        Self {
            load_slots: if used_bulk == 0 {
                0
            } else if used_bulk <= capacity::PORTER / 2 {
                1
            } else {
                2
            },
        }
    }
}

impl Good {
    /// Existing discriminants stay in their original order for replicated and
    /// saved data; new goods are appended.
    pub const ALL: [Self; 9] = [
        Self::Food,
        Self::Wheat,
        Self::Wood,
        Self::Stone,
        Self::Iron,
        Self::Flour,
        Self::Bread,
        Self::Meat,
        Self::Wool,
    ];
    pub const COUNT: usize = Self::ALL.len();

    /// Household pantries consume better prepared food first. Flour is last:
    /// it becomes an ordinary ration only through home baking.
    pub const HOUSEHOLD_FOOD_PRIORITY: [Self; 4] =
        [Self::Bread, Self::Meat, Self::Food, Self::Flour];

    /// Food which can be handed to an unhoused resident and eaten in the Moot
    /// commons. Flour deliberately is not on this list.
    pub const READY_TO_EAT_PRIORITY: [Self; 3] = [Self::Bread, Self::Meat, Self::Food];

    /// Physical inputs a future Tavern may procure. Wheat remains the founding
    /// ale grain even though raw Wheat is not a household ration.
    pub const TAVERN_INPUTS: [Self; 3] = [Self::Meat, Self::Bread, Self::Wheat];

    /// Minimum public exchange able to list and clear this good. All current
    /// founding resources remain Moot-tradeable; Iron is the first specialist
    /// commodity reserved for a level-two Marketplace.
    pub const fn minimum_market_tier(self) -> MarketTradeTier {
        match self {
            Self::Iron => MarketTradeTier::PavedMarketplace,
            Self::Food
            | Self::Wheat
            | Self::Wood
            | Self::Stone
            | Self::Flour
            | Self::Bread
            | Self::Meat
            | Self::Wool => MarketTradeTier::Moot,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            // The original discriminant remains `Food` for save/network
            // stability, but its only physical producer is fishing.
            Self::Food => "Fish",
            Self::Wheat => "Wheat",
            Self::Wood => "Wood",
            Self::Stone => "Stone",
            Self::Iron => "Iron",
            Self::Flour => "Flour",
            Self::Bread => "Bread",
            Self::Meat => "Meat",
            Self::Wool => "Wool",
        }
    }

    /// Whether one unit can satisfy one resident's daily food ration.
    ///
    /// Flour is edible only through a household pantry; direct meal services
    /// must additionally require [`Self::is_ready_to_eat`]. Raw Wheat is never
    /// food after the milling chain was introduced.
    pub const fn is_edible(self) -> bool {
        matches!(self, Self::Food | Self::Flour | Self::Bread | Self::Meat)
    }

    pub const fn is_ready_to_eat(self) -> bool {
        matches!(self, Self::Food | Self::Bread | Self::Meat)
    }

    /// Bread is the first tier-two food. The tier is inspectable now and can
    /// later feed preferences, health and migration without identifying foods
    /// by display string.
    pub const fn food_tier(self) -> u8 {
        match self {
            Self::Food | Self::Flour | Self::Meat => 1,
            Self::Bread => 2,
            Self::Wheat | Self::Wood | Self::Stone | Self::Iron | Self::Wool => 0,
        }
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
            Self::Flour | Self::Bread => 1,
            Self::Meat => 1,
            Self::Wool => 2,
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
            Self::Flour => 120,
            Self::Bread => 180,
            Self::Meat => 140,
            Self::Wool => 90,
        }
    }

    pub const fn index(self) -> usize {
        match self {
            Self::Food => 0,
            Self::Wheat => 1,
            Self::Wood => 2,
            Self::Stone => 3,
            Self::Iron => 4,
            Self::Flour => 5,
            Self::Bread => 6,
            Self::Meat => 7,
            Self::Wool => 8,
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

/// Cash an established company may commit to expansion without spending its
/// employee/tax liabilities or enacted company-wide payroll runway.
pub fn company_expansion_cash(company: &CompanyAccount, protected_payroll: u64) -> u64 {
    company
        .cash
        .saturating_sub(company.wage_arrears)
        .saturating_sub(company.tax_arrears)
        .saturating_sub(protected_payroll)
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
    #[serde(default)]
    pub unavailable_units: u64,
    #[serde(default)]
    pub unaffordable_units: u64,
    #[serde(default)]
    pub funded_unmet_units: u64,
    pub closing_stock: u32,
    pub target_stock: u32,
    pub listed_units: u32,
}

impl MarketGoodHistoryDay {
    pub fn average_producer_price(self) -> Option<u64> {
        (self.producer_units > 0).then(|| self.producer_coin / self.producer_units)
    }

    pub fn average_consumer_price(self) -> Option<u64> {
        (self.consumer_units > 0).then(|| self.consumer_coin / self.consumer_units)
    }

    pub const fn coin_volume(self) -> u64 {
        self.consumer_coin
    }

    pub const fn unmet_units(self) -> u64 {
        self.unavailable_units
            .saturating_add(self.unaffordable_units)
    }
}

/// One completed daily reading of a settlement's money, goods and welfare.
///
/// `total_local_coin` is the conservation-oriented measure: civic treasury +
/// personal wallets + household purses + each locally active company's treasury
/// counted once. Produced goods are tracked separately and valued at that day's
/// last completed sale price. `business_cash` retains its historical wire name but
/// contains company treasury cash, never per-building wallets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettlementHistoryDay {
    pub day: u32,
    pub market: [MarketGoodHistoryDay; Good::COUNT],
    pub civic_treasury: u64,
    pub resident_wallet_money: u64,
    pub household_cash: u64,
    pub business_cash: u64,
    #[serde(default)]
    pub business_wage_arrears: u64,
    #[serde(default)]
    pub business_tax_arrears: u64,
    pub civic_wage_arrears: u64,
    #[serde(default)]
    pub civic: CivicHistoryDay,
    pub physical_stock: [u32; Good::COUNT],
    pub stock_liquidation_value: u64,
    pub total_local_coin: u64,
    pub population: u32,
    pub employed: u32,
    pub hungry: u32,
    #[serde(default)]
    pub job_seekers: u32,
    #[serde(default)]
    pub homeless: u32,
    #[serde(default)]
    pub unpaid_workers: u32,
    #[serde(default)]
    pub unrest: f32,
    #[serde(default)]
    pub unrest_target: f32,
    pub food_reserves: u32,
    #[serde(default)]
    pub purchasable_food: u32,
    #[serde(default)]
    pub unlisted_business_food: u32,
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

/// Daily municipal accounts and the enacted rules which produced them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CivicHistoryDay {
    pub observed: bool,
    pub permit_income: u64,
    pub market_fee_income: u64,
    #[serde(default)]
    pub delivery_fee_income: u64,
    pub profit_tax_income: u64,
    pub public_sale_income: u64,
    pub wage_expense: u64,
    pub poor_relief_expense: u64,
    pub material_expense: u64,
    #[serde(default)]
    pub freight_expense: u64,
    pub filled_positions: u16,
    pub vacant_positions: u16,
    pub market_fee_bps: u16,
    pub business_profit_tax_bps: u16,
    pub poor_relief: crate::components::PoorReliefMode,
    pub food_reserve_target_days: u8,
    pub civic_payroll_reserve_days: u8,
    pub staffing_posture: crate::components::CivicStaffingPosture,
    pub business_permit_subsidy_bps: u16,
    pub strategy: crate::components::CivicStrategy,
    pub autopilot: bool,
    pub adjustment: crate::components::CivicPolicyAdjustment,
    pub reason: crate::components::CivicPolicyReason,
}

/// Up to one in-game year of session history for a single settlement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SettlementHistoryArchive {
    pub settlement: String,
    /// Oldest to newest, never longer than [`SETTLEMENT_HISTORY_DAYS`].
    pub days: Vec<SettlementHistoryDay>,
    /// Individually addressable firm histories in this settlement. These are
    /// pulled with the settlement archive rather than continuously replicated.
    #[serde(default)]
    pub businesses: Vec<BusinessHistoryArchive>,
}

/// One completed daily reading of an individual private business.
///
/// Prices, wages, strategy and solvency are stored beside the P&L so a debug
/// view can explain *why* profit changed rather than showing only the result.
/// `observed` is false only when a developer jumps across multiple calendar
/// days without simulating their intervening boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct BusinessHistoryDay {
    pub day: u32,
    pub observed: bool,
    pub cash: u64,
    #[serde(default)]
    pub protected_working_capital: u64,
    #[serde(default)]
    pub withdrawable_profit: u64,
    pub wage_arrears: u64,
    pub tax_arrears: u64,
    pub gross_revenue: u64,
    #[serde(default)]
    pub internal_revenue: u64,
    pub wage_expense: u64,
    pub input_expense: u64,
    #[serde(default)]
    pub internal_input_expense: u64,
    pub market_fees: u64,
    #[serde(default)]
    pub delivery_fees: u64,
    pub profit_taxes: u64,
    pub owner_withdrawals: u64,
    #[serde(default)]
    pub capital_expenditures: u64,
    #[serde(default)]
    pub book_value: u64,
    pub profit: i64,
    pub produced_units: u32,
    pub sold_units: u32,
    pub purchased_input_units: u32,
    pub workplace_stock: [u32; Good::COUNT],
    pub listed_output_units: u32,
    pub asking_unit_price: u64,
    pub daily_wage: u64,
    pub strategy: BusinessStrategy,
    pub autopilot: bool,
    pub state: BusinessState,
}

/// Up to one in-game year for one stable business identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessHistoryArchive {
    pub id: crate::components::BuildingId,
    pub settlement: crate::components::SettlementId,
    #[serde(default)]
    pub company_id: Option<crate::components::CompanyId>,
    pub kind: crate::components::SettlementBuildingKind,
    pub owner_id: Option<crate::components::PersonId>,
    pub owner_name: Option<String>,
    pub output_good: Option<Good>,
    /// Oldest to newest, never longer than [`SETTLEMENT_HISTORY_DAYS`].
    pub days: Vec<BusinessHistoryDay>,
}

/// Pull-based, cross-settlement history for one legal company.
///
/// The archive intentionally carries the underlying site ledgers instead of a
/// second persisted accounting format. The client can therefore show both a
/// consolidated company result (with internal transfers eliminated) and the
/// exact sites which produced it. It is requested only while a player opens a
/// company ledger; ordinary replication remains current-state only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompanyHistoryArchive {
    pub company: crate::components::CompanyId,
    pub businesses: Vec<BusinessHistoryArchive>,
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
    pub resident_wallet_money: u64,
    pub household_cash: u64,
    /// Sum of authoritative company treasuries. The field keeps its legacy wire
    /// name so existing replicated history remains compatible.
    pub business_cash: u64,
    #[serde(default)]
    pub business_wage_arrears: u64,
    #[serde(default)]
    pub business_tax_arrears: u64,
    pub civic_wage_arrears: u64,
    pub total_local_coin: u64,
    pub stock_liquidation_value: u64,
    pub physical_stock: [u32; Good::COUNT],
    pub food_reserves: u32,
    #[serde(default)]
    pub purchasable_food: u32,
    #[serde(default)]
    pub unlisted_business_food: u32,
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

/// Automatic permit price. Housing is civic approval and remains free;
/// every business permit has a positive floor.
pub fn permit_price(
    kind: crate::components::SettlementBuildingKind,
    applicant_holdings: usize,
    needed_by_settlement: bool,
) -> u64 {
    permit_price_with_subsidy(
        kind,
        applicant_holdings,
        needed_by_settlement,
        DEFAULT_BUSINESS_PERMIT_SUBSIDY_BPS,
    )
}

/// Automatic permit price under the settlement's enacted growth subsidy.
/// Only requested private businesses receive the discount. Housing remains
/// free, civic projects remain public and speculative firms pay full price.
pub fn permit_price_with_subsidy(
    kind: crate::components::SettlementBuildingKind,
    applicant_holdings: usize,
    needed_by_settlement: bool,
    subsidy_bps: u16,
) -> u64 {
    use crate::components::SettlementBuildingKind;
    if kind == SettlementBuildingKind::House {
        return 0;
    }
    if kind == SettlementBuildingKind::Hall {
        return u64::MAX;
    }
    // Turning raw Wheat into the settlement's first edible grain supply is
    // emergency infrastructure. When the opportunity board explicitly asks
    // for a Windmill, the Hall waives the land-use fee rather than taking the
    // processor's scarce opening input cash. Speculative mills still pay.
    if kind == SettlementBuildingKind::Windmill && needed_by_settlement {
        return 0;
    }
    let base: u64 = match kind {
        SettlementBuildingKind::Farmstead
        | SettlementBuildingKind::FishermansHut
        | SettlementBuildingKind::LivestockFarm => 300,
        SettlementBuildingKind::LumberjackHut => 250,
        SettlementBuildingKind::StoneQuarry => 350,
        SettlementBuildingKind::Windmill | SettlementBuildingKind::Bakery => 250,
        SettlementBuildingKind::StorageHall => 400,
        // Civic blockouts are settlement-requested progression infrastructure.
        // Their physical wood still has to be supplied; pricing can be revisited
        // with the wider ownership model without creating a progression lock.
        SettlementBuildingKind::Market
        | SettlementBuildingKind::Tavern
        | SettlementBuildingKind::Church => return 0,
        SettlementBuildingKind::House | SettlementBuildingKind::Hall => unreachable!(),
    };
    let holdings_multiplier_bps =
        BASIS_POINTS.saturating_add((applicant_holdings as u64).saturating_mul(BASIS_POINTS / 2));
    let need_multiplier_bps = if needed_by_settlement {
        BASIS_POINTS.saturating_sub(u64::from(
            subsidy_bps.min(MAXIMUM_BUSINESS_PERMIT_SUBSIDY_BPS),
        ))
    } else {
        BASIS_POINTS
    };
    base.saturating_mul(holdings_multiplier_bps)
        .saturating_mul(need_multiplier_bps)
        .div_ceil(BASIS_POINTS.saturating_mul(BASIS_POINTS))
        .max(PENNIES_PER_COIN)
}

/// Player-facing permit price, including privately commissioned amenities.
///
/// Automatic civic projects still use [`permit_price_with_subsidy`] and cost
/// their Reeve nothing. A player who chooses to own the same unlocked service
/// building pays a real land-use fee; demand may discount it but can never
/// decide whether the permit is legal.
pub fn player_permit_price_with_subsidy(
    kind: crate::components::SettlementBuildingKind,
    applicant_holdings: usize,
    requested_by_settlement: bool,
    subsidy_bps: u16,
) -> u64 {
    use crate::components::SettlementBuildingKind;
    let base: u64 = match kind {
        SettlementBuildingKind::Market => 500,
        SettlementBuildingKind::Tavern => 400,
        SettlementBuildingKind::Church => 600,
        _ => {
            return permit_price_with_subsidy(
                kind,
                applicant_holdings,
                requested_by_settlement,
                subsidy_bps,
            );
        }
    };
    let holdings_multiplier_bps =
        BASIS_POINTS.saturating_add((applicant_holdings as u64).saturating_mul(BASIS_POINTS / 2));
    let demand_multiplier_bps = if requested_by_settlement {
        BASIS_POINTS.saturating_sub(u64::from(
            subsidy_bps.min(MAXIMUM_BUSINESS_PERMIT_SUBSIDY_BPS),
        ))
    } else {
        BASIS_POINTS
    };
    base.saturating_mul(holdings_multiplier_bps)
        .saturating_mul(demand_multiplier_bps)
        .div_ceil(BASIS_POINTS.saturating_mul(BASIS_POINTS))
        .max(PENNIES_PER_COIN)
}

/// Server-owned bounded storage for bulk goods.
///
/// Carried loads, workplaces and houses share one physical bulk allowance.
/// Public market stores can instead opt into equal independent compartments:
/// a full Wood bay then cannot consume the space reserved for Bread or Flour.
/// The fixed arrays keep the strategic tick cheap and serialization stable.
#[derive(Component, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoodsInventory {
    amounts: [u32; Good::COUNT],
    bulk_capacity: u32,
    #[serde(default)]
    partition_bulk_capacity: Option<u32>,
}

impl GoodsInventory {
    pub const fn new(bulk_capacity: u32) -> Self {
        Self {
            amounts: [0; Good::COUNT],
            bulk_capacity,
            partition_bulk_capacity: None,
        }
    }

    /// Build storage with the same independent bulk allowance for every good.
    /// `partition_bulk_capacity` is therefore the capacity of each named bay,
    /// not a total shared between unlike resources.
    pub const fn new_partitioned(partition_bulk_capacity: u32) -> Self {
        Self {
            amounts: [0; Good::COUNT],
            bulk_capacity: partition_bulk_capacity.saturating_mul(Good::COUNT as u32),
            partition_bulk_capacity: Some(partition_bulk_capacity),
        }
    }

    pub const fn bulk_capacity(&self) -> u32 {
        self.bulk_capacity
    }

    pub const fn partition_bulk_capacity(&self) -> Option<u32> {
        self.partition_bulk_capacity
    }

    pub fn bulk_capacity_for(&self, good: Good) -> u32 {
        self.partition_bulk_capacity.unwrap_or_else(|| {
            self.amount(good).saturating_mul(good.bulk_per_unit()) + self.free_bulk()
        })
    }

    /// Resize physical storage without ever deleting goods. A requested
    /// shrink stops at the currently occupied bulk; callers can retry after
    /// the inventory is unloaded.
    pub fn resize_bulk_capacity(&mut self, requested: u32) {
        self.bulk_capacity = requested.max(self.used_bulk());
        self.partition_bulk_capacity = None;
    }

    /// Convert to, or resize, equal per-good compartments without deleting an
    /// over-capacity legacy stack. A later resize can finish shrinking once
    /// that particular good has been removed.
    pub fn resize_partitioned_bulk_capacity(&mut self, requested_per_good: u32) {
        let occupied_high_water = Good::ALL
            .iter()
            .map(|good| self.amount(*good).saturating_mul(good.bulk_per_unit()))
            .max()
            .unwrap_or(0);
        let per_good = requested_per_good.max(occupied_high_water);
        self.partition_bulk_capacity = Some(per_good);
        self.bulk_capacity = per_good.saturating_mul(Good::COUNT as u32);
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
        if self.partition_bulk_capacity.is_some() {
            Good::ALL
                .iter()
                .map(|good| self.free_bulk_for(*good))
                .fold(0, u32::saturating_add)
        } else {
            self.bulk_capacity.saturating_sub(self.used_bulk())
        }
    }

    /// Remaining bulk which can accept this specific good. For ordinary
    /// inventories this is the shared remainder; for a public market it is
    /// only the named resource's compartment.
    pub fn free_bulk_for(&self, good: Good) -> u32 {
        self.partition_bulk_capacity.map_or_else(
            || self.bulk_capacity.saturating_sub(self.used_bulk()),
            |capacity| {
                capacity.saturating_sub(self.amount(good).saturating_mul(good.bulk_per_unit()))
            },
        )
    }

    pub fn free_units(&self, good: Good) -> u32 {
        self.free_bulk_for(good) / good.bulk_per_unit()
    }

    pub fn is_empty(&self) -> bool {
        self.amounts.iter().all(|amount| *amount == 0)
    }

    /// Household-edible portions currently stored here. Flour represents one
    /// basic ration baked at home; raw Wheat is intentionally excluded.
    pub fn edible_amount(&self) -> u32 {
        Good::HOUSEHOLD_FOOD_PRIORITY
            .iter()
            .map(|good| self.amount(*good))
            .fold(0, u32::saturating_add)
    }

    /// Ready meals which can be eaten without a household kitchen.
    pub fn ready_to_eat_amount(&self) -> u32 {
        Good::READY_TO_EAT_PRIORITY
            .iter()
            .map(|good| self.amount(*good))
            .fold(0, u32::saturating_add)
    }

    /// Consume up to `requested` household portions, Bread before fish and
    /// home-baked Flour.
    pub fn remove_edible(&mut self, requested: u32) -> u32 {
        let mut removed = 0u32;
        for good in Good::HOUSEHOLD_FOOD_PRIORITY {
            removed = removed.saturating_add(self.remove(good, requested.saturating_sub(removed)));
            if removed == requested {
                break;
            }
        }
        removed
    }

    /// Add up to `requested` units and return the amount accepted.
    pub fn add(&mut self, good: Good, requested: u32) -> u32 {
        let accepted = requested.min(self.free_units(good));
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
    #[serde(default)]
    pub private_job_positions: u16,
    #[serde(default)]
    pub private_filled_jobs: u16,
    #[serde(default)]
    pub private_vacant_jobs: u16,
    #[serde(default)]
    pub civic_job_positions: u16,
    #[serde(default)]
    pub civic_filled_jobs: u16,
    #[serde(default)]
    pub civic_vacant_jobs: u16,
    #[serde(default)]
    pub job_seekers: u16,
    #[serde(default)]
    pub best_open_private_wage: u64,
    /// Completed beds currently available across the settlement.
    #[serde(default)]
    pub housing_capacity: u32,
    /// Residents without a real [`crate::components::HomeAssignment`].
    #[serde(default)]
    pub homeless_residents: u32,
    /// Current public or private workers attached to a workplace which owes
    /// wage arrears. Historical claimants are not counted here.
    #[serde(default)]
    pub unpaid_workers: u16,
    /// Slow-moving public instability. This is a settlement reading, not a
    /// new need or continuously ticking state on every character.
    #[serde(default)]
    pub unrest: f32,
    /// Today's pressure before persistence is applied.
    #[serde(default)]
    pub unrest_target: f32,
    /// Signed change made on the most recently completed world day.
    #[serde(default)]
    pub unrest_change: f32,
    #[serde(default)]
    pub unrest_hunger_pressure: f32,
    #[serde(default)]
    pub unrest_housing_pressure: f32,
    #[serde(default)]
    pub unrest_wage_pressure: f32,
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
            private_job_positions: 0,
            private_filled_jobs: 0,
            private_vacant_jobs: 0,
            civic_job_positions: 0,
            civic_filled_jobs: 0,
            civic_vacant_jobs: 0,
            job_seekers: 0,
            best_open_private_wage: 0,
            housing_capacity: 0,
            homeless_residents: 0,
            unpaid_workers: 0,
            unrest: 0.0,
            unrest_target: 0.0,
            unrest_change: 0.0,
            unrest_hunger_pressure: 0.0,
            unrest_housing_pressure: 0.0,
            unrest_wage_pressure: 0.0,
        }
    }
}

impl SettlementEconomy {
    pub const fn unrest_label(&self) -> &'static str {
        if self.unrest < 20.0 {
            "Calm"
        } else if self.unrest < 40.0 {
            "Uneasy"
        } else if self.unrest < 60.0 {
            "Tense"
        } else if self.unrest < 80.0 {
            "Volatile"
        } else {
            "Rebellious"
        }
    }

    pub const fn unrest_trend_label(&self) -> &'static str {
        if self.unrest_change > 0.05 {
            "rising"
        } else if self.unrest_change < -0.05 {
            "falling"
        } else {
            "steady"
        }
    }
}

/// A settlement is secure once it can survive this many days without another
/// harvest. Production reliability remains a separate promotion requirement.
pub const FOOD_SECURITY_TARGET_DAYS: f32 = 3.0;
pub const VILLAGE_MIN_RESIDENTS: u32 = 12;
pub const VILLAGE_REQUIRED_SECURE_DAYS: u16 = 3;
pub const VILLAGE_MIN_PROSPERITY: f32 = 65.0;
/// Wood staged beside the founding Moot before its permanent Village Hall is
/// raised. This uses the same paid civic-worksite pipeline as later upgrades.
pub const VILLAGE_HALL_WOOD_REQUIRED: u32 = 12;
pub const TOWN_MIN_RESIDENTS: u32 = 30;
pub const TOWN_MIN_PROSPERITY: f32 = 70.0;
pub const TOWN_MIN_MARKET_VOLUME: u64 = 5_000;
pub const TOWN_REQUIRED_DAYS: u16 = 3;
/// Dressed Stone staged beside the civic centre before a Village can raise
/// its permanent Town Hall. The exchange buys this from real private offers;
/// promotion never withdraws seller-owned stock for free.
pub const TOWN_HALL_STONE_REQUIRED: u32 = 8;
/// Provisional monotonic gate while City population balance remains open.
/// Keeping it above the agreed 30-person Town gate prevents a prosperous
/// small Village from skipping through two civic identities.
pub const CITY_MIN_RESIDENTS: u32 = 75;
pub const CITY_MIN_PROSPERITY: f32 = 75.0;
pub const CITY_REQUIRED_DAYS: u16 = 5;

/// Initial physical capacities, measured in [`Good::bulk_per_unit`] units.
///
/// These are tuning, not economic values. They say how much can physically be
/// present before somebody must haul it elsewhere; they say nothing about who
/// owns the contents or what they are worth. `HALL` and `MARKET` are per-good
/// compartment sizes; the remaining constants are ordinary combined stores.
pub mod capacity {
    /// Personal cargo carried by both villagers and player heroes. Sixteen
    /// bulk fits four Wood bundles (up from three) while remaining far below
    /// even the smallest workplace store.
    pub const VILLAGER: u32 = 16;
    /// Temporary work capacity for Moot Stewards and private company porters.
    /// This is the future hand-cart allowance, not a larger personal backpack.
    pub const PORTER: u32 = 96;
    pub const HOUSE: u32 = 80;
    pub const FARMSTEAD: u32 = 240;
    pub const LIVESTOCK_FARM: u32 = 300;
    pub const LUMBERJACK_HUT: u32 = 240;
    pub const FISHERMANS_HUT: u32 = 240;
    pub const MARKET: u32 = 600;
    pub const TAVERN: u32 = 180;
    pub const CHURCH: u32 = 120;
    pub const WINDMILL: u32 = 240;
    pub const BAKERY: u32 = 240;
    /// A dedicated private store is deliberately much larger than a workshop,
    /// but remains finite so logistics and additional buildings still matter.
    pub const STORAGE_HALL: u32 = 2_400;
    pub const STONE_QUARRY: u32 = 360;
    pub const HALL: u32 = 1_200;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_heroes_begin_with_twenty_coins_without_changing_villager_money() {
        assert_eq!(Wallet::founding_hero().balance(), 20 * PENNIES_PER_COIN);
        assert_eq!(Wallet::founding_villager().balance(), 10 * PENNIES_PER_COIN);
    }

    #[test]
    fn site_consolidation_never_erases_direct_company_capital() {
        let mut account = CompanyAccount {
            cash: 1_000,
            contributed_capital: 1_000,
            ..default()
        };
        account.refresh_from_sites(
            2,
            0,
            0,
            0,
            0,
            0,
            0,
            CompanyDayLedger::empty(2),
            Some(CompanyDayLedger::empty(1)),
        );
        assert_eq!(account.contributed_capital, 1_000);
        account.refresh_from_sites(
            2,
            0,
            0,
            1_500,
            0,
            0,
            0,
            CompanyDayLedger::empty(2),
            Some(CompanyDayLedger::empty(1)),
        );
        assert_eq!(account.contributed_capital, 1_500);
    }

    #[test]
    fn personal_inventory_holds_four_wood_bundles() {
        assert_eq!(capacity::VILLAGER, Good::Wood.bulk_per_unit() * 4);
    }

    #[test]
    fn porter_cart_capacity_is_large_and_inventory_resizing_is_lossless() {
        assert_eq!(capacity::PORTER, capacity::VILLAGER * 6);
        assert_eq!(BusinessSalePolicy::default().max_units_per_collection, 64);
        assert_eq!(PorterCartState::for_used_bulk(0).load_slots, 0);
        assert_eq!(PorterCartState::for_used_bulk(1).load_slots, 1);
        assert_eq!(PorterCartState::for_used_bulk(48).load_slots, 1);
        assert_eq!(PorterCartState::for_used_bulk(49).load_slots, 2);
        assert_eq!(PorterCartState::for_used_bulk(96).load_slots, 2);

        let mut inventory = GoodsInventory::new(capacity::VILLAGER);
        inventory.resize_bulk_capacity(capacity::PORTER);
        assert_eq!(inventory.add(Good::Wood, 24), 24);
        inventory.resize_bulk_capacity(capacity::VILLAGER);
        assert_eq!(inventory.amount(Good::Wood), 24);
        assert_eq!(inventory.bulk_capacity(), capacity::PORTER);
    }

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
    fn partitioned_public_storage_keeps_each_goods_space_independent() {
        let mut inventory = GoodsInventory::new_partitioned(12);

        assert_eq!(inventory.add(Good::Wood, 4), 3);
        assert_eq!(inventory.free_units(Good::Wood), 0);
        assert_eq!(inventory.add(Good::Bread, 12), 12);
        assert_eq!(inventory.amount(Good::Wood), 3);
        assert_eq!(inventory.amount(Good::Bread), 12);
        assert_eq!(inventory.free_units(Good::Bread), 0);
        assert_eq!(inventory.free_units(Good::Flour), 12);
        assert_eq!(inventory.used_bulk(), 24);
        assert_eq!(
            inventory.bulk_capacity(),
            12 * u32::try_from(Good::COUNT).unwrap()
        );
    }

    #[test]
    fn converting_a_full_legacy_store_preserves_every_stack() {
        let mut inventory = GoodsInventory::new(12);
        assert_eq!(inventory.add(Good::Wood, 3), 3);

        inventory.resize_partitioned_bulk_capacity(8);

        assert_eq!(inventory.amount(Good::Wood), 3);
        assert_eq!(inventory.partition_bulk_capacity(), Some(12));
        assert_eq!(inventory.add(Good::Bread, 12), 12);
    }

    #[test]
    fn company_resource_rules_are_independent_between_settlements() {
        let north = crate::components::SettlementId(7);
        let south = crate::components::SettlementId(8);
        let mut policies = CompanyBranchPolicies::default();
        policies.set_resource(
            north,
            Good::Wheat,
            CompanyResourcePolicy {
                retain_units: 40,
                sell_excess: false,
            },
        );
        policies.set_resource(
            south,
            Good::Wheat,
            CompanyResourcePolicy {
                retain_units: 5,
                sell_excess: true,
            },
        );

        assert_eq!(policies.resource(north, Good::Wheat).retain_units, 40);
        assert!(!policies.resource(north, Good::Wheat).sell_excess);
        assert_eq!(policies.resource(south, Good::Wheat).retain_units, 5);
        assert!(policies.resource(south, Good::Wheat).sell_excess);
        assert_eq!(
            policies.resource(north, Good::Bread),
            CompanyResourcePolicy::default()
        );
        assert_eq!(policies.branches().len(), 2);
        let public = CompanyResourcePolicy {
            retain_units: 20,
            sell_excess: true,
        };
        assert_eq!(public.public_surplus(100, 10), 70);
        assert_eq!(public.public_surplus(15, 10), 0);
        assert_eq!(
            policies.resource(north, Good::Wheat).public_surplus(100, 0),
            0
        );
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
    fn meat_flour_and_bread_are_food_but_wheat_and_wool_are_not() {
        let mut inventory = GoodsInventory::new(40);
        inventory.add(Good::Food, 2);
        inventory.add(Good::Wheat, 3);
        inventory.add(Good::Flour, 2);
        inventory.add(Good::Bread, 1);
        inventory.add(Good::Meat, 2);
        inventory.add(Good::Wool, 2);
        inventory.add(Good::Wood, 2);

        assert_eq!(inventory.edible_amount(), 7);
        assert_eq!(inventory.remove_edible(4), 4);
        assert_eq!(inventory.amount(Good::Bread), 0);
        assert_eq!(inventory.amount(Good::Meat), 0);
        assert_eq!(inventory.amount(Good::Food), 1);
        assert_eq!(inventory.amount(Good::Flour), 2);
        assert_eq!(inventory.amount(Good::Wheat), 3);
        assert_eq!(inventory.amount(Good::Wood), 2);
        assert!(!Good::Wheat.is_edible());
        assert!(!Good::Wool.is_edible());
        assert!(Good::Meat.is_ready_to_eat());
        assert!(Good::Flour.is_edible());
        assert!(!Good::Flour.is_ready_to_eat());
        assert_eq!(Good::Bread.food_tier(), 2);
        assert_eq!(Good::TAVERN_INPUTS, [Good::Meat, Good::Bread, Good::Wheat]);
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
    fn market_consignment_pays_only_after_a_real_purchase() {
        let mut market = MootMarket::founding();
        let seller = MarketSeller::Business(crate::components::BuildingId(7));
        market.consign(seller, Good::Wheat, 6, 80);
        assert_eq!(market.seller_listed_units(seller, Good::Wheat), 6);

        let purchase = market.purchase(Good::Wheat, 3, 240, None, None);
        assert_eq!(
            purchase.trade,
            MarketTrade {
                units: 3,
                pennies: 240
            }
        );
        assert_eq!(purchase.fills.len(), 1);
        assert_eq!(purchase.fills[0].seller, seller);
        assert_eq!(purchase.fills[0].gross, 240);
        assert_eq!(purchase.fills[0].market_fee, 12);
        assert_eq!(market.seller_listed_units(seller, Good::Wheat), 3);
    }

    #[test]
    fn contracted_purchase_never_substitutes_a_different_seller() {
        let mut market = MootMarket::founding();
        let contracted = MarketSeller::Business(crate::components::BuildingId(21));
        let cheaper_rival = MarketSeller::Business(crate::components::BuildingId(22));
        market.consign(contracted, Good::Stone, 8, 250);
        market.consign(cheaper_rival, Good::Stone, 8, 100);

        let purchase = market.purchase_from_seller(contracted, Good::Stone, 8, 2_000, Some(250));

        assert_eq!(purchase.trade.units, 8);
        assert_eq!(purchase.trade.pennies, 2_000);
        assert!(purchase.fills.iter().all(|fill| fill.seller == contracted));
        assert_eq!(market.seller_listed_units(contracted, Good::Stone), 0);
        assert_eq!(market.seller_listed_units(cheaper_rival, Good::Stone), 8);
    }

    #[test]
    fn daily_market_flow_keeps_executed_prices_separate_and_resets() {
        let mut market = MootMarket::founding();
        let seller = MarketSeller::Business(crate::components::BuildingId(8));
        market.consign(seller, Good::Wheat, 4, 80);
        let consumer_purchase = market.purchase(Good::Wheat, 2, u64::MAX, None, None);
        let pool = market.pool(Good::Wheat);
        assert_eq!(
            pool.day.producer_units,
            u64::from(consumer_purchase.trade.units)
        );
        let seller_net = consumer_purchase.fills[0]
            .gross
            .saturating_sub(consumer_purchase.fills[0].market_fee);
        assert_eq!(pool.day.producer_coin, seller_net);
        assert_eq!(
            pool.day.consumer_units,
            u64::from(consumer_purchase.trade.units)
        );
        assert_eq!(pool.day.consumer_coin, consumer_purchase.trade.pennies);
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
        assert_eq!(market.pool(Good::Wheat).previous_day.consumer_units, 2);
    }

    #[test]
    fn wholesale_collection_pays_producer_without_inventing_local_consumption() {
        let mut market = MootMarket::founding();
        let seller = MarketSeller::Business(crate::components::BuildingId(18));
        market.consign(seller, Good::Bread, 6, 10);

        let purchase = market.purchase_for_resale(Good::Bread, 4, 40, Some(10), None);
        assert_eq!(purchase.trade.units, 4);
        let flow = market.pool(Good::Bread).day;
        assert_eq!(flow.producer_units, 4);
        assert_eq!(flow.consumer_units, 0);
        assert_eq!(flow.consumer_coin, 0);
    }

    #[test]
    fn market_distinguishes_missing_stock_from_rejected_prices() {
        let mut market = MootMarket::founding();
        let seller = MarketSeller::Business(crate::components::BuildingId(81));
        market.consign(seller, Good::Flour, 4, 600);

        let purchase = market.purchase_recording_demand(Good::Flour, 6, 600, None, None);
        assert_eq!(purchase.trade.units, 1);
        let flow = market.pool(Good::Flour).day;
        assert_eq!(flow.consumer_units, 1);
        assert_eq!(flow.unavailable_units, 2);
        assert_eq!(flow.unaffordable_units, 3);
        assert_eq!(flow.funded_unmet_units, 4);
        assert_eq!(flow.requested_units(), 6);
    }

    #[test]
    fn best_offer_and_last_sale_are_distinct_market_quotes() {
        let mut market = MootMarket::founding();
        let expensive = MarketSeller::Business(crate::components::BuildingId(9));
        let cheap = MarketSeller::Business(crate::components::BuildingId(10));
        market.consign(expensive, Good::Food, 2, 140);
        market.consign(cheap, Good::Food, 1, 90);
        assert_eq!(market.pool(Good::Food).ask, 90);
        assert_eq!(market.pool(Good::Food).bid, 0);

        let purchase = market.purchase(Good::Food, 1, 90, None, None);
        assert_eq!(purchase.trade.units, 1);
        assert_eq!(market.pool(Good::Food).bid, 90);
        assert_eq!(market.pool(Good::Food).ask, 140);
    }

    #[test]
    fn profit_is_revenue_less_real_expenses_and_never_opening_capital() {
        let mut account = BusinessAccount::with_capital(1_000);
        account.record_sale(1, 500, 25, 5);
        account.incur_wages(1, 200);
        account.record_input_purchase(1, 100, 2);

        assert_eq!(account.lifetime_profit(), 175);
        assert_eq!(account.retained_profit(), 175);
        assert_eq!(account.current_day.profit(), 175);
        assert_eq!(account.unposted_company_capital, 1_000);
        assert_eq!(account.contributed_capital, 1_000);
        assert_eq!(account.wage_arrears, 200);
    }

    #[test]
    fn defaulted_wages_remove_the_claim_without_burning_firm_cash() {
        let mut account = BusinessAccount::with_capital(58);
        account.incur_wages(1, 100);

        assert_eq!(account.write_off_wage_claim(100), 100);
        assert_eq!(account.unposted_company_capital, 58);
        assert_eq!(account.wage_arrears, 0);
        assert_eq!(account.defaulted_wages, 100);
    }

    #[test]
    fn completed_shift_wages_do_not_roll_an_open_ledger_backwards() {
        let mut account = BusinessAccount::default();
        account.record_sale(4, 500, 0, 5);
        account.roll_to_day(5);
        account.record_sale(5, 200, 0, 2);

        account.incur_completed_day_wages(4, 100);

        assert_eq!(account.current_day.day, 5);
        assert_eq!(account.current_day.gross_revenue, 200);
        assert_eq!(account.current_day.wage_expense, 0);
        assert_eq!(account.previous_day.day, 4);
        assert_eq!(account.previous_day.gross_revenue, 500);
        assert_eq!(account.previous_day.wage_expense, 100);
        assert_eq!(account.wage_arrears, 100);
    }

    #[test]
    fn company_dividend_capacity_protects_payroll_inputs_and_liabilities() {
        let mut market = MootMarket::founding();
        market.consign(
            MarketSeller::Business(crate::components::BuildingId(91)),
            Good::Wheat,
            20,
            80,
        );
        let wage = BusinessWagePolicy {
            daily_wage: 100,
            ..default()
        };
        let management = BusinessManagementPolicy {
            payroll_reserve_days: 2,
            ..default()
        };
        let procurement = BusinessProcurementPolicy::none().with_rule(
            Good::Wheat,
            BusinessInputRule {
                enabled: true,
                coverage_days: 2,
                reorder_below: 2,
                target_units: 10,
                maximum_unit_price: 100,
            },
        );
        let reserve =
            business_working_capital(2, &wage, &management, &procurement, None, Some(&market));
        assert_eq!(reserve.payroll, 400);
        assert_eq!(reserve.inputs, 800);
        assert_eq!(reserve.operating_buffer, 200);

        let mut held_stock = [0; Good::COUNT];
        held_stock[Good::Wheat.index()] = 6;
        let partially_stocked = business_working_capital(
            2,
            &wage,
            &management,
            &procurement,
            Some(&held_stock),
            Some(&market),
        );
        assert_eq!(
            partially_stocked.inputs, 320,
            "cash protection covers only the missing input target"
        );

        let mut account = BusinessAccount::with_capital(2_000);
        account.record_sale(1, 3_000, 0, 30);
        account.incur_wages(1, 300);
        account.incur_profit_tax(1, 100);
        let mut company = CompanyAccount {
            cash: 5_000,
            wage_arrears: account.wage_arrears,
            tax_arrears: account.tax_arrears,
            ..default()
        };
        let protected = reserve.total_with_liabilities(&account);
        let draw = account
            .retained_profit()
            .min(company.cash.saturating_sub(protected));
        assert_eq!(draw, 2_600, "opening capital is not distributable profit");
        assert!(company.debit(draw));
        account.record_company_dividend(1, draw);
        assert!(company.cash >= reserve.total_with_liabilities(&account));
        assert_eq!(account.retained_profit(), 0);
    }

    #[test]
    fn liquidation_markdown_keeps_food_listed_and_can_clear_below_the_reference_price() {
        let mut market = MootMarket::founding();
        let seller = MarketSeller::Business(crate::components::BuildingId(92));
        market.consign(seller, Good::Bread, 5, 200);
        for _ in 0..20 {
            market.markdown_seller(seller, 1_500);
        }
        assert_eq!(market.seller_listed_units(seller, Good::Bread), 5);
        assert_eq!(market.listed_edible_units(), 5);
        assert!(market.suggested_price(Good::Bread) < Good::Bread.base_price() / 4);
        assert!(market.suggested_price(Good::Bread) > 0);
    }

    #[test]
    fn competing_price_excludes_the_reviewing_seller() {
        let mut market = MootMarket::founding();
        let first = MarketSeller::Business(crate::components::BuildingId(1));
        let second = MarketSeller::Business(crate::components::BuildingId(2));
        market.consign(first, Good::Bread, 4, 90);
        market.consign(second, Good::Bread, 4, 75);

        assert_eq!(market.best_competing_price(first, Good::Bread), Some(75));
        assert_eq!(market.best_competing_price(second, Good::Bread), Some(90));
    }

    #[test]
    fn public_target_shortfall_is_a_signal_not_a_consignment_cap() {
        let mut market = MootMarket::founding();
        let seller = MarketSeller::Business(crate::components::BuildingId(190));
        assert_eq!(market.target_shortfall(Good::Wheat), 4);
        market.consign(seller, Good::Wheat, 3, Good::Wheat.base_price());
        assert_eq!(market.target_shortfall(Good::Wheat), 1);

        market.set_targets(12, 0);
        assert_eq!(market.target_shortfall(Good::Wheat), 29);
        assert_eq!(market.target_shortfall(Good::Stone), 4);
        market.consign(seller, Good::Wheat, 40, Good::Wheat.base_price() - 1);
        assert_eq!(market.target_shortfall(Good::Wheat), 0);
        assert_eq!(market.listed_units(Good::Wheat), 43);
    }

    #[test]
    fn marketplace_deepens_wheat_planning_without_capping_offers() {
        let mut market = MootMarket::founding();
        market.set_targets_with_marketplace(16, 0, false);
        assert_eq!(market.pool(Good::Wheat).target_stock, 32);
        market.set_targets_with_marketplace(16, 0, true);
        assert_eq!(market.pool(Good::Wheat).target_stock, 96);
        assert_eq!(market.pool(Good::Food).target_stock, 96);
    }

    #[test]
    fn public_goods_unlock_once_at_their_declared_market_tier() {
        let seller = MarketSeller::Business(crate::components::BuildingId(195));
        let mut market = MootMarket::founding();

        for good in Good::ALL {
            assert_eq!(
                market.can_trade(good),
                good != Good::Iron,
                "every current founding good except Iron should trade at the Moot",
            );
        }
        market.consign(seller, Good::Iron, 2, Good::Iron.base_price());
        assert_eq!(market.listed_units(Good::Iron), 0);
        assert_eq!(
            market.purchase_recording_demand(Good::Iron, 2, u64::MAX, None, None),
            MarketPurchase::default(),
        );
        assert_eq!(
            market.pool(Good::Iron).day.unmet_units(),
            0,
            "a legally locked good must not look like an economic shortage",
        );

        market.unlock_trade_tier(MarketTradeTier::Marketplace);
        assert!(!market.can_trade(Good::Iron));
        market.unlock_trade_tier(MarketTradeTier::PavedMarketplace);
        assert!(market.can_trade(Good::Iron));
        market.consign(seller, Good::Iron, 2, Good::Iron.base_price());
        assert_eq!(market.listed_units(Good::Iron), 2);

        market.unlock_trade_tier(MarketTradeTier::Moot);
        assert!(
            market.can_trade(Good::Iron),
            "an exchange unlock cannot regress and strand existing listings",
        );
    }

    #[test]
    fn estate_transfer_preserves_the_goods_price_and_future_proceeds() {
        let person = MarketSeller::Person(crate::components::PersonId(93));
        let treasury = MarketSeller::Treasury(crate::components::SettlementId(94));
        let mut market = MootMarket::founding();
        market.consign(person, Good::Flour, 7, 61);

        assert_eq!(market.transfer_seller(person, treasury), 7);
        assert_eq!(market.seller_total_listed_units(person), 0);
        assert_eq!(market.seller_listed_units(treasury, Good::Flour), 7);
        let purchase = market.purchase(Good::Flour, 7, u64::MAX, None, None);
        assert_eq!(purchase.trade.pennies, 7 * 61);
        assert!(purchase
            .fills
            .iter()
            .all(|fill| fill.seller == treasury && fill.unit_price == 61));
    }

    #[test]
    fn purchase_preview_is_exact_and_can_exclude_the_buyers_own_stock() {
        let mut market = MootMarket::founding();
        let buyer = MarketSeller::Business(crate::components::BuildingId(1));
        let rival = MarketSeller::Business(crate::components::BuildingId(2));
        market.consign(buyer, Good::Wheat, 5, 50);
        market.consign(rival, Good::Wheat, 5, 80);

        let preview = market.preview_purchase(Good::Wheat, 3, 240, Some(80), Some(buyer));
        assert_eq!(
            preview,
            MarketTrade {
                units: 3,
                pennies: 240
            }
        );
        let purchase = market.purchase(Good::Wheat, 3, 240, Some(80), Some(buyer));
        assert_eq!(purchase.trade, preview);
        assert_eq!(purchase.fills[0].seller, rival);
        assert_eq!(market.seller_listed_units(buyer, Good::Wheat), 5);
    }

    #[test]
    fn inventory_reconciliation_never_leaves_phantom_market_offers() {
        let settlement = crate::components::SettlementId(3);
        let seller = MarketSeller::Business(crate::components::BuildingId(11));
        let mut market = MootMarket::founding();
        let mut inventory = GoodsInventory::new(40);
        inventory.add(Good::Wheat, 2);
        market.consign(MarketSeller::Treasury(settlement), Good::Wheat, 2, 70);
        market.consign(seller, Good::Wheat, 3, 80);

        market.reconcile_inventory(settlement, &inventory);

        assert_eq!(market.listed_units(Good::Wheat), 2);
        assert_eq!(
            market.seller_listed_units(seller, Good::Wheat),
            2,
            "the migration-created Treasury claim should be discarded first"
        );
        assert_eq!(
            market
                .purchase(Good::Wheat, 99, u64::MAX, None, None)
                .trade
                .units,
            2
        );
    }

    #[test]
    fn only_needed_housing_is_free_while_businesses_have_a_floor() {
        use crate::components::SettlementBuildingKind;
        assert_eq!(permit_price(SettlementBuildingKind::House, 4, true), 0);
        assert!(permit_price(SettlementBuildingKind::Farmstead, 0, true) >= PENNIES_PER_COIN);
        assert_eq!(permit_price(SettlementBuildingKind::Windmill, 0, true), 0);
        assert!(permit_price(SettlementBuildingKind::Windmill, 0, false) > 0);
        assert!(
            permit_price(SettlementBuildingKind::Farmstead, 2, false)
                > permit_price(SettlementBuildingKind::Farmstead, 0, true)
        );
        assert!(
            permit_price_with_subsidy(SettlementBuildingKind::Farmstead, 0, true, 0)
                > permit_price_with_subsidy(
                    SettlementBuildingKind::Farmstead,
                    0,
                    true,
                    DEFAULT_BUSINESS_PERMIT_SUBSIDY_BPS,
                ),
            "the enacted subsidy must reduce requested business permit prices"
        );
        assert_eq!(
            permit_price_with_subsidy(SettlementBuildingKind::Farmstead, 0, false, 0),
            permit_price_with_subsidy(
                SettlementBuildingKind::Farmstead,
                0,
                false,
                MAXIMUM_BUSINESS_PERMIT_SUBSIDY_BPS,
            ),
            "speculative businesses must not receive a demand subsidy"
        );
    }

    #[test]
    fn player_owned_amenity_permits_cost_real_money() {
        use crate::components::SettlementBuildingKind;
        assert_eq!(
            player_permit_price_with_subsidy(
                SettlementBuildingKind::Market,
                0,
                false,
                DEFAULT_BUSINESS_PERMIT_SUBSIDY_BPS,
            ),
            500
        );
        assert_eq!(
            player_permit_price_with_subsidy(
                SettlementBuildingKind::Tavern,
                0,
                false,
                DEFAULT_BUSINESS_PERMIT_SUBSIDY_BPS,
            ),
            400
        );
        assert_eq!(
            player_permit_price_with_subsidy(
                SettlementBuildingKind::Church,
                0,
                false,
                DEFAULT_BUSINESS_PERMIT_SUBSIDY_BPS,
            ),
            600
        );
    }

    #[test]
    fn capital_spending_changes_book_value_without_reducing_operating_profit() {
        let mut account = BusinessAccount::with_project_funding(500, 0, 300, 2);
        account.record_sale(2, 200, 10, 1);
        account.incur_wages(2, 50);
        assert_eq!(account.book_value, 300);
        assert_eq!(account.capital_expenditures, 300);
        assert_eq!(account.current_day.capital_expenditures, 300);
        assert_eq!(account.current_day.profit(), 140);
    }
}
