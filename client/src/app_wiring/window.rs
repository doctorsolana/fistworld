//! window systems.

use super::*;

pub(super) fn apply_connect_window_settings(
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut settings: ResMut<GraphicsSettings>,
    mut ui_scale: ResMut<UiScale>,
    monitors: Query<&Monitor, With<PrimaryMonitor>>,
) {
    for mut window in windows.iter_mut() {
        window.mode = WindowMode::BorderlessFullscreen(MonitorSelection::Primary);
        if let Some(monitor) = monitors.iter().next() {
            window
                .resolution
                .set_physical_resolution(monitor.physical_width, monitor.physical_height);
        }
        #[cfg(target_os = "macos")]
        {
            let base_scale = window.resolution.base_scale_factor();
            window.resolution.set_scale_factor_override(Some(1.0));
            ui_scale.0 = base_scale;
        }
        #[cfg(not(target_os = "macos"))]
        {
            ui_scale.0 = 1.0;
        }
    }
    settings.fullscreen_enabled = true;
}
