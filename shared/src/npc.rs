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

/// Maximum horizontal reach of the procedural anatomical target.
pub const NPC_HUMANOID_HITBOX_REACH: f32 = 0.75;

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
        NpcArchetype::Dummy => 100.0,
        NpcArchetype::CombatDummy => 150.0,
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

// =============================================================================
// RAGDOLL BODY CONTRACT
// =============================================================================
// Single source of truth for the humanoid ragdoll body layout. The server
// spawns physics bodies from it; the client builds the reference Dummy visual
// from it. Offsets are relative to the pelvis (NPC capsule center) in the
// ragdoll spawn frame.

use crate::protocol::RagdollBodyId;

#[derive(Clone, Copy, Debug)]
pub struct RagdollBodyDef {
    pub id: RagdollBodyId,
    pub local_offset: Vec3,
    pub mass: f32,
    pub parent: Option<RagdollBodyId>,
}

/// Shape shared by the combat dummy visual, living hit detection, and
/// authoritative ragdoll physics.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HumanoidBodyShape {
    Sphere { radius: f32 },
    Capsule { half_segment: f32, radius: f32 },
    Cuboid { half_extents: Vec3 },
}

/// Anatomical collider dimensions for one humanoid body.
pub fn humanoid_body_shape(id: RagdollBodyId) -> HumanoidBodyShape {
    use RagdollBodyId as B;
    match id {
        B::Pelvis => HumanoidBodyShape::Cuboid {
            half_extents: Vec3::new(0.16, 0.11, 0.12),
        },
        B::SpineLower => HumanoidBodyShape::Capsule {
            half_segment: 0.10,
            radius: 0.15,
        },
        B::SpineUpper => HumanoidBodyShape::Capsule {
            half_segment: 0.12,
            radius: 0.18,
        },
        B::Head => HumanoidBodyShape::Sphere { radius: 0.13 },
        B::UpperArmL | B::UpperArmR => HumanoidBodyShape::Capsule {
            half_segment: 0.12,
            radius: 0.075,
        },
        B::ForearmL | B::ForearmR => HumanoidBodyShape::Capsule {
            half_segment: 0.14,
            radius: 0.065,
        },
        B::HandL | B::HandR => HumanoidBodyShape::Capsule {
            half_segment: 0.06,
            radius: 0.045,
        },
        B::ThighL | B::ThighR => HumanoidBodyShape::Capsule {
            half_segment: 0.17,
            radius: 0.095,
        },
        B::CalfL | B::CalfR => HumanoidBodyShape::Capsule {
            half_segment: 0.15,
            radius: 0.08,
        },
        B::FootL | B::FootR => HumanoidBodyShape::Cuboid {
            half_extents: Vec3::new(0.065, 0.045, 0.13),
        },
    }
}

pub fn humanoid_body_bounding_radius(shape: HumanoidBodyShape) -> f32 {
    match shape {
        HumanoidBodyShape::Sphere { radius } => radius,
        HumanoidBodyShape::Capsule {
            half_segment,
            radius,
        } => half_segment + radius,
        HumanoidBodyShape::Cuboid { half_extents } => half_extents.length(),
    }
}

pub const HUMANOID_RAGDOLL_BODIES: [RagdollBodyDef; 12] = [
    RagdollBodyDef {
        id: RagdollBodyId::Pelvis,
        local_offset: Vec3::new(0.0, 0.0, 0.0),
        mass: 13.0,
        parent: None,
    },
    RagdollBodyDef {
        id: RagdollBodyId::SpineLower,
        local_offset: Vec3::new(0.0, 0.20, 0.0),
        mass: 8.0,
        parent: Some(RagdollBodyId::Pelvis),
    },
    RagdollBodyDef {
        id: RagdollBodyId::SpineUpper,
        local_offset: Vec3::new(0.0, 0.42, 0.0),
        mass: 7.0,
        parent: Some(RagdollBodyId::SpineLower),
    },
    RagdollBodyDef {
        id: RagdollBodyId::Head,
        local_offset: Vec3::new(0.0, 0.74, 0.0),
        mass: 5.0,
        parent: Some(RagdollBodyId::SpineUpper),
    },
    RagdollBodyDef {
        id: RagdollBodyId::UpperArmL,
        local_offset: Vec3::new(-0.24, 0.44, 0.0),
        mass: 3.0,
        parent: Some(RagdollBodyId::SpineUpper),
    },
    RagdollBodyDef {
        id: RagdollBodyId::UpperArmR,
        local_offset: Vec3::new(0.24, 0.44, 0.0),
        mass: 3.0,
        parent: Some(RagdollBodyId::SpineUpper),
    },
    RagdollBodyDef {
        id: RagdollBodyId::ForearmL,
        local_offset: Vec3::new(-0.48, 0.40, 0.0),
        mass: 2.5,
        parent: Some(RagdollBodyId::UpperArmL),
    },
    RagdollBodyDef {
        id: RagdollBodyId::ForearmR,
        local_offset: Vec3::new(0.48, 0.40, 0.0),
        mass: 2.5,
        parent: Some(RagdollBodyId::UpperArmR),
    },
    RagdollBodyDef {
        id: RagdollBodyId::ThighL,
        local_offset: Vec3::new(-0.11, -0.33, 0.0),
        mass: 5.0,
        parent: Some(RagdollBodyId::Pelvis),
    },
    RagdollBodyDef {
        id: RagdollBodyId::ThighR,
        local_offset: Vec3::new(0.11, -0.33, 0.0),
        mass: 5.0,
        parent: Some(RagdollBodyId::Pelvis),
    },
    RagdollBodyDef {
        id: RagdollBodyId::CalfL,
        local_offset: Vec3::new(-0.11, -0.73, 0.0),
        mass: 4.0,
        parent: Some(RagdollBodyId::ThighL),
    },
    RagdollBodyDef {
        id: RagdollBodyId::CalfR,
        local_offset: Vec3::new(0.11, -0.73, 0.0),
        mass: 4.0,
        parent: Some(RagdollBodyId::ThighR),
    },
];

/// Offset of a body's parent in the shared table (pelvis-relative frame).
pub fn ragdoll_body_parent_offset(def: &RagdollBodyDef) -> Option<Vec3> {
    def.parent.and_then(|parent| {
        HUMANOID_RAGDOLL_BODIES
            .iter()
            .find(|d| d.id == parent)
            .map(|d| d.local_offset)
    })
}

/// The body's long axis in the spawn frame (parent -> body direction).
/// Physics bodies and the Dummy visual both orient their capsules along it.
pub fn ragdoll_body_axis(def: &RagdollBodyDef) -> Vec3 {
    if let Some(parent_offset) = ragdoll_body_parent_offset(def) {
        let axis = def.local_offset - parent_offset;
        if axis.length_squared() > 1.0e-6 {
            return axis.normalize();
        }
    }
    Vec3::Y
}
