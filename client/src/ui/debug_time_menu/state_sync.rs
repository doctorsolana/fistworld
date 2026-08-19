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

pub(super) fn update_perf_button_labels(
    settings: Res<DebugPerfSettings>,
    mut labels: ParamSet<(
        Query<&mut Text, (With<PerfWeightmapLabel>, Without<PerfRenderDiagLabel>)>,
        Query<&mut Text, (With<PerfRenderDiagLabel>, Without<PerfWeightmapLabel>)>,
    )>,
    mut weight_buttons: Query<
        &mut UiButtonStyle,
        (
            With<PerfWeightmapToggleButton>,
            Without<PerfRenderDiagToggleButton>,
        ),
    >,
    mut render_buttons: Query<
        &mut UiButtonStyle,
        (
            With<PerfRenderDiagToggleButton>,
            Without<PerfWeightmapToggleButton>,
        ),
    >,
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
    for mut style in weight_buttons.iter_mut() {
        style.selected = settings.weightmap_stats;
    }

    let render_text = if settings.render_diag_logging {
        "RENDER DIAG LOGGING: ON"
    } else {
        "RENDER DIAG LOGGING: OFF"
    };
    for mut text in labels.p1().iter_mut() {
        text.0 = render_text.to_string();
    }
    for mut style in render_buttons.iter_mut() {
        style.selected = settings.render_diag_logging;
    }
}

pub(super) fn update_cloud_button_styles(
    cloud_override: Res<CloudCoverOverride>,
    mut buttons: Query<(&CloudCoverButton, &mut UiButtonStyle)>,
) {
    if !cloud_override.is_changed() {
        return;
    }
    for (button, mut style) in buttons.iter_mut() {
        style.selected = button.0 == cloud_override.mode;
    }
}
