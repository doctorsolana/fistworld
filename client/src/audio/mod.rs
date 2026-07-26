//! Audio system for game sounds.

pub mod ambient;
pub mod assets;
pub mod limits;
pub mod paths;
pub mod remote_players;
pub mod state;
pub mod vehicles;

pub use ambient::{
    cleanup_ambient_sounds, cleanup_remote_loop_sounds, ensure_ambient_entity,
    update_desert_walking_ambient,
};
pub use assets::{ensure_audio_assets_loaded, setup_audio};
pub use limits::apply_audio_limits;
pub use remote_players::{
    ensure_remote_footstep_emitters, update_remote_footstep_emitters,
};
pub use state::*;
pub use vehicles::{
    cleanup_vehicle_sounds, ensure_remote_vehicle_audio_emitters,
    update_remote_vehicle_audio_emitters, update_vehicle_audio, update_vehicle_audio_state,
};

use bevy::audio::{SpatialAudioSink, Volume};
use bevy::prelude::*;
use bevy::time::common_conditions::on_timer;
use lightyear::prelude::*;
use shared::components::{LocalPlayer, Npc, Player, PlayerPosition};
use shared::terrain::{Biome, WorldTerrain};
use shared::vehicle::{Vehicle, VehicleDriver, VehicleState};
use std::collections::HashSet;
use std::time::Duration;

use crate::camera::peer_id_to_u64;
use crate::input::InputState;
use crate::states::GameState;

/// Audio plugin for easy integration.
pub struct GameAudioPlugin;

impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AudioManager>();

        app.add_systems(Startup, setup_audio);
        app.add_systems(
            OnExit(GameState::Playing),
            (
                cleanup_ambient_sounds,
                cleanup_vehicle_sounds,
                cleanup_remote_loop_sounds,
            ),
        );

        app.add_systems(
            Update,
            ensure_audio_assets_loaded.run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            ensure_ambient_entity.run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            update_desert_walking_ambient.run_if(in_state(GameState::Playing)),
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
            ensure_remote_vehicle_audio_emitters
                .run_if(in_state(GameState::Playing))
                .run_if(on_timer(Duration::from_millis(180))),
        );
        app.add_systems(
            Update,
            update_remote_vehicle_audio_emitters.run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            update_vehicle_audio_state.run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            update_vehicle_audio.run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            apply_audio_limits
                .run_if(in_state(GameState::Playing))
                .run_if(on_timer(Duration::from_millis(100)))
                .after(ensure_remote_footstep_emitters)
                .after(ensure_remote_vehicle_audio_emitters),
        );
    }
}
