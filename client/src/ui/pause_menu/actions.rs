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

pub(super) fn button_interactions(
    mut buttons: Query<
        (
            &Interaction,
            &mut BackgroundColor,
            Option<&GraphicsToggle>,
            Option<&SliderStep>,
            Option<&InputSliderStep>,
            Option<&DisplayConfirmationAction>,
        ),
        (Changed<Interaction>, With<Button>),
    >,
    settings: Res<GraphicsSettings>,
) {
    for (interaction, mut bg_color, toggle_opt, slider_opt, input_slider_opt, confirmation_opt) in
        buttons.iter_mut()
    {
        // For toggle buttons, use green/red based on state
        if let Some(toggle) = toggle_opt {
            let enabled = match toggle {
                GraphicsToggle::Bloom => settings.bloom_enabled,
                GraphicsToggle::Ssao => settings.ssao_enabled,
                GraphicsToggle::Shadows => settings.shadows_enabled,
                GraphicsToggle::Atmosphere => settings.atmosphere_enabled,
                GraphicsToggle::Clouds => settings.clouds_enabled,
                GraphicsToggle::FarTerrain => settings.far_terrain_enabled,
                GraphicsToggle::Props => settings.props_enabled,
                GraphicsToggle::Vsync => settings.vsync_enabled,
                GraphicsToggle::FoliageCutout => settings.foliage_cutout_enabled,
            };

            let base_color = if enabled {
                Color::srgb(0.2, 0.55, 0.3)
            } else {
                Color::srgb(0.55, 0.2, 0.2)
            };

            *bg_color = match interaction {
                Interaction::Pressed => BackgroundColor(base_color.lighter(0.15)),
                Interaction::Hovered => BackgroundColor(base_color.lighter(0.08)),
                Interaction::None => BackgroundColor(base_color),
            };
        } else if let Some(action) = confirmation_opt {
            let base_color = match action {
                DisplayConfirmationAction::Keep => Color::srgb(0.2, 0.55, 0.3),
                DisplayConfirmationAction::Revert => Color::srgb(0.55, 0.2, 0.2),
            };
            *bg_color = match interaction {
                Interaction::Pressed => BackgroundColor(base_color.lighter(0.15)),
                Interaction::Hovered => BackgroundColor(base_color.lighter(0.08)),
                Interaction::None => BackgroundColor(base_color),
            };
        } else if slider_opt.is_some() || input_slider_opt.is_some() {
            // Slider step buttons ([-] [+]) for both graphics and input settings
            let base_color = Color::srgb(0.3, 0.3, 0.35);
            *bg_color = match interaction {
                Interaction::Pressed => BackgroundColor(base_color.lighter(0.2)),
                Interaction::Hovered => BackgroundColor(base_color.lighter(0.1)),
                Interaction::None => BackgroundColor(base_color),
            };
        } else {
            // Regular menu buttons
            *bg_color = match interaction {
                Interaction::Pressed => BackgroundColor(BUTTON_PRESSED),
                Interaction::Hovered => BackgroundColor(BUTTON_HOVERED),
                Interaction::None => BackgroundColor(BUTTON_NORMAL),
            };
        }
    }
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
    mut toggle_buttons: Query<(&GraphicsToggle, &mut BackgroundColor), With<Button>>,
) {
    for (interaction, toggle) in buttons.iter() {
        if *interaction == Interaction::Pressed {
            // Toggle the setting
            let new_value = match toggle {
                GraphicsToggle::Bloom => {
                    settings.bloom_enabled = !settings.bloom_enabled;
                    settings.bloom_enabled
                }
                GraphicsToggle::Ssao => {
                    settings.ssao_enabled = !settings.ssao_enabled;
                    settings.ssao_enabled
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
                GraphicsToggle::FarTerrain => {
                    settings.far_terrain_enabled = !settings.far_terrain_enabled;
                    settings.far_terrain_enabled
                }
                GraphicsToggle::Props => {
                    settings.props_enabled = !settings.props_enabled;
                    settings.props_enabled
                }
                GraphicsToggle::Vsync => {
                    settings.vsync_enabled = !settings.vsync_enabled;
                    settings.vsync_enabled
                }
                GraphicsToggle::FoliageCutout => {
                    settings.foliage_cutout_enabled = !settings.foliage_cutout_enabled;
                    settings.foliage_cutout_enabled
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

            // Update the button color
            let new_color = if new_value {
                Color::srgb(0.2, 0.55, 0.3)
            } else {
                Color::srgb(0.55, 0.2, 0.2)
            };

            for (btn_toggle, mut bg_color) in toggle_buttons.iter_mut() {
                if std::mem::discriminant(btn_toggle) == std::mem::discriminant(toggle) {
                    *bg_color = BackgroundColor(new_color);
                }
            }
        }
    }
}

pub(super) fn handle_slider_steps(
    buttons: Query<(&Interaction, &SliderStep), Changed<Interaction>>,
    mut settings: ResMut<GraphicsSettings>,
    mut slider_texts: Query<(&SliderValueText, &mut Text)>,
    monitors: Query<&Monitor, With<PrimaryMonitor>>,
    mut pending_display: Option<ResMut<PendingDisplayChange>>,
    mut commands: Commands,
) {
    let monitor = monitors.iter().next();
    for (interaction, step) in buttons.iter() {
        if *interaction == Interaction::Pressed {
            match step.control {
                SliderControl::DisplayMode => {
                    let current = settings.display_mode();
                    let new_mode = if step.delta > 0 {
                        current.next()
                    } else {
                        current.prev()
                    };
                    if new_mode != current {
                        arm_display_confirmation(
                            &mut commands,
                            pending_display.as_deref_mut(),
                            &settings,
                        );
                        settings.set_display_mode(new_mode);
                        if new_mode != DisplayMode::Borderless {
                            let choices = available_display_resolutions(
                                new_mode,
                                monitor,
                                settings.display_resolution,
                            );
                            settings.display_resolution =
                                nearest_resolution(settings.display_resolution, &choices);
                        }
                        info!("Display mode = {}", new_mode.label());
                        for (text_control, mut text) in slider_texts.iter_mut() {
                            match text_control.0 {
                                SliderControl::DisplayMode => {
                                    text.0 = new_mode.label().to_string();
                                }
                                SliderControl::Resolution => {
                                    text.0 = settings.displayed_resolution_label(monitor);
                                }
                                _ => {}
                            }
                        }
                    }
                }
                SliderControl::Resolution => {
                    let display_mode = settings.display_mode();
                    if display_mode == DisplayMode::Borderless {
                        info!(
                            "Borderless fullscreen uses the monitor's native resolution; select Windowed or Fullscreen to change it"
                        );
                        continue;
                    }
                    let choices = available_display_resolutions(
                        display_mode,
                        monitor,
                        settings.display_resolution,
                    );
                    let current = nearest_resolution(settings.display_resolution, &choices);
                    let current_idx = choices
                        .iter()
                        .position(|resolution| *resolution == current)
                        .unwrap_or(0);
                    let new_idx = if step.delta > 0 {
                        (current_idx + 1).min(choices.len() - 1)
                    } else {
                        current_idx.saturating_sub(1)
                    };
                    let new_resolution = choices[new_idx];
                    if new_resolution != settings.display_resolution {
                        arm_display_confirmation(
                            &mut commands,
                            pending_display.as_deref_mut(),
                            &settings,
                        );
                        settings.display_resolution = new_resolution;
                        info!("Display resolution = {}", new_resolution.label());
                        for (text_control, mut text) in slider_texts.iter_mut() {
                            if matches!(text_control.0, SliderControl::Resolution) {
                                text.0 = new_resolution.label();
                            }
                        }
                    }
                }
                SliderControl::RenderScale => {
                    // 3D resolution scale: GPU cost scales with the square.
                    let steps = [0.5, 0.58, 0.66, 0.75, 0.85, 1.0];
                    let current_idx = steps
                        .iter()
                        .position(|&x| (x - settings.render_scale).abs() < 0.03)
                        .unwrap_or(3); // Default to 0.75 if not found

                    let new_idx = if step.delta > 0 {
                        (current_idx + 1).min(steps.len() - 1)
                    } else {
                        current_idx.saturating_sub(1)
                    };

                    let new_val = steps[new_idx];
                    if (new_val - settings.render_scale).abs() > 0.01 {
                        settings.render_scale = new_val;
                        info!("Render scale = {:.0}%", new_val * 100.0);

                        for (text_control, mut text) in slider_texts.iter_mut() {
                            if matches!(text_control.0, SliderControl::RenderScale) {
                                text.0 = format!("{:.0}%", new_val * 100.0);
                            }
                        }
                    }
                }
                SliderControl::ShadowQuality => {
                    let new_val = if step.delta > 0 {
                        settings.shadow_quality.next()
                    } else {
                        settings.shadow_quality.prev()
                    };
                    if new_val != settings.shadow_quality {
                        settings.shadow_quality = new_val;
                        info!("Shadow quality = {:?}", new_val);

                        for (text_control, mut text) in slider_texts.iter_mut() {
                            if matches!(text_control.0, SliderControl::ShadowQuality) {
                                text.0 = new_val.label().to_string();
                            }
                        }
                    }
                }
                SliderControl::GroundCoverRenderer => {
                    let new_val = if step.delta > 0 {
                        settings.ground_cover_renderer.next()
                    } else {
                        settings.ground_cover_renderer.prev()
                    };
                    if new_val != settings.ground_cover_renderer {
                        settings.ground_cover_renderer = new_val;
                        info!("3D grass renderer = {}", new_val.label());
                        for (text_control, mut text) in slider_texts.iter_mut() {
                            if matches!(text_control.0, SliderControl::GroundCoverRenderer) {
                                text.0 = new_val.label().to_string();
                            }
                        }
                    }
                }
                SliderControl::Tonemapping => {
                    let options = [
                        Tonemapping::AgX,
                        Tonemapping::AcesFitted,
                        Tonemapping::BlenderFilmic,
                    ];
                    let current_idx = options
                        .iter()
                        .position(|&m| m == settings.tonemapping)
                        .unwrap_or(0);
                    let new_idx = if step.delta > 0 {
                        (current_idx + 1) % options.len()
                    } else {
                        (current_idx + options.len() - 1) % options.len()
                    };
                    let new_val = options[new_idx];
                    if new_val != settings.tonemapping {
                        settings.tonemapping = new_val;
                        info!("Tone mapping = {:?}", new_val);

                        for (text_control, mut text) in slider_texts.iter_mut() {
                            if matches!(text_control.0, SliderControl::Tonemapping) {
                                text.0 = tonemapping_label(new_val).to_string();
                            }
                        }
                    }
                }
                SliderControl::Exposure => {
                    // Color grading exposure: -1.0..2.0 in 0.1 steps
                    let steps: Vec<f32> = (-10..=20).map(|i| i as f32 * 0.1).collect();
                    let current_idx = steps
                        .iter()
                        .position(|&x| (x - settings.grade_exposure).abs() < 0.05)
                        .unwrap_or(12); // Fall back near the 0.2 default

                    let new_idx = if step.delta > 0 {
                        (current_idx + 1).min(steps.len() - 1)
                    } else {
                        current_idx.saturating_sub(1)
                    };

                    let new_val = steps[new_idx];
                    if (new_val - settings.grade_exposure).abs() > 0.01 {
                        settings.grade_exposure = new_val;
                        info!("Grade exposure = {:+.1} EV", new_val);

                        for (text_control, mut text) in slider_texts.iter_mut() {
                            if matches!(text_control.0, SliderControl::Exposure) {
                                text.0 = format!("{:+.1} EV", new_val);
                            }
                        }
                    }
                }
                SliderControl::ViewDistance => {
                    // View distance: 2-16 chunks (128m - 1024m)
                    let new_val = (settings.view_distance + step.delta).clamp(2, 16);
                    if new_val != settings.view_distance {
                        settings.view_distance = new_val;
                        info!("View distance = {} chunks ({}m)", new_val, new_val * 64);

                        // Update text
                        for (text_control, mut text) in slider_texts.iter_mut() {
                            if matches!(text_control.0, SliderControl::ViewDistance) {
                                text.0 = format!("{} chunks", new_val);
                            }
                        }
                    }
                }
                SliderControl::PropDistance => {
                    // Prop render distance: 25%-400% in 25% steps
                    let steps = [
                        0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 2.25, 2.5, 2.75, 3.0, 3.25,
                        3.5, 3.75, 4.0,
                    ];
                    let current_idx = steps
                        .iter()
                        .position(|&x| (x - settings.prop_render_multiplier).abs() < 0.01)
                        .unwrap_or(3); // Default to 1.0 if not found

                    let new_idx = if step.delta > 0 {
                        (current_idx + 1).min(steps.len() - 1)
                    } else {
                        current_idx.saturating_sub(1)
                    };

                    let new_val = steps[new_idx];
                    if (new_val - settings.prop_render_multiplier).abs() > 0.01 {
                        settings.prop_render_multiplier = new_val;
                        info!("Prop render multiplier = {:.0}%", new_val * 100.0);

                        // Update text
                        for (text_control, mut text) in slider_texts.iter_mut() {
                            if matches!(text_control.0, SliderControl::PropDistance) {
                                text.0 = format!("{:.0}%", new_val * 100.0);
                            }
                        }
                    }
                }
                SliderControl::LightingBoost => {
                    // Lighting boost: 50%-400% in 10% steps
                    let steps: Vec<f32> = (5..=40).map(|i| i as f32 * 0.1).collect();
                    let current_idx = steps
                        .iter()
                        .position(|&x| (x - settings.lighting_boost).abs() < 0.05)
                        .unwrap_or(5); // Default to 1.0 (index 5) if not found

                    let new_idx = if step.delta > 0 {
                        (current_idx + 1).min(steps.len() - 1)
                    } else {
                        current_idx.saturating_sub(1)
                    };

                    let new_val = steps[new_idx];
                    if (new_val - settings.lighting_boost).abs() > 0.01 {
                        settings.lighting_boost = new_val;
                        info!("Lighting boost = {:.0}%", new_val * 100.0);

                        // Update text
                        for (text_control, mut text) in slider_texts.iter_mut() {
                            if matches!(text_control.0, SliderControl::LightingBoost) {
                                text.0 = format!("{:.0}%", new_val * 100.0);
                            }
                        }
                    }
                }
            }
        }
    }
}

fn arm_display_confirmation(
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

fn nearest_resolution(
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

pub(super) fn handle_input_slider_steps(
    buttons: Query<(&Interaction, &InputSliderStep), Changed<Interaction>>,
    mut settings: ResMut<InputSettings>,
    mut slider_texts: Query<(&InputSliderValueText, &mut Text)>,
) {
    for (interaction, step) in buttons.iter() {
        if *interaction == Interaction::Pressed {
            match step.control {
                InputSliderControl::MouseSensitivity => {
                    // Mouse sensitivity: 10%-300% in 10% steps
                    let steps: Vec<f32> = (1..=30).map(|i| i as f32 * 0.1).collect();
                    let current_idx = steps
                        .iter()
                        .position(|&x| (x - settings.mouse_sensitivity).abs() < 0.05)
                        .unwrap_or(9); // Default to 1.0 (index 9) if not found

                    let new_idx = if step.delta > 0 {
                        (current_idx + 1).min(steps.len() - 1)
                    } else {
                        current_idx.saturating_sub(1)
                    };

                    let new_val = steps[new_idx];
                    if (new_val - settings.mouse_sensitivity).abs() > 0.01 {
                        settings.mouse_sensitivity = new_val;
                        info!("Mouse sensitivity = {:.0}%", new_val * 100.0);

                        // Update text
                        for (text_control, mut text) in slider_texts.iter_mut() {
                            if matches!(text_control.0, InputSliderControl::MouseSensitivity) {
                                text.0 = format!("{:.0}%", new_val * 100.0);
                            }
                        }
                    }
                }
            }
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
        if input_state.inventory_open || input_state.map_open || input_state.debug_menu_open {
            return;
        }
        pause_open.0 = !pause_open.0;
        input_state.pause_menu_open = pause_open.0;
        sync_modal_cursor(pause_open.0, &input_state, &windows, &mut cursor_opts);
    }
}
