//! Game state machine

use bevy::prelude::*;

/// Main game states.
///
/// There is no `Paused` state: pausing is a UI modal (`InputState::pause_menu_open`)
/// rather than a world state, so the world keeps simulating behind the menu.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GameState {
    #[default]
    MainMenu,
    Connecting,
    Connected, // Waiting for player name submission
    Playing,
}
