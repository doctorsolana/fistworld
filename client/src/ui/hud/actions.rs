//! actions systems.

use super::*;

pub(super) fn handle_mode_toggle_key(
    keyboard: Res<ButtonInput<KeyCode>>,
    input_state: Res<InputState>,
    capability: Res<GodCapability>,
    mut mode: ResMut<HudMode>,
) {
    if !capability.0 || input_state.ui_blocking() {
        return;
    }
    if keyboard.just_pressed(KeyCode::KeyG) {
        *mode = mode.toggled();
    }
}

pub(super) fn handle_mode_chip_button(
    capability: Res<GodCapability>,
    mut mode: ResMut<HudMode>,
    mut buttons: Query<
        (&Interaction, &mut BackgroundColor),
        (With<ModeChipButton>, Changed<Interaction>),
    >,
) {
    for (interaction, mut bg) in buttons.iter_mut() {
        match *interaction {
            Interaction::Pressed => {
                *bg = BUTTON_PRESSED.into();
                if capability.0 {
                    *mode = mode.toggled();
                }
            }
            Interaction::Hovered => {
                *bg = BUTTON_HOVERED.into();
            }
            Interaction::None => {
                *bg = BUTTON_NORMAL.into();
            }
        }
    }
}

pub(super) fn handle_warp_buttons(
    mut dev_sender: Query<
        &mut MessageSender<DevCommand>,
        (With<crate::GameClient>, With<Connected>),
    >,
    buttons: Query<(&Interaction, &WarpButton), Changed<Interaction>>,
) {
    for (interaction, WarpButton(factor)) in buttons.iter() {
        if *interaction == Interaction::Pressed {
            if let Ok(mut sender) = dev_sender.single_mut() {
                sender.send::<ReliableChannel>(DevCommand::SetTimeWarp(*factor));
            }
        }
    }
}
