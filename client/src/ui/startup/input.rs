//! Hidden retained controls must leave native keyboard navigation as well as layout.

use bevy::{
    input_focus::{tab_navigation::TabIndex, FocusCause, InputFocus},
    prelude::*,
    ui::InteractionDisabled,
};

#[derive(Component)]
pub(super) struct StartupScreen;

/// Only restore disabling that this visibility owner installed itself.
#[derive(Component)]
pub(super) struct HiddenStartupControl;

pub(super) fn sync_visible_controls(
    mut commands: Commands,
    mut focus: ResMut<InputFocus>,
    buttons: Query<
        (
            Entity,
            Has<InteractionDisabled>,
            Has<HiddenStartupControl>,
            Has<TabIndex>,
        ),
        With<Button>,
    >,
    hierarchy: Query<(
        &Node,
        Option<&ChildOf>,
        Option<&Visibility>,
        Has<StartupScreen>,
    )>,
    fields: Query<Entity, With<super::widgets::StartupField>>,
) {
    let mut first_visible = None;
    let mut first_field = None;
    let mut focused_visible = false;
    let mut has_screen = false;
    for (entity, disabled, visibility_disabled, tabbable) in &buttons {
        let mut ancestor = Some(entity);
        let mut hidden = false;
        let mut in_startup = false;
        while let Some(current) = ancestor {
            let Ok((node, parent, visibility, screen)) = hierarchy.get(current) else {
                break;
            };
            hidden |= node.display == Display::None || visibility == Some(&Visibility::Hidden);
            if screen {
                in_startup = true;
                break;
            }
            ancestor = parent.map(ChildOf::parent);
        }
        if !in_startup {
            continue;
        }
        has_screen = true;
        if hidden {
            if tabbable {
                commands.entity(entity).remove::<TabIndex>();
            }
            if !disabled {
                commands
                    .entity(entity)
                    .insert((InteractionDisabled, HiddenStartupControl));
            }
            continue;
        }
        if visibility_disabled {
            commands
                .entity(entity)
                .remove::<(InteractionDisabled, HiddenStartupControl)>();
        } else if disabled {
            continue;
        }
        if !tabbable {
            commands.entity(entity).insert(TabIndex(0));
        }
        first_visible.get_or_insert(entity);
        if fields.contains(entity) {
            first_field.get_or_insert(entity);
        }
        focused_visible |= focus.get() == Some(entity);
    }
    // Run after Update: activating a preset cannot reuse the same Enter to
    // activate the restored field, and busy phases focus their visible Cancel.
    if has_screen && !focused_visible {
        if let Some(entity) = first_field.or(first_visible) {
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
    fn hidden_ancestors_remove_tab_stops_and_restore_visible_focus() {
        let mut app = App::new();
        app.init_resource::<InputFocus>()
            .add_systems(Update, sync_visible_controls);
        let root = app.world_mut().spawn((StartupScreen, Node::default())).id();
        let form = app.world_mut().spawn((Node::default(), ChildOf(root))).id();
        let field = app
            .world_mut()
            .spawn((
                Button,
                Node::default(),
                super::super::widgets::StartupField,
                ChildOf(form),
            ))
            .id();
        let busy = app
            .world_mut()
            .spawn((
                Node {
                    display: Display::None,
                    ..default()
                },
                ChildOf(root),
            ))
            .id();
        let cancel = app
            .world_mut()
            .spawn((Button, Node::default(), ChildOf(busy)))
            .id();
        app.update();
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(field));
        assert!(app.world().get::<TabIndex>(field).is_some());
        assert!(app.world().get::<TabIndex>(cancel).is_none());
        assert!(app.world().get::<InteractionDisabled>(cancel).is_some());
        app.world_mut().get_mut::<Node>(form).unwrap().display = Display::None;
        app.world_mut().get_mut::<Node>(busy).unwrap().display = Display::Flex;
        app.update();
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(cancel));
        assert!(app.world().get::<TabIndex>(field).is_none());
        assert!(app.world().get::<InteractionDisabled>(cancel).is_none());
        app.world_mut().get_mut::<Node>(form).unwrap().display = Display::Flex;
        app.world_mut().get_mut::<Node>(busy).unwrap().display = Display::None;
        app.update();
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(field));
    }
}
