//! Keyboard activation and visibility for retained menu pages.
use super::*;
use bevy::{
    input::InputSystems,
    input_focus::{tab_navigation::TabIndex, FocusCause, InputFocus},
    ui::{InteractionDisabled, UiSystems},
};

#[derive(Component)]
pub(super) struct HiddenMenuControl;

/// A separate reason for disabling a control, preserved across page changes.
#[derive(Component)]
pub(super) struct SettingUnavailable;

pub(super) fn sync_unavailable(
    commands: &mut Commands,
    entity: Entity,
    unavailable: bool,
    was_unavailable: bool,
    hidden: bool,
) {
    if unavailable && !was_unavailable {
        commands
            .entity(entity)
            .insert((SettingUnavailable, InteractionDisabled));
    } else if !unavailable && was_unavailable {
        commands.entity(entity).remove::<SettingUnavailable>();
        if !hidden {
            commands.entity(entity).remove::<InteractionDisabled>();
        }
    }
}

pub(super) fn install(app: &mut App) {
    app.add_systems(
        PreUpdate,
        keyboard_activation
            .after(InputSystems)
            .after(UiSystems::Focus)
            .run_if(pause_menu_open),
    );
    app.add_systems(
        PostUpdate,
        sync_visible_controls
            .after(UiSystems::Layout)
            .run_if(pause_menu_open),
    );
}

/// Only graphics steppers/choices need this adapter. Navigation and audio already
/// consume native activation keys in their respective action owners.
fn keyboard_activation(
    keyboard: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    mut buttons: Query<
        &mut Interaction,
        (
            Without<InteractionDisabled>,
            Or<(
                With<GraphicsToggle>,
                With<SliderStep>,
                With<InputSliderStep>,
                With<display::SelectDisplayMode>,
                With<DisplayConfirmationAction>,
            )>,
        ),
    >,
    mut release: Local<Option<Entity>>,
) {
    if let Some(entity) = release.take() {
        if let Ok(mut interaction) = buttons.get_mut(entity) {
            *interaction = Interaction::None;
        }
    }
    if !keyboard.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space]) {
        return;
    }
    if let Some(entity) = focus.get() {
        if let Ok(mut interaction) = buttons.get_mut(entity) {
            *interaction = Interaction::Pressed;
            *release = Some(entity);
        }
    }
}

fn sync_visible_controls(
    mut commands: Commands,
    mut focus: ResMut<InputFocus>,
    opened: Query<(), Added<PauseMenuRoot>>,
    buttons: Query<
        (
            Entity,
            Has<InteractionDisabled>,
            Has<HiddenMenuControl>,
            Has<TabIndex>,
            Has<SettingUnavailable>,
            Option<&Name>,
        ),
        With<UiButtonStyle>,
    >,
    hierarchy: Query<(
        &Node,
        Option<&ChildOf>,
        Option<&Visibility>,
        Has<PauseMenuRoot>,
    )>,
) {
    let mut first = None;
    let mut resume = None;
    let mut focused_visible = false;
    let mut any = false;
    for (entity, disabled, ours, tabbable, unavailable, name) in &buttons {
        let mut current = Some(entity);
        let mut hidden = false;
        let mut in_menu = false;
        while let Some(ancestor) = current {
            let Ok((node, parent, visibility, root)) = hierarchy.get(ancestor) else {
                break;
            };
            hidden |= node.display == Display::None || visibility == Some(&Visibility::Hidden);
            if root {
                in_menu = true;
                break;
            }
            current = parent.map(ChildOf::parent);
        }
        if !in_menu {
            continue;
        }
        any = true;
        if hidden {
            if tabbable {
                commands.entity(entity).remove::<TabIndex>();
            }
            if !ours {
                commands
                    .entity(entity)
                    .insert((InteractionDisabled, HiddenMenuControl));
            }
            continue;
        }
        if ours {
            commands.entity(entity).remove::<HiddenMenuControl>();
            if !unavailable {
                commands.entity(entity).remove::<InteractionDisabled>();
            }
        } else if disabled {
            continue;
        }
        if unavailable {
            continue;
        }
        if !tabbable {
            commands.entity(entity).insert(TabIndex(0));
        }
        first.get_or_insert(entity);
        if name.is_some_and(|name| name.as_str() == "pause-RESUME") {
            resume = Some(entity);
        }
        focused_visible |= focus.get() == Some(entity);
    }
    if any && (!focused_visible || !opened.is_empty()) {
        if let Some(entity) = resume.or(first) {
            focus.set(entity, FocusCause::Navigated);
        } else {
            focus.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_is_the_default_focus_even_when_exit_was_spawned_first() {
        let mut app = App::new();
        app.init_resource::<InputFocus>()
            .add_systems(Update, sync_visible_controls);
        let root = app.world_mut().spawn((PauseMenuRoot, Node::default())).id();
        app.world_mut().spawn((
            Button,
            UiButtonStyle::default(),
            Name::new("pause-EXIT GAME"),
            ChildOf(root),
        ));
        let resume = app
            .world_mut()
            .spawn((
                Button,
                UiButtonStyle::default(),
                Name::new("pause-RESUME"),
                ChildOf(root),
            ))
            .id();
        app.update();
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(resume));
    }

    #[test]
    fn a_settings_limit_survives_hiding_and_showing_its_page() {
        let mut app = App::new();
        app.init_resource::<InputFocus>()
            .init_resource::<GraphicsSettings>()
            .init_resource::<InputSettings>()
            .add_systems(
                Update,
                (sliders::sync_slider_controls, sync_visible_controls).chain(),
            );
        app.world_mut()
            .resource_mut::<InputSettings>()
            .mouse_sensitivity = 0.1;
        let root = app.world_mut().spawn((PauseMenuRoot, Node::default())).id();
        let page = app
            .world_mut()
            .spawn((
                ControlsSettingsPanel,
                Node {
                    display: Display::None,
                    ..default()
                },
                ChildOf(root),
            ))
            .id();
        let lower = app
            .world_mut()
            .spawn((
                Button,
                UiButtonStyle::default(),
                InputSliderStep {
                    control: InputSliderControl::MouseSensitivity,
                    delta: -1,
                },
                ChildOf(page),
            ))
            .id();
        app.update();
        assert!(app.world().get::<SettingUnavailable>(lower).is_some());
        app.world_mut().get_mut::<Node>(page).unwrap().display = Display::Flex;
        app.update();
        assert!(app.world().get::<InteractionDisabled>(lower).is_some());
        assert!(app.world().get::<TabIndex>(lower).is_none());
        assert_ne!(app.world().resource::<InputFocus>().get(), Some(lower));
        app.world_mut().get_mut::<Node>(page).unwrap().display = Display::None;
        app.update();
        app.world_mut()
            .resource_mut::<InputSettings>()
            .mouse_sensitivity = 1.0;
        app.update();
        assert!(app.world().get::<SettingUnavailable>(lower).is_none());
        assert!(app.world().get::<InteractionDisabled>(lower).is_some());
        app.world_mut().get_mut::<Node>(page).unwrap().display = Display::Flex;
        app.update();
        assert!(app.world().get::<InteractionDisabled>(lower).is_none());
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(lower));
    }

    #[test]
    fn changing_page_removes_hidden_controls_from_keyboard_focus() {
        let mut app = App::new();
        app.init_resource::<InputFocus>()
            .add_systems(Update, sync_visible_controls);
        let root = app.world_mut().spawn((PauseMenuRoot, Node::default())).id();
        let first = app.world_mut().spawn((Node::default(), ChildOf(root))).id();
        let second = app
            .world_mut()
            .spawn((
                Node {
                    display: Display::None,
                    ..default()
                },
                ChildOf(root),
            ))
            .id();
        let old = app
            .world_mut()
            .spawn((Button, UiButtonStyle::default(), ChildOf(first)))
            .id();
        let new = app
            .world_mut()
            .spawn((Button, UiButtonStyle::default(), ChildOf(second)))
            .id();
        app.update();
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(old));
        assert!(app.world().get::<InteractionDisabled>(new).is_some());
        app.world_mut().get_mut::<Node>(first).unwrap().display = Display::None;
        app.world_mut().get_mut::<Node>(second).unwrap().display = Display::Flex;
        app.update();
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(new));
        assert!(app.world().get::<TabIndex>(old).is_none());
        assert!(app.world().get::<InteractionDisabled>(new).is_none());
    }
}
