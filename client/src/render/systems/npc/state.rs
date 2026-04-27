//! NPC render-system state and shared constants.

use bevy::animation::graph::{AnimationGraph, AnimationNodeIndex};
use bevy::prelude::*;
use shared::protocol::RagdollBodyId;
use std::collections::HashMap;

/// Loaded NPC scene and animation graph resources.
#[derive(Resource, Clone)]
pub struct NpcAssets {
    pub scene: Handle<Scene>,
    pub animation_graph: Handle<AnimationGraph>,
    // Oilman animation clips
    pub idle_node: AnimationNodeIndex,
    pub jog_forward_node: AnimationNodeIndex,
    pub running_node: AnimationNodeIndex,
    pub look_behind_run_node: AnimationNodeIndex,
}

#[derive(Component)]
pub struct NpcModelRoot;

#[derive(Component)]
pub struct NeedsNpcRigSetup;

/// Marker for NPC models that need double-sided materials (custom GLB).
#[derive(Component)]
pub struct NeedsDoubleSidedMaterials;

#[derive(Component)]
pub struct NpcAnimationRoot;

#[derive(Component, Default, Clone)]
pub struct NpcBoneMap {
    pub bones: HashMap<RagdollBodyId, Entity>,
}

#[derive(Component, Default, Clone)]
pub struct NpcBindPose {
    pub all_bones: Vec<(Entity, Quat)>,
    pub body_local_rotations: HashMap<RagdollBodyId, Quat>,
}

#[derive(Component, Clone, Copy, Debug)]
pub struct NpcRagdollActive;

#[derive(Clone, Copy, Debug)]
pub struct NpcRagdollBodyPoseFrame {
    pub body: RagdollBodyId,
    pub position: Vec3,
    pub rotation: Quat,
}

#[derive(Clone, Debug)]
pub struct NpcRagdollPoseFrame {
    pub seq: u32,
    pub received_at: f32,
    pub root_position: Vec3,
    pub root_rotation: Quat,
    pub bodies: Vec<NpcRagdollBodyPoseFrame>,
}

#[derive(Component, Default, Clone, Debug)]
pub struct NpcRagdollNetState {
    pub prev: Option<NpcRagdollPoseFrame>,
    pub curr: Option<NpcRagdollPoseFrame>,
    pub local_rotation_corrections: HashMap<RagdollBodyId, Quat>,
}

#[derive(Component, Clone, Copy)]
pub(crate) struct NpcShadowState {
    pub(crate) enabled: bool,
}

#[derive(Component, Clone, Copy)]
pub(crate) struct NpcVisibilityState {
    pub(crate) visible: bool,
}

#[derive(Component, Clone, Copy)]
pub(crate) struct NpcFrustumCullOverrideState {
    pub(crate) enabled: bool,
}

/// Per-NPC network smoothing state for sparse replication samples.
#[derive(Component, Clone, Debug)]
pub struct NpcNetSmoothing {
    pub last_net_pos: Vec3,
    pub target_pos: Vec3,
    pub velocity: Vec3,
    pub last_net_yaw: f32,
    pub target_yaw: f32,
    pub yaw_rate: f32,
    pub last_sample_time: f32,
    pub initialized: bool,
}

impl NpcNetSmoothing {
    pub fn from_sample(pos: Vec3, yaw: f32, now: f32) -> Self {
        Self {
            last_net_pos: pos,
            target_pos: pos,
            velocity: Vec3::ZERO,
            last_net_yaw: yaw,
            target_yaw: yaw,
            yaw_rate: 0.0,
            last_sample_time: now,
            initialized: true,
        }
    }
}

/// The NPC entity that owns this rig (cached to avoid per-frame hierarchy walks).
#[derive(Component, Clone, Copy)]
pub struct NpcRigOwner(pub Entity);

/// Movement animation types for NPCs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum NpcMovementAnim {
    #[default]
    Idle,
    Walk,
    Run,
    LookBehindRun,
}

/// Duration for crossfade blending between animations (normal transitions).
pub(crate) const NPC_ANIM_BLEND_DURATION: f32 = 0.15;

/// Faster blend duration for stopping (walk/run -> idle).
pub(crate) const NPC_STOP_BLEND_DURATION: f32 = 0.08;

/// Speed smoothing factor (lower = smoother but slower response).
pub(crate) const NPC_SPEED_SMOOTHING: f32 = 12.0;

/// Threshold for instant stop detection (bypasses smoothing).
pub(crate) const INSTANT_STOP_THRESHOLD: f32 = 0.05;
pub(crate) const NPC_SHADOW_RANGE: f32 = 140.0;
pub(crate) const NPC_SHADOW_RANGE_SQ: f32 = NPC_SHADOW_RANGE * NPC_SHADOW_RANGE;
pub(crate) const NPC_NO_FRUSTUM_CULL_RANGE: f32 = 40.0;
pub(crate) const NPC_NO_FRUSTUM_CULL_RANGE_SQ: f32 =
    NPC_NO_FRUSTUM_CULL_RANGE * NPC_NO_FRUSTUM_CULL_RANGE;
pub(crate) const NPC_NET_EXTRAPOLATE_MAX_SECS: f32 = 0.35;
pub(crate) const NPC_ANIM_MIN_HOLD_IDLE_SECS: f32 = 0.18;
pub(crate) const NPC_ANIM_MIN_HOLD_MOVE_SECS: f32 = 0.12;
pub(crate) const NPC_ANIM_MIN_HOLD_FLEE_SECS: f32 = 0.25;

/// Tracks NPC animation state with blending support.
#[derive(Component, Default)]
pub struct NpcAnimState {
    /// Currently playing animation
    pub current_anim: NpcMovementAnim,
    /// Target animation (for blending)
    pub target_anim: NpcMovementAnim,
    /// Blend progress (0.0 = current, 1.0 = target)
    pub blend_progress: f32,
    /// Is dead (death animation playing)
    pub dead: bool,
    /// Has been initialized (for motion detection)
    pub initialized: bool,
    /// Last position (for motion-based animation detection)
    pub last_pos: Vec3,
    /// Smoothed speed (exponential moving average to prevent jitter)
    pub smoothed_speed: f32,
    /// True if current transition is a stop (walk/run -> idle), uses faster blend
    pub is_stopping: bool,
    /// Remaining lockout before we can transition again (anti-flap guard).
    pub transition_lock_timer: f32,
}

pub(super) struct NpcAnimSet {
    pub(super) idle: AnimationNodeIndex,
    pub(super) walk: AnimationNodeIndex,
    pub(super) run: AnimationNodeIndex,
    pub(super) look_behind_run: AnimationNodeIndex,
}
