use crate::player::PLAYER_HEIGHT;

/// Horizontal clearance around solid architecture for an embodied character.
/// Shared by navigation obstacles and the doorway staging point, so an authored
/// asset anchor cannot become a movement goal inside its inflated blocker.
pub const CHARACTER_NAV_RADIUS: f32 = 0.28;

/// Minimum Y for the capsule center above ground.
#[inline]
pub fn ground_clearance_center() -> f32 {
    PLAYER_HEIGHT * 0.5
}
