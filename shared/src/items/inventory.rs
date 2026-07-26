use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::{ItemStack, ItemType, INVENTORY_SLOTS};

/// Player inventory with fixed slots.
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Inventory {
    slots: [Option<ItemStack>; INVENTORY_SLOTS],
}

impl Default for Inventory {
    fn default() -> Self {
        Self {
            slots: [None; INVENTORY_SLOTS],
        }
    }
}

impl Inventory {
    /// Create a new empty inventory.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create inventory with starting items for a new player.
    pub fn with_starting_items() -> Self {
        let mut inv = Self::new();

        // Starting ammo
        inv.add_item(ItemType::RifleAmmo, 114); // Rifle + revolver bullets
        inv.add_item(ItemType::ShotgunShells, 20);
        inv.add_item(ItemType::SniperRounds, 10);
        inv
    }

    /// Get a slot by index.
    pub fn get_slot(&self, index: usize) -> Option<&ItemStack> {
        self.slots.get(index).and_then(|s| s.as_ref())
    }

    /// Get a mutable slot by index.
    pub fn get_slot_mut(&mut self, index: usize) -> Option<&mut ItemStack> {
        self.slots.get_mut(index).and_then(|s| s.as_mut())
    }

    /// Get all slots.
    pub fn slots(&self) -> &[Option<ItemStack>; INVENTORY_SLOTS] {
        &self.slots
    }

    /// Find first slot with the given item type that has space.
    pub fn find_slot_with_space(&self, item_type: ItemType) -> Option<usize> {
        self.slots.iter().position(|slot| {
            if let Some(stack) = slot {
                stack.item_type == item_type && stack.space_remaining() > 0
            } else {
                false
            }
        })
    }

    /// Find first empty slot.
    pub fn find_empty_slot(&self) -> Option<usize> {
        self.slots.iter().position(|slot| slot.is_none())
    }

    /// Add items to inventory, returns amount that couldn't fit.
    pub fn add_item(&mut self, item_type: ItemType, mut quantity: u32) -> u32 {
        // First, try to stack with existing items
        while quantity > 0 {
            if let Some(slot_idx) = self.find_slot_with_space(item_type) {
                if let Some(stack) = &mut self.slots[slot_idx] {
                    quantity = stack.add(quantity);
                }
            } else {
                break;
            }
        }

        // Then, use empty slots
        while quantity > 0 {
            if let Some(slot_idx) = self.find_empty_slot() {
                let stack_amount = quantity.min(item_type.max_stack_size());
                self.slots[slot_idx] = Some(ItemStack::new(item_type, stack_amount));
                quantity -= stack_amount;
            } else {
                break;
            }
        }

        quantity // Return what couldn't fit
    }

    /// Add an ItemStack to inventory (preserves weapon ammo state).
    /// For non-stackable items (like weapons), finds an empty slot.
    /// Returns the stack if it couldn't be added, None if successful.
    pub fn add_stack(&mut self, stack: ItemStack) -> Option<ItemStack> {
        // Weapons are non-stackable (max_stack_size = 1), so just find an empty slot
        if stack.item_type.max_stack_size() == 1 {
            if let Some(slot_idx) = self.find_empty_slot() {
                self.slots[slot_idx] = Some(stack);
                return None; // Success
            }
            return Some(stack); // No room
        }

        // For stackable items, try to stack then use empty slots
        let mut remaining = stack;

        // Try to stack with existing
        while remaining.quantity > 0 {
            if let Some(slot_idx) = self.find_slot_with_space(remaining.item_type) {
                if let Some(existing) = &mut self.slots[slot_idx] {
                    remaining.quantity = existing.add(remaining.quantity);
                }
            } else {
                break;
            }
        }

        // Use empty slots for remainder
        while remaining.quantity > 0 {
            if let Some(slot_idx) = self.find_empty_slot() {
                let stack_amount = remaining.quantity.min(remaining.item_type.max_stack_size());
                self.slots[slot_idx] = Some(ItemStack {
                    item_type: remaining.item_type,
                    quantity: stack_amount,
                });
                remaining.quantity -= stack_amount;
            } else {
                break;
            }
        }

        if remaining.quantity > 0 {
            Some(remaining)
        } else {
            None
        }
    }

    /// Remove items from inventory, returns amount actually removed.
    pub fn remove_item(&mut self, item_type: ItemType, mut quantity: u32) -> u32 {
        let mut removed = 0;

        for slot in &mut self.slots {
            if quantity == 0 {
                break;
            }
            if let Some(stack) = slot {
                if stack.item_type == item_type {
                    let took = stack.remove(quantity);
                    removed += took;
                    quantity -= took;

                    // Clear slot if empty
                    if stack.quantity == 0 {
                        *slot = None;
                    }
                }
            }
        }

        removed
    }

    /// Remove item from specific slot, returns the removed stack (if any).
    pub fn remove_slot(&mut self, index: usize) -> Option<ItemStack> {
        if index < INVENTORY_SLOTS {
            self.slots[index].take()
        } else {
            None
        }
    }

    /// Set a slot directly (returns false if out of bounds).
    pub fn set_slot(&mut self, index: usize, stack: Option<ItemStack>) -> bool {
        if index >= INVENTORY_SLOTS {
            return false;
        }
        self.slots[index] = stack;
        true
    }

    /// Move an item stack between slots with Valheim-like behavior:
    /// - If target is empty: move
    /// - If same item type and stackable: stack as much as possible
    /// - Else: swap
    ///
    /// Returns true if any change was made.
    pub fn move_or_stack_slot(&mut self, from: usize, to: usize) -> bool {
        if from >= INVENTORY_SLOTS || to >= INVENTORY_SLOTS || from == to {
            return false;
        }

        let Some(mut from_stack) = self.slots[from].take() else {
            return false;
        };

        match self.slots[to].take() {
            None => {
                self.slots[to] = Some(from_stack);
                true
            }
            Some(mut to_stack) => {
                // Try stacking
                if to_stack.item_type == from_stack.item_type
                    && to_stack.item_type.max_stack_size() > 1
                {
                    let space = to_stack.space_remaining();
                    if space > 0 {
                        let transfer = space.min(from_stack.quantity);
                        to_stack.quantity += transfer;
                        from_stack.quantity -= transfer;

                        self.slots[to] = Some(to_stack);
                        if from_stack.quantity == 0 {
                            self.slots[from] = None;
                        } else {
                            self.slots[from] = Some(from_stack);
                        }
                        true
                    } else {
                        // No space; put them back (no-op)
                        self.slots[to] = Some(to_stack);
                        self.slots[from] = Some(from_stack);
                        false
                    }
                } else {
                    // Swap
                    self.slots[to] = Some(from_stack);
                    self.slots[from] = Some(to_stack);
                    true
                }
            }
        }
    }

    /// Count total quantity of an item type.
    pub fn count_item(&self, item_type: ItemType) -> u32 {
        self.slots
            .iter()
            .filter_map(|s| s.as_ref())
            .filter(|s| s.item_type == item_type)
            .map(|s| s.quantity)
            .sum()
    }

    /// Check if inventory has at least the specified quantity.
    pub fn has_item(&self, item_type: ItemType, quantity: u32) -> bool {
        self.count_item(item_type) >= quantity
    }

    /// Check if inventory is completely empty.
    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(|s| s.is_none())
    }

    /// Get all non-empty slots as (index, stack) pairs.
    pub fn iter_items(&self) -> impl Iterator<Item = (usize, &ItemStack)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| slot.as_ref().map(|s| (i, s)))
    }
}

#[cfg(test)]
mod tests {
    use crate::items::{Inventory, ItemType};

    #[test]
    fn test_inventory_add_item() {
        let mut inv = Inventory::new();

        // Add some items
        let overflow = inv.add_item(ItemType::RifleAmmo, 50);
        assert_eq!(overflow, 0);
        assert_eq!(inv.count_item(ItemType::RifleAmmo), 50);

        // Add more that stacks
        let overflow = inv.add_item(ItemType::RifleAmmo, 20);
        assert_eq!(overflow, 0); // 50 + 20 = 70, split across two slots
        assert_eq!(inv.count_item(ItemType::RifleAmmo), 70);
    }

    #[test]
    fn test_inventory_remove_item() {
        let mut inv = Inventory::new();
        inv.add_item(ItemType::Stone, 50);

        let removed = inv.remove_item(ItemType::Stone, 30);
        assert_eq!(removed, 30);
        assert_eq!(inv.count_item(ItemType::Stone), 20);

        // Try to remove more than exists
        let removed = inv.remove_item(ItemType::Stone, 100);
        assert_eq!(removed, 20);
        assert_eq!(inv.count_item(ItemType::Stone), 0);
    }

    #[test]
    fn test_starting_items() {
        let inv = Inventory::with_starting_items();
        assert_eq!(inv.count_item(ItemType::RifleAmmo), 114);
        assert_eq!(inv.count_item(ItemType::ShotgunShells), 20);
        assert_eq!(inv.count_item(ItemType::SniperRounds), 10);
    }
}
