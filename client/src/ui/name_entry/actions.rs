//! Validated, single-flight name submission and safe return to the launcher.

use super::*;
use bevy::input_focus::InputFocus;
use bevy::ui::InteractionDisabled;
use lightyear::prelude::*;
use shared::protocol::{ReliableChannel, SubmitPlayerName};

pub(super) fn reset_name_entry(
    mut input: ResMut<PlayerNameInput>,
    mut phase: ResMut<NameEntryPhase>,
    mut feedback: ResMut<NameSubmissionFeedback>,
    mut editing: ResMut<input::NameInputEditing>,
) {
    input.name.clear();
    input.submitted = false;
    *phase = NameEntryPhase::Editing;
    feedback.error_message = None;
    *editing = default();
}

pub(super) fn handle_submit_button(
    buttons: Query<
        (Entity, &Interaction),
        (
            Changed<Interaction>,
            With<SubmitButton>,
            Without<InteractionDisabled>,
        ),
    >,
    mut input: ResMut<PlayerNameInput>,
    mut phase: ResMut<NameEntryPhase>,
    mut feedback: ResMut<NameSubmissionFeedback>,
    mut clients: Query<
        (Entity, &mut MessageSender<SubmitPlayerName>),
        (With<crate::GameClient>, With<Connected>),
    >,
    mut commands: Commands,
    mut sounds: crate::ui::sound::UiActionSounds,
) {
    if let Some((entity, _)) = buttons
        .iter()
        .find(|(_, interaction)| **interaction == Interaction::Pressed)
    {
        let audible = sounds.pressed(entity);
        submit_name(
            &mut input,
            &mut phase,
            &mut feedback,
            &mut clients,
            &mut commands,
            &mut sounds,
            audible,
        );
    }
}

pub(super) fn handle_enter_key_submit(
    keyboard: Res<ButtonInput<KeyCode>>,
    focus: Option<Res<InputFocus>>,
    back: Query<Entity, With<BackButton>>,
    submit: Query<Entity, (With<SubmitButton>, Without<InteractionDisabled>)>,
    mut input: ResMut<PlayerNameInput>,
    mut phase: ResMut<NameEntryPhase>,
    mut feedback: ResMut<NameSubmissionFeedback>,
    mut clients: Query<
        (Entity, &mut MessageSender<SubmitPlayerName>),
        (With<crate::GameClient>, With<Connected>),
    >,
    mut commands: Commands,
    mut sounds: crate::ui::sound::UiActionSounds,
) {
    let focused = focus.as_ref().and_then(|focus| focus.get());
    let submit_key = keyboard.just_pressed(KeyCode::Enter)
        || keyboard.just_pressed(KeyCode::NumpadEnter)
        || (keyboard.just_pressed(KeyCode::Space)
            && focused.is_some_and(|entity| submit.contains(entity)));
    if !submit_key || focused.is_some_and(|entity| back.contains(entity)) {
        return;
    }
    submit_name(
        &mut input,
        &mut phase,
        &mut feedback,
        &mut clients,
        &mut commands,
        &mut sounds,
        true,
    );
}

/// Mirrors the server's format gate only. Reserved and online names remain
/// authoritative server decisions; the protocol currently bounds UTF-8 bytes.
fn validate_format(name: &str) -> Result<(), &'static str> {
    if name.len() < 3 {
        return Err("Name is too short.");
    }
    if name.len() > 16 {
        return Err("Name is too long.");
    }
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return Err("Use letters, numbers, _ or -.");
    }
    Ok(())
}

fn submit_name(
    input: &mut PlayerNameInput,
    phase: &mut NameEntryPhase,
    feedback: &mut NameSubmissionFeedback,
    clients: &mut Query<
        (Entity, &mut MessageSender<SubmitPlayerName>),
        (With<crate::GameClient>, With<Connected>),
    >,
    commands: &mut Commands,
    sounds: &mut crate::ui::sound::UiActionSounds,
    audible: bool,
) {
    if input.submitted || phase.is_busy() {
        return;
    }
    let name = input.name.trim();
    if let Err(message) = validate_format(name) {
        feedback.error_message = Some(message.into());
        if audible {
            sounds.emit(crate::audio::sfx::SfxCue::UiReject);
        }
        return;
    }
    let Ok((entity, mut sender)) = clients.single_mut() else {
        feedback.error_message = Some("Connection lost. Go back and reconnect.".into());
        if audible {
            sounds.emit(crate::audio::sfx::SfxCue::UiReject);
        }
        return;
    };
    let name = name.to_owned();
    // Lock both the UI and gameplay identity before a second input system can
    // submit in this same frame; deferred component insertion alone is too late.
    input.name.clone_from(&name);
    input.submitted = true;
    *phase = NameEntryPhase::Submitting;
    feedback.error_message = None;
    sender.send::<ReliableChannel>(SubmitPlayerName { name });
    if audible {
        sounds.emit(crate::audio::sfx::SfxCue::UiClick);
    }
    commands.entity(entity).insert(PlayerNameSubmitted);
}

pub(super) fn handle_back(
    keyboard: Res<ButtonInput<KeyCode>>,
    focus: Option<Res<InputFocus>>,
    buttons: Query<(Entity, &Interaction), (With<BackButton>, Without<InteractionDisabled>)>,
    clients: Query<Entity, With<crate::GameClient>>,
    mut commands: Commands,
    mut next: ResMut<NextState<GameState>>,
    mut input: ResMut<PlayerNameInput>,
    mut phase: ResMut<NameEntryPhase>,
    mut feedback: ResMut<NameSubmissionFeedback>,
    mut connection_feedback: ResMut<crate::render::systems::ConnectionFeedback>,
) {
    let activate_key = keyboard.just_pressed(KeyCode::Enter)
        || keyboard.just_pressed(KeyCode::NumpadEnter)
        || keyboard.just_pressed(KeyCode::Space);
    let focused = focus.as_ref().and_then(|focus| focus.get());
    if !keyboard.just_pressed(KeyCode::Escape)
        && !buttons.iter().any(|(entity, interaction)| {
            *interaction == Interaction::Pressed || (activate_key && focused == Some(entity))
        })
    {
        return;
    }
    input.submitted = false;
    *phase = NameEntryPhase::Editing;
    feedback.error_message = None;
    connection_feedback.error_message = None;
    for entity in &clients {
        commands.trigger(Disconnect { entity });
    }
    next.set(GameState::MainMenu);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_gate_matches_current_protocol_bounds_and_name_characters() {
        for name in ["Aldric", "Sir_Robin-2", "Åke", "abcdefghijklmnop"] {
            assert!(validate_format(name).is_ok(), "{name}");
        }
        for name in [
            "Al",
            "abcdefghijklmnopq",
            "Sir Robin",
            "name/../",
            "Robin🙂",
            "Ééééééééé",
        ] {
            assert!(validate_format(name).is_err(), "{name}");
        }
        // These require the server's account registry / reserved-name policy.
        assert!(validate_format("admin").is_ok());
    }

    #[test]
    fn back_during_preparation_unlocks_name_and_returns_to_launcher() {
        let mut app = App::new();
        app.insert_resource(PlayerNameInput {
            name: "Aldric".into(),
            submitted: true,
        })
        .insert_resource(NameEntryPhase::Preparing)
        .init_resource::<NameSubmissionFeedback>()
        .init_resource::<crate::render::systems::ConnectionFeedback>()
        .init_resource::<NextState<GameState>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .add_systems(Update, handle_back);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        assert!(!app.world().resource::<PlayerNameInput>().submitted);
        assert_eq!(
            *app.world().resource::<NameEntryPhase>(),
            NameEntryPhase::Editing
        );
        assert!(matches!(
            app.world().resource::<NextState<GameState>>(),
            NextState::Pending(GameState::MainMenu)
        ));
    }

    #[test]
    fn simultaneous_mouse_and_enter_freeze_one_account_request_immediately() {
        let mut app = App::new();
        app.insert_resource(PlayerNameInput {
            name: "  Aldric  ".into(),
            submitted: false,
        })
        .init_resource::<NameEntryPhase>()
        .init_resource::<NameSubmissionFeedback>()
        .init_resource::<ButtonInput<KeyCode>>()
        .add_systems(
            Update,
            (handle_submit_button, handle_enter_key_submit).chain(),
        );
        app.register_message::<SubmitPlayerName>();
        let client = app
            .world_mut()
            .spawn((
                crate::GameClient,
                RemoteId(PeerId::Server),
                MessageSender::<SubmitPlayerName>::default(),
            ))
            .id();
        app.world_mut().entity_mut(client).insert(Connected);
        app.world_mut().spawn((SubmitButton, Interaction::Pressed));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        let input = app.world().resource::<PlayerNameInput>();
        assert!(input.submitted);
        assert_eq!(input.name, "Aldric");
        assert_eq!(
            *app.world().resource::<NameEntryPhase>(),
            NameEntryPhase::Submitting
        );
        assert!(app.world().get::<PlayerNameSubmitted>(client).is_some());
        // A repeat activation cannot revalidate or alter the accepted draft.
        app.world_mut()
            .resource_mut::<NameSubmissionFeedback>()
            .error_message = Some("sentinel".into());
        app.update();
        assert_eq!(
            app.world()
                .resource::<NameSubmissionFeedback>()
                .error_message
                .as_deref(),
            Some("sentinel")
        );
    }
}
