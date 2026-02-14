//! state sync systems.

use super::*;

pub(super) fn sync_debug_menu_open_state(
    open: Res<DebugTimeMenuOpen>,
    mut input_state: ResMut<InputState>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut cursor_opts: Query<&mut CursorOptions>,
) {
    input_state.debug_menu_open = open.0;
    if open.0 {
        sync_modal_cursor(true, &input_state, &windows, &mut cursor_opts);
    } else if open.is_changed() {
        sync_modal_cursor(false, &input_state, &windows, &mut cursor_opts);
    }
}

/// Sync the debug character selection resource with the local player's actual character.
pub(super) fn sync_debug_character_selection(
    mut selection: ResMut<DebugCharacterSelection>,
    local_player: Query<&PlayerCharacter, With<LocalPlayer>>,
) {
    if let Ok(character) = local_player.single() {
        if selection.current != *character {
            selection.current = *character;
        }
    }
}

/// Update the character toggle button label to match the current selection.
pub(super) fn update_character_button_label(
    selection: Res<DebugCharacterSelection>,
    mut labels: Query<&mut Text, With<CharacterLabel>>,
) {
    if !selection.is_changed() {
        return;
    }
    let label_text = match selection.current {
        PlayerCharacter::Oilman => "OILMAN",
        PlayerCharacter::Base => "BASE",
    };
    for mut text in labels.iter_mut() {
        text.0 = label_text.to_string();
    }
}

pub(super) fn update_perf_button_labels(
    settings: Res<DebugPerfSettings>,
    mut labels: ParamSet<(
        Query<&mut Text, (With<PerfWeightmapLabel>, Without<PerfRenderDiagLabel>)>,
        Query<&mut Text, (With<PerfRenderDiagLabel>, Without<PerfWeightmapLabel>)>,
    )>,
) {
    if !settings.is_changed() {
        return;
    }
    let weightmap_text = if settings.weightmap_stats {
        "WEIGHTMAP STATS: ON"
    } else {
        "WEIGHTMAP STATS: OFF"
    };
    for mut text in labels.p0().iter_mut() {
        text.0 = weightmap_text.to_string();
    }

    let render_text = if settings.render_diag_logging {
        "RENDER DIAG LOGGING: ON"
    } else {
        "RENDER DIAG LOGGING: OFF"
    };
    for mut text in labels.p1().iter_mut() {
        text.0 = render_text.to_string();
    }
}
