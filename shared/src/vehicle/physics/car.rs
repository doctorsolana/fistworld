use bevy::prelude::*;

use crate::terrain::WorldTerrain;
use crate::vehicle::components::{CarSuspensionState, VehicleInput, VehicleState, VehicleType};
use crate::vehicle::tuning::vehicle_def;

use super::common::{get_basis_vectors, surface_mu};

/// Raycast-wheel car physics with proper ground collision.
pub fn step_car_physics(
    input: &VehicleInput,
    state: &mut VehicleState,
    suspension: &mut CarSuspensionState,
    terrain: &WorldTerrain,
    dt: f32,
    has_driver: bool,
    vehicle_type: VehicleType,
) {
    let def = vehicle_def(vehicle_type);
    let (forward, right) = get_basis_vectors(state.heading);

    let throttle = if has_driver { input.throttle } else { 0.0 };
    let brake = if has_driver { input.brake } else { 0.0 };
    let steer = if has_driver { input.steer } else { 0.0 };

    let half_wheel_base = def.wheel_base * 0.5;
    let half_track = def.track_width * 0.5;

    let wheel_local_xz = [
        Vec2::new(-half_track, -half_wheel_base),
        Vec2::new(half_track, -half_wheel_base),
        Vec2::new(-half_track, half_wheel_base),
        Vec2::new(half_track, half_wheel_base),
    ];

    let mut total_force = Vec3::ZERO;
    let mut total_torque_y = 0.0;
    let mut grounded_wheels = 0;
    let mut ground_heights = [0.0f32; 4];

    for (i, local_xz) in wheel_local_xz.iter().copied().enumerate() {
        let is_front = i < 2;

        let wheel_world_pos = state.position + forward * local_xz.y + right * local_xz.x;

        let ground_y = terrain.get_height(wheel_world_pos.x, wheel_world_pos.z);
        ground_heights[i] = ground_y;

        let wheel_contact_y = ground_y + def.wheel_radius;
        let chassis_rest_y = wheel_contact_y + def.suspension_rest;

        let compression = (chassis_rest_y - state.position.y).clamp(0.0, def.suspension_rest);

        let compression_vel = (compression - suspension.last_compression[i]) / dt;
        suspension.last_compression[i] = compression;
        suspension.compression[i] = compression;

        if compression <= 0.001 {
            continue;
        }

        grounded_wheels += 1;

        let spring_force = compression * def.suspension_stiffness;
        let damper_force = compression_vel * def.suspension_damping;
        let normal_force = (spring_force + damper_force).max(0.0);

        let contact_point = Vec3::new(wheel_world_pos.x, ground_y, wheel_world_pos.z);

        let steer_angle = if is_front {
            steer * def.max_steer_angle
        } else {
            0.0
        };
        let steer_rot = Quat::from_rotation_y(steer_angle);
        let wheel_forward = (steer_rot * forward).normalize_or_zero();
        let wheel_right = (steer_rot * right).normalize_or_zero();

        let r = contact_point - state.position;
        let omega = Vec3::Y * state.angular_velocity_yaw;
        let wheel_velocity = state.velocity + omega.cross(r);

        let long_speed = wheel_velocity.dot(wheel_forward);
        let lat_speed = wheel_velocity.dot(wheel_right);

        let mu = surface_mu(&def, terrain.get_biome(contact_point.x, contact_point.z));
        let max_friction = mu * normal_force;

        let drive_force = throttle * def.engine_force * 0.25;

        let brake_force = brake * def.brake_force * 0.25;
        let brake_dir = if long_speed.abs() > 0.1 {
            long_speed.signum()
        } else {
            0.0
        };

        let longitudinal_force =
            (drive_force - brake_force * brake_dir).clamp(-max_friction, max_friction);

        let lateral_force = (-lat_speed * def.lateral_friction).clamp(-max_friction, max_friction);

        let wheel_force = wheel_forward * longitudinal_force
            + wheel_right * lateral_force
            + Vec3::Y * normal_force;
        total_force += wheel_force;

        let horizontal_tire_force =
            wheel_forward * longitudinal_force + wheel_right * lateral_force;
        total_torque_y += r.cross(horizontal_tire_force).y;
    }

    state.grounded = grounded_wheels > 0;

    total_force += Vec3::Y * def.gravity * def.mass;

    let horizontal_vel = Vec3::new(state.velocity.x, 0.0, state.velocity.z);
    let speed = horizontal_vel.length();
    if speed > 0.01 {
        let drag_dir = horizontal_vel.normalize();
        let drag = drag_dir * (def.drag_coefficient * speed * speed);
        total_force -= drag;

        if state.grounded {
            total_force -= drag_dir * def.rolling_resistance;
        }
    }

    if state.grounded && throttle < 0.1 && speed > 0.5 {
        let engine_brake = if has_driver {
            def.engine_brake_driver
        } else {
            def.engine_brake_no_driver
        };
        total_force -= horizontal_vel.normalize() * engine_brake;
    }

    let accel = total_force / def.mass;
    state.velocity += accel * dt;

    state.position += state.velocity * dt;

    // Hard floor constraint.
    let mut min_chassis_y = f32::NEG_INFINITY;
    for local_xz in wheel_local_xz.iter().copied() {
        let wheel_world_pos = state.position + forward * local_xz.y + right * local_xz.x;
        let ground_y = terrain.get_height(wheel_world_pos.x, wheel_world_pos.z);
        let min_y = ground_y + def.wheel_radius;
        min_chassis_y = min_chassis_y.max(min_y);
    }

    if state.position.y < min_chassis_y {
        state.position.y = min_chassis_y;
        if state.velocity.y < 0.0 {
            state.velocity.y *= -def.bounce;
            if state.velocity.y.abs() < 0.5 {
                state.velocity.y = 0.0;
            }
        }
        state.grounded = true;
    }

    if state.grounded && speed < 0.3 && throttle < 0.1 && brake < 0.1 {
        state.velocity.x *= 0.9;
        state.velocity.z *= 0.9;
        if speed < 0.1 {
            state.velocity.x = 0.0;
            state.velocity.z = 0.0;
        }
    }

    let yaw_accel = total_torque_y / def.yaw_inertia.max(1.0);
    state.angular_velocity_yaw += yaw_accel * dt;
    state.angular_velocity_yaw *= (-def.yaw_damping * dt).exp();
    state.heading += state.angular_velocity_yaw * dt;
    state.heading = (state.heading + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;

    if state.grounded && grounded_wheels >= 2 {
        let front_avg_height = (ground_heights[0] + ground_heights[1]) * 0.5;
        let rear_avg_height = (ground_heights[2] + ground_heights[3]) * 0.5;
        let left_avg_height = (ground_heights[0] + ground_heights[2]) * 0.5;
        let right_avg_height = (ground_heights[1] + ground_heights[3]) * 0.5;

        let height_diff_pitch = rear_avg_height - front_avg_height;
        let pitch_target = (height_diff_pitch / def.wheel_base).atan();

        let height_diff_roll = right_avg_height - left_avg_height;
        let roll_target = (height_diff_roll / def.track_width).atan();

        let pitch_response = def.terrain_align_speed * dt;
        let roll_response = def.terrain_align_speed * dt;

        state.pitch += (pitch_target - state.pitch) * pitch_response.min(1.0);
        state.roll += (roll_target - state.roll) * roll_response.min(1.0);

        state.pitch = state
            .pitch
            .clamp(-def.max_terrain_pitch, def.max_terrain_pitch);
        state.roll = state
            .roll
            .clamp(-def.max_terrain_roll, def.max_terrain_roll);
    } else {
        state.pitch *= (-def.body_pitch_response * 0.5 * dt).exp();
        state.roll *= (-def.body_roll_response * 0.5 * dt).exp();
    }
}
