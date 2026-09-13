//! Music control in the normal Escape menu; effects remain independently audible.

use super::audio::AudioEnabledChoice;
use super::*;
use crate::audio::AudioSettings;
use bevy::ui::{InteractionDisabled, RelativeCursorPosition};

pub(super) fn handle_music_toggle(
    mouse: Res<ButtonInput<MouseButton>>,
    buttons: Query<
        (
            Ref<Interaction>,
            &RelativeCursorPosition,
            &AudioEnabledChoice,
        ),
        (
            Changed<Interaction>,
            With<MusicToggle>,
            Without<InteractionDisabled>,
        ),
    >,
    mut settings: ResMut<AudioSettings>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (interaction, cursor, choice) in &buttons {
        if !interaction.is_added()
            && *interaction == Interaction::Pressed
            && cursor.cursor_over
            && settings.music_enabled != choice.0
        {
            settings.music_enabled = choice.0;
        }
    }
}

pub(super) fn sync_music_toggle(
    settings: Res<AudioSettings>,
    mut buttons: Query<(&AudioEnabledChoice, &mut UiButtonStyle), With<MusicToggle>>,
) {
    for (choice, mut style) in &mut buttons {
        let selected = settings.music_enabled == choice.0;
        if style.selected != selected {
            style.selected = selected;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(app: &mut App, button: Entity) {
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::None;
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::Pressed;
        app.update();
    }

    #[test]
    fn music_choices_set_requested_values_idempotently_and_preserve_other_levels() {
        let mut app = App::new();
        app.init_resource::<AudioSettings>()
            .init_resource::<ButtonInput<MouseButton>>()
            .add_systems(Update, (handle_music_toggle, sync_music_toggle).chain());
        app.world_mut().resource_mut::<AudioSettings>().music_volume = 0.4;
        let [on, off] = [true, false].map(|choice| {
            app.world_mut()
                .spawn((
                    MusicToggle,
                    AudioEnabledChoice(choice),
                    Interaction::None,
                    RelativeCursorPosition {
                        cursor_over: true,
                        ..default()
                    },
                    selected_button_chrome(UiButtonVariant::Inverse, choice),
                ))
                .id()
        });
        app.update();
        for (button, enabled) in [
            (on, true),
            (on, true),
            (off, false),
            (off, false),
            (on, true),
        ] {
            click(&mut app, button);
            assert_eq!(
                app.world().resource::<AudioSettings>().music_enabled,
                enabled
            );
            assert_eq!(
                app.world().get::<UiButtonStyle>(on).unwrap().selected,
                enabled
            );
            assert_eq!(
                app.world().get::<UiButtonStyle>(off).unwrap().selected,
                !enabled
            );
            assert_eq!(app.world().resource::<AudioSettings>().music_volume, 0.4);
            assert!(app.world().resource::<AudioSettings>().effects_enabled);
        }
        app.world_mut().entity_mut(off).insert(InteractionDisabled);
        click(&mut app, off);
        assert!(app.world().resource::<AudioSettings>().music_enabled);
    }
}
