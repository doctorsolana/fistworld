//! Player name entry UI
//!
//! Simple name input screen shown when connected to server

pub mod actions;
pub mod input;
pub mod layout;
pub mod network;

use actions::{handle_enter_key_submit, handle_submit_button};
use input::handle_text_input;
use layout::{despawn_name_entry_ui, spawn_name_entry_ui};
use network::handle_name_submission_result;

use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use lightyear::prelude::*;
use shared::protocol::{
    NameRejectionReason, NameSubmissionResult, ReliableChannel, SubmitPlayerName,
};

use crate::states::GameState;

// UI colors
const TEXT_COLOR: Color = Color::srgb(0.9, 0.9, 0.9);
const INPUT_BG: Color = Color::srgb(0.15, 0.15, 0.15);
const BUTTON_NORMAL: Color = Color::srgb(0.25, 0.55, 0.35);
const BUTTON_HOVERED: Color = Color::srgb(0.3, 0.65, 0.45);
const BUTTON_PRESSED: Color = Color::srgb(0.35, 0.75, 0.55);
const ERROR_COLOR: Color = Color::srgb(0.9, 0.3, 0.3);

/// Plugin for name entry UI
pub struct NameEntryPlugin;

impl Plugin for NameEntryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerNameInput>();
        app.init_resource::<NameSubmissionFeedback>();

        app.add_systems(OnEnter(GameState::Connected), spawn_name_entry_ui);
        app.add_systems(OnExit(GameState::Connected), despawn_name_entry_ui);

        app.add_systems(
            Update,
            (
                handle_text_input,
                handle_submit_button,
                handle_enter_key_submit,
                handle_name_submission_result,
            )
                .run_if(in_state(GameState::Connected)),
        );
    }
}

/// Resource holding the player's name input
#[derive(Resource, Default)]
pub struct PlayerNameInput {
    pub name: String,
    pub submitted: bool,
}

/// Resource for feedback messages (errors, etc.)
#[derive(Resource, Default)]
pub struct NameSubmissionFeedback {
    pub error_message: Option<String>,
}

/// Root marker for the name entry UI
#[derive(Component)]
struct NameEntryRoot;

/// Marker for the name input text display
#[derive(Component)]
struct NameInputDisplay;

/// Marker for the error message text
#[derive(Component)]
struct ErrorMessageText;

/// Marker for the submit button
#[derive(Component)]
struct SubmitButton;

/// Marker component to track that we've submitted a name
#[derive(Component)]
pub struct PlayerNameSubmitted;
