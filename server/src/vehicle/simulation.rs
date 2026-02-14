//! Vehicle simulation systems.

use bevy::prelude::*;
use std::collections::HashMap;

use shared::components::Player;
use shared::protocol::FIXED_TIMESTEP_HZ;
use shared::terrain::WorldTerrain;
use shared::vehicle::{
    step_car_physics, step_vehicle_physics, CarSuspensionState, Vehicle, VehicleDriver,
    VehicleState, VehicleType,
};

use crate::net::input::ClientInputs;
use crate::net::peer::peer_id_to_u64;

/// Simulate all vehicles.
pub fn update_vehicles(
    terrain: Res<WorldTerrain>,
    inputs: Res<ClientInputs>,
    players: Query<&Player>,
    mut vehicles: Query<(
        &Vehicle,
        &VehicleDriver,
        &mut VehicleState,
        Option<&mut CarSuspensionState>,
    )>,
) {
    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;
    let mut peer_by_driver_id = HashMap::new();
    for player in players.iter() {
        peer_by_driver_id.insert(peer_id_to_u64(player.client_id), player.client_id);
    }

    for (vehicle, driver, mut state, suspension) in vehicles.iter_mut() {
        let vehicle_input = driver
            .driver_id
            .and_then(|driver_id| peer_by_driver_id.get(&driver_id).copied())
            .and_then(|peer_id| inputs.latest.get(&peer_id))
            .and_then(|input| input.vehicle_input.clone())
            .unwrap_or_default();

        match vehicle.vehicle_type {
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
    }
}

/// Ensure car vehicles have suspension state attached.
pub fn ensure_car_suspension_state(
    mut commands: Commands,
    cars: Query<(Entity, &Vehicle), Without<CarSuspensionState>>,
) {
    for (entity, vehicle) in cars.iter() {
        if vehicle.vehicle_type == VehicleType::Car {
            commands
                .entity(entity)
                .insert(CarSuspensionState::default());
        }
    }
}
