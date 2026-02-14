//! Player-related constants and types

/// Player movement speed (units per second)
pub const PLAYER_SPEED: f32 = 8.0;

/// Player height (for capsule)
pub const PLAYER_HEIGHT: f32 = 1.8;

/// Player radius (for capsule)
pub const PLAYER_RADIUS: f32 = 0.3;

/// Minimum time to show jump animation (seconds).
pub const JUMP_ANIM_MIN_SECS: f32 = 0.18;

/// Maximum height a player can step up onto (small rocks, curbs, etc.)
pub const STEP_UP_HEIGHT: f32 = 0.4;

/// Mouse sensitivity for look
pub const MOUSE_SENSITIVITY: f32 = 0.003;

/// Spawn position for new players (spawn above terrain to prevent clipping)
pub const SPAWN_POSITION: [f32; 3] = [0.0, 10.0, 0.0];

/// Maximum player health
pub const PLAYER_MAX_HEALTH: f32 = 100.0;

/// Time in seconds before a dead player can respawn
pub const RESPAWN_TIME: f32 = 4.0;
