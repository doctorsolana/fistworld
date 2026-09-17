use super::*;
use bevy::ecs::system::RunSystemOnce;
use shared::economy::{
    BusinessAccount, BusinessCondition, BusinessManagementPolicy, BusinessProcurementPolicy,
    BusinessSalePolicy, BusinessStaffingPolicy, BusinessState, BusinessStrategy,
    BusinessSupplyPolicy, BusinessWagePolicy, CompanyDecisionHistory, CompanyDividendCapacity,
    Good,
};

const COMPANY: CompanyId = CompanyId(42);
const MASTER: PersonId = PersonId(7);
const OTHER: PersonId = PersonId(8);

struct Fixture {
    world: World,
    company: Entity,
    master: Entity,
    other: Entity,
    /// Stands in for the `ClientOf` link entity that delivered the order.
    link: Entity,
}

impl Fixture {
    fn new() -> Self {
        let mut world = World::new();
        world.init_resource::<CompanyDividendQueue>();
        let link = world.spawn_empty().id();
        let company = world
            .spawn((
                COMPANY,
                CompanyLeadership { master: MASTER },
                CompanyOwnership::sole(MASTER),
                CompanyAccount {
                    cash: 900,
                    ..default()
                },
                CompanyBranchPolicies::default(),
                CompanyManagementPolicy::default(),
                CompanyShareMarket::default(),
                CompanyDecisionHistory::default(),
            ))
            .id();
        let master = world.spawn((MASTER, Wallet::new(1_000))).id();
        let other = world.spawn((OTHER, Wallet::new(2_000))).id();
        Self {
            world,
            company,
            master,
            other,
            link,
        }
    }

    fn order(
        &mut self,
        actor: Entity,
        action: HeroCompanyAction,
    ) -> Result<Option<String>, &'static str> {
        let person = *self.world.get::<PersonId>(actor).unwrap();
        let link = self.link;
        self.world
            .run_system_once(move |mut commands: CompanyOrders| {
                commands.execute(
                    person,
                    actor,
                    link,
                    HeroCompanyOrder {
                        company: COMPANY,
                        action,
                    },
                    7,
                )
            })
            .unwrap()
    }

    fn pending_dividends(&self) -> Vec<crate::world::village::DividendRequest> {
        self.world
            .resource::<CompanyDividendQueue>()
            .pending()
            .to_vec()
    }
}

#[test]
fn company_governance_and_finance_need_no_live_or_operating_site() {
    for state in [
        None,
        Some(BusinessState::Closed),
        Some(BusinessState::ForSale),
    ] {
        let mut f = Fixture::new();
        if let Some(state) = state {
            f.world.spawn((
                OperatedBy(COMPANY),
                BuildingOf(SettlementId(1)),
                GoodsInventory::new(50),
                BusinessCondition { state, ..default() },
            ));
        }
        f.order(
            f.master,
            HeroCompanyAction::SetStrategy(BusinessStrategy::Conservative),
        )
        .unwrap();
        f.order(f.master, HeroCompanyAction::SetAutomaticDividend { payout_percent: 0 })
            .unwrap();
        f.order(
            f.master,
            HeroCompanyAction::ContributeCapital { amount: 125 },
        )
        .unwrap();
        assert_eq!(
            f.order(
                f.master,
                HeroCompanyAction::DistributeDividend { pennies: u64::MAX }
            ),
            Ok(None),
            "a dividend order is deferred: no immediate result may be sent"
        );
        let policy = f.world.get::<CompanyManagementPolicy>(f.company).unwrap();
        assert_eq!(policy.strategy, BusinessStrategy::Conservative);
        assert!(!policy.autopilot);
        assert_eq!(policy.automatic_payout_percent, 0);
        assert_eq!(f.world.get::<Wallet>(f.master).unwrap().balance(), 875);
        let account = f.world.get::<CompanyAccount>(f.company).unwrap();
        assert_eq!((account.cash, account.contributed_capital), (1_025, 125));
        assert_eq!(
            account.owner_withdrawals, 0,
            "the command only queues the finance pass; nothing is paid synchronously"
        );
        assert_eq!(
            f.pending_dividends(),
            vec![crate::world::village::DividendRequest {
                company: COMPANY,
                pennies: u64::MAX,
                requester_person: MASTER,
                requester_link: f.link,
            }]
        );
    }
}

#[test]
fn distribute_dividend_enqueues_amount_person_and_link_and_sends_no_immediate_result() {
    let mut f = Fixture::new();
    f.world
        .entity_mut(f.company)
        .insert(CompanyDividendCapacity {
            day: 7,
            distributable: 50,
            ..default()
        });
    assert_eq!(
        f.order(
            f.master,
            HeroCompanyAction::DistributeDividend { pennies: 300 }
        ),
        Ok(None)
    );
    assert_eq!(
        f.pending_dividends(),
        vec![crate::world::village::DividendRequest {
            company: COMPANY,
            pennies: 300,
            requester_person: MASTER,
            requester_link: f.link,
        }],
        "amounts above the replicated snapshot are clamped by the finance pass, never rejected here"
    );
    assert_eq!(
        f.order(
            f.master,
            HeroCompanyAction::DistributeDividend { pennies: 500 }
        ),
        Ok(None)
    );
    let pending = f.pending_dividends();
    assert_eq!(
        pending.len(),
        1,
        "one request per company; the newer amount replaces the older"
    );
    assert_eq!(pending[0].pennies, 500);
    assert_eq!(f.world.get::<CompanyAccount>(f.company).unwrap().cash, 900);
    assert_eq!(f.world.get::<Wallet>(f.master).unwrap().balance(), 1_000);
}

#[test]
fn minority_shareholders_can_sell_their_own_interest_but_cannot_direct_company_policy() {
    let mut f = Fixture::new();
    assert!(f
        .world
        .get_mut::<CompanyOwnership>(f.company)
        .unwrap()
        .transfer(MASTER, OTHER, 400));
    let policy = *f.world.get::<CompanyManagementPolicy>(f.company).unwrap();
    for action in [
        HeroCompanyAction::SetStrategy(BusinessStrategy::Aggressive),
        HeroCompanyAction::SetAutopilot(false),
        HeroCompanyAction::SetAutomaticDividend { payout_percent: 0 },
        HeroCompanyAction::DistributeDividend { pennies: u64::MAX },
        HeroCompanyAction::DistributeDividend { pennies: 1 },
        HeroCompanyAction::AppointCompanyMaster(OTHER),
    ] {
        assert!(f.order(f.other, action).is_err());
    }
    assert!(
        f.pending_dividends().is_empty(),
        "a minority holder's dividend request must not reach the finance pass"
    );
    assert_eq!(
        *f.world.get::<CompanyManagementPolicy>(f.company).unwrap(),
        policy
    );
    f.order(
        f.other,
        HeroCompanyAction::ListCompanyShares {
            shares: 100,
            unit_price: 2,
        },
    )
    .unwrap();
    assert_eq!(
        f.world
            .get::<CompanyShareMarket>(f.company)
            .unwrap()
            .offer_from(OTHER)
            .unwrap()
            .shares,
        100
    );
    assert!(f
        .order(
            f.master,
            HeroCompanyAction::AppointCompanyMaster(PersonId(999))
        )
        .is_err());
    f.order(f.master, HeroCompanyAction::AppointCompanyMaster(OTHER))
        .unwrap();
    assert_eq!(
        f.world.get::<CompanyLeadership>(f.company).unwrap().master,
        OTHER
    );
    assert!(
        f.order(f.master, HeroCompanyAction::SetAutomaticDividend { payout_percent: 0 })
            .is_err(),
        "majority ownership does not bypass the appointed executive"
    );
    f.order(f.other, HeroCompanyAction::SetAutomaticDividend { payout_percent: 0 })
        .unwrap();
    f.order(f.other, HeroCompanyAction::CancelCompanyShareListing)
        .unwrap();
    assert!(f
        .world
        .get::<CompanyShareMarket>(f.company)
        .unwrap()
        .offer_from(OTHER)
        .is_none());
}

#[test]
fn company_share_commands_settle_actual_wallets_and_reject_overflow_atomically() {
    let mut f = Fixture::new();
    f.order(
        f.master,
        HeroCompanyAction::ListCompanyShares {
            shares: 1_000,
            unit_price: 1,
        },
    )
    .unwrap();
    f.order(
        f.other,
        HeroCompanyAction::BuyCompanyShares {
            seller: MASTER,
            shares: 100,
        },
    )
    .unwrap();
    assert_eq!(f.world.get::<Wallet>(f.master).unwrap().balance(), 1_100);
    assert_eq!(f.world.get::<Wallet>(f.other).unwrap().balance(), 1_900);
    assert_eq!(
        f.world
            .get::<CompanyOwnership>(f.company)
            .unwrap()
            .share_count(OTHER),
        100
    );
    f.world.entity_mut(f.master).insert(Wallet::new(u64::MAX));
    let ownership = f.world.get::<CompanyOwnership>(f.company).unwrap().clone();
    let market = f
        .world
        .get::<CompanyShareMarket>(f.company)
        .unwrap()
        .clone();
    assert!(f
        .order(
            f.other,
            HeroCompanyAction::BuyCompanyShares {
                seller: MASTER,
                shares: 10
            }
        )
        .is_err());
    assert_eq!(f.world.get::<Wallet>(f.other).unwrap().balance(), 1_900);
    assert_eq!(
        f.world.get::<CompanyOwnership>(f.company).unwrap(),
        &ownership
    );
    assert_eq!(
        f.world.get::<CompanyShareMarket>(f.company).unwrap(),
        &market
    );
    f.world.entity_mut(f.master).insert(Wallet::new(1_100));
    f.order(
        f.other,
        HeroCompanyAction::BuyCompanyShares {
            seller: MASTER,
            shares: 900,
        },
    )
    .unwrap();
    assert_eq!(
        f.world.get::<CompanyLeadership>(f.company).unwrap().master,
        OTHER
    );
    let ownership = f.world.get::<CompanyOwnership>(f.company).unwrap();
    assert_eq!(ownership.share_count(OTHER), COMPANY_TOTAL_SHARES);
    assert_eq!(
        ownership
            .shares()
            .iter()
            .map(|share| u32::from(share.shares))
            .sum::<u32>(),
        1_000
    );
    assert_eq!(
        f.world.get::<Wallet>(f.master).unwrap().balance()
            + f.world.get::<Wallet>(f.other).unwrap().balance(),
        3_000
    );
}

#[test]
fn capital_refusal_preserves_both_ledgers() {
    for (cash, contributed) in [(u64::MAX, 0), (900, u64::MAX)] {
        let mut f = Fixture::new();
        f.world.entity_mut(f.company).insert(CompanyAccount {
            cash,
            contributed_capital: contributed,
            ..default()
        });
        let account = *f.world.get::<CompanyAccount>(f.company).unwrap();
        assert!(f
            .order(f.master, HeroCompanyAction::ContributeCapital { amount: 1 })
            .is_err());
        assert_eq!(*f.world.get::<CompanyAccount>(f.company).unwrap(), account);
        assert_eq!(f.world.get::<Wallet>(f.master).unwrap().balance(), 1_000);
    }
    let mut f = Fixture::new();
    assert!(
        f.order(
            f.master,
            HeroCompanyAction::ContributeCapital { amount: 1_001 }
        )
        .is_err()
    );
    assert!(
        f.order(f.master, HeroCompanyAction::ContributeCapital { amount: 0 })
            .is_err()
    );
    assert_eq!(f.world.get::<CompanyAccount>(f.company).unwrap().cash, 900);
    assert_eq!(f.world.get::<Wallet>(f.master).unwrap().balance(), 1_000);
}

#[test]
fn physical_branch_controls_still_require_the_companys_own_site_in_that_settlement() {
    let mut f = Fixture::new();
    let here = SettlementId(3);
    let elsewhere = SettlementId(4);
    f.world.spawn(here);
    f.world.spawn(elsewhere);
    f.world.spawn((
        OperatedBy(CompanyId(99)),
        BuildingOf(here),
        GoodsInventory::new(50),
    ));
    f.world.spawn((
        OperatedBy(COMPANY),
        BuildingOf(elsewhere),
        GoodsInventory::new(50),
    ));
    for action in [
        HeroCompanyAction::SetRetainUnits {
            settlement: here,
            good: Good::Wood,
            units: 5,
        },
        HeroCompanyAction::SetSellExcess {
            settlement: here,
            good: Good::Wood,
            enabled: false,
        },
    ] {
        assert!(f.order(f.master, action).is_err());
    }
    assert!(f
        .world
        .get::<CompanyBranchPolicies>(f.company)
        .unwrap()
        .branches()
        .is_empty());
    f.order(
        f.master,
        HeroCompanyAction::SetSellExcess {
            settlement: elsewhere,
            good: Good::Wood,
            enabled: false,
        },
    )
    .unwrap();
    let branches = f.world.get::<CompanyBranchPolicies>(f.company).unwrap();
    assert!(!branches.resource(elsewhere, Good::Wood).sell_excess);
    assert!(branches.branch(here).is_none());
}

#[test]
fn company_strategy_reaches_automatic_siblings_but_preserves_a_manual_site_override() {
    let mut f = Fixture::new();
    f.world.spawn(WorldTime::new_default());
    let mut local = BusinessManagementPolicy::default();
    let mut sale = BusinessSalePolicy::for_good(Good::Flour);
    super::super::apply_owner_action(
        shared::protocol::HeroBusinessAction::SetStrategy(BusinessStrategy::Aggressive),
        None,
        &mut local,
        &mut BusinessWagePolicy::default(),
        &mut sale,
        &mut BusinessProcurementPolicy::default(),
        &mut BusinessSupplyPolicy::default(),
        &mut BusinessStaffingPolicy::new(2),
        2,
    )
    .unwrap();
    assert_eq!(
        f.world
            .get::<CompanyManagementPolicy>(f.company)
            .unwrap()
            .strategy,
        BusinessStrategy::Balanced
    );
    let manual = f
        .world
        .spawn((OperatedBy(COMPANY), BusinessAccount::default(), local))
        .id();
    let automatic = f
        .world
        .spawn((
            OperatedBy(COMPANY),
            BusinessAccount::default(),
            BusinessManagementPolicy::default(),
        ))
        .id();
    f.order(
        f.master,
        HeroCompanyAction::SetStrategy(BusinessStrategy::Conservative),
    )
    .unwrap();
    f.world
        .run_system_once(crate::world::village::review_company_strategies)
        .unwrap();
    assert_eq!(
        f.world
            .get::<BusinessManagementPolicy>(automatic)
            .unwrap()
            .strategy,
        BusinessStrategy::Conservative
    );
    let local = f.world.get::<BusinessManagementPolicy>(manual).unwrap();
    assert_eq!(local.strategy, BusinessStrategy::Aggressive);
    assert!(!local.autopilot);
    assert_eq!(
        f.world
            .get::<CompanyManagementPolicy>(f.company)
            .unwrap()
            .strategy,
        BusinessStrategy::Conservative
    );
}

#[test]
fn any_shareholder_may_contribute_capital_as_a_donation() {
    let mut f = Fixture::new();
    assert!(
        f.world
            .get_mut::<CompanyOwnership>(f.company)
            .unwrap()
            .transfer(MASTER, OTHER, 400)
    );
    // The minority holder donates without any Master involvement.
    assert!(
        f.order(
            f.other,
            HeroCompanyAction::ContributeCapital { amount: 100 }
        )
        .is_ok()
    );
    assert_eq!(f.world.get::<Wallet>(f.other).unwrap().balance(), 1_900);
    let account = f.world.get::<CompanyAccount>(f.company).unwrap();
    assert_eq!((account.cash, account.contributed_capital), (1_000, 100));
    // So does the co-owning Master.
    assert!(
        f.order(
            f.master,
            HeroCompanyAction::ContributeCapital { amount: 50 }
        )
        .is_ok()
    );
    assert_eq!(f.world.get::<Wallet>(f.master).unwrap().balance(), 950);
    let account = f.world.get::<CompanyAccount>(f.company).unwrap();
    assert_eq!((account.cash, account.contributed_capital), (1_050, 150));
    let ownership = f.world.get::<CompanyOwnership>(f.company).unwrap();
    assert_eq!(ownership.share_count(MASTER), 600);
    assert_eq!(ownership.share_count(OTHER), 400);
    assert_eq!(
        f.world.get::<CompanyLeadership>(f.company).unwrap().master,
        MASTER
    );
    // Someone with a wallet but no shares may not.
    let stranger = f.world.spawn((PersonId(9), Wallet::new(500))).id();
    assert_eq!(
        f.order(stranger, HeroCompanyAction::ContributeCapital { amount: 1 }),
        Err("Only a shareholder may contribute capital.")
    );
    assert_eq!(f.world.get::<Wallet>(stranger).unwrap().balance(), 500);
    assert_eq!(
        f.world.get::<CompanyAccount>(f.company).unwrap().cash,
        1_050
    );
}

#[test]
fn a_contribution_never_changes_the_cap_table_or_revenue() {
    let mut f = Fixture::new();
    assert!(
        f.world
            .get_mut::<CompanyOwnership>(f.company)
            .unwrap()
            .transfer(MASTER, OTHER, 400)
    );
    f.order(
        f.other,
        HeroCompanyAction::ListCompanyShares {
            shares: 10,
            unit_price: 5,
        },
    )
    .unwrap();
    let ownership = f.world.get::<CompanyOwnership>(f.company).unwrap().clone();
    let market = f
        .world
        .get::<CompanyShareMarket>(f.company)
        .unwrap()
        .clone();
    let before = *f.world.get::<CompanyAccount>(f.company).unwrap();

    f.order(
        f.other,
        HeroCompanyAction::ContributeCapital { amount: 250 },
    )
    .unwrap();

    let account = *f.world.get::<CompanyAccount>(f.company).unwrap();
    assert_eq!(account.cash, before.cash + 250);
    assert_eq!(
        account.contributed_capital,
        before.contributed_capital + 250
    );
    assert_eq!(
        CompanyAccount {
            cash: before.cash,
            contributed_capital: before.contributed_capital,
            ..account
        },
        before,
        "a donation touches cash and the lifetime contribution total only: no revenue, no ledger line, no withdrawal"
    );
    assert_eq!(
        f.world.get::<CompanyOwnership>(f.company).unwrap(),
        &ownership,
        "no shares are issued for a donation"
    );
    assert_eq!(
        f.world.get::<CompanyShareMarket>(f.company).unwrap(),
        &market
    );
}

#[test]
fn a_zero_snapshot_all_request_is_enqueued_not_refused() {
    let mut f = Fixture::new();
    f.world
        .entity_mut(f.company)
        .insert(CompanyDividendCapacity {
            day: 7,
            distributable: 0,
            ..default()
        });
    assert_eq!(
        f.order(
            f.master,
            HeroCompanyAction::DistributeDividend { pennies: u64::MAX }
        ),
        Ok(None),
        "the snapshot is stale within the day; the finance pass answers from the live figure"
    );
    assert_eq!(
        f.pending_dividends(),
        vec![crate::world::village::DividendRequest {
            company: COMPANY,
            pennies: u64::MAX,
            requester_person: MASTER,
            requester_link: f.link,
        }]
    );
    // Even a literal zero is answered by the pass (and republishes the
    // snapshot) instead of being refused against stale data here.
    assert_eq!(
        f.order(
            f.master,
            HeroCompanyAction::DistributeDividend { pennies: 0 }
        ),
        Ok(None)
    );
    assert_eq!(f.pending_dividends()[0].pennies, 0);
    assert_eq!(f.world.get::<CompanyAccount>(f.company).unwrap().cash, 900);
}

fn policy_of(f: &Fixture) -> CompanyManagementPolicy {
    *f.world.get::<CompanyManagementPolicy>(f.company).unwrap()
}

#[test]
fn set_automatic_dividend_rejects_more_than_fifty_percent_and_non_masters() {
    let mut f = Fixture::new();
    f.world
        .entity_mut(f.company)
        .insert(CompanyManagementPolicy {
            automatic_payout_percent: 0,
            ..default()
        });
    // Any share up to half of the headroom is accepted and named in the reply.
    assert_eq!(
        f.order(
            f.master,
            HeroCompanyAction::SetAutomaticDividend { payout_percent: 25 }
        ),
        Ok(Some(
            "Automatic dividend set: 25% of retained profit above the working-capital runway, paid daily."
                .to_string()
        ))
    );
    assert_eq!(policy_of(&f).automatic_payout_percent, 25);
    f.order(
        f.master,
        HeroCompanyAction::SetAutomaticDividend {
            payout_percent: MAX_AUTOMATIC_PAYOUT_PERCENT,
        },
    )
    .unwrap();
    assert_eq!(policy_of(&f).automatic_payout_percent, 50);
    // More than half is refused with the reason and leaves the policy alone.
    assert_eq!(
        f.order(
            f.master,
            HeroCompanyAction::SetAutomaticDividend { payout_percent: 51 }
        ),
        Err(
            "An automatic dividend may pay at most 50% of retained profit above the working-capital runway per day."
        )
    );
    assert!(f
        .order(
            f.master,
            HeroCompanyAction::SetAutomaticDividend {
                payout_percent: u8::MAX
            }
        )
        .is_err());
    assert_eq!(policy_of(&f).automatic_payout_percent, 50);
    // Zero retains profits.
    assert_eq!(
        f.order(
            f.master,
            HeroCompanyAction::SetAutomaticDividend { payout_percent: 0 }
        ),
        Ok(Some(
            "Automatic dividends off: profits are retained.".to_string()
        ))
    );
    assert_eq!(policy_of(&f).automatic_payout_percent, 0);
    // A majority holder who is not the Master and a stranger cannot change it.
    assert!(f
        .world
        .get_mut::<CompanyOwnership>(f.company)
        .unwrap()
        .transfer(MASTER, OTHER, 600));
    let stranger = f.world.spawn((PersonId(9), Wallet::new(0))).id();
    for actor in [f.other, stranger] {
        assert_eq!(
            f.order(
                actor,
                HeroCompanyAction::SetAutomaticDividend { payout_percent: 10 }
            ),
            Err("Only the appointed Company Master may act for this company.")
        );
    }
    assert_eq!(policy_of(&f).automatic_payout_percent, 0);
}

#[test]
fn an_unchanged_policy_write_is_not_marked_changed() {
    fn changed(f: &mut Fixture) -> bool {
        let company = f.company;
        f.world
            .query_filtered::<Entity, Changed<CompanyManagementPolicy>>()
            .iter(&f.world)
            .any(|entity| entity == company)
    }
    let mut f = Fixture::new();
    f.world
        .entity_mut(f.company)
        .insert(CompanyManagementPolicy {
            automatic_payout_percent: 0,
            ..default()
        });
    // A real change is visible to replication.
    f.world.clear_trackers();
    f.order(
        f.master,
        HeroCompanyAction::SetAutomaticDividend { payout_percent: 25 },
    )
    .unwrap();
    assert!(changed(&mut f));
    // Repeating the same choice writes nothing.
    f.world.clear_trackers();
    f.order(
        f.master,
        HeroCompanyAction::SetAutomaticDividend { payout_percent: 25 },
    )
    .unwrap();
    assert!(
        !changed(&mut f),
        "an unchanged dividend policy must not dirty the replicated component"
    );
    // Neither does a refused share, nor an autopilot flag already in that state.
    f.world.clear_trackers();
    assert!(f
        .order(
            f.master,
            HeroCompanyAction::SetAutomaticDividend { payout_percent: 60 }
        )
        .is_err());
    f.order(f.master, HeroCompanyAction::SetAutopilot(true))
        .unwrap();
    assert!(!changed(&mut f));
    assert_eq!(policy_of(&f).automatic_payout_percent, 25);
}
