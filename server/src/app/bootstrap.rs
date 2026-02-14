//! Server bootstrap and lifecycle wiring.

use bevy::prelude::*;
use lightyear::prelude::server::*;
use lightyear::prelude::*;
use std::net::{SocketAddr, ToSocketAddrs};

use shared::protocol::{
    get_server_bind_addr, NETCODE_CLIENT_TIMEOUT_SECS, PRIVATE_KEY, PROTOCOL_ID, SERVER_PORT,
};
use shared::terrain::WorldTerrain;
use shared::vehicle::{Vehicle, VehicleDriver, VehicleState, VehicleType};

use crate::ai;
use crate::collision;
use crate::inventory;
use crate::net;
use crate::world;

/// Marker for the server host entity.
#[derive(Component)]
pub(crate) struct GameServer;

/// Tracks whether startup test vehicles were spawned.
#[derive(Resource)]
pub(crate) struct VehiclesSpawned;

pub(crate) fn spawn_server(mut commands: Commands) {
    let bind_addr = get_server_bind_addr();
    let server_addr: SocketAddr = (bind_addr, SERVER_PORT)
        .to_socket_addrs()
        .ok()
        .and_then(|mut it| it.next())
        .expect("Invalid server bind address");

    info!(
        "Spawning server entity, binding to {:?} (fly.io: {})",
        server_addr,
        std::env::var("FLY_APP_NAME").is_ok()
    );

    commands.spawn((
        GameServer,
        Server::default(),
        ServerUdpIo::default(),
        LocalAddr(server_addr),
        NetcodeServer::new(NetcodeConfig {
            client_timeout_secs: NETCODE_CLIENT_TIMEOUT_SECS,
            protocol_id: PROTOCOL_ID,
            private_key: PRIVATE_KEY,
            ..default()
        }),
    ));
}

pub(crate) fn start_server(
    mut commands: Commands,
    server_query: Query<Entity, (With<GameServer>, Without<Started>, Without<Starting>)>,
) {
    for server_entity in server_query.iter() {
        info!("Starting server...");
        commands.trigger(Start {
            entity: server_entity,
        });
    }
}

pub(crate) fn spawn_vehicles_once(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    spawned: Option<Res<VehiclesSpawned>>,
    server_query: Query<Entity, (With<GameServer>, With<Started>)>,
) {
    if spawned.is_some() || server_query.is_empty() {
        return;
    }

    commands.insert_resource(VehiclesSpawned);

    let bike_positions = [(12.0, 10.0), (15.0, 10.0)];

    for (bike_x, bike_z) in bike_positions {
        let ground_y = terrain.get_height(bike_x, bike_z);
        let spawn_height = ground_y + 5.0;

        commands.spawn((
            Vehicle {
                vehicle_type: VehicleType::Motorbike,
            },
            VehicleState {
                position: Vec3::new(bike_x, spawn_height, bike_z),
                velocity: Vec3::ZERO,
                heading: 0.0,
                pitch: 0.0,
                roll: 0.0,
                angular_velocity_yaw: 0.0,
                angular_velocity_pitch: 0.0,
                angular_velocity_roll: 0.0,
                grounded: false,
            },
            VehicleDriver { driver_id: None },
            Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
        ));

        info!(
            "Spawned motorbike at ({}, {}) - dropping from height {}!",
            bike_x, bike_z, spawn_height
        );
    }
}

pub(crate) fn server_is_started(
    server_query: Query<(), (With<GameServer>, With<Started>)>,
) -> bool {
    !server_query.is_empty()
}

pub(crate) fn configure_bootstrap(app: &mut App) {
    app.add_systems(
        Startup,
        (
            world::bootstrap::setup_world,
            collision::library::setup_baked_colliders,
            spawn_server,
        ),
    );

    app.add_systems(Update, start_server);
    app.add_observer(net::connection::handle_disconnections);

    app.add_systems(
        Update,
        (
            world::time::spawn_world_time_once,
            world::map_state::spawn_cloud_seed_once,
            world::map_state::spawn_active_map_state_once,
        )
            .run_if(server_is_started),
    );

    app.add_systems(Update, spawn_vehicles_once);
    app.add_systems(
        Update,
        inventory::test_spawns::spawn_test_items.run_if(server_is_started),
    );
    app.add_systems(Update, ai::spawn::spawn_npcs_once.run_if(server_is_started));
}
