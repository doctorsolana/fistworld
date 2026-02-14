//! Vehicle interaction systems.

use bevy::prelude::*;
use shared::components::{Player, PlayerPosition};
use shared::vehicle::{can_interact_with_vehicle, InVehicle, VehicleDriver, VehicleState};

use crate::net::input::ClientInputs;
use crate::net::peer::peer_id_to_u64;

/// Handle player vehicle interactions (enter/exit).
pub fn handle_vehicle_interaction_requests(
    mut commands: Commands,
    inputs: Res<ClientInputs>,
    mut players: Query<(Entity, &Player, &PlayerPosition, Option<&InVehicle>)>,
    mut vehicles: Query<(Entity, &mut VehicleDriver, &VehicleState)>,
) {
    for (player_entity, player, player_pos, in_vehicle) in players.iter_mut() {
        let Some(input) = inputs.latest.get(&player.client_id) else {
            continue;
        };

        if !input.interact {
            continue;
        }

        if let Some(in_veh) = in_vehicle {
            for (veh_entity, mut driver, _state) in vehicles.iter_mut() {
                if veh_entity == in_veh.vehicle_entity {
                    driver.driver_id = None;
                    commands.entity(player_entity).remove::<InVehicle>();
                    info!("Player {:?} exited vehicle", player.client_id);
                    break;
                }
            }
        } else {
            for (veh_entity, mut driver, state) in vehicles.iter_mut() {
                if driver.driver_id.is_some() {
                    continue;
                }

                if can_interact_with_vehicle(player_pos.0, state) {
                    driver.driver_id = Some(peer_id_to_u64(player.client_id));
                    commands.entity(player_entity).insert(InVehicle {
                        vehicle_entity: veh_entity,
                    });
                    info!("Player {:?} entered vehicle", player.client_id);
                    break;
                }
            }
        }
    }
}
