//! Vehicle systems
//!
//! Handles speeder bike visuals, spawning, and transform synchronization.

pub mod angles;
pub mod assets;
pub mod hover;
pub mod spawn;
pub mod sync;
pub mod visibility;

pub use assets::setup_vehicle_visual_assets;
pub use hover::update_vehicle_hover;
pub use spawn::handle_vehicle_spawned;
pub use sync::sync_vehicle_transforms;
pub use visibility::{apply_vehicle_shadow_state_to_new_meshes, update_vehicle_shadow_culling};

use angles::{lerp_angle, normalize_angle};

use bevy::camera::visibility::ViewVisibility;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use shared::components::{LocalPlayer, PlayerPosition};
use shared::vehicle::{Vehicle, VehicleState, VehicleType};

// =============================================================================
// COMPONENTS
// =============================================================================

/// Marker for vehicle visual entities
#[derive(Component)]
pub struct VehicleVisual;

/// Tracks shadow casting state for vehicle visuals to avoid per-frame hierarchy scans.
#[derive(Component, Clone, Copy)]
pub struct VehicleShadowState {
    enabled: bool,
}

/// Subtle hover bobbing for hoverbike visuals.
#[derive(Component, Clone, Copy)]
pub struct VehicleHoverBob {
    pub base_offset: Vec3,
    pub amplitude: f32,
    pub frequency: f32,
    pub phase: f32,
}

/// Client-side render smoothing state for vehicles.
/// We integrate using replicated linear/angular velocities each frame.
/// When a *new* authoritative snapshot arrives (replicated `VehicleState` changes), we gently correct
/// toward it. This avoids the high-FPS "rubber pullback" that can cause visible popping/blinking at
/// high speed when render FPS > replication FPS.
#[derive(Component, Clone, Copy)]
pub struct VehicleRenderSmoothing {
    pub initialized: bool,
    pub position: Vec3,
    pub heading: f32,
    pub pitch: f32,
    pub roll: f32,

    /// Last authoritative (replicated) pose we observed. Used to detect new snapshots.
    pub last_server_position: Vec3,
    pub last_server_heading: f32,
    pub last_server_pitch: f32,
    pub last_server_roll: f32,

    /// Smoothed velocity for extrapolation (reduces jitter from velocity discontinuities)
    pub smoothed_velocity: Vec3,
    pub smoothed_angular_yaw: f32,
}

impl Default for VehicleRenderSmoothing {
    fn default() -> Self {
        Self {
            initialized: false,
            position: Vec3::ZERO,
            heading: 0.0,
            pitch: 0.0,
            roll: 0.0,
            last_server_position: Vec3::ZERO,
            last_server_heading: 0.0,
            last_server_pitch: 0.0,
            last_server_roll: 0.0,
            smoothed_velocity: Vec3::ZERO,
            smoothed_angular_yaw: 0.0,
        }
    }
}

/// Shared meshes/materials for the procedural car visual.
#[derive(Resource)]
pub struct CarVisualAssets {
    pub body_mesh: Handle<Mesh>,
    pub front_mesh: Handle<Mesh>,
    pub rear_mesh: Handle<Mesh>,
    pub wheel_mesh: Handle<Mesh>,
    pub body_material: Handle<StandardMaterial>,
    pub front_material: Handle<StandardMaterial>,
    pub rear_material: Handle<StandardMaterial>,
    pub wheel_material: Handle<StandardMaterial>,
}

// =============================================================================
// SPAWNING
// =============================================================================

const HOVERBIKE_SCENE: &str = "game_assets/vehicles/hoverbike.glb#Scene0";
const HOVERBIKE_VISUAL_Y_OFFSET: f32 = 0.01;
const HOVERBIKE_HOVER_AMPLITUDE: f32 = 0.05;
const HOVERBIKE_HOVER_FREQUENCY: f32 = 1.6;
const HOVERBIKE_YAW_OFFSET: f32 = std::f32::consts::PI;
const VEHICLE_SHADOW_RANGE: f32 = 180.0;
const VEHICLE_SHADOW_RANGE_SQ: f32 = VEHICLE_SHADOW_RANGE * VEHICLE_SHADOW_RANGE;

// =============================================================================
// HOVER ANIMATION
// =============================================================================

// =============================================================================
// TRANSFORM SYNC
// =============================================================================

// =============================================================================
// ANGLE HELPERS
// =============================================================================
