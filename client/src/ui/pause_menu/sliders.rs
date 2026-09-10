//! Nondisplay slider values, actions and retained endpoint state.

use super::*;
use crate::render::systems::settings::ShadowQuality;
use bevy::ui::InteractionDisabled;

#[derive(Clone, Copy, PartialEq)]
enum Value {
    Shadows(ShadowQuality),
    Exposure(f32),
    Terrain(i32),
    Scenery(f32),
}

/// Move to the next preset even when a saved value lies between two presets.
fn stepped(value: f32, delta: i32, increment: f32, min: f32, max: f32) -> f32 {
    if delta == 0 {
        return value;
    }
    let units = value / increment;
    let next = if delta > 0 {
        (units + 0.001).floor() + 1.0
    } else {
        (units - 0.001).ceil() - 1.0
    };
    (next * increment).clamp(min, max)
}

fn value(settings: &GraphicsSettings, control: SliderControl, delta: i32) -> Option<Value> {
    Some(match control {
        SliderControl::ShadowQuality => Value::Shadows(if delta > 0 {
            settings.shadow_quality.next()
        } else if delta < 0 {
            settings.shadow_quality.prev()
        } else {
            settings.shadow_quality
        }),
        SliderControl::Exposure => {
            Value::Exposure(stepped(settings.grade_exposure, delta, 0.1, -1.0, 2.0))
        }
        SliderControl::ViewDistance => Value::Terrain(if delta == 0 {
            settings.view_distance
        } else {
            settings
                .view_distance
                .saturating_add(delta.signum())
                .clamp(2, 16)
        }),
        SliderControl::PropDistance => Value::Scenery(stepped(
            settings.prop_render_multiplier,
            delta,
            0.25,
            0.25,
            4.0,
        )),
        _ => return None,
    })
}

pub(super) fn graphics_label(settings: &GraphicsSettings, control: SliderControl) -> String {
    match value(settings, control, 0) {
        Some(Value::Shadows(value)) => value.label().into(),
        Some(Value::Exposure(value)) => format!("{value:+.1} EV"),
        Some(Value::Terrain(value)) => {
            format!("{:.0} m", value as f32 * shared::terrain::CHUNK_SIZE)
        }
        Some(Value::Scenery(value)) => format!("{:.0}%", value * 100.0),
        None => String::new(),
    }
}

pub(super) fn handle_slider_steps(
    buttons: Query<
        (&Interaction, &SliderStep),
        (Changed<Interaction>, Without<InteractionDisabled>),
    >,
    mut settings: ResMut<GraphicsSettings>,
) {
    for (interaction, step) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let next = value(&settings, step.control, step.delta);
        if next == value(&settings, step.control, 0) {
            continue;
        }
        match next {
            Some(Value::Shadows(value)) => settings.shadow_quality = value,
            Some(Value::Exposure(value)) => settings.grade_exposure = value,
            Some(Value::Terrain(value)) => settings.view_distance = value,
            Some(Value::Scenery(value)) => settings.prop_render_multiplier = value,
            None => continue,
        }
    }
}

pub(super) fn handle_input_slider_steps(
    buttons: Query<
        (&Interaction, &InputSliderStep),
        (Changed<Interaction>, Without<InteractionDisabled>),
    >,
    mut settings: ResMut<InputSettings>,
) {
    for (interaction, step) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match step.control {
            InputSliderControl::MouseSensitivity => {
                let next = stepped(settings.mouse_sensitivity, step.delta, 0.1, 0.1, 3.0);
                if next != settings.mouse_sensitivity {
                    settings.mouse_sensitivity = next;
                }
            }
        }
    }
}

pub(super) fn sync_slider_controls(
    settings: Res<GraphicsSettings>,
    input: Res<InputSettings>,
    new_graphics: Query<(), Added<GraphicsSettingsPanel>>,
    new_controls: Query<(), Added<ControlsSettingsPanel>>,
    mut labels: Query<
        (
            &mut Text,
            Option<&SliderValueText>,
            Option<&InputSliderValueText>,
        ),
        Or<(With<SliderValueText>, With<InputSliderValueText>)>,
    >,
    buttons: Query<
        (
            Entity,
            Option<&SliderStep>,
            Option<&InputSliderStep>,
            Has<InteractionDisabled>,
        ),
        Or<(With<SliderStep>, With<InputSliderStep>)>,
    >,
    mut commands: Commands,
) {
    if !settings.is_changed()
        && !input.is_changed()
        && new_graphics.is_empty()
        && new_controls.is_empty()
    {
        return;
    }
    for (mut text, graphics, controls) in &mut labels {
        let label = if let Some(control) = graphics {
            if value(&settings, control.0, 0).is_none() {
                continue;
            }
            graphics_label(&settings, control.0)
        } else if controls.is_some() {
            format!("{:.0}%", input.mouse_sensitivity * 100.0)
        } else {
            continue;
        };
        if text.0 != label {
            text.0 = label;
        }
    }
    for (entity, graphics, controls, disabled) in &buttons {
        let at_limit = if let Some(step) = graphics {
            let Some(current) = value(&settings, step.control, 0) else {
                continue;
            };
            value(&settings, step.control, step.delta) == Some(current)
        } else if let Some(step) = controls {
            stepped(input.mouse_sensitivity, step.delta, 0.1, 0.1, 3.0) == input.mouse_sensitivity
        } else {
            continue;
        };
        if at_limit && !disabled {
            commands.entity(entity).insert(InteractionDisabled);
        } else if !at_limit && disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slider_endpoints_disable_reenable_and_reject_disabled_actions() {
        let mut app = App::new();
        let mut settings = GraphicsSettings::default();
        settings.grade_exposure = -1.0;
        settings.view_distance = 16;
        settings.prop_render_multiplier = 0.25;
        settings.shadow_quality = ShadowQuality::Low;
        app.insert_resource(settings)
            .insert_resource(InputSettings {
                mouse_sensitivity: 3.0,
            });
        app.add_systems(
            Update,
            (
                handle_slider_steps,
                handle_input_slider_steps,
                sync_slider_controls,
            )
                .chain(),
        );
        let steps: Vec<_> = [
            (SliderControl::Exposure, -1),
            (SliderControl::ViewDistance, 1),
            (SliderControl::PropDistance, -1),
            (SliderControl::ShadowQuality, -1),
        ]
        .into_iter()
        .map(|(control, delta)| {
            app.world_mut()
                .spawn((Interaction::None, SliderStep { control, delta }))
                .id()
        })
        .collect();
        let input = app
            .world_mut()
            .spawn((
                Interaction::None,
                InputSliderStep {
                    control: InputSliderControl::MouseSensitivity,
                    delta: 1,
                },
            ))
            .id();
        app.update();
        assert!(steps
            .iter()
            .chain([&input])
            .all(|entity| app.world().get::<InteractionDisabled>(*entity).is_some()));
        // Even a synthetic press cannot bypass disabled state during the frame
        // before a new bound value reenables this button.
        app.world_mut()
            .resource_mut::<GraphicsSettings>()
            .grade_exposure = 0.0;
        *app.world_mut().get_mut::<Interaction>(steps[0]).unwrap() = Interaction::Pressed;
        app.update();
        assert_eq!(
            app.world().resource::<GraphicsSettings>().grade_exposure,
            0.0
        );
        assert!(app.world().get::<InteractionDisabled>(steps[0]).is_none());
        assert_eq!(
            graphics_label(
                app.world().resource::<GraphicsSettings>(),
                SliderControl::ViewDistance
            ),
            "1024 m"
        );
    }

    #[test]
    fn irregular_saved_values_step_monotonically() {
        assert!((stepped(0.34, -1, 0.1, -1.0, 2.0) - 0.3).abs() < 0.001);
        assert!((stepped(0.34, 1, 0.1, -1.0, 2.0) - 0.4).abs() < 0.001);
    }
}
