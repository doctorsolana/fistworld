//! actions systems.

use super::*;

/// Open the selected person's full encyclopedia record. The compact plate
/// remains an at-a-glance selection readout; durable life details belong in
/// the scrollable People page.
pub(super) fn handle_selection_expand_button(
    selection: Res<crate::selection::Selection>,
    characters: Query<&shared::components::CharacterName>,
    mut open: ResMut<crate::ui::encyclopedia::EncyclopediaOpen>,
    mut tab: ResMut<crate::ui::encyclopedia::EncyclopediaTab>,
    mut selected: ResMut<crate::ui::encyclopedia::SelectedPerson>,
    buttons: Query<&Interaction, (With<SelectionExpandButton>, Changed<Interaction>)>,
) {
    for interaction in buttons.iter() {
        if *interaction != Interaction::Pressed || selection.len() != 1 {
            continue;
        }
        let Some(name) = selection
            .primary()
            .and_then(|entity| characters.get(entity).ok())
        else {
            continue;
        };
        selected.0 = Some(name.0.clone());
        *tab = crate::ui::encyclopedia::EncyclopediaTab::People;
        open.0 = true;
    }
}

pub(super) fn handle_mode_toggle_key(
    keyboard: Res<ButtonInput<KeyCode>>,
    input_state: Res<InputState>,
    capability: Res<GodCapability>,
    mut mode: ResMut<HudMode>,
    mut opening: ResMut<crate::boat::OpeningCinematic>,
) {
    if !capability.0 || input_state.ui_blocking() {
        return;
    }
    if keyboard.just_pressed(KeyCode::KeyG) {
        *mode = mode.toggled();
        if *mode == HudMode::God {
            opening.cancel();
        }
    }
}

pub(super) fn handle_mode_chip_button(
    capability: Res<GodCapability>,
    mut mode: ResMut<HudMode>,
    buttons: Query<&Interaction, (With<ModeChipButton>, Changed<Interaction>)>,
) {
    for interaction in buttons.iter() {
        if *interaction == Interaction::Pressed && capability.0 {
            *mode = mode.toggled();
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
    mut creator_purpose: ResMut<crate::ui::hero_creator::HeroCreatorPurpose>,
    mut placement: ResMut<crate::hero::control::WorldPlacementMode>,
    local: Option<Res<crate::camera_rts::LocalPeerId>>,
    heroes: Query<(
        &shared::components::Hero,
        &shared::components::PlayerPosition,
    )>,
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
            *placement = crate::hero::control::WorldPlacementMode::None;
            for mut camera in cameras.iter_mut() {
                camera.focus = at;
            }
            notice.show("Your hero is here — you only get one");
            continue;
        }
        // An armed placement reopens the creator instead of toggling blind.
        *placement = crate::hero::control::WorldPlacementMode::None;
        *creator_purpose = crate::ui::hero_creator::HeroCreatorPurpose::GodPlacement;
        creator.0 = true;
    }
}

/// Arm villager placement. Stays armed across clicks so a crowd can be dropped
/// in one go; Escape or leaving god mode clears it.
pub(super) fn handle_spawn_npc_button(
    mut placement: ResMut<crate::hero::control::WorldPlacementMode>,
    buttons: Query<&Interaction, (With<SpawnNpcButton>, Changed<Interaction>)>,
) {
    for interaction in buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        // Only one placement can be armed: an invisible second armed mode would
        // make the next click do something the player did not ask for.
        *placement = if placement.is_spawn_npc() {
            crate::hero::control::WorldPlacementMode::None
        } else {
            crate::hero::control::WorldPlacementMode::SpawnNpc
        };
    }
}

/// Arm settlement founding. Disarms the other placements: only one thing can be
/// waiting on the next click.
pub(super) fn handle_found_village_button(
    mut placement: ResMut<crate::hero::control::WorldPlacementMode>,
    buttons: Query<&Interaction, (With<FoundVillageButton>, Changed<Interaction>)>,
) {
    for interaction in buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        *placement = if placement.is_found_settlement() {
            crate::hero::control::WorldPlacementMode::None
        } else {
            crate::hero::control::WorldPlacementMode::FoundSettlement
        };
    }
}
