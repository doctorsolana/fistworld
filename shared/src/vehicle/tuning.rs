use bevy::prelude::*;

use super::components::VehicleType;

// =============================================================================
// VEHICLE TUNING CONSTANTS
// =============================================================================

pub mod motorbike {
    pub const MASS: f32 = 180.0;
    pub const ENGINE_FORCE: f32 = 7500.0; // Newtons (boosted for higher top speed)
    pub const MAX_SPEED: f32 = 45.0; // m/s (~162 km/h) - FAST speeder!
    pub const MAX_REVERSE_SPEED: f32 = 8.0; // m/s (~29 km/h) - slow reverse
    pub const REVERSE_FORCE: f32 = 2500.0; // Newtons (weaker than forward)
    pub const BRAKE_FORCE: f32 = 8000.0; // Newtons (stronger brakes for faster bike)
    pub const DRAG_COEFFICIENT: f32 = 0.18; // Lower drag for speed
    pub const ROLLING_RESISTANCE: f32 = 35.0; // Ground friction when rolling

    /// Engine braking when not on throttle (in-gear)
    pub const ENGINE_BRAKE_DRIVER: f32 = 40.0;
    /// Weaker when unmanned (neutral/parked)
    pub const ENGINE_BRAKE_NO_DRIVER: f32 = 15.0;

    pub const STEERING_SPEED_MIN: f32 = 2.5; // rad/s at low speed
    pub const STEERING_SPEED_MAX: f32 = 1.0; // rad/s at max speed
    pub const STEERING_RESPONSE: f32 = 12.0;

    pub const TURN_LEAN_ANGLE: f32 = 0.35; // radians
    pub const LEAN_SPEED: f32 = 8.0;

    /// Friction coefficient (mu) for different surfaces
    pub const MU_DESERT: f32 = 0.75;
    pub const MU_GRASSLANDS: f32 = 0.9;

    /// Lateral (sideways) friction - lower = more drift
    pub const LATERAL_FRICTION: f32 = 8.0;
    /// How much lateral speed to preserve on landing (0 = none, 1 = all)
    pub const LANDING_DRIFT_PRESERVE: f32 = 0.85;

    pub const SIZE: (f32, f32, f32) = (2.2, 1.0, 0.8);
    pub const GRAVITY: f32 = -20.0; // m/s^2 (slightly stronger for game feel)

    /// Air control (only with Shift held)
    pub const AIR_PITCH_TORQUE: f32 = 8.0;
    pub const AIR_ROLL_TORQUE: f32 = 6.0;
    pub const AIR_YAW_TORQUE: f32 = 2.0;
    pub const AIR_ANGULAR_DAMPING: f32 = 0.8;
    pub const AIR_LINEAR_DAMPING: f32 = 0.02; // Very light air drag

    pub const TERRAIN_ALIGN_SPEED: f32 = 15.0;
    pub const MAX_TERRAIN_PITCH: f32 = 1.2; // ~70 degrees
    pub const MAX_TERRAIN_ROLL: f32 = 0.8; // ~45 degrees

    /// Height above ground to be considered "grounded"
    /// SMALLER = easier to get air off bumps/crests
    pub const GROUND_THRESHOLD: f32 = 0.08;

    /// Bounce coefficient on landing (0 = no bounce, 1 = full bounce)
    pub const BOUNCE: f32 = 0.15;
}

/// Tuning values for a vehicle type.
#[derive(Clone, Copy, Debug)]
pub struct VehicleDef {
    pub mass: f32,
    pub engine_force: f32,
    pub max_speed: f32,
    pub max_reverse_speed: f32,
    pub reverse_force: f32,
    pub brake_force: f32,
    pub drag_coefficient: f32,
    pub rolling_resistance: f32,
    pub engine_brake_driver: f32,
    pub engine_brake_no_driver: f32,
    pub steering_speed_min: f32,
    pub steering_speed_max: f32,
    pub steering_response: f32,
    pub turn_lean_angle: f32,
    pub lean_speed: f32,
    pub mu_desert: f32,
    pub mu_grasslands: f32,
    pub lateral_friction: f32,
    pub landing_drift_preserve: f32,
    pub size: Vec3,
    pub gravity: f32,
    pub air_pitch_torque: f32,
    pub air_roll_torque: f32,
    pub air_yaw_torque: f32,
    pub air_angular_damping: f32,
    pub air_linear_damping: f32,
    pub terrain_align_speed: f32,
    pub max_terrain_pitch: f32,
    pub max_terrain_roll: f32,
    pub ground_threshold: f32,
    pub bounce: f32,
    pub wheel_base: f32,
    pub track_width: f32,
    pub wheel_radius: f32,
    pub suspension_rest: f32,
    pub suspension_stiffness: f32,
    pub suspension_damping: f32,
    pub max_steer_angle: f32,
    pub yaw_inertia: f32,
    pub yaw_damping: f32,
    pub body_pitch_response: f32,
    pub body_roll_response: f32,
    pub suspension_pitch_factor: f32,
    pub suspension_roll_factor: f32,
}

pub fn vehicle_def(vehicle_type: VehicleType) -> VehicleDef {
    match vehicle_type {
        VehicleType::Motorbike => VehicleDef {
            mass: motorbike::MASS,
            engine_force: motorbike::ENGINE_FORCE,
            max_speed: motorbike::MAX_SPEED,
            max_reverse_speed: motorbike::MAX_REVERSE_SPEED,
            reverse_force: motorbike::REVERSE_FORCE,
            brake_force: motorbike::BRAKE_FORCE,
            drag_coefficient: motorbike::DRAG_COEFFICIENT,
            rolling_resistance: motorbike::ROLLING_RESISTANCE,
            engine_brake_driver: motorbike::ENGINE_BRAKE_DRIVER,
            engine_brake_no_driver: motorbike::ENGINE_BRAKE_NO_DRIVER,
            steering_speed_min: motorbike::STEERING_SPEED_MIN,
            steering_speed_max: motorbike::STEERING_SPEED_MAX,
            steering_response: motorbike::STEERING_RESPONSE,
            turn_lean_angle: motorbike::TURN_LEAN_ANGLE,
            lean_speed: motorbike::LEAN_SPEED,
            mu_desert: motorbike::MU_DESERT,
            mu_grasslands: motorbike::MU_GRASSLANDS,
            lateral_friction: motorbike::LATERAL_FRICTION,
            landing_drift_preserve: motorbike::LANDING_DRIFT_PRESERVE,
            size: Vec3::new(motorbike::SIZE.0, motorbike::SIZE.1, motorbike::SIZE.2),
            gravity: motorbike::GRAVITY,
            air_pitch_torque: motorbike::AIR_PITCH_TORQUE,
            air_roll_torque: motorbike::AIR_ROLL_TORQUE,
            air_yaw_torque: motorbike::AIR_YAW_TORQUE,
            air_angular_damping: motorbike::AIR_ANGULAR_DAMPING,
            air_linear_damping: motorbike::AIR_LINEAR_DAMPING,
            terrain_align_speed: motorbike::TERRAIN_ALIGN_SPEED,
            max_terrain_pitch: motorbike::MAX_TERRAIN_PITCH,
            max_terrain_roll: motorbike::MAX_TERRAIN_ROLL,
            ground_threshold: motorbike::GROUND_THRESHOLD,
            bounce: motorbike::BOUNCE,
            wheel_base: 1.6,
            track_width: 0.45,
            wheel_radius: 0.32,
            suspension_rest: 0.2,
            suspension_stiffness: 12000.0,
            suspension_damping: 2000.0,
            max_steer_angle: 0.55,
            yaw_inertia: 200.0,
            yaw_damping: 3.0,
            body_pitch_response: 6.0,
            body_roll_response: 6.0,
            suspension_pitch_factor: 0.08,
            suspension_roll_factor: 0.08,
        },
        VehicleType::Car => VehicleDef {
            mass: 1200.0,
            engine_force: 11000.0,
            max_speed: 42.0,
            max_reverse_speed: 10.0,
            reverse_force: 4000.0,
            brake_force: 14000.0,
            drag_coefficient: 0.35,
            rolling_resistance: 60.0,
            engine_brake_driver: 300.0,
            engine_brake_no_driver: 80.0,
            steering_speed_min: 1.6,
            steering_speed_max: 0.7,
            steering_response: 8.0,
            turn_lean_angle: 0.0,
            lean_speed: 4.0,
            mu_desert: 0.9,
            mu_grasslands: 1.0,
            lateral_friction: 12.0,
            landing_drift_preserve: 0.35,
            size: Vec3::new(4.2, 1.6, 1.8),
            gravity: -20.0,
            air_pitch_torque: 1.0,
            air_roll_torque: 1.0,
            air_yaw_torque: 0.5,
            air_angular_damping: 1.2,
            air_linear_damping: 0.05,
            terrain_align_speed: 8.0,
            max_terrain_pitch: 0.7,
            max_terrain_roll: 0.5,
            ground_threshold: 0.15,
            bounce: 0.05,
            wheel_base: 2.7,
            track_width: 1.55,
            wheel_radius: 0.34,
            suspension_rest: 0.35,
            suspension_stiffness: 22000.0,
            suspension_damping: 4500.0,
            max_steer_angle: 0.5,
            yaw_inertia: 1300.0,
            yaw_damping: 2.5,
            body_pitch_response: 4.0,
            body_roll_response: 5.0,
            suspension_pitch_factor: 0.12,
            suspension_roll_factor: 0.15,
        },
    }
}
