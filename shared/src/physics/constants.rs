use crate::player::PLAYER_HEIGHT;

/// Minimum Y for the capsule center above ground.
#[inline]
pub fn ground_clearance_center() -> f32 {
    PLAYER_HEIGHT * 0.5
}
