use bevy::prelude::*;

use crate::terrain::Biome;
use crate::vehicle::tuning::VehicleDef;

pub fn surface_mu(def: &VehicleDef, biome: Biome) -> f32 {
    match biome {
        Biome::Desert => def.mu_desert,
        Biome::Grasslands => def.mu_grasslands,
        Biome::Natureland => def.mu_grasslands,
        Biome::Mountain => def.mu_grasslands,
        Biome::Ocean => def.mu_grasslands,
    }
}

pub(super) fn vehicle_body_rotation(heading: f32, pitch: f32, roll: f32) -> Quat {
    Quat::from_euler(EulerRot::YXZ, heading, pitch, -roll)
}

pub(super) fn vehicle_body_axes(heading: f32, pitch: f32, roll: f32) -> (Vec3, Vec3, Vec3) {
    let rotation = vehicle_body_rotation(heading, pitch, roll);
    (
        rotation * Vec3::NEG_Z,
        rotation * Vec3::X,
        rotation * Vec3::Y,
    )
}

/// Get forward and right vectors from heading.
pub(super) fn get_basis_vectors(heading: f32) -> (Vec3, Vec3) {
    let forward = Vec3::new(-heading.sin(), 0.0, -heading.cos());
    let right = Vec3::new(heading.cos(), 0.0, -heading.sin());
    (forward, right)
}

pub(super) fn shortest_angle_diff(target: f32, current: f32) -> f32 {
    let diff = target - current;
    (diff + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

pub(super) fn terrain_angles_from_normal(
    normal: Vec3,
    heading: f32,
    def: &VehicleDef,
) -> (f32, f32) {
    let (forward, right) = get_basis_vectors(heading);
    let forward_slope = normal.dot(forward);
    let pitch = -forward_slope
        .asin()
        .clamp(-def.max_terrain_pitch, def.max_terrain_pitch);
    let right_slope = normal.dot(right);
    let roll = right_slope
        .asin()
        .clamp(-def.max_terrain_roll, def.max_terrain_roll);
    (pitch, roll)
}

/// Calculate the bike's local up vector in world space based on pitch/roll.
pub(super) fn bike_up_vector(pitch: f32, roll: f32) -> Vec3 {
    let up = Vec3::new(-roll.sin(), pitch.cos() * roll.cos(), pitch.sin());
    up.normalize()
}
