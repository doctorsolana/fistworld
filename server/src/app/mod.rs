//! Application wiring entrypoint for the headless server.

mod bootstrap;
mod resources;
mod schedule;

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use lightyear::prelude::server::ServerPlugins;
use lightyear::prelude::ReplicationMetadata;

use shared::protocol::{tick_duration, ProtocolPlugin, SERVER_PORT};

pub(crate) fn run() {
    let mut app = App::new();

    app.add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(tick_duration())));
    app.add_plugins(bevy::log::LogPlugin::default());
    app.add_plugins(bevy::state::app::StatesPlugin);

    crate::world::bootstrap::prepare_session_terrain(&mut app);
    resources::setup_resources(&mut app);

    app.add_plugins(ServerPlugins {
        tick_duration: tick_duration(),
    });
    // lightyear 0.28: the replication send interval is a global resource that no plugin
    // initializes; `update_replication_tick` panics at runtime without it.
    app.insert_resource(ReplicationMetadata::new(
        crate::net::connection::configured_replication_send_interval(),
    ));
    app.add_plugins(ProtocolPlugin);
    app.add_systems(Startup, crate::city::log_city_layout_summary);

    bootstrap::configure_bootstrap(&mut app);
    schedule::configure_fixed_schedule(&mut app);

    info!("Starting server on port {}", SERVER_PORT);
    app.run();
}
