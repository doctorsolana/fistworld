//! Vehicle simulation systems.

use bevy::prelude::*;

use shared::protocol::FIXED_TIMESTEP_HZ;
use shared::terrain::WorldTerrain;
use shared::vehicle::{
    step_car_physics, step_car_v2_physics, step_vehicle_physics, CarSuspensionState, Vehicle,
    VehicleDriver, VehicleState, VehicleType,
};

use crate::net::input::ClientInputs;

/// Simulate all vehicles.
pub fn update_vehicles(
    terrain: Res<WorldTerrain>,
    inputs: Res<ClientInputs>,
    mut vehicles: Query<(
        &Vehicle,
        &VehicleDriver,
        &mut VehicleState,
        Option<&mut CarSuspensionState>,
    )>,
) {
    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;
    let bounds = terrain.generator.active_map_bounds();
    const EDGE_MARGIN: f32 = 3.0;

    for (vehicle, driver, mut state, suspension) in vehicles.iter_mut() {
        let vehicle_input = driver
            .driver_id
            .and_then(|driver_id| inputs.latest_by_driver_id.get(&driver_id))
            .and_then(|input| input.vehicle_input.clone())
            .unwrap_or_default();

        match vehicle.vehicle_type {
            VehicleType::CarV2 => {
                if let Some(mut suspension) = suspension {
                    step_car_v2_physics(
                        &vehicle_input,
                        &mut state,
                        &mut suspension,
                        &terrain,
                        dt,
                        driver.driver_id.is_some(),
                        vehicle.vehicle_type,
                    );
                } else {
                    step_vehicle_physics(
                        &vehicle_input,
                        &mut state,
                        &terrain,
                        dt,
                        driver.driver_id.is_some(),
                        vehicle.vehicle_type,
                    );
                }
            }
            VehicleType::Car => {
                if let Some(mut suspension) = suspension {
                    step_car_physics(
                        &vehicle_input,
                        &mut state,
                        &mut suspension,
                        &terrain,
                        dt,
                        driver.driver_id.is_some(),
                        vehicle.vehicle_type,
                    );
                } else {
                    step_vehicle_physics(
                        &vehicle_input,
                        &mut state,
                        &terrain,
                        dt,
                        driver.driver_id.is_some(),
                        vehicle.vehicle_type,
                    );
                }
            }
            _ => {
                step_vehicle_physics(
                    &vehicle_input,
                    &mut state,
                    &terrain,
                    dt,
                    driver.driver_id.is_some(),
                    vehicle.vehicle_type,
                );
            }
        }

        // Hard map boundary: nothing drives off the edge of the world. When
        // an axis clamps, its velocity component is zeroed so vehicles stop
        // at the wall instead of grinding against it.
        let min_x = bounds.min[0] + EDGE_MARGIN;
        let max_x = bounds.max[0] - EDGE_MARGIN;
        let min_z = bounds.min[1] + EDGE_MARGIN;
        let max_z = bounds.max[1] - EDGE_MARGIN;
        if state.position.x < min_x || state.position.x > max_x {
            state.position.x = state.position.x.clamp(min_x, max_x);
            state.velocity.x = 0.0;
        }
        if state.position.z < min_z || state.position.z > max_z {
            state.position.z = state.position.z.clamp(min_z, max_z);
            state.velocity.z = 0.0;
        }
    }
}

/// Ensure car vehicles have suspension state attached.
pub fn ensure_car_suspension_state(
    mut commands: Commands,
    cars: Query<(Entity, &Vehicle), Without<CarSuspensionState>>,
) {
    for (entity, vehicle) in cars.iter() {
        if matches!(vehicle.vehicle_type, VehicleType::Car | VehicleType::CarV2) {
            commands
                .entity(entity)
                .insert(CarSuspensionState::default());
        }
    }
}
