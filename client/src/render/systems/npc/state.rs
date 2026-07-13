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

/// One bone in the rig's topological ordering (parents before children).
#[derive(Clone, Debug)]
pub struct RagdollRigBone {
    pub entity: Entity,
    /// Index of the parent bone in `NpcRagdollRig::bones` (None for the rig root).
    pub parent: Option<usize>,
    /// Which streamed physics body drives this bone, if any.
    pub body: Option<RagdollBodyId>,
    /// Full local transform captured at rig setup (bind pose).
    pub bind_local: Transform,
    /// Bind transform relative to the NPC root entity (chained through the
    /// model-root yaw flip and the armature's axis-conversion rotation;
    /// includes the armature's uniform scale in its translation).
    pub bind_rel: Transform,
}

/// Precomputed rig data for ragdoll pose application, captured at rig setup
/// while the skeleton is still in bind pose.
///
/// Mapped bones are driven in world-space ROTATION from their physics bodies
/// (offsets measured at activation against the server's own start frame);
/// bone translations stay at bind so the mesh keeps its proportions (no
/// stretching), and the whole rig is snapped to bind pose at ragdoll start so
/// unmapped bones can't contaminate limb placement.
#[derive(Component, Clone, Debug)]
pub struct NpcRagdollRig {
    /// Transform from the NPC root entity down to (but excluding) the rig
    /// root: the model-root offset/PI yaw plus any intermediate scene nodes.
    pub prefix: Transform,
    /// Bones in parent-before-child order, starting at the rig root.
    pub bones: Vec<RagdollRigBone>,
    /// Bind translation of the Hips bone relative to the NPC root entity (in
    /// world units — armature scale already applied). Used to anchor the mesh
    /// so the hips bone lands exactly on the streamed pelvis body.
    pub hips_offset_from_root: Vec3,
    /// Index of the Hips bone in `bones`.
    pub hips_index: Option<usize>,
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
    /// Per-body rotation offset `O = B0⁻¹ · W0(bone)` captured at activation:
    /// `B0` is the body's world rotation in the start snapshot, `W0` the
    /// bone's bind-pose world rotation under the root frame at that moment.
    /// Per frame the desired bone world rotation is then exactly `B · O`.
    pub body_bone_offsets: HashMap<RagdollBodyId, Quat>,
    /// World rotations of the bodies in the start snapshot, kept so offsets
    /// can be computed lazily if the rig wasn't ready at activation.
    pub start_body_rotations: HashMap<RagdollBodyId, Quat>,
    /// NPC root world rotation frozen at activation (yaw at death). The root
    /// stops rotating; all corpse orientation lives in the bones.
    pub root_fix_rotation: Option<Quat>,
    /// Client time at ragdoll activation (drives the pose blend-in).
    pub started_at: f32,
    /// Whether the rig was snapped to its reference pose at ragdoll start
    /// (clears death-animation contamination from unmapped bones).
    pub pose_reset_done: bool,
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

/// One primitive body part of a ragdoll reference Dummy. Driven DIRECTLY from
/// the streamed physics poses — no skeleton, no bind pose, no mapping — so it
/// renders exactly what the server simulates (ground truth for diagnosis).
#[derive(Component, Clone, Copy)]
pub struct NpcDummyBody {
    pub body: RagdollBodyId,
}

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
    /// True while the AnimationPlayer is paused because the NPC is hidden.
    /// Without pausing, Bevy keeps sampling every bone of hidden/far NPCs
    /// each frame even though nothing is drawn.
    pub anim_paused: bool,
}

pub(super) struct NpcAnimSet {
    pub(super) idle: AnimationNodeIndex,
    pub(super) walk: AnimationNodeIndex,
    pub(super) run: AnimationNodeIndex,
    pub(super) look_behind_run: AnimationNodeIndex,
}
