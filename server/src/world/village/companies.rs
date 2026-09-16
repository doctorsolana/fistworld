//! Stable companies, pooled liquidity and consolidated reporting.
//!
//! Productive buildings remain independently inspectable cost centres. Money
//! exists only in the legal company's treasury; sites record which operation
//! earned revenue or incurred an expense without owning separate wallets.

use super::*;

use shared::components::{
    Company, CompanyId, CompanyLeadership, CompanyOwnership, CompanyShareMarket, OperatedBy,
    OwnedBy, PersonId, PlayerPermitLedger,
};
use shared::economy::{
    business_working_capital, BusinessDayLedger, BusinessProcurementPolicy, BusinessStrategy,
    CompanyAccount, CompanyBranchPolicies, CompanyDayLedger, CompanyDecisionHistory,
    CompanyDecisionReason, CompanyDecisionRecord, CompanyDividendCapacity, CompanyManagementPolicy,
};

/// One authoritative constructor for a legal company. Player incorporation,
/// NPC entrepreneurship and compatibility backfills must all issue the same
/// shares, offices, accounts and replicated policy components.
pub(crate) fn new_company_bundle(
    id: CompanyId,
    name: String,
    founded_day: u32,
    founder: PersonId,
    initial_cash: u64,
    contributed_capital: u64,
) -> impl Bundle {
    let mut account = CompanyAccount {
        cash: initial_cash,
        contributed_capital,
        ..default()
    };
    account.roll_to_day(founded_day);
    (
        Company { name, founded_day },
        id,
        CompanyOwnership::sole(founder),
        CompanyLeadership { master: founder },
        CompanyShareMarket::default(),
        account,
        CompanyManagementPolicy::default(),
        CompanyBranchPolicies::default(),
        CompanyDecisionHistory::default(),
        CompanyDividendCapacity::default(),
        Replicate::to_clients(NetworkTarget::All),
    )
}

/// Backfill one durable company per existing entrepreneur and attach every
/// productive site. Later company-formation/player systems can create several
/// companies for one person; this compatibility path consistently chooses the
/// oldest company they control.
#[allow(clippy::type_complexity)]
pub fn ensure_companies(
    mut commands: Commands,
    mut ids: ResMut<crate::world::identity::WorldIdAllocator>,
    world_time: Query<&WorldTime>,
    mut last_full_audit_day: Local<Option<u32>>,
    changed_businesses: Query<(), Or<(Added<SettlementBuilding>, Changed<OwnedBy>)>>,
    people: Query<(&PersonId, &CharacterName)>,
    companies: Query<(
        Entity,
        &CompanyId,
        &CompanyOwnership,
        Option<&CompanyManagementPolicy>,
        Option<&CompanyLeadership>,
        Option<&CompanyDecisionHistory>,
        Option<&CompanyShareMarket>,
        Option<&CompanyBranchPolicies>,
        Option<&CompanyDividendCapacity>,
    )>,
    buildings: Query<(
        Entity,
        &SettlementBuilding,
        Option<&OwnedBy>,
        Option<&OperatedBy>,
    )>,
) {
    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    let full_audit = *last_full_audit_day != Some(day);
    if !full_audit && changed_businesses.is_empty() {
        return;
    }
    if full_audit {
        *last_full_audit_day = Some(day);
    }
    let names: HashMap<PersonId, &str> = people
        .iter()
        .map(|(id, name)| (*id, name.0.as_str()))
        .collect();
    let mut company_by_controller: HashMap<PersonId, (CompanyId, Entity)> = HashMap::new();
    let mut ownership_by_company: HashMap<CompanyId, &CompanyOwnership> = HashMap::new();
    let living_people: HashSet<PersonId> = names.keys().copied().collect();
    for (
        entity,
        id,
        ownership,
        management,
        leadership,
        decision_history,
        share_market,
        branches,
        dividend_capacity,
    ) in companies.iter()
    {
        if management.is_none() {
            commands
                .entity(entity)
                .insert(CompanyManagementPolicy::default());
        }
        if leadership.is_none_or(|office| !living_people.contains(&office.master)) {
            if let Some(master) = ownership.controlling_shareholder() {
                commands.entity(entity).insert(CompanyLeadership { master });
            }
        }
        if decision_history.is_none() {
            commands
                .entity(entity)
                .insert(CompanyDecisionHistory::default());
        }
        if share_market.is_none() {
            commands
                .entity(entity)
                .insert(CompanyShareMarket::default());
        }
        if branches.is_none() {
            commands
                .entity(entity)
                .insert(CompanyBranchPolicies::default());
        }
        if dividend_capacity.is_none() {
            commands
                .entity(entity)
                .insert(CompanyDividendCapacity::default());
        }
        ownership_by_company.insert(*id, ownership);
        if let Some(controller) = ownership.controlling_shareholder() {
            let candidate = (*id, entity);
            company_by_controller
                .entry(controller)
                .and_modify(|current| {
                    if candidate.0 < current.0 {
                        *current = candidate;
                    }
                })
                .or_insert(candidate);
        }
    }

    for (entity, building, owner, operated_by) in buildings.iter() {
        if !is_private_business(building.kind) {
            if operated_by.is_some() {
                commands.entity(entity).remove::<OperatedBy>();
            }
            continue;
        }
        let Some(owner) = owner.copied() else {
            // Keep the legal company attached while an inherited or insolvent
            // site liquidates. This lets company-funded permit refunds and the
            // final books settle before a buyer's CompanyId replaces it.
            continue;
        };
        if !living_people.contains(&owner.0) {
            continue;
        }
        let current_is_valid = operated_by.is_some_and(|operation| {
            ownership_by_company
                .get(&operation.0)
                .is_some_and(|ownership| ownership.share_count(owner.0) > 0)
        });
        if current_is_valid {
            continue;
        }

        let company_id = if let Some((company, _)) = company_by_controller.get(&owner.0) {
            *company
        } else {
            let company_id = ids.company();
            let owner_name = names.get(&owner.0).copied().unwrap_or("Unnamed");
            let company_entity = commands
                .spawn(new_company_bundle(
                    company_id,
                    format!("{owner_name} & Company"),
                    day,
                    owner.0,
                    0,
                    0,
                ))
                .id();
            company_by_controller.insert(owner.0, (company_id, company_entity));
            company_id
        };
        commands.entity(entity).insert(OperatedBy(company_id));
    }
}

/// Keep a sparse, stable directory of every settlement in which a company has
/// a completed site or active project. Default branch policies still need a
/// branch row: without it an ownerless company that finishes liquidation has
/// no principled settlement to receive its unclaimed residual treasury.
pub fn ensure_company_branches(
    sites: Query<(&OperatedBy, &shared::components::BuildingOf)>,
    projects: Query<(&OperatedBy, &UnderConstruction)>,
    mut companies: Query<(&CompanyId, &mut CompanyBranchPolicies)>,
) {
    let mut required = HashMap::<CompanyId, HashSet<shared::components::SettlementId>>::new();
    for (company, building_of) in sites.iter() {
        required.entry(company.0).or_default().insert(building_of.0);
    }
    for (company, project) in projects.iter() {
        required
            .entry(company.0)
            .or_default()
            .insert(project.settlement_id);
    }
    for (company, mut branches) in companies.iter_mut() {
        let Some(settlements) = required.get(company) else {
            continue;
        };
        for settlement in settlements {
            branches.ensure_branch(*settlement);
        }
    }
}

/// Wind up an ownerless legal shell after its last site and unused permit have
/// moved. A living shareholder may deliberately keep an empty company ready
/// for a later investment, so only a cap table with no living person is an
/// estate. Residual cash then falls to the company's last known settlement,
/// preventing dead companies from trapping circulating money forever.
pub fn cleanup_empty_companies(
    mut commands: Commands,
    mut companies: Query<(
        Entity,
        &CompanyId,
        &CompanyOwnership,
        &CompanyBranchPolicies,
        &mut CompanyAccount,
    )>,
    sites: Query<&OperatedBy>,
    permit_ledgers: Query<&PlayerPermitLedger>,
    living_people: Query<&PersonId, With<CharacterKind>>,
    mut settlements: Query<(&shared::components::SettlementId, &mut Settlement)>,
) {
    if companies.is_empty() {
        return;
    }
    let mut active: HashSet<CompanyId> = sites.iter().map(|company| company.0).collect();
    active.extend(
        permit_ledgers
            .iter()
            .flat_map(|ledger| ledger.permits.iter().filter_map(|permit| permit.company)),
    );
    let living_people: HashSet<PersonId> = living_people.iter().copied().collect();
    for (entity, company, ownership, branches, mut account) in companies.iter_mut() {
        if active.contains(company) || account.wage_arrears > 0 || account.tax_arrears > 0 {
            continue;
        }
        if ownership
            .shares()
            .iter()
            .any(|holding| living_people.contains(&holding.shareholder))
        {
            continue;
        }
        if account.cash > 0 {
            let mut settled = false;
            for branch in branches.branches() {
                if let Some((_, mut settlement)) = settlements
                    .iter_mut()
                    .find(|(id, _)| **id == branch.settlement)
                {
                    settlement.treasury = settlement.treasury.saturating_add(account.cash);
                    settled = true;
                    break;
                }
            }
            if !settled {
                // A freshly incorporated company with no branch can still be
                // referenced by a disconnected save. Keep it funded and
                // visible rather than guessing which settlement owns it.
                continue;
            }
            account.cash = 0;
        }
        commands.entity(entity).despawn();
    }
}

#[derive(Default)]
struct CompanyOperatingReading {
    cash: u64,
    liabilities: u64,
    revenue: u64,
    costs: u64,
    distressed_sites: u16,
}

fn propagate_company_strategies(
    changed: &HashMap<CompanyId, BusinessStrategy>,
    site_policies: &mut Query<(&OperatedBy, &mut BusinessManagementPolicy)>,
) {
    if changed.is_empty() {
        return;
    }
    for (operated_by, mut management) in site_policies.iter_mut() {
        if !management.autopilot {
            continue;
        }
        if let Some(strategy) = changed.get(&operated_by.0).copied() {
            let reserve_days = strategy.payroll_reserve_days();
            if management.strategy != strategy || management.payroll_reserve_days != reserve_days {
                management.strategy = strategy;
                management.payroll_reserve_days = reserve_days;
            }
        }
    }
}

/// The appointed Company Master reviews consolidated results at a deliberately
/// sparse three-day cadence. This is a bounded decision tree over company
/// aggregates, not a per-NPC/per-frame planner. Player Masters can pause it and
/// use the same strategy controls manually.
pub fn review_company_strategies(
    world_time: Query<&WorldTime>,
    mut processed_day: Local<Option<u32>>,
    people: Query<(&PersonId, &CharacterAttributes)>,
    mut companies: Query<(
        &CompanyId,
        &CompanyLeadership,
        &CompanyAccount,
        &mut CompanyManagementPolicy,
        &mut CompanyDecisionHistory,
    )>,
    sites: Query<(&OperatedBy, &BusinessAccount, Option<&BusinessCondition>)>,
    mut site_policies: Query<(&OperatedBy, &mut BusinessManagementPolicy)>,
) {
    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    // Explicit company choices also govern its other automatic sites. Apply
    // those changes on the same day, even with the executive paused or its
    // next three-day financial review still pending. Manual sites keep their
    // local overrides, and unchanged policies do not dirty site replication.
    let chosen: HashMap<_, _> = companies
        .iter_mut()
        .filter(|(_, _, _, policy, _)| policy.is_changed())
        .map(|(id, _, _, policy, _)| (*id, policy.strategy))
        .collect();
    propagate_company_strategies(&chosen, &mut site_policies);
    if *processed_day == Some(day) {
        return;
    }
    *processed_day = Some(day);
    let attributes: HashMap<PersonId, CharacterAttributes> =
        people.iter().map(|(id, values)| (*id, *values)).collect();
    let mut readings: HashMap<CompanyId, CompanyOperatingReading> = HashMap::new();
    for (operated_by, account, condition) in sites.iter() {
        let reading = readings.entry(operated_by.0).or_default();
        reading.liabilities = reading
            .liabilities
            .saturating_add(account.wage_arrears)
            .saturating_add(account.tax_arrears);
        let ledger = account.previous_day;
        if ledger.day != u32::MAX {
            reading.revenue = reading.revenue.saturating_add(ledger.gross_revenue);
            reading.costs = reading.costs.saturating_add(
                ledger
                    .wage_expense
                    .saturating_add(ledger.input_expense)
                    .saturating_add(ledger.market_fees)
                    .saturating_add(ledger.delivery_fees)
                    .saturating_add(ledger.profit_taxes),
            );
        }
        if condition.is_some_and(|condition| {
            matches!(
                condition.state,
                BusinessState::Distressed | BusinessState::Insolvent | BusinessState::Liquidating
            )
        }) {
            reading.distressed_sites = reading.distressed_sites.saturating_add(1);
        }
    }

    let mut changed = HashMap::new();
    for (company_id, leadership, company_account, mut policy, mut history) in companies.iter_mut() {
        if !policy.autopilot
            || (policy.last_review_day != u32::MAX
                && day.saturating_sub(policy.last_review_day) < 3)
        {
            continue;
        }
        policy.last_review_day = day;
        let Some(reading) = readings.get_mut(company_id) else {
            continue;
        };
        reading.cash = company_account.cash;
        let master = attributes
            .get(&leadership.master)
            .copied()
            .unwrap_or_default();
        let profitable = reading.revenue > reading.costs;
        let strongly_profitable =
            reading.revenue >= reading.costs.saturating_mul(3).saturating_div(2).max(1);
        let (next, reason) = if reading.liabilities > 0 || reading.distressed_sites > 0 {
            (
                BusinessStrategy::Conservative,
                CompanyDecisionReason::FinancialStress,
            )
        } else if strongly_profitable && reading.cash > reading.costs.saturating_mul(3) {
            (
                BusinessStrategy::Aggressive,
                CompanyDecisionReason::ProfitableExpansion,
            )
        } else if profitable && master.charm() >= 18 {
            (
                BusinessStrategy::Aggressive,
                CompanyDecisionReason::StrongMarketPosition,
            )
        } else {
            (
                BusinessStrategy::Balanced,
                CompanyDecisionReason::NormalisedOperations,
            )
        };
        if next != policy.strategy {
            let previous = policy.strategy;
            policy.strategy = next;
            policy.payroll_reserve_days = next.payroll_reserve_days();
            history.push(CompanyDecisionRecord {
                day,
                master: leadership.master,
                from: previous,
                to: next,
                reason,
            });
            changed.insert(*company_id, next);
        }
    }
    propagate_company_strategies(&changed, &mut site_policies);
}

#[derive(Clone, Copy)]
struct DividendSite {
    entity: Entity,
    building: shared::components::BuildingId,
    protected: u64,
    gross_revenue: u64,
    operating_expenses: u64,
    prior_withdrawals: u64,
}

impl DividendSite {
    /// Profit this cost centre has earned and not yet had attributed as a
    /// distribution. Dividend memos are attributed against it so that a site
    /// leaving the company takes its revenue and its paid-out share together.
    fn retained_profit(&self) -> u64 {
        self.gross_revenue
            .saturating_sub(self.operating_expenses)
            .saturating_sub(self.prior_withdrawals)
    }
}

/// One Company Master's request for a distribution, addressed by the stable
/// company id and remembering who asked so the deferred reply reaches them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DividendRequest {
    pub company: CompanyId,
    /// `u64::MAX` means everything distributable.
    pub pennies: u64,
    pub requester_person: PersonId,
    /// The `ClientOf` link entity that sent the order.
    pub requester_link: Entity,
}

#[derive(Resource, Default)]
pub struct CompanyDividendQueue {
    requested: Vec<DividendRequest>,
}

impl CompanyDividendQueue {
    /// Queue one request per company; a newer request for the same company
    /// replaces the older one (only the latest amount and requester matter).
    pub fn request(
        &mut self,
        company: CompanyId,
        pennies: u64,
        requester_person: PersonId,
        requester_link: Entity,
    ) {
        let request = DividendRequest {
            company,
            pennies,
            requester_person,
            requester_link,
        };
        if let Some(existing) = self
            .requested
            .iter_mut()
            .find(|existing| existing.company == company)
        {
            *existing = request;
        } else {
            self.requested.push(request);
        }
    }

    #[cfg(test)]
    pub fn pending(&self) -> &[DividendRequest] {
        &self.requested
    }
}

/// Why a manual distribution paid nothing. Each names the concrete branch
/// the finance pass took instead of a silent skip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DividendRefusal {
    /// The company operates no site with business books; contributed capital
    /// alone is never distributable profit.
    NoOperatingSite,
    /// A shareholder on the cap table has no wallet entity, so the exact
    /// pro-rata payment cannot be delivered to everyone.
    ShareholderUnreachable,
    /// Retained profit or treasury cash above reserves is zero right now.
    NothingDistributable,
    /// One receiving wallet would overflow; nothing was redirected.
    WalletFull,
    /// The treasury debit failed after validation (should not happen).
    TreasuryDebitFailed,
    /// The company entity left the finance query between the order and the
    /// pass (wound up or despawned), so there was nothing to review.
    CompanyUnavailable,
}

/// The honest result of one manual request, produced on every path of the
/// finance pass. After the payout the treasury decomposes into
/// `capacity.distributable + withheld_reserves + withheld_profit_cap`:
/// reserves are cash kept for payroll, inputs, taxes and the operating
/// buffer; the profit cap is free cash that is not yet earned profit
/// (contributed capital), which a dividend may never touch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DividendOutcome {
    pub requester_link: Entity,
    pub requester_person: PersonId,
    pub company: CompanyId,
    /// Pennies actually debited from the treasury and credited pro rata.
    pub paid: u64,
    /// `paid / 100`: what ten of the 1,000 shares (1%) received.
    pub per_ten_shares: u64,
    pub shareholders: usize,
    pub own_shares: u16,
    /// The requester's exact pro-rata take (including any penny remainder).
    pub own_take: u64,
    pub withheld_reserves: u64,
    pub withheld_profit_cap: u64,
    pub refusal: Option<DividendRefusal>,
}

/// Outcomes waiting for the network reporter (`report_dividend_outcomes`),
/// which runs in the ingress set after this world tick.
#[derive(Resource, Default)]
pub struct CompanyDividendOutcomes {
    pending: Vec<DividendOutcome>,
}

impl CompanyDividendOutcomes {
    pub fn push(&mut self, outcome: DividendOutcome) {
        self.pending.push(outcome);
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn drain(&mut self) -> std::vec::Drain<'_, DividendOutcome> {
        self.pending.drain(..)
    }
}

/// Escrow which belonged to a company must survive the death/despawn of the
/// hero carrying its permit ledger. Refunds wait here until the stable company
/// entity is available; no personal estate can accidentally inherit it.
#[derive(Resource, Default)]
pub struct CompanyEscrowRefundQueue {
    refunds: HashMap<CompanyId, u64>,
}

impl CompanyEscrowRefundQueue {
    pub fn request(&mut self, company: CompanyId, pennies: u64) {
        let refund = self.refunds.entry(company).or_default();
        *refund = refund.saturating_add(pennies);
    }
}

pub fn refund_company_escrows(
    mut refunds: ResMut<CompanyEscrowRefundQueue>,
    mut companies: Query<(&CompanyId, &mut CompanyAccount)>,
) {
    if refunds.refunds.is_empty() {
        return;
    }
    let pending = std::mem::take(&mut refunds.refunds);
    for (company, pennies) in pending {
        let Some((_, mut account)) = companies.iter_mut().find(|(id, _)| **id == company) else {
            refunds.request(company, pennies);
            continue;
        };
        account.credit(pennies);
    }
}

/// Consolidated reserve/profit figures of one company for one pass.
#[derive(Clone, Copy)]
struct CompanyHeadroom {
    protected: u64,
    retained_profit: u64,
}

impl CompanyHeadroom {
    fn of(sites: &[DividendSite]) -> Self {
        let protected = sites
            .iter()
            .map(|site| site.protected)
            .fold(0u64, u64::saturating_add)
            // A company treasury needs one ordinary operating buffer, not a
            // duplicate two-coin reserve for every cost centre it operates.
            .saturating_add(2 * PENNIES_PER_COIN);
        // Consolidate revenue, expenses and prior withdrawals across every
        // cost centre BEFORE clamping at zero: a loss-making site offsets a
        // profitable sibling, so the company never distributes (or publishes)
        // more than its consolidated retained profit. The per-site
        // `DividendSite::retained_profit` figure is only for memo attribution.
        let revenue = sites
            .iter()
            .map(|site| site.gross_revenue)
            .fold(0u64, u64::saturating_add);
        let expenses = sites
            .iter()
            .map(|site| site.operating_expenses)
            .fold(0u64, u64::saturating_add);
        let prior_withdrawals = sites
            .iter()
            .map(|site| site.prior_withdrawals)
            .fold(0u64, u64::saturating_add);
        let retained_profit = revenue
            .saturating_sub(expenses)
            .saturating_sub(prior_withdrawals);
        Self {
            protected,
            retained_profit,
        }
    }

    fn distributable(self, cash: u64) -> u64 {
        self.retained_profit
            .min(cash.saturating_sub(self.protected))
    }

    /// Free treasury cash that is not earned profit; see [`DividendOutcome`].
    fn unearned_cash(self, cash: u64) -> u64 {
        cash.saturating_sub(self.protected)
            .saturating_sub(self.retained_profit)
    }

    fn after_paying(self, paid: u64) -> Self {
        Self {
            retained_profit: self.retained_profit.saturating_sub(paid),
            ..self
        }
    }
}

/// Validate one distribution without touching the treasury: nothing moves
/// unless every receiving wallet can accept its exact pro-rata amount. Runs
/// before any mutable borrow of the replicated `CompanyAccount`, so a refused
/// request never marks unchanged state as changed.
fn validate_dividend(
    wanted: u64,
    ownership: &CompanyOwnership,
    people_by_id: &HashMap<PersonId, Entity>,
    wallets: &Query<&mut Wallet>,
    split: &mut Vec<(PersonId, u64)>,
) -> Result<(), DividendRefusal> {
    if wanted == 0 {
        return Err(DividendRefusal::NothingDistributable);
    }
    // Wide multiplication preserves the cap-table ratio even for large valid
    // balances; saturating either the ratio or a receiving wallet would
    // otherwise misallocate or destroy real coin.
    shared::economy::pro_rata_split(wanted, ownership, split);
    if split.iter().any(|(holder, amount)| {
        people_by_id.get(holder).is_none_or(|entity| {
            wallets
                .get(*entity)
                .is_ok_and(|wallet| wallet.balance().checked_add(*amount).is_none())
        })
    }) {
        // Retain the money and the profit entitlement for a later request;
        // do not silently redirect one shareholder's share to another.
        return Err(DividendRefusal::WalletFull);
    }
    Ok(())
}

/// Debit and credit one distribution already validated by
/// [`validate_dividend`] (`split` must be that call's result).
fn apply_dividend(
    day: u32,
    wanted: u64,
    company_account: &mut CompanyAccount,
    company_sites: &mut [DividendSite],
    people_by_id: &HashMap<PersonId, Entity>,
    wallets: &mut Query<&mut Wallet>,
    site_accounts: &mut Query<&mut BusinessAccount>,
    split: &[(PersonId, u64)],
) -> Result<(), DividendRefusal> {
    if !company_account.debit(wanted) {
        return Err(DividendRefusal::TreasuryDebitFailed);
    }
    company_account.owner_withdrawals = company_account.owner_withdrawals.saturating_add(wanted);
    company_account.roll_to_day(day);
    company_account.current_day.owner_withdrawals = company_account
        .current_day
        .owner_withdrawals
        .saturating_add(wanted);
    // Attribute the memorandum against each site's own retained profit (in
    // stable building order) without suggesting that a building paid the
    // cash. Writing it all to one site would let the others' revenue count
    // as undistributed again if that site were later removed.
    let mut remaining = wanted;
    for site in company_sites.iter_mut() {
        if remaining == 0 {
            break;
        }
        let portion = remaining.min(site.retained_profit());
        if portion == 0 {
            continue;
        }
        if let Ok(mut account) = site_accounts.get_mut(site.entity) {
            account.record_company_dividend(day, portion);
            site.prior_withdrawals = site.prior_withdrawals.saturating_add(portion);
            remaining -= portion;
        }
    }
    if remaining > 0 {
        if let Some(site) = company_sites.first_mut() {
            if let Ok(mut account) = site_accounts.get_mut(site.entity) {
                account.record_company_dividend(day, remaining);
                site.prior_withdrawals = site.prior_withdrawals.saturating_add(remaining);
            }
        }
    }
    for (holder, amount) in split {
        wallets
            .get_mut(people_by_id[holder])
            .expect("shareholder wallet validated before treasury debit")
            .credit(*amount);
    }
    Ok(())
}

/// Validate, debit and credit one distribution atomically. The replicated
/// `CompanyAccount` is borrowed mutably (and so marked changed) only after
/// validation succeeded.
#[allow(clippy::too_many_arguments)]
fn pay_dividend(
    day: u32,
    wanted: u64,
    ownership: &CompanyOwnership,
    company_account: &mut Mut<CompanyAccount>,
    company_sites: &mut [DividendSite],
    people_by_id: &HashMap<PersonId, Entity>,
    wallets: &mut Query<&mut Wallet>,
    site_accounts: &mut Query<&mut BusinessAccount>,
    split: &mut Vec<(PersonId, u64)>,
) -> Result<(), DividendRefusal> {
    validate_dividend(wanted, ownership, people_by_id, wallets, split)?;
    apply_dividend(
        day,
        wanted,
        &mut **company_account,
        company_sites,
        people_by_id,
        wallets,
        site_accounts,
        split,
    )
}

/// Pay automatic dividends once per day and manual requests as they arrive.
/// Every amount is validated against consolidated retained profit and the
/// single treasury after every site's employee, tax, input and operating
/// reserves; manual requests are clamped to that figure and reported, never
/// rejected on a stale snapshot, and are exempt from `max_daily_dividend` and
/// the one-per-day cadence. Payment follows the cap table: one whole share out
/// of 1,000 receives exactly one-thousandth of the distribution before
/// deterministic penny rounding. The pass publishes `CompanyDividendCapacity`
/// after its payouts (daily for every company, per request for that company,
/// and once for a company founded mid-day whose capacity is still the
/// never-reviewed default) and otherwise runs only at day changes or on a
/// request, so nothing here is per tick beyond one `Added` filter check.
#[allow(clippy::type_complexity)]
pub fn review_company_finance(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    mut requests: ResMut<CompanyDividendQueue>,
    mut outcomes: ResMut<CompanyDividendOutcomes>,
    mut processed_day: Local<Option<u32>>,
    employees: Query<&shared::components::EmployedAt>,
    markets: Query<(&shared::components::SettlementId, &MootMarket), With<Settlement>>,
    // p0: companies whose capacity component appeared since the last pass
    // (a mid-day founding); p1: the review query. One `ParamSet` because the
    // `Added` filter reads the same ticks p1 writes.
    mut companies: ParamSet<(
        Query<(), (With<CompanyId>, Added<CompanyDividendCapacity>)>,
        Query<(
            Entity,
            &CompanyId,
            &CompanyOwnership,
            &mut CompanyAccount,
            &mut CompanyManagementPolicy,
            Option<&mut CompanyDividendCapacity>,
        )>,
    )>,
    mut sites: ParamSet<(
        Query<(
            Entity,
            &shared::components::BuildingId,
            &OperatedBy,
            &SettlementBuilding,
            &GoodsInventory,
            &BusinessAccount,
            &BusinessWagePolicy,
            Option<&BusinessProcurementPolicy>,
            &BusinessManagementPolicy,
            Option<&BusinessStaffingPolicy>,
            Option<&shared::components::BuildingOf>,
        )>,
        Query<&mut BusinessAccount>,
    )>,
    mut people: ParamSet<(Query<(Entity, &PersonId), With<Wallet>>, Query<&mut Wallet>)>,
) {
    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    let daily = *processed_day != Some(day);
    if !daily && requests.requested.is_empty() && companies.p0().is_empty() {
        return;
    }
    *processed_day = Some(day);
    let mut requested: HashMap<CompanyId, DividendRequest> = requests
        .requested
        .drain(..)
        .map(|request| (request.company, request))
        .collect();
    let people_by_id: HashMap<PersonId, Entity> = people
        .p0()
        .iter()
        .map(|(entity, id)| (*id, entity))
        .collect();
    let mut employees_by_site = HashMap::<shared::components::BuildingId, u8>::new();
    for employment in &employees {
        let count = employees_by_site.entry(employment.0).or_default();
        *count = count.saturating_add(1);
    }
    let market_by_settlement: HashMap<_, _> =
        markets.iter().map(|(id, market)| (*id, market)).collect();
    let mut sites_by_company: HashMap<CompanyId, Vec<DividendSite>> = HashMap::new();
    for (
        entity,
        building_id,
        operated_by,
        building,
        inventory,
        account,
        wage,
        procurement,
        management,
        staffing,
        building_of,
    ) in sites.p0().iter()
    {
        let procurement = procurement.copied().unwrap_or_default();
        let enabled_positions = staffing
            .copied()
            .unwrap_or_else(|| BusinessStaffingPolicy::new(building.kind.positions()))
            .target_for(building.kind);
        // A reduction can wait for existing cargo or a threshold crossing.
        // Those employees still earn wages until the ordinary release pass
        // ends their job, so dividends must protect their payroll as well.
        let protected_positions = enabled_positions.max(
            employees_by_site
                .get(building_id)
                .copied()
                .unwrap_or_default(),
        );
        let mut stock = [0; Good::COUNT];
        for good in Good::ALL {
            stock[good.index()] = inventory.amount(good);
        }
        let working = business_working_capital(
            protected_positions,
            wage,
            management,
            &procurement,
            Some(&stock),
            building_of.and_then(|home| market_by_settlement.get(&home.0).copied()),
        );
        let protected = account
            .wage_arrears
            .saturating_add(account.tax_arrears)
            .saturating_add(working.payroll)
            .saturating_add(working.inputs);
        sites_by_company
            .entry(operated_by.0)
            .or_default()
            .push(DividendSite {
                entity,
                building: *building_id,
                protected,
                gross_revenue: account.gross_revenue,
                operating_expenses: account.operating_expenses,
                prior_withdrawals: account.owner_withdrawals,
            });
    }

    let mut split = Vec::<(PersonId, u64)>::new();
    let mut no_sites = Vec::<DividendSite>::new();
    let mut companies = companies.p1();
    for (entity, company_id, ownership, mut company_account, mut policy, capacity) in
        companies.iter_mut()
    {
        let request = requested.remove(company_id);
        let never_reviewed = capacity
            .as_deref()
            .is_none_or(|capacity| capacity.day == u32::MAX);
        if !daily && request.is_none() && !never_reviewed {
            continue;
        }
        let company_sites = match sites_by_company.get_mut(company_id) {
            Some(sites) => {
                sites.sort_unstable_by_key(|site| site.building);
                sites.as_mut_slice()
            }
            None => no_sites.as_mut_slice(),
        };
        let has_sites = !company_sites.is_empty();
        let mut headroom = if has_sites {
            CompanyHeadroom::of(company_sites)
        } else {
            CompanyHeadroom {
                protected: 0,
                retained_profit: 0,
            }
        };
        let mut last_paid: Option<u64> = None;

        // Automatic distribution: once per day, bounded by policy.
        if daily && policy.automatic_dividends && policy.last_dividend_day != day {
            policy.last_dividend_day = day;
            let wanted = headroom
                .distributable(company_account.cash)
                .min(policy.max_daily_dividend);
            if has_sites
                && wanted > 0
                && pay_dividend(
                    day,
                    wanted,
                    ownership,
                    &mut company_account,
                    company_sites,
                    &people_by_id,
                    &mut people.p1(),
                    &mut sites.p1(),
                    &mut split,
                )
                .is_ok()
            {
                headroom = headroom.after_paying(wanted);
                last_paid = Some(wanted);
            }
        }

        // Manual distribution: clamp to the live figure and report honestly.
        if let Some(request) = request {
            let distributable = headroom.distributable(company_account.cash);
            let wanted = request.pennies.min(distributable);
            let result = if !has_sites {
                Err(DividendRefusal::NoOperatingSite)
            } else if wanted == 0 {
                Err(DividendRefusal::NothingDistributable)
            } else if ownership
                .shares()
                .iter()
                .any(|holding| !people_by_id.contains_key(&holding.shareholder))
            {
                Err(DividendRefusal::ShareholderUnreachable)
            } else {
                pay_dividend(
                    day,
                    wanted,
                    ownership,
                    &mut company_account,
                    company_sites,
                    &people_by_id,
                    &mut people.p1(),
                    &mut sites.p1(),
                    &mut split,
                )
            };
            let paid = if result.is_ok() { wanted } else { 0 };
            if result.is_ok() {
                headroom = headroom.after_paying(paid);
                last_paid = Some(paid);
            }
            let own_take = if result.is_ok() {
                split
                    .iter()
                    .find(|(holder, _)| *holder == request.requester_person)
                    .map_or(0, |(_, amount)| *amount)
            } else {
                0
            };
            outcomes.push(DividendOutcome {
                requester_link: request.requester_link,
                requester_person: request.requester_person,
                company: *company_id,
                paid,
                per_ten_shares: paid / u64::from(shared::components::COMPANY_TOTAL_SHARES / 10),
                shareholders: ownership.shares().len(),
                own_shares: ownership.share_count(request.requester_person),
                own_take,
                withheld_reserves: headroom.protected.min(company_account.cash),
                withheld_profit_cap: headroom.unearned_cash(company_account.cash),
                refusal: result.err(),
            });
        }

        // Publish the post-payout snapshot. Consolidate off-component so an
        // identical result never advances Bevy's replication change tick.
        let mut published = capacity.as_deref().copied().unwrap_or_default();
        published.day = day;
        published.distributable = headroom.distributable(company_account.cash);
        published.protected_reserves = headroom.protected;
        published.retained_profit = headroom.retained_profit;
        if let Some(paid) = last_paid {
            published.last_paid_day = day;
            published.last_paid = paid;
        }
        match capacity {
            Some(mut capacity) => {
                capacity.set_if_neq(published);
            }
            None => {
                commands.entity(entity).insert(published);
            }
        }
    }

    // A request whose company left the query since the order was accepted
    // still gets its one honest reply instead of vanishing from the queue.
    for request in requested.into_values() {
        outcomes.push(DividendOutcome {
            requester_link: request.requester_link,
            requester_person: request.requester_person,
            company: request.company,
            paid: 0,
            per_ten_shares: 0,
            shareholders: 0,
            own_shares: 0,
            own_take: 0,
            withheld_reserves: 0,
            withheld_profit_cap: 0,
            refusal: Some(DividendRefusal::CompanyUnavailable),
        });
    }
}

/// Sweep capital emitted by construction/legacy account deserialization into
/// the legal company's one treasury. In normal operation every site field is
/// zero after this pass and no system spends from it.
pub fn post_site_capital_to_company(
    mut sites: Query<(&OperatedBy, &mut BusinessAccount)>,
    mut companies: Query<(&CompanyId, &mut CompanyAccount)>,
) {
    let mut pending = HashMap::<CompanyId, u64>::new();
    for (operated_by, mut account) in sites.iter_mut() {
        if account.unposted_company_capital == 0 {
            continue;
        }
        let amount = std::mem::take(&mut account.unposted_company_capital);
        let total = pending.entry(operated_by.0).or_default();
        *total = total.saturating_add(amount);
    }
    for (company, amount) in pending {
        if let Some((_, mut account)) = companies.iter_mut().find(|(id, _)| **id == company) {
            account.credit(amount);
        } else {
            // `ensure_companies` is ordered before this system, so this can
            // only occur in a malformed test/world. Do not silently burn coin.
            for (operated_by, mut site) in sites.iter_mut() {
                if operated_by.0 == company {
                    site.unposted_company_capital =
                        site.unposted_company_capital.saturating_add(amount);
                    break;
                }
            }
        }
    }
}

/// Publish consolidated site liabilities and performance into the company
/// account. The authoritative treasury balance is deliberately left intact.
#[derive(Clone, Copy)]
struct CompanySiteAggregate {
    wage_arrears: u64,
    tax_arrears: u64,
    contributed_capital: u64,
    capital_expenditures: u64,
    book_value: u64,
    owner_withdrawals: u64,
    current_day: CompanyDayLedger,
    completed_day: Option<CompanyDayLedger>,
}

impl CompanySiteAggregate {
    fn new(day: u32) -> Self {
        Self {
            wage_arrears: 0,
            tax_arrears: 0,
            contributed_capital: 0,
            capital_expenditures: 0,
            book_value: 0,
            owner_withdrawals: 0,
            current_day: CompanyDayLedger::empty(day),
            completed_day: day.checked_sub(1).map(CompanyDayLedger::empty),
        }
    }
}

fn consolidate_business_day(target: &mut CompanyDayLedger, ledger: BusinessDayLedger) {
    target.external_revenue = target.external_revenue.saturating_add(ledger.gross_revenue);
    target.wage_expense = target.wage_expense.saturating_add(ledger.wage_expense);
    target.external_input_expense = target
        .external_input_expense
        .saturating_add(ledger.input_expense);
    target.market_fees = target.market_fees.saturating_add(ledger.market_fees);
    target.delivery_fees = target.delivery_fees.saturating_add(ledger.delivery_fees);
    target.profit_taxes = target.profit_taxes.saturating_add(ledger.profit_taxes);
    target.owner_withdrawals = target
        .owner_withdrawals
        .saturating_add(ledger.owner_withdrawals);
    target.capital_expenditures = target
        .capital_expenditures
        .saturating_add(ledger.capital_expenditures);
    target.internal_revenue = target
        .internal_revenue
        .saturating_add(ledger.internal_revenue);
    target.internal_input_expense = target
        .internal_input_expense
        .saturating_add(ledger.internal_input_expense);
}

pub fn refresh_company_accounts(
    world_time: Query<&WorldTime>,
    mut companies: Query<(&CompanyId, &mut CompanyAccount)>,
    sites: Query<(&OperatedBy, &BusinessAccount)>,
) {
    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    let completed_day = day.checked_sub(1);
    let mut aggregates = HashMap::<CompanyId, CompanySiteAggregate>::new();
    for (operated_by, account) in sites.iter() {
        let aggregate = aggregates
            .entry(operated_by.0)
            .or_insert_with(|| CompanySiteAggregate::new(day));
        aggregate.wage_arrears = aggregate.wage_arrears.saturating_add(account.wage_arrears);
        aggregate.tax_arrears = aggregate.tax_arrears.saturating_add(account.tax_arrears);
        aggregate.contributed_capital = aggregate
            .contributed_capital
            .saturating_add(account.contributed_capital);
        aggregate.capital_expenditures = aggregate
            .capital_expenditures
            .saturating_add(account.capital_expenditures);
        aggregate.book_value = aggregate.book_value.saturating_add(account.book_value);
        aggregate.owner_withdrawals = aggregate
            .owner_withdrawals
            .saturating_add(account.owner_withdrawals);
        if let Some(ledger) = account.ledger_for_day(day) {
            consolidate_business_day(&mut aggregate.current_day, ledger);
        }
        if let Some(completed_day) = completed_day {
            if let Some(ledger) = account.ledger_for_day(completed_day) {
                if let Some(company_day) = aggregate.completed_day.as_mut() {
                    consolidate_business_day(company_day, ledger);
                }
            }
        }
    }
    for (company_id, mut account) in companies.iter_mut() {
        let aggregate = aggregates
            .get(company_id)
            .copied()
            .unwrap_or_else(|| CompanySiteAggregate::new(day));
        // CompanyAccount is a small Copy value. Consolidate off-component so
        // an identical result never advances Bevy's replication change tick.
        let mut refreshed = *account;
        refreshed.refresh_from_sites(
            day,
            aggregate.wage_arrears,
            aggregate.tax_arrears,
            aggregate.contributed_capital,
            aggregate.capital_expenditures,
            aggregate.book_value,
            aggregate.owner_withdrawals,
            aggregate.current_day,
            aggregate.completed_day,
        );
        account.set_if_neq(refreshed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn company_bundle_issues_one_thousand_shares_and_keeps_founders_cash() {
        let mut world = World::new();
        let founder = PersonId(77);
        let company = CompanyId(9);
        let entity = world
            .spawn(new_company_bundle(
                company,
                "North Mill Company".into(),
                4,
                founder,
                1_250,
                1_250,
            ))
            .id();
        assert_eq!(world.get::<CompanyId>(entity), Some(&company));
        assert_eq!(
            world.get::<CompanyLeadership>(entity).unwrap().master,
            founder
        );
        assert_eq!(
            world
                .get::<CompanyOwnership>(entity)
                .unwrap()
                .share_count(founder),
            shared::components::COMPANY_TOTAL_SHARES
        );
        let account = world.get::<CompanyAccount>(entity).unwrap();
        assert_eq!(account.cash, 1_250);
        assert_eq!(account.contributed_capital, 1_250);
    }

    #[test]
    fn living_shareholder_may_keep_an_empty_funded_company() {
        let mut app = App::new();
        app.add_systems(Update, cleanup_empty_companies);
        let founder = PersonId(77);
        app.world_mut().spawn((founder, CharacterKind::Villager));
        let company = app
            .world_mut()
            .spawn(new_company_bundle(
                CompanyId(9),
                "Finished Company".into(),
                4,
                founder,
                1_250,
                1_250,
            ))
            .id();

        app.update();

        assert!(app.world().get_entity(company).is_ok());
        assert_eq!(
            app.world().get::<CompanyAccount>(company).unwrap().cash,
            1_250
        );
    }

    #[test]
    fn dead_empty_company_escheats_residual_cash_to_its_last_branch() {
        let mut app = App::new();
        app.add_systems(Update, cleanup_empty_companies);
        let settlement_id = shared::components::SettlementId(12);
        let hall = app
            .world_mut()
            .spawn((
                settlement_id,
                Settlement {
                    name: "Estateford".into(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 0,
                    treasury: 75,
                },
            ))
            .id();
        let mut branches = CompanyBranchPolicies::default();
        branches.set_resource(
            settlement_id,
            shared::economy::Good::Wood,
            shared::economy::CompanyResourcePolicy::default(),
        );
        let company = app
            .world_mut()
            .spawn(new_company_bundle(
                CompanyId(9),
                "Late Company".into(),
                4,
                PersonId(404),
                1_250,
                1_250,
            ))
            .insert(branches)
            .id();

        app.update();

        assert!(app.world().get_entity(company).is_err());
        assert_eq!(app.world().get::<Settlement>(hall).unwrap().treasury, 1_325);
    }

    #[test]
    fn completed_sites_and_projects_register_company_branches() {
        let mut app = App::new();
        app.add_systems(Update, ensure_company_branches);
        let company = CompanyId(21);
        let company_entity = app
            .world_mut()
            .spawn((company, CompanyBranchPolicies::default()))
            .id();
        let first = shared::components::SettlementId(7);
        let second = shared::components::SettlementId(9);
        app.world_mut()
            .spawn((OperatedBy(company), shared::components::BuildingOf(first)));
        app.world_mut().spawn((
            OperatedBy(company),
            UnderConstruction {
                kind: SettlementBuildingKind::Windmill,
                position: Vec3::ZERO,
                rotation: 0.0,
                owner: Some("Founder".into()),
                owner_id: Some(PersonId(1)),
                builder: None,
                settlement: Entity::PLACEHOLDER,
                settlement_id: second,
                stand: Vec3::ZERO,
                failed_stand_routes: 0,
                stage: BuildStage::Supplying,
                quality: 1.0,
            },
        ));

        app.update();

        let branches = app
            .world()
            .get::<CompanyBranchPolicies>(company_entity)
            .unwrap();
        assert!(branches.branch(first).is_some());
        assert!(branches.branch(second).is_some());
    }

    fn farm_site(company: CompanyId, id: u64, account: BusinessAccount) -> impl Bundle {
        (
            shared::components::BuildingId(id),
            OperatedBy(company),
            SettlementBuilding {
                kind: SettlementBuildingKind::Farmstead,
                settlement: "Shareford".into(),
                owner: Some("Founder".into()),
                quality: 1.0,
                workers: Vec::new(),
            },
            account,
            BusinessWagePolicy::default(),
            BusinessProcurementPolicy::none(),
            BusinessManagementPolicy::default(),
            BusinessCondition {
                state: BusinessState::Operating,
                ..default()
            },
            GoodsInventory::new(shared::economy::capacity::FARMSTEAD),
        )
    }

    #[test]
    fn dividend_follows_the_exact_one_thousand_share_cap_table() {
        let mut app = App::new();
        app.init_resource::<CompanyDividendQueue>();
        app.init_resource::<CompanyDividendOutcomes>();
        app.add_systems(Update, review_company_finance);
        app.world_mut().spawn(WorldTime::new_default());
        let founder = PersonId(1);
        let partner = PersonId(2);
        let company = CompanyId(10);
        let mut ownership = CompanyOwnership::sole(founder);
        assert!(ownership.transfer(founder, partner, 400));
        app.world_mut().spawn((founder, Wallet::new(0)));
        app.world_mut().spawn((partner, Wallet::new(0)));
        let company_entity = app
            .world_mut()
            .spawn((
                company,
                ownership,
                CompanyAccount {
                    cash: 2_000,
                    ..default()
                },
                CompanyManagementPolicy::default(),
            ))
            .id();
        let mut account = BusinessAccount::default();
        account.record_sale(0, 1_000, 0, 10);
        let site = app.world_mut().spawn(farm_site(company, 11, account)).id();
        app.world_mut()
            .resource_mut::<CompanyDividendQueue>()
            .request(company, u64::MAX, PersonId(0), Entity::PLACEHOLDER);

        app.update();

        let balances: HashMap<PersonId, u64> = app
            .world_mut()
            .query::<(&PersonId, &Wallet)>()
            .iter(app.world())
            .map(|(id, wallet)| (*id, wallet.balance()))
            .collect();
        assert_eq!(balances[&founder], 600);
        assert_eq!(balances[&partner], 400);
        assert_eq!(
            app.world()
                .get::<CompanyAccount>(company_entity)
                .unwrap()
                .cash,
            1_000
        );
        assert_eq!(
            app.world()
                .get::<BusinessAccount>(site)
                .unwrap()
                .owner_withdrawals,
            1_000
        );
    }

    #[test]
    fn a_large_dividend_preserves_exact_share_ratios_without_product_overflow() {
        let mut app = App::new();
        app.init_resource::<CompanyDividendQueue>();
        app.init_resource::<CompanyDividendOutcomes>();
        app.add_systems(Update, review_company_finance);
        app.world_mut().spawn(WorldTime::new_default());
        let company = CompanyId(990);
        let first_id = PersonId(990);
        let second_id = PersonId(991);
        let first = app.world_mut().spawn((first_id, Wallet::new(0))).id();
        let second = app.world_mut().spawn((second_id, Wallet::new(0))).id();
        let mut ownership = CompanyOwnership::sole(first_id);
        assert!(ownership.transfer(first_id, second_id, 400));
        let payout = u64::MAX / 2;
        let treasury = app
            .world_mut()
            .spawn((
                company,
                ownership,
                CompanyAccount {
                    cash: u64::MAX,
                    ..default()
                },
                CompanyManagementPolicy::default(),
            ))
            .id();
        app.world_mut().spawn(farm_site(
            company,
            992,
            BusinessAccount {
                gross_revenue: payout,
                ..default()
            },
        ));
        app.world_mut()
            .resource_mut::<CompanyDividendQueue>()
            .request(company, u64::MAX, PersonId(0), Entity::PLACEHOLDER);
        app.update();

        let second_due = (u128::from(payout) * 400 / 1_000) as u64;
        assert_eq!(
            app.world().get::<Wallet>(second).unwrap().balance(),
            second_due
        );
        assert_eq!(
            app.world().get::<Wallet>(first).unwrap().balance(),
            payout - second_due
        );
        assert_eq!(
            app.world().get::<CompanyAccount>(treasury).unwrap().cash,
            u64::MAX - payout
        );
        let total = u128::from(app.world().get::<Wallet>(first).unwrap().balance())
            + u128::from(app.world().get::<Wallet>(second).unwrap().balance())
            + u128::from(app.world().get::<CompanyAccount>(treasury).unwrap().cash);
        assert_eq!(total, u128::from(u64::MAX));
    }

    #[test]
    fn a_full_shareholder_wallet_keeps_the_whole_dividend_in_the_company() {
        let mut app = App::new();
        app.init_resource::<CompanyDividendQueue>();
        app.init_resource::<CompanyDividendOutcomes>();
        app.add_systems(Update, review_company_finance);
        app.world_mut().spawn(WorldTime::new_default());
        let company = CompanyId(994);
        let first_id = PersonId(994);
        let second_id = PersonId(995);
        let first = app
            .world_mut()
            .spawn((first_id, Wallet::new(u64::MAX - 1)))
            .id();
        let second = app.world_mut().spawn((second_id, Wallet::new(0))).id();
        let mut ownership = CompanyOwnership::sole(first_id);
        assert!(ownership.transfer(first_id, second_id, 400));
        let treasury = app
            .world_mut()
            .spawn((
                company,
                ownership,
                CompanyAccount {
                    cash: 2_000,
                    ..default()
                },
                CompanyManagementPolicy::default(),
            ))
            .id();
        let site = app
            .world_mut()
            .spawn(farm_site(
                company,
                996,
                BusinessAccount {
                    gross_revenue: 1_000,
                    ..default()
                },
            ))
            .id();
        app.world_mut()
            .resource_mut::<CompanyDividendQueue>()
            .request(company, u64::MAX, PersonId(0), Entity::PLACEHOLDER);
        app.update();
        assert_eq!(
            app.world().get::<CompanyAccount>(treasury).unwrap().cash,
            2_000
        );
        assert_eq!(
            app.world()
                .get::<CompanyAccount>(treasury)
                .unwrap()
                .owner_withdrawals,
            0
        );
        assert_eq!(
            app.world()
                .get::<BusinessAccount>(site)
                .unwrap()
                .owner_withdrawals,
            0
        );
        assert_eq!(
            app.world().get::<Wallet>(first).unwrap().balance(),
            u64::MAX - 1
        );
        assert_eq!(app.world().get::<Wallet>(second).unwrap().balance(), 0);

        assert!(app
            .world_mut()
            .get_mut::<Wallet>(first)
            .unwrap()
            .debit(1_000));
        app.world_mut()
            .resource_mut::<CompanyDividendQueue>()
            .request(company, u64::MAX, PersonId(0), Entity::PLACEHOLDER);
        app.update();
        assert_eq!(
            app.world().get::<CompanyAccount>(treasury).unwrap().cash,
            1_000
        );
        assert_eq!(
            app.world().get::<Wallet>(first).unwrap().balance(),
            u64::MAX - 401
        );
        assert_eq!(app.world().get::<Wallet>(second).unwrap().balance(), 400);
    }

    #[test]
    fn dividends_protect_employees_until_a_deferred_layoff_really_finishes() {
        let mut app = App::new();
        app.init_resource::<CompanyDividendQueue>();
        app.init_resource::<CompanyDividendOutcomes>();
        app.add_systems(Update, review_company_finance);
        app.world_mut().spawn(WorldTime::new_default());
        let founder = PersonId(960);
        let company = CompanyId(961);
        let person = app.world_mut().spawn((founder, Wallet::new(0))).id();
        let treasury = app
            .world_mut()
            .spawn((
                company,
                CompanyOwnership::sole(founder),
                CompanyAccount {
                    cash: 700,
                    ..default()
                },
                CompanyManagementPolicy::default(),
            ))
            .id();
        let mut account = BusinessAccount::default();
        account.record_sale(0, 2_000, 0, 20);
        app.world_mut()
            .spawn(farm_site(company, 962, account))
            .insert((
                BusinessStaffingPolicy::new(0),
                BusinessWagePolicy {
                    daily_wage: 100,
                    ..default()
                },
            ));
        let worker = app
            .world_mut()
            .spawn(shared::components::EmployedAt(
                shared::components::BuildingId(962),
            ))
            .id();
        app.world_mut()
            .resource_mut::<CompanyDividendQueue>()
            .request(company, u64::MAX, PersonId(0), Entity::PLACEHOLDER);
        app.update();
        assert_eq!(
            app.world().get::<CompanyAccount>(treasury).unwrap().cash,
            500
        );
        assert_eq!(app.world().get::<Wallet>(person).unwrap().balance(), 200);

        // Once cargo/door work is finished, removing the durable employment
        // releases that payroll reserve on the next explicit dividend request.
        app.world_mut()
            .entity_mut(worker)
            .remove::<shared::components::EmployedAt>();
        app.world_mut()
            .resource_mut::<CompanyDividendQueue>()
            .request(company, u64::MAX, PersonId(0), Entity::PLACEHOLDER);
        app.update();
        assert_eq!(
            app.world().get::<CompanyAccount>(treasury).unwrap().cash,
            200
        );
        assert_eq!(app.world().get::<Wallet>(person).unwrap().balance(), 500);
    }

    #[test]
    fn dividends_protect_input_shortfall_at_the_actual_local_quote() {
        let mut app = App::new();
        app.init_resource::<CompanyDividendQueue>();
        app.init_resource::<CompanyDividendOutcomes>();
        app.add_systems(Update, review_company_finance);
        app.world_mut().spawn(WorldTime::new_default());
        let founder = PersonId(970);
        let company = CompanyId(971);
        let town = shared::components::SettlementId(972);
        let person = app.world_mut().spawn((founder, Wallet::new(0))).id();
        let treasury = app
            .world_mut()
            .spawn((
                company,
                CompanyOwnership::sole(founder),
                CompanyAccount {
                    cash: 1_000,
                    ..default()
                },
                CompanyManagementPolicy::default(),
            ))
            .id();
        let mut market = MootMarket::founding();
        market.consign(
            MarketSeller::Business(shared::components::BuildingId(974)),
            Good::Flour,
            8,
            200,
        );
        app.world_mut().spawn((
            town,
            market,
            Settlement {
                name: "Millford".into(),
                tier: shared::components::SettlementTier::Village,
                residents: 1,
                treasury: 0,
            },
        ));
        let mut account = BusinessAccount::default();
        account.record_sale(0, 2_000, 0, 20);
        let mut stock = GoodsInventory::new(100);
        stock.add(Good::Flour, 1);
        app.world_mut()
            .spawn(farm_site(company, 973, account))
            .insert((
                shared::components::BuildingOf(town),
                BusinessStaffingPolicy::new(0),
                stock,
                BusinessProcurementPolicy::none().with_rule(
                    Good::Flour,
                    BusinessInputRule {
                        enabled: true,
                        target_units: 4,
                        ..default()
                    },
                ),
            ));
        app.world_mut()
            .resource_mut::<CompanyDividendQueue>()
            .request(company, u64::MAX, PersonId(0), Entity::PLACEHOLDER);
        app.update();
        assert_eq!(
            app.world().get::<CompanyAccount>(treasury).unwrap().cash,
            800,
            "three missing Flour at the live 200-penny quote plus one 200-penny company buffer"
        );
        assert_eq!(app.world().get::<Wallet>(person).unwrap().balance(), 200);
    }

    const REQUESTER_LINK: Entity = Entity::from_raw_u32(777).unwrap();

    fn finance_app() -> App {
        let mut app = App::new();
        app.init_resource::<CompanyDividendQueue>();
        app.init_resource::<CompanyDividendOutcomes>();
        app.add_systems(Update, review_company_finance);
        app.world_mut().spawn(WorldTime::new_default());
        app
    }

    fn request_dividend(app: &mut App, company: CompanyId, pennies: u64, requester: PersonId) {
        app.world_mut()
            .resource_mut::<CompanyDividendQueue>()
            .request(company, pennies, requester, REQUESTER_LINK);
    }

    fn take_outcomes(app: &mut App) -> Vec<DividendOutcome> {
        app.world_mut()
            .resource_mut::<CompanyDividendOutcomes>()
            .drain()
            .collect()
    }

    fn capacity_of(app: &App, company: Entity) -> CompanyDividendCapacity {
        *app.world()
            .get::<CompanyDividendCapacity>(company)
            .expect("the finance pass publishes a capacity on every company it reviews")
    }

    fn balance_of(app: &App, person: Entity) -> u64 {
        app.world().get::<Wallet>(person).unwrap().balance()
    }

    #[test]
    fn manual_dividend_pays_exactly_the_requested_amount() {
        let mut app = finance_app();
        let founder = PersonId(1);
        let partner = PersonId(2);
        let company = CompanyId(10);
        let mut ownership = CompanyOwnership::sole(founder);
        assert!(ownership.transfer(founder, partner, 400));
        let founder_entity = app.world_mut().spawn((founder, Wallet::new(0))).id();
        let partner_entity = app.world_mut().spawn((partner, Wallet::new(0))).id();
        let treasury = app
            .world_mut()
            .spawn((
                company,
                ownership,
                CompanyAccount {
                    cash: 2_000,
                    ..default()
                },
                CompanyManagementPolicy {
                    automatic_dividends: false,
                    ..default()
                },
            ))
            .id();
        let mut account = BusinessAccount::default();
        account.record_sale(0, 1_000, 0, 10);
        let site = app.world_mut().spawn(farm_site(company, 11, account)).id();
        request_dividend(&mut app, company, 300, founder);

        app.update();

        assert_eq!(balance_of(&app, founder_entity), 180);
        assert_eq!(balance_of(&app, partner_entity), 120);
        let treasury_account = app.world().get::<CompanyAccount>(treasury).unwrap();
        assert_eq!(treasury_account.cash, 1_700);
        assert_eq!(treasury_account.owner_withdrawals, 300);
        assert_eq!(
            app.world()
                .get::<BusinessAccount>(site)
                .unwrap()
                .owner_withdrawals,
            300
        );
        let capacity = capacity_of(&app, treasury);
        assert_eq!(capacity.day, 0);
        assert_eq!(capacity.distributable, 700);
        assert_eq!(capacity.retained_profit, 700);
        assert_eq!((capacity.last_paid_day, capacity.last_paid), (0, 300));
        assert!(capacity.protected_reserves > 0 && capacity.protected_reserves <= 1_000);
        assert_eq!(
            app.world()
                .get::<CompanyManagementPolicy>(treasury)
                .unwrap()
                .last_dividend_day,
            u32::MAX,
            "a manual payout never consumes the automatic one-per-day slot"
        );
        let outcomes = take_outcomes(&mut app);
        assert_eq!(outcomes.len(), 1);
        let outcome = outcomes[0];
        assert_eq!(outcome.requester_link, REQUESTER_LINK);
        assert_eq!(outcome.requester_person, founder);
        assert_eq!(outcome.company, company);
        assert_eq!(outcome.paid, 300);
        assert_eq!(outcome.per_ten_shares, 3);
        assert_eq!(outcome.shareholders, 2);
        assert_eq!(outcome.own_shares, 600);
        assert_eq!(outcome.own_take, 180);
        assert_eq!(outcome.refusal, None);
        assert_eq!(outcome.withheld_reserves, capacity.protected_reserves);
        assert_eq!(
            outcome.withheld_profit_cap,
            1_700 - capacity.protected_reserves - 700,
            "free cash that is not earned profit is reported, not distributable"
        );
    }

    #[test]
    fn manual_dividend_above_distributable_is_clamped_and_reports_withheld() {
        let mut app = finance_app();
        let founder = PersonId(20);
        let company = CompanyId(21);
        let person = app.world_mut().spawn((founder, Wallet::new(0))).id();
        let treasury = app
            .world_mut()
            .spawn((
                company,
                CompanyOwnership::sole(founder),
                CompanyAccount {
                    cash: 2_000,
                    ..default()
                },
                CompanyManagementPolicy {
                    automatic_dividends: false,
                    ..default()
                },
            ))
            .id();
        app.world_mut().spawn(farm_site(
            company,
            22,
            BusinessAccount {
                gross_revenue: 800,
                ..default()
            },
        ));
        request_dividend(&mut app, company, 5_000, founder);

        app.update();

        assert_eq!(balance_of(&app, person), 800);
        assert_eq!(
            app.world().get::<CompanyAccount>(treasury).unwrap().cash,
            1_200
        );
        let capacity = capacity_of(&app, treasury);
        assert_eq!(capacity.distributable, 0);
        assert_eq!(capacity.retained_profit, 0);
        assert_eq!(capacity.last_paid, 800);
        let outcome = take_outcomes(&mut app)[0];
        assert_eq!(outcome.paid, 800, "clamped to the live figure, not refused");
        assert_eq!(outcome.refusal, None);
        assert_eq!(outcome.own_take, 800);
        assert_eq!(outcome.per_ten_shares, 8);
        assert!(outcome.withheld_reserves > 0);
        assert_eq!(outcome.withheld_reserves, capacity.protected_reserves);
        assert!(outcome.withheld_profit_cap > 0);
        assert_eq!(
            outcome.paid + outcome.withheld_reserves + outcome.withheld_profit_cap,
            2_000,
            "the report decomposes the whole pre-payout treasury"
        );
    }

    #[test]
    fn a_loss_making_site_offsets_its_profitable_sibling_in_the_consolidated_headroom() {
        // Cash 2,400 = 2,000 contributed capital + 1,000 sales - 600 wages.
        // Site A earned 1,000, site B spent 600 before its first sale, so the
        // company's consolidated retained profit is 400: a saturating per-site
        // sum would report 1,000 and pay 600 of contributed capital out.
        let mut app = finance_app();
        let founder = PersonId(40);
        let company = CompanyId(41);
        let person = app.world_mut().spawn((founder, Wallet::new(0))).id();
        let treasury = app
            .world_mut()
            .spawn((
                company,
                CompanyOwnership::sole(founder),
                CompanyAccount {
                    cash: 2_400,
                    contributed_capital: 2_000,
                    ..default()
                },
                CompanyManagementPolicy {
                    automatic_dividends: false,
                    ..default()
                },
            ))
            .id();
        app.world_mut().spawn(farm_site(
            company,
            42,
            BusinessAccount {
                gross_revenue: 1_000,
                ..default()
            },
        ));
        app.world_mut().spawn(farm_site(
            company,
            43,
            BusinessAccount {
                operating_expenses: 600,
                ..default()
            },
        ));
        request_dividend(&mut app, company, u64::MAX, founder);

        app.update();

        assert_eq!(
            balance_of(&app, person),
            400,
            "only consolidated profit is paid"
        );
        let account = app.world().get::<CompanyAccount>(treasury).unwrap();
        assert_eq!(account.cash, 2_000);
        assert!(
            account.cash >= account.contributed_capital,
            "contributed capital never leaves the treasury as a dividend"
        );
        let capacity = capacity_of(&app, treasury);
        assert_eq!(capacity.distributable, 0);
        assert_eq!(capacity.retained_profit, 0);
        assert_eq!(capacity.last_paid, 400);
        let outcome = take_outcomes(&mut app)[0];
        assert_eq!(outcome.paid, 400);
        assert_eq!(outcome.refusal, None);
        assert_eq!(
            outcome.paid + outcome.withheld_reserves + outcome.withheld_profit_cap,
            2_400
        );
        assert!(
            outcome.withheld_profit_cap >= 2_000 - outcome.withheld_reserves,
            "the unearned contributed capital is reported as held back"
        );
    }

    #[test]
    fn a_refused_dividend_never_marks_the_replicated_treasury_changed() {
        #[derive(Resource, Default)]
        struct AccountChanges(usize);
        fn count_changes(
            mut changes: ResMut<AccountChanges>,
            accounts: Query<(), Changed<CompanyAccount>>,
        ) {
            changes.0 = accounts.iter().count();
        }

        let mut app = finance_app();
        app.init_resource::<AccountChanges>()
            .add_systems(Update, count_changes.after(review_company_finance));
        let founder = PersonId(50);
        let company = CompanyId(51);
        app.world_mut().spawn((founder, Wallet::new(0)));
        let treasury = app
            .world_mut()
            .spawn((
                company,
                CompanyOwnership::sole(founder),
                CompanyAccount {
                    cash: 2_000,
                    ..default()
                },
                // Automatic dividends on (the policy default) with an operating
                // site that has earned nothing: the daily pass wants 0.
                CompanyManagementPolicy::default(),
            ))
            .id();
        app.world_mut()
            .spawn(farm_site(company, 52, BusinessAccount::default()));
        app.update();

        let clock = app
            .world_mut()
            .query_filtered::<Entity, With<WorldTime>>()
            .single(app.world())
            .unwrap();
        app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 1;
        app.update();
        assert_eq!(
            app.world().resource::<AccountChanges>().0,
            0,
            "a zero automatic payout must not dirty the replicated account"
        );

        request_dividend(&mut app, company, u64::MAX, founder);
        app.update();
        assert_eq!(
            app.world().resource::<AccountChanges>().0,
            0,
            "a refused manual request must not dirty the replicated account"
        );
        let outcome = take_outcomes(&mut app)[0];
        assert_eq!(outcome.refusal, Some(DividendRefusal::NothingDistributable));
        assert_eq!(
            app.world().get::<CompanyAccount>(treasury).unwrap().cash,
            2_000
        );
    }

    #[test]
    fn a_company_founded_mid_day_is_reviewed_before_the_next_day_boundary() {
        let mut app = finance_app();
        // The daily pass for day 0 has already run when the company is founded.
        app.update();
        let founder = PersonId(60);
        let company = CompanyId(61);
        app.world_mut().spawn((founder, Wallet::new(0)));
        let treasury = app
            .world_mut()
            .spawn((
                company,
                CompanyOwnership::sole(founder),
                CompanyAccount {
                    cash: 2_000,
                    ..default()
                },
                CompanyManagementPolicy {
                    automatic_dividends: false,
                    ..default()
                },
                CompanyDividendCapacity::default(),
            ))
            .id();
        app.world_mut().spawn(farm_site(
            company,
            62,
            BusinessAccount {
                gross_revenue: 1_000,
                ..default()
            },
        ));

        app.update();

        let capacity = capacity_of(&app, treasury);
        assert_eq!(
            capacity.day, 0,
            "a never-reviewed capacity is published the same day, not at the next boundary"
        );
        assert_eq!(capacity.retained_profit, 1_000);
        assert!(capacity.distributable > 0 && capacity.distributable <= 1_000);
        assert_eq!(capacity.last_paid_day, u32::MAX);

        // Once published the company is not re-reviewed every tick.
        let published = capacity;
        let site = app
            .world_mut()
            .query_filtered::<Entity, With<OperatedBy>>()
            .single(app.world())
            .unwrap();
        app.world_mut()
            .get_mut::<BusinessAccount>(site)
            .unwrap()
            .record_sale(0, 500, 0, 5);
        app.update();
        assert_eq!(
            capacity_of(&app, treasury),
            published,
            "an already-reviewed company waits for the day boundary or a request"
        );
    }

    #[test]
    fn a_request_for_a_vanished_company_still_gets_one_reply() {
        let mut app = finance_app();
        app.update();
        request_dividend(&mut app, CompanyId(70), 500, PersonId(71));

        app.update();

        let outcomes = take_outcomes(&mut app);
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].requester_link, REQUESTER_LINK);
        assert_eq!(outcomes[0].requester_person, PersonId(71));
        assert_eq!(outcomes[0].company, CompanyId(70));
        assert_eq!(outcomes[0].paid, 0);
        assert_eq!(
            outcomes[0].refusal,
            Some(DividendRefusal::CompanyUnavailable)
        );
        assert!(app
            .world()
            .resource::<CompanyDividendQueue>()
            .pending()
            .is_empty());
    }

    #[test]
    fn zero_distributable_manual_request_reports_a_refusal_instead_of_silence() {
        let mut app = finance_app();
        let founder = PersonId(30);
        let company = CompanyId(31);
        let person = app.world_mut().spawn((founder, Wallet::new(50))).id();
        let treasury = app
            .world_mut()
            .spawn((
                company,
                CompanyOwnership::sole(founder),
                CompanyAccount {
                    cash: 1_000,
                    contributed_capital: 1_000,
                    ..default()
                },
                CompanyManagementPolicy::default(),
            ))
            .id();
        app.world_mut()
            .spawn(farm_site(company, 32, BusinessAccount::default()));
        request_dividend(&mut app, company, u64::MAX, founder);

        app.update();

        assert_eq!(balance_of(&app, person), 50);
        assert_eq!(
            app.world().get::<CompanyAccount>(treasury).unwrap().cash,
            1_000
        );
        let capacity = capacity_of(&app, treasury);
        assert_eq!(capacity.distributable, 0);
        assert_eq!(capacity.last_paid_day, u32::MAX);
        let outcome = take_outcomes(&mut app)[0];
        assert_eq!(outcome.refusal, Some(DividendRefusal::NothingDistributable));
        assert_eq!(
            (outcome.paid, outcome.own_take, outcome.per_ten_shares),
            (0, 0, 0)
        );
        assert_eq!(outcome.withheld_reserves, capacity.protected_reserves);
        assert_eq!(
            outcome.withheld_reserves + outcome.withheld_profit_cap,
            1_000,
            "contributed capital is reserves plus not-yet-earned cash, never a dividend"
        );
    }

    #[test]
    fn no_operating_site_manual_request_names_the_reason() {
        let mut app = finance_app();
        let founder = PersonId(40);
        let company = CompanyId(41);
        let person = app.world_mut().spawn((founder, Wallet::new(0))).id();
        let treasury = app
            .world_mut()
            .spawn(new_company_bundle(
                company,
                "Paper Company".into(),
                0,
                founder,
                5_000,
                5_000,
            ))
            .id();
        request_dividend(&mut app, company, 100, founder);

        app.update();

        assert_eq!(balance_of(&app, person), 0);
        assert_eq!(
            app.world().get::<CompanyAccount>(treasury).unwrap().cash,
            5_000
        );
        let outcome = take_outcomes(&mut app)[0];
        assert_eq!(outcome.refusal, Some(DividendRefusal::NoOperatingSite));
        assert_eq!(outcome.paid, 0);
        assert_eq!(outcome.own_shares, 1_000);
        assert_eq!(
            capacity_of(&app, treasury),
            CompanyDividendCapacity {
                day: 0,
                distributable: 0,
                protected_reserves: 0,
                retained_profit: 0,
                last_paid_day: u32::MAX,
                last_paid: 0,
            }
        );
    }

    #[test]
    fn manual_dividend_is_allowed_after_the_same_days_automatic_payout() {
        let mut app = finance_app();
        let founder = PersonId(50);
        let company = CompanyId(51);
        let person = app.world_mut().spawn((founder, Wallet::new(0))).id();
        let treasury = app
            .world_mut()
            .spawn((
                company,
                CompanyOwnership::sole(founder),
                CompanyAccount {
                    cash: 2_000,
                    ..default()
                },
                CompanyManagementPolicy::default(),
            ))
            .id();
        let mut account = BusinessAccount::default();
        account.record_sale(0, 1_000, 0, 10);
        app.world_mut().spawn(farm_site(company, 52, account));

        // Day start: the automatic policy pays its bounded 2-coin dividend.
        app.update();
        assert_eq!(balance_of(&app, person), 200);
        let policy = *app
            .world()
            .get::<CompanyManagementPolicy>(treasury)
            .unwrap();
        assert_eq!(policy.last_dividend_day, 0);
        let capacity = capacity_of(&app, treasury);
        assert_eq!(
            (
                capacity.distributable,
                capacity.last_paid,
                capacity.last_paid_day
            ),
            (800, 200, 0),
            "capacity is published after the automatic payout"
        );
        assert!(
            take_outcomes(&mut app).is_empty(),
            "automatic payouts report nothing"
        );

        // Same day: a manual request is exempt from the cap and the cadence.
        request_dividend(&mut app, company, u64::MAX, founder);
        app.update();
        assert_eq!(balance_of(&app, person), 1_000);
        assert_eq!(
            app.world().get::<CompanyAccount>(treasury).unwrap().cash,
            1_000
        );
        assert_eq!(
            *app.world()
                .get::<CompanyManagementPolicy>(treasury)
                .unwrap(),
            policy,
            "the manual payout leaves the automatic policy untouched"
        );
        let capacity = capacity_of(&app, treasury);
        assert_eq!((capacity.distributable, capacity.last_paid), (0, 800));
        let outcome = take_outcomes(&mut app)[0];
        assert_eq!(
            (outcome.paid, outcome.own_take, outcome.refusal),
            (800, 800, None)
        );
    }

    #[test]
    fn dividend_capacity_is_published_after_the_automatic_payout_and_only_when_changed() {
        let mut app = finance_app();
        let founder = PersonId(60);
        let company = CompanyId(61);
        app.world_mut().spawn((founder, Wallet::new(0)));
        let treasury = app
            .world_mut()
            .spawn((
                company,
                CompanyOwnership::sole(founder),
                CompanyAccount {
                    cash: 2_000,
                    ..default()
                },
                CompanyManagementPolicy::default(),
                CompanyDividendCapacity::default(),
            ))
            .id();
        let mut account = BusinessAccount::default();
        account.record_sale(0, 1_000, 0, 10);
        app.world_mut().spawn(farm_site(company, 62, account));
        let changed = |app: &mut App| {
            app.world_mut()
                .query_filtered::<Entity, Changed<CompanyDividendCapacity>>()
                .iter(app.world())
                .any(|entity| entity == treasury)
        };

        // `App::update` clears trackers afterwards; drive the schedule directly.
        app.world_mut().clear_trackers();
        app.world_mut().run_schedule(Update);
        assert!(changed(&mut app));
        let capacity = capacity_of(&app, treasury);
        assert_eq!(
            capacity.distributable, 800,
            "1,000 of retained profit minus the 200 automatic payout already made this pass"
        );
        assert_eq!(capacity.retained_profit, 800);
        assert_eq!((capacity.last_paid_day, capacity.last_paid), (0, 200));

        // A same-day tick without a request does not run the pass at all.
        app.world_mut().clear_trackers();
        app.world_mut().run_schedule(Update);
        assert!(!changed(&mut app));

        // A request that pays changes the snapshot.
        request_dividend(&mut app, company, u64::MAX, founder);
        app.world_mut().clear_trackers();
        app.world_mut().run_schedule(Update);
        assert!(changed(&mut app));
        assert_eq!(capacity_of(&app, treasury).distributable, 0);
        assert_eq!(take_outcomes(&mut app).len(), 1);

        // A request that pays nothing recomputes an identical snapshot and
        // must not advance the replication change tick.
        request_dividend(&mut app, company, u64::MAX, founder);
        app.world_mut().clear_trackers();
        app.world_mut().run_schedule(Update);
        assert!(
            !changed(&mut app),
            "an unchanged capacity must not be marked changed"
        );
        let outcome = take_outcomes(&mut app)[0];
        assert_eq!(outcome.refusal, Some(DividendRefusal::NothingDistributable));
    }

    #[test]
    fn a_company_without_a_capacity_component_still_pays_and_gets_one_inserted() {
        let mut app = finance_app();
        let founder = PersonId(70);
        let company = CompanyId(71);
        let person = app.world_mut().spawn((founder, Wallet::new(0))).id();
        // Hand-spawned like the older fixtures and labs: no capacity component.
        let treasury = app
            .world_mut()
            .spawn((
                company,
                CompanyOwnership::sole(founder),
                CompanyAccount {
                    cash: 2_000,
                    ..default()
                },
                CompanyManagementPolicy {
                    automatic_dividends: false,
                    ..default()
                },
            ))
            .id();
        assert!(app
            .world()
            .get::<CompanyDividendCapacity>(treasury)
            .is_none());
        let mut account = BusinessAccount::default();
        account.record_sale(0, 1_000, 0, 10);
        app.world_mut().spawn(farm_site(company, 72, account));
        request_dividend(&mut app, company, 400, founder);

        app.update();

        assert_eq!(balance_of(&app, person), 400);
        let capacity = capacity_of(&app, treasury);
        assert_eq!(
            (capacity.day, capacity.distributable, capacity.last_paid),
            (0, 600, 400)
        );
        assert_eq!(take_outcomes(&mut app)[0].paid, 400);
    }

    #[test]
    fn distributed_profit_is_not_paid_twice_when_the_memo_site_is_removed() {
        let mut app = finance_app();
        let founder = PersonId(80);
        let company = CompanyId(81);
        let person = app.world_mut().spawn((founder, Wallet::new(0))).id();
        let treasury = app
            .world_mut()
            .spawn((
                company,
                CompanyOwnership::sole(founder),
                CompanyAccount {
                    cash: 3_000,
                    ..default()
                },
                CompanyManagementPolicy {
                    automatic_dividends: false,
                    ..default()
                },
            ))
            .id();
        // The first sorted site earned nothing; the second earned everything.
        let idle = app
            .world_mut()
            .spawn(farm_site(company, 82, BusinessAccount::default()))
            .id();
        let mut earning = BusinessAccount::default();
        earning.record_sale(0, 1_000, 0, 10);
        let earning = app.world_mut().spawn(farm_site(company, 83, earning)).id();
        request_dividend(&mut app, company, u64::MAX, founder);
        app.update();
        assert_eq!(balance_of(&app, person), 1_000);
        assert_eq!(
            app.world().get::<CompanyAccount>(treasury).unwrap().cash,
            2_000
        );
        assert_eq!(
            app.world()
                .get::<BusinessAccount>(idle)
                .unwrap()
                .owner_withdrawals,
            0,
            "the memo is attributed to the site whose profit was paid out"
        );
        assert_eq!(
            app.world()
                .get::<BusinessAccount>(earning)
                .unwrap()
                .owner_withdrawals,
            1_000
        );
        take_outcomes(&mut app);

        app.world_mut().despawn(idle);
        request_dividend(&mut app, company, u64::MAX, founder);
        app.update();
        assert_eq!(
            balance_of(&app, person),
            1_000,
            "already-distributed profit must not become distributable again"
        );
        assert_eq!(
            app.world().get::<CompanyAccount>(treasury).unwrap().cash,
            2_000
        );
        let outcome = take_outcomes(&mut app)[0];
        assert_eq!(outcome.refusal, Some(DividendRefusal::NothingDistributable));
        assert_eq!(capacity_of(&app, treasury).distributable, 0);
    }

    #[test]
    fn a_manual_company_choice_reaches_automatic_siblings_on_the_same_day() {
        let mut app = App::new();
        app.add_systems(Update, review_company_strategies);
        app.world_mut().spawn(WorldTime::new_default());
        let company = CompanyId(980);
        let treasury = app
            .world_mut()
            .spawn((
                company,
                CompanyLeadership {
                    master: PersonId(980),
                },
                CompanyAccount::default(),
                CompanyManagementPolicy {
                    autopilot: false,
                    ..default()
                },
                CompanyDecisionHistory::default(),
            ))
            .id();
        let selected = app
            .world_mut()
            .spawn(farm_site(company, 981, default()))
            .id();
        let sibling = app
            .world_mut()
            .spawn(farm_site(company, 982, default()))
            .id();
        let manual = app
            .world_mut()
            .spawn(farm_site(company, 983, default()))
            .insert(BusinessManagementPolicy {
                autopilot: false,
                ..BusinessManagementPolicy::for_strategy(BusinessStrategy::Conservative)
            })
            .id();
        app.update();

        // The owner command changes the selected site and its company policy.
        // The executive is paused and has already reviewed this exact day.
        *app.world_mut()
            .get_mut::<BusinessManagementPolicy>(selected)
            .unwrap() = BusinessManagementPolicy::for_strategy(BusinessStrategy::Aggressive);
        {
            let mut policy = app
                .world_mut()
                .get_mut::<CompanyManagementPolicy>(treasury)
                .unwrap();
            policy.strategy = BusinessStrategy::Aggressive;
            policy.payroll_reserve_days = BusinessStrategy::Aggressive.payroll_reserve_days();
            policy.autopilot = false;
        }
        app.update();
        for site in [selected, sibling] {
            let management = app.world().get::<BusinessManagementPolicy>(site).unwrap();
            assert_eq!(management.strategy, BusinessStrategy::Aggressive);
            assert_eq!(management.payroll_reserve_days, 2);
        }
        let override_policy = app.world().get::<BusinessManagementPolicy>(manual).unwrap();
        assert_eq!(override_policy.strategy, BusinessStrategy::Conservative);
        assert_eq!(override_policy.payroll_reserve_days, 5);
        assert!(!override_policy.autopilot);
        assert!(app
            .world()
            .get::<CompanyDecisionHistory>(treasury)
            .unwrap()
            .entries()
            .is_empty());

        app.world_mut().clear_trackers();
        app.update();
        assert!(
            !app.world_mut()
                .query_filtered::<Entity, Changed<BusinessManagementPolicy>>()
                .iter(app.world())
                .any(|entity| [selected, sibling, manual].contains(&entity)),
            "an unchanged company choice must not dirty its site policies"
        );
    }

    #[test]
    fn appointed_master_changes_strategy_from_consolidated_books() {
        let mut app = App::new();
        app.add_systems(Update, review_company_strategies);
        app.world_mut().spawn(WorldTime::new_default());
        let master = PersonId(4);
        let company = CompanyId(20);
        app.world_mut()
            .spawn((master, CharacterAttributes::new(10, 10, 20)));
        let company_entity = app
            .world_mut()
            .spawn((
                company,
                CompanyLeadership { master },
                CompanyAccount {
                    cash: 2_000,
                    ..default()
                },
                CompanyManagementPolicy::default(),
                CompanyDecisionHistory::default(),
            ))
            .id();
        let account = BusinessAccount {
            previous_day: shared::economy::BusinessDayLedger {
                day: 0,
                gross_revenue: 1_000,
                wage_expense: 200,
                ..default()
            },
            ..default()
        };
        let site = app.world_mut().spawn(farm_site(company, 21, account)).id();

        app.update();

        assert_eq!(
            app.world()
                .get::<CompanyManagementPolicy>(company_entity)
                .unwrap()
                .strategy,
            BusinessStrategy::Aggressive
        );
        assert_eq!(
            app.world()
                .get::<BusinessManagementPolicy>(site)
                .unwrap()
                .strategy,
            BusinessStrategy::Aggressive
        );
        let history = app
            .world()
            .get::<CompanyDecisionHistory>(company_entity)
            .unwrap();
        assert_eq!(history.entries().len(), 1);
        assert_eq!(history.entries()[0].master, master);
    }

    #[test]
    fn empty_company_shells_are_retired_but_active_companies_survive() {
        let mut app = App::new();
        app.add_systems(Update, cleanup_empty_companies);
        let former_owner = PersonId(30);
        let empty = app
            .world_mut()
            .spawn((
                CompanyId(30),
                CompanyOwnership::sole(former_owner),
                CompanyBranchPolicies::default(),
                CompanyAccount::default(),
            ))
            .id();
        let active = app
            .world_mut()
            .spawn((
                CompanyId(31),
                CompanyOwnership::sole(former_owner),
                CompanyBranchPolicies::default(),
                CompanyAccount::default(),
            ))
            .id();
        app.world_mut().spawn(OperatedBy(CompanyId(31)));

        app.update();

        assert!(app.world().get_entity(empty).is_err());
        assert!(app.world().get_entity(active).is_ok());
    }

    #[test]
    fn every_site_posts_startup_capital_into_one_company_treasury() {
        let mut app = App::new();
        app.add_systems(Update, post_site_capital_to_company);
        let company = CompanyId(35);
        let company_entity = app
            .world_mut()
            .spawn((company, CompanyAccount::default()))
            .id();
        let first = app
            .world_mut()
            .spawn((OperatedBy(company), BusinessAccount::with_capital(300)))
            .id();
        let second = app
            .world_mut()
            .spawn((OperatedBy(company), BusinessAccount::with_capital(700)))
            .id();

        app.update();

        assert_eq!(
            app.world()
                .get::<CompanyAccount>(company_entity)
                .unwrap()
                .cash,
            1_000
        );
        for site in [first, second] {
            assert_eq!(
                app.world()
                    .get::<BusinessAccount>(site)
                    .unwrap()
                    .unposted_company_capital,
                0
            );
        }
    }

    #[test]
    fn company_replication_only_changes_when_consolidated_values_change() {
        #[derive(Resource, Default)]
        struct AccountChanges(usize);
        fn count_changes(
            mut changes: ResMut<AccountChanges>,
            accounts: Query<(), Changed<CompanyAccount>>,
        ) {
            changes.0 = accounts.iter().count();
        }

        let mut app = App::new();
        app.init_resource::<AccountChanges>()
            .add_systems(Update, (refresh_company_accounts, count_changes).chain());
        let clock = app.world_mut().spawn(WorldTime::new_default()).id();
        let company = app
            .world_mut()
            .spawn((
                CompanyId(32),
                CompanyAccount {
                    cash: 500,
                    contributed_capital: 250,
                    ..default()
                },
            ))
            .id();
        let site = app
            .world_mut()
            .spawn((OperatedBy(CompanyId(32)), BusinessAccount::default()))
            .id();
        app.update();
        for _ in 0..4 {
            app.update();
            assert_eq!(
                app.world().resource::<AccountChanges>().0,
                0,
                "idle consolidation must not dirty the replicated account"
            );
        }

        app.world_mut()
            .get_mut::<BusinessAccount>(site)
            .unwrap()
            .record_sale(0, 200, 0, 2);
        app.update();
        assert_eq!(app.world().resource::<AccountChanges>().0, 1);
        assert_eq!(
            app.world()
                .get::<CompanyAccount>(company)
                .unwrap()
                .current_day
                .external_revenue,
            200
        );
        app.update();
        assert_eq!(app.world().resource::<AccountChanges>().0, 0);

        app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 1;
        app.update();
        assert_eq!(app.world().resource::<AccountChanges>().0, 1);
        let account = app.world().get::<CompanyAccount>(company).unwrap();
        assert_eq!(account.current_day.day, 1);
        assert_eq!(account.previous_day.external_revenue, 200);
        assert_eq!(account.cash, 500);
        assert_eq!(account.contributed_capital, 250);
        app.update();
        assert_eq!(app.world().resource::<AccountChanges>().0, 0);
    }

    #[test]
    fn company_snapshot_preserves_treasury_when_its_last_site_leaves() {
        let mut app = App::new();
        app.add_systems(Update, refresh_company_accounts);
        app.world_mut().spawn(WorldTime::new_default());
        let company = app
            .world_mut()
            .spawn((
                CompanyId(32),
                CompanyAccount {
                    cash: 500,
                    wage_arrears: 80,
                    book_value: 300,
                    ..default()
                },
            ))
            .id();

        app.update();

        let account = app.world().get::<CompanyAccount>(company).unwrap();
        assert_eq!(account.cash, 500);
        assert_eq!(account.wage_arrears, 0);
        assert_eq!(account.book_value, 0);
    }

    #[test]
    fn company_snapshot_consolidates_current_and_completed_site_wages() {
        let mut app = App::new();
        app.add_systems(Update, refresh_company_accounts);
        let mut clock = WorldTime::new_default();
        clock.day = 2;
        app.world_mut().spawn(clock);
        let company_id = CompanyId(40);
        let company = app
            .world_mut()
            .spawn((company_id, CompanyAccount::default()))
            .id();

        let mut first = BusinessAccount::default();
        first.record_sale(1, 500, 0, 5);
        first.incur_completed_day_wages(1, 100);
        assert_eq!(first.settle_wage_claim(100), 100);
        first.roll_to_day(2);
        first.record_sale(2, 200, 0, 2);
        app.world_mut().spawn((OperatedBy(company_id), first));

        let mut second = BusinessAccount::default();
        second.record_sale(1, 300, 0, 3);
        second.incur_completed_day_wages(1, 200);
        second.roll_to_day(2);
        app.world_mut().spawn((OperatedBy(company_id), second));

        app.update();

        let account = app.world().get::<CompanyAccount>(company).unwrap();
        assert_eq!(account.current_day.day, 2);
        assert_eq!(account.current_day.external_revenue, 200);
        assert_eq!(account.current_day.wage_expense, 0);
        assert_eq!(account.previous_day.day, 1);
        assert_eq!(account.previous_day.external_revenue, 800);
        assert_eq!(account.previous_day.wage_expense, 300);
        assert_eq!(account.wage_arrears, 200);
    }
}
