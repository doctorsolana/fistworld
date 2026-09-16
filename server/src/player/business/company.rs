//! CompanyId-addressed governance, finance and settlement-branch controls.
//! A workplace is never an authorization gateway for a legal company.

use bevy::{ecs::system::SystemParam, prelude::*};
use lightyear::prelude::{server::ClientOf, MessageReceiver, MessageSender, RemoteId};
use shared::{
    components::{
        BuildingOf, CompanyId, CompanyLeadership, CompanyOwnership, CompanyShareMarket, Hero,
        OperatedBy, PersonId, SettlementId, WorldTime, COMPANY_TOTAL_SHARES,
    },
    economy::{
        format_money, CompanyAccount, CompanyBranchPolicies, CompanyDividendCapacity,
        CompanyManagementPolicy, GoodsInventory, Wallet,
    },
    protocol::{HeroCompanyAction, HeroCompanyOrder, HeroCompanyResult, ReliableChannel},
};

use super::{super::hero::OfflineHero, MAX_MANUAL_UNIT_PRICE};
use crate::world::village::CompanyDividendQueue;

pub(super) fn apply_company_policy_action(
    action: HeroCompanyAction,
    policy: &mut CompanyManagementPolicy,
) {
    match action {
        HeroCompanyAction::SetStrategy(strategy) => {
            policy.strategy = strategy;
            policy.payroll_reserve_days = strategy.payroll_reserve_days();
            // Keep this choice until the Master explicitly restarts executive
            // decisions. Existing review propagates it only to automatic sites.
            policy.autopilot = false;
        }
        HeroCompanyAction::SetAutopilot(enabled) => policy.autopilot = enabled,
        HeroCompanyAction::SetAutomaticDividends(enabled) => {
            policy.automatic_dividends = enabled;
        }
        _ => {}
    }
}

pub(super) fn execute_share_purchase(
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

fn authorize(
    action: HeroCompanyAction,
    person: PersonId,
    leadership: &CompanyLeadership,
    ownership: &CompanyOwnership,
) -> Result<(), &'static str> {
    match action {
        HeroCompanyAction::AppointCompanyMaster(_) if !ownership.can_appoint_master(person) => {
            Err("Appointing a Company Master requires more than 500 of the 1,000 shares.")
        }
        HeroCompanyAction::AppointCompanyMaster(_)
        | HeroCompanyAction::ListCompanyShares { .. }
        | HeroCompanyAction::CancelCompanyShareListing
        | HeroCompanyAction::BuyCompanyShares { .. } => Ok(()),
        _ if leadership.can_manage(person) => Ok(()),
        _ => Err("Only the appointed Company Master may act for this company."),
    }
}

/// Shared by the network handler and focused ECS command tests. Queries touch
/// a selected company; only branch controls survey its physical sites. Wallet
/// lookup for a seller occurs on a share purchase, never on every idle update.
#[derive(SystemParam)]
pub(crate) struct CompanyOrders<'w, 's> {
    companies: Query<
        'w,
        's,
        (
            &'static CompanyId,
            &'static mut CompanyLeadership,
            &'static mut CompanyOwnership,
            &'static mut CompanyAccount,
            &'static mut CompanyBranchPolicies,
            &'static mut CompanyManagementPolicy,
            &'static mut CompanyShareMarket,
            Option<&'static CompanyDividendCapacity>,
        ),
    >,
    wallets: Query<'w, 's, (Entity, &'static PersonId, &'static mut Wallet)>,
    settlements: Query<'w, 's, &'static SettlementId>,
    sites: Query<
        'w,
        's,
        (
            &'static OperatedBy,
            &'static BuildingOf,
            &'static GoodsInventory,
        ),
    >,
    dividends: ResMut<'w, CompanyDividendQueue>,
}

impl CompanyOrders<'_, '_> {
    /// `Ok(None)` means the order was accepted but its result is deferred:
    /// the finance pass pays on the next world tick and
    /// `report_dividend_outcomes` sends the one honest reply, so no immediate
    /// `HeroCompanyResult` may be sent for it.
    fn execute(
        &mut self,
        person: PersonId,
        hero: Entity,
        link: Entity,
        order: HeroCompanyOrder,
        day: u32,
    ) -> Result<Option<String>, &'static str> {
        let Some((
            _,
            mut leadership,
            mut ownership,
            mut account,
            mut branches,
            mut management,
            mut market,
            capacity,
        )) = self
            .companies
            .iter_mut()
            .find(|(id, ..)| **id == order.company)
        else {
            return Err("That company is unavailable.");
        };
        authorize(order.action, person, &leadership, &ownership)?;
        if let HeroCompanyAction::DistributeDividend { pennies } = order.action {
            if pennies == 0 {
                return Err(
                    if capacity.is_some_and(|capacity| capacity.distributable == 0) {
                        "Nothing is distributable right now; choose a positive dividend once retained profit exceeds the company's reserves."
                    } else {
                        "Choose a positive dividend amount."
                    },
                );
            }
            // Amount validation happens against live reserves in the finance
            // pass, which clamps and reports; the replicated snapshot is stale
            // within a day and must not reject a request spuriously.
            self.dividends.request(order.company, pennies, person, link);
            return Ok(None);
        }
        let message: Result<String, &'static str> = match order.action {
            HeroCompanyAction::AppointCompanyMaster(candidate) => {
                if ownership.share_count(candidate) == 0 {
                    return Err("The Company Master must be a current shareholder.");
                }
                leadership.master = candidate;
                Ok(format!(
                    "Appointed Person #{} as Company Master.",
                    candidate.0
                ))
            }
            HeroCompanyAction::ListCompanyShares { shares, unit_price } => {
                if shares == 0 || shares > COMPANY_TOTAL_SHARES {
                    return Err("A share offer must contain between 1 and 1,000 shares.");
                }
                if unit_price == 0 || unit_price > MAX_MANUAL_UNIT_PRICE {
                    return Err("That per-share asking price is outside the supported range.");
                }
                if !market.list(&ownership, person, shares, unit_price, day) {
                    return Err("You cannot offer more whole shares than you currently own.");
                }
                Ok(format!(
                    "Listed {shares} shares at {} coin each.",
                    format_money(unit_price)
                ))
            }
            HeroCompanyAction::CancelCompanyShareListing => {
                if !market.cancel(person) {
                    return Err("You have no active share offer in this company.");
                }
                Ok("Cancelled your company share offer.".into())
            }
            HeroCompanyAction::BuyCompanyShares { seller, shares } => {
                let Some(seller_entity) = self
                    .wallets
                    .iter()
                    .find_map(|(entity, id, _)| (*id == seller).then_some(entity))
                else {
                    return Err("The seller's wallet is unavailable; no coin or shares moved.");
                };
                let Ok([(_, _, mut buyer_wallet), (_, _, mut seller_wallet)]) =
                    self.wallets.get_many_mut([hero, seller_entity])
                else {
                    return Err("The buyer or seller wallet is unavailable.");
                };
                let paid = execute_share_purchase(
                    &mut ownership,
                    &mut market,
                    person,
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
                Ok(format!(
                    "Bought {shares} shares for {} coin; the cap table still contains exactly 1,000 shares.",
                    format_money(paid)
                ))
            }
            HeroCompanyAction::SetStrategy(_)
            | HeroCompanyAction::SetAutopilot(_)
            | HeroCompanyAction::SetAutomaticDividends(_) => {
                apply_company_policy_action(order.action, &mut management);
                Ok(match order.action {
                    HeroCompanyAction::SetStrategy(_) => "Company strategy updated; automatic sites follow this choice and executive strategy review is paused.",
                    HeroCompanyAction::SetAutopilot(true) => "Company executive decisions enabled; manual site overrides remain unchanged.",
                    HeroCompanyAction::SetAutopilot(false) => "Company executive decisions paused; the chosen company strategy is retained.",
                    _ => "Company dividend policy updated.",
                }.into())
            }
            HeroCompanyAction::DistributeDividend { .. } => {
                unreachable!("dividend requests are deferred above")
            }
            HeroCompanyAction::ContributeCapital { amount } => {
                if ownership.share_count(person) != COMPANY_TOTAL_SHARES {
                    return Err(
                        "Direct contributions are only available while you own all 1,000 shares; co-owned funding needs a shareholder agreement.",
                    );
                }
                if amount == 0 {
                    return Err("Choose a positive capital contribution.");
                }
                if account.cash.checked_add(amount).is_none()
                    || account.contributed_capital.checked_add(amount).is_none()
                {
                    return Err("The company cannot receive that capital contribution.");
                }
                let Ok((_, _, mut wallet)) = self.wallets.get_mut(hero) else {
                    return Err("Your personal wallet is unavailable.");
                };
                if !wallet.debit(amount) {
                    return Err("Your personal wallet does not hold that much coin.");
                }
                account.credit(amount);
                account.contributed_capital += amount;
                Ok(format!(
                    "Added {} coin of capital. Company treasury: {} coin.",
                    format_money(amount),
                    format_money(account.cash)
                ))
            }
            HeroCompanyAction::SetRetainUnits {
                settlement,
                good,
                units,
            } => {
                if !self.settlements.iter().any(|id| *id == settlement)
                    || !self
                        .sites
                        .iter()
                        .any(|(company, of, _)| company.0 == order.company && of.0 == settlement)
                {
                    return Err("That company has no local branch in this settlement.");
                }
                let capacity = self
                    .sites
                    .iter()
                    .filter(|(company, of, _)| company.0 == order.company && of.0 == settlement)
                    .map(|(_, _, stock)| stock.bulk_capacity() / good.bulk_per_unit().max(1))
                    .fold(0u32, u32::saturating_add);
                let mut policy = branches.resource(settlement, good);
                policy.retain_units = units.min(capacity);
                branches.set_resource(settlement, good, policy);
                management.autopilot = false;
                Ok(format!(
                    "{} local reserve set to {} units.",
                    good.label(),
                    policy.retain_units
                ))
            }
            HeroCompanyAction::SetSellExcess {
                settlement,
                good,
                enabled,
            } => {
                if !self.settlements.iter().any(|id| *id == settlement)
                    || !self
                        .sites
                        .iter()
                        .any(|(company, of, _)| company.0 == order.company && of.0 == settlement)
                {
                    return Err("That company has no local branch in this settlement.");
                }
                let mut policy = branches.resource(settlement, good);
                policy.sell_excess = enabled;
                branches.set_resource(settlement, good, policy);
                Ok(format!(
                    "{} excess sales {} for this branch.",
                    good.label(),
                    if enabled { "enabled" } else { "paused" }
                ))
            }
        };
        message.map(Some)
    }
}

/// Decode the authenticated actor once, then execute solely by CompanyId.
pub fn handle_hero_company_orders(
    mut links: Query<
        (
            Entity,
            &RemoteId,
            &mut MessageReceiver<HeroCompanyOrder>,
            &mut MessageSender<HeroCompanyResult>,
        ),
        With<ClientOf>,
    >,
    heroes: Query<(Entity, &Hero, &PersonId), Without<OfflineHero>>,
    mut orders: CompanyOrders,
    clocks: Query<&WorldTime>,
) {
    let day = clocks.iter().next().map_or(0, |clock| clock.day);
    for (link, remote, mut receiver, mut sender) in links.iter_mut() {
        for order in receiver.receive() {
            let company = order.company;
            let response = if let Some((hero, _, person)) =
                heroes.iter().find(|(_, hero, _)| hero.owner == remote.0)
            {
                orders.execute(*person, hero, link, order, day)
            } else {
                Err("Create your hero before managing a company.")
            };
            let (success, message) = match response {
                // Deferred: the finance pass answers through
                // `report_dividend_outcomes` one tick later.
                Ok(None) => continue,
                Ok(Some(message)) => (true, message),
                Err(message) => (false, message.into()),
            };
            sender.send::<ReliableChannel>(HeroCompanyResult {
                company,
                success,
                message,
            });
        }
    }
}

#[cfg(test)]
mod tests;
