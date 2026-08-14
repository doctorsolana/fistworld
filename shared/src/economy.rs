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
    pub profit_tax_income: u64,
    pub public_sale_income: u64,
    pub wage_expense: u64,
    pub poor_relief_expense: u64,
    pub material_expense: u64,
}

impl CivicDayLedger {
    pub const fn empty(day: u32) -> Self {
        Self {
            day,
            permit_income: 0,
            market_fee_income: 0,
            profit_tax_income: 0,
            public_sale_income: 0,
            wage_expense: 0,
            poor_relief_expense: 0,
            material_expense: 0,
        }
    }

    pub const fn income(self) -> u64 {
        self.permit_income
            .saturating_add(self.market_fee_income)
            .saturating_add(self.profit_tax_income)
            .saturating_add(self.public_sale_income)
    }

    pub const fn spending(self) -> u64 {
        self.wage_expense
            .saturating_add(self.poor_relief_expense)
            .saturating_add(self.material_expense)
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
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

impl Default for CivicAccount {
    fn default() -> Self {
        Self {
            current_day: CivicDayLedger::default(),
            previous_day: CivicDayLedger::default(),
            income_since_review: 0,
            spending_since_review: 0,
            lifetime_income: 0,
            lifetime_spending: 0,
        }
    }
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
    pub gross_revenue: u64,
    pub wage_expense: u64,
    pub input_expense: u64,
    pub market_fees: u64,
    #[serde(default)]
    pub profit_taxes: u64,
    pub owner_withdrawals: u64,
    pub produced_units: u32,
    pub sold_units: u32,
    pub purchased_input_units: u32,
}

impl BusinessDayLedger {
    pub const fn empty(day: u32) -> Self {
        Self {
            day,
            gross_revenue: 0,
            wage_expense: 0,
            input_expense: 0,
            market_fees: 0,
            profit_taxes: 0,
            owner_withdrawals: 0,
            produced_units: 0,
            sold_units: 0,
            purchased_input_units: 0,
        }
    }

    pub const fn operating_expenses(self) -> u64 {
        self.wage_expense
            .saturating_add(self.input_expense)
            .saturating_add(self.market_fees)
            .saturating_add(self.profit_taxes)
    }

    pub const fn pre_tax_expenses(self) -> u64 {
        self.wage_expense
            .saturating_add(self.input_expense)
            .saturating_add(self.market_fees)
    }

    pub fn pre_tax_profit(self) -> u64 {
        self.gross_revenue.saturating_sub(self.pre_tax_expenses())
    }

    pub fn profit(self) -> i64 {
        signed_difference(self.gross_revenue, self.operating_expenses())
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

/// Cash, liabilities and accounts belonging to a business rather than its
/// owner personally.
///
/// `cash` answers whether a payment can happen. Profit is instead revenue less
/// operating expenses, and opening capital is recorded separately. An owner
/// may withdraw retained profit, but can never withdraw contributed capital or
/// money owed as wages merely because it happens to be in this account.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessAccount {
    pub cash: u64,
    pub wage_arrears: u64,
    #[serde(default)]
    pub tax_arrears: u64,
    pub last_payroll_day: u32,
    #[serde(default)]
    pub contributed_capital: u64,
    #[serde(default)]
    pub gross_revenue: u64,
    #[serde(default)]
    pub operating_expenses: u64,
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
            cash: 0,
            wage_arrears: 0,
            tax_arrears: 0,
            last_payroll_day: u32::MAX,
            contributed_capital: 0,
            gross_revenue: 0,
            operating_expenses: 0,
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
            cash: pennies,
            contributed_capital: pennies,
            ..Self::default()
        }
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
        self.cash = self.cash.saturating_add(pennies);
        self.contributed_capital = self.contributed_capital.saturating_add(pennies);
    }

    pub fn record_production(&mut self, day: u32, units: u32) {
        self.roll_to_day(day);
        self.current_day.produced_units = self.current_day.produced_units.saturating_add(units);
    }

    /// Credit one consignment sale. `gross` is what the buyer paid and `fee`
    /// is the marketplace's included share, so business cash receives net.
    pub fn record_sale(&mut self, day: u32, gross: u64, fee: u64, units: u32) {
        self.roll_to_day(day);
        let fee = fee.min(gross);
        self.cash = self.cash.saturating_add(gross.saturating_sub(fee));
        self.gross_revenue = self.gross_revenue.saturating_add(gross);
        self.operating_expenses = self.operating_expenses.saturating_add(fee);
        self.current_day.gross_revenue = self.current_day.gross_revenue.saturating_add(gross);
        self.current_day.market_fees = self.current_day.market_fees.saturating_add(fee);
        self.current_day.sold_units = self.current_day.sold_units.saturating_add(units);
    }

    /// Buy physical inputs without ever spending payroll liabilities.
    pub fn buy_inputs(&mut self, day: u32, pennies: u64, units: u32) -> bool {
        if self
            .cash
            .saturating_sub(self.wage_arrears)
            .saturating_sub(self.tax_arrears)
            < pennies
        {
            return false;
        }
        self.roll_to_day(day);
        self.cash -= pennies;
        self.operating_expenses = self.operating_expenses.saturating_add(pennies);
        self.current_day.input_expense = self.current_day.input_expense.saturating_add(pennies);
        self.current_day.purchased_input_units =
            self.current_day.purchased_input_units.saturating_add(units);
        true
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

    pub fn pay_wage_claim(&mut self, pennies: u64) -> u64 {
        let paid = pennies.min(self.cash).min(self.wage_arrears);
        self.cash -= paid;
        self.wage_arrears -= paid;
        paid
    }

    /// Close an unpayable wage claim without pretending cash changed hands.
    /// Defaults remove the liability and remain visible in business history;
    /// only [`Self::pay_wage_claim`] may reduce firm cash.
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

    pub fn pay_tax_claim(&mut self, pennies: u64) -> u64 {
        let paid = pennies
            .min(self.cash.saturating_sub(self.wage_arrears))
            .min(self.tax_arrears);
        self.cash -= paid;
        self.tax_arrears -= paid;
        paid
    }

    pub fn lifetime_profit(self) -> i64 {
        signed_difference(self.gross_revenue, self.operating_expenses)
    }

    pub fn retained_profit(self) -> u64 {
        self.gross_revenue
            .saturating_sub(self.operating_expenses)
            .saturating_sub(self.owner_withdrawals)
    }

    pub fn withdrawable_profit(self, protected_working_cash: u64) -> u64 {
        self.retained_profit().min(
            self.cash
                .saturating_sub(self.wage_arrears)
                .saturating_sub(self.tax_arrears)
                .saturating_sub(protected_working_cash),
        )
    }

    pub fn withdraw_owner(&mut self, day: u32, wanted: u64, protected_working_cash: u64) -> u64 {
        let withdrawn = wanted.min(self.withdrawable_profit(protected_working_cash));
        if withdrawn == 0 {
            return 0;
        }
        self.roll_to_day(day);
        self.cash -= withdrawn;
        self.owner_withdrawals = self.owner_withdrawals.saturating_add(withdrawn);
        self.current_day.owner_withdrawals =
            self.current_day.owner_withdrawals.saturating_add(withdrawn);
        withdrawn
    }
}

/// Cash which automatic management must leave inside a firm before an owner
/// may draw profit. Liabilities are reported separately because
/// [`BusinessAccount::withdrawable_profit`] already subtracts them before this
/// reserve is considered.
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
    pub keep_units: u32,
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
            keep_units: 2,
            max_units_per_collection: 8,
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
            minimum_unit_price: good.base_price().saturating_mul(35) / 100,
            ..Self::default()
        }
    }
}

/// One input which automatic management may purchase from the local market.
/// The rule is deliberately about stock and price rather than a named building
/// kind: a tavern, bakery, brewery or smithy can all use the same decision path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct BusinessInputRule {
    pub enabled: bool,
    pub reorder_below: u32,
    pub target_units: u32,
    pub maximum_unit_price: u64,
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
        self.can_operate()
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
        Self {
            started_day: day,
            last_review_day: day,
            empty_days: 0,
            reason: BusinessSaleReason::Insolvent,
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

impl Good {
    /// Existing discriminants stay in their original order for replicated and
    /// saved data; new goods are appended.
    pub const ALL: [Self; 7] = [
        Self::Food,
        Self::Wheat,
        Self::Wood,
        Self::Stone,
        Self::Iron,
        Self::Flour,
        Self::Bread,
    ];
    pub const COUNT: usize = Self::ALL.len();

    /// Household pantries consume better prepared food first. Flour is last:
    /// it becomes an ordinary ration only through home baking.
    pub const HOUSEHOLD_FOOD_PRIORITY: [Self; 3] = [Self::Bread, Self::Food, Self::Flour];

    /// Food which can be handed to an unhoused resident and eaten in the Moot
    /// commons. Flour deliberately is not on this list.
    pub const READY_TO_EAT_PRIORITY: [Self; 2] = [Self::Bread, Self::Food];

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
        }
    }

    /// Whether one unit can satisfy one resident's daily food ration.
    ///
    /// Flour is edible only through a household pantry; direct meal services
    /// must additionally require [`Self::is_ready_to_eat`]. Raw Wheat is never
    /// food after the milling chain was introduced.
    pub const fn is_edible(self) -> bool {
        matches!(self, Self::Food | Self::Flour | Self::Bread)
    }

    pub const fn is_ready_to_eat(self) -> bool {
        matches!(self, Self::Food | Self::Bread)
    }

    /// Bread is the first tier-two food. The tier is inspectable now and can
    /// later feed preferences, health and migration without identifying foods
    /// by display string.
    pub const fn food_tier(self) -> u8 {
        match self {
            Self::Food | Self::Flour => 1,
            Self::Bread => 2,
            Self::Wheat | Self::Wood | Self::Stone | Self::Iron => 0,
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
        }
    }

    fn observe_quote(&mut self, bid: u64, ask: u64) {
        self.high_bid = self.high_bid.max(bid);
        self.low_bid = self.low_bid.min(bid);
        self.high_ask = self.high_ask.max(ask);
        self.low_ask = self.low_ask.min(ask);
    }

    fn record_consignment_sale(&mut self, units: u32, gross: u64, seller_net: u64) {
        self.producer_units = self.producer_units.saturating_add(u64::from(units));
        self.producer_coin = self.producer_coin.saturating_add(seller_net);
        self.consumer_units = self.consumer_units.saturating_add(u64::from(units));
        self.consumer_coin = self.consumer_coin.saturating_add(gross);
    }

    fn record_unmet_demand(&mut self, unavailable: u32, unaffordable: u32) {
        self.unavailable_units = self
            .unavailable_units
            .saturating_add(u64::from(unavailable));
        self.unaffordable_units = self
            .unaffordable_units
            .saturating_add(u64::from(unaffordable));
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
            ],
            listings: Vec::new(),
            market_fee_bps: DEFAULT_MARKET_FEE_BPS,
        }
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

    pub fn seller_listed_units(&self, seller: MarketSeller, good: Good) -> u32 {
        self.listings
            .iter()
            .filter(|listing| listing.seller == seller && listing.good == good)
            .map(|listing| listing.units)
            .fold(0u32, u32::saturating_add)
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

    /// Update desired reserves from current population and construction demand.
    pub fn set_targets(&mut self, residents: u32, outstanding_wood: u32) {
        let edible_target = residents.saturating_mul(3).max(4);
        self.pool_mut(Good::Food).target_stock = edible_target;
        self.pool_mut(Good::Flour).target_stock = edible_target;
        self.pool_mut(Good::Bread).target_stock = edible_target;
        // Wheat is working stock for mills rather than a resident reserve.
        self.pool_mut(Good::Wheat).target_stock = residents.max(4);
        self.pool_mut(Good::Wood).target_stock = outstanding_wood.saturating_add(10);
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
        if units == 0 {
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
            let floor = listing.good.base_price().saturating_mul(25) / 100;
            listing.unit_price = listing
                .unit_price
                .saturating_sub(movement)
                .max(floor)
                .max(1);
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
        let mut purchase = MarketPurchase::default();
        let mut remaining = requested;
        for listing in &mut self.listings {
            if remaining == 0 || purchase.trade.pennies >= budget {
                break;
            }
            if listing.good != good
                || listing.units == 0
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
        let purchase = self.purchase(good, requested, budget, maximum_unit_price, excluded_seller);
        debug_assert_eq!(purchase.trade, preview);
        self.pool_mut(good)
            .day
            .record_unmet_demand(unavailable, unaffordable);
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
                u64::from(rule.target_units).saturating_mul(price)
            })
        })
        .fold(0u64, u64::saturating_add);
    BusinessWorkingCapital {
        payroll,
        inputs,
        operating_buffer: 2 * PENNIES_PER_COIN,
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
    #[serde(default)]
    pub unavailable_units: u64,
    #[serde(default)]
    pub unaffordable_units: u64,
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
/// `total_local_coin` is the conservation-oriented measure: treasury + personal
/// wallets + household purses + business accounts. Produced goods are tracked
/// separately and valued at that day's last completed sale price.
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
    pub profit_tax_income: u64,
    pub public_sale_income: u64,
    pub wage_expense: u64,
    pub poor_relief_expense: u64,
    pub material_expense: u64,
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
    pub wage_expense: u64,
    pub input_expense: u64,
    pub market_fees: u64,
    pub profit_taxes: u64,
    pub owner_withdrawals: u64,
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
    pub kind: crate::components::SettlementBuildingKind,
    pub owner_id: Option<crate::components::PersonId>,
    pub owner_name: Option<String>,
    pub output_good: Option<Good>,
    /// Oldest to newest, never longer than [`SETTLEMENT_HISTORY_DAYS`].
    pub days: Vec<BusinessHistoryDay>,
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
    let base: u64 = match kind {
        SettlementBuildingKind::Farmstead | SettlementBuildingKind::FishermansHut => 300,
        SettlementBuildingKind::LumberjackHut => 250,
        SettlementBuildingKind::Windmill | SettlementBuildingKind::Bakery => 250,
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
    pub const WINDMILL: u32 = 240;
    pub const BAKERY: u32 = 240;
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
    fn flour_and_bread_are_food_but_raw_wheat_is_not() {
        let mut inventory = GoodsInventory::new(40);
        inventory.add(Good::Food, 2);
        inventory.add(Good::Wheat, 3);
        inventory.add(Good::Flour, 2);
        inventory.add(Good::Bread, 1);
        inventory.add(Good::Wood, 2);

        assert_eq!(inventory.edible_amount(), 5);
        assert_eq!(inventory.remove_edible(4), 4);
        assert_eq!(inventory.amount(Good::Food), 0);
        assert_eq!(inventory.amount(Good::Flour), 1);
        assert_eq!(inventory.amount(Good::Wheat), 3);
        assert_eq!(inventory.amount(Good::Wood), 2);
        assert!(!Good::Wheat.is_edible());
        assert!(Good::Flour.is_edible());
        assert!(!Good::Flour.is_ready_to_eat());
        assert_eq!(Good::Bread.food_tier(), 2);
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
        assert!(account.buy_inputs(1, 100, 2));

        assert_eq!(account.lifetime_profit(), 175);
        assert_eq!(account.retained_profit(), 175);
        assert_eq!(account.current_day.profit(), 175);
        assert_eq!(account.withdrawable_profit(100), 175);
        assert_eq!(account.withdraw_owner(1, u64::MAX, 100), 175);
        assert_eq!(account.retained_profit(), 0);
        assert_eq!(account.contributed_capital, 1_000);
        assert_eq!(account.wage_arrears, 200);
    }

    #[test]
    fn defaulted_wages_remove_the_claim_without_burning_firm_cash() {
        let mut account = BusinessAccount::with_capital(58);
        account.incur_wages(1, 100);

        assert_eq!(account.write_off_wage_claim(100), 100);
        assert_eq!(account.cash, 58);
        assert_eq!(account.wage_arrears, 0);
        assert_eq!(account.defaulted_wages, 100);
    }

    #[test]
    fn owner_draws_cannot_spend_payroll_inputs_or_liabilities() {
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
                reorder_below: 2,
                target_units: 10,
                maximum_unit_price: 100,
            },
        );
        let reserve = business_working_capital(2, &wage, &management, &procurement, Some(&market));
        assert_eq!(reserve.payroll, 400);
        assert_eq!(reserve.inputs, 800);
        assert_eq!(reserve.operating_buffer, 200);

        let mut account = BusinessAccount::with_capital(2_000);
        account.record_sale(1, 3_000, 0, 30);
        account.incur_wages(1, 300);
        account.incur_profit_tax(1, 100);
        let before = account.cash;
        let draw = account.withdraw_owner(1, u64::MAX, reserve.total());
        assert_eq!(draw, 2_600, "opening capital is not distributable profit");
        assert!(account.cash >= reserve.total_with_liabilities(&account));
        assert_eq!(before - account.cash, draw);
        assert_eq!(account.withdraw_owner(1, u64::MAX, reserve.total()), 0);
    }

    #[test]
    fn liquidation_markdown_keeps_food_listed_and_never_reaches_zero_price() {
        let mut market = MootMarket::founding();
        let seller = MarketSeller::Business(crate::components::BuildingId(92));
        market.consign(seller, Good::Bread, 5, 200);
        for _ in 0..20 {
            market.markdown_seller(seller, 1_500);
        }
        assert_eq!(market.seller_listed_units(seller, Good::Bread), 5);
        assert_eq!(market.listed_edible_units(), 5);
        assert!(market.suggested_price(Good::Bread) >= Good::Bread.base_price() / 4);
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
}
