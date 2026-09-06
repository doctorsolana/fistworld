//! Command hotkeys, remembered control groups and authoritative feedback.

use super::Selection;
use crate::army_roster::ArmyRoster;
use bevy::prelude::*;
use lightyear::prelude::{Connected, MessageReceiver, MessageSender};
use shared::protocol::*;

#[derive(Resource, Default)]
pub struct CommandMode(pub MovementMode);

#[derive(Resource, Default)]
pub struct ControlGroups([UnitSelection; 10]);

pub fn handle_command_keys(
    keys: Res<ButtonInput<KeyCode>>,
    machines: Query<
        (
            &shared::components::CommandedBy,
            &shared::components::Health,
        ),
        With<shared::components::Catapult>,
    >,
    input: Res<crate::input::InputState>,
    placement: Res<crate::hero::control::WorldPlacementMode>,
    combat: Res<crate::combat_mode::CombatMode>,
    roster: Res<ArmyRoster>,
    mut mode: ResMut<CommandMode>,
    mut groups: ResMut<ControlGroups>,
    mut selection: ResMut<Selection>,
    mut senders: Query<&mut MessageSender<UnitOrder>, (With<crate::GameClient>, With<Connected>)>,
    mut notice: ResMut<crate::ui::hud::GodNotice>,
) {
    if !combat.0 {
        if mode.0 != MovementMode::Move {
            mode.0 = MovementMode::Move;
        }
        return;
    }
    if input.ui_blocking() || crate::hero::control::placement_armed(&placement) {
        return;
    }
    if keys.just_pressed(KeyCode::KeyX) {
        mode.0 = MovementMode::AttackMove;
        notice.show("Attack-move: right-click a destination");
    }
    if keys.just_pressed(KeyCode::KeyR) {
        mode.0 = MovementMode::Retreat;
        notice.show("Retreat: right-click a destination");
    }
    if keys.just_pressed(KeyCode::Escape) {
        mode.0 = MovementMode::Move;
    }
    if keys.just_pressed(KeyCode::KeyH) && !selection.is_empty() {
        if let Ok(mut sender) = senders.single_mut() {
            sender.send::<ReliableChannel>(UnitOrder {
                selection: roster.selection(&selection.entities),
                command: UnitCommand::Hold,
            });
            mode.0 = MovementMode::Move;
        }
    }
    let digits = [
        KeyCode::Digit0,
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];
    for (index, key) in digits.into_iter().enumerate() {
        if !keys.just_pressed(key) {
            continue;
        }
        if keys.any_pressed([
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
            KeyCode::SuperLeft,
            KeyCode::SuperRight,
        ]) {
            groups.0[index] = roster.selection(&selection.entities);
            notice.show(&format!("Group {index} saved: {} units", selection.len()));
        } else {
            let mut members = roster.resolve(&groups.0[index]);
            members.extend(groups.0[index].units.iter().copied().filter(|e| {
                machines
                    .get(*e)
                    .is_ok_and(|(o, h)| o.0 == roster.account && !h.is_dead())
            }));
            if !members.is_empty() {
                selection.apply_group(
                    members,
                    keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
                    false,
                );
            }
        }
    }
}

pub fn receive_order_feedback(
    mut receivers: Query<
        &mut MessageReceiver<ArmyOrderFeedback>,
        (With<crate::GameClient>, With<Connected>),
    >,
    mut notice: ResMut<crate::ui::hud::GodNotice>,
) {
    for mut receiver in &mut receivers {
        for result in receiver.receive() {
            notice.show(&result.message);
        }
    }
}

pub fn reset_commands(mut groups: ResMut<ControlGroups>, mut mode: ResMut<CommandMode>) {
    *groups = ControlGroups::default();
    *mode = CommandMode::default();
}
