//! Audio system for game sounds.

pub mod assets;
pub mod limits;
pub mod paths;
pub mod remote_players;
pub mod state;

pub use assets::{ensure_audio_assets_loaded, setup_audio};
pub use limits::apply_audio_limits;
pub use remote_players::{
    ensure_remote_footstep_emitters, update_remote_footstep_emitters,
};
pub use state::*;

use bevy::audio::{SpatialAudioSink, Volume};
use bevy::prelude::*;
use bevy::time::common_conditions::on_timer;
use shared::components::{LocalPlayer, Player};
use std::time::Duration;

use crate::states::GameState;

/// Audio plugin for easy integration.
pub struct GameAudioPlugin;

impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AudioManager>();

        app.add_systems(Startup, setup_audio);
        app.add_systems(
            Update,
            ensure_audio_assets_loaded.run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            ensure_remote_footstep_emitters
                .run_if(in_state(GameState::Playing))
                .run_if(on_timer(Duration::from_millis(150))),
        );
        app.add_systems(
            Update,
            update_remote_footstep_emitters.run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            apply_audio_limits
                .run_if(in_state(GameState::Playing))
                .run_if(on_timer(Duration::from_millis(100)))
                .after(ensure_remote_footstep_emitters),
        );
    }
}
