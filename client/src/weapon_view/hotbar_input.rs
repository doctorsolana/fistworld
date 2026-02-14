use bevy::prelude::*;
use lightyear::prelude::*;
use shared::components::LocalPlayer;
use shared::items::{HotbarSelection, SelectHotbarSlot};
use shared::protocol::ReliableChannel;

use crate::input::InputState;

/// Handle weapon switching with number keys.
pub fn handle_weapon_switch(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut client_query: Query<
        &mut MessageSender<SelectHotbarSlot>,
        (With<crate::GameClient>, With<Connected>),
    >,
    mut local_hotbar: Query<&mut HotbarSelection, With<LocalPlayer>>,
    input_state: Res<InputState>,
) {
    // Don't switch while input is blocked by a modal UI or while driving.
    if input_state.in_vehicle || input_state.ui_blocking() {
        return;
    }

    let new_index: Option<u8> = if keyboard.just_pressed(KeyCode::Digit1) {
        Some(0)
    } else if keyboard.just_pressed(KeyCode::Digit2) {
        Some(1)
    } else if keyboard.just_pressed(KeyCode::Digit3) {
        Some(2)
    } else if keyboard.just_pressed(KeyCode::Digit4) {
        Some(3)
    } else if keyboard.just_pressed(KeyCode::Digit5) {
        Some(4)
    } else if keyboard.just_pressed(KeyCode::Digit6) {
        Some(5)
    } else {
        None
    };

    let Some(index) = new_index else { return };

    // Optimistic local update (UI highlight feels instant).
    if let Ok(mut hotbar) = local_hotbar.single_mut() {
        hotbar.index = index;
    }

    if let Ok(mut sender) = client_query.single_mut() {
        sender.send::<ReliableChannel>(SelectHotbarSlot { index });
    }
}
