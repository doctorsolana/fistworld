//! Client-side NPC visuals, animation, and debug rendering.

pub mod animation;
pub mod assets;
pub mod debug;
pub mod spawn;
pub mod state;
pub mod sync;
pub mod visibility;

pub use animation::update_npc_animation;
pub use assets::setup_npc_assets;
pub use debug::update_npc_hitbox_debug_gizmos;
pub use spawn::{handle_npc_spawned, setup_npc_rig};
pub use state::*;
pub use sync::sync_npc_transforms;
pub use visibility::{
    apply_double_sided_npc_materials, apply_npc_no_frustum_culling_to_new_meshes,
    apply_npc_shadow_state_to_new_meshes, update_npc_visibility,
};

use bevy::animation::graph::{AnimationGraph, AnimationGraphHandle, AnimationNodeIndex};
use bevy::animation::{AnimatedBy, AnimationClip, AnimationPlayer, AnimationTargetId};
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use shared::components::{
    Health, LocalPlayer, Npc, NpcFleeing, NpcPosition, NpcRotation, PlayerPosition,
};
use shared::npc::{
    npc_capsule_endpoints, npc_head_center, NPC_HEAD_RADIUS, NPC_HEIGHT, NPC_RADIUS,
};
use shared::terrain::CHUNK_SIZE;
use shared::weapons::WeaponDebugMode;

use super::GraphicsSettings;
