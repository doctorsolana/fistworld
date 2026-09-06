use super::view::spawn_entries;
use super::*;
use crate::ui::foundation::UiButtonStyle;

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn sync_army_panel(
    mut commands: Commands,
    roster: Res<ArmyRoster>,
    mut state: ResMut<ArmyManagement>,
    time: Res<Time>,
    mut hosts: Query<(Entity, &ListKind, &mut ListSignature)>,
    mut texts: Query<(&BoundText, &mut Text)>,
    added: Query<(), Added<BoundText>>,
    mut buttons: Query<(
        Entity,
        &ArmyAction,
        &mut UiButtonStyle,
        Has<InteractionDisabled>,
        &mut Node,
    )>,
    perf: Res<crate::ui::perf::UiPerf>,
) {
    if state.pending_until > 0.0 && time.elapsed_secs() >= state.pending_until {
        state.pending_until = 0.0;
    }
    if !roster.is_changed() && !state.is_changed() && added.is_empty() {
        return;
    }
    if let Some(after) = state.select_new_after {
        if let Some(new) = roster.battalions.iter().find(|b| b.ordinal > after) {
            state.selected = Some(new.entity);
            state.select_new_after = None;
        }
    }
    if !roster
        .battalions
        .iter()
        .any(|b| Some(b.entity) == state.selected)
    {
        let next = roster.battalions.first().map(|b| b.entity);
        if next != state.selected {
            state.selected = next;
            state.confirm_disband = false;
        }
    }
    let model = PanelModel::new(&roster, &state);
    state.members.retain(|e| model.members.contains(e));
    state.available.retain(|e| model.available.contains(e));
    let pending = state.pending_until > time.elapsed_secs();
    let mut scope = perf.scope("sync_army_panel");
    for (slot, mut text) in &mut texts {
        let next = match *slot {
            BoundText::Summary => format!(
                "{} battalions / {} troops / {} unassigned",
                roster.battalions.len(),
                roster.soldiers.len(),
                roster
                    .soldiers
                    .values()
                    .filter(|s| s.battalion.is_none())
                    .count()
            ),
            BoundText::Title => model
                .unit
                .map_or("No battalion selected", |b| b.name.as_str())
                .into(),
            BoundText::Capacity => model.unit.map_or(
                "Create a battalion, then choose troops to add.".into(),
                |b| {
                    format!(
                        "{} / {} troops  /  {} free slots",
                        b.count, MAX_BATTALION_SIZE, model.room
                    )
                },
            ),
            BoundText::Policy => model
                .unit
                .map_or(
                    "Stances apply while idle; direct orders always take priority.",
                    |b| b.stance.description(),
                )
                .into(),
            BoundText::Notice => {
                if state.confirm_disband {
                    "Disband this battalion? Its troops stay in your army as unassigned.".into()
                } else if pending {
                    "Updating army...".into()
                } else if state.available.len() > model.room {
                    format!(
                        "Only {} free slots. Select fewer troops, or remove some members first.",
                        model.room
                    )
                } else {
                    "Choose rows for bulk changes, or use ADD / REMOVE for one troop.".into()
                }
            }
            BoundText::Members => format!("IN THIS BATTALION ({})", model.members.len()),
            BoundText::Available => format!("AVAILABLE TROOPS ({})", model.available.len()),
            BoundText::BattalionName(e) => roster
                .battalions
                .iter()
                .find(|b| b.entity == e)
                .map_or(String::new(), |b| b.name.clone()),
            BoundText::BattalionSummary(e) => roster
                .battalions
                .iter()
                .find(|b| b.entity == e)
                .map_or(String::new(), |b| {
                    format!(
                        "{} / {} troops\n{}",
                        b.count,
                        MAX_BATTALION_SIZE,
                        b.stance.label()
                    )
                }),
            BoundText::SoldierInfo(e) => roster.soldiers.get(&e).map_or(String::new(), |s| {
                let source = if !s.available {
                    "Aboard ship"
                } else {
                    s.battalion
                        .and_then(|id| roster.battalions.iter().find(|b| b.id == id))
                        .map_or("Unassigned", |b| b.name.as_str())
                };
                format!("STR {} / {:.0} HP / {source}", s.strength, s.current_health)
            }),
            BoundText::Button(action) => model.button(action, &state, &roster, pending).0,
        };
        if text.0 != next {
            text.0 = next;
        }
    }
    for (entity, action, mut style, disabled, mut node) in &mut buttons {
        if *action == ArmyAction::CancelDisband {
            let display = if state.confirm_disband {
                Display::Flex
            } else {
                Display::None
            };
            if node.display != display {
                node.display = display;
            }
        }
        let (_, enabled, selected) = model.button(*action, &state, &roster, pending);
        if style.selected != selected {
            style.selected = selected;
        }
        if enabled == disabled {
            if enabled {
                commands.entity(entity).remove::<InteractionDisabled>();
            } else {
                commands.entity(entity).insert(InteractionDisabled);
            }
        }
    }
    // Queue row replacement last: the button pass above can still need to
    // change InteractionDisabled on old rows. Despawning first would make
    // those deferred commands address dead entities when switching battalions.
    for (entity, kind, mut signature) in &mut hosts {
        let ids = match kind {
            ListKind::Battalions => roster.battalions.iter().map(|b| b.entity).collect(),
            ListKind::Members => model.members.clone(),
            ListKind::Available => model.available.clone(),
        };
        if signature.0.as_ref() != Some(&ids) {
            signature.0 = Some(ids.clone());
            commands
                .entity(entity)
                .despawn_children()
                .with_children(|p| spawn_entries(p, *kind, &ids));
            scope.rebuilt();
        }
    }
}
