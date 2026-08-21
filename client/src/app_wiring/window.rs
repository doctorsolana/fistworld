//! window systems.

use super::*;
use crate::render::systems::{
    available_display_resolutions, best_fullscreen_video_mode, DisplayMode, DisplayResolution,
};
use bevy::window::VideoModeSelection;

/// Apply one complete display request. Shared by the on-connect transition and
/// live graphics changes so mode, output resolution and macOS UI scale cannot
/// drift apart.
///
/// Borderless fullscreen is native-resolution by definition. A lower physical
/// fullscreen resolution therefore uses Bevy's exclusive Fullscreen mode with
/// an exact OS-reported VideoMode; unsupported requests safely fall back to the
/// monitor's current mode rather than handing `bevy_winit` an invalid mode.
pub(crate) fn apply_window_mode(
    window: &mut Window,
    ui_scale: &mut UiScale,
    monitor: Option<&Monitor>,
    display_mode: DisplayMode,
    resolution: DisplayResolution,
) {
    let output_resolution = match display_mode {
        DisplayMode::Windowed => {
            if window.mode != WindowMode::Windowed {
                window.mode = WindowMode::Windowed;
            }
            if window.resolution.physical_width() != resolution.width
                || window.resolution.physical_height() != resolution.height
            {
                window
                    .resolution
                    .set_physical_resolution(resolution.width, resolution.height);
            }
            resolution
        }
        DisplayMode::Borderless => {
            let desired = WindowMode::BorderlessFullscreen(MonitorSelection::Primary);
            if window.mode != desired {
                window.mode = desired;
            }
            // Winit owns the size of a borderless window. Do not submit a
            // competing resize request; the backend will synchronize the
            // Window component to the monitor's current physical size.
            monitor.map_or(resolution, |monitor| {
                DisplayResolution::new(monitor.physical_width, monitor.physical_height)
            })
        }
        DisplayMode::ExclusiveFullscreen => {
            let exact = monitor.and_then(|monitor| best_fullscreen_video_mode(monitor, resolution));
            let output_resolution = exact.map_or_else(
                || {
                    let supported = available_display_resolutions(
                        DisplayMode::ExclusiveFullscreen,
                        monitor,
                        resolution,
                    )
                    .into_iter()
                    .map(DisplayResolution::label)
                    .collect::<Vec<_>>()
                    .join(", ");
                    warn!(
                        "Requested fullscreen resolution {} is unavailable; using the monitor's current video mode. Supported fullscreen resolutions: {}",
                        resolution.label(),
                        supported
                    );
                    monitor.map_or(resolution, |monitor| {
                        DisplayResolution::new(
                            monitor.physical_width,
                            monitor.physical_height,
                        )
                    })
                },
                |mode| {
                    DisplayResolution::new(mode.physical_size.x, mode.physical_size.y)
                },
            );
            let selected = exact
                .map(VideoModeSelection::Specific)
                .unwrap_or(VideoModeSelection::Current);
            let desired = WindowMode::Fullscreen(MonitorSelection::Primary, selected);
            if window.mode != desired {
                window.mode = desired;
            }
            // As with borderless, the fullscreen video mode owns the window's
            // resulting size and emits the authoritative resize event.
            output_resolution
        }
    };

    apply_resolution_aware_ui_scale(window, ui_scale, output_resolution);
}

/// Keep the front-end usable at every supported output resolution.
///
/// The UI was authored around a 1600x900 physical canvas. Scaling by the
/// smaller output axis preserves that composition at 720p, native Retina and
/// ultrawide resolutions alike. Bevy normally folds OS DPI into logical window
/// coordinates; dividing by that scale on non-macOS yields the same final
/// physical UI size. macOS retains the established scale-factor override so
/// cursor, render-target and screenshot coordinates stay in physical pixels.
fn apply_resolution_aware_ui_scale(
    window: &mut Window,
    ui_scale: &mut UiScale,
    output: DisplayResolution,
) {
    let physical_ui_scale = (output.width as f32 / LAUNCHER_RESOLUTION.0 as f32)
        .min(output.height as f32 / LAUNCHER_RESOLUTION.1 as f32)
        .clamp(0.64, 3.0);
    #[cfg(target_os = "macos")]
    {
        window.resolution.set_scale_factor_override(Some(1.0));
        ui_scale.0 = physical_ui_scale;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let base_scale = window.resolution.base_scale_factor().max(f32::EPSILON);
        ui_scale.0 = physical_ui_scale / base_scale;
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
            settings.display_mode(),
            settings.display_resolution,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::window::VideoMode;

    fn monitor_with_modes() -> Monitor {
        Monitor {
            name: Some("Test Display".into()),
            physical_height: 1440,
            physical_width: 2560,
            physical_position: IVec2::ZERO,
            refresh_rate_millihertz: Some(60_000),
            scale_factor: 2.0,
            video_modes: vec![
                VideoMode {
                    physical_size: UVec2::new(1920, 1080),
                    bit_depth: 24,
                    refresh_rate_millihertz: 60_000,
                },
                VideoMode {
                    physical_size: UVec2::new(1920, 1080),
                    bit_depth: 30,
                    refresh_rate_millihertz: 120_000,
                },
            ],
        }
    }

    #[test]
    fn exclusive_fullscreen_uses_an_exact_supported_video_mode() {
        let mut window = Window::default();
        let mut ui_scale = UiScale::default();
        apply_window_mode(
            &mut window,
            &mut ui_scale,
            Some(&monitor_with_modes()),
            DisplayMode::ExclusiveFullscreen,
            DisplayResolution::new(1920, 1080),
        );

        let WindowMode::Fullscreen(_, VideoModeSelection::Specific(mode)) = window.mode else {
            panic!("expected an exact exclusive-fullscreen video mode");
        };
        assert_eq!(mode.physical_size, UVec2::new(1920, 1080));
        assert_eq!(mode.refresh_rate_millihertz, 120_000);
        assert_eq!(mode.bit_depth, 30);
    }

    #[test]
    fn unsupported_exclusive_resolution_falls_back_without_an_invalid_mode() {
        let mut window = Window::default();
        let mut ui_scale = UiScale::default();
        apply_window_mode(
            &mut window,
            &mut ui_scale,
            Some(&monitor_with_modes()),
            DisplayMode::ExclusiveFullscreen,
            DisplayResolution::new(1600, 900),
        );

        assert!(matches!(
            window.mode,
            WindowMode::Fullscreen(_, VideoModeSelection::Current)
        ));
    }

    #[test]
    fn lower_output_resolution_scales_the_ui_to_remain_usable() {
        let mut window = Window::default();
        let mut ui_scale = UiScale::default();
        apply_window_mode(
            &mut window,
            &mut ui_scale,
            Some(&monitor_with_modes()),
            DisplayMode::Windowed,
            DisplayResolution::new(1280, 720),
        );

        #[cfg(target_os = "macos")]
        assert!((ui_scale.0 - 0.8).abs() < f32::EPSILON);
        #[cfg(not(target_os = "macos"))]
        assert!((ui_scale.0 * window.resolution.base_scale_factor() - 0.8).abs() < 0.001);
    }
}
