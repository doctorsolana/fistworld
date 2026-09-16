//! Company executive authority for ship orders and existing route assets.

use super::hero::OfflineHero;
use crate::world::shipping::routes::{ShipRouteRoutine, validate_stops};
use bevy::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::{MessageReceiver, MessageSender, NetworkTarget, RemoteId, Replicate};
use shared::components::*;
use shared::economy::{BusinessCondition, Good, GoodsInventory, PENNIES_PER_COIN};
use shared::protocol::{
    HeroMaritimeAction, HeroMaritimeOrder, HeroMaritimeResult, ReliableChannel,
};

pub(crate) fn validate_values(
    kind: ShipKind,
    good: Good,
    cargo_target: u32,
    maximum_purchase_price: u64,
    minimum_destination_price: u64,
) -> Result<u32, &'static str> {
    if cargo_target == 0 {
        return Err("Cargo target must be at least one unit.");
    }
    if maximum_purchase_price == 0
        || maximum_purchase_price > 1_000 * PENNIES_PER_COIN
        || minimum_destination_price == 0
        || minimum_destination_price > 1_000 * PENNIES_PER_COIN
    {
        return Err("Choose valid purchase and sale prices.");
    }
    Ok(cargo_target.min(kind.capacity() / good.bulk_per_unit().max(1)))
}

pub(crate) fn execute(
    world: &mut World,
    company: CompanyId,
    action: HeroMaritimeAction,
) -> Result<String, &'static str> {
    match action {
        HeroMaritimeAction::OrderShip { port, kind } => {
            crate::world::ports::order_ship(world, company, port, kind)
        }
        HeroMaritimeAction::CancelShipOrder { order } => {
            crate::world::ports::cancel_ship_order(world, company, order)
        }
        HeroMaritimeAction::CreateRoute {
            ship,
            good,
            cargo_target,
            maximum_purchase_price,
            minimum_destination_price,
            automatic,
            stops,
        } => {
            let (hull_entity, hull) = ship_for(world, company, ship)?;
            if hull.assigned_route.is_some()
                || world.get::<ShipRouteRoutine>(hull_entity).is_some()
                || world
                    .query::<&MaritimeTradeRoute>()
                    .iter(world)
                    .any(|route| route.ship == ship)
            {
                return Err("That ship already has a route.");
            }
            if world
                .get::<GoodsInventory>(hull_entity)
                .is_some_and(|cargo| cargo.used_bulk() > 0)
            {
                return Err("Unload the ship before assigning a different cargo.");
            }
            if world
                .query::<&CompanyTradeRoute>()
                .iter(world)
                .filter(|route| route.company == company)
                .count()
                >= 24
            {
                return Err("This company already operates the maximum of 24 routes.");
            }
            let home = port_home(world, &hull)?;
            let warehouse = world
                .query::<(
                    &BuildingId,
                    &OperatedBy,
                    &BuildingOf,
                    &SettlementBuilding,
                    Option<&BusinessCondition>,
                )>()
                .iter(world)
                .filter(|(_, owner, place, building, condition)| {
                    owner.0 == company
                        && place.0 == home
                        && building.kind == SettlementBuildingKind::StorageHall
                        && condition.is_none_or(|condition| condition.state.can_operate())
                })
                .map(|(id, ..)| *id)
                .min()
                .ok_or(
                    "The home port needs an operating company Storage Hall to employ the captain.",
                )?;
            validate_stops(
                &stops,
                home,
                hull.kind,
                world.query::<&SettlementPort>().iter(world).copied(),
            )?;
            let cargo_target = validate_values(
                hull.kind,
                good,
                cargo_target,
                maximum_purchase_price,
                minimum_destination_price,
            )?;
            let destination = stops[1].settlement;
            let schedule =
                TradeRouteSchedule::new(stops).ok_or("Choose between two and eight port stops.")?;
            world.spawn((
                CompanyTradeRoute {
                    company,
                    warehouse,
                    mode: TradeRouteMode::Merchant,
                    origin: home,
                    destination,
                    good,
                    cargo_target,
                    maximum_purchase_price,
                    minimum_destination_price,
                    automatic,
                    autonomous_management: false,
                    expected_trip_profit: 0,
                    decision_confidence: 100,
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
                MaritimeTradeRoute { ship },
                Replicate::to_clients(NetworkTarget::All),
            ));
            // Stable route IDs are assigned by the ordinary identity pass;
            // duplicate creation before that pass is blocked by the marker scan.
            Ok("Shipping route created. Its captain will buy and consign cargo at the selected town ports.".into())
        }
        HeroMaritimeAction::AssignShip { route, ship } => {
            let (hull_entity, hull) = ship_for(world, company, ship)?;
            if hull
                .assigned_route
                .is_some_and(|assigned| assigned != route)
                || world.get::<ShipRouteRoutine>(hull_entity).is_some()
            {
                return Err("Wait for the ship to finish its current route.");
            }
            let route_entity = world
                .query::<(Entity, &TradeRouteId, &CompanyTradeRoute)>()
                .iter(world)
                .find(|(_, id, state)| **id == route && state.company == company)
                .map(|(entity, ..)| entity)
                .ok_or("That company route is unavailable.")?;
            if world
                .query::<(Entity, &MaritimeTradeRoute)>()
                .iter(world)
                .any(|(entity, assignment)| entity != route_entity && assignment.ship == ship)
            {
                return Err("That ship already belongs to another route.");
            }
            let state = *world.get::<CompanyTradeRoute>(route_entity).unwrap();
            if state.mode != TradeRouteMode::Merchant
                || state.assigned_caravaner.is_some()
                || !matches!(
                    state.status,
                    TradeRouteStatus::Idle | TradeRouteStatus::Mothballed
                )
            {
                return Err("Only an idle merchant route can change transport.");
            }
            if state.origin != port_home(world, &hull)? {
                return Err("The ship and route must share a home settlement.");
            }
            if state.cargo_target > hull.kind.capacity() / state.good.bulk_per_unit().max(1) {
                return Err("Reduce the route cargo target to fit this ship before assigning it.");
            }
            if world
                .get::<GoodsInventory>(hull_entity)
                .is_some_and(|cargo| cargo.used_bulk() > 0)
            {
                return Err("Unload the ship before changing routes.");
            }
            let stops = world
                .get::<TradeRouteSchedule>(route_entity)
                .ok_or("That route has no timetable.")?
                .stops()
                .to_vec();
            validate_stops(
                &stops,
                state.origin,
                hull.kind,
                world.query::<&SettlementPort>().iter(world).copied(),
            )?;
            if let Some(previous) = world.get::<MaritimeTradeRoute>(route_entity).copied() {
                let previous_ship = world
                    .query::<(Entity, &ShipId)>()
                    .iter(world)
                    .find(|(_, id)| **id == previous.ship)
                    .map(|(entity, _)| entity);
                if let Some(previous_ship) = previous_ship {
                    if world.get::<ShipRouteRoutine>(previous_ship).is_some() {
                        return Err("The previous ship must complete its voyage first.");
                    }
                    if let Some(mut old) = world.get_mut::<CompanyShip>(previous_ship) {
                        old.assigned_route = None;
                    }
                }
            }
            world
                .entity_mut(route_entity)
                .insert(MaritimeTradeRoute { ship });
            world
                .get_mut::<CompanyShip>(hull_entity)
                .unwrap()
                .assigned_route = Some(route);
            Ok("Ship assigned to the route.".into())
        }
    }
}

fn port_home(world: &mut World, hull: &CompanyShip) -> Result<SettlementId, &'static str> {
    world
        .query::<(&BuildingId, &SettlementPort)>()
        .iter(world)
        .find(|(id, port)| **id == hull.home_port && port.accepts(hull.kind))
        .map(|(_, port)| port.settlement)
        .ok_or("The ship's home port is unavailable or too small.")
}
fn ship_for(
    world: &mut World,
    company: CompanyId,
    ship: ShipId,
) -> Result<(Entity, CompanyShip), &'static str> {
    world
        .query::<(Entity, &ShipId, &CompanyShip)>()
        .iter(world)
        .find(|(_, id, hull)| **id == ship && hull.company == company)
        .map(|(entity, _, hull)| (entity, *hull))
        .ok_or("That company ship is unavailable.")
}

pub(crate) fn handle_hero_maritime_orders(world: &mut World) {
    let requests: Vec<_> = world.query_filtered::<(Entity, &RemoteId, &mut MessageReceiver<HeroMaritimeOrder>), With<ClientOf>>()
        .iter_mut(world).flat_map(|(entity, remote, mut receiver)| receiver.receive().map(|order| (entity, remote.0, order)).collect::<Vec<_>>()).collect();
    for (link, remote, order) in requests {
        let hero = world
            .query_filtered::<(&Hero, &PersonId), Without<OfflineHero>>()
            .iter(world)
            .find(|(hero, _)| hero.owner == remote)
            .map(|(_, id)| *id);
        let authorized = hero.is_some_and(|person| {
            world
                .query::<(&CompanyId, &CompanyLeadership)>()
                .iter(world)
                .any(|(id, leadership)| *id == order.company && leadership.can_manage(person))
        });
        let result = if authorized {
            execute(world, order.company, order.action)
        } else {
            Err("Only the appointed Company Master may manage this company's ships.")
        };
        let (success, message) = match result {
            Ok(message) => (true, message),
            Err(message) => (false, message.to_string()),
        };
        if let Some(mut sender) = world.get_mut::<MessageSender<HeroMaritimeResult>>(link) {
            sender.send::<ReliableChannel>(HeroMaritimeResult { success, message });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capacity_is_real_hull_bulk_and_prices_cannot_bypass_validation() {
        for kind in ShipKind::ALL {
            for good in Good::ALL {
                assert_eq!(
                    validate_values(kind, good, u32::MAX, 1, 1).unwrap(),
                    kind.capacity() / good.bulk_per_unit()
                );
            }
        }
        assert!(validate_values(ShipKind::Cog, Good::Wood, 1, 0, 1).is_err());
        assert!(validate_values(ShipKind::Cog, Good::Wood, 0, 1, 1).is_err());
    }
}
