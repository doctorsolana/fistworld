//! Application wiring entrypoint for the headless server.

mod bootstrap;
mod resources;
mod schedule;

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use bevy_rapier3d::plugin::{NoUserData, RapierPhysicsPlugin};
use bevy_rapier3d::prelude::{DefaultRapierContext, RapierConfiguration};
use lightyear::prelude::server::ServerPlugins;

use shared::{
    physics::GRAVITY,
    protocol::{tick_duration, ProtocolPlugin, SERVER_PORT},
};

fn configure_rapier_gravity(
    mut configuration: Query<&mut RapierConfiguration, With<DefaultRapierContext>>,
) {
    let mut configuration = configuration
        .single_mut()
        .expect("default Rapier context should exist before Startup");
    configuration.gravity = Vec3::Y * GRAVITY;
}

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
    app.add_systems(
        Startup,
        (
            configure_rapier_gravity,
            crate::city::log_city_layout_summary,
        )
            .chain(),
    );

    bootstrap::configure_bootstrap(&mut app);
    schedule::configure_fixed_schedule(&mut app);

    info!("Starting server on port {}", SERVER_PORT);
    app.run();
}
