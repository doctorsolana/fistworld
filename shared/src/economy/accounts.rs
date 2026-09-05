//! Site cost-centre ledgers, liabilities and operating profit.

use super::money::signed_difference;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

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
}

impl Default for BusinessDayLedger {
    fn default() -> Self {
        Self::empty(u32::MAX)
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
