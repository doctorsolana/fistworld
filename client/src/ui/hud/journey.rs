//! Bounded recent action notices shared by exploration and combat.
//!
//! The clock owns the bell's position; this module owns expansion, unread state
//! and the retained drawer. Character facts and navigation belong to the HUD shell.

mod notices;
mod view;

use super::GodNotice;
use crate::ui::foundation::UiButtonStyle;
use crate::{input::InputState, states::GameState};
use bevy::prelude::*;
use notices::Notices;

pub(super) use view::{notice_button, view};

#[derive(Component)]
struct NoticeButton;

#[derive(Component)]
struct JourneyDetails;

#[derive(Component)]
enum JourneyText {
    Unread,
    Message(usize),
}

#[derive(Component, Clone, Copy)]
enum JourneyAction {
    Toggle,
    Clear,
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<Notices>();
    app.add_systems(OnEnter(GameState::Playing), reset);
    app.add_systems(Update, collect_notices.run_if(in_state(GameState::Playing)));
    app.add_systems(
        Update,
        (handle_actions, bind.after(collect_notices))
            .chain()
            .run_if(in_state(GameState::Playing)),
    );
}

fn reset(mut notices: ResMut<Notices>, notice: Res<GodNotice>) {
    *notices = Notices::default();
    // Ignore the previous connection's last action result.
    notices.observe(notice.sequence, "");
}

fn collect_notices(notice: Res<GodNotice>, mut notices: ResMut<Notices>) {
    if !notices.has_seen(notice.sequence) {
        notices.observe(notice.sequence, &notice.text);
    }
}

fn available(input: &InputState, opening: Option<&crate::boat::OpeningCinematic>) -> bool {
    !input.ui_blocking() && !opening.is_some_and(|opening| opening.is_active())
}

fn handle_actions(
    input: Res<InputState>,
    opening: Option<Res<crate::boat::OpeningCinematic>>,
    buttons: Query<
        (&Interaction, &JourneyAction),
        (Changed<Interaction>, Without<bevy::ui::InteractionDisabled>),
    >,
    mut notices: ResMut<Notices>,
) {
    if !available(&input, opening.as_deref()) {
        return;
    }
    for (interaction, action) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match action {
            JourneyAction::Toggle => notices.toggle(),
            JourneyAction::Clear => notices.clear(),
        }
    }
}

#[allow(clippy::type_complexity)]
fn bind(
    input: Res<InputState>,
    opening: Option<Res<crate::boat::OpeningCinematic>>,
    mut notices: ResMut<Notices>,
    mut bell: Query<
        (&mut Node, &mut UiButtonStyle),
        (
            With<NoticeButton>,
            Without<JourneyDetails>,
            Without<JourneyText>,
        ),
    >,
    mut details: Query<
        (&mut Node, &ComputedNode, &InheritedVisibility),
        (
            With<JourneyDetails>,
            Without<NoticeButton>,
            Without<JourneyText>,
        ),
    >,
    mut labels: Query<
        (&JourneyText, &mut Text, &mut Node),
        (Without<NoticeButton>, Without<JourneyDetails>),
    >,
    clear_buttons: Query<(Entity, &JourneyAction, Has<bevy::ui::InteractionDisabled>)>,
    mut commands: Commands,
) {
    let showing = available(&input, opening.as_deref());
    for (mut node, mut style) in &mut bell {
        set_display(&mut node, showing);
        if style.selected != notices.expanded {
            style.selected = notices.expanded;
        }
    }
    let mut drawer_visible = false;
    for (mut node, computed, inherited) in &mut details {
        let expanded = showing && notices.expanded;
        // The prior frame must actually have laid out a visible drawer. A
        // queued expansion behind a modal cannot silently consume an alert.
        drawer_visible |= expanded
            && node.display != Display::None
            && computed.size().min_element() > 0.0
            && inherited.get();
        set_display(&mut node, expanded);
    }
    if drawer_visible && notices.unread() > 0 {
        notices.mark_read();
    }
    if !notices.is_changed() {
        return;
    }
    for (field, mut text, mut node) in &mut labels {
        match field {
            JourneyText::Unread => {
                let count = notices.unread();
                set_display(&mut node, count > 0);
                if count > 0 {
                    set_text(&mut text, &count.to_string());
                }
            }
            JourneyText::Message(index) => {
                let message = notices.entry(*index);
                set_display(&mut node, *index == 0 || message.is_some());
                set_text(&mut text, message.unwrap_or("No recent messages."));
            }
        }
    }
    for (entity, action, disabled) in &clear_buttons {
        if !matches!(action, JourneyAction::Clear) {
            continue;
        }
        let empty = notices.entry(0).is_none();
        if empty != disabled {
            if empty {
                commands
                    .entity(entity)
                    .insert(bevy::ui::InteractionDisabled);
            } else {
                commands
                    .entity(entity)
                    .remove::<bevy::ui::InteractionDisabled>();
            }
        }
    }
}

fn set_text(text: &mut Mut<Text>, value: &str) {
    if text.0 != value {
        text.0.clear();
        text.0.push_str(value);
    }
}

fn set_display(node: &mut Mut<Node>, showing: bool) {
    let display = if showing {
        Display::Flex
    } else {
        Display::None
    };
    if node.display != display {
        node.display = display;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notice_countdown_does_not_dirty_retained_history() {
        let mut app = App::new();
        app.init_resource::<GodNotice>()
            .init_resource::<Notices>()
            .add_systems(Update, collect_notices);
        app.world_mut().resource_mut::<GodNotice>().show("Moving");
        app.update();
        app.world_mut().clear_trackers();
        app.world_mut().resource_mut::<GodNotice>().seconds_left -= 0.1;
        app.update();
        assert!(
            !app.world()
                .get_resource_ref::<Notices>()
                .unwrap()
                .is_changed()
        );
        assert_eq!(app.world().resource::<Notices>().unread(), 1);
    }

    #[test]
    fn bell_works_in_combat_without_a_hero_and_respects_modal_input() {
        let mut app = App::new();
        app.init_resource::<InputState>()
            .init_resource::<Notices>()
            .insert_resource(crate::combat_mode::CombatMode(true))
            .add_systems(Update, handle_actions);
        let button = app
            .world_mut()
            .spawn((JourneyAction::Toggle, Interaction::Pressed))
            .id();
        app.world_mut()
            .resource_mut::<Notices>()
            .observe(1, "Hold position");
        app.update();
        assert!(app.world().resource::<Notices>().expanded);
        assert_eq!(
            app.world().resource::<Notices>().unread(),
            1,
            "opening alone does not prove a message was visible"
        );
        app.world_mut().resource_mut::<InputState>().modal_open = true;
        app.world_mut()
            .entity_mut(button)
            .insert(Interaction::Pressed);
        app.update();
        assert!(app.world().resource::<Notices>().expanded);
    }

    #[test]
    fn unread_messages_wait_for_visible_drawer_layout() {
        let mut app = App::new();
        app.init_resource::<InputState>()
            .init_resource::<Notices>()
            .add_systems(Update, bind);
        let details = app
            .world_mut()
            .spawn((
                JourneyDetails,
                Node {
                    display: Display::None,
                    ..default()
                },
                ComputedNode::default(),
                InheritedVisibility::VISIBLE,
            ))
            .id();
        {
            let mut notices = app.world_mut().resource_mut::<Notices>();
            notices.observe(1, "A battalion is under attack");
            notices.toggle();
        }
        app.update();
        assert_eq!(app.world().resource::<Notices>().unread(), 1);
        app.world_mut().entity_mut(details).insert(ComputedNode {
            size: Vec2::new(340.0, 156.0),
            ..default()
        });
        app.world_mut().resource_mut::<InputState>().map_open = true;
        app.update();
        assert_eq!(app.world().resource::<Notices>().unread(), 1);
        app.world_mut().resource_mut::<InputState>().map_open = false;
        app.update();
        assert_eq!(app.world().resource::<Notices>().unread(), 1);
        app.update();
        assert_eq!(app.world().resource::<Notices>().unread(), 0);
    }
}
