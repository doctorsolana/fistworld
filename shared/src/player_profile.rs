//! Player persistence - serializable player profile data
//!
//! This module defines the PlayerProfile structure used to save/load player state
//! across disconnects and server restarts. Uses bincode serialization like the
//! collider baker system.

use crate::player::SPAWN_POSITION;
use serde::{Deserialize, Serialize};

/// Current profile version for migration support
pub const PROFILE_VERSION: u32 = 7;

/// Serializable player profile containing all persistent state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerProfile {
    /// Profile format version for migration
    pub version: u32,

    /// Player's chosen name (permanent, case-insensitive unique)
    pub player_name: String,

    // === Commander view ===
    // NOT a body. Since the RTS pivot the commander is a bodiless camera, so
    // these restore where the player was LOOKING. The player's actual body is
    // `hero` below.
    /// Camera focus point [x, y, z].
    pub position: [f32; 3],
    /// Camera yaw in radians.
    pub rotation: f32,

    // === Hero ===
    /// The player's embodied character, if they have one.
    ///
    /// A hero is not lost by logging off: the entity stays standing in the
    /// world (the world lives without players — WORLD-DESIGN pillar 1) and is
    /// re-adopted on reconnect. This copy is what restores it after a SERVER
    /// RESTART, when no entities survive.
    pub hero: Option<HeroSave>,

    // === Combat State ===

    // === Inventory ===

    // === Death State ===

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
    /// Charm attribute for the embodied character.
    #[serde(default)]
    pub charm: u32,
    /// Global banked gold accessible from bank branches
    #[serde(default)]
    pub bank_gold: u64,

    // === Metadata ===
    /// Last login timestamp
    pub last_login: std::time::SystemTime,
    /// Total time played in seconds
    pub total_playtime_secs: u64,
}

/// Persisted hero body.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HeroSave {
    /// Feet position [x, y, z]; Y is re-snapped to terrain on restore.
    pub position: [f32; 3],
    /// Facing yaw in radians.
    pub rotation: f32,
    /// Wardrobe item index per slot (see [`crate::components::HeroOutfit`]).
    pub outfit_slots: [u8; crate::components::HERO_SLOT_MAX],
    /// Skin tone index.
    pub outfit_skin: u8,
}

impl HeroSave {
    pub fn outfit(&self) -> crate::components::HeroOutfit {
        crate::components::HeroOutfit {
            slots: self.outfit_slots,
            skin: self.outfit_skin,
        }
    }

    pub fn from_parts(
        position: bevy::prelude::Vec3,
        rotation: f32,
        outfit: &crate::components::HeroOutfit,
    ) -> Self {
        Self {
            position: [position.x, position.y, position.z],
            rotation,
            outfit_slots: outfit.slots,
            outfit_skin: outfit.skin,
        }
    }
}

impl PlayerProfile {
    /// Create a new player profile with default starting state
    pub fn new_player(name: String) -> Self {
        Self {
            version: PROFILE_VERSION,
            player_name: name,

            // Spawn at default spawn position
            position: SPAWN_POSITION,
            rotation: 0.0,
            hero: None,

            // Not dead

            // Progression defaults
            level: 0,
            prestige: 0,
            reputation: 0,
            stamina: 10,
            intelligence: 10,
            charm: 10,
            bank_gold: 0,

            // Metadata
            last_login: std::time::SystemTime::now(),
            total_playtime_secs: 0,
        }
    }

    pub fn character_attributes(&self) -> crate::components::CharacterAttributes {
        crate::components::CharacterAttributes::new(
            self.stamina.min(100) as u8,
            self.intelligence.min(100) as u8,
            self.charm.min(100) as u8,
        )
    }
}
