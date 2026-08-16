//! actions systems.

use super::*;

pub(super) fn handle_submit_button(
    interaction_query: Query<&Interaction, (Changed<Interaction>, With<SubmitButton>)>,
    name_input: Res<PlayerNameInput>,
    mut feedback: ResMut<NameSubmissionFeedback>,
    client_query: Query<(Entity, &MessageSender<SubmitPlayerName>), With<crate::GameClient>>,
    mut error_text_query: Query<&mut Text, With<ErrorMessageText>>,
    mut commands: Commands,
) {
    for interaction in interaction_query.iter() {
        if *interaction == Interaction::Pressed {
            submit_name(
                &name_input,
                &mut feedback,
                client_query,
                &mut error_text_query,
                &mut commands,
            );
        }
    }
}

pub(super) fn handle_enter_key_submit(
    keyboard: Res<ButtonInput<KeyCode>>,
    name_input: Res<PlayerNameInput>,
    mut feedback: ResMut<NameSubmissionFeedback>,
    client_query: Query<(Entity, &MessageSender<SubmitPlayerName>), With<crate::GameClient>>,
    mut error_text_query: Query<&mut Text, With<ErrorMessageText>>,
    mut commands: Commands,
) {
    if !keyboard.just_pressed(KeyCode::Enter) {
        return;
    }

    submit_name(
        &name_input,
        &mut feedback,
        client_query,
        &mut error_text_query,
        &mut commands,
    );
}

pub(super) fn submit_name(
    name_input: &PlayerNameInput,
    feedback: &mut NameSubmissionFeedback,
    client_query: Query<(Entity, &MessageSender<SubmitPlayerName>), With<crate::GameClient>>,
    error_text_query: &mut Query<&mut Text, With<ErrorMessageText>>,
    commands: &mut Commands,
) {
    // Don't submit if already submitted
    if name_input.submitted {
        return;
    }

    let name = name_input.name.trim();

    // Basic validation
    if name.len() < 3 {
        let error_msg = "Name must be at least 3 characters".to_string();
        feedback.error_message = Some(error_msg.clone());
        for mut text in error_text_query.iter_mut() {
            text.0 = error_msg.clone();
        }
        return;
    }
    if name.len() > 16 {
        let error_msg = "Name must be at most 16 characters".to_string();
        feedback.error_message = Some(error_msg.clone());
        for mut text in error_text_query.iter_mut() {
            text.0 = error_msg.clone();
        }
        return;
    }

    // Send to server
    let Ok((client_entity, _sender)) = client_query.single() else {
        warn!("No client entity found - cannot submit name");
        return;
    };

    info!("Submitting player name: '{}'", name);

    // Clone the name for the closure
    let name_clone = name.to_string();

    // Send message using commands queue
    commands.queue(move |world: &mut World| {
        if let Some(mut sender) = world.get_mut::<MessageSender<SubmitPlayerName>>(client_entity) {
            sender.send::<ReliableChannel>(SubmitPlayerName { name: name_clone });
        }
    });

    // Mark as submitted
    commands.entity(client_entity).insert(PlayerNameSubmitted);

    // Clear error
    feedback.error_message = None;
    for mut text in error_text_query.iter_mut() {
        text.0 = String::new();
    }
}
