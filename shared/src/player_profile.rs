//! Player persistence - serializable player profile data
//!
//! This module defines the PlayerProfile structure used to save/load player state
//! across disconnects and server restarts. Uses bincode serialization like the
//! collider baker system.

use crate::player::SPAWN_POSITION;
use serde::{Deserialize, Serialize};

/// Current profile version for migration support
pub const PROFILE_VERSION: u32 = 5;

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
        Self {
            version: PROFILE_VERSION,
            player_name: name,

            // Spawn at default spawn position
            position: SPAWN_POSITION,
            rotation: 0.0,



            // Not dead

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
