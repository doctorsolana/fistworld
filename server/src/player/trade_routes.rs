//! Authoritative player setup for company caravan timetables.
//!
//! The editor sends an ordered draft; this boundary proves executive
//! authority, warehouse/porter capacity and every stop before a durable route
//! asset is created or changed.

use std::collections::HashSet;

use bevy::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::{MessageReceiver, MessageSender, NetworkTarget, RemoteId, Replicate};

use shared::components::{
    BuildingId, BuildingOf, CompanyId, CompanyLeadership, CompanyTradeRoute, Hero, OperatedBy,
    PersonId, SettlementBuilding, SettlementBuildingKind, SettlementId, TradeRouteHistory,
    TradeRouteId, TradeRouteMode, TradeRouteSchedule, TradeRouteStatus, TradeRouteStop,
    TradeRouteStopAction, MAX_TRADE_ROUTE_STOPS,
};
use shared::economy::{BusinessCondition, Good, PENNIES_PER_COIN};
use shared::protocol::{
    HeroTradeRouteAction, HeroTradeRouteOrder, HeroTradeRouteResult, ReliableChannel,
};

use super::hero::OfflineHero;
use crate::world::village::CompanyPorter;

const MAX_ROUTES_PER_COMPANY: usize = 24;
const MAX_ROUTE_UNIT_PRICE: u64 = 1_000 * PENNIES_PER_COIN;

fn validate_schedule(
    stops: &[TradeRouteStop],
    origin: SettlementId,
    known_settlements: &HashSet<SettlementId>,
    storage_settlements: &HashSet<SettlementId>,
) -> Result<(), &'static str> {
    if stops.len() < 2 {
        return Err("A caravan route needs at least two stops.");
    }
    if stops.len() > MAX_TRADE_ROUTE_STOPS {
        return Err("A caravan route may contain at most eight stops.");
    }
    if stops[0].settlement != origin {
        return Err("The first stop must be the route's home settlement.");
    }
    if stops
        .iter()
        .any(|stop| !known_settlements.contains(&stop.settlement))
    {
        return Err("One of those settlements is no longer available.");
    }
    if stops
        .windows(2)
        .any(|pair| pair[0].settlement == pair[1].settlement)
    {
        return Err("Consecutive stops must use different settlements.");
    }
    if stops.iter().any(|stop| stop.action.is_locked_contract()) {
        return Err("Contract pickup and delivery instructions are assigned by the civic buyer.");
    }
    if !stops.iter().any(|stop| stop.action.is_loading()) {
        return Err("Add at least one Load or Buy stop.");
    }
    if !stops.iter().any(|stop| stop.action.is_unloading()) {
        return Err("Add at least one Unload or Sell stop.");
    }
    if stops.iter().any(|stop| {
        matches!(
            stop.action,
            TradeRouteStopAction::Load | TradeRouteStopAction::Unload
        ) && !storage_settlements.contains(&stop.settlement)
    }) {
        return Err("Load and Unload stops require a company Storage Hall in that settlement.");
    }
    Ok(())
}

fn validate_trade_values(
    good: Good,
    cargo_target: u32,
    maximum_purchase_price: u64,
    minimum_destination_price: u64,
) -> Result<u32, &'static str> {
    let capacity = shared::economy::capacity::PORTER / good.bulk_per_unit().max(1);
    if cargo_target == 0 {
        return Err("Cargo target must be at least one unit.");
    }
    if maximum_purchase_price == 0 || maximum_purchase_price > MAX_ROUTE_UNIT_PRICE {
        return Err("Choose a valid maximum purchase price.");
    }
    if minimum_destination_price == 0 || minimum_destination_price > MAX_ROUTE_UNIT_PRICE {
        return Err("Choose a valid minimum sale price.");
    }
    Ok(cargo_target.min(capacity.max(1)))
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn handle_hero_trade_route_orders(
    mut commands: Commands,
    mut links: Query<
        (
            &RemoteId,
            &mut MessageReceiver<HeroTradeRouteOrder>,
            &mut MessageSender<HeroTradeRouteResult>,
        ),
        With<ClientOf>,
    >,
    heroes: Query<(&Hero, &PersonId), Without<OfflineHero>>,
    companies: Query<(&CompanyId, &CompanyLeadership)>,
    settlements: Query<&SettlementId>,
    warehouses: Query<(
        &BuildingId,
        &OperatedBy,
        &BuildingOf,
        &SettlementBuilding,
        Option<&BusinessCondition>,
    )>,
    porters: Query<&CompanyPorter>,
    mut routes: Query<(
        Entity,
        &TradeRouteId,
        &mut CompanyTradeRoute,
        &mut TradeRouteSchedule,
    )>,
) {
    let known_settlements: HashSet<_> = settlements.iter().copied().collect();
    for (remote, mut receiver, mut sender) in links.iter_mut() {
        for order in receiver.receive() {
            let response = (|| -> Result<String, &'static str> {
                let Some((_, person)) = heroes.iter().find(|(hero, ..)| hero.owner == remote.0)
                else {
                    return Err("Create your hero before managing a caravan route.");
                };
                let Some((_, leadership)) = companies
                    .iter()
                    .find(|(company, ..)| **company == order.company)
                else {
                    return Err("That company is unavailable.");
                };
                if !leadership.can_manage(*person) {
                    return Err("Only the appointed Company Master may manage this route.");
                }

                let company_warehouses: Vec<_> = warehouses
                    .iter()
                    .filter(|(_, operated_by, _, building, condition)| {
                        operated_by.0 == order.company
                            && building.kind == SettlementBuildingKind::StorageHall
                            && condition.is_none_or(|condition| condition.state.can_operate())
                    })
                    .collect();
                let storage_settlements: HashSet<_> = company_warehouses
                    .iter()
                    .map(|(_, _, building_of, ..)| building_of.0)
                    .collect();

                match order.action {
                    HeroTradeRouteAction::Create {
                        warehouse,
                        good,
                        cargo_target,
                        maximum_purchase_price,
                        minimum_destination_price,
                        automatic,
                        stops,
                    } => {
                        if routes
                            .iter()
                            .filter(|(_, _, route, _)| route.company == order.company)
                            .count()
                            >= MAX_ROUTES_PER_COMPANY
                        {
                            return Err("This company already operates the maximum of 24 routes.");
                        }
                        let Some((_, _, building_of, ..)) =
                            company_warehouses.iter().find(|(id, ..)| **id == warehouse)
                        else {
                            return Err(
                                "Choose a completed company Storage Hall as the route base.",
                            );
                        };
                        if !porters.iter().any(|porter| {
                            porter.company == order.company && porter.storage_hall == warehouse
                        }) {
                            return Err("That Storage Hall must employ a Company Porter before opening a route.");
                        }
                        validate_schedule(
                            &stops,
                            building_of.0,
                            &known_settlements,
                            &storage_settlements,
                        )?;
                        let cargo_target = validate_trade_values(
                            good,
                            cargo_target,
                            maximum_purchase_price,
                            minimum_destination_price,
                        )?;
                        let destination = stops[1].settlement;
                        let schedule = TradeRouteSchedule::new(stops)
                            .ok_or("Choose between two and eight valid stops.")?;
                        commands.spawn((
                            CompanyTradeRoute {
                                company: order.company,
                                warehouse,
                                mode: TradeRouteMode::Merchant,
                                origin: building_of.0,
                                destination,
                                good,
                                cargo_target,
                                maximum_purchase_price,
                                minimum_destination_price,
                                automatic,
                                active_contract: None,
                                assigned_caravaner: None,
                                current_stop: 0,
                                status: if automatic {
                                    TradeRouteStatus::WaitingForPorter
                                } else {
                                    TradeRouteStatus::Idle
                                },
                                completed_trips: 0,
                                lifetime_units: 0,
                                lifetime_delivery_revenue: 0,
                                lifetime_purchase_cost: 0,
                                lifetime_consigned_value: 0,
                            },
                            schedule,
                            TradeRouteHistory::default(),
                            Replicate::to_clients(NetworkTarget::All),
                        ));
                        Ok("Merchant caravan route created. Its porter will follow the ordered stop list physically.".to_string())
                    }
                    HeroTradeRouteAction::Update {
                        route,
                        good,
                        cargo_target,
                        maximum_purchase_price,
                        minimum_destination_price,
                        automatic,
                        stops,
                    } => {
                        let Some((_, _, mut route_state, mut schedule)) =
                            routes.iter_mut().find(|(_, id, route_state, _)| {
                                **id == route && route_state.company == order.company
                            })
                        else {
                            return Err("That company route is unavailable.");
                        };
                        if route_state.mode != TradeRouteMode::Merchant {
                            return Err(
                                "Buyer-funded contract stops are fixed by the civic order.",
                            );
                        }
                        if route_state.assigned_caravaner.is_some()
                            || !matches!(
                                route_state.status,
                                TradeRouteStatus::Idle | TradeRouteStatus::Mothballed
                            )
                        {
                            return Err(
                                "Wait for the caravan to return before changing its timetable.",
                            );
                        }
                        validate_schedule(
                            &stops,
                            route_state.origin,
                            &known_settlements,
                            &storage_settlements,
                        )?;
                        let cargo_target = validate_trade_values(
                            good,
                            cargo_target,
                            maximum_purchase_price,
                            minimum_destination_price,
                        )?;
                        if !schedule.replace(stops) {
                            return Err("Choose between two and eight valid stops.");
                        }
                        route_state.destination = schedule.stops()[1].settlement;
                        route_state.good = good;
                        route_state.cargo_target = cargo_target;
                        route_state.maximum_purchase_price = maximum_purchase_price;
                        route_state.minimum_destination_price = minimum_destination_price;
                        route_state.automatic = automatic;
                        if route_state.status != TradeRouteStatus::Mothballed {
                            route_state.status = if automatic {
                                TradeRouteStatus::WaitingForPorter
                            } else {
                                TradeRouteStatus::Idle
                            };
                        }
                        Ok("Caravan timetable updated.".to_string())
                    }
                    HeroTradeRouteAction::SetMothballed { route, mothballed } => {
                        let Some((_, _, mut route_state, _)) =
                            routes.iter_mut().find(|(_, id, route_state, _)| {
                                **id == route && route_state.company == order.company
                            })
                        else {
                            return Err("That company route is unavailable.");
                        };
                        if route_state.mode != TradeRouteMode::Merchant {
                            return Err("An active public delivery contract cannot be mothballed by the carrier.");
                        }
                        if route_state.assigned_caravaner.is_some()
                            || !matches!(
                                route_state.status,
                                TradeRouteStatus::Idle | TradeRouteStatus::Mothballed
                            )
                        {
                            return Err(
                                "Wait for the caravan to return before changing its service state.",
                            );
                        }
                        route_state.status = if mothballed {
                            TradeRouteStatus::Mothballed
                        } else {
                            TradeRouteStatus::WaitingForPorter
                        };
                        Ok(if mothballed {
                            "Caravan route mothballed.".to_string()
                        } else {
                            "Caravan route reopened and queued for a porter.".to_string()
                        })
                    }
                    HeroTradeRouteAction::DispatchOnce { route } => {
                        let Some((_, _, mut route_state, _)) =
                            routes.iter_mut().find(|(_, id, route_state, _)| {
                                **id == route && route_state.company == order.company
                            })
                        else {
                            return Err("That company route is unavailable.");
                        };
                        if route_state.mode != TradeRouteMode::Merchant {
                            return Err("Public contracts dispatch automatically once assigned.");
                        }
                        if route_state.status != TradeRouteStatus::Idle
                            || route_state.assigned_caravaner.is_some()
                        {
                            return Err("This caravan is not idle at its home warehouse.");
                        }
                        route_state.status = TradeRouteStatus::WaitingForPorter;
                        Ok("One caravan circuit queued.".to_string())
                    }
                }
            })();
            let (success, message) = match response {
                Ok(message) => (true, message),
                Err(message) => (false, message.to_string()),
            };
            sender.send::<ReliableChannel>(HeroTradeRouteResult { success, message });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: SettlementId = SettlementId(1);
    const AWAY: SettlementId = SettlementId(2);

    fn known() -> HashSet<SettlementId> {
        [HOME, AWAY].into_iter().collect()
    }

    #[test]
    fn merchant_schedule_requires_real_storage_for_private_loads() {
        let stops = [
            TradeRouteStop {
                settlement: HOME,
                action: TradeRouteStopAction::Load,
            },
            TradeRouteStop {
                settlement: AWAY,
                action: TradeRouteStopAction::Sell,
            },
        ];
        assert!(validate_schedule(&stops, HOME, &known(), &HashSet::new()).is_err());
        assert!(validate_schedule(&stops, HOME, &known(), &[HOME].into_iter().collect()).is_ok());
    }

    #[test]
    fn multi_town_schedule_accepts_buy_sell_and_private_unload() {
        let third = SettlementId(3);
        let known: HashSet<_> = [HOME, AWAY, third].into_iter().collect();
        let storage: HashSet<_> = [HOME, third].into_iter().collect();
        let stops = [
            TradeRouteStop {
                settlement: HOME,
                action: TradeRouteStopAction::Buy,
            },
            TradeRouteStop {
                settlement: AWAY,
                action: TradeRouteStopAction::Sell,
            },
            TradeRouteStop {
                settlement: third,
                action: TradeRouteStopAction::Unload,
            },
        ];
        assert!(validate_schedule(&stops, HOME, &known, &storage).is_ok());
    }

    #[test]
    fn one_circuit_may_return_home_for_a_final_unload_or_sale() {
        let stops = [
            TradeRouteStop {
                settlement: HOME,
                action: TradeRouteStopAction::Load,
            },
            TradeRouteStop {
                settlement: AWAY,
                action: TradeRouteStopAction::Buy,
            },
            TradeRouteStop {
                settlement: HOME,
                action: TradeRouteStopAction::Unload,
            },
        ];
        assert!(validate_schedule(&stops, HOME, &known(), &[HOME].into_iter().collect()).is_ok());
    }
}
