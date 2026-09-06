use super::super::{ClickGuard, EncyclopediaOpen};
use super::*;
use crate::camera_rts::CommanderCamera;
use lightyear::prelude::{Connected, MessageSender};
use shared::protocol::{ArmyOrder, ReliableChannel};

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn handle_army_buttons(
    guard: Res<ClickGuard>,
    roster: Res<ArmyRoster>,
    mut state: ResMut<ArmyManagement>,
    time: Res<Time>,
    buttons: Query<
        (&Interaction, &ArmyAction),
        (Changed<Interaction>, Without<InteractionDisabled>),
    >,
    mut selection: ResMut<crate::selection::Selection>,
    mut open: ResMut<EncyclopediaOpen>,
    positions: Query<&PlayerPosition, With<Battalion>>,
    mut cameras: Query<&mut CommanderCamera>,
    mut senders: Query<&mut MessageSender<ArmyOrder>, (With<crate::GameClient>, With<Connected>)>,
) {
    if !guard.0 {
        return;
    }
    for (interaction, &action) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        debug!("Army control pressed: {action:?}");
        let model = PanelModel::new(&roster, &state);
        if !model
            .button(
                action,
                &state,
                &roster,
                time.elapsed_secs() < state.pending_until,
            )
            .1
        {
            continue;
        }
        match action {
            ArmyAction::Choose(entity) => {
                state.selected = Some(entity);
                state.members.clear();
                state.available.clear();
                state.confirm_disband = false;
            }
            ArmyAction::Source(other) => {
                state.other_battalions = other;
                state.available.clear();
            }
            ArmyAction::Toggle(entity) => {
                let selected = if model.members.contains(&entity) {
                    &mut state.members
                } else {
                    &mut state.available
                };
                if !selected.remove(&entity) {
                    selected.insert(entity);
                }
            }
            ArmyAction::SelectMembers => {
                let all = model
                    .members
                    .iter()
                    .filter(|e| roster.soldiers[e].available)
                    .copied()
                    .collect();
                if state.members == all {
                    state.members.clear();
                } else {
                    state.members = all;
                }
            }
            ArmyAction::SelectAvailable => {
                let all = model
                    .available
                    .iter()
                    .filter(|e| roster.soldiers[e].available)
                    .take(model.room)
                    .copied()
                    .collect();
                if state.available == all {
                    state.available.clear();
                } else {
                    state.available = all;
                }
            }
            ArmyAction::Disband if !state.confirm_disband => state.confirm_disband = true,
            ArmyAction::CancelDisband => state.confirm_disband = false,
            ArmyAction::SelectMap | ArmyAction::Locate => {
                let Some(unit) = model.unit else {
                    continue;
                };
                if action == ArmyAction::SelectMap {
                    selection.set(
                        unit.members
                            .iter()
                            .filter(|e| roster.soldiers[e].available)
                            .copied()
                            .collect(),
                    );
                } else if let Ok(position) = positions.get(unit.entity) {
                    for mut camera in &mut cameras {
                        camera.focus_target = position.0;
                        camera.zoom_target = 120.0_f32.clamp(camera.zoom_min, camera.zoom_max);
                    }
                }
                open.0 = false;
            }
            _ => {
                let Some(order) = model.command(action, &state, &roster) else {
                    continue;
                };
                if let Ok(mut sender) = senders.single_mut() {
                    debug!("Army control sending: {order:?}");
                    sender.send::<ReliableChannel>(order);
                    state.pending_until =
                        time.elapsed_secs() + if action == ArmyAction::New { 1.0 } else { 0.35 };
                    if action == ArmyAction::New {
                        state.select_new_after = Some(
                            roster
                                .battalions
                                .iter()
                                .map(|b| b.ordinal)
                                .max()
                                .unwrap_or(0),
                        );
                    }
                    state.members.clear();
                    state.available.clear();
                    state.confirm_disband = false;
                }
            }
        }
    }
}
