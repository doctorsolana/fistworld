use super::*;
use crate::states::GameState;
use crate::ui::encyclopedia::{ClickGuard, EncyclopediaOpen, EncyclopediaRoot, EncyclopediaTab};

fn app() -> App {
    let mut app = App::new();
    app.init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<Messages<SfxRequest>>()
        .insert_resource(State::new(GameState::Playing))
        .init_resource::<EncyclopediaOpen>()
        .init_resource::<EncyclopediaTab>()
        .init_resource::<ClickGuard>();
    install(&mut app);
    app
}

fn button(app: &mut App) -> Entity {
    app.world_mut()
        .spawn((
            Button,
            Interaction::None,
            RelativeCursorPosition {
                cursor_over: true,
                ..default()
            },
            UiButtonStyle::new(UiButtonVariant::Secondary),
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

fn press(app: &mut App, button: Entity) {
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::Pressed;
}

fn release(app: &mut App, button: Entity) {
    let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
    mouse.release(MouseButton::Left);
    mouse.clear();
    *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::None;
    app.update();
}

#[test]
fn genuine_press_is_once_while_held_rebuilds_and_hover_are_silent() {
    let mut app = app();
    let control = button(&mut app);
    app.update();
    assert!(cues(&mut app).is_empty());
    press(&mut app, control);
    app.update();
    assert_eq!(cues(&mut app), [SfxCue::UiClick]);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    // Even a component rewrite or replacement tree cannot replay a held press.
    *app.world_mut().get_mut::<Interaction>(control).unwrap() = Interaction::Pressed;
    let rebuilt = button(&mut app);
    *app.world_mut().get_mut::<Interaction>(rebuilt).unwrap() = Interaction::Pressed;
    app.update();
    assert!(cues(&mut app).is_empty());
    release(&mut app, control);
    *app.world_mut().get_mut::<Interaction>(control).unwrap() = Interaction::Hovered;
    app.update();
    assert!(cues(&mut app).is_empty());
}

#[test]
fn disabled_stale_off_pointer_and_new_controls_are_silent() {
    let mut app = app();
    let control = button(&mut app);
    app.update();
    app.world_mut()
        .entity_mut(control)
        .insert(InteractionDisabled);
    press(&mut app, control);
    app.update();
    assert!(cues(&mut app).is_empty());
    release(&mut app, control);
    app.world_mut()
        .entity_mut(control)
        .remove::<InteractionDisabled>();
    app.world_mut()
        .get_mut::<RelativeCursorPosition>(control)
        .unwrap()
        .cursor_over = false;
    press(&mut app, control);
    app.update();
    assert!(cues(&mut app).is_empty());
    release(&mut app, control);
    let new_control = button(&mut app);
    press(&mut app, new_control);
    app.update();
    assert!(cues(&mut app).is_empty());
}

#[test]
fn modal_opening_click_and_obscured_controls_are_silent_until_armed() {
    let mut app = app();
    let behind = button(&mut app);
    let inside = button(&mut app);
    let modal = app
        .world_mut()
        .spawn((ModalRoot, EncyclopediaRoot))
        .add_child(inside)
        .id();
    press(&mut app, inside);
    app.update();
    assert!(cues(&mut app).is_empty());
    release(&mut app, inside);
    // The adapter must respect the screen's guard as well as modal ancestry.
    press(&mut app, inside);
    app.update();
    assert!(cues(&mut app).is_empty());
    release(&mut app, inside);
    app.world_mut().resource_mut::<ClickGuard>().0 = true;
    press(&mut app, behind);
    app.update();
    assert!(cues(&mut app).is_empty());
    release(&mut app, behind);
    press(&mut app, inside);
    app.update();
    assert_eq!(cues(&mut app), [SfxCue::UiClick]);
    assert!(app.world().entities().contains(modal));
}

#[test]
fn book_open_close_and_page_changes_replace_generic_click_and_dedupe() {
    let mut app = app();
    let control = button(&mut app);
    app.update();
    press(&mut app, control);
    app.world_mut().resource_mut::<EncyclopediaOpen>().0 = true;
    *app.world_mut().resource_mut::<EncyclopediaTab>() = EncyclopediaTab::Places;
    app.update();
    assert_eq!(cues(&mut app), [SfxCue::BookOpen]);
    release(&mut app, control);
    press(&mut app, control);
    *app.world_mut().resource_mut::<EncyclopediaTab>() = EncyclopediaTab::Companies;
    app.update();
    assert_eq!(cues(&mut app), [SfxCue::PageTurn]);
    release(&mut app, control);
    // Reapplying the same logical tab is not a new page.
    *app.world_mut().resource_mut::<EncyclopediaTab>() = EncyclopediaTab::Companies;
    app.update();
    assert!(cues(&mut app).is_empty());
    app.world_mut().resource_mut::<EncyclopediaOpen>().0 = false;
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    app.update();
    assert_eq!(cues(&mut app), [SfxCue::BookClose]);
    app.update();
    assert!(cues(&mut app).is_empty());
}

#[test]
fn startup_first_snapshot_session_cleanup_and_background_refresh_are_silent() {
    let mut app = app();
    app.world_mut().resource_mut::<EncyclopediaOpen>().0 = true;
    app.update();
    assert!(cues(&mut app).is_empty());
    *app.world_mut().resource_mut::<EncyclopediaTab>() = EncyclopediaTab::Places;
    app.update();
    assert!(cues(&mut app).is_empty());
    app.world_mut()
        .insert_resource(State::new(GameState::MainMenu));
    app.world_mut().resource_mut::<EncyclopediaOpen>().0 = false;
    app.update();
    assert!(cues(&mut app).is_empty());
}

#[test]
fn nested_page_back_uses_one_page_turn_without_closing_book() {
    let mut app = app();
    app.init_resource::<crate::ui::company_founding::FoundingPageOpen>();
    app.world_mut().resource_mut::<EncyclopediaOpen>().0 = true;
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.world_mut()
        .resource_mut::<crate::ui::company_founding::FoundingPageOpen>()
        .0 = true;
    app.update();
    assert_eq!(cues(&mut app), [SfxCue::PageTurn]);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    app.world_mut()
        .resource_mut::<crate::ui::company_founding::FoundingPageOpen>()
        .0 = false;
    app.update();
    assert_eq!(cues(&mut app), [SfxCue::PageTurn]);
    assert!(app.world().resource::<EncyclopediaOpen>().0);
}

#[test]
fn accepted_specific_action_suppresses_the_generic_press() {
    fn reject(mouse: Res<ButtonInput<MouseButton>>, mut sounds: UiActionSounds) {
        if mouse.just_pressed(MouseButton::Left) {
            sounds.emit(SfxCue::UiReject);
        }
    }
    let mut app = app();
    let control = button(&mut app);
    app.add_systems(Update, reject);
    app.update();
    press(&mut app, control);
    app.update();
    assert_eq!(cues(&mut app), [SfxCue::UiReject]);
}
