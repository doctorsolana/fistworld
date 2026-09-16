//! Ordinary company timetables executed by a real hull and employed captain.
//! A completed berth is another access point to its town's existing market.

use super::{
    crew,
    traffic::{self, ChannelDirection},
};
use crate::player::boat::{VesselGoal, VesselNavigationQueue, VesselRoute, VesselRouteFailed};
use crate::world::simulation_time::SimulationDelta;
use crate::world::village::BusinessEventQueue;
use bevy::prelude::*;
use shared::components::*;
use shared::economy::{BusinessAccount, CompanyAccount, GoodsInventory, MarketSeller, MootMarket};

/// Reservation lasts until the previous hull physically clears its departure
/// point, including paused/loading ships. One public pier serves one hull.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct PortBerthReservation(pub ShipId);

#[derive(Component)]
pub(crate) struct StopAfterVoyage;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    AtStop,
    PlanningDeparture,
    Departing,
    Sailing,
    ApproachingBerth,
    Home,
}

#[derive(Component, Clone, Debug)]
pub(crate) struct ShipRouteRoutine {
    route: Entity,
    port: Entity,
    next_port: Entity,
    stop: usize,
    returning: bool,
    phase: Phase,
    departed_day: u32,
    started_at: f64,
    purchased_units: u32,
    purchase_cost: u64,
    market_fees: u64,
    consigned_value: u64,
    stops_visited: u8,
    retry_in: f32,
    departure_goal: Option<Vec3>,
    holding_goal: Option<Vec3>,
}

#[derive(Resource, Default)]
struct ShippingCadence(f32);

pub(crate) fn validate_stops(
    stops: &[TradeRouteStop],
    home: SettlementId,
    kind: ShipKind,
    ports: impl Iterator<Item = SettlementPort>,
) -> Result<(), &'static str> {
    if !(2..=MAX_TRADE_ROUTE_STOPS).contains(&stops.len()) {
        return Err("Choose between two and eight port stops.");
    }
    if stops[0].settlement != home {
        return Err("The first stop must be the ship's home port.");
    }
    if stops
        .windows(2)
        .any(|pair| pair[0].settlement == pair[1].settlement)
    {
        return Err("Consecutive stops must use different ports.");
    }
    if stops.iter().any(|stop| {
        !matches!(
            stop.action,
            TradeRouteStopAction::Buy | TradeRouteStopAction::Sell
        )
    }) {
        return Err(
            "Ships buy and sell at town ports; private warehouse Load/Unload stops require a land caravan.",
        );
    }
    if !stops
        .iter()
        .any(|stop| stop.action == TradeRouteStopAction::Buy)
        || !stops
            .iter()
            .any(|stop| stop.action == TradeRouteStopAction::Sell)
    {
        return Err("Add at least one Buy stop and one Sell stop.");
    }
    let accessible: Vec<_> = ports
        .filter(|port| port.accepts(kind))
        .map(|port| port.settlement)
        .collect();
    if stops
        .iter()
        .any(|stop| !accessible.contains(&stop.settlement))
    {
        return Err("Every stop needs a completed port large enough for this ship.");
    }
    Ok(())
}

fn status(world: &mut World, ship: Entity, value: ShipStatus) {
    if let Some(mut state) = world.get_mut::<CompanyShip>(ship) {
        if state.status != value {
            state.status = value;
        }
    }
}
fn at(world: &World, ship: Entity, point: Vec3) -> bool {
    world
        .get::<PlayerPosition>(ship)
        .is_some_and(|position| position.0.xz().distance_squared(point.xz()) <= 0.35 * 0.35)
}
pub(crate) fn berth_available(world: &World, port: Entity, ship: Option<ShipId>) -> bool {
    world
        .get::<PortBerthReservation>(port)
        .is_none_or(|held| Some(held.0) == ship)
}

fn reserve(world: &mut World, port: Entity, ship: ShipId) -> bool {
    match world.get::<PortBerthReservation>(port) {
        Some(occupant) => occupant.0 == ship,
        None => {
            world.entity_mut(port).insert(PortBerthReservation(ship));
            true
        }
    }
}
fn release_berth(world: &mut World, port: Entity, ship: ShipId) {
    if world
        .get::<PortBerthReservation>(port)
        .is_some_and(|held| held.0 == ship)
    {
        world.entity_mut(port).remove::<PortBerthReservation>();
    }
}
fn sail(
    world: &mut World,
    ship: Entity,
    goal: Vec3,
    routine: &mut ShipRouteRoutine,
    dt: f32,
) -> bool {
    if at(world, ship, goal) {
        return true;
    }
    routine.retry_in = (routine.retry_in - dt).max(0.0);
    let pending = world
        .get_resource::<VesselNavigationQueue>()
        .is_some_and(|queue| queue.is_pending(ship));
    if world.get::<VesselRoute>(ship).is_none() && !pending && routine.retry_in <= 0.0 {
        if world
            .get::<VesselRouteFailed>(ship)
            .is_some_and(|failed| failed.goal.distance_squared(goal.xz()) < 0.01)
        {
            status(world, ship, ShipStatus::Blocked);
        }
        world.entity_mut(ship).remove::<VesselRouteFailed>();
        world
            .resource_mut::<VesselNavigationQueue>()
            .request(ship, VesselGoal::Sail(goal.xz()));
        routine.retry_in = 5.0;
    }
    false
}

fn port_for(world: &mut World, settlement: SettlementId, kind: ShipKind) -> Option<Entity> {
    world
        .query::<(Entity, &BuildingId, &SettlementPort)>()
        .iter(world)
        .filter(|(_, _, port)| port.settlement == settlement && port.accepts(kind))
        .min_by_key(|(_, id, _)| **id)
        .map(|(entity, _, _)| entity)
}
fn home_port(world: &mut World, ship: &CompanyShip) -> Option<Entity> {
    world
        .query::<(Entity, &BuildingId, &SettlementPort)>()
        .iter(world)
        .find(|(_, id, port)| **id == ship.home_port && port.accepts(ship.kind))
        .map(|(entity, _, _)| entity)
}

/// Returns true when the physical stop is finished. Unavailable goods or
/// capacity retain the hull and cargo at this berth for a later retry.
fn transact(
    world: &mut World,
    ship: Entity,
    route: &mut CompanyTradeRoute,
    stop: TradeRouteStop,
    routine: &mut ShipRouteRoutine,
    day: u32,
) -> bool {
    let Some(port) = world.get::<SettlementPort>(routine.port) else {
        return false;
    };
    if !port.built || port.settlement != stop.settlement || !at(world, ship, port.geometry.berth) {
        return false;
    }
    let Some(hall) = world
        .query_filtered::<(Entity, &SettlementId), With<MootMarket>>()
        .iter(world)
        .find(|(_, id)| **id == stop.settlement)
        .map(|(entity, _)| entity)
    else {
        return false;
    };
    if world
        .get::<MootMarket>(hall)
        .is_none_or(|market| !market.supports_regional_trade())
    {
        return false;
    }
    let Some(warehouse) = world
        .query::<(Entity, &BuildingId, &OperatedBy)>()
        .iter(world)
        .find(|(_, id, owner)| **id == route.warehouse && owner.0 == route.company)
        .map(|(entity, _, _)| entity)
    else {
        return false;
    };
    if world.get::<BusinessAccount>(warehouse).is_none() {
        return false;
    }
    match stop.action {
        TradeRouteStopAction::Buy => {
            let Some(company) = world
                .query::<(Entity, &CompanyId)>()
                .iter(world)
                .find(|(_, id)| **id == route.company)
                .map(|(entity, _)| entity)
            else {
                return false;
            };
            let Some(account) = world.get::<CompanyAccount>(company) else {
                return false;
            };
            let budget = account.cash;
            let Some(cargo) = world.get::<GoodsInventory>(ship) else {
                return false;
            };
            let wanted = route
                .cargo_target
                .saturating_sub(cargo.amount(route.good))
                .min(cargo.free_bulk_for(route.good) / route.good.bulk_per_unit().max(1));
            if wanted == 0 {
                return true;
            }
            let requested = wanted.min(
                world
                    .get::<GoodsInventory>(hall)
                    .map_or(0, |store| store.amount(route.good)),
            );
            let purchase = world
                .get_mut::<MootMarket>(hall)
                .unwrap()
                .purchase_for_resale(
                    route.good,
                    requested,
                    budget,
                    Some(route.maximum_purchase_price),
                    Some(MarketSeller::Business(route.warehouse)),
                );
            if purchase.trade.units == 0 {
                status(world, ship, ShipStatus::WaitingForCargo);
                return false;
            }
            // The exclusive transaction holds the verified budget, real stock
            // and cargo space until this entire settlement finishes.
            assert!(
                world
                    .get_mut::<CompanyAccount>(company)
                    .unwrap()
                    .debit(purchase.trade.pennies)
            );
            let mut stores = world.query::<&mut GoodsInventory>();
            let [mut source, mut cargo] = stores.get_many_mut(world, [hall, ship]).unwrap();
            let moved = source.transfer_to(&mut cargo, route.good, purchase.trade.units);
            assert_eq!(moved, purchase.trade.units);
            let fees = purchase
                .fills
                .iter()
                .map(|fill| fill.market_fee)
                .fold(0u64, u64::saturating_add);
            routine.purchased_units = routine.purchased_units.saturating_add(moved);
            routine.purchase_cost = routine.purchase_cost.saturating_add(purchase.trade.pennies);
            routine.market_fees = routine.market_fees.saturating_add(fees);
            route.lifetime_purchase_cost = route
                .lifetime_purchase_cost
                .saturating_add(purchase.trade.pennies);
            world
                .get_mut::<BusinessAccount>(warehouse)
                .unwrap()
                .record_input_purchase(day, purchase.trade.pennies, moved);
            world
                .resource_mut::<BusinessEventQueue>()
                .record_market_purchase(day, stop.settlement, purchase.fills);
            true // A real affordable partial load may depart; no authored quota.
        }
        TradeRouteStopAction::Sell => {
            let carried = world
                .get::<GoodsInventory>(ship)
                .map_or(0, |cargo| cargo.amount(route.good));
            if carried == 0 {
                return true;
            }
            let mut stores = world.query::<&mut GoodsInventory>();
            let Ok([mut cargo, mut stock]) = stores.get_many_mut(world, [ship, hall]) else {
                return false;
            };
            let moved = cargo.transfer_to(&mut stock, route.good, carried);
            world.get_mut::<MootMarket>(hall).unwrap().consign(
                MarketSeller::Business(route.warehouse),
                route.good,
                moved,
                route.minimum_destination_price,
            );
            let value = u64::from(moved).saturating_mul(route.minimum_destination_price);
            routine.consigned_value = routine.consigned_value.saturating_add(value);
            route.lifetime_consigned_value = route.lifetime_consigned_value.saturating_add(value);
            // Consignment is titled stock, not earned cash. Ordinary later
            // town purchases pay the warehouse ledger and public market fee.
            moved == carried
        }
        _ => false,
    }
}

pub(crate) fn advance_shipping(world: &mut World) {
    let dt = world
        .get_resource::<SimulationDelta>()
        .copied()
        .unwrap_or_default()
        .world_seconds();
    world.init_resource::<ShippingCadence>();
    let elapsed = {
        let mut cadence = world.resource_mut::<ShippingCadence>();
        cadence.0 += dt;
        if cadence.0 < 0.25 {
            return;
        }
        std::mem::take(&mut cadence.0)
    };
    let (day, now) = world
        .query::<&WorldTime>()
        .iter(world)
        .next()
        .map_or((0, 0.0), |clock| {
            (
                clock.day,
                f64::from(clock.day) * f64::from(clock.cycle_duration())
                    + f64::from(clock.seconds_in_cycle),
            )
        });
    let routes: Vec<_> = world
        .query::<(
            Entity,
            &TradeRouteId,
            &CompanyTradeRoute,
            &MaritimeTradeRoute,
        )>()
        .iter(world)
        .map(|(entity, id, route, ship)| (entity, *id, *route, ship.ship))
        .collect();
    let hulls: std::collections::HashMap<_, _> = world
        .query::<(Entity, &ShipId, &CompanyShip)>()
        .iter(world)
        .map(|(entity, id, hull)| (*id, (entity, *hull)))
        .collect();
    let stale_berths: Vec<_> = world
        .query::<(Entity, &PortBerthReservation)>()
        .iter(world)
        .filter(|(_, held)| !hulls.contains_key(&held.0))
        .map(|(entity, _)| entity)
        .collect();
    for port in stale_berths {
        world.entity_mut(port).remove::<PortBerthReservation>();
    }
    for (entity, id, mut route, ship_id) in routes {
        let Some((ship, hull)) = hulls
            .get(&ship_id)
            .copied()
            .filter(|(_, hull)| hull.company == route.company)
        else {
            continue;
        };
        if hull.assigned_route != Some(id) {
            world.get_mut::<CompanyShip>(ship).unwrap().assigned_route = Some(id);
        }
        if world.get::<ShipRouteRoutine>(ship).is_none() {
            if matches!(
                route.status,
                TradeRouteStatus::Idle | TradeRouteStatus::Mothballed
            ) {
                continue;
            }
            let Some(port) = home_port(world, &hull) else {
                status(world, ship, ShipStatus::Blocked);
                continue;
            };
            let port_state = *world.get::<SettlementPort>(port).unwrap();
            if !at(world, ship, port_state.geometry.berth) || !reserve(world, port, ship_id) {
                status(world, ship, ShipStatus::WaitingForBerth);
                continue;
            }
            world.entity_mut(ship).insert(ShipRouteRoutine {
                route: entity,
                port,
                next_port: port,
                stop: 0,
                returning: false,
                phase: Phase::AtStop,
                departed_day: day,
                started_at: now,
                purchased_units: 0,
                purchase_cost: 0,
                market_fees: 0,
                consigned_value: 0,
                stops_visited: 0,
                retry_in: 0.0,
                departure_goal: None,
                holding_goal: None,
            });
        }
        let mut routine = world.get::<ShipRouteRoutine>(ship).unwrap().clone();
        if routine.route != entity {
            continue;
        }
        let Some(port) = world.get::<SettlementPort>(routine.port).copied() else {
            status(world, ship, ShipStatus::Blocked);
            continue;
        };
        match routine.phase {
            Phase::AtStop => {
                if !port.accepts(hull.kind)
                    || !at(world, ship, port.geometry.berth)
                    || !reserve(world, routine.port, ship_id)
                {
                    status(world, ship, ShipStatus::Blocked);
                    continue;
                }
                world
                    .entity_mut(ship)
                    .insert(PlayerRotation(port.geometry.yaw));
                if world.get::<StopAfterVoyage>(entity).is_some()
                    && world
                        .get::<GoodsInventory>(ship)
                        .is_none_or(GoodsInventory::is_empty)
                {
                    routine.stop = world
                        .get::<TradeRouteSchedule>(entity)
                        .map_or(0, |schedule| schedule.stops().len());
                    routine.phase = Phase::PlanningDeparture;
                    world.entity_mut(ship).insert(routine);
                    continue;
                }
                routine.retry_in = (routine.retry_in - elapsed).max(0.0);
                if routine.retry_in > 0.0 {
                    world.entity_mut(ship).insert(routine);
                    continue;
                }
                if !crew::acquire_crew(world, ship, route.company, route.warehouse, &port) {
                    routine.retry_in = 5.0;
                    status(world, ship, ShipStatus::WaitingForCrew);
                    world.entity_mut(ship).insert(routine);
                    continue;
                }
                routine.retry_in = 0.0;
                route.assigned_caravaner = crew::assigned_person(world, ship);
                let Some(schedule) = world.get::<TradeRouteSchedule>(entity) else {
                    continue;
                };
                let Some(stop) = schedule.stops().get(routine.stop).copied() else {
                    continue;
                };
                status(
                    world,
                    ship,
                    if stop.action.is_loading() {
                        ShipStatus::Loading
                    } else {
                        ShipStatus::Unloading
                    },
                );
                route.status = TradeRouteStatus::Loading;
                route.current_stop = routine.stop as u8;
                if transact(world, ship, &mut route, stop, &mut routine, day) {
                    routine.stops_visited = routine.stops_visited.saturating_add(1);
                    routine.stop += 1;
                    routine.phase = Phase::PlanningDeparture;
                } else {
                    routine.retry_in = 2.0;
                }
            }
            Phase::PlanningDeparture => {
                let next = world
                    .get::<TradeRouteSchedule>(entity)
                    .and_then(|schedule| schedule.stops().get(routine.stop))
                    .copied();
                routine.returning = next.is_none();
                let destination = if let Some(next) = next {
                    port_for(world, next.settlement, hull.kind)
                } else {
                    home_port(world, &hull)
                };
                if let Some(destination) = destination {
                    routine.next_port = destination;
                    if routine.returning && destination == routine.port {
                        routine.phase = Phase::Home;
                    } else {
                        routine.retry_in = (routine.retry_in - elapsed).max(0.0);
                        if routine.retry_in <= 0.0 {
                            routine.holding_goal = traffic::holding_point(world, destination, ship);
                            routine.departure_goal =
                                traffic::departure_clear_point(world, routine.port, ship);
                            if routine.holding_goal.is_some() && routine.departure_goal.is_some() {
                                routine.phase = Phase::Departing;
                            } else {
                                status(world, ship, ShipStatus::WaitingForBerth);
                                routine.retry_in = 5.0;
                            }
                        }
                    }
                } else {
                    status(world, ship, ShipStatus::Blocked);
                }
            }
            Phase::Departing => {
                // No new leg without a fed living captain, but never strand a
                // hull midway through an already accepted safe water leg.
                let at_berth = at(world, ship, port.geometry.berth);
                if at_berth && !crew::crew_ready(world, ship) {
                    // Waiting for a clear channel may last past another meal.
                    // Replenish at this real market access point, or finish a
                    // requested captain's physical exit before hiring again.
                    routine.retry_in = (routine.retry_in - elapsed).max(0.0);
                    if routine.retry_in <= 0.0 {
                        let ready =
                            crew::acquire_crew(world, ship, route.company, route.warehouse, &port);
                        routine.retry_in = if ready { 0.0 } else { 5.0 };
                    }
                }
                if at_berth && !crew::crew_ready(world, ship) {
                    status(world, ship, ShipStatus::WaitingForCrew);
                } else if !traffic::acquire_channel(
                    world,
                    routine.port,
                    ship,
                    ChannelDirection::Departure,
                ) {
                    status(world, ship, ShipStatus::WaitingForBerth);
                } else if routine.departure_goal.is_some_and(|goal| {
                    route.assigned_caravaner = crew::assigned_person(world, ship);
                    sail(world, ship, goal, &mut routine, elapsed)
                }) {
                    release_berth(world, routine.port, ship_id);
                    traffic::release_channel(world, routine.port, ship_id);
                    traffic::release_holding(world, routine.port, ship_id);
                    routine.port = routine.next_port;
                    routine.phase = Phase::Sailing;
                }
            }
            Phase::Sailing => {
                route.status = if routine.returning {
                    TradeRouteStatus::Returning
                } else {
                    TradeRouteStatus::InTransit
                };
                status(world, ship, ShipStatus::Sailing);
                if routine
                    .holding_goal
                    .is_some_and(|goal| sail(world, ship, goal, &mut routine, elapsed))
                {
                    if traffic::acquire_channel(
                        world,
                        routine.port,
                        ship,
                        ChannelDirection::Arrival,
                    ) && reserve(world, routine.port, ship_id)
                    {
                        routine.phase = Phase::ApproachingBerth;
                    } else {
                        status(world, ship, ShipStatus::WaitingForBerth);
                    }
                }
            }
            Phase::ApproachingBerth => {
                if !reserve(world, routine.port, ship_id) {
                    status(world, ship, ShipStatus::WaitingForBerth);
                } else if sail(world, ship, port.geometry.berth, &mut routine, elapsed) {
                    traffic::release_channel(world, routine.port, ship_id);
                    traffic::release_holding(world, routine.port, ship_id);
                    routine.phase = if routine.returning {
                        Phase::Home
                    } else {
                        Phase::AtStop
                    };
                }
            }
            Phase::Home => {
                if routine.stops_visited > 0 {
                    route.completed_trips = route.completed_trips.saturating_add(1);
                    route.lifetime_units =
                        route.lifetime_units.saturating_add(routine.purchased_units);
                    if let Some(mut history) = world.get_mut::<TradeRouteHistory>(entity) {
                        history.record(TradeRouteTrip {
                            departed_day: routine.departed_day,
                            completed_day: day,
                            units: routine.purchased_units,
                            source_purchase_cost: routine.purchase_cost,
                            source_market_fees: routine.market_fees,
                            delivery_revenue: 0,
                            consigned_value: routine.consigned_value,
                            stops_visited: routine.stops_visited,
                            travel_world_seconds: (now - routine.started_at)
                                .max(0.0)
                                .min(f64::from(u32::MAX))
                                as u32,
                        });
                    }
                }
                route.current_stop = 0;
                route.assigned_caravaner = None;
                let stop_requested = world.get::<StopAfterVoyage>(entity).is_some();
                route.status = if stop_requested {
                    TradeRouteStatus::Mothballed
                } else if route.automatic {
                    TradeRouteStatus::WaitingForPorter
                } else {
                    TradeRouteStatus::Idle
                };
                if !route.automatic || stop_requested {
                    crew::release_crew(world, ship, &port);
                }
                world.entity_mut(entity).remove::<StopAfterVoyage>();
                status(world, ship, ShipStatus::Moored);
                world.entity_mut(ship).remove::<ShipRouteRoutine>();
                world.entity_mut(entity).insert(route);
                continue;
            }
        }
        world.entity_mut(ship).insert(routine);
        world.entity_mut(entity).insert(route);
    }
}

#[cfg(test)]
mod tests;
