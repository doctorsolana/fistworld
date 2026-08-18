use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::building::BuildingType;

/// Component for placed buildings (replicated).
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlacedBuilding {
    pub building_type: BuildingType,
    /// Rotation in radians using the city's XZ footprint convention.
    ///
    /// Use [`building_rotation_quat`] before applying this value to a Bevy
    /// `Transform` or to local-space 3D collider geometry.
    pub rotation: f32,
}

/// Network position for placed buildings.
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildingPosition(pub Vec3);

/// Convert the city's 2D footprint yaw into Bevy's Y-axis quaternion convention.
///
/// City geometry maps local +X to `(cos(yaw), sin(yaw))` in world XZ. Bevy's
/// right-handed Y rotation uses the opposite sign for that mapping.
#[inline]
pub fn building_rotation_quat(rotation_y: f32) -> Quat {
    Quat::from_rotation_y(-rotation_y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn building_quaternion_matches_city_footprint_axes() {
        let yaw = 37.0_f32.to_radians();
        let rotation = building_rotation_quat(yaw);
        let axis_x = Vec2::new(yaw.cos(), yaw.sin());
        let axis_z = Vec2::new(-yaw.sin(), yaw.cos());

        let world_x = rotation * Vec3::X;
        let world_z = rotation * Vec3::Z;

        assert!(world_x.distance(Vec3::new(axis_x.x, 0.0, axis_x.y)) < 1.0e-5);
        assert!(world_z.distance(Vec3::new(axis_z.x, 0.0, axis_z.y)) < 1.0e-5);
    }
}
