//! Server-authoritative management of player-owned businesses.
//!
//! Player owners edit the same policies the NPC decision tree reads. The UI is
//! merely a remote control: identity, ownership, operating state and numeric
//! bounds are proved again here for every click.

use std::collections::HashMap;

use bevy::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::{MessageReceiver, MessageSender, RemoteId};

use shared::components::{
    BuildingId, BuildingOf, CompanyId, CompanyLeadership, CompanyOwnership, CompanyShareMarket,
    Hero, OperatedBy, OwnedBy, PersonId, SettlementBuilding, SettlementId, WorldTime,
};
use shared::economy::{
    format_money, BusinessAccount, BusinessCondition, BusinessManagementPolicy,
    BusinessProcurementPolicy, BusinessSalePolicy, BusinessStaffingPolicy, BusinessSupplyPolicy,
    BusinessWagePolicy, CompanyAccount, CompanyBranchPolicies, CompanyManagementPolicy,
    GoodsInventory, MarketSeller, MootMarket, Wallet, MAXIMUM_BUSINESS_DAILY_WAGE,
    MINIMUM_BUSINESS_DAILY_WAGE, PENNIES_PER_COIN,
};
use shared::protocol::{
    HeroBusinessAction, HeroBusinessOrder, HeroBusinessResult, HeroCompanyAction, HeroCompanyOrder,
    HeroCompanyResult, ReliableChannel,
};

use super::hero::OfflineHero;

/// A deliberately generous sanity bound, not a market price control. It keeps
/// hostile packets from filling histories/UI with overflow-adjacent values.
const MAX_MANUAL_UNIT_PRICE: u64 = 1_000 * PENNIES_PER_COIN;

fn execute_share_purchase(
    ownership: &mut CompanyOwnership,
    market: &mut CompanyShareMarket,
    buyer: PersonId,
    buyer_wallet: &mut Wallet,
    seller: PersonId,
    seller_wallet: &mut Wallet,
    shares: u16,
) -> Result<u64, &'static str> {
    if buyer == seller {
        return Err("You cannot buy your own share offer.");
    }
    let Some(offer) = market.offer_from(seller) else {
        return Err("That share offer is no longer available.");
    };
    if shares == 0 || shares > offer.shares {
        return Err("That many shares are not available from this seller.");
    }
    let Some(total_price) = offer.unit_price.checked_mul(u64::from(shares)) else {
        return Err("That share purchase is too large.");
    };
    if !buyer_wallet.can_afford(total_price) {
        return Err("You cannot afford that share purchase.");
    }
    if seller_wallet.balance().checked_add(total_price).is_none() {
        return Err("The seller cannot receive that payment.");
    }
    if !buyer_wallet.debit(total_price) {
        return Err("You cannot afford that share purchase.");
    }
    seller_wallet.credit(total_price);
    if !market.fill(ownership, seller, buyer, shares) {
        let rolled_back = seller_wallet.debit(total_price);
        buyer_wallet.credit(total_price);
        debug_assert!(rolled_back, "share-payment rollback must conserve coin");
        return Err("The cap table changed before the purchase completed; payment was refunded.");
    }
    Ok(total_price)
}

#[allow(clippy::too_many_arguments)]
fn apply_owner_action(
    action: HeroBusinessAction,
    management: &mut BusinessManagementPolicy,
    wage: &mut BusinessWagePolicy,
    sale: &mut BusinessSalePolicy,
    procurement: &mut BusinessProcurementPolicy,
    supply: &mut BusinessSupplyPolicy,
    staffing: &mut BusinessStaffingPolicy,
    maximum_positions: u8,
) -> Result<&'static str, &'static str> {
    match action {
        HeroBusinessAction::AppointCompanyMaster(_)
        | HeroBusinessAction::ListCompanyShares { .. }
        | HeroBusinessAction::CancelCompanyShareListing
        | HeroBusinessAction::BuyCompanyShares { .. } => {
            Err("That company action requires the cap table.")
        }
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
        HeroBusinessAction::SetEnabledPositions(positions) => {
            staffing.enabled_positions = positions.min(maximum_positions);
            management.autopilot = false;
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
            Ok("Company output reserve updated; owner autopilot is paused.")
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
    mut wallets: Query<(Entity, &PersonId, &mut Wallet)>,
    mut dividend_requests: ResMut<crate::world::village::CompanyDividendQueue>,
    mut companies: Query<(
        &CompanyId,
        &mut CompanyOwnership,
        &mut CompanyLeadership,
        &mut CompanyManagementPolicy,
        &mut CompanyShareMarket,
    )>,
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
        Option<&BusinessCondition>,
    )>,
    suppliers: Query<(&BuildingId, &OperatedBy, &SettlementBuilding)>,
    mut halls: Query<(&SettlementId, &mut MootMarket)>,
    world_time: Query<&WorldTime>,
) {
    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    let wallet_entities: HashMap<PersonId, Entity> = wallets
        .iter()
        .map(|(entity, person, _)| (*person, entity))
        .collect();
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
                    condition,
                )) = businesses.get_mut(order.business)
                else {
                    return Err("That building is not an operating business.");
                };
                let company_id = operated_by.map(|company| company.0);
                let company_authorized = company_id.is_some_and(|wanted| {
                    companies
                        .iter()
                        .find(|(id, ..)| **id == wanted)
                        .is_some_and(|(_, _, leadership, ..)| leadership.can_manage(*person_id))
                });
                let governance_authorized = company_id.is_some_and(|wanted| {
                    companies
                        .iter()
                        .find(|(id, ..)| **id == wanted)
                        .is_some_and(|(_, ownership, ..)| ownership.can_appoint_master(*person_id))
                });
                let legacy_authorized =
                    company_id.is_none() && owned_by.is_some_and(|owner| owner.0 == *person_id);
                let appointing =
                    matches!(order.action, HeroBusinessAction::AppointCompanyMaster(_));
                let trading_shares = matches!(
                    order.action,
                    HeroBusinessAction::ListCompanyShares { .. }
                        | HeroBusinessAction::CancelCompanyShareListing
                        | HeroBusinessAction::BuyCompanyShares { .. }
                );
                if appointing && !governance_authorized {
                    return Err(
                        "Appointing a Company Master requires more than 500 of the 1,000 shares.",
                    );
                }
                if !appointing && !trading_shares && !company_authorized && !legacy_authorized {
                    return Err("Only this company's appointed Company Master may change operating decisions.");
                }
                if !appointing
                    && !trading_shares
                    && condition.is_some_and(|condition| !condition.state.can_operate())
                {
                    return Err("A closed or liquidating business cannot change operating policy.");
                }
                let mut market = halls
                    .iter_mut()
                    .find(|(settlement_id, _)| **settlement_id == building_of.0)
                    .map(|(_, market)| market);
                if let HeroBusinessAction::SetPreferredSupplier {
                    good,
                    supplier: Some(supplier),
                } = order.action
                {
                    let Some(company_id) = company_id else {
                        return Err("Private suppliers require a company.");
                    };
                    let valid = suppliers.iter().any(|(id, operation, candidate)| {
                        *id == supplier
                            && operation.0 == company_id
                            && crate::world::village::business_output(candidate.kind) == Some(good)
                    });
                    if !valid {
                        return Err("That site is not an owned supplier of the selected good.");
                    }
                }
                let message = if let HeroBusinessAction::AppointCompanyMaster(candidate) =
                    order.action
                {
                    let Some(company_id) = company_id else {
                        return Err("An independent site has no Company Master office.");
                    };
                    let Some((_, ownership, mut leadership, ..)) =
                        companies.iter_mut().find(|(id, ..)| **id == company_id)
                    else {
                        return Err("That company is unavailable.");
                    };
                    if ownership.share_count(candidate) == 0 {
                        return Err(
                            "The first governance UI may appoint only a current shareholder.",
                        );
                    }
                    leadership.master = candidate;
                    format!("Appointed Person #{} as Company Master.", candidate.0)
                } else if let HeroBusinessAction::ListCompanyShares { shares, unit_price } =
                    order.action
                {
                    let Some(company_id) = company_id else {
                        return Err("An independent site has no company shares.");
                    };
                    if shares == 0 || shares > shared::components::COMPANY_TOTAL_SHARES {
                        return Err("A share offer must contain between 1 and 1,000 shares.");
                    }
                    if unit_price == 0 || unit_price > MAX_MANUAL_UNIT_PRICE {
                        return Err("That per-share asking price is outside the supported range.");
                    }
                    let Some((_, ownership, _, _, mut share_market)) =
                        companies.iter_mut().find(|(id, ..)| **id == company_id)
                    else {
                        return Err("That company is unavailable.");
                    };
                    if !share_market.list(&ownership, *person_id, shares, unit_price, day) {
                        return Err("You cannot offer more whole shares than you currently own.");
                    }
                    format!(
                        "Listed {shares} shares at {} coin each.",
                        format_money(unit_price)
                    )
                } else if order.action == HeroBusinessAction::CancelCompanyShareListing {
                    let Some(company_id) = company_id else {
                        return Err("An independent site has no company shares.");
                    };
                    let Some((_, _, _, _, mut share_market)) =
                        companies.iter_mut().find(|(id, ..)| **id == company_id)
                    else {
                        return Err("That company is unavailable.");
                    };
                    if !share_market.cancel(*person_id) {
                        return Err("You have no active share offer in this company.");
                    }
                    "Cancelled your company share offer.".to_string()
                } else if let HeroBusinessAction::BuyCompanyShares { seller, shares } = order.action
                {
                    let Some(company_id) = company_id else {
                        return Err("An independent site has no company shares.");
                    };
                    let Some((_, mut ownership, mut leadership, _, mut share_market)) =
                        companies.iter_mut().find(|(id, ..)| **id == company_id)
                    else {
                        return Err("That company is unavailable.");
                    };
                    let Some(seller_entity) = wallet_entities.get(&seller).copied() else {
                        return Err("The seller's wallet is unavailable; no coin or shares moved.");
                    };
                    let Ok([(_, _, mut buyer_wallet), (_, _, mut seller_wallet)]) =
                        wallets.get_many_mut([hero_entity, seller_entity])
                    else {
                        return Err("The buyer or seller wallet is unavailable.");
                    };
                    let total_price = execute_share_purchase(
                        &mut ownership,
                        &mut share_market,
                        *person_id,
                        &mut buyer_wallet,
                        seller,
                        &mut seller_wallet,
                        shares,
                    )?;
                    if ownership.share_count(leadership.master) == 0 {
                        if let Some(successor) = ownership.controlling_shareholder() {
                            leadership.master = successor;
                        }
                    }
                    format!(
                        "Bought {shares} shares for {} coin; the cap table still contains exactly 1,000 shares.",
                        format_money(total_price)
                    )
                } else if order.action == HeroBusinessAction::WithdrawAvailableProfit {
                    let Some(company_id) = company_id else {
                        return Err(
                            "That site is not attached to a company treasury yet; no money moved.",
                        );
                    };
                    dividend_requests.request(company_id);
                    "Requested the maximum dividend available after company-wide liabilities and working-capital reserves. It will be distributed to all shareholders by share count.".to_string()
                } else {
                    let message = apply_owner_action(
                        order.action,
                        &mut management,
                        &mut wage,
                        &mut sale,
                        &mut procurement,
                        &mut supply,
                        &mut staffing,
                        building.kind.positions(),
                    )?;
                    if let Some(company_id) = company_id {
                        if let Some((_, _, _, mut company_policy, _)) =
                            companies.iter_mut().find(|(id, ..)| **id == company_id)
                        {
                            match order.action {
                                HeroBusinessAction::SetStrategy(strategy) => {
                                    company_policy.strategy = strategy;
                                    company_policy.payroll_reserve_days =
                                        strategy.payroll_reserve_days();
                                }
                                HeroBusinessAction::SetAutopilot(enabled) => {
                                    company_policy.autopilot = enabled;
                                }
                                HeroBusinessAction::SetAutomaticWithdrawals(enabled) => {
                                    company_policy.automatic_dividends = enabled;
                                }
                                _ => {}
                            }
                        }
                    }
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

/// Apply branch-wide physical stock controls. Unlike a site order this uses a
/// stable CompanyId and SettlementId and therefore remains correct even when a
/// company owns several buildings—or no longer owns the particular building
/// from which its UI was opened.
pub fn handle_hero_company_orders(
    mut links: Query<
        (
            &RemoteId,
            &mut MessageReceiver<HeroCompanyOrder>,
            &mut MessageSender<HeroCompanyResult>,
        ),
        With<ClientOf>,
    >,
    mut heroes: Query<(&Hero, &PersonId, &mut Wallet), Without<OfflineHero>>,
    mut companies: Query<(
        &CompanyId,
        &CompanyLeadership,
        &CompanyOwnership,
        &mut CompanyAccount,
        &mut CompanyBranchPolicies,
        &mut CompanyManagementPolicy,
    )>,
    settlements: Query<&SettlementId>,
    sites: Query<(&OperatedBy, &BuildingOf, &GoodsInventory)>,
) {
    for (remote, mut receiver, mut sender) in links.iter_mut() {
        for order in receiver.receive() {
            let response = (|| {
                let Some((_, person, mut wallet)) =
                    heroes.iter_mut().find(|(hero, ..)| hero.owner == remote.0)
                else {
                    return Err("Create your hero before managing a company.");
                };
                let Some((_, leadership, ownership, mut account, mut branches, mut management)) =
                    companies
                        .iter_mut()
                        .find(|(company, ..)| **company == order.company)
                else {
                    return Err("That company is unavailable.");
                };
                if !leadership.can_manage(*person) {
                    return Err("Only the appointed Company Master may act for this company.");
                }
                if let HeroCompanyAction::ContributeCapital { amount } = order.action {
                    if ownership.share_count(*person) != shared::components::COMPANY_TOTAL_SHARES {
                        return Err("Direct contributions are only available while you own all 1,000 shares; co-owned funding needs a shareholder agreement.");
                    }
                    if amount == 0 {
                        return Err("Choose a positive capital contribution.");
                    }
                    if !wallet.debit(amount) {
                        return Err("Your personal wallet does not hold that much coin.");
                    }
                    account.credit(amount);
                    account.contributed_capital =
                        account.contributed_capital.saturating_add(amount);
                    return Ok(format!(
                        "Added {} coin of capital. Company treasury: {} coin.",
                        format_money(amount),
                        format_money(account.cash),
                    ));
                }
                let settlement = match order.action {
                    HeroCompanyAction::ContributeCapital { .. } => unreachable!(),
                    HeroCompanyAction::SetRetainUnits { settlement, .. }
                    | HeroCompanyAction::SetSellExcess { settlement, .. } => settlement,
                };
                if !settlements.iter().any(|id| *id == settlement)
                    || !sites.iter().any(|(company, building_of, _)| {
                        company.0 == order.company && building_of.0 == settlement
                    })
                {
                    return Err("That company has no local branch in this settlement.");
                }
                let message = match order.action {
                    HeroCompanyAction::ContributeCapital { .. } => unreachable!(),
                    HeroCompanyAction::SetRetainUnits {
                        settlement,
                        good,
                        units,
                    } => {
                        let branch_capacity = sites
                            .iter()
                            .filter(|(company, building_of, _)| {
                                company.0 == order.company && building_of.0 == settlement
                            })
                            .map(|(_, _, inventory)| {
                                inventory.bulk_capacity() / good.bulk_per_unit().max(1)
                            })
                            .fold(0u32, u32::saturating_add);
                        let mut policy = branches.resource(settlement, good);
                        policy.retain_units = units.min(branch_capacity);
                        branches.set_resource(settlement, good, policy);
                        management.autopilot = false;
                        format!(
                            "{} local reserve set to {} units.",
                            good.label(),
                            policy.retain_units
                        )
                    }
                    HeroCompanyAction::SetSellExcess {
                        settlement,
                        good,
                        enabled,
                    } => {
                        let mut policy = branches.resource(settlement, good);
                        policy.sell_excess = enabled;
                        branches.set_resource(settlement, good, policy);
                        format!(
                            "{} excess sales {} for this branch.",
                            good.label(),
                            if enabled { "enabled" } else { "paused" }
                        )
                    }
                };
                Ok(message)
            })();
            let (success, message) = match response {
                Ok(message) => (true, message),
                Err(message) => (false, message.to_string()),
            };
            sender.send::<ReliableChannel>(HeroCompanyResult { success, message });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::components::COMPANY_TOTAL_SHARES;
    use shared::economy::{BusinessStrategy, Good};

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
    fn selecting_strategy_updates_the_parameters_that_autopilot_reads() {
        let mut management = BusinessManagementPolicy::default();
        let mut wage = BusinessWagePolicy::default();
        let mut sale = BusinessSalePolicy::for_good(Good::Flour);
        let mut procurement = BusinessProcurementPolicy::default();
        let mut supply = BusinessSupplyPolicy::default();
        let mut staffing = BusinessStaffingPolicy::new(2);
        apply_owner_action(
            HeroBusinessAction::SetStrategy(BusinessStrategy::Growth),
            &mut management,
            &mut wage,
            &mut sale,
            &mut procurement,
            &mut supply,
            &mut staffing,
            2,
        )
        .unwrap();
        assert_eq!(management.strategy, BusinessStrategy::Growth);
        assert_eq!(management.payroll_reserve_days, 2);
        assert_eq!(sale.target_margin_bps, 750);
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
