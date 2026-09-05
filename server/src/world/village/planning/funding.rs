//! Company expansion reserves and exact contributions for approved permits.

#[cfg(test)]
use super::demand::has_planned_food_extractor;
use crate::world::village::*;

/// Autonomous founders preserve a few days of personal purchasing power.
/// Company formation is an investment decision, not permission to commit the
/// resident's last meal money. Player-directed contributions remain explicit.
pub(super) const NPC_PERSONAL_INVESTMENT_RESERVE: u64 = 3 * PENNIES_PER_COIN;

#[derive(Debug, Default)]
pub(super) struct CompanyExpansionFunds {
    pub(super) available: u64,
    /// Existing liabilities and payroll runway not presently backed by cash.
    /// A sole owner must repair this before new-site capital is truly free.
    pub(super) reserve_shortfall: u64,
    pub(super) entity: Option<Entity>,
}

impl CompanyExpansionFunds {
    pub(super) fn personal_contribution_required(&self, planned_cash: u64) -> u64 {
        planned_cash
            .saturating_sub(self.available)
            .saturating_add(self.reserve_shortfall)
    }

    pub(super) fn record_personal_contribution(&mut self, contribution: u64, permit_fee: u64) {
        let repaired = contribution.min(self.reserve_shortfall);
        self.reserve_shortfall -= repaired;
        self.available = self
            .available
            .saturating_add(contribution.saturating_sub(repaired))
            .saturating_sub(permit_fee);
    }
}

pub(super) fn debit_company_expansion(
    funds: &mut CompanyExpansionFunds,
    accounts: &mut Query<(
        Entity,
        &shared::components::CompanyId,
        &mut shared::economy::CompanyAccount,
    )>,
    amount: u64,
) -> bool {
    if funds.available < amount {
        return false;
    }
    let Some(entity) = funds.entity else {
        return false;
    };
    let Ok((_, _, mut account)) = accounts.get_mut(entity) else {
        return false;
    };
    if !account.debit(amount) {
        return false;
    }
    funds.available -= amount;
    true
}

#[cfg(test)]
mod company_funding_tests {
    use super::*;
    use bevy::ecs::system::SystemState;

    #[test]
    fn company_expansion_debit_conserves_cash_and_never_touches_reserves() {
        let mut world = World::new();
        let company = shared::components::CompanyId(8);
        let company_entity = world
            .spawn((
                company,
                shared::economy::CompanyAccount {
                    cash: 1_500,
                    ..default()
                },
            ))
            .id();
        let mut funds = CompanyExpansionFunds {
            available: 800,
            reserve_shortfall: 0,
            entity: Some(company_entity),
        };
        let mut state = SystemState::<
            Query<(
                Entity,
                &shared::components::CompanyId,
                &mut shared::economy::CompanyAccount,
            )>,
        >::new(&mut world);
        {
            let mut accounts = state
                .get_mut(&mut world)
                .expect("valid company account query");
            assert!(debit_company_expansion(&mut funds, &mut accounts, 650));
        }
        state.apply(&mut world);

        assert_eq!(funds.available, 150);
        assert_eq!(
            world
                .get::<shared::economy::CompanyAccount>(company_entity)
                .unwrap()
                .cash,
            850
        );
    }

    #[test]
    fn owner_repairs_missing_payroll_before_funding_expansion() {
        let mut funds = CompanyExpansionFunds {
            available: 0,
            reserve_shortfall: 300,
            entity: None,
        };
        let planned_cash = 220 + 400;
        let contribution = funds.personal_contribution_required(planned_cash);
        assert_eq!(contribution, 920);

        funds.record_personal_contribution(contribution, 220);
        assert_eq!(funds.reserve_shortfall, 0);
        assert_eq!(funds.available, 400);
    }

    #[test]
    fn every_extractor_kind_closes_the_founders_only_food_fallback() {
        for kind in [
            SettlementBuildingKind::Farmstead,
            SettlementBuildingKind::FishermansHut,
            SettlementBuildingKind::LivestockFarm,
        ] {
            let have = HashMap::from([(kind, 1)]);
            assert!(
                has_planned_food_extractor(&have),
                "{} must count as established food capacity",
                kind.label()
            );
        }
        assert!(!has_planned_food_extractor(&HashMap::from([(
            SettlementBuildingKind::House,
            4,
        )])));
    }
}
