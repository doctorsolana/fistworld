//! Application wiring entrypoint for the headless server.

mod bootstrap;
mod resources;
mod schedule;

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use lightyear::prelude::server::ServerPlugins;

use shared::protocol::{tick_duration, ProtocolPlugin, SERVER_PORT};

pub(crate) fn run() {
    let mut app = App::new();

    app.add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(tick_duration())));
    app.add_plugins(bevy::log::LogPlugin::default());
    app.add_plugins(bevy::state::app::StatesPlugin);

    resources::setup_resources(&mut app);

    app.add_plugins(ServerPlugins {
        tick_duration: tick_duration(),
    });
    app.add_plugins(ProtocolPlugin);

    bootstrap::configure_bootstrap(&mut app);
    schedule::configure_fixed_schedule(&mut app);

    info!("Starting server on port {}", SERVER_PORT);
    app.run();
}
