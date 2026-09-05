//! Public income, spending and bounded policy defaults.

use super::PENNIES_PER_COIN;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Public daily wage for each combined Moot Steward position.
pub const MOOT_STEWARD_DAILY_SALARY: u64 = PENNIES_PER_COIN;

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
