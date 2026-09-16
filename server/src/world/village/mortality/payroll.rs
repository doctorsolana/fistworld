//! Settle deceased workers' named claims before distributing their estates.

use super::*;
use crate::world::village::commerce::payroll_claims::PrivatePayrollClaims;
use shared::components::{BuildingId, CompanyId, CompanyOwnership, CompanyShareMarket, OperatedBy};
use shared::economy::{BusinessWageClaim, CompanyAccount};

#[derive(bevy::ecs::system::SystemParam)]
pub struct DeathCompanyAccounts<'w, 's> {
    pub(super) ownership: Query<
        'w,
        's,
        (
            Entity,
            &'static CompanyId,
            &'static CompanyOwnership,
            Option<&'static CompanyShareMarket>,
        ),
    >,
    treasuries: Query<'w, 's, (Entity, &'static CompanyId, &'static mut CompanyAccount)>,
    sites: Query<
        'w,
        's,
        (
            Entity,
            &'static mut BusinessAccount,
            Option<&'static BuildingId>,
            Option<&'static OperatedBy>,
            Option<&'static mut PrivatePayrollClaims>,
            Option<&'static mut BusinessLiquidation>,
        ),
    >,
}

impl DeathCompanyAccounts<'_, '_> {
    pub(super) fn has_liquidation(&self, entity: Entity) -> bool {
        self.sites
            .get(entity)
            .is_ok_and(|(.., liquidation)| liquidation.is_some())
    }

    /// Index once for the whole mortality pass. There is no claim scan on
    /// ordinary ticks, and multiple deaths do not repeat the workplace scan.
    pub(super) fn settle(&mut self, deceased: &HashSet<PersonId>) -> HashMap<PersonId, u64> {
        if deceased.is_empty() {
            return HashMap::new();
        }
        let companies: HashMap<_, _> = self
            .treasuries
            .iter()
            .map(|(entity, id, _)| (*id, entity))
            .collect();
        let mut claims = Vec::new();
        for (entity, _, id, _, operating, liquidation) in self.sites.iter_mut() {
            let stable_id = id.map_or(entity.to_bits(), |id| id.0);
            let mut extract = |records: &mut Vec<BusinessWageClaim>| {
                records.retain(|claim| {
                    if deceased.contains(&claim.worker) {
                        claims.push((stable_id, claim.worker, entity, claim.pennies));
                        false
                    } else {
                        true
                    }
                });
            };
            if let Some(mut operating) = operating {
                extract(&mut operating.claims);
            }
            if let Some(mut liquidation) = liquidation {
                extract(&mut liquidation.wage_claims);
            }
        }
        // A scarce company treasury follows stable site/person order, never
        // archetype iteration or display-name matching.
        claims.sort_unstable();
        let mut paid_estates = HashMap::<PersonId, u64>::new();
        for (_, worker, entity, pennies) in claims {
            let Ok((_, mut account, _, operation, _, _)) = self.sites.get_mut(entity) else {
                continue;
            };
            let recognized = pennies.min(account.wage_arrears);
            let mut paid = 0;
            if let Some(company_entity) =
                operation.and_then(|operation| companies.get(&operation.0))
            {
                if let Ok((_, _, mut company)) = self.treasuries.get_mut(*company_entity) {
                    paid = recognized.min(company.cash);
                    if paid > 0 {
                        let debited = company.debit(paid);
                        debug_assert!(debited);
                    }
                    // Consolidation will rebuild this aggregate from sites;
                    // keep the immediate spendable reserve honest as well.
                    company.wage_arrears = company.wage_arrears.saturating_sub(recognized);
                }
            }
            let settled = account.settle_wage_claim(paid);
            debug_assert_eq!(settled, paid);
            let defaulted = account.write_off_wage_claim(recognized - paid);
            if paid > 0 {
                let estate = paid_estates.entry(worker).or_default();
                *estate = estate.saturating_add(paid);
            }
            if defaulted > 0 {
                warn!(
                    "Person#{} died with {} coin of unpaid private wages written off",
                    worker.0,
                    shared::economy::format_money(defaulted)
                );
            }
        }
        paid_estates
    }
}

#[cfg(test)]
#[path = "payroll_tests.rs"]
mod tests;
