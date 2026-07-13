use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Marker component for a replicated vehicle entity.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Vehicle {
    pub vehicle_type: VehicleType,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
pub enum VehicleType {
    #[default]
    Motorbike,
    Car,
    /// Same steam-car visual as `Car`, driven by the v2 physics model
    /// (slip tires, load transfer, dynamic body attitude) for A/B comparison.
    CarV2,
}

#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct VehicleState {
    pub position: Vec3,
    pub velocity: Vec3,
    pub heading: f32,
    pub pitch: f32,
    pub roll: f32,
    pub angular_velocity_yaw: f32,
    pub angular_velocity_pitch: f32,
    pub angular_velocity_roll: f32,
    pub grounded: bool,
}

/// Per-vehicle suspension state for cars (server-authoritative only).
#[derive(Component, Clone, Debug)]
pub struct CarSuspensionState {
    pub compression: [f32; 4],
    pub last_compression: [f32; 4],
    pub steer_angle: f32,
}

impl Default for CarSuspensionState {
    fn default() -> Self {
        Self {
            compression: [0.0; 4],
            last_compression: [0.0; 4],
            steer_angle: 0.0,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct VehicleInput {
    pub throttle: f32,
    pub brake: f32,
    pub steer: f32,
    /// Hold Shift to enable air tricks (pitch/roll control while airborne)
    pub air_control: bool,
}

#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct VehicleDriver {
    pub driver_id: Option<u64>,
}

#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct InVehicle {
    pub vehicle_entity: Entity,
}
