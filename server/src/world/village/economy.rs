//! Business policy decisions that do not require physical-world queries.

use shared::economy::{
    BusinessWagePolicy, BUSINESS_WAGE_REVIEW_STEP, FULLY_STAFFED_DAYS_BEFORE_REVIEW,
    MAXIMUM_BUSINESS_DAILY_WAGE, MINIMUM_BUSINESS_DAILY_WAGE, PAYROLL_STRESS_DAYS_BEFORE_CUT,
    VACANCY_DAYS_BEFORE_RAISE,
};

pub(crate) fn review_automatic_wage_offer(
    policy: &mut BusinessWagePolicy,
    elapsed_days: u32,
    filled_positions: usize,
    total_positions: usize,
    cash: u64,
    arrears: u64,
) {
    if !policy.automatic || elapsed_days == 0 || total_positions == 0 {
        return;
    }
    let elapsed = elapsed_days.min(u32::from(u16::MAX)) as u16;
    if filled_positions < total_positions {
        policy.vacancy_days = policy.vacancy_days.saturating_add(elapsed);
        policy.fully_staffed_days = 0;
        policy.payroll_stress_days = 0;
        let next = policy
            .daily_wage
            .saturating_add(BUSINESS_WAGE_REVIEW_STEP)
            .min(MAXIMUM_BUSINESS_DAILY_WAGE);
        let full_day_cost = next.saturating_mul(total_positions as u64);
        if policy.vacancy_days >= VACANCY_DAYS_BEFORE_RAISE
            && next > policy.daily_wage
            && cash >= full_day_cost
        {
            policy.daily_wage = next;
            policy.vacancy_days = 0;
        }
        return;
    }

    policy.vacancy_days = 0;
    policy.fully_staffed_days = policy.fully_staffed_days.saturating_add(elapsed);
    if arrears > 0 {
        policy.payroll_stress_days = policy.payroll_stress_days.saturating_add(elapsed);
    } else {
        policy.payroll_stress_days = 0;
    }
    let staffed_payroll = policy.daily_wage.saturating_mul(filled_positions as u64);
    let easy_to_fill_but_cash_tight = policy.fully_staffed_days >= FULLY_STAFFED_DAYS_BEFORE_REVIEW
        && cash < staffed_payroll.saturating_mul(2);
    if policy.payroll_stress_days >= PAYROLL_STRESS_DAYS_BEFORE_CUT || easy_to_fill_but_cash_tight {
        policy.daily_wage = policy
            .daily_wage
            .saturating_sub(BUSINESS_WAGE_REVIEW_STEP)
            .max(MINIMUM_BUSINESS_DAILY_WAGE);
        policy.payroll_stress_days = 0;
        policy.fully_staffed_days = 0;
    }
}
