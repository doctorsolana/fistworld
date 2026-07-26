//! assets systems.

use super::*;

/// Load all audio assets on startup
pub fn setup_audio(mut commands: Commands, asset_server: Res<AssetServer>) {
    info!("Audio system: Loading audio assets...");

    let desert_ambient = asset_server.load(paths::AMBIENT_WALKING_DESERT);

    // Vehicle sounds
    let hover_idle = asset_server.load(paths::SFX_HOVER_IDLE_LOOP);
    let bike_cruise = asset_server.load(paths::SFX_BIKE_CRUISE_LOOP);

    info!("Audio handles created: desert_ambient={:?}", desert_ambient);
    info!(
        "Vehicle audio handles: hover_idle={:?}, bike_cruise={:?}",
        hover_idle, bike_cruise
    );

    commands.insert_resource(GameAudio {
        desert_ambient,
        hover_idle,
        bike_cruise,
    });

    commands.init_resource::<AudioState>();
    commands.init_resource::<VehicleAudioState>();
}

/// Check if audio assets are loaded.
///
/// NOTE: `assets_ready` gates *all* client audio (ambient, footsteps, vehicles).
/// Anything required here must actually be loaded, or the entire audio system goes
/// permanently silent with no compile error and no log line.
pub fn ensure_audio_assets_loaded(
    audio: Option<Res<GameAudio>>,
    mut audio_state: ResMut<AudioState>,
    asset_server: Res<AssetServer>,
) {
    if audio_state.assets_ready {
        return;
    }

    let Some(audio) = audio else { return };

    use bevy::asset::RecursiveDependencyLoadState;

    match asset_server.get_recursive_dependency_load_state(&audio.desert_ambient) {
        Some(RecursiveDependencyLoadState::Loaded) => {
            info!("Audio assets loaded successfully!");
            audio_state.assets_ready = true;
        }
        Some(RecursiveDependencyLoadState::Failed(_)) => {
            error!("Failed to load walking_desert.ogg!");
        }
        _ => {
            // Still loading
        }
    }
}
