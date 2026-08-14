//! Server-authoritative management of player-owned businesses.
//!
//! Player owners edit the same policies the NPC decision tree reads. The UI is
//! merely a remote control: identity, ownership, operating state and numeric
//! bounds are proved again here for every click.

use bevy::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::{MessageReceiver, MessageSender, RemoteId};

use shared::components::{
    BuildingId, BuildingOf, Hero, OwnedBy, PersonId, SettlementBuilding, SettlementId, WorldTime,
};
use shared::economy::{
    business_working_capital, format_money, BusinessAccount, BusinessCondition,
    BusinessManagementPolicy, BusinessProcurementPolicy, BusinessSalePolicy, BusinessWagePolicy,
    MarketSeller, MootMarket, Wallet, MAXIMUM_BUSINESS_DAILY_WAGE, MINIMUM_BUSINESS_DAILY_WAGE,
    PENNIES_PER_COIN,
};
use shared::protocol::{
    HeroBusinessAction, HeroBusinessOrder, HeroBusinessResult, ReliableChannel,
};

use super::hero::OfflineHero;

/// A deliberately generous sanity bound, not a market price control. It keeps
/// hostile packets from filling histories/UI with overflow-adjacent values.
const MAX_MANUAL_UNIT_PRICE: u64 = 1_000 * PENNIES_PER_COIN;

#[allow(clippy::too_many_arguments)]
fn apply_owner_action(
    action: HeroBusinessAction,
    management: &mut BusinessManagementPolicy,
    wage: &mut BusinessWagePolicy,
    sale: &mut BusinessSalePolicy,
    procurement: &mut BusinessProcurementPolicy,
) -> Result<&'static str, &'static str> {
    match action {
        HeroBusinessAction::SetStrategy(strategy) => {
            management.strategy = strategy;
            management.payroll_reserve_days = strategy.payroll_reserve_days();
            sale.target_margin_bps = strategy.target_margin_bps();
            sale.max_daily_price_change_bps = strategy.daily_price_step_bps();
            Ok("Business strategy updated.")
        }
        HeroBusinessAction::SetAutopilot(enabled) => {
            management.autopilot = enabled;
            Ok(if enabled {
                "Owner autopilot enabled."
            } else {
                "Owner autopilot paused; your policy values will be retained."
            })
        }
        HeroBusinessAction::SetAutomaticWithdrawals(enabled) => {
            management.automatic_withdrawals = enabled;
            Ok("Profit-withdrawal policy updated.")
        }
        HeroBusinessAction::WithdrawAvailableProfit => {
            Err("Profit withdrawal requires the business account.")
        }
        HeroBusinessAction::SetDailyWage(value) => {
            wage.daily_wage = value.clamp(MINIMUM_BUSINESS_DAILY_WAGE, MAXIMUM_BUSINESS_DAILY_WAGE);
            wage.automatic = false;
            Ok("Daily wage set; automatic wage review is off.")
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
        HeroBusinessAction::SetAutomaticProcurement(enabled) => {
            procurement.automatic = enabled;
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
    }
}

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
    heroes: Query<(Entity, &Hero, &PersonId), Without<OfflineHero>>,
    mut hero_wallets: Query<&mut Wallet, With<Hero>>,
    mut businesses: Query<(
        &OwnedBy,
        &SettlementBuilding,
        &BuildingId,
        &BuildingOf,
        &mut BusinessAccount,
        &mut BusinessManagementPolicy,
        &mut BusinessWagePolicy,
        &mut BusinessSalePolicy,
        &mut BusinessProcurementPolicy,
        Option<&BusinessCondition>,
    )>,
    mut halls: Query<(&SettlementId, &mut MootMarket)>,
    world_time: Query<&WorldTime>,
) {
    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    for (remote, mut receiver, mut sender) in links.iter_mut() {
        for order in receiver.receive() {
            let response = (|| {
                if order.business == Entity::PLACEHOLDER {
                    return Err("That business is unavailable.");
                }
                let Some((hero_entity, _, person_id)) =
                    heroes.iter().find(|(_, hero, _)| hero.owner == remote.0)
                else {
                    return Err("Create your hero before managing a business.");
                };
                let Ok((
                    owned_by,
                    building,
                    building_id,
                    building_of,
                    mut account,
                    mut management,
                    mut wage,
                    mut sale,
                    mut procurement,
                    condition,
                )) = businesses.get_mut(order.business)
                else {
                    return Err("That building is not an operating business.");
                };
                if owned_by.0 != *person_id {
                    return Err("You do not own that business.");
                }
                if condition.is_some_and(|condition| !condition.state.can_operate()) {
                    return Err("A closed or liquidating business cannot change operating policy.");
                }
                let mut market = halls
                    .iter_mut()
                    .find(|(settlement_id, _)| **settlement_id == building_of.0)
                    .map(|(_, market)| market);
                let message = if order.action == HeroBusinessAction::WithdrawAvailableProfit {
                    // Prove the destination before debiting the firm. A malformed
                    // hero entity must never make business cash disappear.
                    let Ok(mut wallet) = hero_wallets.get_mut(hero_entity) else {
                        return Err("Your hero wallet is unavailable.");
                    };
                    let reserve = business_working_capital(
                        building.kind.positions(),
                        &wage,
                        &management,
                        &procurement,
                        market.as_deref(),
                    )
                    .total();
                    let withdrawn = account.withdraw_owner(day, u64::MAX, reserve);
                    if withdrawn == 0 {
                        "No profit is currently available above payroll, input, liability and operating reserves.".to_string()
                    } else {
                        wallet.credit(withdrawn);
                        format!(
                            "Withdrew {} coin of protected profit.",
                            format_money(withdrawn)
                        )
                    }
                } else {
                    let message = apply_owner_action(
                        order.action,
                        &mut management,
                        &mut wage,
                        &mut sale,
                        &mut procurement,
                    )?;
                    // Reprice stock already at the Hall immediately. Otherwise
                    // a valid manual ask looks ignored until the next day.
                    if matches!(order.action, HeroBusinessAction::SetAskingPrice(_)) {
                        if let (Some(market), Some(good)) = (
                            market.as_deref_mut(),
                            crate::world::village::business_output(building.kind),
                        ) {
                            market.reprice(
                                MarketSeller::Business(*building_id),
                                good,
                                sale.asking_unit_price,
                            );
                        }
                    }
                    message.to_string()
                };
                Ok(format!("{}: {message}", building.kind.label()))
            })();
            let (success, message) = match response {
                Ok(message) => (true, message),
                Err(message) => (false, message.to_string()),
            };
            sender.send::<ReliableChannel>(HeroBusinessResult { success, message });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::economy::{BusinessStrategy, Good};

    #[test]
    fn manual_values_disable_only_their_own_automation() {
        let mut management = BusinessManagementPolicy::default();
        let mut wage = BusinessWagePolicy::default();
        let mut sale = BusinessSalePolicy::for_good(Good::Flour);
        let mut procurement = BusinessProcurementPolicy::default();
        apply_owner_action(
            HeroBusinessAction::SetDailyWage(275),
            &mut management,
            &mut wage,
            &mut sale,
            &mut procurement,
        )
        .unwrap();
        assert_eq!(wage.daily_wage, 275);
        assert!(!wage.automatic);
        assert!(sale.automatic_pricing);
        assert!(management.autopilot);
    }

    #[test]
    fn selecting_strategy_updates_the_parameters_that_autopilot_reads() {
        let mut management = BusinessManagementPolicy::default();
        let mut wage = BusinessWagePolicy::default();
        let mut sale = BusinessSalePolicy::for_good(Good::Flour);
        let mut procurement = BusinessProcurementPolicy::default();
        apply_owner_action(
            HeroBusinessAction::SetStrategy(BusinessStrategy::Growth),
            &mut management,
            &mut wage,
            &mut sale,
            &mut procurement,
        )
        .unwrap();
        assert_eq!(management.strategy, BusinessStrategy::Growth);
        assert_eq!(management.payroll_reserve_days, 2);
        assert_eq!(sale.target_margin_bps, 750);
    }
}
