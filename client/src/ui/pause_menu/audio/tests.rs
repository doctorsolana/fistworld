use super::*;
use crate::audio::sfx::SfxRequest;
use crate::ui::sound::UiSoundHandled;

fn app() -> App {
    let mut app = App::new();
    app.insert_resource(PauseMenuOpen(true))
        .insert_resource(PauseMenuState {
            audio_open: true,
            ..default()
        })
        .init_resource::<AudioSettings>()
        .init_resource::<AudioDrag>()
        .init_resource::<InputFocus>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<Messages<SfxRequest>>()
        .add_systems(Update, (handle_audio_controls, sync_audio_controls).chain());
    crate::ui::sound::install(&mut app);
    app
}

fn slider(app: &mut App, control: AudioControl) -> Entity {
    app.world_mut()
        .spawn((
            Button,
            AudioSlider(control),
            UiSoundHandled,
            button_chrome(UiButtonVariant::Inverse),
            Interaction::None,
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::new(-0.25, 0.0)),
            },
        ))
        .id()
}

fn cues(app: &mut App) -> Vec<SfxCue> {
    app.world_mut()
        .resource_mut::<Messages<SfxRequest>>()
        .drain()
        .map(|request| request.cue)
        .collect()
}

#[test]
fn drag_updates_live_level_and_label_without_repeating_the_gesture_sound() {
    let mut app = app();
    let slider = slider(&mut app, AudioControl::Music);
    let value = app
        .world_mut()
        .spawn((AudioValue(AudioControl::Music), Text::new("100%")))
        .id();
    let fill = app
        .world_mut()
        .spawn((AudioFill(AudioControl::Music), Node::default()))
        .id();
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    *app.world_mut().get_mut::<Interaction>(slider).unwrap() = Interaction::Pressed;
    app.update();
    assert_eq!(app.world().resource::<AudioSettings>().music_volume, 0.25);
    assert_eq!(app.world().resource::<AudioSettings>().effects_volume, 1.0);
    assert_eq!(app.world().get::<Text>(value).unwrap().0, "25%");
    assert_eq!(
        app.world().get::<Node>(fill).unwrap().width,
        Val::Percent(25.0)
    );
    assert_eq!(cues(&mut app), [SfxCue::UiClick]);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.world_mut()
        .get_mut::<RelativeCursorPosition>(slider)
        .unwrap()
        .normalized = Some(Vec2::new(0.2, 0.0));
    app.update();
    assert!((app.world().resource::<AudioSettings>().music_volume - 0.7).abs() < 0.0001);
    assert_eq!(app.world().get::<Text>(value).unwrap().0, "70%");
    assert!(cues(&mut app).is_empty());
    app.world_mut()
        .get_mut::<RelativeCursorPosition>(slider)
        .unwrap()
        .normalized = Some(Vec2::new(2.0, 0.0));
    app.update();
    assert_eq!(app.world().resource::<AudioSettings>().music_volume, 1.0);
    assert!(cues(&mut app).is_empty());
}

#[test]
fn opening_held_click_disabled_control_and_closed_page_cannot_change_a_level() {
    let mut app = app();
    let slider = slider(&mut app, AudioControl::Master);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    *app.world_mut().get_mut::<Interaction>(slider).unwrap() = Interaction::Pressed;
    app.update();
    assert_eq!(app.world().resource::<AudioSettings>().master_volume, 1.0);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    *app.world_mut().get_mut::<Interaction>(slider).unwrap() = Interaction::None;
    app.update();
    app.world_mut()
        .entity_mut(slider)
        .insert(InteractionDisabled);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    *app.world_mut().get_mut::<Interaction>(slider).unwrap() = Interaction::Pressed;
    app.update();
    assert_eq!(app.world().resource::<AudioSettings>().master_volume, 1.0);
    app.world_mut()
        .entity_mut(slider)
        .remove::<InteractionDisabled>();
    app.world_mut().resource_mut::<PauseMenuState>().audio_open = false;
    app.update();
    assert_eq!(app.world().resource::<AudioSettings>().master_volume, 1.0);
    assert!(app.world().resource::<AudioDrag>().active.is_none());
    assert!(!app.world().resource::<AudioDrag>().armed);
    assert!(cues(&mut app).is_empty());
}

#[test]
fn keyboard_levels_are_precise_and_endpoint_steps_disable_then_reenable() {
    let mut app = app();
    let slider = slider(&mut app, AudioControl::Effects);
    let increase = app
        .world_mut()
        .spawn((
            AudioStep {
                control: AudioControl::Effects,
                delta: 1,
            },
            Interaction::None,
            RelativeCursorPosition::default(),
        ))
        .id();
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(slider, bevy::input_focus::FocusCause::Navigated);
    app.update();
    assert!(app.world().get::<InteractionDisabled>(increase).is_some());
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ArrowLeft);
    app.update();
    assert_eq!(app.world().resource::<AudioSettings>().effects_volume, 0.95);
    assert!(app.world().get::<InteractionDisabled>(increase).is_none());
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Home);
    app.update();
    assert_eq!(app.world().resource::<AudioSettings>().effects_volume, 0.0);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::End);
    app.update();
    assert_eq!(app.world().resource::<AudioSettings>().effects_volume, 1.0);
    assert!(app.world().get::<InteractionDisabled>(increase).is_some());
}

#[test]
fn arbitrary_saved_values_step_monotonically_and_mutes_preserve_levels() {
    assert!((step(0.33, -1) - 0.3).abs() < 0.0001);
    assert!((step(0.33, 1) - 0.35).abs() < 0.0001);
    let mut app = app();
    let toggle = app
        .world_mut()
        .spawn((MusicToggle, AudioEnabledChoice(false)))
        .id();
    app.world_mut().resource_mut::<AudioSettings>().music_volume = 0.4;
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(toggle, bevy::input_focus::FocusCause::Navigated);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Space);
    app.update();
    assert!(!app.world().resource::<AudioSettings>().music_enabled);
    assert_eq!(app.world().resource::<AudioSettings>().music_volume, 0.4);
    assert!(app.world().resource::<AudioSettings>().effects_enabled);
}

fn choice(app: &mut App, control: AudioControl, enabled: bool) -> Entity {
    let mut button = app.world_mut().spawn((
        AudioEnabledChoice(enabled),
        Interaction::None,
        RelativeCursorPosition {
            cursor_over: true,
            ..default()
        },
    ));
    if control == AudioControl::Music {
        button.insert(MusicToggle);
    } else {
        button.insert(EffectsToggle);
    }
    button.id()
}

fn is_enabled(app: &App, control: AudioControl) -> bool {
    let settings = app.world().resource::<AudioSettings>();
    if control == AudioControl::Music {
        settings.music_enabled
    } else {
        settings.effects_enabled
    }
}

#[test]
fn explicit_on_off_choices_are_idempotent_for_keyboard_and_ignore_disabled_choices() {
    for control in [AudioControl::Music, AudioControl::Effects] {
        let mut app = app();
        let on = choice(&mut app, control, true);
        let off = choice(&mut app, control, false);
        app.update();
        for (button, enabled) in [
            (on, true),
            (on, true),
            (off, false),
            (off, false),
            (on, true),
        ] {
            app.world_mut()
                .resource_mut::<InputFocus>()
                .set(button, bevy::input_focus::FocusCause::Navigated);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .release(KeyCode::Space);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .clear();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Space);
            app.update();
            assert_eq!(is_enabled(&app, control), enabled);
        }
        app.world_mut().entity_mut(off).insert(InteractionDisabled);
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(off, bevy::input_focus::FocusCause::Navigated);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        assert!(is_enabled(&app, control));
    }
}

#[test]
fn explicit_on_off_choices_are_idempotent_for_mouse() {
    for control in [AudioControl::Music, AudioControl::Effects] {
        let mut app = app();
        app.add_systems(
            Update,
            (
                super::super::music::handle_music_toggle,
                super::super::effects::handle_effects_toggle,
            )
                .before(handle_audio_controls),
        );
        let on = choice(&mut app, control, true);
        let off = choice(&mut app, control, false);
        app.update();
        for (button, enabled) in [
            (on, true),
            (on, true),
            (off, false),
            (off, false),
            (on, true),
        ] {
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
            assert_eq!(is_enabled(&app, control), enabled);
            let settings = app.world().resource::<AudioSettings>();
            assert_eq!(settings.master_volume, 1.0);
            assert_eq!(settings.music_volume, 0.5);
            assert_eq!(settings.effects_volume, 1.0);
        }
    }
}
