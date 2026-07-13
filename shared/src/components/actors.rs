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

/// Which NPC character model to use on the client.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum NpcArchetype {
    #[default]
    Oilman,
    DesertOutpost,
    /// Untextured reference dummy whose visual is built directly from the
    /// ragdoll body definitions (no skeleton/bind-pose mapping) — ground
    /// truth for diagnosing ragdoll issues.
    Dummy,
}

/// Which player character model to use on the client.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PlayerCharacter {
    #[default]
    Oilman,
    Base,
}

/// Marker component for NPC entities (server authoritative, replicated to clients)
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Npc {
    pub id: u64,
    pub archetype: NpcArchetype,
}

/// High-level server-authoritative NPC intent/state for animation and behavior.
///
/// This is intentionally semantic and stable (what the NPC is doing), not low-level
/// motion-derived state.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum NpcActivityKind {
    #[default]
    Idle,
    Walk,
    Run,
    Sit,
    Work,
    Talk,
    Sleep,
    Flee,
    Dead,
}

/// Replicated high-level activity state for NPCs.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct NpcActivity(pub NpcActivityKind);

/// Identity data for NPCs (replicated).
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct NpcIdentity {
    pub name: String,
    pub occupation: String,
    pub faction: Option<String>,
}

/// NPC position component - replicated across network.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct NpcPosition(pub Vec3);

/// NPC rotation (yaw) - replicated across network.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct NpcRotation(pub f32);

/// NPC linear velocity (server-authoritative, replicated for smoothing/telemetry).
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct NpcVelocity(pub Vec3);

/// Marker for NPCs that are fleeing (replicated across network).
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct NpcFleeing;

/// Replicated debug rigidbody box entity.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DebugPhysicsBox {
    pub id: u64,
    pub half_extents: Vec3,
}

/// Replicated debug box position.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct DebugPhysicsBoxPosition(pub Vec3);

/// Replicated debug box rotation.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct DebugPhysicsBoxRotation(pub Quat);

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
