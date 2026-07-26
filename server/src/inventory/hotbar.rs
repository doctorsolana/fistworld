//! Hotbar and inventory slot systems.

use bevy::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::{MessageReceiver, RemoteId};
use shared::components::Player;
use shared::items::{
    HotbarSelection, Inventory, InventoryMoveRequest, SelectHotbarSlot, HOTBAR_SLOTS,
};

use crate::player::index::PlayerEntityIndex;

/// Handle hotbar selection requests from clients.
pub fn handle_hotbar_selection_requests(
    mut client_links: Query<(&RemoteId, &mut MessageReceiver<SelectHotbarSlot>), With<ClientOf>>,
    player_index: Res<PlayerEntityIndex>,
    mut players: Query<&mut HotbarSelection, With<Player>>,
) {
    for (remote_id, mut receiver) in client_links.iter_mut() {
        let peer_id = remote_id.0;

        for request in receiver.receive() {
            let clamped = (request.index as usize).min(HOTBAR_SLOTS.saturating_sub(1)) as u8;

            if let Some(entity) = player_index.entity_for_peer(peer_id) {
                if let Ok(mut selection) = players.get_mut(entity) {
                    selection.index = clamped;
                }
            }
        }
    }
}

/// Handle inventory move requests (drag & drop) from clients.
pub fn handle_inventory_move_requests(
    mut client_links: Query<
        (&RemoteId, &mut MessageReceiver<InventoryMoveRequest>),
        With<ClientOf>,
    >,
    player_index: Res<PlayerEntityIndex>,
    mut players: Query<&mut Inventory, With<Player>>,
) {
    for (remote_id, mut receiver) in client_links.iter_mut() {
        let peer_id = remote_id.0;

        for request in receiver.receive() {
            let from = request.from as usize;
            let to = request.to as usize;

            if let Some(entity) = player_index.entity_for_peer(peer_id) {
                let Ok(mut inventory) = players.get_mut(entity) else {
                    continue;
                };
                let _ = inventory.move_or_stack_slot(from, to);
            }
        }
    }
}
