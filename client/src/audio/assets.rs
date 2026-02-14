//! assets systems.

use super::*;

/// Load all audio assets on startup
pub fn setup_audio(mut commands: Commands, asset_server: Res<AssetServer>) {
    info!("Audio system: Loading audio assets...");

    let assault_shot = asset_server.load(paths::SFX_ASSAULT_SHOT);
    let revolver_shot = asset_server.load(paths::SFX_REVOLVER_SHOT);
    let shotgun_shot = asset_server.load(paths::SFX_SHOTGUN_SHOT);
    let sniper_shot = asset_server.load(paths::SFX_SNIPER_SHOT);
    let desert_ambient = asset_server.load(paths::AMBIENT_WALKING_DESERT);

    // Vehicle sounds
    let hover_idle = asset_server.load(paths::SFX_HOVER_IDLE_LOOP);
    let bike_cruise = asset_server.load(paths::SFX_BIKE_CRUISE_LOOP);

    info!(
        "Audio handles created: assault_shot={:?}, revolver_shot={:?}, shotgun_shot={:?}, sniper_shot={:?}, desert_ambient={:?}",
        assault_shot, revolver_shot, shotgun_shot, sniper_shot, desert_ambient
    );
    info!(
        "Vehicle audio handles: hover_idle={:?}, bike_cruise={:?}",
        hover_idle, bike_cruise
    );

    commands.insert_resource(GameAudio {
        assault_shot,
        revolver_shot,
        shotgun_shot,
        sniper_shot,
        desert_ambient,
        hover_idle,
        bike_cruise,
    });

    commands.init_resource::<AudioState>();
    commands.init_resource::<VehicleAudioState>();
}

/// Check if audio assets are loaded
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

    let assault_state = asset_server.get_recursive_dependency_load_state(&audio.assault_shot);
    let revolver_state = asset_server.get_recursive_dependency_load_state(&audio.revolver_shot);
    let shotgun_state = asset_server.get_recursive_dependency_load_state(&audio.shotgun_shot);
    let sniper_state = asset_server.get_recursive_dependency_load_state(&audio.sniper_shot);
    let ambient_state = asset_server.get_recursive_dependency_load_state(&audio.desert_ambient);

    match (
        assault_state,
        revolver_state,
        shotgun_state,
        sniper_state,
        ambient_state,
    ) {
        (
            Some(RecursiveDependencyLoadState::Loaded),
            Some(RecursiveDependencyLoadState::Loaded),
            Some(RecursiveDependencyLoadState::Loaded),
            Some(RecursiveDependencyLoadState::Loaded),
            Some(RecursiveDependencyLoadState::Loaded),
        ) => {
            info!("Audio assets loaded successfully!");
            audio_state.assets_ready = true;
        }
        (Some(RecursiveDependencyLoadState::Failed(_)), _, _, _, _) => {
            error!("Failed to load assault_shot.ogg!");
        }
        (_, Some(RecursiveDependencyLoadState::Failed(_)), _, _, _) => {
            error!("Failed to load revolver_shot.ogg!");
        }
        (_, _, Some(RecursiveDependencyLoadState::Failed(_)), _, _) => {
            error!("Failed to load shutgun_shot.ogg!");
        }
        (_, _, _, Some(RecursiveDependencyLoadState::Failed(_)), _) => {
            error!("Failed to load sniper_shot.ogg!");
        }
        (_, _, _, _, Some(RecursiveDependencyLoadState::Failed(_))) => {
            error!("Failed to load walking_desert.ogg!");
        }
        _ => {
            // Still loading
        }
    }
}
