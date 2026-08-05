//! Encyclopedia input handling.
//!
//! Click handlers require the real mouse-down edge AND a click guard that only
//! arms once the button has been released since the window opened. That is
//! ordinary menu hygiene: the click that opens a window, or a button still
//! held as it appears, must not fall through onto whatever now sits under the
//! cursor.

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use lightyear::prelude::{Connected, MessageSender};

use shared::protocol::{ReliableChannel, RequestCharacterRoster};

use super::*;
use crate::input::InputState;
use crate::ui::modal::{handle_backdrop_pressed, update_modal_click_guard};

/// N opens/closes. Gated on `ui_blocking` so it never fights another modal,
/// and on the encyclopedia's own flag so it can always close itself.
pub(super) fn toggle_encyclopedia(
    keyboard: Res<ButtonInput<KeyCode>>,
    input_state: Res<InputState>,
    mut open: ResMut<EncyclopediaOpen>,
) {
    if !keyboard.just_pressed(KeyCode::KeyN) {
        return;
    }
    if open.0 {
        open.0 = false;
    } else if !input_state.ui_blocking() {
        open.0 = true;
    }
}

/// Keep the click guard current; must run before any click handler.
pub(super) fn update_click_guard(
    open: Res<EncyclopediaOpen>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut guard: ResMut<ClickGuard>,
) {
    let armed = update_modal_click_guard(open.0, &mouse, &mut guard.0);
    debug_assert!(armed == guard.0);
}

/// Ask the server for the roster once per open, so levels are never stale.
pub(super) fn request_roster_on_open(
    mut people: ResMut<KnownPeople>,
    mut senders: Query<
        &mut MessageSender<RequestCharacterRoster>,
        (With<crate::GameClient>, With<Connected>),
    >,
) {
    if people.requested {
        return;
    }
    let Ok(mut sender) = senders.single_mut() else {
        // Offline (capture tool, or not connected yet) — leave it unrequested
        // so it retries if a connection appears while the window is open.
        return;
    };
    sender.send::<ReliableChannel>(RequestCharacterRoster);
    people.requested = true;
}

pub(super) fn handle_tab_buttons(
    guard: Res<ClickGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut tab: ResMut<EncyclopediaTab>,
    buttons: Query<(&Interaction, &TabButton), Changed<Interaction>>,
) {
    if !guard.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (interaction, TabButton(target)) in buttons.iter() {
        if *interaction == Interaction::Pressed && *tab != *target {
            *tab = *target;
        }
    }
}

pub(super) fn handle_filter_buttons(
    guard: Res<ClickGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut filter: ResMut<PeopleFilter>,
    buttons: Query<(&Interaction, &FilterButton), Changed<Interaction>>,
) {
    if !guard.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (interaction, FilterButton(target)) in buttons.iter() {
        if *interaction == Interaction::Pressed && *filter != *target {
            *filter = *target;
        }
    }
}

pub(super) fn handle_person_rows(
    guard: Res<ClickGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut selected: ResMut<SelectedPerson>,
    rows: Query<(&Interaction, &PersonRow), Changed<Interaction>>,
) {
    if !guard.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (interaction, PersonRow(name)) in rows.iter() {
        // The empty-state row carries the marker so it gets cleaned up, but it
        // names nobody -- clicking it must not select a person who is not there.
        if *interaction == Interaction::Pressed && !name.is_empty() {
            selected.0 = Some(name.clone());
        }
    }
}

const SCROLL_LINE_HEIGHT: f32 = 28.0;

/// Send wheel input into the UI hierarchy under the pointer.
///
/// This follows Bevy 0.19's official scroll example: the event starts at the
/// hovered leaf and bubbles until a scrollable ancestor consumes it. That is
/// what lets the file tree and detail sheet scroll independently instead of a
/// global wheel handler guessing which pane the player meant.
pub(super) fn send_scroll_events(
    mut wheel: MessageReader<MouseWheel>,
    hover_map: Res<HoverMap>,
    mut commands: Commands,
) {
    for event in wheel.read() {
        let mut delta = -Vec2::new(event.x, event.y);
        if event.unit == MouseScrollUnit::Line {
            delta *= SCROLL_LINE_HEIGHT;
        }
        for pointer_map in hover_map.values() {
            for entity in pointer_map.keys().copied() {
                commands.trigger(EncyclopediaScroll { entity, delta });
            }
        }
    }
}

/// Wheel delta in logical UI pixels, bubbling toward a scrollable ancestor.
#[derive(EntityEvent, Debug)]
#[entity_event(propagate, auto_propagate)]
pub(super) struct EncyclopediaScroll {
    entity: Entity,
    delta: Vec2,
}

pub(super) fn on_scroll(
    mut event: On<EncyclopediaScroll>,
    mut nodes: Query<(&mut ScrollPosition, &Node, &ComputedNode)>,
) {
    let Ok((mut position, node, computed)) = nodes.get_mut(event.entity) else {
        return;
    };
    let max_offset = (computed.content_size() - computed.size()) * computed.inverse_scale_factor();
    let delta = &mut event.delta;

    if node.overflow.x == OverflowAxis::Scroll && delta.x != 0.0 {
        let at_edge = if delta.x > 0.0 {
            position.x >= max_offset.x
        } else {
            position.x <= 0.0
        };
        if !at_edge {
            position.x = (position.x + delta.x).clamp(0.0, max_offset.x.max(0.0));
            delta.x = 0.0;
        }
    }
    if node.overflow.y == OverflowAxis::Scroll && delta.y != 0.0 {
        let at_edge = if delta.y > 0.0 {
            position.y >= max_offset.y
        } else {
            position.y <= 0.0
        };
        if !at_edge {
            position.y = (position.y + delta.y).clamp(0.0, max_offset.y.max(0.0));
            delta.y = 0.0;
        }
    }
    if *delta == Vec2::ZERO {
        event.propagate(false);
    }
}

pub(super) fn close_on_escape_or_backdrop(
    keyboard: Res<ButtonInput<KeyCode>>,
    guard: Res<ClickGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    backdrop: Query<&Interaction, (With<EncyclopediaBackdrop>, Changed<Interaction>)>,
    close_button: Query<&Interaction, (With<EncyclopediaCloseButton>, Changed<Interaction>)>,
    mut open: ResMut<EncyclopediaOpen>,
) {
    let clicked = guard.0 && mouse.just_pressed(MouseButton::Left);
    let clicked_out = clicked && handle_backdrop_pressed(&backdrop);
    let clicked_close = clicked
        && close_button
            .iter()
            .any(|interaction| *interaction == Interaction::Pressed);
    if keyboard.just_pressed(KeyCode::Escape) || clicked_out || clicked_close {
        open.0 = false;
    }
}

/// God-only: step the selected person's banner.
///
/// Sends intent and waits for the server to replicate the result back, exactly
/// like every other world change. Setting the local record directly would show
/// a banner the world has not agreed to, and affiliation decides who is hostile
/// to whom -- that is not a thing to guess at locally.
pub(super) fn handle_banner_buttons(
    guard: Res<ClickGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    god: Res<crate::ui::hud::GodCapability>,
    selected: Res<SelectedPerson>,
    people: Res<KnownPeople>,
    buttons: Query<(&Interaction, &BannerButton), Changed<Interaction>>,
    mut senders: Query<
        &mut MessageSender<shared::protocol::DevCommand>,
        (With<crate::GameClient>, With<Connected>),
    >,
) {
    if !guard.0 || !god.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(name) = selected.0.clone() else {
        return;
    };
    let Some(record) = people.find(&name) else {
        return;
    };
    for (interaction, BannerButton(step)) in buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let next = record.affiliation.cycled(*step);
        if let Ok(mut sender) = senders.single_mut() {
            sender.send::<ReliableChannel>(shared::protocol::DevCommand::SetAffiliation {
                character: name.clone(),
                banner: next.0,
            });
        }
    }
}

/// God-only: take the selected villager into your retinue, or dismiss it.
///
/// Sends intent and waits for replication, like every other world change: a
/// locally-flipped retinue would show a unit as commandable that the server has
/// not agreed to, and the next order would silently do nothing -- which is
/// exactly the failure this whole feature exists to remove.
pub(super) fn handle_retinue_button(
    guard: Res<ClickGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    god: Res<crate::ui::hud::GodCapability>,
    selected: Res<SelectedPerson>,
    people: Res<KnownPeople>,
    account: Option<Res<crate::ui::name_entry::PlayerNameInput>>,
    characters: Query<(Entity, &shared::components::CharacterName)>,
    buttons: Query<&Interaction, (With<RetinueButton>, Changed<Interaction>)>,
    mut senders: Query<
        &mut MessageSender<shared::protocol::DevCommand>,
        (With<crate::GameClient>, With<Connected>),
    >,
) {
    if !guard.0 || !god.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    if !buttons
        .iter()
        .any(|interaction| *interaction == Interaction::Pressed)
    {
        return;
    }
    let Some(name) = selected.0.clone() else {
        return;
    };
    let Some(record) = people.find(&name) else {
        return;
    };
    // Targeted by ENTITY, so this only works on someone you can currently see.
    // That is a real limitation and the honest one: generated names collide, and
    // conscripting the wrong namesake is worse than not offering the button.
    let Some((entity, _)) = characters.iter().find(|(_, n)| n.0 == name) else {
        return;
    };
    let my_account = account
        .as_ref()
        .map(|input| input.name.trim().to_lowercase())
        .unwrap_or_default();
    let already_mine = record.commanded_by.as_deref() == Some(my_account.as_str());
    if let Ok(mut sender) = senders.single_mut() {
        sender.send::<ReliableChannel>(shared::protocol::DevCommand::SetRetinue {
            unit: entity,
            commanded: !already_mine,
        });
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;

    use super::*;

    #[test]
    fn close_button_closes_the_encyclopedia() {
        let mut world = World::new();
        let mut mouse = ButtonInput::<MouseButton>::default();
        mouse.press(MouseButton::Left);
        world.insert_resource(mouse);
        world.insert_resource(ButtonInput::<KeyCode>::default());
        world.insert_resource(ClickGuard(true));
        world.insert_resource(EncyclopediaOpen(true));
        world.spawn((EncyclopediaBackdrop, Interaction::None));
        world.spawn((EncyclopediaCloseButton, Interaction::Pressed));

        world.run_system_once(close_on_escape_or_backdrop).unwrap();

        assert!(!world.resource::<EncyclopediaOpen>().0);
    }

    #[test]
    fn wheel_event_bubbles_to_and_scrolls_the_people_viewport() {
        let mut app = App::new();
        app.add_observer(on_scroll);

        let viewport = app
            .world_mut()
            .spawn((
                Node {
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
                ComputedNode {
                    size: Vec2::new(320.0, 100.0),
                    content_size: Vec2::new(320.0, 300.0),
                    inverse_scale_factor: 1.0,
                    ..default()
                },
            ))
            .id();
        let hovered_row = app
            .world_mut()
            .spawn((Node::default(), ChildOf(viewport)))
            .id();

        app.world_mut()
            .entity_mut(hovered_row)
            .trigger(|entity| EncyclopediaScroll {
                entity,
                delta: Vec2::new(0.0, SCROLL_LINE_HEIGHT),
            });

        assert_eq!(
            app.world().get::<ScrollPosition>(viewport).unwrap().y,
            SCROLL_LINE_HEIGHT
        );
    }
}
