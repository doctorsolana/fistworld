use super::*;

fn manifest() -> HeroManifest {
    HeroManifest(
        ron::from_str(
            r#"(
        version: 1, scene: "characters/Test.glb#Scene0", body: "Body",
        slots: [(name: "top", default: "Top_Linen", items: ["Top_Linen", "Top_Wool_Tunic"])],
        skin: (material: "Skin", default: "Fair", tones: [
            (name: "Fair", rgb: (0.9, 0.8, 0.7)),
            (name: "Warm_Tan", rgb: (0.6, 0.4, 0.3)),
        ]), body_clips: [], face_clips: [],
    )"#,
        )
        .unwrap(),
    )
}

fn app() -> App {
    let mut app = App::new();
    app.insert_resource(manifest())
        .insert_resource(HeroCreatorOpen(true))
        .insert_resource(CreatorClickGuard {
            armed: true,
            ..default()
        })
        .init_resource::<SelectedOutfit>()
        .init_resource::<WorldPlacementMode>()
        .init_resource::<GodNotice>()
        .init_resource::<CreatorFeedback>()
        .init_resource::<GodCapability>()
        .init_resource::<HudMode>()
        .init_resource::<crate::input::InputState>()
        .init_resource::<crate::boat::OpeningCinematic>()
        .init_resource::<InputFocus>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>();
    app
}

fn key(app: &mut App, key: KeyCode, target: Entity) {
    let mut keyboard = ButtonInput::default();
    keyboard.press(key);
    app.insert_resource(keyboard);
    app.insert_resource(InputFocus::from_entity(target));
}

#[test]
fn leaving_playing_resets_the_opening_click_guard_for_a_new_session() {
    let mut app = app();
    app.add_systems(Update, force_close_creator);
    {
        let mut guard = app.world_mut().resource_mut::<CreatorClickGuard>();
        guard.armed = true;
        guard.press_in_flight = true;
        guard.completed_click = true;
    }
    app.update();
    assert!(!app.world().resource::<HeroCreatorOpen>().0);
    let guard = app.world().resource::<CreatorClickGuard>();
    assert!(!guard.armed && !guard.press_in_flight && !guard.completed_click);
}

#[test]
fn opening_click_never_activates_and_first_fresh_release_is_accepted() {
    let mut guard = CreatorClickGuard::default();
    guard.advance(true, true, true, false);
    assert!(!guard.armed && !guard.completed_click);
    guard.advance(true, false, false, true);
    assert!(guard.armed && !guard.completed_click);
    // No cursor/hover is needed on this first genuine press; release coordinates
    // are consumed by the action, after the OS has delivered its cursor state.
    guard.advance(true, true, true, false);
    assert!(!guard.completed_click);
    guard.advance(true, false, false, true);
    assert!(guard.completed_click);
    guard.advance(true, false, false, false);
    assert!(!guard.completed_click);
    guard.advance(false, false, false, false);
    assert!(!guard.armed);
    guard.advance(true, false, true, true);
    assert!(
        !guard.armed && !guard.completed_click,
        "same-frame opening click"
    );
    guard.advance(true, false, false, false);
    guard.advance(true, false, true, true);
    assert!(guard.completed_click, "a fast later click remains valid");
}

#[test]
fn keyboard_cycles_wrap_and_sync_only_the_local_preview() {
    let mut app = app();
    app.add_systems(Update, (handle_arrow_buttons, sync_preview_outfit).chain());
    let previous = app
        .world_mut()
        .spawn((
            RelativeCursorPosition::default(),
            ArrowButton {
                row: CreatorRow::Slot(0),
                dir: -1,
            },
        ))
        .id();
    let preview = app
        .world_mut()
        .spawn((HeroPreviewRig, HeroOutfit::default()))
        .id();
    let world_hero = app.world_mut().spawn(HeroOutfit::default()).id();
    key(&mut app, KeyCode::Enter, previous);
    app.update();
    assert_eq!(app.world().resource::<SelectedOutfit>().0.slot(0), 1);
    assert_eq!(app.world().get::<HeroOutfit>(preview).unwrap().slot(0), 1);
    assert_eq!(
        app.world().get::<HeroOutfit>(world_hero).unwrap().slot(0),
        0
    );
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    app.update();
    assert_eq!(
        app.world().resource::<SelectedOutfit>().0.slot(0),
        1,
        "held Enter does not repeat"
    );
    key(&mut app, KeyCode::Space, previous);
    app.update();
    assert_eq!(app.world().resource::<SelectedOutfit>().0.slot(0), 0);
    app.world_mut()
        .entity_mut(previous)
        .insert(InteractionDisabled);
    key(&mut app, KeyCode::NumpadEnter, previous);
    app.update();
    assert_eq!(app.world().resource::<SelectedOutfit>().0.slot(0), 0);
    app.world_mut().resource_mut::<SelectedOutfit>().0.skin = 1;
    let fresh = app
        .world_mut()
        .spawn((HeroPreviewRig, HeroOutfit::default()))
        .id();
    app.update();
    assert_eq!(app.world().get::<HeroOutfit>(preview).unwrap().skin, 1);
    assert_eq!(app.world().get::<HeroOutfit>(fresh).unwrap().skin, 1);
}

#[test]
fn empty_or_missing_choices_are_noops_and_skin_wraps() {
    let mut manifest = manifest();
    let mut outfit = HeroOutfit::default();
    cycle_outfit(&mut outfit, CreatorRow::Skin, -1, &manifest);
    assert_eq!(outfit.skin, 1);
    cycle_outfit(&mut outfit, CreatorRow::Skin, 1, &manifest);
    assert_eq!(outfit.skin, 0);
    cycle_outfit(&mut outfit, CreatorRow::Slot(99), 1, &manifest);
    manifest.0.slots[0].items.clear();
    cycle_outfit(&mut outfit, CreatorRow::Slot(0), -1, &manifest);
    assert_eq!(outfit, HeroOutfit::default());
}

#[derive(Resource, Default)]
struct LabelChanges(Vec<Entity>);

fn record_label_changes(
    labels: Query<Entity, (With<SlotValueText>, Changed<Text>)>,
    mut changes: ResMut<LabelChanges>,
) {
    changes.0.clear();
    changes.0.extend(labels.iter());
}

#[test]
fn labels_bind_only_changed_rows_and_new_widgets_without_rebuilding() {
    let mut app = app();
    app.init_resource::<LabelChanges>();
    app.add_systems(Update, (sync_slot_labels, record_label_changes).chain());
    let top = app
        .world_mut()
        .spawn((SlotValueText(CreatorRow::Slot(0)), Text::new("")))
        .id();
    let skin = app
        .world_mut()
        .spawn((SlotValueText(CreatorRow::Skin), Text::new("")))
        .id();
    app.update();
    assert_eq!(app.world().get::<Text>(top).unwrap().0, "Linen");
    assert_eq!(app.world().get::<Text>(skin).unwrap().0, "Fair");
    app.update();
    assert!(app.world().resource::<LabelChanges>().0.is_empty());
    app.world_mut().resource_mut::<SelectedOutfit>().0.slots[0] = 1;
    app.update();
    assert_eq!(app.world().resource::<LabelChanges>().0, vec![top]);
    assert_eq!(app.world().get::<Text>(top).unwrap().0, "Wool tunic");
    let duplicate = app
        .world_mut()
        .spawn((SlotValueText(CreatorRow::Slot(0)), Text::new("")))
        .id();
    app.update();
    assert_eq!(app.world().get::<Text>(duplicate).unwrap().0, "Wool tunic");
    app.world_mut().resource_mut::<HeroManifest>().0.slots[0].items[1] = "Top_QuiltedWool".into();
    app.update();
    assert_eq!(app.world().get::<Text>(top).unwrap().0, "Quilted wool");
    assert_eq!(
        app.world().get::<Text>(duplicate).unwrap().0,
        "Quilted wool"
    );
    assert!(!app.world().resource::<LabelChanges>().0.contains(&skin));
}

#[test]
fn confirm_requires_a_completed_click_or_focused_key_and_respects_disabled() {
    let mut app = app();
    app.add_systems(Update, handle_confirm_buttons);
    let confirm = app
        .world_mut()
        .spawn((
            BeginJourneyButton,
            RelativeCursorPosition {
                cursor_over: true,
                ..default()
            },
        ))
        .id();
    app.update();
    assert!(app.world().resource::<HeroCreatorOpen>().0);
    assert!(app.world().resource::<CreatorFeedback>().text.is_empty());
    app.world_mut()
        .resource_mut::<CreatorClickGuard>()
        .completed_click = true;
    app.world_mut()
        .entity_mut(confirm)
        .insert(InteractionDisabled);
    app.update();
    assert!(app.world().resource::<CreatorFeedback>().text.is_empty());
    app.world_mut()
        .entity_mut(confirm)
        .remove::<InteractionDisabled>();
    app.update();
    assert!(app.world().resource::<HeroCreatorOpen>().0);
    assert!(app
        .world()
        .resource::<CreatorFeedback>()
        .text
        .contains("Still connecting"));
    assert!(matches!(
        app.world().resource::<WorldPlacementMode>(),
        WorldPlacementMode::None
    ));
}

#[test]
fn disconnected_begin_journey_keeps_the_draft_and_shows_retry_feedback() {
    let mut app = app();
    app.add_systems(Update, (handle_confirm_buttons, sync_status_text).chain());
    let status = app
        .world_mut()
        .spawn((CreatorStatusText, Text::new("")))
        .id();
    let confirm = app
        .world_mut()
        .spawn((BeginJourneyButton, RelativeCursorPosition::default()))
        .id();
    app.world_mut().resource_mut::<SelectedOutfit>().0.skin = 1;
    key(&mut app, KeyCode::Space, confirm);
    app.update();
    assert!(app.world().resource::<HeroCreatorOpen>().0);
    assert_eq!(app.world().resource::<SelectedOutfit>().0.skin, 1);
    assert!(app
        .world()
        .resource::<GodNotice>()
        .text
        .contains("Still connecting"));
    assert!(app
        .world()
        .get::<Text>(status)
        .unwrap()
        .0
        .contains("Still connecting"));
    assert!(matches!(
        app.world().resource::<WorldPlacementMode>(),
        WorldPlacementMode::None
    ));
}

#[test]
fn feedback_is_cleared_before_a_reopened_creator_accepts_input() {
    let mut app = app();
    app.add_systems(Update, (update_click_guard, sync_status_text).chain());
    let status = app
        .world_mut()
        .spawn((CreatorStatusText, Text::new("")))
        .id();
    app.update();
    app.world_mut().resource_mut::<CreatorFeedback>().text = "Still connecting".into();
    app.update();
    assert_eq!(
        app.world().get::<Text>(status).unwrap().0,
        "Still connecting"
    );
    app.world_mut().resource_mut::<HeroCreatorOpen>().0 = false;
    app.update();
    app.world_mut().resource_mut::<HeroCreatorOpen>().0 = true;
    app.update();
    assert_eq!(app.world().get::<Text>(status).unwrap().0, "");
}

#[test]
fn mandatory_voyage_ignores_escape_but_preserves_capability_checked_god_exit() {
    let mut app = app();
    app.add_systems(Update, handle_developer_skip);
    let unused = app.world_mut().spawn_empty().id();
    key(&mut app, KeyCode::Escape, unused);
    app.update();
    assert!(app.world().resource::<HeroCreatorOpen>().0);
    key(&mut app, KeyCode::KeyG, unused);
    app.update();
    assert!(app.world().resource::<HeroCreatorOpen>().0);
    app.world_mut().resource_mut::<GodCapability>().0 = true;
    app.world_mut()
        .resource_mut::<crate::boat::OpeningCinematic>()
        .arm();
    key(&mut app, KeyCode::KeyG, unused);
    app.update();
    assert!(!app.world().resource::<HeroCreatorOpen>().0);
    assert_eq!(*app.world().resource::<HudMode>(), HudMode::God);
    assert!(!app
        .world()
        .resource::<crate::boat::OpeningCinematic>()
        .is_active());
}

fn focus_fixture(app: &mut App) -> (Entity, Entity, Entity, Entity) {
    use bevy::input_focus::tab_navigation::TabGroup;
    let root = app.world_mut().spawn((CreatorRoot, TabGroup::modal())).id();
    let previous = app
        .world_mut()
        .spawn((
            ChildOf(root),
            TabIndex(0),
            RelativeCursorPosition::default(),
            ArrowButton {
                row: CreatorRow::Slot(0),
                dir: -1,
            },
        ))
        .id();
    let next = app
        .world_mut()
        .spawn((
            ChildOf(root),
            TabIndex(0),
            RelativeCursorPosition::default(),
            ArrowButton {
                row: CreatorRow::Slot(0),
                dir: 1,
            },
        ))
        .id();
    let confirm = app
        .world_mut()
        .spawn((
            ChildOf(root),
            TabIndex(0),
            RelativeCursorPosition::default(),
            BeginJourneyButton,
        ))
        .id();
    app.world_mut().resource_mut::<HeroCreatorOpen>().0 = true;
    (root, previous, next, confirm)
}

fn press_real_key(app: &mut App, window: Entity, key_code: KeyCode) {
    use bevy::input::{
        keyboard::{Key, KeyboardInput},
        ButtonState,
    };
    for state in [ButtonState::Pressed, ButtonState::Released] {
        app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key: Key::Character(" ".into()),
            state,
            text: None,
            repeat: false,
            window,
        });
        app.update();
    }
}

#[test]
fn real_tab_navigation_stays_in_creator_and_releases_focus_on_exit() {
    use bevy::input::InputPlugin;
    use bevy::input_focus::{
        tab_navigation::{TabGroup, TabNavigationPlugin},
        InputDispatchPlugin, InputFocusPlugin,
    };
    use bevy::window::PrimaryWindow;

    let mut app = app();
    app.add_plugins((
        InputPlugin,
        InputFocusPlugin,
        InputDispatchPlugin,
        TabNavigationPlugin,
    ))
    .init_resource::<CreatorFocus>()
    .add_systems(
        Update,
        (
            handle_arrow_buttons,
            handle_confirm_buttons,
            release_creator_focus,
            super::super::layout::despawn_creator.run_if(super::super::creator_closed),
        )
            .chain(),
    )
    .add_systems(PostUpdate, initialize_creator_focus);
    app.world_mut().resource_mut::<HeroCreatorOpen>().0 = false;
    let window = app
        .world_mut()
        .spawn((Window::default(), PrimaryWindow))
        .id();
    let background = app.world_mut().spawn(TabGroup::default()).id();
    let prior_control = app
        .world_mut()
        .spawn((ChildOf(background), TabIndex(0)))
        .id();
    app.update();
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(prior_control, FocusCause::Navigated);

    let (root, previous, next, confirm) = focus_fixture(&mut app);
    app.update();
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(previous));
    press_real_key(&mut app, window, KeyCode::Tab);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(next));
    press_real_key(&mut app, window, KeyCode::Space);
    assert_eq!(app.world().resource::<SelectedOutfit>().0.slot(0), 1);
    press_real_key(&mut app, window, KeyCode::Tab);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(confirm));
    press_real_key(&mut app, window, KeyCode::Tab);
    assert_eq!(
        app.world().resource::<InputFocus>().get(),
        Some(previous),
        "Tab stays in the modal"
    );
    press_real_key(&mut app, window, KeyCode::Tab);
    press_real_key(&mut app, window, KeyCode::Tab);
    press_real_key(&mut app, window, KeyCode::Enter);
    assert!(
        app.world().resource::<HeroCreatorOpen>().0,
        "disconnected confirmation retains the creator"
    );
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(confirm));
    // Session exit or successful creation closes the modal and releases focus.
    app.world_mut().resource_mut::<HeroCreatorOpen>().0 = false;
    app.update();
    assert!(!app.world().resource::<HeroCreatorOpen>().0);
    assert!(app.world().get_entity(root).is_err());
    assert_eq!(
        app.world().resource::<InputFocus>().get(),
        Some(prior_control)
    );

    let (_, reopened_previous, reopened_next, _) = focus_fixture(&mut app);
    app.update();
    assert_ne!(previous, reopened_previous);
    assert_eq!(
        app.world().resource::<InputFocus>().get(),
        Some(reopened_previous)
    );
    press_real_key(&mut app, window, KeyCode::Tab);
    assert_eq!(
        app.world().resource::<InputFocus>().get(),
        Some(reopened_next)
    );
    press_real_key(&mut app, window, KeyCode::Space);
    assert_eq!(app.world().resource::<SelectedOutfit>().0.slot(0), 0);
    // The previous screen may disappear while this modal is open. Never
    // restore that now-invalid entity when the creator closes again.
    app.world_mut().entity_mut(background).despawn();
    app.world_mut().resource_mut::<HeroCreatorOpen>().0 = false;
    app.update();
    assert_eq!(app.world().resource::<InputFocus>().get(), None);
}

#[test]
fn blank_click_recovers_modal_focus_but_cleanup_respects_another_screen() {
    let mut app = app();
    app.init_resource::<CreatorFocus>()
        .add_systems(Update, release_creator_focus)
        .add_systems(PostUpdate, initialize_creator_focus);
    let (_, previous, _, _) = focus_fixture(&mut app);
    app.update();
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(previous));
    app.world_mut().resource_mut::<InputFocus>().clear();
    app.update();
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(previous));
    let other_control = app.world_mut().spawn(TabIndex(0)).id();
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(other_control, FocusCause::Navigated);
    app.update();
    assert_eq!(
        app.world().resource::<InputFocus>().get(),
        Some(other_control)
    );
    app.world_mut().resource_mut::<HeroCreatorOpen>().0 = false;
    app.update();
    assert_eq!(
        app.world().resource::<InputFocus>().get(),
        Some(other_control)
    );
}
