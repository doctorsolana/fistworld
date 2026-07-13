//! World vehicle spawning.
//!
//! Fresh players previously had no way to obtain a vehicle at all (vehicles
//! only restored from profiles that already had one). Spawn a small garage
//! near the map spawn point: the v1 car and the v2 car side by side for A/B
//! comparison, plus a motorbike.

use bevy::prelude::*;
use lightyear::prelude::*;

use shared::player::SPAWN_POSITION;
use shared::terrain::WorldTerrain;
use shared::vehicle::{
    car_v2_static_ride_height, Vehicle, VehicleDriver, VehicleState, VehicleType,
};

const WORLD_VEHICLE_REPLICATION_PRIORITY: f32 = 5.0;

/// (type, offset from spawn in meters: +X east, +Z south)
const WORLD_VEHICLES: [(VehicleType, [f32; 2]); 3] = [
    (VehicleType::Car, [8.0, -6.0]),
    (VehicleType::CarV2, [14.0, -6.0]),
    (VehicleType::Motorbike, [20.0, -6.0]),
];

pub fn spawn_world_vehicles(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    mut spawned: Local<bool>,
) {
    if *spawned {
        return;
    }
    *spawned = true;

    let spawn = terrain
        .generator
        .loaded_map()
        .definition
        .player_spawn
        .unwrap_or(SPAWN_POSITION);

    for (vehicle_type, offset) in WORLD_VEHICLES {
        let x = spawn[0] + offset[0];
        let z = spawn[2] + offset[1];
        let ground_y = terrain.get_height(x, z);
        let ride = car_v2_static_ride_height(vehicle_type).max(0.4);

        commands.spawn((
            Vehicle { vehicle_type },
            VehicleState {
                position: Vec3::new(x, ground_y + ride + 0.1, z),
                grounded: true,
                ..Default::default()
            },
            VehicleDriver { driver_id: None },
            ReplicationGroup::new_from_entity()
                .set_priority(WORLD_VEHICLE_REPLICATION_PRIORITY),
            Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
        ));
        info!(
            "Spawned world vehicle {:?} at ({:.1}, {:.1})",
            vehicle_type, x, z
        );
    }
}
