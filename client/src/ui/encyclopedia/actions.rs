//! Encyclopedia input handling.
//!
//! Click handlers require the real mouse-down edge AND a click guard that only
//! arms once the button has been released since the window opened. That is
//! ordinary menu hygiene: the click that opens a window, or a button still
//! held as it appears, must not fall through onto whatever now sits under the
//! cursor.

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use lightyear::prelude::{Connected, MessageSender};

use shared::protocol::{ReliableChannel, RequestPlayerRoster};

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
        &mut MessageSender<RequestPlayerRoster>,
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
    sender.send::<ReliableChannel>(RequestPlayerRoster);
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
        if *interaction == Interaction::Pressed {
            selected.0 = Some(name.clone());
        }
    }
}

/// Mouse wheel scrolls the people list. Bevy 0.19 gives the scrolling
/// viewport (`Overflow::scroll_y`) but not the input that drives it.
pub(super) fn scroll_people_list(
    mut wheel: MessageReader<MouseWheel>,
    mut viewports: Query<(&mut ScrollPosition, &ComputedNode), With<PeopleListViewport>>,
    content: Query<&ComputedNode, With<PeopleListContent>>,
) {
    let mut delta = 0.0;
    for event in wheel.read() {
        delta += match event.unit {
            // Line deltas are ~1 per notch; give them a usable row-sized step.
            MouseScrollUnit::Line => event.y * 28.0,
            MouseScrollUnit::Pixel => event.y,
        };
    }
    if delta == 0.0 {
        return;
    }
    for (mut scroll, viewport) in viewports.iter_mut() {
        // Clamp so the list cannot be flung past its own content.
        let content_height = content
            .iter()
            .map(|node| node.size().y)
            .fold(0.0_f32, f32::max);
        let max_scroll = (content_height - viewport.size().y).max(0.0);
        let next = (scroll.0.y - delta).clamp(0.0, max_scroll);
        if scroll.0.y != next {
            scroll.0.y = next;
        }
    }
}

pub(super) fn close_on_escape_or_backdrop(
    keyboard: Res<ButtonInput<KeyCode>>,
    guard: Res<ClickGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    backdrop: Query<&Interaction, (With<EncyclopediaBackdrop>, Changed<Interaction>)>,
    mut open: ResMut<EncyclopediaOpen>,
) {
    let clicked_out =
        guard.0 && mouse.just_pressed(MouseButton::Left) && handle_backdrop_pressed(&backdrop);
    if keyboard.just_pressed(KeyCode::Escape) || clicked_out {
        open.0 = false;
    }
}
