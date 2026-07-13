//! actions systems.

use super::*;

pub(super) fn debug_menu_open(open: Res<DebugTimeMenuOpen>) -> bool {
    open.0
}

pub(super) fn debug_menu_closed(open: Res<DebugTimeMenuOpen>) -> bool {
    !open.0
}

pub(super) fn toggle_debug_time_menu(
    keyboard: Res<ButtonInput<KeyCode>>,
    game_state: Res<State<GameState>>,
    mut open: ResMut<DebugTimeMenuOpen>,
    input_state: Res<InputState>,
) {
    if game_state.get() != &GameState::Playing {
        return;
    }
    if input_state.inventory_open || input_state.pause_menu_open || input_state.map_open {
        return;
    }
    if keyboard.just_pressed(KeyCode::KeyJ) {
        open.0 = !open.0;
    }
}

pub(super) fn close_debug_time_menu_on_escape(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut open: ResMut<DebugTimeMenuOpen>,
) {
    if open.0 && keyboard.just_pressed(KeyCode::Escape) {
        open.0 = false;
    }
}

pub(super) fn handle_backdrop_click(
    backdrop: Query<&Interaction, (With<DebugMenuBackdrop>, Changed<Interaction>)>,
    mut open: ResMut<DebugTimeMenuOpen>,
) {
    if !open.0 {
        return;
    }
    if handle_backdrop_pressed::<DebugMenuBackdrop>(&backdrop) {
        open.0 = false;
    }
}

pub(super) fn handle_debug_menu_interactions(
    mut open: ResMut<DebugTimeMenuOpen>,
    mut input_state: ResMut<InputState>,
    mut cover: ResMut<CloudCover>,
    mut cover_override: ResMut<CloudCoverOverride>,
    mut char_selection: ResMut<DebugCharacterSelection>,
    mut perf_settings: ResMut<DebugPerfSettings>,
    mut time_sender: Query<
        &mut MessageSender<SetTimeOfDay>,
        (With<crate::GameClient>, With<Connected>),
    >,
    mut char_sender: Query<
        &mut MessageSender<SetPlayerCharacter>,
        (With<crate::GameClient>, With<Connected>),
    >,
    mut npc_spawn_sender: Query<
        &mut MessageSender<SpawnOilmanDebug>,
        (With<crate::GameClient>, With<Connected>),
    >,
    mut physics_box_sender: Query<
        &mut MessageSender<SpawnPhysicsBoxDebug>,
        (With<crate::GameClient>, With<Connected>),
    >,
    local_player_transforms: Query<&Transform, With<LocalPlayer>>,
    mut buttons: Query<
        (
            &Interaction,
            Option<&TimeButton>,
            Option<&CloseButton>,
            Option<&FlyToggleButton>,
            Option<&CloudCoverButton>,
            Option<&CharacterToggleButton>,
            Option<&PerfWeightmapToggleButton>,
            Option<&PerfRenderDiagToggleButton>,
            Option<&SpawnOilmanNpcButton>,
            Option<&SpawnDummyNpcButton>,
            Option<&SpawnPhysicsBoxButton>,
            &mut BackgroundColor,
        ),
        Changed<Interaction>,
    >,
) {
    for (
        interaction,
        time_button,
        close_button,
        fly_button,
        cover_button,
        char_button,
        weightmap_button,
        render_diag_button,
        oilman_spawn_button,
        dummy_spawn_button,
        physics_box_button,
        mut bg,
    ) in buttons.iter_mut()
    {
        match *interaction {
            Interaction::Pressed => {
                *bg = BUTTON_PRESSED.into();

                if close_button.is_some() {
                    open.0 = false;
                    continue;
                }

                if fly_button.is_some() {
                    input_state.fly_mode = !input_state.fly_mode;
                    continue;
                }

                if let Some(CloudCoverButton(mode)) = cover_button {
                    cover_override.mode = *mode;
                    match mode {
                        CloudCoverMode::Auto => {
                            cover.segment = -1;
                        }
                        CloudCoverMode::Clear => {
                            cover.current = 0.0;
                            cover.target = 0.0;
                        }
                        CloudCoverMode::Cloudy => {
                            cover.current = 1.0;
                            cover.target = 1.0;
                        }
                    }
                    continue;
                }

                if char_button.is_some() {
                    let next = match char_selection.current {
                        PlayerCharacter::Oilman => PlayerCharacter::Base,
                        PlayerCharacter::Base => PlayerCharacter::Oilman,
                    };
                    char_selection.current = next;
                    if let Ok(mut sender) = char_sender.single_mut() {
                        sender.send::<ReliableChannel>(SetPlayerCharacter { character: next });
                    }
                    continue;
                }

                if weightmap_button.is_some() {
                    perf_settings.weightmap_stats = !perf_settings.weightmap_stats;
                    continue;
                }

                if render_diag_button.is_some() {
                    perf_settings.render_diag_logging = !perf_settings.render_diag_logging;
                    continue;
                }

                if oilman_spawn_button.is_some() {
                    if let Ok(mut sender) = npc_spawn_sender.single_mut() {
                        sender.send::<ReliableChannel>(SpawnOilmanDebug {
                            count: 10,
                            archetype: shared::components::NpcArchetype::Oilman,
                        });
                    }
                    continue;
                }

                if dummy_spawn_button.is_some() {
                    if let Ok(mut sender) = npc_spawn_sender.single_mut() {
                        sender.send::<ReliableChannel>(SpawnOilmanDebug {
                            count: 1,
                            archetype: shared::components::NpcArchetype::Dummy,
                        });
                    }
                    continue;
                }

                if physics_box_button.is_some() {
                    let anchor_position = local_player_transforms
                        .iter()
                        .next()
                        .map(|transform| transform.translation);
                    if let Ok(mut sender) = physics_box_sender.single_mut() {
                        sender.send::<ReliableChannel>(SpawnPhysicsBoxDebug {
                            count: 1,
                            anchor_position,
                        });
                        info!(
                            "Requested debug physics box spawn at local anchor={:?}",
                            anchor_position
                        );
                    } else {
                        warn!("No SpawnPhysicsBoxDebug sender available on client");
                    }
                    continue;
                }

                if let Some(TimeButton(preset)) = time_button {
                    if let Ok(mut sender) = time_sender.single_mut() {
                        sender.send::<ReliableChannel>(SetTimeOfDay { preset: *preset });
                    }
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

pub(super) fn close_debug_menu_on_main_menu(
    mut open: ResMut<DebugTimeMenuOpen>,
    mut input_state: ResMut<InputState>,
) {
    open.0 = false;
    input_state.debug_menu_open = false;
}
