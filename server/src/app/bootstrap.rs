//! Server bootstrap and lifecycle wiring.

use bevy::prelude::*;
use lightyear::prelude::server::*;
use lightyear::prelude::*;
use std::net::{SocketAddr, ToSocketAddrs};

use shared::protocol::{
    get_server_bind_addr, NETCODE_CLIENT_TIMEOUT_SECS, PRIVATE_KEY, PROTOCOL_ID, SERVER_PORT,
};

use crate::collision;
use crate::net;
use crate::world;

/// Marker for the server host entity.
#[derive(Component)]
pub(crate) struct GameServer;

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
            world::regions::build_region_registry,
            world::village_lab_scenario::stage_rendered_lab_once,
            world::village_lab_scenario::stage_rendered_lab_arrivals,
            world::village_lab_scenario::log_rendered_village_diagnostics,
        )
            .chain()
            .run_if(server_is_started),
    );
}
