//! Resolve the editable address before requesting a connection.
use super::*;
use bevy::input_focus::InputFocus;

pub(super) fn handle_menu_actions(
    buttons: Query<(Entity, Ref<Interaction>, &MenuButton)>,
    keyboard: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    mut editing: ResMut<network_input::ServerAddressEditing>,
    mut address: ResMut<ServerAddress>,
    mut feedback: ResMut<crate::render::systems::ConnectionFeedback>,
    mut next_state: ResMut<NextState<GameState>>,
    mut exit: MessageWriter<AppExit>,
    mut sounds: crate::ui::sound::UiActionSounds,
) {
    let submit_requested = std::mem::take(&mut editing.submit_requested);
    let mut action = submit_requested.then_some(MenuButton::Connect);
    let mut audible = submit_requested;
    let activate =
        keyboard.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space]);
    for (entity, interaction, button) in &buttons {
        if (interaction.is_changed() && *interaction == Interaction::Pressed)
            || (activate && focus.get() == Some(entity))
        {
            action = Some(*button);
            audible = (activate && focus.get() == Some(entity)) || sounds.pressed(entity);
        }
    }
    if focus.get().is_none() && keyboard.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter]) {
        action = Some(MenuButton::Connect);
        audible = true;
    }
    match action {
        Some(MenuButton::Connect) => match editing.resolve(address.port) {
            Ok(validated) => {
                *address = validated;
                feedback.error_message = None;
                next_state.set(GameState::Connecting);
                if audible {
                    sounds.emit(crate::audio::sfx::SfxCue::UiConfirm);
                }
            }
            Err(message) => {
                feedback.error_message = Some(message.into());
                if audible {
                    sounds.emit(crate::audio::sfx::SfxCue::UiReject);
                }
            }
        },
        Some(MenuButton::Exit) => {
            if audible {
                sounds.emit(crate::audio::sfx::SfxCue::UiClick);
            }
            exit.write(AppExit::Success);
        }
        None => {}
    }
}
