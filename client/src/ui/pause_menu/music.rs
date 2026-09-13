//! Music control in the normal Escape menu; effects remain independently audible.

use super::*;
use crate::audio::AudioSettings;

pub(super) const fn label(enabled: bool) -> &'static str {
    if enabled {
        "MUSIC: ON"
    } else {
        "MUSIC: OFF"
    }
}

pub(super) fn handle_music_toggle(
    buttons: Query<&Interaction, (Changed<Interaction>, With<MusicToggle>)>,
    mut settings: ResMut<AudioSettings>,
) {
    for interaction in &buttons {
        if *interaction == Interaction::Pressed {
            settings.music_enabled = !settings.music_enabled;
        }
    }
}

pub(super) fn sync_music_toggle(
    settings: Res<AudioSettings>,
    mut labels: Query<&mut Text, With<MusicToggleLabel>>,
    mut buttons: Query<&mut UiButtonStyle, With<MusicToggle>>,
) {
    for mut text in &mut labels {
        if text.0 != label(settings.music_enabled) {
            text.0 = label(settings.music_enabled).to_string();
        }
    }
    for mut style in &mut buttons {
        if style.selected != settings.music_enabled {
            style.selected = settings.music_enabled;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressed_music_control_toggles_once_and_updates_its_label() {
        let mut app = App::new();
        app.init_resource::<AudioSettings>();
        app.add_systems(Update, (handle_music_toggle, sync_music_toggle).chain());
        let button = app
            .world_mut()
            .spawn((MusicToggle, Interaction::Pressed))
            .id();
        let label = app
            .world_mut()
            .spawn((MusicToggleLabel, Text::new("MUSIC: ON")))
            .id();
        app.update();
        assert!(!app.world().resource::<AudioSettings>().music_enabled);
        assert_eq!(app.world().get::<Text>(label).unwrap().0, "MUSIC: OFF");
        app.update();
        assert!(!app.world().resource::<AudioSettings>().music_enabled);
        *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::None;
        app.update();
        *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::Pressed;
        app.update();
        assert!(app.world().resource::<AudioSettings>().music_enabled);
        assert_eq!(app.world().get::<Text>(label).unwrap().0, "MUSIC: ON");
    }
}
