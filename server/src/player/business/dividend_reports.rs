//! Deferred, honest replies to manual dividend requests.
//!
//! `handle_hero_company_orders` only enqueues a `DistributeDividend` order;
//! the world-tick finance pass pays (or refuses) it and records one
//! [`DividendOutcome`] per request. This ingress system, scheduled right after
//! the order handler, turns each outcome into exactly one `HeroCompanyResult`
//! for the link that asked, so the player reads what was actually paid rather
//! than a canned acknowledgement. A requester who has since disconnected is
//! dropped silently.

use bevy::prelude::*;
use lightyear::prelude::{server::ClientOf, MessageSender};
use shared::{
    economy::{format_money, COMPANY_DIVIDEND_FLOAT},
    protocol::{HeroCompanyResult, ReliableChannel},
};

use crate::world::village::{CompanyDividendOutcomes, DividendOutcome, DividendRefusal};

pub fn report_dividend_outcomes(
    mut outcomes: ResMut<CompanyDividendOutcomes>,
    mut links: Query<&mut MessageSender<HeroCompanyResult>, With<ClientOf>>,
) {
    if outcomes.is_empty() {
        return;
    }
    for outcome in outcomes.drain() {
        let Ok(mut sender) = links.get_mut(outcome.requester_link) else {
            continue;
        };
        sender.send::<ReliableChannel>(HeroCompanyResult {
            company: outcome.company,
            success: outcome.refusal.is_none(),
            message: dividend_report_text(&outcome),
        });
    }
}

/// Player-facing text for one outcome. Amounts are coin with two decimals
/// (`format_money`); per-share is sub-penny, so the rate is quoted per ten
/// shares (1%) and the requester's exact own take is named separately. A
/// payout names how much of it retained profit covered and how much handed
/// contributed capital back; every reply names what the reserve consists of.
pub(crate) fn dividend_report_text(outcome: &DividendOutcome) -> String {
    let held_back = format!(
        "Held back {} coin: wage debt / tax debt / one day of payroll / {} coin float.",
        format_money(outcome.withheld_reserves),
        format_money(COMPANY_DIVIDEND_FLOAT)
    );
    match outcome.refusal {
        None => format!(
            "Paid {} coin to {} shareholder{}: {} coin per 10 shares; your {} shares received {} coin. {} coin of it from retained profit, {} coin a return of capital. {held_back}",
            format_money(outcome.paid),
            outcome.shareholders,
            if outcome.shareholders == 1 { "" } else { "s" },
            format_money(outcome.per_ten_shares),
            outcome.own_shares,
            format_money(outcome.own_take),
            format_money(outcome.from_profit),
            format_money(outcome.return_of_capital),
        ),
        Some(DividendRefusal::NoOperatingSite) => format!(
            "No dividend: the company operates no site with business books to attribute a distribution to. {held_back}"
        ),
        Some(DividendRefusal::ShareholderUnreachable) => format!(
            "No dividend: a shareholder on the cap table has no wallet to receive their share, so nothing was paid to anyone. {held_back}"
        ),
        Some(DividendRefusal::NothingDistributable) => {
            format!("No dividend: nothing is distributable right now. {held_back}")
        }
        Some(DividendRefusal::NothingRequested) => {
            format!("No dividend: no amount was requested. {held_back}")
        }
        Some(DividendRefusal::WalletFull) => format!(
            "No dividend: one shareholder's wallet cannot receive its share, so the whole distribution stays in the treasury. {held_back}"
        ),
        Some(DividendRefusal::TreasuryDebitFailed) => {
            format!("No dividend: the treasury could not be debited. {held_back}")
        }
        Some(DividendRefusal::CompanyUnavailable) => {
            "No dividend: the company no longer exists to review.".to_string()
        }
    }
}

#[cfg(test)]
mod tests;
