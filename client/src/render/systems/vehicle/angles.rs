//! angles systems.

#[allow(dead_code)]
pub(super) fn angle_diff(from: f32, to: f32) -> f32 {
    let diff = to - from;
    ((diff + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)) - std::f32::consts::PI
}

pub(super) fn normalize_angle(angle: f32) -> f32 {
    ((angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)) - std::f32::consts::PI
}

pub(super) fn lerp_angle(from: f32, to: f32, t: f32) -> f32 {
    normalize_angle(from + angle_diff(from, to) * t)
}
