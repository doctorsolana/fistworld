//! The one Y-rotation convention, so nothing can disagree about which way a
//! building faces.
//!
//! This exists because the same sign error appeared twice, independently, in
//! two places that both describe the ground under a building: the build zone
//! that decides which trees to clear, and the flatten rectangle that levels the
//! plot. Each was internally consistent and each was turned the opposite way
//! from the model, so a rotated building cleared and levelled the wrong patch —
//! invisible on a square footprint, and on an oblong one it leaves a wedge with
//! a tree still standing in it.
//!
//! The ground truth is not a choice: the model is placed with
//! `Transform::with_rotation(Quat::from_rotation_y(r))`, so wherever the walls
//! are is wherever that quaternion puts them. Everything else must invert it.

use bevy::prelude::*;

/// Rotate a ground-plane offset the way the renderer rotates a model.
///
/// Matches `Quat::from_rotation_y(rotation_y) * Vec3::new(local.x, 0.0, local.y)`
/// projected back to XZ, and is tested against exactly that.
#[inline]
pub fn local_to_world_xz(local: Vec2, rotation_y: f32) -> Vec2 {
    let (sin_r, cos_r) = rotation_y.sin_cos();
    Vec2::new(
        local.x * cos_r + local.y * sin_r,
        -local.x * sin_r + local.y * cos_r,
    )
}

/// Bring a world-space ground offset into a building's own frame.
///
/// The exact inverse of [`local_to_world_xz`]. Use this for "is this point
/// inside the building's footprint" — never hand-roll it, because a flipped
/// sign here is silent on square buildings and wrong on every other one.
#[inline]
pub fn world_to_local_xz(rel: Vec2, rotation_y: f32) -> Vec2 {
    let (sin_r, cos_r) = rotation_y.sin_cos();
    Vec2::new(rel.x * cos_r - rel.y * sin_r, rel.x * sin_r + rel.y * cos_r)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The forward map must BE the renderer's, not merely resemble it.
    #[test]
    fn local_to_world_matches_the_quaternion_that_places_the_model() {
        let local = Vec2::new(2.75, -1.25);
        for step in 0..24 {
            let rotation = std::f32::consts::TAU * step as f32 / 24.0;
            let ours = local_to_world_xz(local, rotation);
            let bevy = Quat::from_rotation_y(rotation) * Vec3::new(local.x, 0.0, local.y);
            assert!(
                (ours - Vec2::new(bevy.x, bevy.z)).length() < 1e-4,
                "rotation {rotation:.3}: ours {ours:?} vs Quat::from_rotation_y {:?}",
                Vec2::new(bevy.x, bevy.z)
            );
        }
    }

    /// And the inverse must actually invert, with no slack to hide in.
    #[test]
    fn world_to_local_round_trips() {
        let local = Vec2::new(2.75, -1.25);
        for step in 0..24 {
            let rotation = std::f32::consts::TAU * step as f32 / 24.0;
            let back = world_to_local_xz(local_to_world_xz(local, rotation), rotation);
            assert!(
                (back - local).length() < 1e-4,
                "rotation {rotation:.3}: {local:?} came back as {back:?}"
            );
        }
    }
}
