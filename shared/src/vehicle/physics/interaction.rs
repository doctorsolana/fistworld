use bevy::prelude::*;

use crate::vehicle::components::VehicleState;

pub fn can_interact_with_vehicle(player_pos: Vec3, vehicle_state: &VehicleState) -> bool {
    let dist = (player_pos - vehicle_state.position).length();
    dist < 3.0
}
