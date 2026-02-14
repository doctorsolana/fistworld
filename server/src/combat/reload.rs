//! Reload systems.

use bevy::prelude::*;
use lightyear::prelude::server::*;
use lightyear::prelude::*;

use shared::components::{EquippedWeapon, Player};
use shared::items::Inventory;
use shared::protocol::ReloadRequest;
use shared::weapons::WeaponType;

use crate::player::index::PlayerEntityIndex;

/// Server-side reload state (not replicated).
#[derive(Component, Clone, Debug)]
pub struct WeaponReload {
    pub weapon_type: WeaponType,
    pub end_time: f32,
    pub pending_ammo: u32,
}

/// Finish reloads once their timers elapse (or cancel if weapon switched).
pub fn update_reload_timers(
    mut commands: Commands,
    time: Res<Time>,
    mut players: Query<(
        Entity,
        &mut EquippedWeapon,
        &mut Inventory,
        Option<&mut WeaponReload>,
    )>,
) {
    let current_time = time.elapsed_secs();

    for (entity, mut weapon, mut inventory, reload) in players.iter_mut() {
        let Some(reload) = reload else { continue };

        // Cancel reload if we switched weapons.
        if weapon.weapon_type != reload.weapon_type {
            if reload.pending_ammo > 0 {
                inventory.add_item(reload.weapon_type.ammo_type(), reload.pending_ammo);
            }
            commands.entity(entity).remove::<WeaponReload>();
            continue;
        }

        if current_time < reload.end_time {
            continue;
        }

        let stats = weapon.weapon_type.stats();
        let needed = stats.magazine_size.saturating_sub(weapon.ammo_in_mag);
        let to_add = reload.pending_ammo.min(needed);
        if to_add > 0 {
            weapon.ammo_in_mag += to_add;
        }

        let leftover = reload.pending_ammo.saturating_sub(to_add);
        if leftover > 0 {
            inventory.add_item(weapon.weapon_type.ammo_type(), leftover);
        }

        commands.entity(entity).remove::<WeaponReload>();
    }
}

/// Handle reload requests from clients.
pub fn handle_reload_request(
    mut commands: Commands,
    mut client_links: Query<(&RemoteId, &mut MessageReceiver<ReloadRequest>), With<ClientOf>>,
    player_index: Res<PlayerEntityIndex>,
    mut players: Query<
        (
            Entity,
            &mut EquippedWeapon,
            &mut Inventory,
            Option<&WeaponReload>,
        ),
        With<Player>,
    >,
    time: Res<Time>,
) {
    let current_time = time.elapsed_secs();

    for (remote_id, mut receiver) in client_links.iter_mut() {
        let peer_id = remote_id.0;

        for _msg in receiver.receive() {
            let Some(player_entity) = player_index.entity_for_peer(peer_id) else {
                continue;
            };
            let Ok((entity, mut weapon, mut inventory, reload)) = players.get_mut(player_entity)
            else {
                continue;
            };

            // Ignore reload requests while already reloading the same weapon.
            if let Some(reload) = reload {
                if reload.weapon_type == weapon.weapon_type && current_time < reload.end_time {
                    continue;
                }
            }

            let stats = weapon.weapon_type.stats();
            let ammo_type = weapon.weapon_type.ammo_type();
            let needed = stats.magazine_size.saturating_sub(weapon.ammo_in_mag);
            let reserve_in_inventory = inventory.count_item(ammo_type);
            let pending = needed.min(reserve_in_inventory);

            if pending == 0 {
                continue;
            }

            // Take ammo from inventory now; add to mag when reload finishes.
            let taken = inventory.remove_item(ammo_type, pending);
            if taken == 0 {
                continue;
            }

            let duration = weapon.weapon_type.reload_duration(taken);
            if duration <= 0.0 {
                let to_add = taken.min(stats.magazine_size.saturating_sub(weapon.ammo_in_mag));
                weapon.ammo_in_mag += to_add;
                let leftover = taken.saturating_sub(to_add);
                if leftover > 0 {
                    inventory.add_item(ammo_type, leftover);
                }
                continue;
            }

            commands.entity(entity).insert(WeaponReload {
                weapon_type: weapon.weapon_type,
                end_time: current_time + duration,
                pending_ammo: taken,
            });

            info!(
                "Player {:?} started reload {:?}: +{} ammo in {:.2}s",
                peer_id, weapon.weapon_type, taken, duration
            );
        }
    }
}
