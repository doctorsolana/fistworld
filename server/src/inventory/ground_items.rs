//! Ground item pickup/drop systems.

use bevy::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::{MessageReceiver, NetworkTarget, RemoteId, Replicate, ReplicationMode};
use shared::components::{Player, PlayerPosition};
use shared::items::{
    DropRequest, GroundItem, GroundItemPosition, Inventory, ItemType, PickupRequest, PICKUP_RANGE,
};

use crate::player::index::PlayerEntityIndex;

/// Handle pickup requests from clients.
pub fn handle_pickup_requests(
    mut commands: Commands,
    mut client_links: Query<(&RemoteId, &mut MessageReceiver<PickupRequest>), With<ClientOf>>,
    player_index: Res<PlayerEntityIndex>,
    mut players: Query<(&PlayerPosition, &mut Inventory), With<Player>>,
    ground_items: Query<(Entity, &GroundItem, &GroundItemPosition)>,
) {
    for (remote_id, mut receiver) in client_links.iter_mut() {
        let peer_id = remote_id.0;

        for _request in receiver.receive() {
            let Some(player_entity) = player_index.entity_for_peer(peer_id) else {
                warn!("Pickup request from unknown player {:?}", peer_id);
                continue;
            };
            let Ok((player_pos, mut inventory)) = players.get_mut(player_entity) else {
                warn!("Pickup request from stale player entity {:?}", peer_id);
                continue;
            };

            let mut closest: Option<(Entity, &GroundItem, f32)> = None;
            let pickup_range_sq = PICKUP_RANGE * PICKUP_RANGE;
            for (entity, item, pos) in ground_items.iter() {
                let distance_sq = player_pos.0.distance_squared(pos.0);
                if distance_sq <= pickup_range_sq
                    && (closest.is_none() || distance_sq < closest.as_ref().expect("checked").2)
                {
                    closest = Some((entity, item, distance_sq));
                }
            }

            let Some((item_entity, ground_item, _distance)) = closest else {
                continue;
            };

            let stack = ground_item.to_stack();
            if inventory.add_stack(stack).is_none() {
                if crate::telemetry::hotlog_enabled() {
                    info!(
                        "Player {:?} picked up {}x {} (mag: {:?})",
                        peer_id,
                        ground_item.quantity,
                        ground_item.item_type.display_name(),
                        ground_item.ammo_in_mag
                    );
                } else {
                    trace!(
                        "Player {:?} picked up {}x {} (mag: {:?})",
                        peer_id,
                        ground_item.quantity,
                        ground_item.item_type.display_name(),
                        ground_item.ammo_in_mag
                    );
                }
                commands.entity(item_entity).despawn();
            } else if crate::telemetry::hotlog_enabled() {
                info!(
                    "Player {:?} inventory full, couldn't pick up {}",
                    peer_id,
                    ground_item.item_type.display_name()
                );
            } else {
                trace!(
                    "Player {:?} inventory full, couldn't pick up {}",
                    peer_id,
                    ground_item.item_type.display_name()
                );
            }
        }
    }
}

/// Handle drop requests from clients.
pub fn handle_drop_requests(
    mut commands: Commands,
    mut client_links: Query<(&RemoteId, &mut MessageReceiver<DropRequest>), With<ClientOf>>,
    player_index: Res<PlayerEntityIndex>,
    mut players: Query<(&PlayerPosition, &mut Inventory), With<Player>>,
) {
    for (remote_id, mut receiver) in client_links.iter_mut() {
        let peer_id = remote_id.0;

        for request in receiver.receive() {
            let Some(player_entity) = player_index.entity_for_peer(peer_id) else {
                continue;
            };
            let Ok((player_pos, mut inventory)) = players.get_mut(player_entity) else {
                continue;
            };

            if let Some(mut stack) = inventory.remove_slot(request.slot_index) {
                if let Some(weapon_type) = stack.item_type.as_weapon_type() {
                    let ammo_in_mag = stack.get_weapon_ammo();
                    if ammo_in_mag > 0 {
                        let ammo_type = weapon_type.ammo_type();
                        inventory.add_item(ammo_type, ammo_in_mag);
                        if crate::telemetry::hotlog_enabled() {
                            info!(
                                "Player {:?} returned {} {} to inventory from dropped weapon",
                                peer_id,
                                ammo_in_mag,
                                ammo_type.display_name()
                            );
                        } else {
                            trace!(
                                "Player {:?} returned {} {} to inventory from dropped weapon",
                                peer_id,
                                ammo_in_mag,
                                ammo_type.display_name()
                            );
                        }
                    }
                    stack.set_weapon_ammo(0);
                }

                if crate::telemetry::hotlog_enabled() {
                    info!(
                        "Player {:?} dropped {}x {} from slot {}",
                        peer_id,
                        stack.quantity,
                        stack.item_type.display_name(),
                        request.slot_index
                    );
                } else {
                    trace!(
                        "Player {:?} dropped {}x {} from slot {}",
                        peer_id,
                        stack.quantity,
                        stack.item_type.display_name(),
                        request.slot_index
                    );
                }

                let drop_offset = Vec3::new(0.0, 0.0, 1.5);
                let drop_pos = player_pos.0 + drop_offset;
                spawn_ground_item_from_stack(&mut commands, &stack, drop_pos);
            }
        }
    }
}

/// Spawn a ground item in the world.
pub fn spawn_ground_item(
    commands: &mut Commands,
    item_type: ItemType,
    quantity: u32,
    position: Vec3,
) -> Entity {
    commands
        .spawn((
            GroundItem::new(item_type, quantity),
            GroundItemPosition(position),
            Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
        ))
        .id()
}

/// Spawn a ground item from an `ItemStack` (preserves weapon ammo state).
pub fn spawn_ground_item_from_stack(
    commands: &mut Commands,
    stack: &shared::items::ItemStack,
    position: Vec3,
) -> Entity {
    commands
        .spawn((
            GroundItem::from_stack(stack),
            GroundItemPosition(position),
            Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
        ))
        .id()
}
