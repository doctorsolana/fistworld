//! Named private wage creditors. The replicated account remains the single
//! aggregate liability; this server-only ledger records who earned each penny.

use bevy::prelude::*;
use shared::components::PersonId;
use shared::economy::BusinessWageClaim;

#[derive(Component, Debug, Default)]
pub(crate) struct PrivatePayrollClaims {
    pub(crate) claims: Vec<BusinessWageClaim>,
}

impl PrivatePayrollClaims {
    pub(crate) fn outstanding(&self) -> u64 {
        self.claims
            .iter()
            .map(|claim| claim.pennies)
            .fold(0, u64::saturating_add)
    }

    /// Return the accepted addition so the site aggregate cannot grow by more
    /// than the named obligation, even at the monetary representation limit.
    pub(crate) fn accrue(&mut self, worker: PersonId, pennies: u64) -> u64 {
        let pennies = pennies.min(u64::MAX - self.outstanding());
        if pennies == 0 {
            return 0;
        }
        match self
            .claims
            .binary_search_by_key(&worker, |claim| claim.worker)
        {
            Ok(index) => {
                let accepted = pennies.min(u64::MAX - self.claims[index].pennies);
                self.claims[index].pennies += accepted;
                accepted
            }
            Err(index) => {
                self.claims
                    .insert(index, BusinessWageClaim { worker, pennies });
                pennies
            }
        }
    }

    pub(crate) fn settle(&mut self, worker: PersonId, pennies: u64) -> u64 {
        let Some(index) = self.claims.iter().position(|claim| claim.worker == worker) else {
            return 0;
        };
        let paid = pennies.min(self.claims[index].pennies);
        self.claims[index].pennies -= paid;
        if self.claims[index].pennies == 0 {
            self.claims.remove(index);
        }
        paid
    }

    pub(crate) fn take(&mut self) -> Vec<BusinessWageClaim> {
        std::mem::take(&mut self.claims)
    }

    /// Repay in proportion to the amount owed. A stable daily rotation assigns
    /// the indivisible pennies fairly and does not depend on ECS entity order.
    pub(crate) fn payment_plan(&self, budget: u64, day: u32) -> Vec<BusinessWageClaim> {
        let total: u128 = self
            .claims
            .iter()
            .map(|claim| u128::from(claim.pennies))
            .sum();
        if total == 0 || budget == 0 {
            return Vec::new();
        }
        let budget = u128::from(budget).min(total);
        let mut result: Vec<_> = self
            .claims
            .iter()
            .copied()
            .filter(|claim| claim.pennies > 0)
            .collect();
        result.sort_unstable_by_key(|claim| claim.worker);
        let mut paid = 0_u64;
        for claim in &mut result {
            claim.pennies = (budget * u128::from(claim.pennies) / total) as u64;
            paid += claim.pennies;
        }
        let remainder = budget as u64 - paid;
        let rotation = day as usize % result.len();
        for index in 0..remainder as usize {
            let index = (rotation + index) % result.len();
            result[index].pennies += 1;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::village::*;
    use shared::components::{
        BuildingId, BuildingOf, CompanyId, EmployedAt, OperatedBy, SettlementId,
    };
    use shared::economy::CompanyAccount;

    struct PayrollFixture {
        app: App,
        clock: Entity,
        company: Entity,
        site: Entity,
        worker: Entity,
        settlement: Entity,
        building: BuildingId,
    }

    fn unpaid_shift() -> PayrollFixture {
        let mut app = App::new();
        app.add_systems(Update, run_business_payroll_and_owner_leisure);
        let mut time = WorldTime::new_default();
        time.day = 2;
        let clock = app.world_mut().spawn(time).id();
        let settlement_id = SettlementId(1);
        let settlement = app.world_mut().spawn(settlement_id).id();
        let company_id = CompanyId(2);
        let company = app
            .world_mut()
            .spawn((company_id, CompanyAccount::default()))
            .id();
        let building = BuildingId(3);
        let site = app
            .world_mut()
            .spawn((
                building,
                BuildingOf(settlement_id),
                OperatedBy(company_id),
                SettlementBuilding {
                    kind: SettlementBuildingKind::Farmstead,
                    settlement: "Named wages".into(),
                    owner: None,
                    quality: 1.0,
                    workers: Vec::new(),
                },
                BusinessAccount {
                    last_payroll_day: 1,
                    ..default()
                },
                BusinessWagePolicy::default(),
            ))
            .id();
        let worker = app
            .world_mut()
            .spawn((
                PersonId(4),
                CharacterName("Former worker".into()),
                VillagerIntent::Resident { settlement },
                EmployedAt(building),
                Wallet::default(),
                Occupation(Some("Farmer".into())),
                WorkStatus::Employed,
            ))
            .id();
        app.update();
        assert_eq!(
            app.world()
                .get::<BusinessAccount>(site)
                .unwrap()
                .wage_arrears,
            100
        );
        PayrollFixture {
            app,
            clock,
            company,
            site,
            worker,
            settlement,
            building,
        }
    }

    #[test]
    fn payroll_repays_departed_workers_even_without_a_current_roster() {
        for employed_elsewhere in [false, true] {
            let mut fixture = unpaid_shift();
            let mut person = fixture.app.world_mut().entity_mut(fixture.worker);
            person.remove::<(EmployedAt, VillagerIntent, Occupation, WorkStatus)>();
            if employed_elsewhere {
                person.insert(EmployedAt(BuildingId(99)));
            }
            fixture
                .app
                .world_mut()
                .get_mut::<CompanyAccount>(fixture.company)
                .unwrap()
                .cash = 100;
            fixture
                .app
                .world_mut()
                .get_mut::<WorldTime>(fixture.clock)
                .unwrap()
                .day = 3;
            fixture.app.update();
            assert_eq!(
                fixture
                    .app
                    .world()
                    .get::<Wallet>(fixture.worker)
                    .unwrap()
                    .balance(),
                100
            );
            assert_eq!(
                fixture
                    .app
                    .world()
                    .get::<CompanyAccount>(fixture.company)
                    .unwrap()
                    .cash,
                0
            );
            assert_eq!(
                fixture
                    .app
                    .world()
                    .get::<BusinessAccount>(fixture.site)
                    .unwrap()
                    .wage_arrears,
                0
            );
            assert_eq!(
                fixture
                    .app
                    .world()
                    .get::<PrivatePayrollClaims>(fixture.site)
                    .unwrap()
                    .outstanding(),
                0
            );
        }
    }

    #[test]
    fn a_replacement_earns_the_new_wage_without_receiving_the_former_workers_debt() {
        let mut fixture = unpaid_shift();
        fixture
            .app
            .world_mut()
            .entity_mut(fixture.worker)
            .remove::<EmployedAt>();
        let replacement = fixture
            .app
            .world_mut()
            .spawn((
                PersonId(5),
                CharacterName("Replacement".into()),
                VillagerIntent::Resident {
                    settlement: fixture.settlement,
                },
                EmployedAt(fixture.building),
                Wallet::default(),
                Occupation(Some("Farmer".into())),
                WorkStatus::Employed,
            ))
            .id();
        fixture
            .app
            .world_mut()
            .get_mut::<BusinessWagePolicy>(fixture.site)
            .unwrap()
            .daily_wage = 150;
        fixture
            .app
            .world_mut()
            .get_mut::<CompanyAccount>(fixture.company)
            .unwrap()
            .cash = 250;
        fixture
            .app
            .world_mut()
            .get_mut::<WorldTime>(fixture.clock)
            .unwrap()
            .day = 3;
        fixture.app.update();
        assert_eq!(
            fixture
                .app
                .world()
                .get::<Wallet>(fixture.worker)
                .unwrap()
                .balance(),
            100
        );
        assert_eq!(
            fixture
                .app
                .world()
                .get::<Wallet>(replacement)
                .unwrap()
                .balance(),
            150
        );
        assert_eq!(
            fixture
                .app
                .world()
                .get::<CompanyAccount>(fixture.company)
                .unwrap()
                .cash,
            0
        );
        assert_eq!(
            fixture
                .app
                .world()
                .get::<BusinessAccount>(fixture.site)
                .unwrap()
                .wage_arrears,
            0
        );
    }

    #[test]
    fn an_owner_with_committed_cargo_keeps_their_work_until_it_finishes() {
        let mut fixture = unpaid_shift();
        fixture
            .app
            .world_mut()
            .entity_mut(fixture.site)
            .insert(shared::components::OwnedBy(PersonId(4)));
        fixture
            .app
            .world_mut()
            .entity_mut(fixture.worker)
            .insert((Wallet::new(10_000), shared::components::AboardBoat));
        fixture.app.world_mut().spawn((
            PersonId(5),
            CharacterName("Ready replacement".into()),
            VillagerIntent::Resident {
                settlement: fixture.settlement,
            },
            Wallet::default(),
            Occupation::default(),
            WorkStatus::LookingForWork,
        ));
        fixture
            .app
            .world_mut()
            .get_mut::<CompanyAccount>(fixture.company)
            .unwrap()
            .cash = 10_000;
        fixture
            .app
            .world_mut()
            .get_mut::<WorldTime>(fixture.clock)
            .unwrap()
            .day = 3;
        fixture.app.update();
        assert!(
            fixture
                .app
                .world()
                .get::<EmployedAt>(fixture.worker)
                .is_some()
        );
        assert_eq!(
            *fixture
                .app
                .world()
                .get::<WorkStatus>(fixture.worker)
                .unwrap(),
            WorkStatus::Employed
        );
        fixture
            .app
            .world_mut()
            .entity_mut(fixture.worker)
            .remove::<shared::components::AboardBoat>();
        fixture
            .app
            .world_mut()
            .get_mut::<WorldTime>(fixture.clock)
            .unwrap()
            .day = 4;
        fixture.app.update();
        assert!(
            fixture
                .app
                .world()
                .get::<EmployedAt>(fixture.worker)
                .is_none()
        );
        assert_eq!(
            *fixture
                .app
                .world()
                .get::<WorkStatus>(fixture.worker)
                .unwrap(),
            WorkStatus::Chilling
        );
    }

    #[test]
    fn combined_creditors_cannot_exceed_the_site_liability_limit() {
        let mut claims = PrivatePayrollClaims::default();
        assert_eq!(claims.accrue(PersonId(1), u64::MAX - 5), u64::MAX - 5);
        assert_eq!(claims.accrue(PersonId(2), 10), 5);
        assert_eq!(claims.accrue(PersonId(3), 10), 0);
        assert_eq!(claims.outstanding(), u64::MAX);
        assert_eq!(claims.claims.len(), 2);
        assert_eq!(claims.settle(PersonId(1), 20), 20);
        assert_eq!(claims.accrue(PersonId(2), 30), 20);
        assert_eq!(claims.outstanding(), u64::MAX);
    }

    #[test]
    fn old_wages_keep_their_creditor_and_amount_when_new_staff_join() {
        let mut claims = PrivatePayrollClaims::default();
        claims.accrue(PersonId(7), 100);
        claims.accrue(PersonId(8), 300);
        claims.accrue(PersonId(7), 150);
        assert_eq!(claims.outstanding(), 550);
        assert_eq!(claims.settle(PersonId(8), 400), 300);
        assert_eq!(
            claims.claims,
            vec![BusinessWageClaim {
                worker: PersonId(7),
                pennies: 250
            }]
        );
    }

    #[test]
    fn payroll_rounding_conserves_budget_and_rotates_indivisible_pennies() {
        let mut claims = PrivatePayrollClaims::default();
        for worker in [3, 1, 2] {
            claims.accrue(PersonId(worker), 100);
        }
        for day in 0..3 {
            let payments = claims.payment_plan(5, day);
            assert_eq!(payments.iter().map(|claim| claim.pennies).sum::<u64>(), 5);
            assert_eq!(
                payments.iter().filter(|claim| claim.pennies == 2).count(),
                2
            );
            assert_eq!(payments[(day as usize + 2) % 3].pennies, 1);
        }
        let full = claims.payment_plan(u64::MAX, 0);
        assert!(full.iter().all(|claim| claim.pennies == 100));
    }

    #[test]
    fn monetary_extremes_remain_bounded() {
        let mut claims = PrivatePayrollClaims::default();
        assert_eq!(claims.accrue(PersonId(1), u64::MAX), u64::MAX);
        assert_eq!(claims.accrue(PersonId(1), 1), 0);
        assert_eq!(claims.payment_plan(u64::MAX, 0)[0].pennies, u64::MAX);
        assert_eq!(claims.settle(PersonId(1), u64::MAX), u64::MAX);
        assert_eq!(claims.outstanding(), 0);
    }
}
