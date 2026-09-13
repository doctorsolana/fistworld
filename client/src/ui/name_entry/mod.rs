//! Account-name entry and the asynchronous authoritative world join.

pub mod actions;
pub(crate) mod capture;
pub mod input;
pub mod layout;
pub mod network;

use crate::states::GameState;
use bevy::prelude::*;

/// Busy states are separate from validation errors; the UI never represents
/// world preparation as a failed submission or a made-up percentage.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum NameEntryPhase {
    #[default]
    Editing,
    Submitting,
    Preparing,
}

impl NameEntryPhase {
    pub fn is_busy(self) -> bool {
        self != Self::Editing
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Editing => "Join game",
            Self::Submitting => "Joining game…",
            Self::Preparing => "Preparing world…",
        }
    }
}

pub struct NameEntryPlugin;

impl Plugin for NameEntryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerNameInput>()
            .init_resource::<NameEntryPhase>()
            .init_resource::<NameSubmissionFeedback>()
            .init_resource::<input::NameInputEditing>()
            .init_resource::<network::PendingWorldJoin>()
            .add_systems(
                OnEnter(GameState::Connected),
                (actions::reset_name_entry, layout::spawn_name_entry_ui).chain(),
            )
            .add_systems(
                OnExit(GameState::Connected),
                (network::cancel_world_join, layout::despawn_name_entry_ui),
            )
            .add_systems(
                PostUpdate,
                input::fit_name_input
                    .after(bevy::ui::widget::text_system)
                    .run_if(in_state(GameState::Connected)),
            )
            .add_systems(
                Update,
                (
                    input::handle_text_input,
                    actions::handle_submit_button,
                    actions::handle_enter_key_submit,
                    network::handle_name_submission_result,
                    actions::handle_back,
                    layout::sync_name_entry_ui,
                )
                    .chain()
                    .run_if(in_state(GameState::Connected)),
            );
    }
}

/// The submitted name remains the local account identity throughout gameplay.
#[derive(Resource, Default)]
pub struct PlayerNameInput {
    pub name: String,
    pub submitted: bool,
}

#[derive(Resource, Default)]
pub struct NameSubmissionFeedback {
    pub error_message: Option<String>,
}

#[derive(Component)]
struct NameEntryRoot;
#[derive(Component)]
struct NameInputDisplay;
#[derive(Component)]
struct ErrorMessageText;
#[derive(Component)]
struct SubmitButton;
#[derive(Component)]
pub(crate) struct NameInputField;
#[derive(Component)]
pub(crate) struct BackButton;

/// Marks a reliable request awaiting its authoritative reply.
#[derive(Component)]
pub struct PlayerNameSubmitted;
