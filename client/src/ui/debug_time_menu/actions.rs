//! actions systems.

use super::*;

type DebugMenuControl = Or<(
    With<TimeButton>,
    With<CloseButton>,
    With<CloudCoverButton>,
    With<PerfWeightmapToggleButton>,
    With<PerfRenderDiagToggleButton>,
    With<GodAccessSubmitButton>,
)>;

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
    god: Res<GodCapability>,
    mut hud_mode: ResMut<HudMode>,
    mut access: ResMut<GodAccessInput>,
) {
    if game_state.get() != &GameState::Playing {
        return;
    }
    if !open.0 && input_state.ui_blocking() {
        return;
    }
    if !keyboard.just_pressed(KeyCode::KeyJ) {
        return;
    }
    if !god.0 {
        // Once open, J is ordinary key input and must not close the challenge.
        if !open.0 {
            access.key.clear();
            access.feedback.clear();
            access.submitted = false;
            access.skip_text_frame = true;
            open.0 = true;
        }
        return;
    }
    if *hud_mode != HudMode::God {
        *hud_mode = HudMode::God;
        open.0 = true;
    } else {
        open.0 = !open.0;
    }
}

pub(super) fn handle_god_access_input(
    mut access: ResMut<GodAccessInput>,
    god: Res<GodCapability>,
    mut key_events: MessageReader<KeyboardInput>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut displays: Query<&mut Text, With<GodAccessInputDisplay>>,
    mut feedback: Query<&mut Text, (With<GodAccessFeedbackText>, Without<GodAccessInputDisplay>)>,
    mut senders: Query<
        &mut MessageSender<RequestGodAccess>,
        (With<crate::GameClient>, With<Connected>),
    >,
) {
    if god.0 {
        return;
    }
    if access.skip_text_frame {
        access.skip_text_frame = false;
        key_events.clear();
    } else if !access.submitted {
        if keyboard.just_pressed(KeyCode::Backspace) {
            access.key.pop();
        }
        for event in key_events.read() {
            if !event.state.is_pressed() {
                continue;
            }
            if let Key::Character(text) = &event.logical_key {
                for character in text.chars().filter(|character| !character.is_control()) {
                    if access.key.len() < 96 {
                        access.key.push(character);
                    }
                }
            }
        }
    }

    let enter =
        keyboard.just_pressed(KeyCode::Enter) || keyboard.just_pressed(KeyCode::NumpadEnter);
    if enter && !access.submitted && !access.key.trim().is_empty() {
        submit_god_access(&mut access, &mut senders);
    }

    for mut text in displays.iter_mut() {
        text.0 = if access.key.is_empty() {
            "_".to_string()
        } else {
            format!("{} _", "•".repeat(access.key.chars().count()))
        };
    }
    for mut text in feedback.iter_mut() {
        text.0.clone_from(&access.feedback);
    }
}

fn submit_god_access(
    access: &mut GodAccessInput,
    senders: &mut Query<
        &mut MessageSender<RequestGodAccess>,
        (With<crate::GameClient>, With<Connected>),
    >,
) {
    let key = access.key.trim().to_string();
    if key.is_empty() {
        return;
    }
    if let Ok(mut sender) = senders.single_mut() {
        sender.send::<ReliableChannel>(RequestGodAccess { key });
        access.submitted = true;
        access.feedback = "Checking with server…".to_string();
    } else {
        access.feedback = "No active server connection".to_string();
    }
}

pub(super) fn receive_god_access_result(
    mut commands: Commands,
    mut receivers: Query<&mut MessageReceiver<GodAccessResult>, With<crate::GameClient>>,
    mut access: ResMut<GodAccessInput>,
    mut capability: ResMut<GodCapability>,
    mut mode: ResMut<HudMode>,
    roots: Query<Entity, With<DebugMenuRoot>>,
) {
    for mut receiver in receivers.iter_mut() {
        for result in receiver.receive() {
            access.submitted = false;
            access.feedback = result.message;
            if result.granted {
                access.key.clear();
                capability.0 = true;
                *mode = HudMode::God;
                // Rebuild the open modal as the full debug menu immediately.
                for root in roots.iter() {
                    commands.entity(root).despawn();
                }
            }
        }
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
    _input_state: ResMut<InputState>,
    mut cover: ResMut<CloudCover>,
    mut cover_override: ResMut<CloudCoverOverride>,
    mut perf_settings: ResMut<DebugPerfSettings>,
    mut time_sender: Query<
        &mut MessageSender<SetTimeOfDay>,
        (With<crate::GameClient>, With<Connected>),
    >,
    mut god_sender: Query<
        &mut MessageSender<RequestGodAccess>,
        (With<crate::GameClient>, With<Connected>),
    >,
    mut access: ResMut<GodAccessInput>,
    mut buttons: Query<
        (
            &Interaction,
            Option<&TimeButton>,
            Option<&CloseButton>,
            Option<&CloudCoverButton>,
            Option<&PerfWeightmapToggleButton>,
            Option<&PerfRenderDiagToggleButton>,
            Option<&GodAccessSubmitButton>,
        ),
        (Changed<Interaction>, DebugMenuControl),
    >,
) {
    for (
        interaction,
        time_button,
        close_button,
        cover_button,
        weightmap_button,
        render_diag_button,
        god_access_button,
    ) in buttons.iter_mut()
    {
        match *interaction {
            Interaction::Pressed => {
                if close_button.is_some() {
                    open.0 = false;
                    continue;
                }

                if god_access_button.is_some() {
                    if !access.submitted {
                        submit_god_access(&mut access, &mut god_sender);
                    }
                    continue;
                }

                if let Some(CloudCoverButton(mode)) = cover_button {
                    cover_override.mode = *mode;
                    if *mode == CloudCoverMode::Auto {
                        // Resume natural weather from the CURRENT sky — a
                        // snap back to the default cover would pop.
                        cover.segment = -1;
                    } else {
                        *cover = crate::render::systems::CloudCover::snapped(*mode);
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

                if let Some(TimeButton(preset)) = time_button {
                    if let Ok(mut sender) = time_sender.single_mut() {
                        sender.send::<ReliableChannel>(SetTimeOfDay { preset: *preset });
                    }
                }
            }
            Interaction::Hovered | Interaction::None => {}
        }
    }
}

pub(super) fn close_debug_menu_on_main_menu(
    mut open: ResMut<DebugTimeMenuOpen>,
    mut input_state: ResMut<InputState>,
    mut access: ResMut<GodAccessInput>,
) {
    open.0 = false;
    input_state.debug_menu_open = false;
    *access = GodAccessInput::default();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_button_styling_cannot_capture_the_modal_backdrop() {
        let mut world = World::new();
        let backdrop = world
            .spawn((
                DebugMenuBackdrop,
                Interaction::Hovered,
                BackgroundColor::default(),
            ))
            .id();
        let control = world
            .spawn((
                CloseButton,
                Interaction::Hovered,
                BackgroundColor::default(),
            ))
            .id();

        let mut controls = world.query_filtered::<Entity, DebugMenuControl>();
        let matches: Vec<_> = controls.iter(&world).collect();
        assert_eq!(matches, vec![control]);
        assert!(!matches.contains(&backdrop));
    }
}
