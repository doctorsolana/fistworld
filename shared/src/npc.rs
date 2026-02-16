//! NPC-related constants and helper utilities.

use bevy::prelude::*;

use crate::components::NpcArchetype;
use crate::player::{PLAYER_HEIGHT, PLAYER_RADIUS};

// =============================================================================
// NPC GEOMETRY
// =============================================================================

/// NPC capsule height (shared humanoid rig).
pub const NPC_HEIGHT: f32 = PLAYER_HEIGHT;

/// NPC capsule radius.
pub const NPC_RADIUS: f32 = PLAYER_RADIUS;

/// Head hitbox radius.
pub const NPC_HEAD_RADIUS: f32 = 0.18;

/// Returns the approximate head hitbox center for an upright NPC.
///
/// `npc_center` is the NPC's capsule *center* position.
#[inline]
pub fn npc_head_center(npc_center: Vec3) -> Vec3 {
    // Slightly below the capsule top.
    npc_center + Vec3::new(0.0, NPC_HEIGHT * 0.5 - NPC_HEAD_RADIUS, 0.0)
}

/// Returns endpoints (sphere centers) for an upright capsule representing the NPC body.
///
/// `npc_center` is the NPC's capsule *center* position.
#[inline]
pub fn npc_capsule_endpoints(npc_center: Vec3) -> (Vec3, Vec3) {
    let half = NPC_HEIGHT * 0.5;
    let a = npc_center + Vec3::new(0.0, -(half - NPC_RADIUS), 0.0);
    let b = npc_center + Vec3::new(0.0, half - NPC_RADIUS, 0.0);
    (a, b)
}

// =============================================================================
// NPC MOVEMENT CONSTANTS
// =============================================================================

/// NPC walking speed in meters per second.
pub const NPC_MOVE_SPEED: f32 = 3.5;

/// NPC rotation speed in radians per second for smooth turning.
pub const NPC_TURN_SPEED: f32 = 6.0;

/// Maximum distance NPCs can wander from their home position.
pub const NPC_WANDER_RADIUS: f32 = 60.0;

/// Minimum idle time in seconds after reaching a wander target.
pub const NPC_IDLE_TIME_MIN: f32 = 1.5;

/// Maximum idle time in seconds after reaching a wander target.
pub const NPC_IDLE_TIME_MAX: f32 = 4.0;

/// Minimum distance for selecting a new wander target.
pub const NPC_MIN_TARGET_DIST: f32 = 15.0;

/// Time in seconds before a dead NPC despawns.
pub const DEAD_NPC_DESPAWN_TIME: f32 = 60.0;

// =============================================================================
// NPC HEALTH
// =============================================================================

/// Returns the maximum health for an NPC based on archetype.
pub fn npc_max_health(archetype: NpcArchetype) -> f32 {
    match archetype {
        NpcArchetype::Oilman => 100.0,
        NpcArchetype::DesertOutpost => 100.0,
    }
}

// =============================================================================
// NPC IDENTITY
// =============================================================================

const NPC_FIRST_NAMES: &[&str] = &[
    "Alden", "Bran", "Cass", "Dara", "Elias", "Faye", "Galen", "Hale", "Iris", "Joren", "Kara",
    "Lysa", "Mira", "Niko", "Orin", "Perrin", "Quinn", "Rhea", "Soren", "Tara", "Ulric", "Vera",
    "Wren", "Xara", "Yara", "Zane",
];

const NPC_LAST_NAMES: &[&str] = &[
    "Ashford",
    "Briar",
    "Caldwell",
    "Dunlow",
    "Ember",
    "Fallow",
    "Graves",
    "Harrow",
    "Iron",
    "Junewick",
    "Kestrel",
    "Locke",
    "Marrow",
    "Nighthill",
    "Oakley",
    "Pryce",
    "Quarry",
    "Rowan",
    "Slate",
    "Thorne",
    "Umber",
    "Vale",
    "Ward",
    "Xanthis",
    "Yew",
    "Zephyr",
];

#[inline]
fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Deterministic NPC name from world seed + NPC id.
pub fn npc_name_for_id(seed: u32, id: u64) -> String {
    let base = splitmix64(seed as u64 ^ id);
    let first = NPC_FIRST_NAMES[(base as usize) % NPC_FIRST_NAMES.len()];
    let last = NPC_LAST_NAMES[((base >> 32) as usize) % NPC_LAST_NAMES.len()];
    format!("{first} {last}")
}
