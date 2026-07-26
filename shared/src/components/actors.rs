use bevy::prelude::*;
use lightyear::prelude::PeerId;
use serde::{Deserialize, Serialize};

/// Marker component for player entities.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Player {
    pub client_id: PeerId,
}

/// Player progression data (replicated + persisted).
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct PlayerProgression {
    /// Main level (starts at 0)
    pub level: u32,
    /// Prestige count (starts at 0)
    pub prestige: u32,
    /// Reputation can be negative or positive
    pub reputation: i32,
    /// Stamina attribute (scaffold-only)
    pub stamina: u32,
    /// Intelligence attribute (scaffold-only)
    pub intelligence: u32,
}

/// Which player character model to use on the client.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PlayerCharacter {
    #[default]
    Oilman,
    Base,
}

/// Player position component - replicated across network.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct PlayerPosition(pub Vec3);

/// Player rotation (yaw only for simplicity) - replicated across network.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct PlayerRotation(pub f32);

/// Player velocity (server-authoritative). Not replicated right now.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct PlayerVelocity(pub Vec3);

/// Player grounded state (server-authoritative)
/// Computed each tick based on terrain proximity and static collider contacts.
#[derive(Component, Clone, Debug, Default)]
pub struct PlayerGrounded {
    /// Whether player is on terrain
    pub on_terrain: bool,
    /// Whether player is on a static collider (prop/structure)
    pub on_static: bool,
    /// Time since last grounded (for coyote time)
    pub time_since_grounded: f32,
}

/// Server-authoritative water state (replicated).
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct PlayerWaterState {
    pub in_water: bool,
    pub surface_y: f32,
    /// Water surface depth below the player's capsule center.
    pub depth: f32,
}

/// Server-authoritative jump animation state (replicated).
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct PlayerJumpState {
    /// Seconds remaining to show jump animation.
    pub timer: f32,
}

/// Debug fly mode (server-authoritative).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct FlyMode;

impl PlayerGrounded {
    /// Small grace period for jumping after leaving ground.
    pub const COYOTE_TIME: f32 = 0.1;

    /// Check if player can jump (grounded or within coyote time).
    pub fn can_jump(&self) -> bool {
        self.is_grounded() || self.time_since_grounded < Self::COYOTE_TIME
    }

    /// Check if player is grounded on anything.
    pub fn is_grounded(&self) -> bool {
        self.on_terrain || self.on_static
    }
}

/// Marker for the local player (client-side only).
#[derive(Component)]
pub struct LocalPlayer;

/// Marker for ground/terrain.
#[derive(Component)]
pub struct Ground;
