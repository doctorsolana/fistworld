//! network systems.

use super::*;

pub(super) fn handle_name_submission_result(
    mut next_state: ResMut<NextState<GameState>>,
    mut feedback: ResMut<NameSubmissionFeedback>,
    mut client_query: Query<
        (Entity, &mut MessageReceiver<NameSubmissionResult>),
        (With<crate::GameClient>, With<PlayerNameSubmitted>),
    >,
    mut error_text_query: Query<&mut Text, With<ErrorMessageText>>,
    mut commands: Commands,
) {
    let Ok((client_entity, mut receiver)) = client_query.single_mut() else {
        return;
    };

    for result in receiver.receive() {
        match result {
            NameSubmissionResult::Accepted { profile_loaded } => {
                if profile_loaded {
                    info!("Name accepted! Loaded existing profile");
                } else {
                    info!("Name accepted! Created new profile");
                }

                // Clear the submitted marker
                commands
                    .entity(client_entity)
                    .remove::<PlayerNameSubmitted>();

                // Transition to Playing state
                next_state.set(GameState::Playing);
            }
            NameSubmissionResult::Rejected { reason } => {
                warn!("Name rejected: {:?}", reason);

                // Clear the submitted marker so user can try again
                commands
                    .entity(client_entity)
                    .remove::<PlayerNameSubmitted>();

                // Show error message
                let error_msg = match reason {
                    NameRejectionReason::InvalidCharacters => {
                        "Name contains invalid characters".to_string()
                    }
                    NameRejectionReason::TooShort => {
                        "Name is too short (min 3 characters)".to_string()
                    }
                    NameRejectionReason::TooLong => {
                        "Name is too long (max 16 characters)".to_string()
                    }
                    NameRejectionReason::Reserved => "This name is reserved".to_string(),
                    NameRejectionReason::AlreadyOnline => "This name is already in use".to_string(),
                };

                feedback.error_message = Some(error_msg.clone());

                // Update error text
                for mut text in error_text_query.iter_mut() {
                    text.0 = error_msg.clone();
                }
            }
        }
    }
}
