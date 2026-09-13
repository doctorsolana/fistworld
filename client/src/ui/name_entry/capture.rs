//! Offline presentation inputs for the real startup name form and loading view.
//! These fixtures never submit a name, connect, or claim world-generation progress.

use super::{NameEntryPhase, NameSubmissionFeedback, PlayerNameInput};
use bevy::prelude::*;

/// Installed only by the offline capture binary.
pub(crate) fn install(app: &mut App) {
    let phase = match std::env::var("FISTFORCE_CAPTURE_FRONTEND").as_deref() {
        Ok("name" | "name-error") => NameEntryPhase::Editing,
        Ok("submitting") => NameEntryPhase::Submitting,
        Ok("preparing") => NameEntryPhase::Preparing,
        _ => return,
    };
    let error = (std::env::var("FISTFORCE_CAPTURE_FRONTEND").as_deref() == Ok("name-error"))
        .then(|| "That name is already online. Choose another name.".to_string());
    app.add_systems(
        OnEnter(crate::states::GameState::Connected),
        (move |mut name: ResMut<PlayerNameInput>,
               mut state: ResMut<NameEntryPhase>,
               mut feedback: ResMut<NameSubmissionFeedback>| {
            name.name = "aldric".into();
            name.submitted = phase != NameEntryPhase::Editing;
            *state = phase;
            feedback.error_message.clone_from(&error);
        })
        .after(super::layout::spawn_name_entry_ui),
    );
}
