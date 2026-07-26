//! Hotbar and inventory slot systems.

use bevy::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::{MessageReceiver, RemoteId};
use shared::components::{EquippedWeapon, Player};
use shared::items::{
    HotbarSelection, Inventory, InventoryMoveRequest, SelectHotbarSlot, HOTBAR_SLOTS,
    INVENTORY_SLOTS,
};
use shared::weapons::WeaponType;

use crate::player::index::PlayerEntityIndex;

/// Tracks which hotbar slot was previously active (for ammo save/load).
#[derive(Component, Default, Clone)]
pub struct PreviousHotbarSlot {
    pub index: Option<usize>,
}

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

/// Server-authoritative: ensure `EquippedWeapon` matches the active hotbar slot.
/// If the active slot does not contain a weapon item, the player is `Unarmed`.
/// Also syncs ammo_in_mag between EquippedWeapon and the inventory slot.
pub fn sync_equipped_weapon_from_hotbar(
    mut players: Query<(
        &mut Inventory,
        &HotbarSelection,
        &mut EquippedWeapon,
        &mut PreviousHotbarSlot,
    )>,
) {
    for (mut inventory, selection, mut equipped, mut prev_slot) in players.iter_mut() {
        let slot_idx = (selection.index as usize)
            .min(HOTBAR_SLOTS.saturating_sub(1))
            .min(INVENTORY_SLOTS.saturating_sub(1));

        let (desired, slot_ammo) = inventory
            .get_slot(slot_idx)
            .map(|stack| {
                let wt = stack
                    .item_type
                    .as_weapon_type()
                    .unwrap_or(WeaponType::Unarmed);
                let ammo = stack.get_weapon_ammo();
                (wt, ammo)
            })
            .unwrap_or((WeaponType::Unarmed, 0));

        // Off-hand shield: a shield in any NON-selected hotbar slot rides in
        // the left hand, but only alongside one-handed weapons (must agree
        // with EquippedWeapon::can_block, and two-handed rifles would clip).
        let one_handed = matches!(
            desired,
            WeaponType::Sword | WeaponType::Pistol | WeaponType::Unarmed
        );
        let offhand_shield = one_handed
            && (0..HOTBAR_SLOTS).any(|idx| {
                idx != slot_idx
                    && inventory
                        .get_slot(idx)
                        .and_then(|stack| stack.item_type.as_weapon_type())
                        .map(|wt| wt.is_shield())
                        .unwrap_or(false)
            });
        if equipped.offhand_shield != offhand_shield {
            equipped.offhand_shield = offhand_shield;
            if !offhand_shield && !equipped.weapon_type.is_shield() {
                equipped.blocking = false;
            }
        }

        let switching = prev_slot.index != Some(slot_idx) || equipped.weapon_type != desired;

        if switching {
            if let Some(prev_idx) = prev_slot.index {
                if equipped.weapon_type != WeaponType::Unarmed {
                    if let Some(stack) = inventory.get_slot_mut(prev_idx) {
                        if stack.item_type.as_weapon_type() == Some(equipped.weapon_type) {
                            stack.set_weapon_ammo(equipped.ammo_in_mag);
                        }
                    }
                }
            }

            equipped.weapon_type = desired;
            equipped.aiming = false;
            equipped.blocking = false;
            equipped.last_fire_time = -10.0;

            if desired != WeaponType::Unarmed {
                equipped.ammo_in_mag = slot_ammo;
            } else {
                equipped.ammo_in_mag = 0;
            }

            prev_slot.index = Some(slot_idx);
        } else if equipped.weapon_type != WeaponType::Unarmed {
            if let Some(stack) = inventory.get_slot_mut(slot_idx) {
                if stack.item_type.as_weapon_type() == Some(equipped.weapon_type) {
                    stack.set_weapon_ammo(equipped.ammo_in_mag);
                }
            }
        }
    }
}
