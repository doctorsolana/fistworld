//! Player-facing company formation.
//!
//! A productive permit is a company action, so incorporation happens before
//! the first permit rather than being silently backfilled after construction.

use std::collections::HashSet;

use bevy::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::{MessageReceiver, MessageSender, RemoteId};

use shared::components::{Company, CompanyId, Hero, PersonId, PlayerPosition, Settlement};
use shared::economy::{format_money, Wallet, PENNIES_PER_COIN};
use shared::protocol::{HeroCompanyFoundingOrder, HeroCompanyFoundingResult, ReliableChannel};

use super::hero::OfflineHero;

pub const MINIMUM_FOUNDING_CAPITAL: u64 = PENNIES_PER_COIN;
const MAXIMUM_COMPANY_NAME_CHARS: usize = 40;
const MAX_MASTERED_COMPANIES: usize = 32;

fn normalize_company_name(raw: &str) -> Result<String, &'static str> {
    let name = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let length = name.chars().count();
    if length < 3 {
        return Err("Company names must contain at least three characters.");
    }
    if length > MAXIMUM_COMPANY_NAME_CHARS {
        return Err("Company names may contain at most 40 characters.");
    }
    if !name
        .chars()
        .all(|character| character.is_alphanumeric() || matches!(character, ' ' | '&' | '-' | '\''))
    {
        return Err("Use letters, numbers, spaces, &, apostrophes or hyphens in the company name.");
    }
    Ok(name)
}

/// Found a funded, sole-owned company at the Hall. The transfer and company
/// creation occur in one authoritative operation so coin and shares cannot be
/// duplicated by retries or hostile clients.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn handle_hero_company_founding(
    mut commands: Commands,
    mut ids: ResMut<crate::world::identity::WorldIdAllocator>,
    mut links: Query<
        (
            &RemoteId,
            &mut MessageReceiver<HeroCompanyFoundingOrder>,
            &mut MessageSender<HeroCompanyFoundingResult>,
        ),
        With<ClientOf>,
    >,
    mut heroes: Query<(&Hero, &PersonId, &PlayerPosition, &mut Wallet), Without<OfflineHero>>,
    halls: Query<(&Settlement, &PlayerPosition)>,
    companies: Query<(&CompanyId, &Company, &shared::components::CompanyLeadership)>,
    world_time: Query<&shared::components::WorldTime>,
) {
    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    let mut reserved_names: HashSet<String> = companies
        .iter()
        .map(|(_, company, _)| company.name.to_lowercase())
        .collect();
    let mut newly_mastered = std::collections::HashMap::<PersonId, usize>::new();

    for (remote, mut receiver, mut sender) in links.iter_mut() {
        for order in receiver.receive() {
            let response = (|| {
                let Some((_, founder, position, mut wallet)) =
                    heroes.iter_mut().find(|(hero, ..)| hero.owner == remote.0)
                else {
                    return Err("Create your hero before founding a company.".to_string());
                };
                let Ok((settlement, hall_position)) = halls.get(order.hall) else {
                    return Err("Companies must be registered at a settlement Hall.".to_string());
                };
                let distance = Vec2::new(position.0.x, position.0.z)
                    .distance(Vec2::new(hall_position.0.x, hall_position.0.z));
                if distance > super::permits::HERO_PERMIT_INTERACTION_RANGE {
                    return Err(format!(
                        "Move within {:.0}m of {} Hall to found the company.",
                        super::permits::HERO_PERMIT_INTERACTION_RANGE,
                        settlement.name,
                    ));
                }
                let existing = companies
                    .iter()
                    .filter(|(_, _, leadership)| leadership.master == *founder)
                    .count();
                let added = newly_mastered.get(founder).copied().unwrap_or(0);
                if existing.saturating_add(added) >= MAX_MASTERED_COMPANIES {
                    return Err(
                        "Close or transfer an existing dormant company before founding another."
                            .to_string(),
                    );
                }
                if order.initial_capital < MINIMUM_FOUNDING_CAPITAL {
                    return Err(format!(
                        "A company needs at least {} coin of founding capital.",
                        format_money(MINIMUM_FOUNDING_CAPITAL)
                    ));
                }
                let name = normalize_company_name(&order.name).map_err(str::to_string)?;
                let key = name.to_lowercase();
                if reserved_names.contains(&key) {
                    return Err("A company already uses that name.".to_string());
                }
                if !wallet.debit(order.initial_capital) {
                    return Err(format!(
                        "Your wallet holds {} coin; the requested contribution is {}.",
                        format_money(wallet.balance()),
                        format_money(order.initial_capital),
                    ));
                }

                let company = ids.company();
                commands.spawn(crate::world::village::new_company_bundle(
                    company,
                    name.clone(),
                    day,
                    *founder,
                    order.initial_capital,
                    order.initial_capital,
                ));
                reserved_names.insert(key);
                *newly_mastered.entry(*founder).or_default() += 1;
                info!(
                    "Player founded company '{}' (#{}), contributing {} coin",
                    name,
                    company.0,
                    format_money(order.initial_capital),
                );
                Ok((
                    company,
                    format!(
                        "{} founded with {} coin. You hold all 1,000 shares and are Company Master.",
                        name,
                        format_money(order.initial_capital),
                    ),
                ))
            })();

            match response {
                Ok((company, message)) => {
                    sender.send::<ReliableChannel>(HeroCompanyFoundingResult {
                        success: true,
                        company: Some(company),
                        message,
                    })
                }
                Err(message) => sender.send::<ReliableChannel>(HeroCompanyFoundingResult {
                    success: false,
                    company: None,
                    message,
                }),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn company_names_are_trimmed_and_bounded() {
        assert_eq!(
            normalize_company_name("  North   Mill & Sons ").unwrap(),
            "North Mill & Sons"
        );
        assert!(normalize_company_name("x").is_err());
        assert!(normalize_company_name("No/Slashes").is_err());
    }
}
