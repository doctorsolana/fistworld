//! Retained display choices and scene-resolution feedback.

use super::*;
use bevy::ui::InteractionDisabled;

const RENDER_SCALES: &[f32] = &[0.25, 0.33, 0.4, 0.5, 0.55, 0.6, 0.66, 0.75, 0.85, 1.0];

#[derive(Component)]
pub(super) struct SelectDisplayMode(pub(super) DisplayMode);

#[derive(Component)]
pub(super) struct DisplayModeHint;

pub(super) fn spawn_display_modes(parent: &mut ChildSpawnerCommands<'_>, selected: DisplayMode) {
    parent.spawn((
        Text::new("DISPLAY MODE"),
        crate::ui::typography::text(14.0),
        TextColor(INK_INVERSE_HEADING),
    ));
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            column_gap: Val::Px(8.0),
            margin: UiRect::vertical(Val::Px(12.0)),
            ..default()
        })
        .with_children(|row| {
            for (label, mode) in [
                ("Windowed", DisplayMode::Windowed),
                ("Borderless", DisplayMode::Borderless),
                ("Exclusive", DisplayMode::ExclusiveFullscreen),
            ] {
                row.spawn((
                    Button,
                    SelectDisplayMode(mode),
                    selected_button_chrome(UiButtonVariant::Inverse, mode == selected),
                    Node {
                        flex_grow: 1.0,
                        height: Val::Px(36.0),
                        padding: UiRect::horizontal(Val::Px(12.0)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(Val::Px(2.0)),
                        border_radius: BorderRadius::all(Val::Px(RADIUS)),
                        ..default()
                    },
                ))
                .with_children(|button| {
                    button.spawn((
                        Text::new(label),
                        UiButtonLabel,
                        crate::ui::typography::text(15.0),
                        TextColor(INK_INVERSE),
                    ));
                });
            }
        });
}

pub(super) fn handle_display_modes(
    buttons: Query<(&Interaction, &SelectDisplayMode), Changed<Interaction>>,
    mut settings: ResMut<GraphicsSettings>,
    monitors: Query<&Monitor, With<PrimaryMonitor>>,
    mut pending: Option<ResMut<PendingDisplayChange>>,
    mut commands: Commands,
) {
    for (interaction, choice) in &buttons {
        if *interaction != Interaction::Pressed || choice.0 == settings.display_mode() {
            continue;
        }
        actions::arm_display_confirmation(&mut commands, pending.as_deref_mut(), &settings);
        settings.set_display_mode(choice.0);
        if choice.0 != DisplayMode::Borderless {
            let choices = available_display_resolutions(
                choice.0,
                monitors.iter().next(),
                settings.display_resolution,
            );
            settings.display_resolution =
                actions::nearest_resolution(settings.display_resolution, &choices);
        }
        info!("Display mode = {}", choice.0.label());
    }
}

fn stepped_scale(current: f32, direction: i32) -> f32 {
    let current = clamped_render_scale(current);
    // Move monotonically even when an environment override or saved file uses
    // a value between presets; choosing a nearby index used to skip steps.
    if direction > 0 {
        RENDER_SCALES
            .iter()
            .copied()
            .find(|scale| *scale > current + 0.001)
            .unwrap_or(current)
    } else {
        RENDER_SCALES
            .iter()
            .rev()
            .copied()
            .find(|scale| *scale < current - 0.001)
            .unwrap_or(current)
    }
}

/// A missing persisted size first adopts a supported choice. Do not treat its
/// nearest neighbour as already selected: that strands a single-mode monitor
/// with both arrows disabled while an invalid request remains in settings.
fn stepped_resolution(
    requested: DisplayResolution,
    choices: &[DisplayResolution],
    delta: i32,
) -> Option<DisplayResolution> {
    if choices.is_empty() {
        return None;
    }
    let Some(index) = choices.iter().position(|choice| *choice == requested) else {
        return Some(actions::nearest_resolution(requested, choices));
    };
    let next = if delta > 0 {
        index.checked_add(1)?
    } else {
        index.checked_sub(1)?
    };
    choices.get(next).copied()
}

pub(super) fn handle_display_steps(
    buttons: Query<
        (&Interaction, &SliderStep),
        (Changed<Interaction>, Without<InteractionDisabled>),
    >,
    mut settings: ResMut<GraphicsSettings>,
    monitors: Query<&Monitor, With<PrimaryMonitor>>,
    mut pending: Option<ResMut<PendingDisplayChange>>,
    mut commands: Commands,
) {
    for (interaction, step) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match step.control {
            SliderControl::Resolution => {
                if settings.display_mode() == DisplayMode::Borderless {
                    continue;
                }
                let choices = available_display_resolutions(
                    settings.display_mode(),
                    monitors.iter().next(),
                    settings.display_resolution,
                );
                if let Some(next) =
                    stepped_resolution(settings.display_resolution, &choices, step.delta)
                {
                    actions::arm_display_confirmation(
                        &mut commands,
                        pending.as_deref_mut(),
                        &settings,
                    );
                    settings.display_resolution = next;
                    info!("Display resolution = {}", next.label());
                }
            }
            SliderControl::RenderScale => {
                let scale = stepped_scale(settings.render_scale, step.delta);
                if scale != settings.render_scale {
                    settings.render_scale = scale;
                    info!("3D render scale = {:.0}%", scale * 100.0);
                }
            }
            _ => {}
        }
    }
}

/// Runs after settings application, window resize and automatic revert. Values
/// and button states come from current resources, never from a previous click.
pub(super) fn sync_display_controls(
    settings: Res<GraphicsSettings>,
    windows: Query<&Window, With<PrimaryWindow>>,
    monitors: Query<Ref<Monitor>, With<PrimaryMonitor>>,
    new_panels: Query<(), Added<GraphicsSettingsPanel>>,
    mut labels: Query<
        (&mut Text, Option<&SliderValueText>, Has<DisplayModeHint>),
        Or<(With<SliderValueText>, With<DisplayModeHint>)>,
    >,
    mut modes: Query<(&SelectDisplayMode, &mut UiButtonStyle)>,
    steps: Query<(Entity, &SliderStep, Has<InteractionDisabled>)>,
    mut commands: Commands,
    mut applied: Local<Option<(DisplayMode, DisplayResolution, u32, UVec2)>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let monitor = monitors.iter().next();
    let key = (
        settings.display_mode(),
        settings.display_resolution,
        settings.render_scale.to_bits(),
        window.physical_size(),
    );
    if *applied == Some(key)
        && new_panels.is_empty()
        && monitor.as_ref().is_none_or(|monitor| !monitor.is_changed())
    {
        return;
    }
    *applied = Some(key);
    let mode = settings.display_mode();
    let choices =
        available_display_resolutions(mode, monitor.as_deref(), settings.display_resolution);
    let scene = scene_render_resolution(window, settings.render_scale);
    for (mut text, control, hint) in &mut labels {
        let value = if hint {
            Some(match mode {
                DisplayMode::Borderless => "Borderless uses the desktop output. Lower 3D Resolution for better performance; menus stay sharp.".to_string(),
                DisplayMode::ExclusiveFullscreen => "Exclusive changes the monitor video mode. Lower 3D Resolution reduces scene detail further. On Mac, Command-Tab and Spaces are unavailable in this mode.".to_string(),
                DisplayMode::Windowed => "Output Resolution sets the window size. Lower 3D Resolution improves performance while keeping menus sharp.".to_string(),
            })
        } else {
            match control.map(|control| control.0) {
                Some(SliderControl::Resolution) => {
                    Some(settings.displayed_resolution_label(monitor.as_deref()))
                }
                Some(SliderControl::RenderScale) => Some(format!(
                    "{} ({:.0}%)",
                    scene.label(),
                    clamped_render_scale(settings.render_scale) * 100.0
                )),
                _ => None,
            }
        };
        if let Some(value) = value {
            if text.0 != value {
                text.0 = value;
            }
        }
    }
    for (choice, mut style) in &mut modes {
        let selected = choice.0 == mode;
        if style.selected != selected {
            style.selected = selected;
        }
    }
    for (entity, step, was_disabled) in &steps {
        let disabled = match step.control {
            SliderControl::Resolution => {
                mode == DisplayMode::Borderless
                    || stepped_resolution(settings.display_resolution, &choices, step.delta)
                        .is_none()
            }
            SliderControl::RenderScale => {
                stepped_scale(settings.render_scale, step.delta)
                    == clamped_render_scale(settings.render_scale)
            }
            _ => continue,
        };
        if disabled && !was_disabled {
            commands.entity(entity).insert(InteractionDisabled);
        } else if !disabled && was_disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arbitrary_saved_scale_steps_in_the_requested_direction() {
        assert_eq!(stepped_scale(0.70, -1), 0.66);
        assert_eq!(stepped_scale(0.70, 1), 0.75);
        assert_eq!(stepped_scale(0.6, -1), 0.55);
        assert_eq!(stepped_scale(0.6, 1), 0.66);
    }

    #[test]
    fn unsupported_saved_size_can_adopt_a_single_mode_monitor() {
        use bevy::window::VideoMode;
        let mut app = App::new();
        let mut settings = GraphicsSettings::default();
        settings.set_display_mode(DisplayMode::ExclusiveFullscreen);
        settings.display_resolution = DisplayResolution::new(2560, 1440);
        let monitor = Monitor {
            name: None,
            physical_width: 1920,
            physical_height: 1080,
            physical_position: IVec2::ZERO,
            scale_factor: 1.0,
            refresh_rate_millihertz: Some(60_000),
            video_modes: vec![VideoMode {
                physical_size: UVec2::new(1920, 1080),
                bit_depth: 24,
                refresh_rate_millihertz: 60_000,
            }],
        };
        assert_eq!(
            settings.displayed_resolution_label(Some(&monitor)),
            "Current 1920 x 1080"
        );
        app.insert_resource(settings);
        app.world_mut().spawn((monitor, PrimaryMonitor));
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        let lower = app
            .world_mut()
            .spawn((
                Interaction::None,
                SliderStep {
                    control: SliderControl::Resolution,
                    delta: -1,
                },
            ))
            .id();
        let higher = app
            .world_mut()
            .spawn((
                Interaction::None,
                SliderStep {
                    control: SliderControl::Resolution,
                    delta: 1,
                },
            ))
            .id();
        app.add_systems(
            Update,
            (handle_display_steps, sync_display_controls).chain(),
        );
        app.update();
        assert!(app.world().get::<InteractionDisabled>(lower).is_none());
        assert!(app.world().get::<InteractionDisabled>(higher).is_none());
        *app.world_mut().get_mut::<Interaction>(higher).unwrap() = Interaction::Pressed;
        app.update();
        assert_eq!(
            app.world()
                .resource::<GraphicsSettings>()
                .display_resolution,
            DisplayResolution::new(1920, 1080)
        );
        assert!(app.world().get::<InteractionDisabled>(lower).is_some());
        assert!(app.world().get::<InteractionDisabled>(higher).is_some());
        assert!(app.world().contains_resource::<PendingDisplayChange>());
    }

    #[test]
    fn exclusive_mode_is_directly_selectable_from_windowed() {
        let mut app = App::new();
        let mut settings = GraphicsSettings::default();
        settings.set_display_mode(DisplayMode::Windowed);
        app.insert_resource(settings);
        app.add_systems(Update, handle_display_modes);
        app.world_mut().spawn((
            Interaction::Pressed,
            SelectDisplayMode(DisplayMode::ExclusiveFullscreen),
        ));
        app.update();
        assert_eq!(
            app.world().resource::<GraphicsSettings>().display_mode(),
            DisplayMode::ExclusiveFullscreen
        );
        assert_eq!(
            app.world().resource::<PendingDisplayChange>().previous_mode,
            DisplayMode::Windowed
        );
    }

    #[test]
    fn borderless_scene_resolution_changes_without_a_video_mode_confirmation() {
        let mut app = App::new();
        let mut settings = GraphicsSettings::default();
        settings.set_display_mode(DisplayMode::Borderless);
        settings.render_scale = 0.6;
        app.insert_resource(settings);
        app.add_systems(Update, handle_display_steps);
        app.world_mut().spawn((
            Interaction::Pressed,
            SliderStep {
                control: SliderControl::RenderScale,
                delta: -1,
            },
        ));
        app.update();
        assert_eq!(
            app.world().resource::<GraphicsSettings>().render_scale,
            0.55
        );
        assert_eq!(
            app.world().resource::<GraphicsSettings>().display_mode(),
            DisplayMode::Borderless
        );
        assert!(!app.world().contains_resource::<PendingDisplayChange>());
    }

    #[test]
    fn display_labels_follow_revert_and_resize_without_another_click() {
        let mut app = App::new();
        let mut settings = GraphicsSettings::default();
        settings.set_display_mode(DisplayMode::Borderless);
        settings.render_scale = 0.5;
        app.insert_resource(settings);
        app.add_systems(Update, sync_display_controls);
        let window = app
            .world_mut()
            .spawn((
                Window {
                    resolution: bevy::window::WindowResolution::new(3000, 2000),
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        let label = app
            .world_mut()
            .spawn((
                Text::new("stale"),
                SliderValueText(SliderControl::RenderScale),
            ))
            .id();
        let mode = app
            .world_mut()
            .spawn((
                SelectDisplayMode(DisplayMode::Windowed),
                UiButtonStyle::default(),
            ))
            .id();
        app.update();
        assert_eq!(
            app.world().get::<Text>(label).unwrap().0,
            "1500 x 1000 (50%)"
        );
        app.world_mut()
            .resource_mut::<GraphicsSettings>()
            .set_display_mode(DisplayMode::Windowed);
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .resolution
            .set_physical_resolution(1600, 900);
        app.update();
        assert_eq!(app.world().get::<Text>(label).unwrap().0, "800 x 450 (50%)");
        assert!(app.world().get::<UiButtonStyle>(mode).unwrap().selected);
    }
}
