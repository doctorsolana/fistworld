//! window systems.

use super::*;

/// Apply a window mode from the fullscreen preference. Shared by the
/// on-connect transition and the pause-menu toggle so both paths size the
/// resolution and (on macOS) the UI scale identically.
pub(crate) fn apply_window_mode(
    window: &mut Window,
    ui_scale: &mut UiScale,
    monitor: Option<&Monitor>,
    fullscreen: bool,
) {
    if fullscreen {
        window.mode = WindowMode::BorderlessFullscreen(MonitorSelection::Primary);
        if let Some(monitor) = monitor {
            window
                .resolution
                .set_physical_resolution(monitor.physical_width, monitor.physical_height);
        }
    } else {
        window.mode = WindowMode::Windowed;
        // The resolution still holds the monitor's physical size after
        // fullscreen; bevy won't shrink it on its own.
        window.resolution.set_physical_resolution(
            crate::render::systems::LAUNCHER_RESOLUTION.0,
            crate::render::systems::LAUNCHER_RESOLUTION.1,
        );
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

/// Entering the game applies the player's fullscreen preference (persisted in
/// GraphicsSettings) instead of forcing fullscreen unconditionally.
pub(super) fn apply_connect_window_settings(
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    settings: Res<GraphicsSettings>,
    mut ui_scale: ResMut<UiScale>,
    monitors: Query<&Monitor, With<PrimaryMonitor>>,
) {
    let monitor = monitors.iter().next();
    for mut window in windows.iter_mut() {
        apply_window_mode(
            &mut window,
            &mut ui_scale,
            monitor,
            settings.fullscreen_enabled,
        );
    }
}
