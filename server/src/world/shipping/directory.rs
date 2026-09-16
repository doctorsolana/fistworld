//! Small retained ownership/availability summaries, independent of ship regions.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use shared::components::*;
use shared::economy::{Good, GoodsInventory};

pub(crate) fn sync_maritime_directory(
    mut commands: Commands,
    mut companies: Query<(Entity, &CompanyId, Option<&mut CompanyFleet>), With<Company>>,
    ships: Query<(Ref<ShipId>, Ref<CompanyShip>, Option<Ref<GoodsInventory>>)>,
    orders: Query<(Ref<ShipOrderId>, Ref<ShipConstructionOrder>)>,
    ports: Query<(Ref<BuildingId>, Ref<SettlementPort>)>,
    towns: Query<(Entity, &SettlementSummary, Option<&SettlementPortSummary>)>,
    mut removed_ships: RemovedComponents<CompanyShip>,
    mut removed_orders: RemovedComponents<ShipConstructionOrder>,
    mut removed_ports: RemovedComponents<SettlementPort>,
    mut initialized: Local<bool>,
) {
    let dirty = !*initialized
        || !removed_ships.is_empty()
        || !removed_orders.is_empty()
        || !removed_ports.is_empty()
        || ships
            .iter()
            .any(|(id, ship, cargo)| id.is_changed() || ship.is_changed() || cargo.is_some_and(|cargo| cargo.is_changed()))
        || orders
            .iter()
            .any(|(id, order)| id.is_changed() || order.is_changed())
        || ports
            .iter()
            .any(|(id, port)| id.is_changed() || port.is_changed());
    removed_ships.clear();
    removed_orders.clear();
    removed_ports.clear();
    if !dirty {
        return;
    }
    *initialized = true;
    let mut fleets = HashMap::<CompanyId, CompanyFleet>::new();
    for (id, ship, cargo) in &ships {
        if let Some(cargo) = cargo {
            fleets.entry(ship.company).or_default().cargo.extend(Good::ALL.into_iter().filter_map(|good| {
                let amount = cargo.amount(good); (amount > 0).then_some((*id, good, amount))
            }));
        }
        fleets
            .entry(ship.company)
            .or_default()
            .ships
            .push((*id, *ship));
    }
    for (id, order) in &orders {
        if !matches!(
            order.status,
            ShipOrderStatus::Completed | ShipOrderStatus::Cancelled
        ) {
            fleets
                .entry(order.company)
                .or_default()
                .orders
                .push((*id, *order));
        }
    }
    for (entity, id, existing) in &mut companies {
        let mut next = fleets.remove(id).unwrap_or_default();
        next.ships.sort_unstable_by_key(|(id, _)| *id);
        next.cargo.sort_unstable_by_key(|(id, good, _)| (*id, *good as u8));
        next.orders.sort_unstable_by_key(|(id, _)| *id);
        if let Some(mut existing) = existing {
            existing.set_if_neq(next);
        } else if !next.ships.is_empty() || !next.orders.is_empty() {
            commands.entity(entity).insert(next);
        }
    }
    let availability: HashMap<_, _> = ports
        .iter()
        .map(|(id, port)| {
            (
                port.settlement,
                SettlementPortSummary {
                    port: *id,
                    maximum_ship: port.geometry.maximum_ship,
                    built: port.built,
                },
            )
        })
        .collect();
    for (entity, town, existing) in &towns {
        match availability.get(&town.id) {
            Some(next) if existing != Some(next) => {
                commands.entity(entity).insert(*next);
            }
            None if existing.is_some() => {
                commands.entity(entity).remove::<SettlementPortSummary>();
            }
            _ => {}
        }
    }
}
