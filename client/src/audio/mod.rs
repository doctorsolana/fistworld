//! Small cue bank, bounded presentation effects and independent music playback.

pub(crate) mod carts;
mod catalog;
pub(crate) mod filtered;
pub(crate) mod music;
pub mod paths;
mod perspective;
mod settings;
pub(crate) mod sfx;

pub use settings::AudioSettings;

use crate::states::GameState;
use bevy::prelude::*;

pub struct GameAudioPlugin;

impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<settings::AudioSettingsStore>();
        let settings = app
            .world()
            .resource::<settings::AudioSettingsStore>()
            .load();
        app.insert_resource(settings)
            .init_resource::<music::MusicPlayback>()
            .add_systems(Last, settings::save_audio_settings)
            .add_systems(
                PostUpdate,
                music::update_music
                    .before(bevy::transform::TransformSystems::Propagate)
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(OnExit(GameState::Playing), music::stop_music);
        sfx::install(app);
        filtered::install(app);
        carts::install(app);
    }
}
