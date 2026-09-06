//! Shared tactile feedback. Move labels, never the button hit area.
use super::{
    foundation::{button_colors, UiButtonStyle, UiButtonVariant},
    motion::Spring,
};
use bevy::{prelude::*, ui::InteractionDisabled};

#[derive(Component)]
pub(super) struct ButtonMotion {
    spring: Spring,
    style: Option<UiButtonStyle>,
}
impl ButtonMotion {
    pub(super) fn offset(&self) -> f32 {
        self.spring.value
    }
}

impl Default for ButtonMotion {
    fn default() -> Self {
        Self {
            spring: Spring::new(0.0),
            style: None,
        }
    }
}

pub(super) fn animate_buttons(
    time: Res<Time>,
    mut buttons: Query<(
        &Interaction,
        &UiButtonStyle,
        Has<InteractionDisabled>,
        &mut BackgroundColor,
        &mut BorderColor,
        &mut ButtonMotion,
    )>,
) {
    let dt = time.delta_secs();
    let blend = 1.0 - (-24.0 * dt).exp();
    for (interaction, style, disabled, mut background, mut border, mut motion) in &mut buttons {
        let (fill, rule) = button_colors(*interaction, *style, disabled);
        let next = if motion.style != Some(*style) || disabled || background.0 == fill {
            fill
        } else {
            let current = background.0.to_linear();
            let target = fill.to_linear();
            if (current.to_vec4() - target.to_vec4()).length_squared() < 0.00001 {
                fill
            } else {
                Color::LinearRgba(current.mix(&target, blend))
            }
        };
        if background.0 != next {
            background.0 = next;
        }
        let next_border = BorderColor::all(rule);
        if *border != next_border {
            *border = next_border;
        }
        if motion.style != Some(*style) {
            motion.style = Some(*style);
        }
        let target =
            if disabled || matches!(style.variant, UiButtonVariant::Row | UiButtonVariant::Tab) {
                0.0
            } else {
                match interaction {
                    Interaction::Hovered => -1.25,
                    Interaction::Pressed => 1.0,
                    Interaction::None => 0.0,
                }
            };
        motion.spring.step(target, dt, 320.0, 24.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{foundation::button_chrome, styles::EMBER};
    use std::time::Duration;
    #[test]
    fn selection_snaps_for_label_contrast_and_settled_controls_stop_writing() {
        let mut app = App::new();
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_secs_f32(1.0 / 60.0));
        app.insert_resource(time)
            .add_systems(Update, animate_buttons);
        let button = app
            .world_mut()
            .spawn((Button, button_chrome(UiButtonVariant::Secondary)))
            .id();
        app.update();
        app.world_mut()
            .get_mut::<UiButtonStyle>(button)
            .unwrap()
            .selected = true;
        app.update();
        assert_eq!(app.world().get::<BackgroundColor>(button).unwrap().0, EMBER);
        app.world_mut()
            .get_mut::<Interaction>(button)
            .unwrap()
            .clone_from(&Interaction::Hovered);
        for _ in 0..180 {
            app.update();
        }
        app.world_mut().clear_trackers();
        app.update();
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, Changed<bevy::ui::UiTransform>>()
                .iter(app.world())
                .count(),
            0
        );
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, Changed<BackgroundColor>>()
                .iter(app.world())
                .count(),
            0
        );
    }
}
