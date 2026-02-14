//! Chest open/close/transfer systems.

use bevy::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::{
    MessageReceiver, NetworkTarget, PeerId, RemoteId, Replicate, ReplicationMode,
};
use shared::components::{Player, PlayerPosition};
use shared::items::{
    ChestPosition, ChestStorage, ChestTransferRequest, CloseChestRequest, Inventory, ItemStack,
    OpenChestRequest, CHEST_RANGE, CHEST_SLOTS, INVENTORY_SLOTS,
};
use std::collections::HashMap;

use crate::player::index::PlayerEntityIndex;

/// Tracks which player has which chest open (PeerId -> chest entity).
#[derive(Resource, Default)]
pub struct OpenChests {
    pub map: HashMap<PeerId, Entity>,
}

/// Spawn a chest in the world with initial items.
pub fn spawn_chest(commands: &mut Commands, position: Vec3, items: Vec<ItemStack>) -> Entity {
    commands
        .spawn((
            ChestStorage::with_items(items),
            ChestPosition(position),
            Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
        ))
        .id()
}

/// Handle open chest requests from clients.
pub fn handle_open_chest_requests(
    mut client_links: Query<(&RemoteId, &mut MessageReceiver<OpenChestRequest>), With<ClientOf>>,
    player_index: Res<PlayerEntityIndex>,
    players: Query<&PlayerPosition, With<Player>>,
    chests: Query<(Entity, &ChestPosition)>,
    mut open_chests: ResMut<OpenChests>,
) {
    for (remote_id, mut receiver) in client_links.iter_mut() {
        let peer_id = remote_id.0;

        for _request in receiver.receive() {
            let Some(player_entity) = player_index.entity_for_peer(peer_id) else {
                continue;
            };
            let Ok(player_pos) = players.get(player_entity) else {
                continue;
            };

            let mut closest: Option<(Entity, f32)> = None;
            for (chest_entity, chest_pos) in chests.iter() {
                let distance = player_pos.0.distance(chest_pos.0);
                if distance <= CHEST_RANGE
                    && (closest.is_none() || distance < closest.expect("checked").1)
                {
                    closest = Some((chest_entity, distance));
                }
            }

            if let Some((chest_entity, _)) = closest {
                open_chests.map.insert(peer_id, chest_entity);
                if crate::telemetry::hotlog_enabled() {
                    info!("Player {:?} opened chest {:?}", peer_id, chest_entity);
                } else {
                    trace!("Player {:?} opened chest {:?}", peer_id, chest_entity);
                }
            }
        }
    }
}

/// Handle close chest requests from clients.
pub fn handle_close_chest_requests(
    mut client_links: Query<(&RemoteId, &mut MessageReceiver<CloseChestRequest>), With<ClientOf>>,
    mut open_chests: ResMut<OpenChests>,
) {
    for (remote_id, mut receiver) in client_links.iter_mut() {
        let peer_id = remote_id.0;

        for _request in receiver.receive() {
            if open_chests.map.remove(&peer_id).is_some() {
                if crate::telemetry::hotlog_enabled() {
                    info!("Player {:?} closed chest", peer_id);
                } else {
                    trace!("Player {:?} closed chest", peer_id);
                }
            }
        }
    }
}

/// Handle chest transfer requests (move items between player inventory and chest).
pub fn handle_chest_transfer_requests(
    mut client_links: Query<
        (&RemoteId, &mut MessageReceiver<ChestTransferRequest>),
        With<ClientOf>,
    >,
    player_index: Res<PlayerEntityIndex>,
    mut players: Query<&mut Inventory, With<Player>>,
    mut chests: Query<&mut ChestStorage>,
    open_chests: Res<OpenChests>,
) {
    for (remote_id, mut receiver) in client_links.iter_mut() {
        let peer_id = remote_id.0;

        for request in receiver.receive() {
            let Some(&chest_entity) = open_chests.map.get(&peer_id) else {
                continue;
            };

            let Some(player_entity) = player_index.entity_for_peer(peer_id) else {
                continue;
            };
            let Ok(mut inventory) = players.get_mut(player_entity) else {
                continue;
            };

            let Ok(mut chest) = chests.get_mut(chest_entity) else {
                continue;
            };

            let from_slot = request.from_slot as usize;
            let to_slot = request.to_slot as usize;

            if request.from_chest {
                if from_slot >= CHEST_SLOTS || to_slot >= INVENTORY_SLOTS {
                    continue;
                }

                if let Some(stack) = chest.take_slot(from_slot) {
                    if let Some(existing) = inventory.get_slot(to_slot).cloned() {
                        if existing.item_type == stack.item_type
                            && stack.item_type.max_stack_size() > 1
                        {
                            if let Some(inv_stack) = inventory.get_slot_mut(to_slot) {
                                let space =
                                    inv_stack.item_type.max_stack_size() - inv_stack.quantity;
                                let transfer = space.min(stack.quantity);
                                inv_stack.quantity += transfer;
                                if transfer < stack.quantity {
                                    let mut remainder = stack;
                                    remainder.quantity -= transfer;
                                    let _ = chest.put_slot(from_slot, remainder);
                                }
                            }
                        } else {
                            inventory.set_slot(to_slot, Some(stack));
                            let _ = chest.put_slot(from_slot, existing);
                        }
                    } else {
                        inventory.set_slot(to_slot, Some(stack));
                    }
                    if crate::telemetry::hotlog_enabled() {
                        info!(
                            "Player {:?} transferred item from chest slot {} to inventory slot {}",
                            peer_id, from_slot, to_slot
                        );
                    } else {
                        trace!(
                            "Player {:?} transferred item from chest slot {} to inventory slot {}",
                            peer_id,
                            from_slot,
                            to_slot
                        );
                    }
                }
            } else {
                if from_slot >= INVENTORY_SLOTS || to_slot >= CHEST_SLOTS {
                    continue;
                }

                if let Some(stack) = inventory.remove_slot(from_slot) {
                    if let Some(existing) = chest.get_slot(to_slot).cloned() {
                        if existing.item_type == stack.item_type
                            && stack.item_type.max_stack_size() > 1
                        {
                            if let Some(chest_stack) = chest.get_slot_mut(to_slot) {
                                let space =
                                    chest_stack.item_type.max_stack_size() - chest_stack.quantity;
                                let transfer = space.min(stack.quantity);
                                chest_stack.quantity += transfer;
                                if transfer < stack.quantity {
                                    let mut remainder = stack;
                                    remainder.quantity -= transfer;
                                    inventory.set_slot(from_slot, Some(remainder));
                                }
                            }
                        } else {
                            chest.slots[to_slot] = Some(stack);
                            inventory.set_slot(from_slot, Some(existing));
                        }
                    } else {
                        let _ = chest.put_slot(to_slot, stack);
                    }
                    if crate::telemetry::hotlog_enabled() {
                        info!(
                            "Player {:?} transferred item from inventory slot {} to chest slot {}",
                            peer_id, from_slot, to_slot
                        );
                    } else {
                        trace!(
                            "Player {:?} transferred item from inventory slot {} to chest slot {}",
                            peer_id,
                            from_slot,
                            to_slot
                        );
                    }
                }
            }
        }
    }
}

/// Auto-close chests when players walk too far away.
pub fn update_distant_chest_auto_close(
    player_index: Res<PlayerEntityIndex>,
    players: Query<&PlayerPosition, With<Player>>,
    chests: Query<&ChestPosition>,
    mut open_chests: ResMut<OpenChests>,
) {
    let mut to_close = Vec::new();

    for (&client_id, &chest_entity) in open_chests.map.iter() {
        let Some(player_entity) = player_index.entity_for_peer(client_id) else {
            to_close.push(client_id);
            continue;
        };
        let Ok(player_pos) = players.get(player_entity) else {
            to_close.push(client_id);
            continue;
        };

        let Ok(chest_pos) = chests.get(chest_entity) else {
            to_close.push(client_id);
            continue;
        };

        if player_pos.0.distance(chest_pos.0) > CHEST_RANGE + 1.0 {
            to_close.push(client_id);
            if crate::telemetry::hotlog_enabled() {
                info!(
                    "Auto-closing chest for player {:?} (walked away)",
                    client_id
                );
            } else {
                trace!(
                    "Auto-closing chest for player {:?} (walked away)",
                    client_id
                );
            }
        }
    }

    for client_id in to_close {
        open_chests.map.remove(&client_id);
    }
}
