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
    let config = super::start_config::WorldStartConfig::from_environment()
        .unwrap_or_else(|error| panic!("Invalid world start configuration: {error}"));
    let seed = match std::env::var("FISTWORLD_WORLD_SEED") {
        Ok(value) => value
            .parse::<u64>()
            .expect("FISTWORLD_WORLD_SEED must be an unsigned integer"),
        Err(std::env::VarError::NotPresent) => config.seed.unwrap_or_else(rand::random),
        Err(error) => panic!("Invalid FISTWORLD_WORLD_SEED: {error}"),
    };
    println!(
        "Creating new world: seed={seed}; repeat with FISTWORLD_WORLD_SEED={seed} and this same world configuration"
    );
    println!("World start configuration: {config:?}");
    let map = shared::map::load_session_map(&config.recipe(seed))
        .expect("New world terrain generation failed");
    app.insert_resource(config);
    app.insert_resource(shared::terrain::WorldTerrain::from_loaded_map(map));
    super::new_world::install_acceptance_observer(app);
}
