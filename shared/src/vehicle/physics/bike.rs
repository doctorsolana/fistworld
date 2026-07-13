use bevy::prelude::*;

use crate::terrain::WorldTerrain;
use crate::vehicle::components::{VehicleInput, VehicleState, VehicleType};
use crate::vehicle::tuning::vehicle_def;

use super::common::{
    bike_up_vector, get_basis_vectors, shortest_angle_diff, surface_mu, terrain_angles_from_normal,
};

pub fn step_vehicle_physics(
    input: &VehicleInput,
    state: &mut VehicleState,
    terrain: &WorldTerrain,
    dt: f32,
    has_driver: bool,
    vehicle_type: VehicleType,
) {
    let def = vehicle_def(vehicle_type);
    let half_height = def.size.y * 0.5;
    let prev_pos = state.position;

    let ground_y = terrain.get_height(state.position.x, state.position.z);
    let ground_normal = terrain.get_normal(state.position.x, state.position.z);

    let bottom_y = state.position.y - half_height;
    let height_above_ground = bottom_y - ground_y;
    let was_grounded = state.grounded;

    state.grounded = height_above_ground <= def.ground_threshold && height_above_ground >= -0.5;

    let bike_up = bike_up_vector(state.heading, state.pitch, state.roll);
    let wheel_contact = bike_up.dot(ground_normal).max(0.0);

    let wheel_contact_factor = if wheel_contact > 0.5 {
        wheel_contact
    } else if wheel_contact > 0.1 {
        wheel_contact * 0.3
    } else {
        0.0
    };

    let wheels_down = wheel_contact_factor > 0.1;

    let biome = terrain.get_biome(state.position.x, state.position.z);
    let mu = surface_mu(&def, biome);

    let horizontal_speed = Vec3::new(state.velocity.x, 0.0, state.velocity.z).length();

    if state.grounded && wheels_down {
        let speed_t = (horizontal_speed / def.max_speed).clamp(0.0, 1.0);
        let steer_rate =
            def.steering_speed_min * (1.0 - speed_t) + def.steering_speed_max * speed_t;
        let turn_effectiveness = (horizontal_speed / 2.0).clamp(0.0, 1.0);
        let target_yaw_vel = -input.steer * steer_rate * turn_effectiveness * wheel_contact_factor;
        state.angular_velocity_yaw +=
            (target_yaw_vel - state.angular_velocity_yaw) * def.steering_response * dt;
    } else {
        state.angular_velocity_yaw += -input.steer * def.air_yaw_torque * dt;
        state.angular_velocity_yaw *= (-def.air_angular_damping * dt).exp();
    }

    state.heading += state.angular_velocity_yaw * dt;
    state.heading = (state.heading + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;

    if state.grounded && wheels_down {
        let (terrain_pitch, terrain_roll) =
            terrain_angles_from_normal(ground_normal, state.heading, &def);

        let turn_lean = if input.steer.abs() > 0.05 && horizontal_speed > 1.0 {
            let speed_factor = (horizontal_speed / def.max_speed).clamp(0.0, 1.0);
            input.steer * def.turn_lean_angle * (0.3 + 0.7 * speed_factor)
        } else {
            0.0
        };

        let target_pitch = terrain_pitch;
        let target_roll = terrain_roll + turn_lean;

        let align_strength = def.terrain_align_speed * wheel_contact_factor;

        let pitch_error = shortest_angle_diff(target_pitch, state.pitch);
        state.angular_velocity_pitch += pitch_error * align_strength * dt;
        state.angular_velocity_pitch *= 0.8;
        state.pitch += state.angular_velocity_pitch * dt;

        let roll_error = shortest_angle_diff(target_roll, state.roll);
        state.angular_velocity_roll += roll_error * def.lean_speed * wheel_contact_factor * dt;
        state.angular_velocity_roll *= 0.85;
        state.roll += state.angular_velocity_roll * dt;
    } else {
        if input.air_control {
            state.angular_velocity_pitch +=
                (input.throttle - input.brake) * def.air_pitch_torque * dt;
            state.angular_velocity_roll += input.steer * def.air_roll_torque * dt;
        }

        let ang_damp = (-def.air_angular_damping * dt).exp();
        state.angular_velocity_pitch *= ang_damp;
        state.angular_velocity_roll *= ang_damp;

        state.pitch += state.angular_velocity_pitch * dt;
        state.roll += state.angular_velocity_roll * dt;
    }

    if state.grounded {
        let (forward_flat, _) = get_basis_vectors(state.heading);
        let forward_tangent =
            (forward_flat - ground_normal * forward_flat.dot(ground_normal)).normalize_or_zero();
        let right_tangent = forward_tangent.cross(ground_normal).normalize_or_zero();

        let forward_speed = state.velocity.dot(forward_tangent);
        let lateral_speed = state.velocity.dot(right_tangent);

        let g_mag = -def.gravity;
        let cos_slope = ground_normal.y.max(0.0);
        let normal_force = def.mass * g_mag * cos_slope;

        let max_tire_force = mu * normal_force * wheel_contact_factor;

        let gravity_vec = Vec3::new(0.0, def.gravity, 0.0);
        let gravity_parallel = gravity_vec - ground_normal * gravity_vec.dot(ground_normal);
        let g_forward = gravity_parallel.dot(forward_tangent);
        let g_lateral = gravity_parallel.dot(right_tangent);

        let speed_ratio = (forward_speed.abs() / def.max_speed).clamp(0.0, 1.0);
        let power_falloff = 1.0 - speed_ratio * 0.6;
        let engine_request = input.throttle * def.engine_force * power_falloff;
        let engine_force = engine_request.min(max_tire_force);

        let is_reversing = forward_speed < 2.0 && input.brake > 0.1 && input.throttle < 0.1;
        let reverse_speed_ratio =
            ((-forward_speed).max(0.0) / def.max_reverse_speed).clamp(0.0, 1.0);
        let reverse_power_falloff = 1.0 - reverse_speed_ratio * 0.7;
        let reverse_request = if is_reversing {
            input.brake * def.reverse_force * reverse_power_falloff
        } else {
            0.0
        };
        let reverse_force = reverse_request.min(max_tire_force);

        let brake_request = if !is_reversing {
            input.brake * def.brake_force
        } else {
            0.0
        };
        let brake_force = brake_request.min(max_tire_force);

        let drag = def.drag_coefficient * forward_speed * forward_speed.abs();

        let rolling = def.rolling_resistance
            * forward_speed.signum()
            * (forward_speed.abs() > 0.1) as i32 as f32
            * wheel_contact_factor;

        let engine_brake_coeff = if has_driver {
            def.engine_brake_driver
        } else {
            def.engine_brake_no_driver
        };
        let engine_brake = if input.throttle < 0.1 && wheels_down {
            engine_brake_coeff * forward_speed * wheel_contact_factor
        } else {
            0.0
        };

        let net_force = engine_force
            - reverse_force
            - brake_force * forward_speed.signum()
            - drag
            - rolling
            - engine_brake;
        let accel = net_force / def.mass;

        let mut new_forward_speed = forward_speed + accel * dt;
        new_forward_speed += g_forward * dt;

        let mut new_lateral_speed = lateral_speed;
        new_lateral_speed += g_lateral * dt;
        let lateral_grip = def.lateral_friction * mu * wheel_contact_factor;
        new_lateral_speed *= (-lateral_grip * dt).exp();

        new_forward_speed = new_forward_speed.clamp(-def.max_reverse_speed, def.max_speed);

        if wheels_down
            && new_forward_speed.abs() < 0.15
            && input.throttle < 0.1
            && input.brake < 0.1
            && g_forward.abs() < 2.0
        {
            new_forward_speed = 0.0;
        }
        if wheels_down && new_lateral_speed.abs() < 0.1 && g_lateral.abs() < 1.0 {
            new_lateral_speed = 0.0;
        }

        if !wheels_down && state.grounded {
            let scrape_friction = 0.25;
            let forward_damp = (-scrape_friction * 6.0 * dt).exp();
            let lateral_damp = (-scrape_friction * 2.0 * dt).exp();
            new_forward_speed *= forward_damp;
            new_lateral_speed *= lateral_damp;
        }

        state.velocity = forward_tangent * new_forward_speed + right_tangent * new_lateral_speed;
        state.position += state.velocity * dt;

        let new_ground_y = terrain.get_height(state.position.x, state.position.z);
        let new_bottom_y = state.position.y - half_height;
        let new_height_above = new_bottom_y - new_ground_y;

        if new_height_above <= def.ground_threshold {
            state.position.y = new_ground_y + half_height;
            state.velocity = (state.position - prev_pos) / dt;
            state.grounded = true;
        } else {
            state.grounded = false;
        }
    }

    if !state.grounded {
        state.velocity.y += def.gravity * dt;
        state.velocity *= (-def.air_linear_damping * dt).exp();
        state.position += state.velocity * dt;
    }

    let final_ground = terrain.get_height(state.position.x, state.position.z);
    let min_y = final_ground + half_height;

    if state.position.y < min_y {
        state.position.y = min_y;

        if state.velocity.y < 0.0 {
            let impact_speed = -state.velocity.y;
            state.velocity.y *= -def.bounce;
            if state.velocity.y.abs() < 0.5 {
                state.velocity.y = 0.0;
            }

            if impact_speed > 10.0 {
                let (forward_flat, right_flat) = get_basis_vectors(state.heading);
                let forward_speed = state.velocity.dot(forward_flat);
                let lateral_speed = state.velocity.dot(right_flat);

                let forward_damp = 0.85;
                let lateral_damp = def.landing_drift_preserve;

                state.velocity = forward_flat * (forward_speed * forward_damp)
                    + right_flat * (lateral_speed * lateral_damp)
                    + Vec3::new(0.0, state.velocity.y, 0.0);
            }
        }
        state.grounded = true;
    }

    if !was_grounded && state.grounded {
        state.angular_velocity_roll *= 0.7;
        state.angular_velocity_pitch *= 0.7;
    }
}
