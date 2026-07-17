//! Car physics v2: slip-based tires, load transfer, and dynamic body attitude.
//!
//! Differences from `car.rs` (v1):
//! - Yaw comes ONLY from real tire torques (no kinematic-bicycle blend), so
//!   oversteer/countersteer emerge naturally.
//! - Tires use a Pacejka-lite slip-angle curve with a shared friction circle
//!   (longitudinal force spends grip budget before lateral), so grip breaks
//!   away progressively and powerslides/handbrake drifts are possible.
//! - Body pitch/roll are integrated spring-damper systems driven by terrain
//!   plus longitudinal/lateral acceleration (brake dive, accel squat,
//!   cornering roll), and wheel loads are read from the *attitude-displaced*
//!   suspension, so load transfer redistributes grip for real.
//! - Ride height integrates vertical velocity from suspension forces instead
//!   of snapping to a target (the body genuinely bounces on its springs).
//! - Shift acts as a handbrake while grounded (locks rear grip down).
//!
//! # Sign conventions (Bevy right-handed; see also `vehicle_body_rotation`)
//! - forward = rotation * -Z, right = rotation * +X, up = rotation * +Y.
//! - heading: rotation about +Y; POSITIVE heading = turn LEFT (CCW from above).
//! - steer input: +1 = D key = turn RIGHT, so wheel angle = -steer * max
//!   (positive wheel angle steers left, matching positive yaw).
//! - pitch: positive = nose UP. Brake dive is NEGATIVE pitch.
//! - roll: positive = lean RIGHT (rendered as -roll about Z). Cornering right
//!   rolls the body LEFT (outward), i.e. NEGATIVE roll.
//! These conventions are pinned by the tests at the bottom of this file —
//! mirrored-motion bugs have bitten this repo repeatedly; if you change any
//! sign here, the tests must be updated deliberately, not silenced.

use bevy::prelude::*;

use crate::terrain::WorldTerrain;
use crate::vehicle::components::{CarSuspensionState, VehicleInput, VehicleState, VehicleType};
use crate::vehicle::tuning::vehicle_def;

use super::common::{surface_mu, terrain_angles_from_normal, vehicle_body_rotation};

/// Steering wheel slew rate (rad/s at the road wheel).
const STEER_RATE: f32 = 3.5;
/// Effective steering shrinks with speed: max / (1 + speed * this).
const STEER_SPEED_SENSITIVITY: f32 = 0.055;
/// Slip angle (rad) where the tire curve reaches its working range.
const PEAK_SLIP_ANGLE: f32 = 0.14;
/// Pacejka-lite shape constants: f(x) = sin(C1 * atan(C2 * x)).
const TIRE_SHAPE_C1: f32 = 1.2;
const TIRE_SHAPE_C2: f32 = 1.6;
/// Brake distribution front/rear (front-biased like a real car).
const BRAKE_BIAS_FRONT: f32 = 0.62;
/// Handbrake (Shift while grounded): rear brake force and rear grip loss.
const HANDBRAKE_FORCE: f32 = 16000.0;
const HANDBRAKE_REAR_GRIP_MULT: f32 = 0.45;
/// Body attitude response: dive/squat per m/s² of longitudinal acceleration,
/// roll per m/s² of lateral acceleration.
const DIVE_GAIN: f32 = 0.010;
const ROLL_GAIN: f32 = 0.014;
/// Attitude spring-damper (rad/s² per rad, and per rad/s).
const ATTITUDE_STIFFNESS: f32 = 45.0;
const ATTITUDE_DAMPING: f32 = 9.0;
/// Light stabilizing yaw damping (tire forces do most of the work).
const YAW_DAMPING_V2: f32 = 0.35;
/// Speed below which auto-hold arrests residual creep (no position writes).
const AUTO_HOLD_SPEED: f32 = 0.25;
const AUTO_HOLD_DAMP: f32 = 12.0;

/// Normalized tire force vs normalized slip: progressive rise, gentle peak,
/// slow falloff — grip "breaks away" instead of switching off.
fn tire_curve(normalized_slip: f32) -> f32 {
    (TIRE_SHAPE_C1 * (TIRE_SHAPE_C2 * normalized_slip.abs()).atan())
        .sin()
        .copysign(normalized_slip)
}

#[allow(clippy::too_many_arguments)]
pub fn step_car_v2_physics(
    input: &VehicleInput,
    state: &mut VehicleState,
    suspension: &mut CarSuspensionState,
    terrain: &WorldTerrain,
    dt: f32,
    has_driver: bool,
    vehicle_type: VehicleType,
) {
    let def = vehicle_def(vehicle_type);

    // Planar basis from heading (slip model works in the yaw plane; pitch and
    // roll feed back through wheel loads, not through the tire basis).
    let planar_forward = Vec3::new(-state.heading.sin(), 0.0, -state.heading.cos());
    let planar_right = Vec3::new(state.heading.cos(), 0.0, -state.heading.sin());
    let planar_velocity = Vec3::new(state.velocity.x, 0.0, state.velocity.z);
    let forward_speed = planar_velocity.dot(planar_forward);
    let speed = planar_velocity.length();

    // --- Steering: rate-limited, speed-sensitive. steer +1 (right) => negative wheel angle.
    let max_steer_now = def.max_steer_angle / (1.0 + speed * STEER_SPEED_SENSITIVITY);
    let steer_target = if has_driver {
        -input.steer * max_steer_now
    } else {
        0.0
    };
    let steer_delta =
        (steer_target - suspension.steer_angle).clamp(-STEER_RATE * dt, STEER_RATE * dt);
    suspension.steer_angle += steer_delta;

    // --- Suspension pass: wheel positions use the FULL body rotation, so
    // pitch/roll displace the wheels and redistribute load (weight transfer).
    let rotation = vehicle_body_rotation(state.heading, state.pitch, state.roll);
    let half_track = def.track_width * 0.5;
    let half_wheel_base = def.wheel_base * 0.5;
    // FL, FR, RL, RR (local -Z = front, -X = left; same layout as v1).
    let wheel_local_offsets = [
        Vec3::new(-half_track, 0.0, -half_wheel_base),
        Vec3::new(half_track, 0.0, -half_wheel_base),
        Vec3::new(-half_track, 0.0, half_wheel_base),
        Vec3::new(half_track, 0.0, half_wheel_base),
    ];
    let is_front = [true, true, false, false];

    let biome = terrain.get_biome(state.position.x, state.position.z);
    let mu = surface_mu(&def, biome);
    let handbrake = has_driver && input.air_control;

    let mut total_force = Vec3::ZERO;
    let mut total_yaw_torque = 0.0_f32;
    let mut grounded_wheels = 0u32;
    let mut ground_normal_sum = Vec3::ZERO;
    let mut ground_pitch_sum = 0.0;
    let mut ground_roll_sum = 0.0;

    for (i, local) in wheel_local_offsets.iter().enumerate() {
        let wheel_world = state.position + rotation * *local;
        let ground_y = terrain.get_height(wheel_world.x, wheel_world.z);
        let ground_normal = terrain.get_normal(wheel_world.x, wheel_world.z);

        let contact_y = ground_y + def.wheel_radius;
        let compression =
            (contact_y + def.suspension_rest - wheel_world.y).clamp(0.0, def.suspension_rest);
        let compression_vel = (compression - suspension.last_compression[i]) / dt.max(1.0e-4);
        suspension.last_compression[i] = suspension.compression[i];
        suspension.compression[i] = compression;

        let normal_force = (compression * def.suspension_stiffness
            + compression_vel * def.suspension_damping)
            .max(0.0);
        if compression <= 1.0e-4 {
            continue;
        }
        grounded_wheels += 1;
        ground_normal_sum += ground_normal;
        let (wheel_pitch, wheel_roll) =
            terrain_angles_from_normal(ground_normal, state.heading, &def);
        ground_pitch_sum += wheel_pitch;
        ground_roll_sum += wheel_roll;

        // Wheel heading basis (front wheels rotate by steer angle about +Y;
        // positive steer angle = left, consistent with positive yaw).
        let (wheel_forward, wheel_right) = if is_front[i] {
            let steer_rot = Quat::from_rotation_y(suspension.steer_angle);
            (steer_rot * planar_forward, steer_rot * planar_right)
        } else {
            (planar_forward, planar_right)
        };

        // Contact-patch velocity including yaw rotation (r × ω term). This is
        // what lets the rear axle develop slip in a spin — pure oversteer.
        let r_world = rotation * *local;
        let r_planar = Vec3::new(r_world.x, 0.0, r_world.z);
        let yaw_vel = Vec3::Y * state.angular_velocity_yaw;
        let contact_velocity = planar_velocity + yaw_vel.cross(r_planar);
        let v_long = contact_velocity.dot(wheel_forward);
        let v_lat = contact_velocity.dot(wheel_right);

        // Longitudinal force: RWD drive, front-biased brakes, handbrake rear.
        let rear_grip_mult = if handbrake && !is_front[i] {
            HANDBRAKE_REAR_GRIP_MULT
        } else {
            1.0
        };
        let max_friction = mu * normal_force * rear_grip_mult;

        let mut longitudinal = 0.0_f32;
        if has_driver {
            if !is_front[i] {
                // Engine on the rear axle, power falls off toward top speed.
                let speed_ratio = (forward_speed / def.max_speed).clamp(0.0, 1.0);
                let power_falloff = 1.0 - speed_ratio * 0.6;
                longitudinal += input.throttle * def.engine_force * 0.5 * power_falloff;
                // Brake input reverses when (nearly) stopped.
                if input.brake > 0.0 && forward_speed < 0.5 {
                    let reverse_ratio = (-forward_speed / def.max_reverse_speed).clamp(0.0, 1.0);
                    longitudinal -= input.brake * def.reverse_force * 0.5 * (1.0 - reverse_ratio);
                }
            }
            if input.brake > 0.0 && forward_speed >= 0.5 {
                let bias = if is_front[i] {
                    BRAKE_BIAS_FRONT * 0.5
                } else {
                    (1.0 - BRAKE_BIAS_FRONT) * 0.5
                };
                longitudinal -= input.brake * def.brake_force * bias * v_long.signum();
            }
            if handbrake && !is_front[i] && v_long.abs() > 0.2 {
                longitudinal -= HANDBRAKE_FORCE * 0.5 * v_long.signum();
            }
        }
        // Engine braking / rolling resistance per wheel.
        let engine_brake = if has_driver {
            def.engine_brake_driver
        } else {
            def.engine_brake_no_driver
        };
        if input.throttle <= 0.0 && v_long.abs() > 0.15 {
            longitudinal -= (engine_brake * 0.25 + def.rolling_resistance * 0.25) * v_long.signum();
        }
        let longitudinal = longitudinal.clamp(-max_friction, max_friction);

        // Lateral force from slip angle, limited by the REMAINING friction
        // budget (shared circle): heavy braking/throttle eats cornering grip.
        let lateral_budget = (max_friction * max_friction - longitudinal * longitudinal)
            .max(0.0)
            .sqrt();
        let slip_angle = v_lat.atan2(v_long.abs().max(0.6));
        let lateral = (-tire_curve(slip_angle / PEAK_SLIP_ANGLE) * max_friction)
            .clamp(-lateral_budget, lateral_budget);

        let tire_force = wheel_forward * longitudinal + wheel_right * lateral;
        total_force += tire_force + ground_normal * normal_force;
        total_yaw_torque += r_planar.cross(tire_force).y;
    }

    state.grounded = grounded_wheels > 0;

    // --- Rigid-body-ish integration.
    let mut acceleration = total_force / def.mass + Vec3::Y * def.gravity;
    // Aero drag + a touch of air damping when fully airborne.
    let drag = def.drag_coefficient * speed * planar_velocity;
    acceleration -= drag / def.mass;
    if grounded_wheels == 0 {
        acceleration -= state.velocity * def.air_linear_damping;
    }

    let planar_accel_long = (acceleration - Vec3::Y * acceleration.y).dot(planar_forward);
    let planar_accel_lat = (acceleration - Vec3::Y * acceleration.y).dot(planar_right);

    state.velocity += acceleration * dt;

    // Auto-hold: bleed residual creep when parked (velocity-only, no teleport).
    if state.grounded
        && (!has_driver || (input.throttle <= 0.0 && input.brake <= 0.0))
        && speed < AUTO_HOLD_SPEED
    {
        let damp = (-AUTO_HOLD_DAMP * dt).exp();
        state.velocity.x *= damp;
        state.velocity.z *= damp;
    }

    state.position += state.velocity * dt;

    // --- Yaw purely from tire torques (light damping for numeric sanity).
    let yaw_inertia = (def.mass / 12.0) * (def.size.x * def.size.x + def.size.z * def.size.z);
    state.angular_velocity_yaw += (total_yaw_torque / yaw_inertia.max(1.0)) * dt;
    state.angular_velocity_yaw *= (-YAW_DAMPING_V2 * dt).exp();
    state.heading = (state.heading + state.angular_velocity_yaw * dt + std::f32::consts::PI)
        .rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;

    // --- Body attitude: terrain conformity + acceleration lean, integrated as
    // a spring-damper so it dives, squats, rolls, and settles like suspension.
    // Signs: decel (braking, accel_long < 0) => nose DOWN => negative pitch.
    // Lateral accel to the RIGHT (right turn) => body rolls LEFT => negative roll.
    let (terrain_pitch, terrain_roll) = if grounded_wheels > 0 {
        (
            ground_pitch_sum / grounded_wheels as f32,
            ground_roll_sum / grounded_wheels as f32,
        )
    } else {
        (state.pitch, state.roll)
    };
    let target_pitch = (terrain_pitch + DIVE_GAIN * planar_accel_long)
        .clamp(-def.max_terrain_pitch, def.max_terrain_pitch);
    let target_roll = (terrain_roll - ROLL_GAIN * planar_accel_lat)
        .clamp(-def.max_terrain_roll, def.max_terrain_roll);

    if grounded_wheels > 0 {
        let pitch_accel = (target_pitch - state.pitch) * ATTITUDE_STIFFNESS
            - state.angular_velocity_pitch * ATTITUDE_DAMPING;
        state.angular_velocity_pitch += pitch_accel * dt;
        state.pitch += state.angular_velocity_pitch * dt;

        let roll_accel = (target_roll - state.roll) * ATTITUDE_STIFFNESS
            - state.angular_velocity_roll * ATTITUDE_DAMPING;
        state.angular_velocity_roll += roll_accel * dt;
        state.roll += state.angular_velocity_roll * dt;
    } else {
        // Airborne: keep tumbling gently with damping (optional air control).
        if has_driver && input.air_control {
            state.angular_velocity_pitch +=
                (input.throttle - input.brake) * def.air_pitch_torque * dt;
            state.angular_velocity_roll += input.steer * def.air_roll_torque * dt;
        }
        let damp = (-def.air_angular_damping * dt).exp();
        state.angular_velocity_pitch *= damp;
        state.angular_velocity_roll *= damp;
        state.pitch += state.angular_velocity_pitch * dt;
        state.roll += state.angular_velocity_roll * dt;
    }

    // --- Safety floor: never let the chassis bottom sink through the ground.
    let center_ground = terrain.get_height(state.position.x, state.position.z);
    let min_center_y = center_ground + def.size.y * 0.25;
    if state.position.y < min_center_y {
        state.position.y = min_center_y;
        if state.velocity.y < 0.0 {
            state.velocity.y = -state.velocity.y * def.bounce;
        }
    }
}

/// Settled ride height above ground for spawning (static compression).
pub fn car_v2_static_ride_height(vehicle_type: VehicleType) -> f32 {
    let def = vehicle_def(vehicle_type);
    let static_compression =
        (def.mass * def.gravity.abs() / 4.0) / def.suspension_stiffness.max(1.0);
    def.wheel_radius + (def.suspension_rest - static_compression).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Find a reasonably flat, dry test patch on the real map so sign tests
    /// don't depend on hand-picked coordinates.
    fn flat_test_spot(terrain: &WorldTerrain) -> Vec3 {
        let mut best = (f32::INFINITY, Vec3::ZERO);
        for gx in -20..=20 {
            for gz in -20..=20 {
                let x = gx as f32 * 25.0;
                let z = gz as f32 * 25.0;
                let center = terrain.get_height(x, z);
                if center < -1.5 {
                    continue; // avoid water
                }
                let mut spread = 0.0_f32;
                for (dx, dz) in [(-6.0, 0.0), (6.0, 0.0), (0.0, -6.0), (0.0, 6.0)] {
                    spread = spread.max((terrain.get_height(x + dx, z + dz) - center).abs());
                }
                if spread < best.0 {
                    best = (spread, Vec3::new(x, center, z));
                }
            }
        }
        assert!(best.0 < 0.25, "no flat spot found (best spread {})", best.0);
        best.1
    }

    fn spawn_state(terrain: &WorldTerrain) -> (VehicleState, CarSuspensionState) {
        let spot = flat_test_spot(terrain);
        let ride = car_v2_static_ride_height(VehicleType::CarV2);
        let state = VehicleState {
            position: Vec3::new(spot.x, spot.y + ride + 0.05, spot.z),
            grounded: true,
            ..Default::default()
        };
        (state, CarSuspensionState::default())
    }

    fn step_n(
        input: &VehicleInput,
        state: &mut VehicleState,
        suspension: &mut CarSuspensionState,
        terrain: &WorldTerrain,
        n: usize,
    ) {
        for _ in 0..n {
            step_car_v2_physics(
                input,
                state,
                suspension,
                terrain,
                1.0 / 60.0,
                true,
                VehicleType::CarV2,
            );
        }
    }

    #[test]
    fn settles_on_suspension_without_input() {
        let terrain = WorldTerrain::default();
        let (mut state, mut suspension) = spawn_state(&terrain);
        step_n(
            &VehicleInput::default(),
            &mut state,
            &mut suspension,
            &terrain,
            240,
        );
        assert!(state.grounded, "car should settle grounded");
        assert!(
            state.velocity.length() < 0.6,
            "car should come to rest, velocity {:?}",
            state.velocity
        );
    }

    /// Steer +1 is the D key = turn RIGHT: heading must decrease (negative
    /// yaw), and the body must roll LEFT (outward), i.e. roll < 0.
    /// This test pins the handedness conventions — do not "fix" a mirrored
    /// visual by flipping signs here; fix the visual instead.
    #[test]
    fn steering_right_turns_right_and_rolls_outward() {
        let terrain = WorldTerrain::default();
        let (mut state, mut suspension) = spawn_state(&terrain);
        // Get up to speed first.
        step_n(
            &VehicleInput {
                throttle: 1.0,
                ..Default::default()
            },
            &mut state,
            &mut suspension,
            &terrain,
            240,
        );
        let heading_before = state.heading;
        let mut min_roll = f32::INFINITY;
        let input = VehicleInput {
            throttle: 0.6,
            steer: 1.0,
            ..Default::default()
        };
        for _ in 0..90 {
            step_car_v2_physics(
                &input,
                &mut state,
                &mut suspension,
                &terrain,
                1.0 / 60.0,
                true,
                VehicleType::CarV2,
            );
            min_roll = min_roll.min(state.roll);
        }
        let heading_delta = (state.heading - heading_before + std::f32::consts::PI)
            .rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        assert!(
            heading_delta < -0.05,
            "steer +1 must turn right (heading decreases), got delta {heading_delta}"
        );
        assert!(
            state.angular_velocity_yaw < 0.0,
            "yaw rate must be negative in a right turn"
        );
        assert!(
            min_roll < -0.005,
            "body must roll LEFT (outward, negative) in a right turn, min roll {min_roll}"
        );
    }

    /// Braking from speed must dive the nose: pitch dips negative.
    #[test]
    fn braking_dives_the_nose() {
        let terrain = WorldTerrain::default();
        let (mut state, mut suspension) = spawn_state(&terrain);
        step_n(
            &VehicleInput {
                throttle: 1.0,
                ..Default::default()
            },
            &mut state,
            &mut suspension,
            &terrain,
            300,
        );
        let mut min_pitch = f32::INFINITY;
        let input = VehicleInput {
            brake: 1.0,
            ..Default::default()
        };
        for _ in 0..60 {
            step_car_v2_physics(
                &input,
                &mut state,
                &mut suspension,
                &terrain,
                1.0 / 60.0,
                true,
                VehicleType::CarV2,
            );
            min_pitch = min_pitch.min(state.pitch);
        }
        assert!(
            min_pitch < -0.004,
            "braking must dive the nose (negative pitch), min pitch {min_pitch}"
        );
    }

    /// Throttle accelerates forward along -Z at heading 0.
    #[test]
    fn throttle_moves_forward() {
        let terrain = WorldTerrain::default();
        let (mut state, mut suspension) = spawn_state(&terrain);
        let start = state.position;
        step_n(
            &VehicleInput {
                throttle: 1.0,
                ..Default::default()
            },
            &mut state,
            &mut suspension,
            &terrain,
            180,
        );
        let planar_forward = Vec3::new(0.0, 0.0, -1.0); // heading ~0
        let travelled = (state.position - start).dot(planar_forward);
        assert!(
            travelled > 5.0,
            "throttle must move the car forward (-Z at heading 0), travelled {travelled}"
        );
    }
}
