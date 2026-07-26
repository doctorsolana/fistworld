use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::weapons::WeaponType;

/// Health component for damageable entities.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Health {
    pub current: f32,
    pub max: f32,
}

impl Default for Health {
    fn default() -> Self {
        Self {
            current: 100.0,
            max: 100.0,
        }
    }
}

impl Health {
    pub fn new(max: f32) -> Self {
        Self { current: max, max }
    }

    pub fn take_damage(&mut self, amount: f32) -> bool {
        self.current = (self.current - amount).max(0.0);
        self.current <= 0.0
    }

    pub fn heal(&mut self, amount: f32) {
        self.current = (self.current + amount).min(self.max);
    }

    pub fn is_dead(&self) -> bool {
        self.current <= 0.0
    }

    pub fn percentage(&self) -> f32 {
        self.current / self.max
    }
}

/// Currently equipped weapon.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct EquippedWeapon {
    pub weapon_type: WeaponType,
    pub ammo_in_mag: u32,
    /// Time of last shot (game time in seconds)
    pub last_fire_time: f32,
    /// Whether currently aiming down sights
    pub aiming: bool,
    /// Whether the shield is raised (server-stamped from PlayerInput).
    pub blocking: bool,
    /// A shield sits in a non-selected hotbar slot while a one-handed
    /// weapon is equipped — rendered in the left hand, enables blocking.
    pub offhand_shield: bool,
}

impl Default for EquippedWeapon {
    fn default() -> Self {
        let weapon = WeaponType::default();
        let stats = weapon.stats();
        Self {
            weapon_type: weapon,
            ammo_in_mag: stats.magazine_size,
            last_fire_time: -10.0, // Allow immediate first shot
            aiming: false,
            blocking: false,
            offhand_shield: false,
        }
    }
}

impl EquippedWeapon {
    pub fn new(weapon_type: WeaponType) -> Self {
        let stats = weapon_type.stats();
        Self {
            weapon_type,
            ammo_in_mag: stats.magazine_size,
            last_fire_time: -10.0,
            aiming: false,
            blocking: false,
            offhand_shield: false,
        }
    }

    /// Can this loadout block right now? A shield blocks in the main hand,
    /// or from the off-hand alongside a one-handed weapon (sword, revolver,
    /// bare fists). Two-handed guns can't pair with a shield.
    pub fn can_block(&self) -> bool {
        self.weapon_type.is_shield()
            || (self.offhand_shield
                && matches!(
                    self.weapon_type,
                    WeaponType::Sword | WeaponType::Unarmed | WeaponType::Pistol
                ))
    }

    /// Check if weapon can fire (has ammo and cooldown passed).
    pub fn can_fire(&self, current_time: f32) -> bool {
        let cooldown = self.weapon_type.fire_cooldown();
        self.ammo_in_mag > 0 && (current_time - self.last_fire_time) >= cooldown
    }

    /// Fire the weapon, consuming ammo.
    pub fn fire(&mut self, current_time: f32) -> bool {
        if self.can_fire(current_time) {
            self.ammo_in_mag -= 1;
            self.last_fire_time = current_time;
            true
        } else {
            false
        }
    }

    /// Reload from inventory, returns amount of ammo consumed from inventory.
    pub fn reload_from_inventory(&mut self, inventory: &mut crate::items::Inventory) -> u32 {
        let stats = self.weapon_type.stats();
        let ammo_type = self.weapon_type.ammo_type();
        let needed = stats.magazine_size - self.ammo_in_mag;

        if needed == 0 {
            return 0;
        }

        // Take ammo from inventory
        let taken = inventory.remove_item(ammo_type, needed);
        self.ammo_in_mag += taken;
        taken
    }

    /// Check how much reserve ammo is available in inventory.
    pub fn get_reserve_from_inventory(&self, inventory: &crate::items::Inventory) -> u32 {
        inventory.count_item(self.weapon_type.ammo_type())
    }

    /// Get current spread based on aiming state.
    pub fn current_spread(&self) -> f32 {
        let stats = self.weapon_type.stats();
        if self.aiming {
            stats.spread_ads
        } else {
            stats.spread_hip
        }
    }
}

/// Bullet entity component - server authoritative, replicated to clients.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Bullet {
    /// Client ID of the shooter
    pub owner_id: u64,
    /// Weapon that fired this bullet
    pub weapon_type: WeaponType,
    /// Where the bullet was spawned (for damage falloff calculation)
    pub spawn_position: Vec3,
    /// Initial velocity at spawn (for debug visualization / deterministic re-sim)
    pub initial_velocity: Vec3,
    /// When the bullet was spawned (game time)
    pub spawn_time: f32,
}

/// Bullet velocity component - updated each physics tick.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct BulletVelocity(pub Vec3);

/// Previous position for hit detection (raycast from prev to current).
#[derive(Component, Clone, Debug, Default)]
pub struct BulletPrevPosition(pub Vec3);

/// Marker for local tracer visuals (client-side only, not replicated).
#[derive(Component)]
pub struct LocalTracer {
    pub spawn_time: f32,
    pub lifetime: f32,
}

/// Event component to signal that an NPC took damage (server-side only, not replicated).
/// This is added temporarily and consumed by the AI system.
#[derive(Component, Clone, Debug)]
pub struct NpcDamageEvent {
    pub damage_source_position: Vec3,
    pub damage_amount: f32,
    pub attacker_player_id: Option<u64>,
    pub hit_zone: crate::weapons::damage::HitZone,
    pub body_part: Option<crate::weapons::damage::HitBodyPart>,
}
