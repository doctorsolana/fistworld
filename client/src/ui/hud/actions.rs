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

/// SPAWN HERO opens the character creator (the modal owns outfit choice and
/// arms placement on PLACE). Dead while a hero exists — one per player.
pub(super) fn handle_spawn_hero_button(
    mut creator: ResMut<crate::ui::hero_creator::HeroCreatorOpen>,
    mut arm: ResMut<crate::hero::control::HeroSpawnArm>,
    local: Option<Res<crate::camera_rts::LocalPeerId>>,
    heroes: Query<&shared::components::Hero>,
    buttons: Query<&Interaction, (With<SpawnHeroButton>, Changed<Interaction>)>,
) {
    for interaction in buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let owns_hero = local
            .as_ref()
            .is_some_and(|local| crate::hero::control::local_hero_exists(&heroes, local));
        if owns_hero {
            arm.0 = false;
            continue;
        }
        // An armed placement reopens the creator instead of toggling blind.
        arm.0 = false;
        creator.0 = true;
    }
}
