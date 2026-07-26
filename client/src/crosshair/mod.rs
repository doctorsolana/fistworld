//! Crosshair UI for first-person shooting
//!
//! Simple centered dot crosshair that shows in first-person mode.
//! Shrinks when aiming down sights (ADS).

pub mod death_screen;
pub mod hit_markers;
pub mod hud;

pub use death_screen::{despawn_death_screen, spawn_death_screen, update_death_screen};
pub use hit_markers::{spawn_hit_marker, update_hit_markers};
pub use hud::{
    despawn_crosshair, spawn_crosshair, update_crosshair_ads, update_crosshair_visibility,
};

use crate::input::CameraMode;
use bevy::prelude::*;

const SNIPER_SCOPE_SIZE: f32 = 310.0;
const SNIPER_SCOPE_HALF: f32 = SNIPER_SCOPE_SIZE * 0.5;
const SNIPER_SCOPE_MASK_SIZE: f32 = 2400.0;
const SNIPER_SCOPE_CORNER_RADIUS: f32 = 18.0;
const SNIPER_SCOPE_RETICLE_MARGIN: f32 = 18.0;

/// Marker component for the crosshair UI
#[derive(Component)]
pub struct Crosshair;

/// Marker for the center dot
#[derive(Component)]
pub struct CrosshairDot;

/// Marker for crosshair lines (top/bottom/left/right)
#[derive(Component)]
pub struct CrosshairLine {
    /// Which direction this line points
    pub direction: CrosshairLineDir,
}

#[derive(Clone, Copy)]
pub enum CrosshairLineDir {
    Top,
    Bottom,
    Left,
    Right,
}

/// Marker for the hit marker overlay
#[derive(Component)]
pub struct HitMarker {
    pub spawn_time: f32,
    pub is_kill: bool,
}

/// Sniper ADS overlay with boxed scope mask and reticle
#[derive(Component)]
pub struct SniperScopeOverlay;

// =============================================================================
// DEATH SCREEN UI
// =============================================================================

/// Marker for the death screen overlay
#[derive(Component)]
pub struct DeathScreen;

/// Marker for the respawn timer text
#[derive(Component)]
pub struct RespawnTimerText;
