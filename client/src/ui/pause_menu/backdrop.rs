//! Reuse the pre-UI world target behind the open menu, with one bounded soft filter.

use super::{actions, layout, PauseMenuOpen, PauseMenuRoot};
use crate::{
    input::InputState, render::systems::scaled_target::SceneRenderTarget,
    ui::startup::StartupBackdropMaterial,
};
use bevy::{
    prelude::*,
    ui::{RelativeCursorPosition, UiSystems},
    window::{CursorOptions, PrimaryWindow},
};

#[derive(Component)]
struct PauseBackdrop;

/// The transparent shared backdrop owns pointer input but is never a tabbable
/// menu action. A new menu must see a release before accepting an outside press.
#[derive(Component, Default)]
pub(super) struct OutsideDismiss {
    armed: bool,
}

struct ActiveBackdrop {
    root: Entity,
    node: Entity,
    material: Handle<StartupBackdropMaterial>,
    source: Handle<Image>,
    size: UVec2,
}

pub(super) fn install(app: &mut App) {
    // Target resizing belongs to Update. Material extraction sees this binding
    // refresh in the same frame, before the native-resolution UI is prepared.
    app.add_systems(PostUpdate, sync_backdrop.before(UiSystems::Prepare));
    app.add_systems(
        Update,
        dismiss_outside
            .after(actions::handle_escape_key)
            .after(actions::handle_pause_actions)
            .after(layout::spawn_pause_menu),
    );
}

fn dismiss_outside(
    mouse: Res<ButtonInput<MouseButton>>,
    mut open: ResMut<PauseMenuOpen>,
    mut input: ResMut<InputState>,
    mut targets: Query<(
        &mut OutsideDismiss,
        Ref<Interaction>,
        &RelativeCursorPosition,
    )>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut cursors: Query<&mut CursorOptions>,
) {
    for (mut target, interaction, cursor) in &mut targets {
        if !open.0 {
            target.armed = false;
            continue;
        }
        if !target.armed {
            target.armed =
                !mouse.pressed(MouseButton::Left) && !mouse.just_pressed(MouseButton::Left);
            continue;
        }
        if mouse.just_pressed(MouseButton::Left)
            && interaction.is_changed()
            && !interaction.is_added()
            && *interaction == Interaction::Pressed
            && cursor.cursor_over
        {
            open.0 = false;
            input.pause_menu_open = false;
            crate::ui::modal::sync_modal_cursor(false, &input, &windows, &mut cursors);
        }
    }
}

fn sync_backdrop(
    mut commands: Commands,
    roots: Query<Entity, With<PauseMenuRoot>>,
    nodes: Query<(), With<PauseBackdrop>>,
    target: Option<Res<SceneRenderTarget>>,
    images: Res<Assets<Image>>,
    mut image_events: MessageReader<AssetEvent<Image>>,
    mut materials: ResMut<Assets<StartupBackdropMaterial>>,
    mut active: Local<Option<ActiveBackdrop>>,
) {
    let root = roots.iter().next();
    let source = target.as_ref().and_then(|target| {
        images
            .get(&target.image)
            .map(|image| (&target.image, image.size()))
    });
    if active.as_ref().is_some_and(|backdrop| {
        Some(backdrop.root) != root || source.is_none() || !nodes.contains(backdrop.node)
    }) {
        let old = active.take().unwrap();
        materials.remove(old.material.id());
        if nodes.contains(old.node) {
            commands.entity(old.node).despawn();
        }
    }
    let (Some(root), Some((source, size))) = (root, source) else {
        image_events.clear();
        return;
    };

    // Images retain their handle across a resize, but their GPU texture view is
    // replaced. A material modification explicitly refreshes that bind group.
    let mut source_modified = false;
    for event in image_events.read() {
        if matches!(event, AssetEvent::Added { id } | AssetEvent::Modified { id } if *id == source.id())
        {
            source_modified = true;
        }
    }
    if let Some(backdrop) = active.as_mut() {
        if backdrop.source != *source || backdrop.size != size || source_modified {
            if let Some(mut material) = materials.get_mut(&backdrop.material) {
                *material = StartupBackdropMaterial::live_scene(source.clone());
            }
            backdrop.source = source.clone();
            backdrop.size = size;
        }
        return;
    }

    let material = materials.add(StartupBackdropMaterial::live_scene(source.clone()));
    let node = commands
        .spawn((
            PauseBackdrop,
            Name::new("pause-live-world-backdrop"),
            ChildOf(root),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            MaterialNode(material.clone()),
            // The root is transparent. The filter sits behind its menu siblings
            // while inheriting the modal's global layer above the ordinary HUD.
            ZIndex(-2),
            Pickable::IGNORE,
        ))
        .id();
    *active = Some(ActiveBackdrop {
        root,
        node,
        material,
        source: source.clone(),
        size,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> (App, Entity) {
        let mut app = App::new();
        app.insert_resource(PauseMenuOpen(true))
            .insert_resource(InputState {
                pause_menu_open: true,
                ..default()
            })
            .init_resource::<ButtonInput<MouseButton>>()
            .add_systems(Update, dismiss_outside);
        let target = app
            .world_mut()
            .spawn((
                OutsideDismiss::default(),
                crate::ui::modal::modal_backdrop_chrome(Color::NONE),
                crate::ui::foundation::surface_block(),
                RelativeCursorPosition {
                    cursor_over: true,
                    ..default()
                },
            ))
            .id();
        (app, target)
    }

    fn press(app: &mut App, target: Entity) {
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        *app.world_mut().get_mut::<Interaction>(target).unwrap() = Interaction::Pressed;
    }

    fn release(app: &mut App, target: Entity) {
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        *app.world_mut().get_mut::<Interaction>(target).unwrap() = Interaction::None;
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
    }

    #[test]
    fn opening_during_a_held_click_waits_for_release_and_a_fresh_outside_press() {
        let (mut app, target) = app();
        press(&mut app, target);
        app.update();
        assert!(app.world().resource::<PauseMenuOpen>().0);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        app.update();
        assert!(app.world().resource::<PauseMenuOpen>().0);
        release(&mut app, target);
        assert!(app.world().resource::<PauseMenuOpen>().0);
        assert!(app
            .world()
            .get::<bevy::input_focus::tab_navigation::TabIndex>(target)
            .is_none());
        press(&mut app, target);
        app.update();
        assert!(!app.world().resource::<PauseMenuOpen>().0);
        assert!(!app.world().resource::<InputState>().pause_menu_open);
    }

    #[test]
    fn dragging_a_held_panel_press_outside_does_not_dismiss() {
        let (mut app, target) = app();
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        // The press began over the panel, so its sibling backdrop wasn't hit.
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        *app.world_mut().get_mut::<Interaction>(target).unwrap() = Interaction::Pressed;
        app.update();
        assert!(app.world().resource::<PauseMenuOpen>().0);
        release(&mut app, target);
        assert!(app.world().resource::<PauseMenuOpen>().0);
    }
}
