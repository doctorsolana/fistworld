//! network systems.

use super::*;

pub(super) fn handle_name_submission_result(
    mut next_state: ResMut<NextState<GameState>>,
    mut feedback: ResMut<NameSubmissionFeedback>,
    mut client_query: Query<
        (
            Entity,
            &mut MessageReceiver<NameSubmissionResult>,
            &mut MessageSender<shared::protocol::CreateHero>,
        ),
        (With<crate::GameClient>, With<PlayerNameSubmitted>),
    >,
    mut error_text_query: Query<&mut Text, With<ErrorMessageText>>,
    mut commands: Commands,
    mut creator_open: ResMut<crate::ui::hero_creator::HeroCreatorOpen>,
    mut creator_purpose: ResMut<crate::ui::hero_creator::HeroCreatorPurpose>,
    mut cinematic: ResMut<crate::boat::OpeningCinematic>,
    selected: Res<crate::hero::control::SelectedOutfit>,
    mut pending_view: ResMut<crate::camera_rts::PendingCommanderView>,
) {
    let Ok((client_entity, mut receiver, mut create_sender)) = client_query.single_mut() else {
        return;
    };

    for result in receiver.receive() {
        match result {
            NameSubmissionResult::Accepted {
                profile_loaded,
                needs_hero_creation,
                commander_view,
            } => {
                pending_view.0 = commander_view;
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
                // The automated rendering/smoke harness deliberately uses the
                // old God spawn hook. Do not put a mandatory modal in front of
                // it; ordinary players always receive the voyage creator.
                let automated_god_spawn =
                    std::env::var("FISTWORLD_AUTOSPAWN_HERO").is_ok_and(|value| value == "1");
                let automated_voyage =
                    std::env::var("FISTWORLD_AUTOCREATE_VOYAGE").is_ok_and(|value| value == "1");
                if needs_hero_creation && automated_voyage {
                    cinematic.arm();
                    create_sender.send::<ReliableChannel>(shared::protocol::CreateHero {
                        outfit: selected.0,
                    });
                    info!("FISTWORLD_AUTOCREATE_VOYAGE: sent normal CreateHero request");
                } else if needs_hero_creation && !automated_god_spawn {
                    *creator_purpose = crate::ui::hero_creator::HeroCreatorPurpose::NewPlayerVoyage;
                    creator_open.0 = true;
                    cinematic.arm();
                }
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
