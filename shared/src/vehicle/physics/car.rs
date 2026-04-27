use bevy::prelude::*;

use crate::terrain::WorldTerrain;
use crate::vehicle::components::{CarSuspensionState, VehicleInput, VehicleState, VehicleType};
use crate::vehicle::tuning::{vehicle_def, VehicleDef};

use super::common::{surface_mu, vehicle_body_axes, vehicle_body_rotation};

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
    let body_rotation = vehicle_body_rotation(state.heading, state.pitch, state.roll);
    let (body_forward, body_right, _) = vehicle_body_axes(state.heading, state.pitch, state.roll);
    let forward_flat = Vec3::new(body_forward.x, 0.0, body_forward.z).normalize_or_zero();

    let throttle = if has_driver { input.throttle } else { 0.0 };
    let brake = if has_driver { input.brake } else { 0.0 };
    let steer = if has_driver { input.steer } else { 0.0 }.clamp(-1.0, 1.0);
    let horizontal_speed = Vec3::new(state.velocity.x, 0.0, state.velocity.z).length();
    let forward_speed = state.velocity.dot(forward_flat);
    let speed_t = (horizontal_speed / def.max_speed.max(1.0)).clamp(0.0, 1.0);
    let steer_speed = def.steering_speed_min * (1.0 - speed_t) + def.steering_speed_max * speed_t;
    let target_steer_angle = -steer * def.max_steer_angle;
    let steer_response_t = 1.0 - (-def.steering_response * dt).exp();
    let steer_step = ((target_steer_angle - suspension.steer_angle) * steer_response_t)
        .clamp(-steer_speed * dt, steer_speed * dt);
    suspension.steer_angle =
        (suspension.steer_angle + steer_step).clamp(-def.max_steer_angle, def.max_steer_angle);

    let half_wheel_base = def.wheel_base * 0.5;
    let half_track = def.track_width * 0.5;

    let wheel_local_offsets = [
        Vec3::new(-half_track, 0.0, -half_wheel_base),
        Vec3::new(half_track, 0.0, -half_wheel_base),
        Vec3::new(-half_track, 0.0, half_wheel_base),
        Vec3::new(half_track, 0.0, half_wheel_base),
    ];

    let mut total_force = Vec3::ZERO;
    let mut total_torque_y = 0.0;
    let mut grounded_wheels = 0;
    let mut ground_heights = [0.0f32; 4];

    for (i, local_offset) in wheel_local_offsets.iter().copied().enumerate() {
        let is_front = i < 2;
        let is_driven_wheel = !is_front;

        let wheel_rel = body_right * local_offset.x + body_forward * local_offset.z;
        let suspension_origin = state.position + wheel_rel;

        let ground_y = terrain.get_height(suspension_origin.x, suspension_origin.z);
        let ground_normal = terrain.get_normal(suspension_origin.x, suspension_origin.z);
        ground_heights[i] = ground_y;

        let wheel_contact_y = ground_y + def.wheel_radius;
        let compression = (wheel_contact_y + def.suspension_rest - suspension_origin.y)
            .clamp(0.0, def.suspension_rest);

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

        let contact_point = Vec3::new(suspension_origin.x, ground_y, suspension_origin.z);

        let steer_angle = if is_front {
            suspension.steer_angle
        } else {
            0.0
        };
        let local_wheel_forward = Quat::from_rotation_y(steer_angle) * Vec3::NEG_Z;
        let raw_wheel_forward = (body_rotation * local_wheel_forward).normalize_or_zero();
        let wheel_forward = (raw_wheel_forward
            - ground_normal * raw_wheel_forward.dot(ground_normal))
        .normalize_or_zero();
        if wheel_forward.length_squared() <= 1e-4 {
            continue;
        }
        let wheel_right = wheel_forward.cross(ground_normal).normalize_or_zero();
        if wheel_right.length_squared() <= 1e-4 {
            continue;
        }

        let r = contact_point - state.position;
        let omega = Vec3::Y * state.angular_velocity_yaw;
        let wheel_velocity = state.velocity + omega.cross(r);
        let wheel_velocity = wheel_velocity - ground_normal * wheel_velocity.dot(ground_normal);

        let long_speed = wheel_velocity.dot(wheel_forward);
        let lat_speed = wheel_velocity.dot(wheel_right);

        let mu = surface_mu(&def, terrain.get_biome(contact_point.x, contact_point.z));
        let max_friction = mu * normal_force;

        let speed_ratio = (long_speed.abs() / def.max_speed).clamp(0.0, 1.0);
        let power_falloff = 1.0 - speed_ratio * 0.7;
        let drive_force = if is_driven_wheel {
            throttle * def.engine_force * 0.5 * power_falloff
        } else {
            0.0
        };

        let brake_force = brake * def.brake_force * 0.25;
        let brake_dir = if long_speed.abs() > 0.1 {
            long_speed.signum()
        } else {
            0.0
        };

        let longitudinal_force =
            (drive_force - brake_force * brake_dir).clamp(-max_friction, max_friction);

        let low_speed_grip = if horizontal_speed < 10.0 {
            1.35 - 0.035 * horizontal_speed
        } else {
            1.0
        };
        let lateral_force =
            (-lat_speed * def.lateral_friction * low_speed_grip).clamp(-max_friction, max_friction);

        let tire_force = wheel_forward * longitudinal_force + wheel_right * lateral_force;
        total_force += tire_force + ground_normal * normal_force;
        total_torque_y += r.cross(tire_force).y;
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

    let prev_position = state.position;
    state.position += state.velocity * dt;

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
    if state.grounded && grounded_wheels >= 2 {
        let traction = (grounded_wheels as f32 / 4.0).clamp(0.0, 1.0);
        let understeer = 1.0 / (1.0 + horizontal_speed * 0.08);
        let target_yaw_rate = forward_speed * suspension.steer_angle.tan()
            / def.wheel_base.max(0.1)
            * traction
            * understeer;
        let yaw_follow_t = 1.0 - (-(def.steering_response * (0.75 + traction * 0.5)) * dt).exp();
        state.angular_velocity_yaw += (target_yaw_rate - state.angular_velocity_yaw) * yaw_follow_t;
    }
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

        // Match the shared vehicle roll convention: terrain higher on the right means negative roll.
        let height_diff_roll = left_avg_height - right_avg_height;
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

    let (aligned_forward, aligned_right, _) =
        vehicle_body_axes(state.heading, state.pitch, state.roll);
    let mut contact_count = 0;
    let mut target_y_sum = 0.0;
    let mut min_chassis_y = f32::NEG_INFINITY;
    let mut surface_normal_sum = Vec3::ZERO;
    let ride_height = static_ride_height(&def);

    for local_offset in wheel_local_offsets.iter().copied() {
        let wheel_rel = aligned_right * local_offset.x + aligned_forward * local_offset.z;
        let suspension_origin = state.position + wheel_rel;
        let ground_y = terrain.get_height(suspension_origin.x, suspension_origin.z);
        let ground_normal = terrain.get_normal(suspension_origin.x, suspension_origin.z);
        let compression = (ground_y + def.wheel_radius + def.suspension_rest - suspension_origin.y)
            .clamp(0.0, def.suspension_rest);

        min_chassis_y = min_chassis_y.max(ground_y + def.wheel_radius - wheel_rel.y);

        if compression <= 0.001 {
            continue;
        }

        contact_count += 1;
        target_y_sum += ground_y + ride_height - wheel_rel.y;
        surface_normal_sum += ground_normal;
    }

    if contact_count >= 2 {
        let target_y = target_y_sum / contact_count as f32;
        let ride_follow_t = 1.0 - (-(def.terrain_align_speed * 1.8) * dt).exp();
        state.position.y += (target_y - state.position.y) * ride_follow_t;

        let surface_normal = surface_normal_sum.normalize_or_zero();
        let surface_forward = (aligned_forward
            - surface_normal * aligned_forward.dot(surface_normal))
        .normalize_or_zero();
        if surface_forward.length_squared() > 1e-4 {
            let surface_right = surface_forward.cross(surface_normal).normalize_or_zero();
            let mut forward_speed = state.velocity.dot(surface_forward);
            let mut lateral_speed = state.velocity.dot(surface_right);
            let lateral_damp = (-def.lateral_friction * 0.2 * dt).exp();
            lateral_speed *= lateral_damp;

            let auto_hold =
                (!has_driver || brake > 0.05 || (throttle.abs() < 0.05 && brake < 0.05))
                    && forward_speed.abs() < 1.1
                    && lateral_speed.abs() < 0.9;
            if auto_hold {
                let gravity = Vec3::new(0.0, def.gravity, 0.0);
                let gravity_parallel = gravity - surface_normal * gravity.dot(surface_normal);
                forward_speed -= gravity_parallel.dot(surface_forward) * dt;
                lateral_speed -= gravity_parallel.dot(surface_right) * dt;

                let hold_t = 1.0 - (-32.0 * dt).exp();
                forward_speed += (0.0 - forward_speed) * hold_t;
                lateral_speed += (0.0 - lateral_speed) * hold_t;

                if forward_speed.abs() < 0.08 {
                    forward_speed = 0.0;
                }
                if lateral_speed.abs() < 0.08 {
                    lateral_speed = 0.0;
                }

                state.position.x = prev_position.x;
                state.position.z = prev_position.z;
            }

            state.velocity = surface_forward * forward_speed + surface_right * lateral_speed;
        }

        state.grounded = true;
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
}

fn static_ride_height(def: &VehicleDef) -> f32 {
    let static_compression = (def.mass * -def.gravity) / (4.0 * def.suspension_stiffness.max(1.0));
    (def.wheel_radius + def.suspension_rest - static_compression).clamp(
        def.wheel_radius * 0.95,
        def.wheel_radius + def.suspension_rest,
    )
}
