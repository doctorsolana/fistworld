//! NPC render-system state and shared constants.

use bevy::animation::graph::{AnimationGraph, AnimationNodeIndex};
use bevy::prelude::*;

/// Loaded NPC scene and animation graph resources.
#[derive(Resource, Clone)]
pub struct NpcAssets {
    pub scene: Handle<Scene>,
    pub animation_graph: Handle<AnimationGraph>,
    // Oilman animation clips
    pub tpose_node: AnimationNodeIndex,
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

#[derive(Component, Clone, Copy)]
pub(crate) struct NpcShadowState {
    pub(crate) enabled: bool,
}

#[derive(Component, Clone, Copy)]
pub(crate) struct NpcVisibilityState {
    pub(crate) visible: bool,
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
}

pub(super) struct NpcAnimSet {
    pub(super) idle: AnimationNodeIndex,
    pub(super) walk: AnimationNodeIndex,
    pub(super) run: AnimationNodeIndex,
    pub(super) look_behind_run: AnimationNodeIndex,
    pub(super) death: AnimationNodeIndex,
}
