//! actions systems.

use super::*;

#[cfg(test)]
mod tests;

/// Open the selected person's full encyclopedia record. The compact plate
/// remains an at-a-glance selection readout; durable life details belong in
/// the scrollable People page.
pub(super) fn handle_selection_expand_button(
    selection: Res<crate::selection::Selection>,
    input: Res<InputState>,
    characters: Query<&shared::components::PersonId>,
    mut open: ResMut<crate::ui::encyclopedia::EncyclopediaOpen>,
    mut tab: ResMut<crate::ui::encyclopedia::EncyclopediaTab>,
    mut selected: ResMut<crate::ui::encyclopedia::SelectedPerson>,
    buttons: Query<
        &Interaction,
        (
            With<SelectionExpandButton>,
            Changed<Interaction>,
            Without<bevy::ui::InteractionDisabled>,
        ),
    >,
) {
    if input.gameplay_blocking() {
        return;
    }
    for interaction in buttons.iter() {
        if *interaction != Interaction::Pressed || selection.len() != 1 {
            continue;
        }
        // By durable id, never by name: names collide, ids do not.
        let Some(person) = selection
            .primary()
            .and_then(|entity| characters.get(entity).ok())
            .filter(|person| person.is_assigned())
        else {
            continue;
        };
        selected.0 = Some(*person);
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
    mut debug_menu: ResMut<crate::ui::debug_time_menu::DebugTimeMenuOpen>,
    other_modals: Query<
        (),
        (
            With<crate::ui::modal::ModalRoot>,
            Without<crate::ui::debug_time_menu::DebugMenuRoot>,
        ),
    >,
) {
    if input_state.text_input_blocking() || !keyboard.just_pressed(KeyCode::KeyG) {
        return;
    }
    // The developer console hides the HUD switch and owns the modal input
    // mutex. It must still allow leaving God mode, even while the server is
    // busy advancing accelerated time. Other screens retain their input.
    if *mode == HudMode::God
        && debug_menu.0
        && other_modals.is_empty()
        && !input_state.permit_tray_open
    {
        *mode = HudMode::Play;
        debug_menu.0 = false;
    } else if !input_state.gameplay_blocking() && (*mode == HudMode::God || capability.0) {
        *mode = mode.toggled();
        if *mode == HudMode::God {
            opening.cancel();
        }
    }
}

pub(super) fn handle_mode_chip_button(
    capability: Res<GodCapability>,
    keyboard: Res<ButtonInput<KeyCode>>,
    input: Res<InputState>,
    mut mode: ResMut<HudMode>,
    mut opening: ResMut<crate::boat::OpeningCinematic>,
    buttons: Query<
        &Interaction,
        (
            With<ModeChipButton>,
            Changed<Interaction>,
            Without<bevy::ui::InteractionDisabled>,
        ),
    >,
) {
    // A key and mouse edge can arrive together after a long frame. Apply one
    // transition; an invisible HUD control cannot undo the console's exit.
    if input.gameplay_blocking() || keyboard.just_pressed(KeyCode::KeyG) {
        return;
    }
    for interaction in buttons.iter() {
        if *interaction == Interaction::Pressed && (*mode == HudMode::God || capability.0) {
            *mode = mode.toggled();
            if *mode == HudMode::God {
                opening.cancel();
            }
            break;
        }
    }
}

pub(super) fn handle_warp_buttons(
    mode: Res<HudMode>,
    capability: Res<GodCapability>,
    input: Res<InputState>,
    mut dev_sender: Query<
        &mut MessageSender<DevCommand>,
        (With<crate::GameClient>, With<Connected>),
    >,
    buttons: Query<(&Interaction, &WarpButton), Changed<Interaction>>,
) {
    if *mode != HudMode::God || !capability.0 || input.gameplay_blocking() {
        return;
    }
    for (interaction, WarpButton(factor)) in buttons.iter() {
        if *interaction == Interaction::Pressed {
            if let Ok(mut sender) = dev_sender.single_mut() {
                sender.send::<ReliableChannel>(DevCommand::SetTimeWarp(*factor));
            }
        }
    }
}

/// Arm villager placement. Stays armed across clicks so a crowd can be dropped
/// in one go; Escape or leaving god mode clears it.
pub(super) fn handle_spawn_npc_button(
    input: Res<InputState>,
    mut placement: ResMut<crate::hero::control::WorldPlacementMode>,
    buttons: Query<&Interaction, (With<SpawnNpcButton>, Changed<Interaction>)>,
) {
    if input.gameplay_blocking() {
        return;
    }
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

/// Ask the server for one production-path immigrant voyage without disturbing
/// the simulation speed the player deliberately selected.
pub(super) fn handle_immigrant_boat_button(
    input: Res<InputState>,
    mut watch: ResMut<ImmigrantBoatWatch>,
    mut notice: ResMut<GodNotice>,
    mut senders: Query<&mut MessageSender<DevCommand>, (With<crate::GameClient>, With<Connected>)>,
    buttons: Query<&Interaction, (With<SpawnImmigrantBoatButton>, Changed<Interaction>)>,
) {
    if input.gameplay_blocking() {
        return;
    }
    for interaction in buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if watch.active() {
            watch.clear();
            notice.show("Stopped following the immigrant voyage");
            continue;
        }
        let Ok(mut sender) = senders.single_mut() else {
            notice.show("The server is not connected");
            continue;
        };
        sender.send::<ReliableChannel>(DevCommand::SpawnImmigrantBoat);
        watch.waiting = true;
        watch.waited_seconds = 0.0;
        notice.show("Finding a random coast and launching an immigrant…");
    }
}

/// Acquire the newly replicated dinghy, keep it centred during the voyage,
/// then leave the camera at the beach so the player sees the villager step
/// ashore and begin the ordinary walk toward the Moot Hall.
pub(super) fn watch_immigrant_boat(
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
    input: Res<InputState>,
    mode: Res<HudMode>,
    mut watch: ResMut<ImmigrantBoatWatch>,
    boats: Query<
        (Entity, &shared::components::PlayerPosition),
        With<shared::components::ImmigrantArrivalBoat>,
    >,
    mut cameras: Query<&mut crate::camera_rts::CommanderCamera>,
    mut notice: ResMut<GodNotice>,
) {
    const WATCH_ZOOM: f32 = 82.0;
    const WAIT_TIMEOUT_SECONDS: f32 = 30.0;

    if watch.active()
        && (*mode != HudMode::God
            || (!input.text_input_blocking() && keyboard.just_pressed(KeyCode::Escape)))
    {
        watch.clear();
        notice.show("Stopped following the immigrant voyage");
    }

    let live = boats.iter().map(|(entity, _)| entity).collect::<Vec<_>>();
    if let Some(following) = watch.following {
        if let Ok((_, position)) = boats.get(following) {
            for mut camera in cameras.iter_mut() {
                camera.focus = position.0;
                camera.focus_target = position.0;
            }
        } else {
            watch.following = None;
            notice.show("Immigrant ashore — now walking to the Moot Hall");
        }
    } else if watch.waiting {
        watch.waited_seconds += time.delta_secs();
        if let Some((boat, position)) = boats
            .iter()
            .find(|(entity, _)| !watch.known.contains(entity))
        {
            watch.waiting = false;
            watch.following = Some(boat);
            for mut camera in cameras.iter_mut() {
                camera.focus = position.0;
                camera.focus_target = position.0;
                camera.zoom = WATCH_ZOOM.clamp(camera.zoom_min, camera.zoom_max);
                camera.zoom_target = camera.zoom;
            }
            notice.show("Following the immigrant boat — Escape or the button stops watching");
        } else if watch.waited_seconds >= WAIT_TIMEOUT_SECONDS {
            watch.clear();
            notice.show("No voyage launched — found a Moot and try again");
        }
    }

    watch.known.retain(|entity| live.contains(entity));
    watch.known.extend(live);
}

/// Arm settlement founding. Disarms the other placements: only one thing can be
/// waiting on the next click.
pub(super) fn handle_found_village_button(
    input: Res<InputState>,
    mut placement: ResMut<crate::hero::control::WorldPlacementMode>,
    buttons: Query<&Interaction, (With<FoundVillageButton>, Changed<Interaction>)>,
) {
    if input.gameplay_blocking() {
        return;
    }
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

pub(super) fn handle_spawn_catapult_button(
    input: Res<InputState>,
    mut placement: ResMut<crate::hero::control::WorldPlacementMode>,
    buttons: Query<&Interaction, (With<SpawnCatapultButton>, Changed<Interaction>)>,
) {
    if input.gameplay_blocking() {
        return;
    }
    for interaction in buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        // Only one placement can be armed: an invisible second armed mode would
        // make the next click do something the player did not ask for.
        *placement = if placement.is_spawn_catapult() {
            crate::hero::control::WorldPlacementMode::None
        } else {
            crate::hero::control::WorldPlacementMode::SpawnCatapult
        };
    }
}
