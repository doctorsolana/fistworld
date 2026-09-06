//! One derived roster for selection, the combat bar and the encyclopedia.
//! Walking does not invalidate it; only membership, identity and vitals do.

use bevy::prelude::*;
use shared::components::*;
use shared::protocol::UnitSelection;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Debug, PartialEq)]
pub struct SoldierFacts {
    pub entity: Entity,
    pub identity: u64,
    pub name: String,
    pub battalion: Option<BattalionId>,
    pub strength: u8,
    pub current_health: f32,
    pub max_health: f32,
    pub available: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BattalionFacts {
    pub entity: Entity,
    pub id: BattalionId,
    pub name: String,
    pub ordinal: u64,
    pub members: Vec<Entity>,
    pub count: usize,
    pub mean_strength: u32,
    pub health_fraction: f32,
    pub stance: BattalionStance,
}

#[derive(Resource, Default)]
pub struct ArmyRoster {
    pub account: String,
    pub battalions: Vec<BattalionFacts>,
    pub soldiers: HashMap<Entity, SoldierFacts>,
    pub revision: u64,
}

impl ArmyRoster {
    /// Compress only completely selected battalions. Partial selections stay
    /// individual, so an Alt-click never commands the rest of that battalion.
    pub fn selection(&self, entities: &[Entity]) -> UnitSelection {
        let selected: HashSet<_> = entities.iter().copied().collect();
        let mut represented = HashSet::new();
        let battalions = self
            .battalions
            .iter()
            .filter(|b| !b.members.is_empty() && b.members.iter().all(|e| selected.contains(e)))
            .map(|b| {
                represented.extend(b.members.iter().copied());
                b.id
            })
            .collect();
        UnitSelection {
            units: entities
                .iter()
                .filter(|e| !represented.contains(e))
                .copied()
                .collect(),
            battalions,
        }
    }

    pub fn resolve(&self, selection: &UnitSelection) -> Vec<Entity> {
        let mut units: Vec<_> = selection
            .units
            .iter()
            .filter(|e| self.soldiers.contains_key(e))
            .copied()
            .collect();
        for battalion in &self.battalions {
            if selection.battalions.contains(&battalion.id) {
                units.extend(&battalion.members);
            }
        }
        let mut seen = HashSet::new();
        units.retain(|e| seen.insert(*e));
        units
    }

    pub fn muster_candidates(&self, selection: &crate::selection::Selection) -> Vec<Entity> {
        selection
            .entities
            .iter()
            .filter(|e| self.soldiers.get(e).is_some_and(|s| s.available))
            .copied()
            .take(MAX_BATTALION_SIZE)
            .collect()
    }
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArmyRosterSet;

#[derive(bevy::ecs::system::SystemParam)]
pub struct RosterChanges<'w, 's> {
    changed: Query<
        'w,
        's,
        (),
        (
            With<CommandedBy>,
            Or<(
                Changed<CommandedBy>,
                Changed<Health>,
                Changed<CharacterAttributes>,
                Changed<CharacterName>,
                Changed<PersonId>,
                Changed<Battalion>,
                Changed<BattalionStance>,
                Changed<MemberOfBattalion>,
                Added<AboardBoat>,
            )>,
        ),
    >,
    members: RemovedComponents<'w, 's, MemberOfBattalion>,
    owners: RemovedComponents<'w, 's, CommandedBy>,
    battalions: RemovedComponents<'w, 's, Battalion>,
    aboard: RemovedComponents<'w, 's, AboardBoat>,
    health: RemovedComponents<'w, 's, Health>,
}

#[allow(clippy::type_complexity)]
pub fn refresh_army_roster(
    name: Res<crate::ui::name_entry::PlayerNameInput>,
    mut changes: RosterChanges,
    soldiers: Query<
        (
            Entity,
            &CommandedBy,
            Option<&CharacterName>,
            Option<&CharacterAttributes>,
            Option<&MemberOfBattalion>,
            Option<&Health>,
            Has<AboardBoat>,
            Option<&PersonId>,
        ),
        With<CharacterKind>,
    >,
    battalions: Query<(Entity, &Battalion, &CommandedBy, Option<&BattalionStance>)>,
    mut roster: ResMut<ArmyRoster>,
) {
    let removed = changes.members.read().count()
        + changes.owners.read().count()
        + changes.battalions.read().count()
        + changes.aboard.read().count()
        + changes.health.read().count();
    if !name.is_changed() && changes.changed.is_empty() && removed == 0 {
        return;
    }
    let account = name.name.trim().to_lowercase();
    let mut troops = HashMap::new();
    let mut serving = BTreeMap::<BattalionId, Vec<Entity>>::new();
    for (entity, owner, name, attributes, member, health, aboard, identity) in &soldiers {
        if account.is_empty() || owner.0 != account || health.is_some_and(|h| h.is_dead()) {
            continue;
        }
        if let Some(member) = member {
            serving.entry(member.0).or_default().push(entity);
        }
        troops.insert(
            entity,
            SoldierFacts {
                entity,
                identity: identity.map_or(entity.to_bits(), |id| id.0),
                name: name.map_or_else(|| "Soldier".into(), |n| n.0.clone()),
                battalion: member.map(|m| m.0),
                strength: attributes.map_or(0, |a| a.physique()),
                current_health: health.map_or(CHARACTER_MAX_HEALTH, |h| h.current),
                max_health: health.map_or(CHARACTER_MAX_HEALTH, |h| h.max),
                available: !aboard,
            },
        );
    }
    let mut units = Vec::new();
    for (entity, battalion, owner, stance) in &battalions {
        if account.is_empty() || owner.0 != account {
            continue;
        }
        let mut members = serving.remove(&battalion.id).unwrap_or_default();
        members.sort();
        let mut health = 0.0;
        let mut max_health = 0.0;
        let mut strength = 0;
        for member in &members {
            let soldier = &troops[member];
            health += soldier.current_health.max(0.0);
            max_health += soldier.max_health;
            strength += u32::from(soldier.strength);
        }
        units.push(BattalionFacts {
            entity,
            id: battalion.id,
            name: battalion.name.clone(),
            ordinal: battalion.ordinal,
            stance: stance.copied().unwrap_or_default(),
            count: members.len(),
            mean_strength: strength / members.len().max(1) as u32,
            health_fraction: if max_health > 0.0 {
                (health / max_health).clamp(0.0, 1.0)
            } else {
                0.0
            },
            members,
        });
    }
    units.sort_by_key(|b| b.ordinal);
    if roster.account != account || roster.battalions != units || roster.soldiers != troops {
        roster.account = account;
        roster.battalions = units;
        roster.soldiers = troops;
        roster.revision += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app() -> App {
        let mut app = App::new();
        app.init_resource::<ArmyRoster>();
        app.insert_resource(crate::ui::name_entry::PlayerNameInput {
            name: "alice".into(),
            submitted: true,
        });
        app.add_systems(Update, refresh_army_roster);
        app.world_mut().spawn((
            Battalion {
                id: BattalionId(1),
                name: "First".into(),
                ordinal: 1,
            },
            CommandedBy("alice".into()),
        ));
        app
    }
    fn soldier(app: &mut App) -> Entity {
        app.world_mut()
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(Vec3::ZERO),
                MemberOfBattalion(BattalionId(1)),
                CommandedBy("alice".into()),
                Health::default(),
            ))
            .id()
    }
    #[test]
    fn walking_does_not_rebuild_roster_but_casualties_do() {
        let mut app = app();
        let soldier = soldier(&mut app);
        app.update();
        let revision = app.world().resource::<ArmyRoster>().revision;
        for _ in 0..10 {
            app.world_mut()
                .get_mut::<PlayerPosition>(soldier)
                .unwrap()
                .0
                .x += 1.0;
            app.update();
            assert_eq!(app.world().resource::<ArmyRoster>().revision, revision);
        }
        app.world_mut().get_mut::<Health>(soldier).unwrap().current = 0.0;
        app.update();
        assert!(app.world().resource::<ArmyRoster>().soldiers.is_empty());
        assert_eq!(app.world().resource::<ArmyRoster>().battalions[0].count, 0);
    }
    #[test]
    fn full_and_partial_control_groups_keep_their_selection_meaning() {
        let mut app = app();
        let a = soldier(&mut app);
        let b = soldier(&mut app);
        app.update();
        let roster = app.world().resource::<ArmyRoster>();
        let whole = roster.selection(&[a, b]);
        let partial = roster.selection(&[a]);
        assert_eq!(whole.battalions, vec![BattalionId(1)]);
        assert!(whole.units.is_empty());
        assert_eq!(partial.units, vec![a]);
        assert!(partial.battalions.is_empty());
        let c = soldier(&mut app);
        app.world_mut().despawn(b);
        app.update();
        let roster = app.world().resource::<ArmyRoster>();
        assert_eq!(roster.resolve(&whole).len(), 2);
        assert!(roster.resolve(&whole).contains(&c));
        assert_eq!(roster.resolve(&partial), vec![a]);
    }
}
