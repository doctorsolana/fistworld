//! Company authority enters through maritime commands; orders retain their
//! own identity, materials and finite work budget until a real hull is ready.

use super::{
    construction::{self, PortWorkProject},
    funding,
};
use bevy::prelude::*;
use lightyear::prelude::{NetworkTarget, Replicate};
use shared::{components::*, economy::*, region::RegionCoord};

const MAX_OPEN_ORDERS: usize = 32;
#[derive(Component)]
struct PendingShipOrder {
    retry_at: f64,
}

pub(crate) fn order_ship(
    world: &mut World,
    company: CompanyId,
    port: BuildingId,
    kind: ShipKind,
) -> Result<String, &'static str> {
    if funding::company_for(world, company).is_none() {
        return Err("That company is unavailable.");
    }
    let (port_entity, port_state) = world
        .query::<(Entity, &BuildingId, &SettlementPort)>()
        .iter(world)
        .find(|(_, id, _)| **id == port)
        .map(|(entity, _, port)| (entity, *port))
        .ok_or("That port is unavailable.")?;
    if !port_state.accepts(kind) {
        return Err("This port is unfinished or cannot accommodate that hull.");
    }
    if !super::siting::port_still_valid(world, port_entity) {
        return Err("That port's berth no longer has safe water access.");
    }
    let warehouse = world
        .query::<(
            &OperatedBy,
            &BuildingOf,
            &SettlementBuilding,
            Option<&BusinessCondition>,
        )>()
        .iter(world)
        .any(|(owner, town, building, condition)| {
            owner.0 == company
                && town.0 == port_state.settlement
                && building.kind == SettlementBuildingKind::StorageHall
                && condition.is_none_or(|condition| condition.state.can_operate())
        });
    if !warehouse {
        return Err("The company needs an operating Storage Hall in this port's town.");
    }
    let ships = world
        .query::<&CompanyShip>()
        .iter(world)
        .filter(|ship| ship.company == company)
        .count();
    let (open, company_open) = world
        .query::<&ShipConstructionOrder>()
        .iter(world)
        .filter(|order| {
            !matches!(
                order.status,
                ShipOrderStatus::Completed | ShipOrderStatus::Cancelled
            )
        })
        .fold((0usize, 0usize), |(all, own), order| {
            (all + 1, own + usize::from(order.company == company))
        });
    if ships + company_open >= MAX_COMPANY_SHIPS {
        return Err("This company already owns or is building eight ships.");
    }
    if open >= MAX_OPEN_ORDERS {
        return Err("The shipyards are busy; wait for an existing hull order to finish.");
    }
    let id = world
        .resource_mut::<crate::world::identity::WorldIdAllocator>()
        .ship_order();
    world.spawn((
        id,
        ShipConstructionOrder {
            company,
            port,
            kind,
            status: ShipOrderStatus::AwaitingMaterials,
            delivered: [0; 3],
            progress: 0,
        },
        OperatedBy(company),
        BusinessAccount::default(),
        PendingShipOrder { retry_at: 0.0 },
        Replicate::to_clients(NetworkTarget::All),
    ));
    Ok(format!(
        "{} ordered. Construction waits for actual wood, iron, wool and company funding.",
        kind.label()
    ))
}

pub(crate) fn cancel_ship_order(
    world: &mut World,
    company: CompanyId,
    order: ShipOrderId,
) -> Result<String, &'static str> {
    let entity = world
        .query::<(Entity, &ShipOrderId, &ShipConstructionOrder)>()
        .iter(world)
        .find(|(_, id, state)| **id == order && state.company == company)
        .map(|(entity, _, _)| entity)
        .ok_or("That company order is unavailable.")?;
    let status = world.get::<ShipConstructionOrder>(entity).unwrap().status;
    if matches!(
        status,
        ShipOrderStatus::Completed | ShipOrderStatus::Cancelled
    ) {
        return Err("That order has already finished.");
    }
    if let Some(mut project) = world.get_mut::<PortWorkProject>(entity) {
        project.cancelling = true;
        Ok("Cancellation requested. Carried materials return, unused labour funds are refunded, and remaining goods are offered through the town market under company ownership.".into())
    } else {
        world
            .get_mut::<ShipConstructionOrder>(entity)
            .unwrap()
            .status = ShipOrderStatus::Cancelled;
        world
            .entity_mut(entity)
            .remove::<(PendingShipOrder, OperatedBy, BusinessAccount)>();
        Ok("Unfunded ship order cancelled.".into())
    }
}

fn fund(
    world: &mut World,
    entity: Entity,
    order: ShipConstructionOrder,
    clock: &WorldTime,
) -> bool {
    let Some((port, port_state)) = world
        .query::<(Entity, &BuildingId, &SettlementPort)>()
        .iter(world)
        .find(|(_, id, _)| **id == order.port)
        .map(|(entity, _, port)| (entity, *port))
    else {
        return false;
    };
    if !port_state.accepts(order.kind) || !super::siting::port_still_valid(world, port) {
        return false;
    }
    // Only one hull may occupy this single-berth shipyard at a time. Keep later
    // orders unfunded, instead of reserving all of a town's scarce iron.
    if world
        .query::<(Entity, &ShipConstructionOrder)>()
        .iter(world)
        .any(|(other, state)| {
            other != entity
                && state.port == order.port
                && matches!(
                    state.status,
                    ShipOrderStatus::Hauling | ShipOrderStatus::Building
                )
        })
    {
        return false;
    }
    let Some(hall) = funding::hall_for(world, port_state.settlement) else {
        return false;
    };
    let Some(pickup) = funding::hall_pickup(world, hall) else {
        return false;
    };
    let materials = order.kind.materials();
    if materials.iter().any(|(good, units)| {
        world
            .get::<GoodsInventory>(hall)
            .is_none_or(|stock| stock.amount(*good) < *units)
            || world.get::<MootMarket>(hall).is_none_or(|market| {
                !market.can_trade(*good) || market.listed_units(*good) < *units
            })
    }) {
        return false;
    }
    let Some((company, available)) = funding::company_spendable(world, order.company) else {
        return false;
    };
    let wage = funding::daily_wage(world, hall);
    let labour = funding::labour_quote(clock, wage, f64::from(order.kind.build_seconds()));
    let haul_fees: Vec<_> = materials
        .iter()
        .map(|(good, units)| {
            crate::world::shipping::quote_haul_fee(
                clock,
                pickup,
                port_state.geometry.shore,
                *good,
                *units,
                wage,
            )
        })
        .collect();
    // A paid public handling charge per actual construction bulk helps recover
    // the town's pier capital. All labour is separately backed, with no idle
    // harbour salary or money creation when no ships are being serviced.
    let harbour_fee = u64::from(funding::material_bulk(&materials).unwrap());
    let Some(base) = labour.checked_add(harbour_fee) else {
        return false;
    };
    let Some(fees) = haul_fees
        .iter()
        .try_fold(base, |sum, fee| sum.checked_add(*fee))
    else {
        return false;
    };
    let Some(budget) = available.checked_sub(fees) else {
        return false;
    };
    let Some(quote) = funding::quote_materials(world, hall, &materials, budget, None) else {
        return false;
    };
    let Some(total) = quote.cost.checked_add(fees) else {
        return false;
    };
    if world
        .get::<Settlement>(hall)
        .is_none_or(|town| town.treasury.checked_add(harbour_fee).is_none())
    {
        return false;
    }
    if !world
        .get_mut::<CompanyAccount>(company)
        .unwrap()
        .debit(total)
    {
        return false;
    }
    world.get_mut::<Settlement>(hall).unwrap().treasury += harbour_fee;
    if let Some(mut civic) = world.get_mut::<CivicAccount>(hall) {
        civic.record_delivery_fee_income(clock.day, harbour_fee);
    }
    construction::capitalize(world, entity, clock.day, quote.cost + harbour_fee);
    construction::start_work(
        world,
        entity,
        hall,
        port_state.settlement,
        port,
        PortCargoOwner::Company(order.company),
        materials.to_vec(),
        order.kind.build_seconds(),
        labour,
        haul_fees,
        quote,
        clock,
    );
    world
        .get_mut::<ShipConstructionOrder>(entity)
        .unwrap()
        .status = ShipOrderStatus::Hauling;
    world.entity_mut(entity).remove::<PendingShipOrder>();
    true
}

pub(crate) fn advance_ship_orders(world: &mut World) {
    let Some(clock) = funding::clock(world) else {
        return;
    };
    let now = construction::time(&clock);
    let pending: Vec<_> = world
        .query::<(Entity, &ShipConstructionOrder, &PendingShipOrder)>()
        .iter(world)
        .filter(|(_, _, pending)| now >= pending.retry_at)
        .map(|(entity, order, _)| (entity, *order))
        .collect();
    // At most four material baskets are considered per update. Each failed
    // order waits thirty simulated seconds and contributes no fictional goods.
    for (entity, order) in pending.into_iter().take(4) {
        if !fund(world, entity, order, &clock) {
            world.get_mut::<PendingShipOrder>(entity).unwrap().retry_at = now + 30.0;
        }
    }
    let deliveries: Vec<_> = world
        .query::<(Entity, &ShipConstructionOrder, &PortWorkProject)>()
        .iter(world)
        .filter(|(_, _, project)| !project.finished)
        .map(|(entity, order, project)| {
            let amounts = order.kind.materials().map(|(good, _)| {
                world
                    .get::<GoodsInventory>(project.destination)
                    .map_or(0, |stock| stock.amount(good))
            });
            (entity, amounts)
        })
        .collect();
    for (entity, delivered) in deliveries {
        if let Some(mut order) = world.get_mut::<ShipConstructionOrder>(entity) {
            if order.delivered != delivered {
                order.delivered = delivered;
            }
        }
    }
}

pub(super) fn launch(
    world: &mut World,
    order_entity: Entity,
    project: &PortWorkProject,
    _clock: &WorldTime,
) -> bool {
    let Some(order) = world.get::<ShipConstructionOrder>(order_entity).copied() else {
        return false;
    };
    let Some(port) = world.get::<SettlementPort>(project.port).copied() else {
        return false;
    };
    if !port.accepts(order.kind) || !super::siting::port_still_valid(world, project.port) {
        return false;
    }
    let radius = crate::player::boat::clearance::WatercraftClearance::for_ship(order.kind).radius;
    if world
        .query_filtered::<(&PlayerPosition, Option<&CompanyShip>), With<Vessel>>()
        .iter(world)
        .any(|(position, ship)| {
            let other = ship.map_or(2.35, |ship| {
                crate::player::boat::clearance::WatercraftClearance::for_ship(ship.kind).radius
            });
            position.0.xz().distance_squared(port.geometry.berth.xz())
                < (radius + other + 0.5).powi(2)
        })
    {
        return false;
    }
    if !crate::world::shipping::routes::berth_available(world, project.port, None) {
        return false;
    }
    let id = world
        .resource_mut::<crate::world::identity::WorldIdAllocator>()
        .ship();
    world
        .entity_mut(project.port)
        .insert(crate::world::shipping::routes::PortBerthReservation(id));
    let account = world
        .entity_mut(order_entity)
        .take::<BusinessAccount>()
        .unwrap_or_default();
    world.spawn((
        id,
        CompanyShip {
            company: order.company,
            kind: order.kind,
            home_port: order.port,
            assigned_route: None,
            status: ShipStatus::Moored,
        },
        OperatedBy(order.company),
        account,
        Vessel,
        crate::player::boat::VesselNavigation::for_ship(order.kind),
        GoodsInventory::new(order.kind.capacity()),
        PlayerPosition(port.geometry.berth),
        PlayerRotation(port.geometry.yaw),
        RegionCoord::from_world_pos(port.geometry.berth),
        Replicate::to_clients(NetworkTarget::All),
    ));
    world.entity_mut(order_entity).remove::<OperatedBy>();
    world
        .get_mut::<ShipConstructionOrder>(order_entity)
        .unwrap()
        .status = ShipOrderStatus::Completed;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unfunded_ship_cancellation_requires_own_company_and_leaves_no_fake_asset() {
        let mut world = World::new();
        let owner = CompanyId(2);
        let entity = world
            .spawn((
                ShipOrderId(7),
                ShipConstructionOrder {
                    company: owner,
                    port: BuildingId(4),
                    kind: ShipKind::Coaster,
                    status: ShipOrderStatus::AwaitingMaterials,
                    delivered: [0; 3],
                    progress: 0,
                },
                OperatedBy(owner),
                BusinessAccount::default(),
                PendingShipOrder { retry_at: 0. },
            ))
            .id();
        assert!(cancel_ship_order(&mut world, CompanyId(9), ShipOrderId(7)).is_err());
        assert_eq!(
            world.get::<ShipConstructionOrder>(entity).unwrap().status,
            ShipOrderStatus::AwaitingMaterials
        );
        assert!(cancel_ship_order(&mut world, owner, ShipOrderId(7)).is_ok());
        assert_eq!(
            world.get::<ShipConstructionOrder>(entity).unwrap().status,
            ShipOrderStatus::Cancelled
        );
        assert!(world.get::<OperatedBy>(entity).is_none());
        assert!(world.get::<BusinessAccount>(entity).is_none());
        assert!(world.get::<PendingShipOrder>(entity).is_none());
        assert!(cancel_ship_order(&mut world, owner, ShipOrderId(7)).is_err());
    }
}
