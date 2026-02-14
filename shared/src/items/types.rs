use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::weapons::WeaponType;

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
    // Weapons (non-stackable)
    Weapon(WeaponType),
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
            ItemType::Weapon(_) => 1,
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
            ItemType::Weapon(w) => match w {
                WeaponType::Unarmed => "Unarmed",
                WeaponType::Pistol => "Revolver",
                WeaponType::AssaultRifle => "Automatic Rifle",
                WeaponType::Sniper => "Sniper Rifle",
                WeaponType::Shotgun => "Shotgun",
            },
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
            ItemType::Weapon(w) => match w {
                WeaponType::Unarmed => Color::srgb(0.6, 0.6, 0.6),
                WeaponType::Pistol => Color::srgb(0.55, 0.55, 0.6),
                WeaponType::AssaultRifle => Color::srgb(0.25, 0.75, 0.35),
                WeaponType::Shotgun => Color::srgb(0.8, 0.55, 0.25),
                WeaponType::Sniper => Color::srgb(0.75, 0.25, 0.55),
            },
        }
    }

    /// If this item is a weapon, return its WeaponType.
    pub fn as_weapon_type(&self) -> Option<WeaponType> {
        match self {
            ItemType::Weapon(w) => Some(*w),
            _ => None,
        }
    }
}

/// A stack of items (type + quantity).
/// For weapon items, also tracks the ammo currently loaded in the magazine.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ItemStack {
    pub item_type: ItemType,
    pub quantity: u32,
    /// For weapon items only: ammo currently in the magazine.
    /// None means the weapon has a full magazine (or this isn't a weapon).
    pub ammo_in_mag: Option<u32>,
}

impl ItemStack {
    pub fn new(item_type: ItemType, quantity: u32) -> Self {
        Self {
            item_type,
            quantity,
            ammo_in_mag: None,
        }
    }

    /// Create a weapon item with specified magazine ammo.
    pub fn new_weapon(weapon_type: WeaponType, ammo_in_mag: u32) -> Self {
        Self {
            item_type: ItemType::Weapon(weapon_type),
            quantity: 1,
            ammo_in_mag: Some(ammo_in_mag),
        }
    }

    /// Create a weapon item with a full magazine.
    pub fn new_weapon_full_mag(weapon_type: WeaponType) -> Self {
        let mag_size = weapon_type.stats().magazine_size;
        Self::new_weapon(weapon_type, mag_size)
    }

    /// Get the ammo in mag for a weapon (returns full mag size if not set).
    pub fn get_weapon_ammo(&self) -> u32 {
        if let ItemType::Weapon(w) = self.item_type {
            self.ammo_in_mag.unwrap_or_else(|| w.stats().magazine_size)
        } else {
            0
        }
    }

    /// Set the ammo in mag for a weapon.
    pub fn set_weapon_ammo(&mut self, ammo: u32) {
        if self.item_type.as_weapon_type().is_some() {
            self.ammo_in_mag = Some(ammo);
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
