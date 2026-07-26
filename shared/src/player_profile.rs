//! Player persistence - serializable player profile data
//!
//! This module defines the PlayerProfile structure used to save/load player state
//! across disconnects and server restarts. Uses bincode serialization like the
//! collider baker system.

use crate::items::{ItemStack, ItemType, INVENTORY_SLOTS};
use crate::player::SPAWN_POSITION;
use crate::vehicle::VehicleType;
use crate::weapons::WeaponType;
use serde::{Deserialize, Serialize};

/// Current profile version for migration support
pub const PROFILE_VERSION: u32 = 1;

/// Serializable player profile containing all persistent state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerProfile {
    /// Profile format version for migration
    pub version: u32,

    /// Player's chosen name (permanent, case-insensitive unique)
    pub player_name: String,

    // === Position State ===
    /// World position [x, y, z]
    pub position: [f32; 3],
    /// Yaw rotation in radians
    pub rotation: f32,
    /// Movement velocity [x, y, z]
    pub velocity: [f32; 3],

    // === Combat State ===
    /// Current health points
    pub health_current: f32,
    /// Maximum health points
    pub health_max: f32,
    /// Currently equipped weapon type
    pub equipped_weapon: WeaponType,
    /// Ammo loaded in equipped weapon's magazine
    pub weapon_ammo_in_mag: u32,

    // === Inventory ===
    /// 24-slot inventory (None = empty slot)
    pub inventory_slots: [Option<ItemStack>; INVENTORY_SLOTS],
    /// Active hotbar slot index (0-5)
    pub hotbar_selection: u8,

    // === Vehicle State ===
    /// Whether player was in a vehicle when they disconnected
    pub in_vehicle: bool,
    /// Type of vehicle (if in_vehicle == true)
    pub vehicle_type: Option<VehicleType>,
    /// Vehicle world position [x, y, z]
    pub vehicle_position: Option<[f32; 3]>,
    /// Vehicle orientation [heading, pitch, roll] in radians
    pub vehicle_rotation: Option<[f32; 3]>,
    /// Vehicle linear velocity [x, y, z]
    pub vehicle_velocity: Option<[f32; 3]>,
    /// Vehicle angular velocity [yaw, pitch, roll] in radians/sec
    pub vehicle_angular_velocity: Option<[f32; 3]>,

    // === Death State ===
    /// Whether player is currently dead (awaiting respawn)
    pub is_dead: bool,
    /// Timestamp when player died (for analytics)
    pub death_timestamp: Option<f64>,

    // === Progression ===
    /// Player's main level (starts at 0)
    #[serde(default)]
    pub level: u32,
    /// Prestige count (starts at 0)
    #[serde(default)]
    pub prestige: u32,
    /// Reputation (can be negative)
    #[serde(default)]
    pub reputation: i32,
    /// Stamina attribute (scaffold)
    #[serde(default)]
    pub stamina: u32,
    /// Intelligence attribute (scaffold)
    #[serde(default)]
    pub intelligence: u32,
    /// Global banked gold accessible from bank branches
    #[serde(default)]
    pub bank_gold: u64,

    // === Metadata ===
    /// Last login timestamp
    pub last_login: std::time::SystemTime,
    /// Total time played in seconds
    pub total_playtime_secs: u64,
}

impl PlayerProfile {
    /// Create a new player profile with default starting state
    pub fn new_player(name: String) -> Self {
        // Get starting inventory slots from Inventory::with_starting_items()
        // We'll construct this manually since we can't call the method directly
        let mut inventory_slots = [None; INVENTORY_SLOTS];

        // Slot 0: Assault Rifle with full mag (30 rounds)
        inventory_slots[0] = Some(ItemStack {
            item_type: ItemType::Weapon(WeaponType::AssaultRifle),
            quantity: 1,
            ammo_in_mag: Some(30),
        });

        // Slot 1: Sword, Slot 2: Shield (shield rides the off-hand while a
        // one-handed weapon is selected — hold RMB to block).
        inventory_slots[1] = Some(ItemStack {
            item_type: ItemType::Weapon(WeaponType::Sword),
            quantity: 1,
            ammo_in_mag: Some(0),
        });
        inventory_slots[2] = Some(ItemStack {
            item_type: ItemType::Weapon(WeaponType::Shield),
            quantity: 1,
            ammo_in_mag: Some(0),
        });

        // Slots 3-5 (rest of the hotbar): ammo
        inventory_slots[3] = Some(ItemStack {
            item_type: ItemType::RifleAmmo,
            quantity: 30,
            ammo_in_mag: None,
        });
        inventory_slots[4] = Some(ItemStack {
            item_type: ItemType::ShotgunShells,
            quantity: 20,
            ammo_in_mag: None,
        });
        inventory_slots[5] = Some(ItemStack {
            item_type: ItemType::RifleAmmo,
            quantity: 30,
            ammo_in_mag: None,
        });

        // Backpack: more reserve ammo
        inventory_slots[6] = Some(ItemStack {
            item_type: ItemType::RifleAmmo,
            quantity: 54,
            ammo_in_mag: None,
        });
        inventory_slots[7] = Some(ItemStack {
            item_type: ItemType::SniperRounds,
            quantity: 10,
            ammo_in_mag: None,
        });

        Self {
            version: PROFILE_VERSION,
            player_name: name,

            // Spawn at default spawn position
            position: SPAWN_POSITION,
            rotation: 0.0,
            velocity: [0.0, 0.0, 0.0],

            // Default combat stats
            health_current: 100.0,
            health_max: 100.0,
            equipped_weapon: WeaponType::AssaultRifle,
            weapon_ammo_in_mag: 30,

            // Starting inventory
            inventory_slots,
            hotbar_selection: 0,

            // Not in vehicle
            in_vehicle: false,
            vehicle_type: None,
            vehicle_position: None,
            vehicle_rotation: None,
            vehicle_velocity: None,
            vehicle_angular_velocity: None,

            // Not dead
            is_dead: false,
            death_timestamp: None,

            // Progression defaults
            level: 0,
            prestige: 0,
            reputation: 0,
            stamina: 0,
            intelligence: 0,
            bank_gold: 0,

            // Metadata
            last_login: std::time::SystemTime::now(),
            total_playtime_secs: 0,
        }
    }
}
