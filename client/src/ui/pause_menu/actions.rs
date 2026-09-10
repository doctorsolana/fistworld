//! actions systems.

use super::*;

pub(super) fn pause_menu_open(open: Res<PauseMenuOpen>) -> bool {
    open.0
}

pub(super) fn pause_menu_closed(open: Res<PauseMenuOpen>) -> bool {
    !open.0
}

pub(super) fn sync_pause_menu_cursor(
    open: Res<PauseMenuOpen>,
    input_state: Res<InputState>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut cursor_opts: Query<&mut CursorOptions>,
) {
    if !open.0 {
        return;
    }
    sync_modal_cursor(true, &input_state, &windows, &mut cursor_opts);
}

pub(super) fn reset_menu_state(mut state: ResMut<PauseMenuState>) {
    state.graphics_open = false;
    state.controls_open = false;
    state.transition = 0.0;
}

pub(super) fn handle_pause_actions(
    buttons: Query<(&Interaction, &PauseButton), Changed<Interaction>>,
    mut pause_open: ResMut<PauseMenuOpen>,
    mut exit: MessageWriter<AppExit>,
    mut commands: Commands,
    client_query: Query<Entity, With<GameClient>>,
    mut menu_state: ResMut<PauseMenuState>,
    mut input_state: ResMut<InputState>,
    mut next_state: ResMut<NextState<GameState>>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut cursor_opts: Query<&mut CursorOptions>,
) {
    for (interaction, action) in buttons.iter() {
        if *interaction == Interaction::Pressed {
            match action {
                PauseButton::Resume => {
                    pause_open.0 = false;
                    input_state.pause_menu_open = false;
                    sync_modal_cursor(false, &input_state, &windows, &mut cursor_opts);
                }
                PauseButton::Graphics => {
                    // Toggle the graphics panel (close controls if open)
                    if menu_state.graphics_open {
                        menu_state.graphics_open = false;
                    } else {
                        menu_state.controls_open = false;
                        menu_state.graphics_open = true;
                    }
                }
                PauseButton::Controls => {
                    // Toggle the controls panel (close graphics if open)
                    if menu_state.controls_open {
                        menu_state.controls_open = false;
                    } else {
                        menu_state.graphics_open = false;
                        menu_state.controls_open = true;
                    }
                }
                PauseButton::Disconnect => {
                    info!("Disconnecting from server...");
                    // In Lightyear 0.26, trigger Disconnect on the client entity
                    if let Some(client_entity) = client_query.iter().next() {
                        commands.trigger(Disconnect {
                            entity: client_entity,
                        });
                    }
                    pause_open.0 = false;
                    input_state.pause_menu_open = false;
                    sync_modal_cursor(false, &input_state, &windows, &mut cursor_opts);
                    next_state.set(GameState::MainMenu);
                }
                PauseButton::Exit => {
                    info!("Exiting game...");
                    exit.write(AppExit::Success);
                }
            }
        }
    }
}

pub(super) fn handle_graphics_toggles(
    buttons: Query<(&Interaction, &GraphicsToggle), Changed<Interaction>>,
    mut settings: ResMut<GraphicsSettings>,
    mut toggle_texts: Query<(&ToggleText, &mut Text)>,
    mut toggle_buttons: Query<(&GraphicsToggle, &mut UiButtonStyle), With<Button>>,
) {
    for (interaction, toggle) in buttons.iter() {
        if *interaction == Interaction::Pressed {
            // Toggle the setting
            let new_value = match toggle {
                GraphicsToggle::Bloom => {
                    settings.bloom_enabled = !settings.bloom_enabled;
                    settings.bloom_enabled
                }
                GraphicsToggle::Shadows => {
                    settings.shadows_enabled = !settings.shadows_enabled;
                    settings.shadows_enabled
                }
                GraphicsToggle::Atmosphere => {
                    settings.atmosphere_enabled = !settings.atmosphere_enabled;
                    settings.atmosphere_enabled
                }
                GraphicsToggle::Clouds => {
                    settings.clouds_enabled = !settings.clouds_enabled;
                    settings.clouds_enabled
                }
                GraphicsToggle::Vsync => {
                    settings.vsync_enabled = !settings.vsync_enabled;
                    settings.vsync_enabled
                }
            };

            info!("Graphics toggle {:?} = {}", toggle, new_value);

            // Update the text
            for (toggle_text, mut text) in toggle_texts.iter_mut() {
                if std::mem::discriminant(&toggle_text.0) == std::mem::discriminant(toggle) {
                    text.0 = if new_value {
                        "ON".to_string()
                    } else {
                        "OFF".to_string()
                    };
                }
            }

            for (btn_toggle, mut style) in toggle_buttons.iter_mut() {
                if std::mem::discriminant(btn_toggle) == std::mem::discriminant(toggle) {
                    style.selected = new_value;
                }
            }
        }
    }
}

pub(super) fn arm_display_confirmation(
    commands: &mut Commands,
    pending: Option<&mut PendingDisplayChange>,
    settings: &GraphicsSettings,
) {
    if let Some(pending) = pending {
        pending.restart_countdown();
    } else {
        commands.insert_resource(PendingDisplayChange::new(settings));
    }
}

pub(super) fn nearest_resolution(
    requested: DisplayResolution,
    choices: &[DisplayResolution],
) -> DisplayResolution {
    choices
        .iter()
        .copied()
        .min_by_key(|choice| {
            u64::from(choice.width.abs_diff(requested.width)).pow(2)
                + u64::from(choice.height.abs_diff(requested.height)).pow(2)
        })
        .unwrap_or(requested)
}

pub(super) fn handle_display_confirmation(
    buttons: Query<(&Interaction, &DisplayConfirmationAction), Changed<Interaction>>,
    pending: Option<Res<PendingDisplayChange>>,
    mut settings: ResMut<GraphicsSettings>,
    mut commands: Commands,
) {
    let Some(pending) = pending else {
        return;
    };
    for (interaction, action) in buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match action {
            DisplayConfirmationAction::Keep => {
                info!(
                    "Kept display setting: {} at {}",
                    settings.display_mode().label(),
                    settings.display_resolution.label()
                );
            }
            DisplayConfirmationAction::Revert => {
                settings.set_display_mode(pending.previous_mode);
                settings.display_resolution = pending.previous_resolution;
                info!(
                    "Restored display setting: {} at {}",
                    pending.previous_mode.label(),
                    pending.previous_resolution.label()
                );
            }
        }
        commands.remove_resource::<PendingDisplayChange>();
    }
}

pub(super) fn sync_display_confirmation(
    pending: Option<Res<PendingDisplayChange>>,
    mut panels: Query<&mut Node, With<DisplayConfirmationPanel>>,
    mut labels: Query<&mut Text, With<DisplayConfirmationText>>,
) {
    let visible = pending.is_some();
    for mut panel in panels.iter_mut() {
        panel.display = if visible {
            Display::Flex
        } else {
            Display::None
        };
    }
    if let Some(pending) = pending {
        let seconds = pending.seconds_left.ceil().max(0.0) as u32;
        for mut label in labels.iter_mut() {
            label.0 = format!("Keep this display setting? Reverting in {seconds}s");
        }
    }
}

pub(super) fn handle_escape_key(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut input_state: ResMut<InputState>,
    mut pause_open: ResMut<PauseMenuOpen>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut cursor_opts: Query<&mut CursorOptions>,
) {
    if keyboard.just_pressed(KeyCode::Escape) {
        if !pause_open.0 && input_state.ui_blocking() {
            return;
        }
        pause_open.0 = !pause_open.0;
        input_state.pause_menu_open = pause_open.0;
        sync_modal_cursor(pause_open.0, &input_state, &windows, &mut cursor_opts);
    }
}
