//! Bullet physics simulation
//!
//! Realistic ballistics with gravity and air drag.

use bevy::prelude::*;

/// Gravity acceleration for bullets (m/s²)
pub const BULLET_GRAVITY: f32 = -9.81;

/// Maximum bullet lifetime in seconds (supports long-range sniper shots)
pub const BULLET_MAX_LIFETIME: f32 = 8.0;

/// Air drag coefficient (simplified model)
/// Higher = more drag, bullets slow down faster
pub const BULLET_DRAG_COEFFICIENT: f32 = 0.00008;

/// Minimum bullet speed before despawn (m/s)
pub const BULLET_MIN_SPEED: f32 = 50.0;

/// Maximum range before bullet despawns (m)
pub const BULLET_MAX_RANGE: f32 = 1500.0;

/// Simulate one physics step for a bullet
///
/// Returns (new_position, new_velocity)
///
/// Physics model:
/// - Gravity causes bullet drop over distance
/// - Air drag slows the bullet (velocity-squared model)
pub fn step_bullet_physics(position: Vec3, velocity: Vec3, dt: f32) -> (Vec3, Vec3) {
    let mut vel = velocity;

    // Apply gravity (bullet drop)
    vel.y += BULLET_GRAVITY * dt;

    // Apply air drag (velocity-squared drag model)
    // F_drag = -k * v² * v_hat
    let speed = vel.length();
    if speed > 0.1 {
        let drag_magnitude = BULLET_DRAG_COEFFICIENT * speed * speed;
        let drag_force = -vel.normalize() * drag_magnitude;
        vel += drag_force * dt;
    }

    // Integrate position
    let new_pos = position + vel * dt;

    (new_pos, vel)
}

/// Calculate bullet drop at a given distance for zeroing/aiming
///
/// Returns the vertical drop in meters
pub fn calculate_bullet_drop(distance: f32, bullet_speed: f32) -> f32 {
    // Simple approximation: time = distance / speed
    // drop = 0.5 * g * t²
    let time_of_flight = distance / bullet_speed;
    0.5 * (-BULLET_GRAVITY) * time_of_flight * time_of_flight
}

/// Calculate time of flight to a target at given distance
pub fn calculate_time_of_flight(distance: f32, bullet_speed: f32) -> f32 {
    distance / bullet_speed
}

/// Check if bullet should be despawned
pub fn should_despawn_bullet(
    velocity: Vec3,
    spawn_position: Vec3,
    current_position: Vec3,
    spawn_time: f32,
    current_time: f32,
) -> bool {
    let speed = velocity.length();
    let distance = (current_position - spawn_position).length();
    let lifetime = current_time - spawn_time;

    speed < BULLET_MIN_SPEED || distance > BULLET_MAX_RANGE || lifetime > BULLET_MAX_LIFETIME
}

/// Apply random spread to a shot direction
///
/// Returns a new direction with random deviation within the spread cone
pub fn apply_spread(direction: Vec3, spread_radians: f32) -> Vec3 {
    if spread_radians <= 0.0 {
        return direction.normalize();
    }

    // Generate random angle within spread cone
    let random_angle = rand::random::<f32>() * std::f32::consts::TAU;
    let random_radius = rand::random::<f32>().sqrt() * spread_radians;

    // Create perpendicular vectors for the spread plane
    let up = if direction.y.abs() < 0.9 {
        Vec3::Y
    } else {
        Vec3::X
    };
    let right = direction.cross(up).normalize();
    let actual_up = right.cross(direction).normalize();

    // Apply offset
    let offset = right * (random_radius * random_angle.cos())
        + actual_up * (random_radius * random_angle.sin());

    (direction + offset).normalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bullet_drop() {
        // At 100m with 900 m/s bullet
        let drop = calculate_bullet_drop(100.0, 900.0);
        // t = 100/900 ≈ 0.111s
        // drop = 0.5 * 9.81 * 0.111² ≈ 0.06m
        assert!(drop > 0.05 && drop < 0.07);
    }

    #[test]
    fn test_step_physics() {
        let pos = Vec3::ZERO;
        let vel = Vec3::new(0.0, 0.0, -900.0); // 900 m/s forward

        let (new_pos, new_vel) = step_bullet_physics(pos, vel, 0.016); // 60fps

        // Should have moved forward
        assert!(new_pos.z < 0.0);
        // Should have dropped slightly
        assert!(new_vel.y < 0.0);
        // Should have slowed slightly due to drag
        assert!(new_vel.length() < vel.length());
    }
}
