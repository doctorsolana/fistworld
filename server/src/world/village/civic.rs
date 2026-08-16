//! Municipal finance, payroll and bounded policy review.
//!
//! The treasury is cash, `CivicAccount` explains each movement, and
//! `SettlementPolicies` contains the enacted rules. Staffing systems ask this
//! module whether a new recurring wage is affordable; they do not invent
//! their own budget tests.

use super::*;

use shared::components::{
    BuildingId, CivicEmployment, CivicPayrollEntry, CivicPolicyAdjustment, CivicPolicyReason,
    CivicRole, CivicStaffingPosture, CivicStrategy, CompanyId, OperatedBy, PersonId,
    PoorReliefMode, SettlementId,
};
use shared::economy::{
    CivicAccount, BASIS_POINTS, CIVIC_POLICY_REVIEW_DAYS, DEFAULT_BUSINESS_PERMIT_SUBSIDY_BPS,
    DEFAULT_MARKET_FEE_BPS, FOUNDING_DAILY_WAGE, MARKET_FEE_REVIEW_STEP_BPS,
    MAXIMUM_BUSINESS_PERMIT_SUBSIDY_BPS, MAXIMUM_BUSINESS_PROFIT_TAX_BPS,
    MAXIMUM_CIVIC_PAYROLL_RESERVE_DAYS, MAXIMUM_FOOD_RESERVE_TARGET_DAYS, MAXIMUM_MARKET_FEE_BPS,
    MINIMUM_FOOD_RESERVE_TARGET_DAYS, MINIMUM_MARKET_FEE_BPS, PERMIT_SUBSIDY_REVIEW_STEP_BPS,
    PROFIT_TAX_REVIEW_STEP_BPS,
};

const CIVIC_UNPAID_DAYS_BEFORE_RESIGNATION: u64 = 3;

pub(crate) fn filled_civic_positions(administration: &MootAdministration) -> usize {
    let workers = administration.city_workers.len().max(usize::from(
        administration.road_steward.is_some() || administration.market_porter.is_some(),
    ));
    usize::from(administration.reeve.is_some()) + workers + administration.guards.len()
}

pub(crate) fn civic_staffing_targets(
    tier: shared::components::SettlementTier,
    posture: CivicStaffingPosture,
) -> (usize, usize) {
    let (workers, guards) = posture.targets(tier);
    (usize::from(workers), usize::from(guards))
}

pub(crate) fn desired_civic_positions(
    tier: shared::components::SettlementTier,
    posture: CivicStaffingPosture,
) -> usize {
    let (workers, guards) = civic_staffing_targets(tier, posture);
    // The Reeve is the separate administrative position; workers already
    // include the tier's combined Moot Steward slots.
    1 + workers + guards
}

pub(crate) fn civic_daily_payroll(administration: &MootAdministration) -> u64 {
    (filled_civic_positions(administration) as u64).saturating_mul(FOUNDING_DAILY_WAGE)
}

pub(crate) fn can_afford_new_civic_hire(
    settlement: &Settlement,
    administration: &MootAdministration,
    policies: &SettlementPolicies,
) -> bool {
    can_afford_civic_positions(
        settlement,
        administration,
        policies,
        filled_civic_positions(administration) + 1,
    )
}

pub(crate) fn can_afford_civic_positions(
    settlement: &Settlement,
    administration: &MootAdministration,
    policies: &SettlementPolicies,
    projected_positions: usize,
) -> bool {
    let projected_daily = (projected_positions as u64).saturating_mul(FOUNDING_DAILY_WAGE);
    let reserve = projected_daily.saturating_mul(u64::from(policies.civic_payroll_reserve_days));
    settlement.treasury >= administration.wage_arrears.saturating_add(reserve)
}

pub(crate) fn civic_discretionary_budget(
    settlement: &Settlement,
    administration: Option<&MootAdministration>,
    policies: Option<&SettlementPolicies>,
) -> u64 {
    let arrears = administration.map_or(0, |office| office.wage_arrears);
    let payroll = administration.map_or(0, civic_daily_payroll);
    let reserve_days = policies.map_or(
        u64::from(shared::economy::DEFAULT_CIVIC_PAYROLL_RESERVE_DAYS),
        |policy| u64::from(policy.civic_payroll_reserve_days),
    );
    settlement
        .treasury
        .saturating_sub(arrears)
        .saturating_sub(payroll.saturating_mul(reserve_days))
}

pub fn ensure_civic_accounts(
    mut commands: Commands,
    halls: Query<
        (
            Entity,
            &Settlement,
            Option<&PlayerPosition>,
            Option<&CivicAccount>,
            Option<&SettlementPolicies>,
        ),
        With<Settlement>,
    >,
) {
    for (entity, settlement, position, account, policies) in halls.iter() {
        let mut entity = commands.entity(entity);
        if account.is_none() {
            entity.insert(CivicAccount::default());
        }
        if policies.is_none() {
            entity.insert(SettlementPolicies::from_foundation(
                &settlement.name,
                position.map_or(Vec3::ZERO, |position| position.0),
            ));
        }
    }
}

pub fn sync_civic_market_policy(
    mut halls: Query<(&mut SettlementPolicies, &mut MootMarket), With<Settlement>>,
) {
    for (mut policies, mut market) in halls.iter_mut() {
        policies.market_fee_bps = policies
            .market_fee_bps
            .clamp(MINIMUM_MARKET_FEE_BPS, MAXIMUM_MARKET_FEE_BPS);
        policies.business_profit_tax_bps = policies
            .business_profit_tax_bps
            .min(MAXIMUM_BUSINESS_PROFIT_TAX_BPS);
        policies.food_reserve_target_days = policies.food_reserve_target_days.clamp(
            MINIMUM_FOOD_RESERVE_TARGET_DAYS,
            MAXIMUM_FOOD_RESERVE_TARGET_DAYS,
        );
        policies.civic_payroll_reserve_days = policies
            .civic_payroll_reserve_days
            .min(MAXIMUM_CIVIC_PAYROLL_RESERVE_DAYS);
        policies.business_permit_subsidy_bps = policies
            .business_permit_subsidy_bps
            .min(MAXIMUM_BUSINESS_PERMIT_SUBSIDY_BPS);
        market.set_market_fee_bps(policies.market_fee_bps);
    }
}

/// Accrue every public salary and settle the affordable claims fairly. A
/// former employee remains in the ledger until their debt reaches zero.
#[allow(clippy::type_complexity)]
pub fn run_civic_payroll(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    mut halls: Query<(
        &SettlementId,
        &mut Settlement,
        &mut MootAdministration,
        &mut CivicAccount,
    )>,
    mut villagers: Query<(
        Entity,
        &PersonId,
        &CharacterName,
        &CivicEmployment,
        &mut Wallet,
        Option<&mut Occupation>,
        Option<&mut WorkStatus>,
        Has<MarketCollectionRoutine>,
        Has<RoadBuilderRoutine>,
    )>,
) {
    let Some(day) = world_time.iter().next().map(|clock| clock.day) else {
        return;
    };
    let mut workers: HashMap<SettlementId, Vec<(Entity, PersonId, String, CivicRole)>> =
        HashMap::new();
    let mut people: HashMap<PersonId, Entity> = HashMap::new();
    for (entity, person_id, name, job, ..) in villagers.iter() {
        people.insert(*person_id, entity);
        if matches!(
            job.role,
            CivicRole::Reeve | CivicRole::MootSteward | CivicRole::CityWorker | CivicRole::Guard
        ) {
            workers.entry(job.settlement).or_default().push((
                entity,
                *person_id,
                name.0.clone(),
                job.role,
            ));
        }
    }

    for (settlement_id, mut settlement, mut administration, mut account) in halls.iter_mut() {
        for entry in &mut administration.payroll {
            entry.active = false;
        }
        let mut active = workers.remove(settlement_id).unwrap_or_default();
        active.sort_by_key(|(_, person_id, _, role)| (*person_id, *role as u8));
        for (_, person_id, name, role) in active {
            let index = administration
                .payroll
                .iter()
                .position(|entry| entry.person_id == person_id && entry.role == role)
                .unwrap_or_else(|| {
                    administration.payroll.push(CivicPayrollEntry {
                        person_id,
                        name: name.clone(),
                        role,
                        daily_wage: FOUNDING_DAILY_WAGE,
                        arrears: 0,
                        last_accrual_day: day,
                        active: true,
                    });
                    administration.payroll.len() - 1
                });
            let entry = &mut administration.payroll[index];
            entry.name = name;
            entry.active = true;
            entry.daily_wage = FOUNDING_DAILY_WAGE;
            let elapsed = day.saturating_sub(entry.last_accrual_day);
            for offset in 0..elapsed {
                let due_day = entry.last_accrual_day.saturating_add(offset);
                entry.arrears = entry.arrears.saturating_add(entry.daily_wage);
                account.record_wage_expense(due_day.saturating_add(1), entry.daily_wage);
            }
            entry.last_accrual_day = day;
        }

        // Rotating the first creditor prevents one stable PersonId from always
        // receiving the last scarce coin while another role is never paid.
        let mut creditors: Vec<usize> = administration
            .payroll
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| (entry.arrears > 0).then_some(index))
            .collect();
        creditors.sort_by_key(|index| administration.payroll[*index].person_id);
        if !creditors.is_empty() {
            let rotation = day as usize % creditors.len();
            creditors.rotate_left(rotation);
        }
        for index in creditors {
            if settlement.treasury == 0 {
                break;
            }
            let entry = &mut administration.payroll[index];
            let payment = entry.arrears.min(settlement.treasury);
            let Some(worker) = people.get(&entry.person_id).copied() else {
                continue;
            };
            let Ok((_, _, _, _, mut wallet, ..)) = villagers.get_mut(worker) else {
                continue;
            };
            settlement.treasury -= payment;
            entry.arrears -= payment;
            wallet.credit(payment);
        }

        // A public salary is a contract, not a vow of lifelong unpaid
        // service. After three completely unpaid days, an idle worker keeps
        // their durable claim but leaves for the ordinary labour market. A
        // Moot Steward first finishes any promised cart or road movement so
        // no physical cargo or half-built connector is orphaned.
        let resignations: Vec<_> = administration
            .payroll
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                (entry.active
                    && entry.arrears
                        >= entry
                            .daily_wage
                            .saturating_mul(CIVIC_UNPAID_DAYS_BEFORE_RESIGNATION))
                .then_some((index, entry.person_id, entry.role))
            })
            .collect();
        for (index, person_id, role) in resignations {
            let Some(worker) = people.get(&person_id).copied() else {
                continue;
            };
            let Ok((_, _, name, _, _, occupation, status, collecting, road_work)) =
                villagers.get_mut(worker)
            else {
                continue;
            };
            if collecting || road_work {
                continue;
            }
            if let Some(mut occupation) = occupation {
                occupation.0 = None;
            }
            if let Some(mut status) = status {
                *status = WorkStatus::LookingForWork;
            } else {
                commands.entity(worker).insert(WorkStatus::LookingForWork);
            }
            administration.payroll[index].active = false;
            commands
                .entity(worker)
                .remove::<CivicEmployment>()
                .remove::<crate::world::village_roads::RoadSteward>()
                .remove::<MarketPorter>()
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();
            info!(
                "{} left the {} post in '{}' after three unpaid days; the wage claim remains",
                name.0,
                role.label(),
                settlement.name,
            );
        }
        administration
            .payroll
            .retain(|entry| entry.active || entry.arrears > 0);
        administration.wage_arrears = administration
            .payroll
            .iter()
            .map(|entry| entry.arrears)
            .fold(0u64, u64::saturating_add);
        administration.road_steward_daily_salary = FOUNDING_DAILY_WAGE;
    }
}

#[derive(Default)]
struct CompanyTaxGroup {
    sites: Vec<(BuildingId, Entity)>,
    external_revenue: u64,
    real_pre_tax_costs: u64,
}

/// Collect one enacted levy from each company's consolidated positive result
/// in a settlement. Same-company transfer credits and charges are eliminated,
/// so vertically integrated goods are not taxed once at every processing site.
/// Wage claims remain senior and any unpaid levy stays explicit on the site to
/// which the consolidated assessment was allocated.
pub fn collect_business_profit_taxes(
    world_time: Query<&WorldTime>,
    mut processed_day: Local<Option<u32>>,
    mut halls: ParamSet<(
        Query<(Entity, &SettlementId, &SettlementPolicies), With<Settlement>>,
        Query<(&mut Settlement, &mut CivicAccount)>,
    )>,
    mut businesses: ParamSet<(
        Query<(
            Entity,
            &BuildingId,
            &shared::components::BuildingOf,
            &OperatedBy,
            &BusinessAccount,
        )>,
        Query<&mut BusinessAccount>,
    )>,
    company_entities: Query<(Entity, &CompanyId)>,
    mut company_accounts: Query<&mut shared::economy::CompanyAccount>,
) {
    let Some(day) = world_time.iter().next().map(|clock| clock.day) else {
        return;
    };
    if *processed_day == Some(day) {
        return;
    }
    *processed_day = Some(day);
    if day == 0 {
        return;
    }
    let completed_day = day - 1;
    let hall_by_settlement: HashMap<SettlementId, (Entity, u16)> = halls
        .p0()
        .iter()
        .map(|(entity, id, policy)| (id.to_owned(), (entity, policy.business_profit_tax_bps)))
        .collect();
    let mut groups: HashMap<(SettlementId, CompanyId), CompanyTaxGroup> = HashMap::new();
    let mut company_wage_arrears = HashMap::<CompanyId, u64>::new();
    for (entity, building_id, building_of, operated_by, account) in businesses.p0().iter() {
        let arrears = company_wage_arrears.entry(operated_by.0).or_default();
        *arrears = arrears.saturating_add(account.wage_arrears);
        let group = groups.entry((building_of.0, operated_by.0)).or_default();
        group.sites.push((*building_id, entity));
        let Some(ledger) = account.ledger_for_day(completed_day) else {
            continue;
        };
        group.external_revenue = group.external_revenue.saturating_add(ledger.gross_revenue);
        group.real_pre_tax_costs = group.real_pre_tax_costs.saturating_add(
            ledger
                .wage_expense
                .saturating_add(ledger.input_expense)
                .saturating_add(ledger.market_fees)
                .saturating_add(ledger.delivery_fees),
        );
    }

    let companies_by_id: HashMap<CompanyId, Entity> = company_entities
        .iter()
        .map(|(entity, id)| (*id, entity))
        .collect();
    for ((settlement_id, company_id), mut group) in groups {
        let Some((hall, tax_bps)) = hall_by_settlement.get(&settlement_id).copied() else {
            continue;
        };
        group
            .sites
            .sort_unstable_by_key(|(building_id, _)| *building_id);
        let due = group
            .external_revenue
            .saturating_sub(group.real_pre_tax_costs)
            .saturating_mul(u64::from(tax_bps))
            .div_ceil(BASIS_POINTS);
        if let Some((_, assessed_site)) = group.sites.first().copied() {
            if let Ok(mut account) = businesses.p1().get_mut(assessed_site) {
                account.incur_completed_day_profit_tax(completed_day, due);
            }
        }

        // A company is one payer. The site owns the local tax liability for
        // reporting, while payment comes directly from the single treasury
        // and never outranks employee wage claims anywhere in the company.
        let Some((_, claimant)) = group.sites.first().copied() else {
            continue;
        };
        let claim = businesses
            .p1()
            .get_mut(claimant)
            .map_or(0, |account| account.tax_arrears);
        let paid = companies_by_id
            .get(&company_id)
            .and_then(|entity| company_accounts.get_mut(*entity).ok())
            .map_or(0, |mut company| {
                let wage_reserve = company_wage_arrears
                    .get(&company_id)
                    .copied()
                    .unwrap_or_default();
                let payable = claim.min(company.cash.saturating_sub(wage_reserve));
                if company.debit(payable) {
                    payable
                } else {
                    0
                }
            });
        if paid > 0 {
            if let Ok(mut account) = businesses.p1().get_mut(claimant) {
                let settled = account.settle_tax_claim(paid);
                debug_assert_eq!(settled, paid);
            }
        }
        if paid > 0 {
            if let Ok((mut settlement, mut civic)) = halls.p1().get_mut(hall) {
                settlement.treasury = settlement.treasury.saturating_add(paid);
                civic.record_profit_tax_income(day, paid);
            }
        }
    }
}

#[derive(Clone, Copy)]
struct CivicPolicyTarget {
    market_fee_bps: u16,
    profit_tax_bps: u16,
    poor_relief: PoorReliefMode,
    staffing: CivicStaffingPosture,
    permit_subsidy_bps: u16,
}

fn target_policy(strategy: CivicStrategy) -> CivicPolicyTarget {
    match strategy {
        CivicStrategy::Balanced => CivicPolicyTarget {
            market_fee_bps: DEFAULT_MARKET_FEE_BPS,
            profit_tax_bps: 1_000,
            poor_relief: PoorReliefMode::SurplusOnly,
            staffing: CivicStaffingPosture::Balanced,
            permit_subsidy_bps: DEFAULT_BUSINESS_PERMIT_SUBSIDY_BPS,
        },
        CivicStrategy::Frugal => CivicPolicyTarget {
            market_fee_bps: 300,
            profit_tax_bps: 500,
            poor_relief: PoorReliefMode::Off,
            staffing: CivicStaffingPosture::Essential,
            permit_subsidy_bps: 2_000,
        },
        CivicStrategy::Mercantile => CivicPolicyTarget {
            market_fee_bps: 400,
            profit_tax_bps: 750,
            poor_relief: PoorReliefMode::SurplusOnly,
            staffing: CivicStaffingPosture::Balanced,
            permit_subsidy_bps: 3_500,
        },
        CivicStrategy::MutualAid => CivicPolicyTarget {
            market_fee_bps: 600,
            profit_tax_bps: 1_250,
            poor_relief: PoorReliefMode::SurplusOnly,
            staffing: CivicStaffingPosture::Full,
            permit_subsidy_bps: 4_000,
        },
        CivicStrategy::Growth => CivicPolicyTarget {
            market_fee_bps: 400,
            profit_tax_bps: 750,
            poor_relief: PoorReliefMode::SurplusOnly,
            staffing: CivicStaffingPosture::Full,
            permit_subsidy_bps: 6_500,
        },
    }
}

/// Review at most once a week and change at most one lever. This creates
/// legible policy inertia rather than daily tax oscillation.
pub fn review_civic_policies(
    world_time: Query<&WorldTime>,
    mut halls: Query<(
        &Settlement,
        &SettlementEconomy,
        &MootAdministration,
        &mut SettlementPolicies,
        &mut CivicAccount,
    )>,
) {
    let Some(day) = world_time.iter().next().map(|clock| clock.day) else {
        return;
    };
    for (settlement, economy, administration, mut policy, mut account) in halls.iter_mut() {
        if policy.last_review_day == u32::MAX {
            policy.last_review_day = day;
            account.close_review_window();
            continue;
        }
        if !policy.autopilot
            || administration.reeve.is_none()
            || day.saturating_sub(policy.last_review_day) < CIVIC_POLICY_REVIEW_DAYS
        {
            continue;
        }
        policy.last_review_day = day;
        let (income, spending) = account.close_review_window();
        let payroll = civic_daily_payroll(&administration);
        let three_day_payroll = payroll.saturating_mul(3);
        let healthy_reserve = payroll.saturating_mul(14);
        let stressed = administration.wage_arrears > 0
            || (payroll > 0 && settlement.treasury < three_day_payroll)
            || spending > income.saturating_mul(2).max(PENNIES_PER_COIN);
        let mut adjustment = CivicPolicyAdjustment::None;
        let mut reason = CivicPolicyReason::None;
        let target = target_policy(policy.strategy);

        if stressed {
            reason = if administration.wage_arrears > 0 {
                CivicPolicyReason::PayrollArrears
            } else {
                CivicPolicyReason::TreasuryStress
            };
            if policy.business_profit_tax_bps < MAXIMUM_BUSINESS_PROFIT_TAX_BPS {
                policy.business_profit_tax_bps = policy
                    .business_profit_tax_bps
                    .saturating_add(PROFIT_TAX_REVIEW_STEP_BPS)
                    .min(MAXIMUM_BUSINESS_PROFIT_TAX_BPS);
                adjustment = CivicPolicyAdjustment::RaisedProfitTax;
            } else if policy.market_fee_bps < MAXIMUM_MARKET_FEE_BPS {
                policy.market_fee_bps = policy
                    .market_fee_bps
                    .saturating_add(MARKET_FEE_REVIEW_STEP_BPS)
                    .min(MAXIMUM_MARKET_FEE_BPS);
                adjustment = CivicPolicyAdjustment::RaisedMarketFee;
            } else if policy.business_permit_subsidy_bps > 0 {
                policy.business_permit_subsidy_bps = policy
                    .business_permit_subsidy_bps
                    .saturating_sub(PERMIT_SUBSIDY_REVIEW_STEP_BPS);
                adjustment = CivicPolicyAdjustment::LoweredGrowthSubsidy;
            } else if policy.staffing_posture != CivicStaffingPosture::Essential {
                policy.staffing_posture = match policy.staffing_posture {
                    CivicStaffingPosture::Full => CivicStaffingPosture::Balanced,
                    CivicStaffingPosture::Balanced | CivicStaffingPosture::Essential => {
                        CivicStaffingPosture::Essential
                    }
                };
                adjustment = CivicPolicyAdjustment::ReducedStaffing;
            } else if policy.poor_relief != PoorReliefMode::Off
                && policy.strategy != CivicStrategy::MutualAid
            {
                policy.poor_relief = PoorReliefMode::Off;
                adjustment = CivicPolicyAdjustment::DisabledPoorRelief;
            }
        } else {
            let sustainable_food = economy.recent_food_production >= settlement.residents as f32
                && economy.reserve_days
                    >= f32::from(policy.food_reserve_target_days.saturating_add(1));
            if policy.poor_relief != target.poor_relief && target.poor_relief == PoorReliefMode::Off
            {
                policy.poor_relief = PoorReliefMode::Off;
                adjustment = CivicPolicyAdjustment::DisabledPoorRelief;
                reason = CivicPolicyReason::HealthySurplus;
            } else if policy.poor_relief != target.poor_relief
                && economy.unmet_food > 0
                && sustainable_food
                && settlement.treasury
                    >= payroll.saturating_mul(u64::from(policy.civic_payroll_reserve_days))
            {
                policy.poor_relief = target.poor_relief;
                adjustment = CivicPolicyAdjustment::EnabledPoorRelief;
                reason = CivicPolicyReason::SustainableRelief;
            } else if policy.staffing_posture != target.staffing {
                let expanding = target.staffing.level() > policy.staffing_posture.level();
                policy.staffing_posture = match (policy.staffing_posture, target.staffing) {
                    (CivicStaffingPosture::Essential, CivicStaffingPosture::Full) => {
                        CivicStaffingPosture::Balanced
                    }
                    (_, target) => target,
                };
                adjustment = if expanding {
                    CivicPolicyAdjustment::ExpandedStaffing
                } else {
                    CivicPolicyAdjustment::ReducedStaffing
                };
                reason = CivicPolicyReason::HealthySurplus;
            } else if policy.business_permit_subsidy_bps < target.permit_subsidy_bps {
                policy.business_permit_subsidy_bps = policy
                    .business_permit_subsidy_bps
                    .saturating_add(PERMIT_SUBSIDY_REVIEW_STEP_BPS)
                    .min(target.permit_subsidy_bps)
                    .min(MAXIMUM_BUSINESS_PERMIT_SUBSIDY_BPS);
                adjustment = CivicPolicyAdjustment::RaisedGrowthSubsidy;
                reason = CivicPolicyReason::HealthySurplus;
            } else if policy.business_permit_subsidy_bps > target.permit_subsidy_bps {
                policy.business_permit_subsidy_bps = policy
                    .business_permit_subsidy_bps
                    .saturating_sub(PERMIT_SUBSIDY_REVIEW_STEP_BPS)
                    .max(target.permit_subsidy_bps);
                adjustment = CivicPolicyAdjustment::LoweredGrowthSubsidy;
                reason = CivicPolicyReason::HealthySurplus;
            } else if settlement.treasury > healthy_reserve && income > spending {
                reason = CivicPolicyReason::HealthySurplus;
                if policy.business_profit_tax_bps > target.profit_tax_bps {
                    policy.business_profit_tax_bps = policy
                        .business_profit_tax_bps
                        .saturating_sub(PROFIT_TAX_REVIEW_STEP_BPS)
                        .max(target.profit_tax_bps);
                    adjustment = CivicPolicyAdjustment::LoweredProfitTax;
                } else if policy.market_fee_bps > target.market_fee_bps {
                    policy.market_fee_bps = policy
                        .market_fee_bps
                        .saturating_sub(MARKET_FEE_REVIEW_STEP_BPS)
                        .max(target.market_fee_bps);
                    adjustment = CivicPolicyAdjustment::LoweredMarketFee;
                }
            }
        }

        if adjustment != CivicPolicyAdjustment::None {
            policy.last_change_day = day;
            policy.last_adjustment = adjustment;
            policy.last_reason = reason;
            info!(
                "Settlement '{}' policy: {} ({})",
                settlement.name,
                adjustment.label(),
                reason.label()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::economy::CompanyAccount;

    fn civic_worker(
        app: &mut App,
        person: u64,
        settlement: SettlementId,
        role: CivicRole,
    ) -> Entity {
        app.world_mut()
            .spawn((
                PersonId(person),
                CharacterName(format!("Worker {person}")),
                CivicEmployment { settlement, role },
                Wallet::new(0),
            ))
            .id()
    }

    #[test]
    fn every_civic_role_is_paid_and_a_departure_cannot_erase_arrears() {
        let mut app = App::new();
        app.add_systems(Update, run_civic_payroll);
        let clock = app.world_mut().spawn(WorldTime::new_default()).id();
        let settlement_id = SettlementId(1);
        let hall = app
            .world_mut()
            .spawn((
                settlement_id,
                Settlement {
                    name: "Ledgerford".into(),
                    tier: shared::components::SettlementTier::Village,
                    residents: 4,
                    treasury: 250,
                },
                MootAdministration::default(),
                CivicAccount::default(),
            ))
            .id();
        let reeve = civic_worker(&mut app, 1, settlement_id, CivicRole::Reeve);
        let steward = civic_worker(&mut app, 2, settlement_id, CivicRole::MootSteward);
        let city_worker = civic_worker(&mut app, 3, settlement_id, CivicRole::CityWorker);
        let guard = civic_worker(&mut app, 4, settlement_id, CivicRole::Guard);

        app.update();
        app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 1;
        app.update();

        let administration = app.world().get::<MootAdministration>(hall).unwrap();
        assert_eq!(administration.payroll.len(), 4);
        assert_eq!(administration.wage_arrears, 150);
        assert_eq!(app.world().get::<Settlement>(hall).unwrap().treasury, 0);
        let paid = [reeve, steward, city_worker, guard]
            .into_iter()
            .map(|worker| app.world().get::<Wallet>(worker).unwrap().balance())
            .sum::<u64>();
        assert_eq!(paid, 250, "public payroll must conserve every penny");

        app.world_mut()
            .entity_mut(reeve)
            .remove::<CivicEmployment>();
        app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 2;
        app.update();
        let administration = app.world().get::<MootAdministration>(hall).unwrap();
        assert_eq!(administration.wage_arrears, 450);
        assert!(administration
            .payroll
            .iter()
            .any(|entry| entry.person_id == PersonId(1) && !entry.active && entry.arrears > 0));
    }

    #[test]
    fn idle_civic_worker_resigns_after_three_unpaid_days_but_keeps_the_claim() {
        let mut app = App::new();
        app.add_systems(Update, run_civic_payroll);
        let clock = app.world_mut().spawn(WorldTime::new_default()).id();
        let settlement_id = SettlementId(9);
        let hall = app
            .world_mut()
            .spawn((
                settlement_id,
                Settlement {
                    name: "Emptycoffer".into(),
                    tier: shared::components::SettlementTier::Village,
                    residents: 2,
                    treasury: 0,
                },
                MootAdministration::default(),
                CivicAccount::default(),
            ))
            .id();
        let guard = app
            .world_mut()
            .spawn((
                PersonId(90),
                CharacterName("Unpaid Guard".into()),
                CivicEmployment {
                    settlement: settlement_id,
                    role: CivicRole::Guard,
                },
                Wallet::new(0),
                Occupation(Some("Town Guard".into())),
                WorkStatus::Employed,
            ))
            .id();

        app.update();
        app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 3;
        app.update();

        assert!(app.world().get::<CivicEmployment>(guard).is_none());
        assert_eq!(
            app.world().get::<WorkStatus>(guard),
            Some(&WorkStatus::LookingForWork)
        );
        assert_eq!(
            app.world().get::<Occupation>(guard).unwrap().0,
            None,
            "resignation must reopen the ordinary labour market"
        );
        let administration = app.world().get::<MootAdministration>(hall).unwrap();
        let claim = administration
            .payroll
            .iter()
            .find(|entry| entry.person_id == PersonId(90))
            .expect("the debt must survive the employment relationship");
        assert_eq!(claim.arrears, 3 * FOUNDING_DAILY_WAGE);
        assert!(!claim.active);
    }

    #[test]
    fn profit_levy_taxes_positive_operating_profit_after_wages() {
        let mut app = App::new();
        app.add_systems(Update, collect_business_profit_taxes);
        let mut clock = WorldTime::new_default();
        clock.day = 1;
        app.world_mut().spawn(clock);
        let settlement_id = SettlementId(2);
        let hall = app
            .world_mut()
            .spawn((
                settlement_id,
                Settlement {
                    name: "Taxmere".into(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 2,
                    treasury: 0,
                },
                SettlementPolicies::default(),
                CivicAccount::default(),
            ))
            .id();
        let mut profitable = BusinessAccount::with_capital(200);
        profitable.record_sale(0, 1_000, 0, 10);
        profitable.incur_wages(0, 400);
        assert_eq!(profitable.settle_wage_claim(400), 400);
        let profitable_company = app
            .world_mut()
            .spawn((
                CompanyId(30),
                CompanyAccount {
                    cash: 800,
                    ..default()
                },
            ))
            .id();
        let business = app
            .world_mut()
            .spawn((
                BuildingId(20),
                shared::components::BuildingOf(settlement_id),
                OperatedBy(CompanyId(30)),
                profitable,
            ))
            .id();
        let mut loss = BusinessAccount::with_capital(100);
        loss.record_sale(0, 100, 0, 1);
        loss.incur_wages(0, 200);
        app.world_mut().spawn((
            CompanyId(31),
            CompanyAccount {
                cash: 100,
                ..default()
            },
        ));
        let loss_business = app
            .world_mut()
            .spawn((
                BuildingId(21),
                shared::components::BuildingOf(settlement_id),
                OperatedBy(CompanyId(31)),
                loss,
            ))
            .id();

        app.update();

        let account = app.world().get::<BusinessAccount>(business).unwrap();
        assert_eq!(account.current_day.profit_taxes, 60);
        assert_eq!(account.tax_arrears, 0);
        assert_eq!(
            app.world()
                .get::<CompanyAccount>(profitable_company)
                .unwrap()
                .cash,
            740
        );
        assert_eq!(
            app.world()
                .get::<BusinessAccount>(loss_business)
                .unwrap()
                .current_day
                .profit_taxes,
            0
        );
        assert_eq!(app.world().get::<Settlement>(hall).unwrap().treasury, 60);
        assert_eq!(
            app.world()
                .get::<CivicAccount>(hall)
                .unwrap()
                .current_day
                .profit_tax_income,
            60
        );
    }

    #[test]
    fn company_profit_levy_eliminates_internal_site_turnover() {
        let mut app = App::new();
        app.add_systems(Update, collect_business_profit_taxes);
        let mut clock = WorldTime::new_default();
        clock.day = 1;
        app.world_mut().spawn(clock);
        let settlement_id = SettlementId(42);
        let company = CompanyId(77);
        let hall = app
            .world_mut()
            .spawn((
                settlement_id,
                Settlement {
                    name: "Consolidated Ford".into(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 4,
                    treasury: 0,
                },
                SettlementPolicies::default(),
                CivicAccount::default(),
            ))
            .id();
        let mut farm = BusinessAccount::with_capital(200);
        farm.record_internal_output(0, 1_000, 10);
        let farm = app
            .world_mut()
            .spawn((
                BuildingId(501),
                shared::components::BuildingOf(settlement_id),
                OperatedBy(company),
                farm,
            ))
            .id();
        let mut mill = BusinessAccount::with_capital(200);
        mill.record_internal_input(0, 1_000, 10);
        let mill = app
            .world_mut()
            .spawn((
                BuildingId(502),
                shared::components::BuildingOf(settlement_id),
                OperatedBy(company),
                mill,
            ))
            .id();

        app.update();

        assert_eq!(
            app.world().get::<Settlement>(hall).unwrap().treasury,
            0,
            "moving the company's own goods between sites is not taxable revenue"
        );
        assert_eq!(
            app.world()
                .get::<BusinessAccount>(farm)
                .unwrap()
                .tax_arrears,
            0
        );
        assert_eq!(
            app.world()
                .get::<BusinessAccount>(mill)
                .unwrap()
                .tax_arrears,
            0
        );
    }

    #[test]
    fn hiring_budget_protects_the_enacted_payroll_reserve() {
        let policy = SettlementPolicies::default();
        let office = MootAdministration::default();
        let mut settlement = Settlement {
            name: "Budgeton".into(),
            tier: shared::components::SettlementTier::Hamlet,
            residents: 5,
            treasury: 699,
        };
        assert!(!can_afford_new_civic_hire(&settlement, &office, &policy));
        settlement.treasury = 700;
        assert!(can_afford_new_civic_hire(&settlement, &office, &policy));
    }

    #[test]
    fn duplicate_display_names_do_not_merge_civic_positions() {
        let office = MootAdministration {
            reeve: Some("Alda".into()),
            road_steward: Some("Alda".into()),
            market_porter: Some("Alda".into()),
            city_workers: vec!["Alda".into(), "Alda".into()],
            guards: vec!["Alda".into()],
            ..default()
        };
        assert_eq!(filled_civic_positions(&office), 4);
    }

    #[test]
    fn staffing_postures_have_distinct_tier_bounded_targets() {
        use shared::components::SettlementTier;
        assert_eq!(
            civic_staffing_targets(SettlementTier::Village, CivicStaffingPosture::Essential),
            (1, 0)
        );
        assert_eq!(
            civic_staffing_targets(SettlementTier::Village, CivicStaffingPosture::Balanced),
            (2, 1)
        );
        assert_eq!(
            civic_staffing_targets(SettlementTier::Village, CivicStaffingPosture::Full),
            (2, 2)
        );
    }

    #[test]
    fn hostile_manual_policy_values_are_clamped_at_the_authoritative_boundary() {
        let mut app = App::new();
        app.add_systems(Update, sync_civic_market_policy);
        let mut policy = SettlementPolicies::default();
        policy.market_fee_bps = u16::MAX;
        policy.business_profit_tax_bps = u16::MAX;
        policy.food_reserve_target_days = 0;
        policy.civic_payroll_reserve_days = u8::MAX;
        policy.business_permit_subsidy_bps = u16::MAX;
        let hall = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Clampford".into(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 0,
                    treasury: 0,
                },
                policy,
                MootMarket::founding(),
            ))
            .id();

        app.update();
        let policy = app.world().get::<SettlementPolicies>(hall).unwrap();
        assert_eq!(policy.market_fee_bps, MAXIMUM_MARKET_FEE_BPS);
        assert_eq!(
            policy.business_profit_tax_bps,
            MAXIMUM_BUSINESS_PROFIT_TAX_BPS
        );
        assert_eq!(
            policy.food_reserve_target_days,
            MINIMUM_FOOD_RESERVE_TARGET_DAYS
        );
        assert_eq!(
            policy.civic_payroll_reserve_days,
            MAXIMUM_CIVIC_PAYROLL_RESERVE_DAYS
        );
        assert_eq!(
            policy.business_permit_subsidy_bps,
            MAXIMUM_BUSINESS_PERMIT_SUBSIDY_BPS
        );
        assert_eq!(
            app.world()
                .get::<MootMarket>(hall)
                .unwrap()
                .market_fee_bps(),
            MAXIMUM_MARKET_FEE_BPS
        );
    }

    #[test]
    fn automatic_policy_review_changes_only_one_revenue_lever_per_week() {
        let mut app = App::new();
        app.add_systems(Update, review_civic_policies);
        let mut clock = WorldTime::new_default();
        clock.day = 7;
        app.world_mut().spawn(clock);
        let mut policy = SettlementPolicies::default();
        policy.last_review_day = 0;
        let hall = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Reviewick".into(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 3,
                    treasury: 50,
                },
                SettlementEconomy::default(),
                MootAdministration {
                    reeve: Some("Rhea".into()),
                    wage_arrears: 100,
                    ..default()
                },
                policy,
                CivicAccount::default(),
            ))
            .id();

        app.update();
        let policy = app.world().get::<SettlementPolicies>(hall).unwrap();
        assert_eq!(policy.business_profit_tax_bps, 1_250);
        assert_eq!(policy.market_fee_bps, DEFAULT_MARKET_FEE_BPS);
        assert_eq!(
            policy.last_adjustment,
            CivicPolicyAdjustment::RaisedProfitTax
        );
        assert_eq!(policy.last_reason, CivicPolicyReason::PayrollArrears);
    }

    #[test]
    fn manual_policy_control_freezes_the_enacted_values() {
        let mut app = App::new();
        app.add_systems(Update, review_civic_policies);
        let mut clock = WorldTime::new_default();
        clock.day = 20;
        app.world_mut().spawn(clock);
        let mut policy = SettlementPolicies::default();
        policy.autopilot = false;
        policy.last_review_day = 0;
        let hall = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Manualford".into(),
                    tier: shared::components::SettlementTier::Village,
                    residents: 20,
                    treasury: 0,
                },
                SettlementEconomy::default(),
                MootAdministration {
                    reeve: Some("Rhea".into()),
                    wage_arrears: 10_000,
                    ..default()
                },
                policy,
                CivicAccount::default(),
            ))
            .id();

        app.update();
        assert_eq!(
            *app.world().get::<SettlementPolicies>(hall).unwrap(),
            policy
        );
    }

    #[test]
    fn every_foundation_begins_with_the_agreed_balanced_charter() {
        let position = Vec3::new(1712.0, 3.0, -91.0);
        let a = SettlementPolicies::from_foundation("Oakfell", position);
        let b = SettlementPolicies::from_foundation("Oakfell", position);
        assert_eq!(a, b);
        assert_eq!(a, SettlementPolicies::default());
        assert_eq!(a.strategy, CivicStrategy::Balanced);
        assert_eq!(a.market_fee_bps, 500);
        assert_eq!(a.business_profit_tax_bps, 1_000);
        assert_eq!(a.poor_relief, PoorReliefMode::SurplusOnly);
        assert_eq!(a.food_reserve_target_days, 3);
        assert_eq!(a.civic_payroll_reserve_days, 7);
        assert_eq!(a.staffing_posture, CivicStaffingPosture::Balanced);
        assert_eq!(a.business_permit_subsidy_bps, 4_500);
    }
}
