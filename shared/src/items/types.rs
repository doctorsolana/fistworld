use bevy::prelude::*;
use serde::{Deserialize, Serialize};


/// All item types in the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ItemType {
    // Ammo types
    #[default]
    RifleAmmo,
    ShotgunShells,
    SniperRounds,
    // Resources
    Stone,
    Wood,
    GoldCoin,
}

impl ItemType {
    /// Get the maximum stack size for this item type.
    pub fn max_stack_size(&self) -> u32 {
        match self {
            // Ammo stacks to 60
            ItemType::RifleAmmo => 60,
            ItemType::ShotgunShells => 60,
            ItemType::SniperRounds => 60,
            // Resources stack to 100
            ItemType::Stone => 100,
            ItemType::Wood => 100,
            ItemType::GoldCoin => 10000,
            // Weapons are non-stackable
        }
    }

    /// Get display name for this item.
    pub fn display_name(&self) -> &'static str {
        match self {
            ItemType::RifleAmmo => "Bullets",
            ItemType::ShotgunShells => "Shotgun Shells",
            ItemType::SniperRounds => "Sniper Rounds",
            ItemType::Stone => "Stone",
            ItemType::Wood => "Wood",
            ItemType::GoldCoin => "Gold Coins",
        }
    }

    /// Get color for UI/visuals.
    pub fn color(&self) -> Color {
        match self {
            ItemType::RifleAmmo => Color::srgb(0.8, 0.6, 0.2), // Brass
            ItemType::ShotgunShells => Color::srgb(0.9, 0.2, 0.2), // Red
            ItemType::SniperRounds => Color::srgb(0.2, 0.6, 0.2), // Green tip
            ItemType::Stone => Color::srgb(0.5, 0.5, 0.5),     // Gray
            ItemType::Wood => Color::srgb(0.6, 0.4, 0.2),      // Brown
            ItemType::GoldCoin => Color::srgb(0.96, 0.81, 0.18), // Gold
        }
    }
}

/// A stack of items (type + quantity).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ItemStack {
    pub item_type: ItemType,
    pub quantity: u32,
}

impl ItemStack {
    pub fn new(item_type: ItemType, quantity: u32) -> Self {
        Self {
            item_type,
            quantity,
        }
    }

    /// Check if this stack can accept more of the same item type.
    pub fn can_add(&self, amount: u32) -> bool {
        self.quantity + amount <= self.item_type.max_stack_size()
    }

    /// How much more can this stack hold?
    pub fn space_remaining(&self) -> u32 {
        self.item_type
            .max_stack_size()
            .saturating_sub(self.quantity)
    }

    /// Add to this stack, returns amount that couldn't fit.
    pub fn add(&mut self, amount: u32) -> u32 {
        let can_add = self.space_remaining().min(amount);
        self.quantity += can_add;
        amount - can_add
    }

    /// Remove from this stack, returns amount actually removed.
    pub fn remove(&mut self, amount: u32) -> u32 {
        let can_remove = self.quantity.min(amount);
        self.quantity -= can_remove;
        can_remove
    }
}
