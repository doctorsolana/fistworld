//! Server-authoritative management of player-owned businesses.
//!
//! Player owners edit the same policies the NPC decision tree reads. The UI is
//! merely a remote control: identity, ownership, operating state and numeric
//! bounds are proved again here for every click.

use bevy::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::{MessageReceiver, MessageSender, RemoteId};

use shared::components::{
    BuildingId, BuildingOf, CompanyId, CompanyLeadership, Hero, OperatedBy, OwnedBy, PersonId,
    SettlementBuilding, SettlementId,
};
use shared::economy::{
    BusinessAccount, BusinessCondition, BusinessManagementPolicy, BusinessProcurementPolicy,
    BusinessSalePolicy, BusinessStaffingPolicy, BusinessState, BusinessStrategy,
    BusinessSupplyPolicy, BusinessWagePolicy, CompanyManagementPolicy, MarketSeller, MootMarket,
    MAXIMUM_BUSINESS_DAILY_WAGE, MINIMUM_BUSINESS_DAILY_WAGE, PENNIES_PER_COIN,
};
use shared::protocol::{
    HeroBusinessAction, HeroBusinessOrder, HeroBusinessResult, ReliableChannel,
};

use super::hero::OfflineHero;

/// A deliberately generous sanity bound, not a market price control. It keeps
/// hostile packets from filling histories/UI with overflow-adjacent values.
const MAX_MANUAL_UNIT_PRICE: u64 = 1_000 * PENNIES_PER_COIN;

fn accepts_operating_policy(condition: Option<&BusinessCondition>) -> bool {
    condition.is_none_or(|condition| {
        condition.state.can_operate() || condition.state == BusinessState::Mothballed
    })
}

fn apply_site_strategy(
    strategy: BusinessStrategy,
    management: &mut BusinessManagementPolicy,
    sale: &mut BusinessSalePolicy,
) {
    management.strategy = strategy;
    management.payroll_reserve_days = strategy.payroll_reserve_days();
    sale.target_margin_bps = strategy.target_margin_bps();
    sale.max_daily_price_change_bps = strategy.daily_price_step_bps();
}

#[allow(clippy::too_many_arguments)]
fn apply_owner_action(
    action: HeroBusinessAction,
    condition: Option<&mut BusinessCondition>,
    management: &mut BusinessManagementPolicy,
    wage: &mut BusinessWagePolicy,
    sale: &mut BusinessSalePolicy,
    procurement: &mut BusinessProcurementPolicy,
    supply: &mut BusinessSupplyPolicy,
    staffing: &mut BusinessStaffingPolicy,
    maximum_positions: u8,
) -> Result<&'static str, &'static str> {
    if !accepts_operating_policy(condition.as_deref()) {
        return Err("A closed, liquidating or for-sale business cannot change operating policy.");
    }
    match action {
        HeroBusinessAction::SetStrategy(strategy) => {
            apply_site_strategy(strategy, management, sale);
            management.autopilot = false;
            Ok("Site strategy updated; automatic company policy is disabled for this site.")
        }
        HeroBusinessAction::SetAutopilot(enabled) => {
            management.autopilot = enabled;
            Ok(if enabled {
                "Automatic site management enabled."
            } else {
                "Automatic site management paused; your local policy values will be retained."
            })
        }
        HeroBusinessAction::SetDailyWage(value) => {
            wage.daily_wage = value.clamp(MINIMUM_BUSINESS_DAILY_WAGE, MAXIMUM_BUSINESS_DAILY_WAGE);
            wage.automatic = false;
            Ok("Daily wage set; automatic wage review is off.")
        }
        HeroBusinessAction::SetEnabledPositions(positions) => {
            staffing.enabled_positions = positions.min(maximum_positions);
            management.autopilot = false;
            if staffing.enabled_positions > 0 {
                if let Some(condition) =
                    condition.filter(|condition| condition.state == BusinessState::Mothballed)
                {
                    // Reuse the intact site. Financial liabilities, production
                    // history and physical inventory survive this manual restart.
                    condition.state = BusinessState::Operating;
                    return Ok(
                        "Business reopened with your open-position target; owner autopilot is paused.",
                    );
                }
            }
            Ok("Open-position target updated; owner autopilot is paused.")
        }
        HeroBusinessAction::SetAutomaticWage(enabled) => {
            wage.automatic = enabled;
            Ok("Wage review policy updated.")
        }
        HeroBusinessAction::SetAskingPrice(value) => {
            sale.asking_unit_price = value.clamp(1, MAX_MANUAL_UNIT_PRICE);
            sale.automatic_pricing = false;
            Ok("Asking price set; automatic pricing is off.")
        }
        HeroBusinessAction::SetAutomaticPricing(enabled) => {
            sale.automatic_pricing = enabled;
            Ok("Sale-pricing policy updated.")
        }
        HeroBusinessAction::SetCollectionEnabled(enabled) => {
            sale.collection_enabled = enabled;
            Ok("Hall collection policy updated.")
        }
        HeroBusinessAction::SetOutputReserveDays(days) => {
            sale.company_reserve_days = days.min(shared::economy::MAXIMUM_STOCK_COVERAGE_DAYS);
            management.autopilot = false;
            Ok("Site output reserve updated; automatic site management is paused.")
        }
        HeroBusinessAction::SetAutomaticProcurement(enabled) => {
            procurement.automatic = enabled;
            supply.automatic = enabled;
            Ok("Input procurement policy updated.")
        }
        HeroBusinessAction::SetInputMaximumPrice { good, unit_price } => {
            let mut rule = procurement.rule(good);
            if !rule.enabled {
                return Err("That business does not consume this input.");
            }
            rule.maximum_unit_price = unit_price.clamp(1, MAX_MANUAL_UNIT_PRICE);
            procurement.set_rule(good, rule);
            Ok("Input bid ceiling updated.")
        }
        HeroBusinessAction::SetInputCoverageDays { good, days } => {
            let mut rule = procurement.rule(good);
            if !rule.enabled {
                return Err("That business does not consume this input.");
            }
            rule.set_coverage_days(days);
            procurement.set_rule(good, rule);
            management.autopilot = false;
            Ok("Input coverage updated; owner autopilot is paused.")
        }
        HeroBusinessAction::SetInputSourcingMode { good, mode } => {
            let mut rule = supply.rule(good);
            if !rule.enabled || !procurement.rule(good).enabled {
                return Err("That business does not consume this input.");
            }
            rule.sourcing = mode;
            supply.set_rule(good, rule);
            Ok("Private sourcing strategy updated.")
        }
        HeroBusinessAction::SetPreferredSupplier { good, supplier } => {
            let mut rule = supply.rule(good);
            if !rule.enabled || !procurement.rule(good).enabled {
                return Err("That business does not consume this input.");
            }
            rule.preferred_supplier = supplier;
            supply.set_rule(good, rule);
            Ok("Preferred company supplier updated.")
        }
    }
}

mod company;
mod dividend_reports;
pub use company::handle_hero_company_orders;
pub use dividend_reports::report_dividend_outcomes;

#[allow(clippy::type_complexity)]
pub fn handle_hero_business_orders(
    mut links: Query<
        (
            &RemoteId,
            &mut MessageReceiver<HeroBusinessOrder>,
            &mut MessageSender<HeroBusinessResult>,
        ),
        With<ClientOf>,
    >,
    heroes: Query<(&Hero, &PersonId), Without<OfflineHero>>,
    companies: Query<(&CompanyId, &CompanyLeadership, &CompanyManagementPolicy)>,
    mut businesses: Query<(
        Option<&OwnedBy>,
        Option<&OperatedBy>,
        &SettlementBuilding,
        &BuildingId,
        &BuildingOf,
        &BusinessAccount,
        &mut BusinessManagementPolicy,
        &mut BusinessWagePolicy,
        &mut BusinessSalePolicy,
        &mut BusinessProcurementPolicy,
        &mut BusinessSupplyPolicy,
        &mut BusinessStaffingPolicy,
        Option<&mut BusinessCondition>,
    )>,
    suppliers: Query<(&BuildingId, &OperatedBy, &SettlementBuilding)>,
    mut halls: Query<(&SettlementId, &mut MootMarket)>,
) {
    for (remote, mut receiver, mut sender) in links.iter_mut() {
        for order in receiver.receive() {
            let response = (|| {
                if order.business == Entity::PLACEHOLDER {
                    return Err("That business is unavailable.");
                }
                let Some((_, person)) = heroes.iter().find(|(hero, _)| hero.owner == remote.0)
                else {
                    return Err("Create your hero before managing a business.");
                };
                let Ok((
                    owned_by,
                    operated_by,
                    building,
                    building_id,
                    building_of,
                    _account,
                    mut management,
                    mut wage,
                    mut sale,
                    mut procurement,
                    mut supply,
                    mut staffing,
                    mut condition,
                )) = businesses.get_mut(order.business)
                else {
                    return Err("That building is not an operating business.");
                };
                let company = operated_by.map(|operation| operation.0);
                let company_policy =
                    company.and_then(|company| companies.iter().find(|(id, ..)| **id == company));
                let authorized = if company.is_some() {
                    company_policy.is_some_and(|(_, leadership, _)| leadership.can_manage(*person))
                } else {
                    owned_by.is_some_and(|owner| owner.0 == *person)
                };
                if !authorized {
                    return Err(
                        "Only this company's appointed Company Master may change operating decisions.",
                    );
                }
                if let HeroBusinessAction::SetPreferredSupplier {
                    good,
                    supplier: Some(supplier),
                } = order.action
                {
                    let Some(company) = company else {
                        return Err("Private suppliers require a company.");
                    };
                    if !suppliers.iter().any(|(id, operation, candidate)| {
                        *id == supplier
                            && operation.0 == company
                            && crate::world::village::business_output(candidate.kind) == Some(good)
                    }) {
                        return Err("That site is not an owned supplier of the selected good.");
                    }
                }
                let message = apply_owner_action(
                    order.action,
                    condition.as_deref_mut(),
                    &mut management,
                    &mut wage,
                    &mut sale,
                    &mut procurement,
                    &mut supply,
                    &mut staffing,
                    building.kind.positions(),
                )?;
                if matches!(order.action, HeroBusinessAction::SetAutopilot(true)) {
                    if let Some((_, _, policy)) = company_policy {
                        // Company fanout is change-driven. Resume this site from
                        // its current default even when the company is unchanged.
                        apply_site_strategy(policy.strategy, &mut management, &mut sale);
                    }
                }
                // Site commands never mutate company governance or policy.
                // Reprice this site's existing consignment immediately.
                if matches!(order.action, HeroBusinessAction::SetAskingPrice(_)) {
                    if let (Some((_, mut market)), Some(good)) = (
                        halls.iter_mut().find(|(id, _)| **id == building_of.0),
                        crate::world::village::business_output(building.kind),
                    ) {
                        market.reprice(
                            MarketSeller::Business(*building_id),
                            good,
                            sale.asking_unit_price,
                        );
                    }
                }
                Ok(format!("{}: {message}", building.kind.label()))
            })();
            let (success, message) = match response {
                Ok(message) => (true, message),
                Err(message) => (false, message.to_string()),
            };
            sender.send::<ReliableChannel>(HeroBusinessResult {
                business: order.business,
                success,
                message,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::company::{apply_company_policy_action, execute_share_purchase};
    use super::*;
    use shared::components::{CompanyOwnership, CompanyShareMarket, COMPANY_TOTAL_SHARES};
    use shared::economy::{BusinessStrategy, CompanyManagementPolicy, Good, Wallet};
    use shared::protocol::HeroCompanyAction;

    #[test]
    fn manual_values_disable_only_their_own_automation() {
        let mut management = BusinessManagementPolicy::default();
        let mut wage = BusinessWagePolicy::default();
        let mut sale = BusinessSalePolicy::for_good(Good::Flour);
        let mut procurement = BusinessProcurementPolicy::default();
        let mut supply = BusinessSupplyPolicy::default();
        let mut staffing = BusinessStaffingPolicy::new(2);
        apply_owner_action(
            HeroBusinessAction::SetDailyWage(275),
            None,
            &mut management,
            &mut wage,
            &mut sale,
            &mut procurement,
            &mut supply,
            &mut staffing,
            2,
        )
        .unwrap();
        assert_eq!(wage.daily_wage, 275);
        assert!(!wage.automatic);
        assert!(sale.automatic_pricing);
        assert!(management.autopilot);
    }

    #[test]
    fn a_paused_site_can_be_edited_but_only_a_positive_staffing_order_reopens_it() {
        let mut management = BusinessManagementPolicy::default();
        let mut wage = BusinessWagePolicy::default();
        let mut sale = BusinessSalePolicy::for_good(Good::Flour);
        let mut procurement = BusinessProcurementPolicy::default();
        let mut supply = BusinessSupplyPolicy::default();
        let mut staffing = BusinessStaffingPolicy::new(0);
        let mut condition = BusinessCondition {
            state: BusinessState::Mothballed,
            opened_day: 2,
            operating_days: 8,
            cash_tight_days: 3,
            ..default()
        };
        for action in [
            HeroBusinessAction::SetAskingPrice(77),
            HeroBusinessAction::SetEnabledPositions(0),
        ] {
            apply_owner_action(
                action,
                Some(&mut condition),
                &mut management,
                &mut wage,
                &mut sale,
                &mut procurement,
                &mut supply,
                &mut staffing,
                2,
            )
            .unwrap();
            assert_eq!(
                condition.state,
                BusinessState::Mothballed,
                "editing a paused policy must not create an unrequested shift"
            );
        }
        assert_eq!(sale.asking_unit_price, 77);
        assert!(!sale.automatic_pricing);
        apply_owner_action(
            HeroBusinessAction::SetEnabledPositions(9),
            Some(&mut condition),
            &mut management,
            &mut wage,
            &mut sale,
            &mut procurement,
            &mut supply,
            &mut staffing,
            2,
        )
        .unwrap();
        assert_eq!(condition.state, BusinessState::Operating);
        assert_eq!(staffing.enabled_positions, 2);
        assert!(
            !management.autopilot,
            "manual staffing must survive the next automatic review"
        );
        assert_eq!(
            (
                condition.opened_day,
                condition.operating_days,
                condition.cash_tight_days
            ),
            (2, 8, 3)
        );
    }

    #[test]
    fn terminal_business_states_reject_policy_and_restart_commands_without_mutating_them() {
        for state in [
            BusinessState::Closed,
            BusinessState::Liquidating,
            BusinessState::ForSale,
        ] {
            let mut management = BusinessManagementPolicy::default();
            let mut wage = BusinessWagePolicy::default();
            let mut sale = BusinessSalePolicy::for_good(Good::Flour);
            let mut procurement = BusinessProcurementPolicy::default();
            let mut supply = BusinessSupplyPolicy::default();
            let mut staffing = BusinessStaffingPolicy::new(0);
            let mut condition = BusinessCondition { state, ..default() };
            assert!(apply_owner_action(
                HeroBusinessAction::SetEnabledPositions(2),
                Some(&mut condition),
                &mut management,
                &mut wage,
                &mut sale,
                &mut procurement,
                &mut supply,
                &mut staffing,
                2
            )
            .is_err());
            assert_eq!(condition.state, state);
            assert_eq!(staffing.enabled_positions, 0);
            assert!(management.autopilot);
        }
    }

    #[test]
    fn a_manual_strategy_remains_authoritative_until_autopilot_is_explicitly_reenabled() {
        let mut policy = CompanyManagementPolicy::default();
        apply_company_policy_action(
            HeroCompanyAction::SetStrategy(BusinessStrategy::Aggressive),
            &mut policy,
        );
        assert_eq!(policy.strategy, BusinessStrategy::Aggressive);
        assert_eq!(
            policy.payroll_reserve_days,
            BusinessStrategy::Aggressive.payroll_reserve_days()
        );
        assert!(!policy.autopilot);
        apply_company_policy_action(HeroCompanyAction::SetAutopilot(true), &mut policy);
        assert!(policy.autopilot);
        assert_eq!(policy.strategy, BusinessStrategy::Aggressive);
    }

    #[test]
    fn selecting_strategy_updates_the_parameters_that_autopilot_reads() {
        let mut management = BusinessManagementPolicy::default();
        let mut wage = BusinessWagePolicy::default();
        let mut sale = BusinessSalePolicy::for_good(Good::Flour);
        let mut procurement = BusinessProcurementPolicy::default();
        let mut supply = BusinessSupplyPolicy::default();
        let mut staffing = BusinessStaffingPolicy::new(2);
        apply_owner_action(
            HeroBusinessAction::SetStrategy(BusinessStrategy::Aggressive),
            None,
            &mut management,
            &mut wage,
            &mut sale,
            &mut procurement,
            &mut supply,
            &mut staffing,
            2,
        )
        .unwrap();
        assert_eq!(management.strategy, BusinessStrategy::Aggressive);
        assert!(
            !management.autopilot,
            "a site override must survive company review"
        );
        assert_eq!(
            management.payroll_reserve_days,
            BusinessStrategy::Aggressive.payroll_reserve_days()
        );
        assert_eq!(
            sale.target_margin_bps,
            BusinessStrategy::Aggressive.target_margin_bps()
        );
    }

    #[test]
    fn simple_stock_controls_clamp_days_and_preserve_derived_units() {
        let mut management = BusinessManagementPolicy::default();
        let mut wage = BusinessWagePolicy::default();
        let mut sale = BusinessSalePolicy {
            company_reserve_units: 37,
            ..BusinessSalePolicy::for_good(Good::Flour)
        };
        let mut procurement = BusinessProcurementPolicy::none();
        procurement.set_rule(
            Good::Wheat,
            shared::economy::BusinessInputRule {
                enabled: true,
                coverage_days: 2,
                reorder_below: 9,
                target_units: 18,
                maximum_unit_price: 200,
            },
        );
        let mut supply = BusinessSupplyPolicy::none().with_rule(
            Good::Wheat,
            shared::economy::BusinessPrivateInputRule {
                enabled: true,
                ..default()
            },
        );
        let mut staffing = BusinessStaffingPolicy::new(2);

        apply_owner_action(
            HeroBusinessAction::SetOutputReserveDays(99),
            None,
            &mut management,
            &mut wage,
            &mut sale,
            &mut procurement,
            &mut supply,
            &mut staffing,
            2,
        )
        .unwrap();
        assert_eq!(sale.company_reserve_days, 7);
        assert_eq!(sale.company_reserve_units, 37);
        assert!(!management.autopilot);

        management.autopilot = true;
        apply_owner_action(
            HeroBusinessAction::SetInputCoverageDays {
                good: Good::Wheat,
                days: 99,
            },
            None,
            &mut management,
            &mut wage,
            &mut sale,
            &mut procurement,
            &mut supply,
            &mut staffing,
            2,
        )
        .unwrap();
        let rule = procurement.rule(Good::Wheat);
        assert_eq!(rule.coverage_days, 7);
        assert_eq!(rule.target_units, 18);
        assert_eq!(rule.reorder_below, 9);
        assert!(!management.autopilot);
    }

    #[test]
    fn pausing_input_sourcing_pauses_company_and_market_orders_together() {
        let mut management = BusinessManagementPolicy::default();
        let mut wage = BusinessWagePolicy::default();
        let mut sale = BusinessSalePolicy::for_good(Good::Bread);
        let mut procurement = BusinessProcurementPolicy::default();
        let mut supply = BusinessSupplyPolicy::default();
        let mut staffing = BusinessStaffingPolicy::new(2);

        apply_owner_action(
            HeroBusinessAction::SetAutomaticProcurement(false),
            None,
            &mut management,
            &mut wage,
            &mut sale,
            &mut procurement,
            &mut supply,
            &mut staffing,
            2,
        )
        .unwrap();
        assert!(!procurement.automatic);
        assert!(!supply.automatic);
    }

    #[test]
    fn share_purchase_moves_coin_and_whole_shares_without_changing_totals() {
        let seller = PersonId(2);
        let buyer = PersonId(3);
        let mut ownership = CompanyOwnership::sole(seller);
        let mut market = CompanyShareMarket::default();
        assert!(market.list(&ownership, seller, 300, 75, 5));
        let mut seller_wallet = Wallet::new(125);
        let mut buyer_wallet = Wallet::new(10_000);
        let coin_before = seller_wallet.balance() + buyer_wallet.balance();

        let paid = execute_share_purchase(
            &mut ownership,
            &mut market,
            buyer,
            &mut buyer_wallet,
            seller,
            &mut seller_wallet,
            100,
        )
        .unwrap();

        assert_eq!(paid, 7_500);
        assert_eq!(
            seller_wallet.balance() + buyer_wallet.balance(),
            coin_before
        );
        assert_eq!(ownership.share_count(seller), 900);
        assert_eq!(ownership.share_count(buyer), 100);
        assert_eq!(market.offer_from(seller).unwrap().shares, 200);
        assert_eq!(
            ownership
                .shares()
                .iter()
                .map(|holding| u32::from(holding.shares))
                .sum::<u32>(),
            u32::from(COMPANY_TOTAL_SHARES),
        );
    }
}
