//! actions systems.

use super::*;

type DebugMenuControl = Or<(
    With<TimeButton>,
    With<CloseButton>,
    With<CloudCoverButton>,
    With<PerfWeightmapToggleButton>,
    With<PerfRenderDiagToggleButton>,
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
    hud_mode: Res<HudMode>,
) {
    if game_state.get() != &GameState::Playing {
        return;
    }
    // Dev tools require the server-granted god capability and the HUD in god mode.
    if !god.0 || *hud_mode != HudMode::God {
        return;
    }
    if !open.0 && input_state.ui_blocking() {
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
    _input_state: ResMut<InputState>,
    mut cover: ResMut<CloudCover>,
    mut cover_override: ResMut<CloudCoverOverride>,
    mut perf_settings: ResMut<DebugPerfSettings>,
    mut time_sender: Query<
        &mut MessageSender<SetTimeOfDay>,
        (With<crate::GameClient>, With<Connected>),
    >,
    mut buttons: Query<
        (
            &Interaction,
            Option<&TimeButton>,
            Option<&CloseButton>,
            Option<&CloudCoverButton>,
            Option<&PerfWeightmapToggleButton>,
            Option<&PerfRenderDiagToggleButton>,
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
    ) in buttons.iter_mut()
    {
        match *interaction {
            Interaction::Pressed => {
                if close_button.is_some() {
                    open.0 = false;
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
) {
    open.0 = false;
    input_state.debug_menu_open = false;
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
