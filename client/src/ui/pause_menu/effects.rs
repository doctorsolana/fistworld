//! One persisted switch for the first effects pack, beside the music switch.

use super::*;
use crate::audio::AudioSettings;
use bevy::ui::{InteractionDisabled, RelativeCursorPosition};

pub(super) const fn label(enabled: bool) -> &'static str {
    if enabled {
        "EFFECTS: ON"
    } else {
        "EFFECTS: OFF"
    }
}

pub(super) fn handle_effects_toggle(
    mouse: Res<ButtonInput<MouseButton>>,
    buttons: Query<
        (Ref<Interaction>, &RelativeCursorPosition),
        (With<EffectsToggle>, Without<InteractionDisabled>),
    >,
    mut settings: ResMut<AudioSettings>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    if buttons.iter().any(|(interaction, cursor)| {
        interaction.is_changed()
            && !interaction.is_added()
            && *interaction == Interaction::Pressed
            && cursor.cursor_over
    }) {
        settings.effects_enabled = !settings.effects_enabled;
    }
}

pub(super) fn sync_effects_toggle(
    settings: Res<AudioSettings>,
    mut labels: Query<&mut Text, With<EffectsToggleLabel>>,
    mut buttons: Query<&mut UiButtonStyle, With<EffectsToggle>>,
) {
    for mut text in &mut labels {
        if text.0 != label(settings.effects_enabled) {
            text.0 = label(settings.effects_enabled).into();
        }
    }
    for mut style in &mut buttons {
        if style.selected != settings.effects_enabled {
            style.selected = settings.effects_enabled;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effects_toggle_uses_live_edges_and_leaves_music_alone() {
        let mut app = App::new();
        app.init_resource::<AudioSettings>()
            .init_resource::<ButtonInput<MouseButton>>()
            .add_systems(Update, (handle_effects_toggle, sync_effects_toggle).chain());
        let button = app
            .world_mut()
            .spawn((
                EffectsToggle,
                Interaction::None,
                RelativeCursorPosition {
                    cursor_over: true,
                    ..default()
                },
            ))
            .id();
        let text = app
            .world_mut()
            .spawn((EffectsToggleLabel, Text::new("EFFECTS: ON")))
            .id();
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::Pressed;
        app.update();
        assert!(!app.world().resource::<AudioSettings>().effects_enabled);
        assert!(app.world().resource::<AudioSettings>().music_enabled);
        assert_eq!(app.world().get::<Text>(text).unwrap().0, "EFFECTS: OFF");
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
            .insert(InteractionDisabled);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::Pressed;
        app.update();
        assert!(!app.world().resource::<AudioSettings>().effects_enabled);
    }
}
