//! Client-side weapon systems
//!
//! Handles shooting input, local tracers for prediction, and weapon effects.
//! Updated for Lightyear 0.26 / Bevy 0.18

pub mod assets;
pub mod debug;
pub mod effects;
pub mod input;
pub mod paths;
pub mod projectiles;
pub mod state;
pub mod warmup;

pub use assets::{setup_weapon_audio_assets, setup_weapon_visual_assets};
pub use debug::{
    despawn_debug_overlay, emit_client_perf_summary, handle_toggle_debug_mode,
    handle_toggle_perf_overlay, spawn_debug_overlay, update_client_perf_snapshot,
    update_debug_overlay, update_perf_drop_monitor, update_trajectory_debug_gizmos,
};
pub use effects::{
    update_blood_bursts, update_blood_droplets, update_blood_ground_splats, update_impact_markers,
    update_muzzle_flash, update_muzzle_smoke,
};
pub use input::{
    cleanup_shoot_input_suppress, handle_reload_input, handle_shoot_input, handle_weapon_sounds,
    update_recoil_recovery,
};
pub use projectiles::{
    handle_bullet_impacts, handle_bullet_spawned, handle_hit_confirms, reset_projectile_indices,
    sync_player_owner_index, sync_remote_muzzle_index, update_bullet_visuals, update_local_tracers,
};
pub use state::*;
pub use warmup::{cleanup_weapon_warmups, spawn_weapon_warmups, update_weapon_warmup_queue};

pub(crate) use effects::{spawn_blood_splatter, spawn_muzzle_flash, spawn_muzzle_smoke};

use bevy::audio::Volume;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::diagnostic::{
    DiagnosticsStore, EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin,
};
use bevy::light::NotShadowCaster;
use bevy::math::primitives::Plane3d;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use lightyear::prelude::*;
use shared::components::{
    Bullet, BulletVelocity, EquippedWeapon, LocalPlayer, LocalTracer, Player, PlayerPosition,
};
use shared::protocol::{
    BulletImpact, BulletImpactSurface, HitConfirm, MeleeAttackRequest, ReliableChannel,
    ReloadRequest, ShootRequest,
};
use shared::terrain::{ChunkCoord, WorldTerrain};
use shared::vehicle::Vehicle;
use shared::weapons::{
    ballistics, muzzle_offset, WeaponDebugMode, WeaponType, RECOIL_ACCUMULATION_MULT,
    RECOIL_ADS_MULTIPLIER, RECOIL_BURST_RESET_TIME, RECOIL_RECOVERY_SPEED,
};
use std::collections::HashMap;

#[derive(Component)]
pub(crate) struct WarmupWeaponFx {
    timer: Timer,
}

const WEAPON_WARMUP_POS: Vec3 = Vec3::new(0.0, -520.0, 0.0);
const WEAPON_WARMUP_SCALE: f32 = 0.02;
const WEAPON_WARMUP_LIFETIME: f32 = 0.35;
const WEAPON_WARMUP_PER_FRAME: usize = 2;

use crate::camera::peer_id_to_u64;
use crate::crosshair;
use crate::input::{CameraMode, InputState};
use crate::render::systems::ClientWorldRoot;
use crate::states::GameState;
use crate::weapon_view::RemoteThirdPersonWeapon;

/// Small padding so client doesn't finish reload before server.
const CLIENT_RELOAD_PAD_SECS: f32 = 0.12;

/// Prevent the "click to focus/grab cursor" from also firing a shot.
///
/// When the cursor transitions from unlocked -> locked, we suppress firing until the
/// left mouse button is released once.
#[derive(Default)]
pub(crate) struct CursorGrabShootGuard {
    last_locked: Option<bool>,
    suppress_until_release: bool,
}

// Recoil settings are now imported from shared::weapons

#[derive(Clone, Copy)]
struct SmokeSettings {
    puff_count: usize,
    base_scale: f32,
    scale_jitter: f32,
    lifetime: f32,
    speed: f32,
    spread: f32,
    forward_offset: f32,
}

#[derive(Clone, Copy)]
struct FlashSettings {
    base_scale: f32,
    scale_jitter: f32,
    lifetime: f32,
    forward_offset: f32,
}

// =============================================================================
// BLOOD SPLATTER EFFECTS
// =============================================================================

const BLOOD_GRAVITY: f32 = 12.0; // Gravity for blood droplets
const BLOOD_DROPLET_LIFETIME: f32 = 3.0; // Max time before despawn if never lands
const BLOOD_SPLAT_LIFETIME: f32 = 30.0; // Ground decals persist and dry out
const BLOOD_AIR_DRAG: f32 = 0.9; // Simple linear drag

// Entity caps to prevent explosion during sustained firefights
const MAX_BLOOD_GROUND_SPLATS: usize = 96;
const MAX_BLOOD_MISTS: usize = 30;
const MAX_BLOOD_DROPLETS: usize = 48;
const MAX_IMPACT_MARKERS: usize = 40;

// =============================================================================
// DEBUG OVERLAY (FPS counter, etc.)
// =============================================================================

// =============================================================================
// WEAPON VISUAL ASSET SETUP
// =============================================================================
