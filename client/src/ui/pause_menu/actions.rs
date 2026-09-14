//! actions systems.

use super::*;

#[cfg(test)]
mod tests;

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
    state.audio_open = false;
}

pub(super) fn handle_pause_actions(
    buttons: Query<
        (Entity, Ref<Interaction>, &PauseButton),
        Without<bevy::ui::InteractionDisabled>,
    >,
    mut pause_open: ResMut<PauseMenuOpen>,
    mut exit: MessageWriter<AppExit>,
    mut commands: Commands,
    client_query: Query<Entity, With<GameClient>>,
    mut menu_state: ResMut<PauseMenuState>,
    mut input_state: ResMut<InputState>,
    mut next_state: ResMut<NextState<GameState>>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut cursor_opts: Query<&mut CursorOptions>,
    focus: Option<Res<bevy::input_focus::InputFocus>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut sounds: crate::ui::sound::UiActionSounds,
) {
    if input_state.text_input_blocking() {
        return;
    }
    let activate =
        keyboard.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space]);
    for (entity, interaction, action) in buttons.iter() {
        let key = activate
            && focus
                .as_ref()
                .is_some_and(|focus| focus.get() == Some(entity));
        if (interaction.is_changed() && *interaction == Interaction::Pressed) || key {
            if key {
                sounds.emit(crate::audio::sfx::SfxCue::UiClick);
            }
            match action {
                PauseButton::Resume => {
                    pause_open.0 = false;
                    input_state.pause_menu_open = false;
                    sync_modal_cursor(false, &input_state, &windows, &mut cursor_opts);
                }
                PauseButton::Back => {
                    menu_state.graphics_open = false;
                    menu_state.audio_open = false;
                    menu_state.controls_open = false;
                }
                PauseButton::Graphics | PauseButton::Controls | PauseButton::Audio => {
                    menu_state.graphics_open = matches!(action, PauseButton::Graphics);
                    menu_state.controls_open = matches!(action, PauseButton::Controls);
                    menu_state.audio_open = matches!(action, PauseButton::Audio);
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
    buttons: Query<
        (
            &Interaction,
            &GraphicsToggle,
            &widgets::GraphicsToggleChoice,
        ),
        (Changed<Interaction>, Without<bevy::ui::InteractionDisabled>),
    >,
    mut settings: ResMut<GraphicsSettings>,
    mut toggle_buttons: Query<
        (
            &GraphicsToggle,
            &widgets::GraphicsToggleChoice,
            &mut UiButtonStyle,
        ),
        With<Button>,
    >,
) {
    for (interaction, toggle, choice) in buttons.iter() {
        if *interaction == Interaction::Pressed {
            // Apply the explicit choice; clicking selected ON/OFF is idempotent.
            let current = match toggle {
                GraphicsToggle::Bloom => settings.bloom_enabled,
                GraphicsToggle::Shadows => settings.shadows_enabled,
                GraphicsToggle::Atmosphere => settings.atmosphere_enabled,
                GraphicsToggle::Clouds => settings.clouds_enabled,
                GraphicsToggle::Vsync => settings.vsync_enabled,
            };
            if current == choice.0 {
                continue;
            }
            let new_value = match toggle {
                GraphicsToggle::Bloom => {
                    settings.bloom_enabled = choice.0;
                    settings.bloom_enabled
                }
                GraphicsToggle::Shadows => {
                    settings.shadows_enabled = choice.0;
                    settings.shadows_enabled
                }
                GraphicsToggle::Atmosphere => {
                    settings.atmosphere_enabled = choice.0;
                    settings.atmosphere_enabled
                }
                GraphicsToggle::Clouds => {
                    settings.clouds_enabled = choice.0;
                    settings.clouds_enabled
                }
                GraphicsToggle::Vsync => {
                    settings.vsync_enabled = choice.0;
                    settings.vsync_enabled
                }
            };

            info!("Graphics toggle {:?} = {}", toggle, new_value);

            for (btn_toggle, choice, mut style) in toggle_buttons.iter_mut() {
                if std::mem::discriminant(btn_toggle) == std::mem::discriminant(toggle) {
                    style.selected = choice.0 == new_value;
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
    buttons: Query<
        (&Interaction, &DisplayConfirmationAction),
        (Changed<Interaction>, Without<bevy::ui::InteractionDisabled>),
    >,
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
    mut menu: ResMut<PauseMenuState>,
    mut pause_open: ResMut<PauseMenuOpen>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut cursor_opts: Query<&mut CursorOptions>,
) {
    if input_state.text_input_blocking() {
        return;
    }
    if keyboard.just_pressed(KeyCode::Escape) {
        if !pause_open.0 && input_state.ui_blocking() {
            return;
        }
        if pause_open.0 && (menu.graphics_open || menu.controls_open || menu.audio_open) {
            menu.graphics_open = false;
            menu.controls_open = false;
            menu.audio_open = false;
            return;
        }
        pause_open.0 = !pause_open.0;
        input_state.pause_menu_open = pause_open.0;
        sync_modal_cursor(pause_open.0, &input_state, &windows, &mut cursor_opts);
    }
}
