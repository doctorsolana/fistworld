//! Session account and hero snapshots.
//!
//! This module defines the session profile used to restore a connection's
//! commander state. The running server retains live heroes and possessions;
//! restarting the server deliberately starts a fresh world.

use crate::player::SPAWN_POSITION;
/// Compact account/commander snapshot retained for the running session.
#[derive(Debug, Clone)]
pub struct PlayerProfile {
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
    /// running world and is re-adopted on reconnect. This compact copy supports
    /// session metadata; live body state is authoritative.
    pub hero: Option<HeroSave>,

    // === Progression ===
    /// Player's main level (starts at 0)
    pub level: u32,
    /// Prestige count (starts at 0)
    pub prestige: u32,
    /// Reputation (can be negative)
    pub reputation: i32,
    /// Stamina attribute (scaffold)
    pub stamina: u32,
    /// Intelligence attribute (scaffold)
    pub intelligence: u32,
    /// Charm attribute for the embodied character.
    pub charm: u32,
    /// Global banked gold accessible from bank branches
    pub bank_gold: u64,

    // === Metadata ===
    /// Last login timestamp
    pub last_login: std::time::SystemTime,
    /// Total time played in seconds
    pub total_playtime_secs: u64,
}

/// Compact hero body snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct HeroSave {
    /// Feet position [x, y, z]; Y is re-snapped to terrain on restore.
    pub position: [f32; 3],
    /// Facing yaw in radians.
    pub rotation: f32,
    /// Wardrobe item index per slot (see [`crate::components::HeroOutfit`]).
    pub outfit_slots: [u8; crate::components::HERO_SLOT_MAX],
    /// Skin tone index.
    pub outfit_skin: u8,
    /// Authoritative health at the last durable snapshot.
    pub health_current: f32,
    pub health_max: f32,
}

impl HeroSave {
    pub fn outfit(&self) -> crate::components::HeroOutfit {
        crate::components::HeroOutfit {
            slots: self.outfit_slots,
            skin: self.outfit_skin,
        }
    }

    pub fn health(&self) -> crate::components::Health {
        let max = if self.health_max.is_finite() && self.health_max > 0.0 {
            self.health_max
        } else {
            crate::components::CHARACTER_MAX_HEALTH
        };
        let current = if self.health_current.is_finite() {
            self.health_current.clamp(0.0, max)
        } else {
            max
        };
        crate::components::Health { current, max }
    }

    pub fn from_parts(
        position: bevy::prelude::Vec3,
        rotation: f32,
        outfit: &crate::components::HeroOutfit,
        health: &crate::components::Health,
    ) -> Self {
        Self {
            position: [position.x, position.y, position.z],
            rotation,
            outfit_slots: outfit.slots,
            outfit_skin: outfit.skin,
            health_current: health.current,
            health_max: health.max,
        }
    }
}

impl PlayerProfile {
    /// Create a new player profile with default starting state
    pub fn new_player(name: String) -> Self {
        Self {
            player_name: name,

            // Spawn at default spawn position
            position: SPAWN_POSITION,
            rotation: 0.0,
            hero: None,

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
