use crate::player::PLAYER_HEIGHT;

/// Gravity in m/s^2 (negative Y).
pub const GRAVITY: f32 = -18.0;
/// Horizontal acceleration in m/s^2.
pub const MOVE_ACCEL: f32 = 45.0;
/// Horizontal deceleration when no input ("friction") in m/s^2.
pub const MOVE_BRAKE: f32 = 55.0;
/// How close to the ground we "snap" when falling.
pub const GROUND_SNAP_DISTANCE: f32 = 0.35;
/// Water depth at which we switch to swimming (meters, relative to player center).
pub const WATER_SWIM_DEPTH: f32 = 0.45;
/// Horizontal speed multiplier when swimming.
pub const WATER_SPEED_MULT: f32 = 0.55;
/// Horizontal acceleration multiplier in water.
pub const WATER_ACCEL_MULT: f32 = 0.6;
/// Horizontal brake multiplier in water.
pub const WATER_BRAKE_MULT: f32 = 0.7;
/// Gravity scale applied while swimming.
pub const WATER_GRAVITY_SCALE: f32 = 0.25;
/// Vertical drag in water.
pub const WATER_VERTICAL_DRAG: f32 = 4.0;
/// Horizontal drag in water.
pub const WATER_HORIZONTAL_DRAG: f32 = 2.5;
/// Buoyancy force per meter of submersion.
pub const WATER_BUOYANCY: f32 = 14.0;
/// Swim-up speed when holding jump in water.
pub const SWIM_UP_SPEED: f32 = 4.5;
/// Jump velocity in m/s (upward).
pub const JUMP_VELOCITY: f32 = 7.5;
/// Debug fly mode speed (m/s).
pub const FLY_SPEED: f32 = 40.0;
/// Debug fly fast multiplier (Shift).
pub const FLY_FAST_MULT: f32 = 3.0;
/// Threshold for determining if a surface normal is "walkable".
pub const WALKABLE_THRESHOLD: f32 = 0.5;

/// Minimum Y for the capsule center above ground.
#[inline]
pub fn ground_clearance_center() -> f32 {
    PLAYER_HEIGHT * 0.5
}
