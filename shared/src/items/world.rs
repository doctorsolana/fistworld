use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::weapons::WeaponType;

use super::{ItemStack, ItemType, CHEST_SLOTS};

/// An item that exists in the world and can be picked up.
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroundItem {
    pub item_type: ItemType,
    pub quantity: u32,
    /// For weapon items: ammo currently in the magazine (None = empty mag, needs reload)
    pub ammo_in_mag: Option<u32>,
}

impl GroundItem {
    pub fn new(item_type: ItemType, quantity: u32) -> Self {
        Self {
            item_type,
            quantity,
            ammo_in_mag: None,
        }
    }

    /// Create a ground item from an ItemStack (preserves weapon ammo).
    pub fn from_stack(stack: &ItemStack) -> Self {
        Self {
            item_type: stack.item_type,
            quantity: stack.quantity,
            ammo_in_mag: stack.ammo_in_mag,
        }
    }

    /// Create a weapon ground item with specified ammo in mag.
    pub fn new_weapon(weapon_type: WeaponType, ammo_in_mag: u32) -> Self {
        Self {
            item_type: ItemType::Weapon(weapon_type),
            quantity: 1,
            ammo_in_mag: Some(ammo_in_mag),
        }
    }

    /// Convert to an ItemStack (for adding to inventory).
    pub fn to_stack(&self) -> ItemStack {
        ItemStack {
            item_type: self.item_type,
            quantity: self.quantity,
            ammo_in_mag: self.ammo_in_mag,
        }
    }
}

/// Position component for ground items (separate from transform for networking).
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroundItemPosition(pub Vec3);

/// A chest/storage container with item slots.
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChestStorage {
    pub slots: [Option<ItemStack>; CHEST_SLOTS],
}

impl Default for ChestStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl ChestStorage {
    pub fn new() -> Self {
        Self {
            slots: [None; CHEST_SLOTS],
        }
    }

    /// Create a chest with initial items.
    pub fn with_items(items: Vec<ItemStack>) -> Self {
        let mut chest = Self::new();
        for (i, item) in items.into_iter().take(CHEST_SLOTS).enumerate() {
            chest.slots[i] = Some(item);
        }
        chest
    }

    /// Get a slot by index.
    pub fn get_slot(&self, index: usize) -> Option<&ItemStack> {
        self.slots.get(index).and_then(|s| s.as_ref())
    }

    /// Get a mutable slot by index.
    pub fn get_slot_mut(&mut self, index: usize) -> Option<&mut ItemStack> {
        self.slots.get_mut(index).and_then(|s| s.as_mut())
    }

    /// Take an item from a slot (removes it).
    pub fn take_slot(&mut self, index: usize) -> Option<ItemStack> {
        if index < CHEST_SLOTS {
            self.slots[index].take()
        } else {
            None
        }
    }

    /// Put an item into a slot (fails if slot occupied or out of bounds).
    pub fn put_slot(&mut self, index: usize, stack: ItemStack) -> Result<(), ItemStack> {
        if index >= CHEST_SLOTS {
            return Err(stack);
        }
        if self.slots[index].is_some() {
            return Err(stack);
        }
        self.slots[index] = Some(stack);
        Ok(())
    }

    /// Find first empty slot.
    pub fn find_empty_slot(&self) -> Option<usize> {
        self.slots.iter().position(|slot| slot.is_none())
    }

    /// Check if chest is empty.
    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(|s| s.is_none())
    }
}

/// Position component for chests (separate from transform for networking).
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChestPosition(pub Vec3);
