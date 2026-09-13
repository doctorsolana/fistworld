//! Regressions for the retained page navigation and input ownership.

use super::*;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::ui::InteractionDisabled;

fn app() -> App {
    let mut app = App::new();
    app.init_resource::<PauseMenuState>()
        .insert_resource(PauseMenuOpen(true))
        .insert_resource(InputState {
            pause_menu_open: true,
            ..default()
        })
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<InputFocus>()
        .init_resource::<NextState<GameState>>()
        .add_message::<AppExit>();
    app
}

fn press_key(app: &mut App, key: KeyCode) {
    let mut keyboard = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keyboard.release(key);
    keyboard.clear();
    keyboard.press(key);
}

#[test]
fn escape_returns_from_each_settings_page_then_resumes_then_reopens() {
    for page in [
        PauseButton::Graphics,
        PauseButton::Audio,
        PauseButton::Controls,
    ] {
        let mut app = app();
        app.add_systems(Update, handle_escape_key);
        {
            let mut state = app.world_mut().resource_mut::<PauseMenuState>();
            state.graphics_open = matches!(page, PauseButton::Graphics);
            state.audio_open = matches!(page, PauseButton::Audio);
            state.controls_open = matches!(page, PauseButton::Controls);
        }
        press_key(&mut app, KeyCode::Escape);
        app.update();
        let state = app.world().resource::<PauseMenuState>();
        assert!(!state.graphics_open && !state.controls_open && !state.audio_open);
        assert!(app.world().resource::<PauseMenuOpen>().0);
        assert!(app.world().resource::<InputState>().pause_menu_open);

        press_key(&mut app, KeyCode::Escape);
        app.update();
        assert!(!app.world().resource::<PauseMenuOpen>().0);
        assert!(!app.world().resource::<InputState>().pause_menu_open);

        press_key(&mut app, KeyCode::Escape);
        app.update();
        assert!(app.world().resource::<PauseMenuOpen>().0);
        assert!(app.world().resource::<InputState>().pause_menu_open);
    }
}

#[test]
fn escape_does_not_open_game_menu_over_another_modal() {
    for creator in [false, true] {
        let mut app = app();
        app.add_systems(Update, handle_escape_key);
        app.world_mut().resource_mut::<PauseMenuOpen>().0 = false;
        {
            let mut input = app.world_mut().resource_mut::<InputState>();
            input.pause_menu_open = false;
            input.encyclopedia_open = !creator;
            input.hero_creator_open = creator;
        }
        press_key(&mut app, KeyCode::Escape);
        app.update();
        assert!(!app.world().resource::<PauseMenuOpen>().0);
        let input = app.world().resource::<InputState>();
        assert!(!input.pause_menu_open);
        assert_eq!(input.encyclopedia_open, !creator);
        assert_eq!(input.hero_creator_open, creator);
    }
}

#[test]
fn back_keeps_the_menu_open_and_resume_releases_its_input_block() {
    let mut app = app();
    app.add_systems(Update, handle_pause_actions);
    app.world_mut().resource_mut::<PauseMenuState>().audio_open = true;
    let back = app
        .world_mut()
        .spawn((Button, PauseButton::Back, Interaction::Pressed))
        .id();
    app.update();
    assert!(app.world().resource::<PauseMenuOpen>().0);
    assert!(app.world().resource::<InputState>().pause_menu_open);
    assert!(!app.world().resource::<PauseMenuState>().audio_open);

    *app.world_mut().get_mut::<Interaction>(back).unwrap() = Interaction::None;
    app.world_mut()
        .spawn((Button, PauseButton::Resume, Interaction::Pressed));
    app.update();
    assert!(!app.world().resource::<PauseMenuOpen>().0);
    assert!(!app.world().resource::<InputState>().pause_menu_open);
}

#[test]
fn a_navigation_key_is_not_reapplied_after_the_page_changes() {
    let mut app = app();
    app.add_systems(Update, handle_pause_actions);
    let graphics = app.world_mut().spawn((Button, PauseButton::Graphics)).id();
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(graphics, FocusCause::Navigated);
    press_key(&mut app, KeyCode::Enter);
    app.update();
    assert!(app.world().resource::<PauseMenuState>().graphics_open);

    // Keep Enter physically down, but clear its edge as Bevy's InputSystems does.
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    app.update();
    assert!(app.world().resource::<PauseMenuState>().graphics_open);
    assert!(app.world().resource::<PauseMenuOpen>().0);

    let audio = app.world_mut().spawn((Button, PauseButton::Audio)).id();
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(audio, FocusCause::Navigated);
    app.update();
    assert!(app.world().resource::<PauseMenuState>().graphics_open);
    assert!(!app.world().resource::<PauseMenuState>().audio_open);

    press_key(&mut app, KeyCode::Enter);
    app.update();
    assert!(!app.world().resource::<PauseMenuState>().graphics_open);
    assert!(app.world().resource::<PauseMenuState>().audio_open);
}

#[test]
fn disabled_retained_navigation_cannot_activate_from_a_stale_focus_or_press() {
    let mut app = app();
    app.add_systems(Update, handle_pause_actions);
    let hidden = app
        .world_mut()
        .spawn((
            Button,
            PauseButton::Resume,
            Interaction::Pressed,
            InteractionDisabled,
        ))
        .id();
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(hidden, FocusCause::Navigated);
    press_key(&mut app, KeyCode::Space);
    app.update();
    assert!(app.world().resource::<PauseMenuOpen>().0);
    assert!(app.world().resource::<InputState>().pause_menu_open);
}
