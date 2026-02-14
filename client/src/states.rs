//! Game state machine

use bevy::prelude::*;

/// Main game states
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GameState {
    #[default]
    MainMenu,
    Connecting,
    Connected, // Waiting for player name submission
    Playing,
    Paused,
}
