//! World time systems.

use bevy::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::{MessageReceiver, NetworkTarget, RemoteId, Replicate};

use shared::components::{TimeWarp, WorldTime};
use shared::protocol::SetTimeOfDay;

use crate::world::dev::DevMode;

/// One-shot resource to ensure we only spawn `WorldTime` once.
#[derive(Resource)]
pub struct WorldTimeSpawned;

/// Spawn the server-authoritative day/night clock replicated to all clients.
///
/// This should run after the server has started networking, so clients actually receive it.
pub fn spawn_world_time_once(mut commands: Commands, spawned: Option<Res<WorldTimeSpawned>>) {
    if spawned.is_some() {
        return;
    }
    commands.insert_resource(WorldTimeSpawned);

    commands.spawn((
        WorldTime::new_default(),
        TimeWarp::default(),
        Replicate::to_clients(NetworkTarget::All),
    ));

    info!("Spawned WorldTime (day/night cycle) replicated to all clients");
}

/// Advance the world clock every fixed tick (server-authoritative).
///
/// KNOWN idle-traffic floor (accepted for now): mutating WorldTime every tick
/// change-flags it every tick, so every client receives a ~30/s WorldTime
/// trickle forever (~1 KiB/s each). The proper fix is client-side clock
/// prediction (integrate locally from TimeWarp, correct on sparse syncs) —
/// do that before worrying about idle bandwidth measurements.
pub fn update_world_time(
    mut world_time: Query<&mut WorldTime>,
    simulation_time: super::simulation_time::SimulationTime,
) {
    let real_dt = simulation_time.real_seconds();
    let world_dt = simulation_time.world_seconds();
    for mut wt in world_time.iter_mut() {
        // Ocean clock stays wall-clock even under warp/pause — see WorldTime::advance.
        wt.advance(world_dt, real_dt);
    }
}

/// Handle debug requests to set time of day.
pub fn handle_set_time_of_day(
    dev: Res<DevMode>,
    mut client_links: Query<(&RemoteId, &mut MessageReceiver<SetTimeOfDay>), With<ClientOf>>,
    mut world_time: Query<&mut WorldTime>,
) {
    let Some(mut wt) = world_time.iter_mut().next() else {
        return;
    };

    for (remote_id, mut receiver) in client_links.iter_mut() {
        for msg in receiver.receive() {
            if !dev.0 {
                warn!(
                    "Ignoring SetTimeOfDay from {:?}: dev mode disabled",
                    remote_id.0
                );
                continue;
            }
            let normalized = msg.preset.normalized_time();
            wt.set_normalized_time(normalized);
            info!("Debug: set time of day to {:?}", msg.preset);
        }
    }
}
