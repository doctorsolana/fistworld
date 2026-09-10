//! World bootstrap systems.

use bevy::prelude::*;

/// The server chooses a seed once per new world, never once per joining player.
/// Explicit authored/lab maps retain their own recipe and empty-world behavior.
pub(crate) fn prepare_session_terrain(app: &mut App) {
    let map_id = std::env::var("CITYSIM_MAP_ID").unwrap_or_default();
    if !map_id.trim().is_empty() && map_id != shared::map::SESSION_MAP_ID {
        return;
    }
    if std::env::var_os("FISTWORLD_VILLAGE_LAB_RUNTIME").is_some()
        || std::env::var_os("FISTWORLD_REALWORLD_LAB_RUNTIME").is_some()
    {
        return;
    }
    let seed = match std::env::var("FISTWORLD_WORLD_SEED") {
        Ok(value) => value
            .parse::<u64>()
            .expect("FISTWORLD_WORLD_SEED must be an unsigned integer"),
        Err(std::env::VarError::NotPresent) => rand::random(),
        Err(error) => panic!("Invalid FISTWORLD_WORLD_SEED: {error}"),
    };
    println!(
        "Creating new world: seed={seed}; reproduce with FISTWORLD_WORLD_SEED={seed} ./run.sh"
    );
    let map = shared::map::load_session_map(&shared::map::new_world_recipe(seed))
        .expect("New world terrain generation failed");
    app.insert_resource(shared::terrain::WorldTerrain::from_loaded_map(map));
}
