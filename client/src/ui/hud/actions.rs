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
    heroes: Query<(&shared::components::Hero, &shared::components::PlayerPosition)>,
    mut cameras: Query<&mut crate::camera_rts::CommanderCamera>,
    mut notice: ResMut<super::GodNotice>,
    buttons: Query<&Interaction, (With<SpawnHeroButton>, Changed<Interaction>)>,
) {
    for interaction in buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        // You only ever have one hero, and it PERSISTS -- leaving the game does
        // not delete it, so on rejoining the button will say HERO ACTIVE and
        // refuse to place another. That is correct, but "HERO ACTIVE" answered
        // a question nobody asked and left the button dead.
        //
        // It now takes you to them. The most likely reason a player is pressing
        // it is that they cannot see their hero, and the honest answer to that
        // is not a label, it is the camera.
        let mine = local.as_ref().and_then(|local| {
            heroes
                .iter()
                .find(|(hero, _)| shared::player::peer_id_to_u64(hero.owner) == local.0)
                .map(|(_, at)| at.0)
        });
        if let Some(at) = mine {
            arm.0 = false;
            for mut camera in cameras.iter_mut() {
                camera.focus = at;
            }
            notice.show("Your hero is here — you only get one");
            continue;
        }
        // An armed placement reopens the creator instead of toggling blind.
        arm.0 = false;
        creator.0 = true;
    }
}

/// Arm villager placement. Stays armed across clicks so a crowd can be dropped
/// in one go; Escape or leaving god mode clears it.
pub(super) fn handle_spawn_npc_button(
    mut npc_arm: ResMut<crate::hero::control::NpcSpawnArm>,
    mut hero_arm: ResMut<crate::hero::control::HeroSpawnArm>,
    buttons: Query<&Interaction, (With<SpawnNpcButton>, Changed<Interaction>)>,
) {
    for interaction in buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        // Only one placement can be armed: an invisible second armed mode would
        // make the next click do something the player did not ask for.
        hero_arm.0 = false;
        npc_arm.0 = !npc_arm.0;
    }
}

/// Arm settlement founding. Disarms the other placements: only one thing can be
/// waiting on the next click.
pub(super) fn handle_found_village_button(
    mut found_arm: ResMut<crate::hero::control::FoundSpawnArm>,
    mut hero_arm: ResMut<crate::hero::control::HeroSpawnArm>,
    mut npc_arm: ResMut<crate::hero::control::NpcSpawnArm>,
    buttons: Query<&Interaction, (With<FoundVillageButton>, Changed<Interaction>)>,
) {
    for interaction in buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        hero_arm.0 = false;
        npc_arm.0 = false;
        found_arm.0 = !found_arm.0;
    }
}
