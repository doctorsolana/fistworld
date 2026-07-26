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
