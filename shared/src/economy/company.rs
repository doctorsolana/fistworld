//! Company treasuries, consolidation, branch policies and decision records.

use super::money::signed_difference;
use super::{BusinessStrategy, Good};
use crate::components::{CompanyOwnership, PersonId, COMPANY_TOTAL_SHARES};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

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

/// Replicated dividend headroom of one company, published by the server's
/// finance pass once per world day and after every manual distribution, never
/// per tick. `distributable` is what a distribution could pay right now:
/// consolidated retained profit capped by treasury cash above every site's
/// payroll, input, tax and operating reserves. `day` is the world day of that
/// snapshot (`u32::MAX` until the first pass); `last_paid_day`/`last_paid`
/// record the most recent automatic or manual payout (`u32::MAX` = never).
/// Cash moves between passes, so this is a snapshot for display and drafting;
/// the server clamps every request against the live figure when it pays.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompanyDividendCapacity {
    pub day: u32,
    pub distributable: u64,
    pub protected_reserves: u64,
    pub retained_profit: u64,
    pub last_paid_day: u32,
    pub last_paid: u64,
}

impl Default for CompanyDividendCapacity {
    /// Hand-written so that "never paid" and "not yet reviewed" are
    /// `u32::MAX`, not world day 0.
    fn default() -> Self {
        Self {
            day: u32::MAX,
            distributable: 0,
            protected_reserves: 0,
            retained_profit: 0,
            last_paid_day: u32::MAX,
            last_paid: 0,
        }
    }
}

/// Divide a distribution over the exact 1,000-share cap table. Every holder
/// receives the wide-integer floor of their share; the whole-penny remainder
/// goes to the first cap-table entry (sorted by `PersonId`), so the sum always
/// equals `pennies`. The server pays and the client previews with this one
/// function so both agree penny for penny. `out` is cleared and refilled in
/// cap-table order.
pub fn pro_rata_split(pennies: u64, ownership: &CompanyOwnership, out: &mut Vec<(PersonId, u64)>) {
    out.clear();
    let mut allocated = 0u64;
    for holding in ownership.shares() {
        let amount = (u128::from(pennies) * u128::from(holding.shares)
            / u128::from(COMPANY_TOTAL_SHARES)) as u64;
        allocated += amount;
        out.push((holding.shareholder, amount));
    }
    if let Some((_, first)) = out.first_mut() {
        *first += pennies - allocated;
    }
}

/// The automatic payout share NPC autopilots start with. Each day the finance
/// pass distributes this percentage of the retained profit above the
/// working-capital runway, so a firm earning a steady daily profit `P`
/// settles at about `100 / percent` days of profit above its runway (four
/// days at 25%) and pays out `P` per day from then on; a loss simply pays
/// nothing. Chosen by measurement in the village lab; see
/// `docs/COMPANY-ECONOMY-IMPLEMENTATION.md`, "NPC company decisions".
pub const DEFAULT_NPC_PAYOUT_PERCENT: u8 = 25;

/// The largest automatic payout share a Company Master may set. Half of the
/// headroom per day already returns the whole buffer within a few days.
pub const MAX_AUTOMATIC_PAYOUT_PERCENT: u8 = 50;

/// The daily automatic dividend for `headroom` (consolidated retained profit
/// above the working-capital runway): `headroom × percent / 100`, floored to
/// whole pennies. `0` retains everything. The share never exceeds
/// [`MAX_AUTOMATIC_PAYOUT_PERCENT`] even for a corrupt policy value.
pub fn automatic_payout(headroom: u64, percent: u8) -> u64 {
    let percent = u128::from(percent.min(MAX_AUTOMATIC_PAYOUT_PERCENT));
    (u128::from(headroom) * percent / 100) as u64
}

/// Decisions applying to the whole legal company rather than one operating
/// site. Individual sites still control their own product, price, wage offer,
/// stock retention and input route.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompanyManagementPolicy {
    pub strategy: BusinessStrategy,
    pub autopilot: bool,
    /// `0` retains every profit until the Company Master distributes it;
    /// `1..=MAX_AUTOMATIC_PAYOUT_PERCENT` pays that share of the automatic
    /// headroom (retained profit above the working-capital runway) once a
    /// day. There is no other cap: the share is self-limiting.
    pub automatic_payout_percent: u8,
    pub payroll_reserve_days: u8,
    pub last_review_day: u32,
    pub last_dividend_day: u32,
}

impl CompanyManagementPolicy {
    /// Whether the finance pass distributes anything without a request.
    pub fn pays_automatic_dividends(&self) -> bool {
        self.automatic_payout_percent > 0
    }

    /// Today's automatic dividend for `headroom`; see [`automatic_payout`].
    pub fn automatic_payout(&self, headroom: u64) -> u64 {
        automatic_payout(headroom, self.automatic_payout_percent)
    }
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
            automatic_payout_percent: DEFAULT_NPC_PAYOUT_PERCENT,
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

/// Cash an established company may commit to expansion without spending its
/// employee/tax liabilities or enacted company-wide payroll runway.
pub fn company_expansion_cash(company: &CompanyAccount, protected_payroll: u64) -> u64 {
    company
        .cash
        .saturating_sub(company.wage_arrears)
        .saturating_sub(company.tax_arrears)
        .saturating_sub(protected_payroll)
}
