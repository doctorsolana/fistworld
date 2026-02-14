//! Weapon tuning constants.

/// Speed at which recoil recovers back to center (per second).
pub const RECOIL_RECOVERY_SPEED: f32 = 4.0;

/// Time in seconds before burst shot counter resets.
pub const RECOIL_BURST_RESET_TIME: f32 = 0.35;

/// Recoil multiplier when aiming down sights (lower = less recoil).
pub const RECOIL_ADS_MULTIPLIER: f32 = 0.5;

/// Multiplier applied to recoil for consecutive shots in a burst.
pub const RECOIL_ACCUMULATION_MULT: f32 = 1.15;
