//! Player character systems
//!
//! Handles custom player model loading, animation, spawning, and transform sync.

pub mod animation;
pub mod assets;
pub mod ids;
pub mod spawn;
pub mod sync;
pub mod visibility;

pub use animation::{setup_player_rig, update_player_animation};
pub use assets::setup_player_character_assets;
pub(crate) use ids::peer_id_to_u64;
pub use spawn::{ensure_local_player_tag, handle_player_spawned, sync_player_character_models};
pub use sync::sync_player_transforms;
pub use visibility::{
    apply_player_shadow_state_to_new_meshes, update_local_player_visibility,
    update_player_shadow_culling,
};

use bevy::animation::graph::{AnimationGraph, AnimationGraphHandle, AnimationNodeIndex};
use bevy::animation::{AnimationClip, AnimationPlayer};
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use lightyear::prelude::client::Connected;
use lightyear::prelude::*;
use shared::components::{
    Health, LocalPlayer, Player, PlayerCharacter, PlayerJumpState, PlayerPosition, PlayerRotation,
    PlayerWaterState,
};
use shared::physics::{ground_clearance_center, GROUND_SNAP_DISTANCE};
use shared::player::{JUMP_ANIM_MIN_SECS, PLAYER_HEIGHT};
use shared::terrain::WorldTerrain;
use shared::vehicle::{Vehicle, VehicleDriver, VehicleType};
use std::collections::{HashMap, HashSet};

use crate::input::{CameraMode, InputState};
use crate::render::systems::VehicleHoverBob;

// =============================================================================
// COMPONENTS & RESOURCES
// =============================================================================

/// Per-character assets (scene + animation graph + mapping).
#[derive(Clone)]
pub struct CharacterAssets {
    pub scene: Handle<Scene>,
    pub animation_graph: Handle<AnimationGraph>,
    // Movement animations
    pub idle_node: AnimationNodeIndex,
    pub walk_node: AnimationNodeIndex,
    pub walk_back_node: AnimationNodeIndex,
    pub strafe_left_node: AnimationNodeIndex,
    pub strafe_right_node: AnimationNodeIndex,
    pub run_node: AnimationNodeIndex,
    pub driving_node: AnimationNodeIndex,
    pub jump_node: AnimationNodeIndex,
    pub fall_node: AnimationNodeIndex,
    // Visual tweaks
    pub model_scale: f32,
    pub model_yaw_offset: f32,
}

/// Loaded player character assets keyed by PlayerCharacter.
#[derive(Resource, Clone)]
pub struct PlayerCharacterAssets {
    pub characters: HashMap<PlayerCharacter, CharacterAssets>,
}

/// The entity we spawn `SceneRoot` onto for the player character model.
#[derive(Component)]
pub struct PlayerModelRoot {
    pub character: PlayerCharacter,
}

/// Tracks shadow casting state for player models to avoid per-frame hierarchy scans.
#[derive(Component, Clone, Copy)]
pub(crate) struct PlayerShadowState {
    enabled: bool,
}

/// We spawned the model, but the internal GLTF hierarchy might not be ready yet.
#[derive(Component)]
pub struct NeedsPlayerRigSetup;

/// The animation root inside the player scene (entity with `AnimationPlayer`).
#[derive(Component)]
pub struct PlayerAnimationRoot;

/// The Player entity that owns this rig (cached to avoid per-frame hierarchy walks).
#[derive(Component, Clone, Copy)]
pub struct PlayerRigOwner(pub Entity);

/// The character type this rig uses (for picking animation nodes).
#[derive(Component, Clone, Copy)]
pub struct PlayerRigCharacter(pub PlayerCharacter);

/// Movement animation types
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MovementAnim {
    #[default]
    Idle,
    Walk,
    WalkBack,
    StrafeLeft,
    StrafeRight,
    Run,
    Driving,
    Jump,
    Fall,
}

/// Duration for crossfade blending between animations
const ANIM_BLEND_DURATION: f32 = 0.2;

/// Visual tweaks for player models.
const BASE_MODEL_SCALE: f32 = 1.0;
const OILMAN_MODEL_SCALE: f32 = 1.0;
/// The base model faces -X; rotate to align with game forward (-Z).
const BASE_MODEL_YAW_OFFSET: f32 = std::f32::consts::FRAC_PI_2;
/// Oilman needs a 180° turn to face game forward (-Z).
const OILMAN_MODEL_YAW_OFFSET: f32 = std::f32::consts::PI;
const PLAYER_SHADOW_RANGE: f32 = 160.0;
const PLAYER_SHADOW_RANGE_SQ: f32 = PLAYER_SHADOW_RANGE * PLAYER_SHADOW_RANGE;

/// Vertical velocity threshold to trigger falling animation
const FALL_VELOCITY_THRESHOLD: f32 = -1.2;
/// Velocity threshold to start landing confirmation.
const LANDING_VELOCITY_THRESHOLD: f32 = 0.25;
/// Time with low vertical velocity before exiting airborne state.
const LANDING_CONFIRM_SECS: f32 = 0.12;
/// Speed thresholds for idle/move hysteresis (reduces idle/walk flicker).
const MOVE_START_SPEED: f32 = 0.35;
const MOVE_STOP_SPEED: f32 = 0.2;

/// Tracks player animation state with blending support
#[derive(Component, Default)]
pub struct PlayerAnimState {
    /// Currently playing animation
    pub current_anim: MovementAnim,
    /// Target animation (for blending)
    pub target_anim: MovementAnim,
    /// Blend progress (0.0 = current, 1.0 = target)
    pub blend_progress: f32,
    /// Is dead (death animation playing)
    pub dead: bool,
    /// Has been initialized (for motion detection)
    pub initialized: bool,
    /// Last position (for motion-based animation detection)
    pub last_pos: Vec3,
    /// Last Y position (for vertical velocity detection)
    pub last_y: f32,
    /// Smoothed horizontal speed (for stable remote player animations)
    pub smoothed_speed: f32,
    /// True while the player is airborne (jumping or falling).
    pub airborne: bool,
    /// Time remaining to keep jump animation active.
    pub jump_timer: f32,
    /// Accumulates time near zero vertical velocity to confirm landing.
    pub landing_timer: f32,
    /// Tracks jump input edge (local player only).
    pub last_jump_pressed: bool,
    /// True if current airborne state started from a jump input.
    pub airborne_from_jump: bool,
}

/// Marker for the local player's model (used for visibility toggling)
#[derive(Component)]
pub struct LocalPlayerModel;

/// Tracks the last camera mode to detect changes
#[derive(Resource, Default)]
pub struct LastCameraMode(pub Option<CameraMode>);

// =============================================================================
// ASSET LOADING
// =============================================================================
