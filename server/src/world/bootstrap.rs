//! World bootstrap systems.

use bevy::prelude::*;

/// Set up the game world (server-side, no rendering).
pub fn setup_world(_commands: Commands) {
    info!("Server world initialized");

    // The server doesn't need to spawn visual elements,
    // but we could spawn physics colliders here if using physics.
    //
    // For now, we just log that the world is ready.
    // In the future, this would include:
    // - Terrain collision data
    // - NPC spawning
    // - World object placement
}
