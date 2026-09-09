use super::*;
use shared::protocol::ArmyOrder;
use std::collections::HashSet;

#[derive(Resource, Default)]
pub(crate) struct ArmyManagement {
    pub selected: Option<Entity>,
    pub members: HashSet<Entity>,
    pub available: HashSet<Entity>,
    pub other_battalions: bool,
    pub confirm_disband: bool,
    pub pending_until: f32,
    pub select_new_after: Option<u64>,
}
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ArmyAction {
    New,
    Choose(Entity),
    SelectMap,
    Locate,
    Disband,
    CancelDisband,
    Stance(BattalionStance),
    Role(SoldierRole),
    Fire(FirePolicy),
    Rearm,
    Source(bool),
    Toggle(Entity),
    SelectMembers,
    SelectAvailable,
    Add(Entity),
    Remove(Entity),
    AddChecked,
    RemoveChecked,
    Fill,
}
#[derive(Component, Clone, Copy)]
pub(crate) enum BoundText {
    Summary,
    Title,
    Capacity,
    Policy,
    Equipment,
    Notice,
    Members,
    Available,
    BattalionName(Entity),
    BattalionSummary(Entity),
    SoldierInfo(Entity),
    Button(ArmyAction),
}
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ListKind {
    Battalions,
    Members,
    Available,
}
#[derive(Component, Default)]
pub(crate) struct ListSignature(pub Option<Vec<Entity>>);

pub(super) struct PanelModel<'a> {
    pub unit: Option<&'a BattalionFacts>,
    pub members: Vec<Entity>,
    pub available: Vec<Entity>,
    pub reserves: Vec<Entity>,
    pub room: usize,
}
impl<'a> PanelModel<'a> {
    pub fn new(roster: &'a ArmyRoster, state: &ArmyManagement) -> Self {
        let unit = roster
            .battalions
            .iter()
            .find(|b| Some(b.entity) == state.selected);
        let mut members = Vec::new();
        let mut available = Vec::new();
        let mut reserves = Vec::new();
        for s in roster.soldiers.values() {
            let compatible = unit.is_none_or(|b| {
                b.role == s.role
                    || (b.role != SoldierRole::Cavalry && s.role != SoldierRole::Cavalry)
            });
            if s.battalion.is_none() && s.available && compatible {
                reserves.push(s.entity);
            }
            if unit.is_some_and(|b| s.battalion == Some(b.id)) {
                members.push(s.entity);
            } else if unit.is_some()
                && compatible
                && s.battalion.is_some() == state.other_battalions
            {
                available.push(s.entity);
            }
        }
        let sort = |v: &mut Vec<Entity>| {
            v.sort_by(|a, b| {
                let a = &roster.soldiers[a];
                let b = &roster.soldiers[b];
                b.available
                    .cmp(&a.available)
                    .then(b.strength.cmp(&a.strength))
                    .then(a.name.cmp(&b.name))
                    .then(a.identity.cmp(&b.identity))
            })
        };
        sort(&mut members);
        sort(&mut available);
        sort(&mut reserves);
        Self {
            room: unit.map_or(0, |b| MAX_BATTALION_SIZE.saturating_sub(b.count)),
            unit,
            members,
            available,
            reserves,
        }
    }
    pub fn checked(
        &self,
        state: &ArmyManagement,
        roster: &ArmyRoster,
        members: bool,
    ) -> Vec<Entity> {
        let (list, checked) = if members {
            (&self.members, &state.members)
        } else {
            (&self.available, &state.available)
        };
        list.iter()
            .filter(|e| checked.contains(e) && roster.soldiers[e].available)
            .copied()
            .collect()
    }
    pub fn command(
        &self,
        action: ArmyAction,
        state: &ArmyManagement,
        roster: &ArmyRoster,
    ) -> Option<ArmyOrder> {
        let assignment = |members: Vec<Entity>| {
            let battalion = self.unit?.entity;
            (!members.is_empty() && members.len() <= self.room)
                .then_some(ArmyOrder::Assign { battalion, members })
        };
        match action {
            ArmyAction::New => (roster.battalions.len() < MAX_BATTALIONS_PER_ACCOUNT)
                .then_some(ArmyOrder::Muster { members: vec![] }),
            ArmyAction::Role(role) if self.unit?.role != SoldierRole::Cavalry => {
                Some(ArmyOrder::SetRole {
                    battalion: self.unit?.entity,
                    role,
                })
            }
            ArmyAction::Fire(policy) => Some(ArmyOrder::SetFirePolicy {
                battalion: self.unit?.entity,
                policy,
            }),
            ArmyAction::Rearm => Some(ArmyOrder::Rearm {
                battalion: self.unit?.entity,
            }),
            ArmyAction::Stance(stance) => Some(ArmyOrder::SetStance {
                battalion: self.unit?.entity,
                stance,
            }),
            ArmyAction::Disband if state.confirm_disband => Some(ArmyOrder::Disband {
                battalion: self.unit?.entity,
            }),
            ArmyAction::Add(e) if self.available.contains(&e) && roster.soldiers[&e].available => {
                assignment(vec![e])
            }
            ArmyAction::AddChecked => assignment(self.checked(state, roster, false)),
            ArmyAction::Fill => assignment(self.reserves.iter().copied().take(self.room).collect()),
            ArmyAction::Remove(e) if self.members.contains(&e) && roster.soldiers[&e].available => {
                Some(ArmyOrder::Dismiss { members: vec![e] })
            }
            ArmyAction::RemoveChecked => {
                let members = self.checked(state, roster, true);
                (!members.is_empty()).then_some(ArmyOrder::Dismiss { members })
            }
            _ => None,
        }
    }
    pub fn button(
        &self,
        action: ArmyAction,
        state: &ArmyManagement,
        roster: &ArmyRoster,
        pending: bool,
    ) -> (String, bool, bool) {
        if matches!(action,ArmyAction::Toggle(e)|ArmyAction::Add(e)|ArmyAction::Remove(e) if !roster.soldiers.contains_key(&e))
        {
            return ("Unavailable".into(), false, false);
        }
        let exists = self.unit.is_some();
        let (label, enabled, selected) = match action {
            ArmyAction::New => (
                "NEW BATTALION".into(),
                roster.battalions.len() < MAX_BATTALIONS_PER_ACCOUNT,
                false,
            ),
            ArmyAction::Choose(e) => (
                roster
                    .battalions
                    .iter()
                    .find(|b| b.entity == e)
                    .map_or("Battalion", |b| b.name.as_str())
                    .into(),
                true,
                state.selected == Some(e),
            ),
            ArmyAction::SelectMap => (
                "SELECT ON MAP".into(),
                self.unit.is_some_and(|b| b.count > 0),
                false,
            ),
            ArmyAction::Locate => (
                "LOCATE".into(),
                self.unit.is_some_and(|b| b.count > 0),
                false,
            ),
            ArmyAction::Disband => (
                if state.confirm_disband {
                    "CONFIRM DISBAND"
                } else {
                    "DISBAND"
                }
                .into(),
                exists,
                false,
            ),
            ArmyAction::CancelDisband => ("CANCEL".into(), state.confirm_disband, false),
            ArmyAction::Role(role) => (
                role.label().to_uppercase(),
                self.unit.is_some_and(|b| b.role != SoldierRole::Cavalry),
                self.unit.is_some_and(|b| b.role == role),
            ),
            ArmyAction::Fire(policy) => (
                policy.label().to_uppercase(),
                self.unit.is_some_and(|b| b.role == SoldierRole::Archer),
                self.unit.is_some_and(|b| b.fire_policy == policy),
            ),
            ArmyAction::Rearm => (
                "REARM QUIVERS".into(),
                self.unit.is_some_and(|b| {
                    b.role == SoldierRole::Archer
                        && b.arrows < b.count * usize::from(QUIVER_CAPACITY)
                }),
                false,
            ),
            ArmyAction::Stance(s) => (
                s.label().to_uppercase(),
                exists,
                self.unit.is_some_and(|b| b.stance == s),
            ),
            ArmyAction::Source(other) => (
                if other {
                    "OTHER BATTALIONS"
                } else {
                    "UNASSIGNED"
                }
                .into(),
                exists,
                state.other_battalions == other,
            ),
            ArmyAction::Toggle(e) => {
                let s = &roster.soldiers[&e];
                (
                    format!(
                        "{} {}",
                        if state.members.contains(&e) || state.available.contains(&e) {
                            "[x]"
                        } else {
                            "[ ]"
                        },
                        s.name
                    ),
                    s.available,
                    state.members.contains(&e) || state.available.contains(&e),
                )
            }
            ArmyAction::SelectMembers => ("SELECT ALL".into(), !self.members.is_empty(), false),
            ArmyAction::SelectAvailable => (
                "SELECT TO FILL".into(),
                self.room > 0 && !self.available.is_empty(),
                false,
            ),
            ArmyAction::Add(e) => (
                if roster.soldiers[&e].battalion.is_some() {
                    "TRANSFER"
                } else {
                    "ADD"
                }
                .into(),
                self.command(action, state, roster).is_some(),
                false,
            ),
            ArmyAction::Remove(_) => (
                "REMOVE".into(),
                self.command(action, state, roster).is_some(),
                false,
            ),
            ArmyAction::AddChecked => (
                format!(
                    "ADD SELECTED ({})",
                    self.checked(state, roster, false).len()
                ),
                self.command(action, state, roster).is_some(),
                false,
            ),
            ArmyAction::RemoveChecked => (
                format!(
                    "REMOVE SELECTED ({})",
                    self.checked(state, roster, true).len()
                ),
                self.command(action, state, roster).is_some(),
                false,
            ),
            ArmyAction::Fill => (
                format!(
                    "FILL FROM RESERVES ({})",
                    self.room.min(self.reserves.len())
                ),
                self.command(action, state, roster).is_some(),
                false,
            ),
        };
        let network = matches!(
            action,
            ArmyAction::New
                | ArmyAction::Stance(_)
                | ArmyAction::Role(_)
                | ArmyAction::Fire(_)
                | ArmyAction::Rearm
                | ArmyAction::Disband
                | ArmyAction::Add(_)
                | ArmyAction::Remove(_)
                | ArmyAction::AddChecked
                | ArmyAction::RemoveChecked
                | ArmyAction::Fill
        );
        (label, enabled && (!network || !pending), selected)
    }
}
