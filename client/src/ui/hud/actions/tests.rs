use super::*;
use crate::ui::debug_time_menu::DebugTimeMenuOpen;

fn app() -> App {
    let mut app = App::new();
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<InputState>()
        .init_resource::<GodCapability>()
        .init_resource::<HudMode>()
        .init_resource::<crate::boat::OpeningCinematic>()
        .init_resource::<DebugTimeMenuOpen>();
    app.add_systems(
        Update,
        (handle_mode_toggle_key, handle_mode_chip_button).chain(),
    );
    app
}

fn press_g(app: &mut App) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyG);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
}

#[test]
fn accelerated_time_console_does_not_capture_the_god_exit_key() {
    let mut app = app();
    *app.world_mut().resource_mut::<HudMode>() = HudMode::God;
    app.world_mut().resource_mut::<GodCapability>().0 = true;
    app.world_mut().resource_mut::<DebugTimeMenuOpen>().0 = true;
    *app.world_mut().resource_mut::<InputState>() = InputState {
        modal_open: true,
        debug_menu_open: true,
        ..default()
    };
    let clock = app
        .world_mut()
        .spawn((WorldTime::new_default(), TimeWarp(100.0)))
        .id();
    // No server sender or connection is needed to reduce local privileges.
    // A stale hidden chip press must not toggle back after the key exits.
    app.world_mut()
        .spawn((ModeChipButton, Interaction::Pressed));
    press_g(&mut app);
    assert_eq!(*app.world().resource::<HudMode>(), HudMode::Play);
    assert!(!app.world().resource::<DebugTimeMenuOpen>().0);
    assert_eq!(app.world().get::<TimeWarp>(clock).unwrap().0, 100.0);
}

#[test]
fn entering_god_requires_capability_and_preserves_other_modal_input() {
    let mut app = app();
    press_g(&mut app);
    assert_eq!(*app.world().resource::<HudMode>(), HudMode::Play);
    app.world_mut().resource_mut::<GodCapability>().0 = true;
    app.world_mut()
        .resource_mut::<InputState>()
        .business_management_open = true;
    press_g(&mut app);
    assert_eq!(*app.world().resource::<HudMode>(), HudMode::Play);
    *app.world_mut().resource_mut::<HudMode>() = HudMode::God;
    press_g(&mut app);
    assert_eq!(*app.world().resource::<HudMode>(), HudMode::God);
}

#[test]
fn a_developer_console_behind_another_modal_does_not_steal_typed_g() {
    let mut app = app();
    *app.world_mut().resource_mut::<HudMode>() = HudMode::God;
    app.world_mut().resource_mut::<GodCapability>().0 = true;
    app.world_mut().resource_mut::<DebugTimeMenuOpen>().0 = true;
    app.world_mut().resource_mut::<InputState>().modal_open = true;
    app.world_mut().spawn((
        crate::ui::modal::ModalRoot,
        crate::ui::debug_time_menu::DebugMenuRoot,
    ));
    app.world_mut().spawn(crate::ui::modal::ModalRoot);
    press_g(&mut app);
    assert_eq!(*app.world().resource::<HudMode>(), HudMode::God);
    assert!(app.world().resource::<DebugTimeMenuOpen>().0);
}

#[test]
fn simultaneous_keyboard_and_visible_chip_apply_only_one_mode_change() {
    let mut app = app();
    app.world_mut().resource_mut::<GodCapability>().0 = true;
    *app.world_mut().resource_mut::<HudMode>() = HudMode::God;
    app.world_mut()
        .spawn((ModeChipButton, Interaction::Pressed));
    press_g(&mut app);
    assert_eq!(*app.world().resource::<HudMode>(), HudMode::Play);
}

#[test]
fn clicking_the_mode_chip_cancels_the_opening_and_hidden_chips_do_not_toggle() {
    let mut app = app();
    app.world_mut().resource_mut::<GodCapability>().0 = true;
    app.world_mut()
        .resource_mut::<crate::boat::OpeningCinematic>()
        .arm();
    let button = app
        .world_mut()
        .spawn((ModeChipButton, Interaction::Pressed))
        .id();
    app.update();
    assert_eq!(*app.world().resource::<HudMode>(), HudMode::God);
    assert!(!app
        .world()
        .resource::<crate::boat::OpeningCinematic>()
        .is_active());
    app.world_mut().resource_mut::<InputState>().modal_open = true;
    *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::Pressed;
    app.update();
    assert_eq!(*app.world().resource::<HudMode>(), HudMode::God);
}

#[test]
fn leaving_god_stops_the_developer_boat_camera_watch() {
    let mut app = app();
    app.init_resource::<Time>()
        .init_resource::<GodNotice>()
        .init_resource::<ImmigrantBoatWatch>();
    app.add_systems(Update, watch_immigrant_boat);
    app.world_mut().resource_mut::<ImmigrantBoatWatch>().waiting = true;
    app.update();
    assert!(!app.world().resource::<ImmigrantBoatWatch>().active());
}
