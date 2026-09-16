//! Daily labour-market offers and company-wide affordability.

use super::*;
use shared::economy::{
    BUSINESS_WAGE_REVIEW_STEP, FULLY_STAFFED_DAYS_BEFORE_REVIEW, PAYROLL_STRESS_DAYS_BEFORE_CUT,
    VACANCY_DAYS_BEFORE_RAISE,
};

#[derive(Clone, Copy)]
pub(crate) struct WageReview {
    pub elapsed_days: u32,
    pub filled: usize,
    pub positions: usize,
    /// Extra cash after all company liabilities, inputs and two payroll days.
    pub headroom: u64,
    pub arrears: u64,
    pub competing_offer: u64,
    pub living_cost: u64,
    pub sustainable_wage: u64,
    pub scarce_labour: bool,
}

pub(crate) fn review_automatic_wage_offer(policy: &mut BusinessWagePolicy, review: WageReview) {
    if !policy.automatic || review.elapsed_days == 0 || review.positions == 0 {
        return;
    }
    let elapsed = review.elapsed_days.min(u32::from(u16::MAX)) as u16;
    let vacant = review.filled < review.positions;
    policy.vacancy_days = if vacant {
        policy.vacancy_days.saturating_add(elapsed)
    } else {
        0
    };
    policy.fully_staffed_days = if vacant {
        0
    } else {
        policy.fully_staffed_days.saturating_add(elapsed)
    };
    policy.payroll_stress_days = if review.arrears > 0 {
        policy.payroll_stress_days.saturating_add(elapsed)
    } else {
        0
    };
    // Debt takes precedence even with a vacancy; never advertise a raise while
    // withholding the wage already earned by another employee.
    if policy.payroll_stress_days >= PAYROLL_STRESS_DAYS_BEFORE_CUT {
        policy.daily_wage = policy
            .daily_wage
            .saturating_sub(BUSINESS_WAGE_REVIEW_STEP)
            .max(MINIMUM_BUSINESS_DAILY_WAGE);
        policy.payroll_stress_days = 0;
        return;
    }
    if review.arrears > 0 {
        return;
    }
    let step = BUSINESS_WAGE_REVIEW_STEP.max(policy.daily_wage.div_ceil(10));
    let retention = review.competing_offer > policy.daily_wage && review.scarce_labour;
    if (vacant && policy.vacancy_days >= VACANCY_DAYS_BEFORE_RAISE) || retention {
        let market_target = if vacant {
            review.competing_offer.saturating_add(
                review
                    .competing_offer
                    .div_ceil(10)
                    .max(BUSINESS_WAGE_REVIEW_STEP),
            )
        } else {
            review.competing_offer
        };
        let target = market_target
            .max(review.living_cost)
            .max(policy.daily_wage.saturating_add(step));
        let affordable = policy
            .daily_wage
            .saturating_add(review.headroom / (review.positions.max(review.filled) as u64 * 2));
        let next = target
            .min(policy.daily_wage.saturating_add(step))
            .min(review.sustainable_wage)
            .min(affordable)
            .min(MAXIMUM_BUSINESS_DAILY_WAGE);
        if next > policy.daily_wage {
            policy.daily_wage = next;
            policy.vacancy_days = 0;
        }
    } else if !vacant
        && !review.scarce_labour
        && policy.fully_staffed_days >= FULLY_STAFFED_DAYS_BEFORE_REVIEW
        && review.sustainable_wage < policy.daily_wage
        && review.headroom == 0
    {
        policy.daily_wage = policy
            .daily_wage
            .saturating_sub(BUSINESS_WAGE_REVIEW_STEP)
            .max(MINIMUM_BUSINESS_DAILY_WAGE);
        policy.fully_staffed_days = 0;
    }
}

#[derive(Default)]
struct WageMarket {
    offers: Vec<(shared::components::CompanyId, u64)>,
    vacancies: usize,
    seekers: usize,
    living_cost: u64,
    input_quotes: [u64; Good::COUNT],
}

/// Payroll completes first. One stable daily pass then reserves every site's
/// existing commitments before dividing any remaining cash among wage raises.
#[allow(clippy::type_complexity)]
pub(crate) fn review_business_wages(
    clock: Query<&WorldTime>,
    halls: Query<(&shared::components::SettlementId, Option<&MootMarket>)>,
    people: Query<(
        Option<&shared::components::EmployedAt>,
        Option<&shared::components::ResidentOf>,
        &WorkStatus,
    )>,
    companies: Query<(
        &shared::components::CompanyId,
        &shared::economy::CompanyAccount,
    )>,
    mut sites: Query<(
        Entity,
        &shared::components::BuildingId,
        &shared::components::BuildingOf,
        &shared::components::OperatedBy,
        &SettlementBuilding,
        &BusinessAccount,
        &mut BusinessWagePolicy,
        Option<&BusinessStaffingPolicy>,
        Option<&BusinessCondition>,
        Option<&BusinessStaffingForecast>,
    )>,
    mut last_day: Local<Option<u32>>,
) {
    let Some(day) = clock.iter().next().map(|clock| clock.day) else {
        return;
    };
    if *last_day == Some(day) {
        return;
    }
    let elapsed = last_day.map_or(1, |prior| day.saturating_sub(prior));
    *last_day = Some(day);
    let mut markets: HashMap<_, WageMarket> = halls
        .iter()
        .map(|(id, market)| {
            (
                *id,
                WageMarket {
                    living_cost: super::commerce::owner_daily_living_cost(market),
                    input_quotes: std::array::from_fn(|i| {
                        market.map_or(Good::ALL[i].base_price(), |m| {
                            m.suggested_price(Good::ALL[i])
                        })
                    }),
                    ..default()
                },
            )
        })
        .collect();
    let mut filled = HashMap::<shared::components::BuildingId, usize>::new();
    for (job, resident, status) in people.iter() {
        if let Some(job) = job {
            *filled.entry(job.0).or_default() += 1;
        } else if *status == WorkStatus::LookingForWork {
            if let Some(resident) = resident {
                markets.entry(resident.0).or_default().seekers += 1;
            }
        }
    }
    let mut payroll = HashMap::<shared::components::CompanyId, u64>::new();
    let mut reserved = HashMap::<shared::components::CompanyId, u64>::new();
    let mut debts = HashMap::<shared::components::CompanyId, u64>::new();
    let mut ordered = Vec::new();
    for (entity, id, settlement, company, building, account, wage, staffing, condition, _) in
        sites.iter()
    {
        let active = condition.is_none_or(|c| c.state.accepts_new_workers());
        let positions = if active {
            staffing.map_or(building.kind.positions(), |s| s.target_for(building.kind))
        } else {
            0
        };
        let roster = filled.get(id).copied().unwrap_or(0);
        let market = markets.entry(settlement.0).or_default();
        let vacant = usize::from(positions).saturating_sub(roster);
        market.vacancies += vacant;
        if vacant > 0 && account.wage_arrears == 0 {
            market.offers.push((company.0, wage.daily_wage));
        }
        let inputs = processing_recipe(building.kind).map_or(0, |recipe| {
            let quote = market.input_quotes[recipe.input.index()].max(1);
            // Reserve a real staffed day's inputs. A sales forecast cannot
            // shorten an employee's shift or turn a manual site's old
            // "uncapped" sentinel into a thousand fictional recipe units.
            production::rated_input_stock_targets(
                building.kind, recipe.input, usize::from(positions).max(roster), 1,
            ).map_or(0, |targets| u64::from(targets.daily_units).saturating_mul(quote))
        });
        let company_payroll = payroll.entry(company.0).or_default();
        *company_payroll = company_payroll.saturating_add(
            wage.daily_wage
                .saturating_mul(usize::from(positions).max(roster) as u64),
        );
        let company_reserve = reserved.entry(company.0).or_default();
        *company_reserve = company_reserve.saturating_add(
            wage.daily_wage
                .saturating_mul(u64::from(positions).max(roster as u64))
                .saturating_mul(2)
                .saturating_add(inputs),
        );
        let company_debt = debts.entry(company.0).or_default();
        *company_debt = company_debt
            .saturating_add(account.wage_arrears)
            .saturating_add(account.tax_arrears);
        ordered.push((*id, entity));
    }
    let mut headroom: HashMap<_, _> = companies
        .iter()
        .map(|(id, account)| {
            (
                *id,
                account
                    .cash
                    .saturating_sub(reserved.get(id).copied().unwrap_or(0))
                    .saturating_sub(debts.get(id).copied().unwrap_or(0)),
            )
        })
        .collect();
    let company_cash: HashMap<_, _> = companies
        .iter()
        .map(|(id, account)| (*id, account.cash))
        .collect();
    for market in markets.values_mut() {
        market.offers.retain(|(company, _)| {
            debts.get(company).copied().unwrap_or(0) == 0
                && company_cash.get(company).copied().unwrap_or(0)
                    >= payroll.get(company).copied().unwrap_or(u64::MAX)
        });
        market
            .offers
            .sort_unstable_by_key(|(company, wage)| (std::cmp::Reverse(*wage), *company));
        // Only the best offer from each of the two leading companies is needed
        // to exclude an employer's own vacancies from retention pressure.
        if let Some(first) = market.offers.first().copied() {
            let second = market
                .offers
                .iter()
                .find(|offer| offer.0 != first.0)
                .copied();
            market.offers.clear();
            market.offers.push(first);
            if let Some(second) = second {
                market.offers.push(second);
            }
        }
    }
    ordered.sort_unstable_by_key(|(id, _)| *id);
    for (_, entity) in ordered {
        let Ok((
            _,
            id,
            settlement,
            company,
            building,
            account,
            mut wage,
            staffing,
            condition,
            plan,
        )) = sites.get_mut(entity)
        else {
            continue;
        };
        if condition.is_some_and(|c| !c.state.accepts_new_workers()) {
            continue;
        }
        let Some(market) = markets.get(&settlement.0) else {
            continue;
        };
        let positions = usize::from(
            staffing.map_or(building.kind.positions(), |s| s.target_for(building.kind)),
        );
        let roster = filled.get(id).copied().unwrap_or(0);
        let realised = account.previous_day.profit().max(0) as u64;
        let expected = plan
            .filter(|p| p.day <= day && day - p.day <= 1)
            .map_or(0, |p| p.marginal_daily_profit.max(0) as u64);
        // Keep half the contribution surplus for inputs, reinvestment and errors.
        let mut sustainable = wage
            .daily_wage
            .saturating_add(realised.max(expected) / (positions.max(1) as u64 * 2));
        if realised == 0
            && expected == 0
            && account.previous_day.day < day
            && account.previous_day.wage_expense > 0
        {
            let contribution = account
                .previous_day
                .gross_revenue
                .saturating_add(account.previous_day.internal_revenue)
                .saturating_sub(account.previous_day.input_expense)
                .saturating_sub(account.previous_day.internal_input_expense)
                .saturating_sub(account.previous_day.market_fees)
                .saturating_sub(account.previous_day.delivery_fees);
            sustainable = contribution / roster.max(1) as u64;
        }
        let spare = headroom.entry(company.0).or_default();
        let before = wage.daily_wage;
        review_automatic_wage_offer(
            &mut wage,
            WageReview {
                elapsed_days: elapsed,
                filled: roster,
                positions,
                headroom: *spare,
                arrears: debts.get(&company.0).copied().unwrap_or(0),
                competing_offer: market
                    .offers
                    .iter()
                    .find(|offer| offer.0 != company.0)
                    .map_or(0, |offer| offer.1),
                living_cost: market.living_cost,
                sustainable_wage: sustainable,
                scarce_labour: market.vacancies > market.seekers,
            },
        );
        *spare = spare.saturating_sub(
            wage.daily_wage
                .saturating_sub(before)
                .saturating_mul(positions.max(roster) as u64)
                .saturating_mul(2),
        );
    }
}
