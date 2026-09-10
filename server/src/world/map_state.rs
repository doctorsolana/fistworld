//! Replicated world map metadata resources.

use bevy::prelude::*;
use lightyear::prelude::{NetworkTarget, Replicate};

use shared::components::{ActiveMapState, CloudSeed};
use shared::terrain::WorldTerrain;

/// One-shot resource to ensure we only spawn `CloudSeed` once.
#[derive(Resource)]
pub struct CloudSeedSpawned;

/// One-shot resource to ensure we only spawn `ActiveMapState` once.
#[derive(Resource)]
pub struct ActiveMapStateSpawned;

/// Spawn the server-authoritative cloud seed replicated to all clients.
pub fn spawn_cloud_seed_once(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    spawned: Option<Res<CloudSeedSpawned>>,
) {
    if spawned.is_some() {
        return;
    }
    commands.insert_resource(CloudSeedSpawned);

    let seed = terrain
        .generator
        .loaded_map()
        .definition
        .generated
        .as_ref()
        .map_or(shared::terrain::WORLD_SEED as u64, |recipe| recipe.seed)
        ^ 0xC10D_5EED_F00D_BA5Eu64;
    commands.spawn((
        CloudSeed { seed },
        Replicate::to_clients(NetworkTarget::All),
    ));

    info!("Spawned CloudSeed (sky/cloud seed) replicated to all clients");
}

/// Spawn active map metadata replicated to all clients.
pub fn spawn_active_map_state_once(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    spawned: Option<Res<ActiveMapStateSpawned>>,
) {
    if spawned.is_some() {
        return;
    }
    commands.insert_resource(ActiveMapStateSpawned);

    let map_bounds = terrain.generator.active_map_bounds();
    let map_state = ActiveMapState::from_terrain(&terrain);

    commands.spawn((map_state, Replicate::to_clients(NetworkTarget::All)));

    info!(
        "Spawned ActiveMapState map_id={} bounds=({:.1},{:.1})..({:.1},{:.1}) hash={:016x}",
        terrain.generator.active_map_id(),
        map_bounds.min[0],
        map_bounds.min[1],
        map_bounds.max[0],
        map_bounds.max[1],
        terrain.generator.active_map_content_hash()
    );
}
