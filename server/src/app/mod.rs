//! Application wiring entrypoint for the headless server.

mod bootstrap;
mod resources;
mod schedule;

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use bevy_rapier3d::plugin::{NoUserData, RapierPhysicsPlugin};
use lightyear::prelude::server::ServerPlugins;

use shared::protocol::{tick_duration, ProtocolPlugin, SERVER_PORT};

pub(crate) fn run() {
    let mut app = App::new();

    app.add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(tick_duration())));
    app.add_plugins(bevy::log::LogPlugin::default());
    app.add_plugins(bevy::state::app::StatesPlugin);
    // Rapier (PR pin) schedules Bevy transform propagation systems directly, but
    // does not initialize this resource when running on MinimalPlugins.
    app.init_resource::<bevy::transform::systems::StaticTransformOptimizations>();

    resources::setup_resources(&mut app);

    app.add_plugins(ServerPlugins {
        tick_duration: tick_duration(),
    });
    app.add_plugins(ProtocolPlugin);
    app.add_plugins(RapierPhysicsPlugin::<NoUserData>::default().in_fixed_schedule());

    bootstrap::configure_bootstrap(&mut app);
    schedule::configure_fixed_schedule(&mut app);

    info!("Starting server on port {}", SERVER_PORT);
    app.run();
}
