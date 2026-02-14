//! Inventory drop-on-death systems.

use bevy::prelude::*;
use shared::components::{Health, Player, PlayerPosition};
use shared::items::{Inventory, ItemType};

use super::ground_items::{spawn_ground_item, spawn_ground_item_from_stack};

/// Drop all inventory items when a player dies.
pub fn handle_inventory_drop_on_death(
    mut commands: Commands,
    mut players: Query<(&Player, &PlayerPosition, &mut Inventory, &Health), Changed<Health>>,
) {
    for (player, position, mut inventory, health) in players.iter_mut() {
        if health.is_dead() && !inventory.is_empty() {
            info!("Player {:?} died, dropping inventory", player.client_id);

            let mut items_to_drop: Vec<(shared::items::ItemStack, Vec3)> = Vec::new();
            let mut extra_ammo: Vec<(ItemType, u32)> = Vec::new();
            let mut drop_offset = 0.0_f32;

            for (_, stack) in inventory.iter_items() {
                if let Some(weapon_type) = stack.item_type.as_weapon_type() {
                    let ammo_in_mag = stack.get_weapon_ammo();
                    if ammo_in_mag > 0 {
                        extra_ammo.push((weapon_type.ammo_type(), ammo_in_mag));
                    }

                    let mut weapon_stack = *stack;
                    weapon_stack.set_weapon_ammo(0);

                    let offset = Vec3::new(drop_offset.sin() * 1.5, 0.5, drop_offset.cos() * 1.5);
                    items_to_drop.push((weapon_stack, position.0 + offset));
                } else {
                    let offset = Vec3::new(drop_offset.sin() * 1.5, 0.5, drop_offset.cos() * 1.5);
                    items_to_drop.push((*stack, position.0 + offset));
                }
                drop_offset += 1.2;
            }

            for (stack, pos) in items_to_drop {
                spawn_ground_item_from_stack(&mut commands, &stack, pos);
            }

            for (ammo_type, quantity) in extra_ammo {
                let offset = Vec3::new(drop_offset.sin() * 1.5, 0.5, drop_offset.cos() * 1.5);
                spawn_ground_item(&mut commands, ammo_type, quantity, position.0 + offset);
                drop_offset += 1.2;
            }

            *inventory = Inventory::new();
        }
    }
}
