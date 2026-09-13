//! One persisted switch for the first effects pack, beside the music switch.

use super::audio::AudioEnabledChoice;
use super::*;
use crate::audio::AudioSettings;
use bevy::ui::{InteractionDisabled, RelativeCursorPosition};

pub(super) fn handle_effects_toggle(
    mouse: Res<ButtonInput<MouseButton>>,
    buttons: Query<
        (
            Ref<Interaction>,
            &RelativeCursorPosition,
            &AudioEnabledChoice,
        ),
        (With<EffectsToggle>, Without<InteractionDisabled>),
    >,
    mut settings: ResMut<AudioSettings>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (interaction, cursor, choice) in &buttons {
        if interaction.is_changed()
            && !interaction.is_added()
            && *interaction == Interaction::Pressed
            && cursor.cursor_over
            && settings.effects_enabled != choice.0
        {
            settings.effects_enabled = choice.0;
        }
    }
}

pub(super) fn sync_effects_toggle(
    settings: Res<AudioSettings>,
    mut buttons: Query<(&AudioEnabledChoice, &mut UiButtonStyle), With<EffectsToggle>>,
) {
    for (choice, mut style) in &mut buttons {
        let selected = settings.effects_enabled == choice.0;
        if style.selected != selected {
            style.selected = selected;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effects_choices_use_live_edges_and_preserve_music() {
        let mut app = App::new();
        app.init_resource::<AudioSettings>()
            .init_resource::<ButtonInput<MouseButton>>()
            .add_systems(Update, (handle_effects_toggle, sync_effects_toggle).chain());
        let button = app
            .world_mut()
            .spawn((
                EffectsToggle,
                AudioEnabledChoice(false),
                Interaction::None,
                selected_button_chrome(UiButtonVariant::Inverse, true),
                RelativeCursorPosition {
                    cursor_over: true,
                    ..default()
                },
            ))
            .id();
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::Pressed;
        app.update();
        assert!(!app.world().resource::<AudioSettings>().effects_enabled);
        assert!(app.world().resource::<AudioSettings>().music_enabled);
        assert!(app.world().get::<UiButtonStyle>(button).unwrap().selected);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        app.update();
        assert!(!app.world().resource::<AudioSettings>().effects_enabled);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::None;
        app.update();
        app.world_mut()
            .entity_mut(button)
            .insert((InteractionDisabled, AudioEnabledChoice(true)));
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::Pressed;
        app.update();
        assert!(!app.world().resource::<AudioSettings>().effects_enabled);
    }
}
