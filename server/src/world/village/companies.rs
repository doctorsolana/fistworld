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
    CompanyDecisionReason, CompanyDecisionRecord, CompanyManagementPolicy,
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
    for (entity, id, ownership, management, leadership, decision_history, share_market, branches) in
        companies.iter()
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
                BusinessStrategy::Cautious,
                CompanyDecisionReason::FinancialStress,
            )
        } else if strongly_profitable && reading.cash > reading.costs.saturating_mul(3) {
            if master.charm() > master.intelligence().saturating_add(3) {
                (
                    BusinessStrategy::HighMargin,
                    CompanyDecisionReason::StrongMarketPosition,
                )
            } else {
                (
                    BusinessStrategy::Growth,
                    CompanyDecisionReason::ProfitableExpansion,
                )
            }
        } else if profitable && master.charm() >= 18 {
            (
                BusinessStrategy::Opportunistic,
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
    if changed.is_empty() {
        return;
    }
    for (operated_by, mut management) in site_policies.iter_mut() {
        if !management.autopilot {
            continue;
        }
        if let Some(strategy) = changed.get(&operated_by.0).copied() {
            management.strategy = strategy;
            management.payroll_reserve_days = strategy.payroll_reserve_days();
        }
    }
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

#[derive(Resource, Default)]
pub struct CompanyDividendQueue {
    requested: Vec<CompanyId>,
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

impl CompanyDividendQueue {
    pub fn request(&mut self, company: CompanyId) {
        if !self.requested.contains(&company) {
            self.requested.push(company);
        }
    }
}

/// Pay at most one company dividend per day. The amount is validated against
/// consolidated retained profit and the single treasury after every site's employee,
/// tax, input and operating reserves. Payment then follows the cap table: one
/// whole share out of 1,000 receives exactly one-thousandth of the distribution
/// before deterministic penny rounding.
#[allow(clippy::type_complexity)]
pub fn review_company_finance(
    world_time: Query<&WorldTime>,
    mut requests: ResMut<CompanyDividendQueue>,
    mut processed_day: Local<Option<u32>>,
    mut companies: Query<(
        &CompanyId,
        &CompanyOwnership,
        &mut CompanyAccount,
        &mut CompanyManagementPolicy,
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
        )>,
        Query<&mut BusinessAccount>,
    )>,
    mut people: ParamSet<(Query<(Entity, &PersonId), With<Wallet>>, Query<&mut Wallet>)>,
) {
    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    if *processed_day == Some(day) && requests.requested.is_empty() {
        return;
    }
    *processed_day = Some(day);
    let requested: HashSet<CompanyId> = requests.requested.drain(..).collect();
    let people_by_id: HashMap<PersonId, Entity> = people
        .p0()
        .iter()
        .map(|(entity, id)| (*id, entity))
        .collect();
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
    ) in sites.p0().iter()
    {
        let procurement = procurement.copied().unwrap_or_default();
        let enabled_positions = staffing
            .copied()
            .unwrap_or_else(|| BusinessStaffingPolicy::new(building.kind.positions()))
            .target_for(building.kind);
        let mut stock = [0; Good::COUNT];
        for good in Good::ALL {
            stock[good.index()] = inventory.amount(good);
        }
        let working = business_working_capital(
            enabled_positions,
            wage,
            management,
            &procurement,
            Some(&stock),
            None,
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

    for (company_id, ownership, mut company_account, mut policy) in companies.iter_mut() {
        let manual = requested.contains(company_id);
        if !manual && (!policy.automatic_dividends || policy.last_dividend_day == day) {
            continue;
        }
        if !manual {
            policy.last_dividend_day = day;
        }
        let Some(company_sites) = sites_by_company.get_mut(company_id) else {
            continue;
        };
        if ownership
            .shares()
            .iter()
            .any(|holding| !people_by_id.contains_key(&holding.shareholder))
        {
            continue;
        }
        company_sites.sort_unstable_by_key(|site| site.building);
        let protected = company_sites
            .iter()
            .map(|site| site.protected)
            .fold(0u64, u64::saturating_add)
            // A company treasury needs one ordinary operating buffer, not a
            // duplicate two-coin reserve for every cost centre it operates.
            .saturating_add(2 * PENNIES_PER_COIN);
        let revenue = company_sites
            .iter()
            .map(|site| site.gross_revenue)
            .fold(0u64, u64::saturating_add);
        let expenses = company_sites
            .iter()
            .map(|site| site.operating_expenses)
            .fold(0u64, u64::saturating_add);
        let prior_withdrawals = company_sites
            .iter()
            .map(|site| site.prior_withdrawals)
            .fold(0u64, u64::saturating_add);
        let retained_profit = revenue
            .saturating_sub(expenses)
            .saturating_sub(prior_withdrawals);
        let wanted = retained_profit
            .min(company_account.cash.saturating_sub(protected))
            .min(if manual {
                u64::MAX
            } else {
                policy.max_daily_dividend
            });
        if wanted == 0 {
            continue;
        }

        if !company_account.debit(wanted) {
            continue;
        }
        let distributed = wanted;
        company_account.owner_withdrawals = company_account
            .owner_withdrawals
            .saturating_add(distributed);
        company_account.roll_to_day(day);
        company_account.current_day.owner_withdrawals = company_account
            .current_day
            .owner_withdrawals
            .saturating_add(distributed);
        // Keep one site-level memorandum for the detailed ledger without
        // suggesting that this building owned or paid the cash.
        if let Some(site) = company_sites.first() {
            if let Ok(mut account) = sites.p1().get_mut(site.entity) {
                account.record_company_dividend(day, distributed);
            }
        }
        let mut credited = 0u64;
        for holding in ownership.shares() {
            let dividend = distributed.saturating_mul(u64::from(holding.shares))
                / u64::from(shared::components::COMPANY_TOTAL_SHARES);
            if let Some(entity) = people_by_id.get(&holding.shareholder).copied() {
                if let Ok(mut wallet) = people.p1().get_mut(entity) {
                    wallet.credit(dividend);
                    credited = credited.saturating_add(dividend);
                }
            }
        }
        // Whole pennies cannot always divide cleanly across 1,000 shares. The
        // first (stable PersonId-sorted) shareholder receives the remainder.
        let rounding = distributed.saturating_sub(credited);
        if rounding > 0 {
            if let Some(first) = ownership.shares().first() {
                if let Some(entity) = people_by_id.get(&first.shareholder).copied() {
                    if let Ok(mut wallet) = people.p1().get_mut(entity) {
                        wallet.credit(rounding);
                    }
                }
            }
        }
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
            .request(company);

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
            BusinessStrategy::HighMargin
        );
        assert_eq!(
            app.world()
                .get::<BusinessManagementPolicy>(site)
                .unwrap()
                .strategy,
            BusinessStrategy::HighMargin
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
