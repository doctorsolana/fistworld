//! Small, retained staff lists address people through replicated employment IDs.
//! Unknown/offline identities are never guessed from a potentially duplicated name.

use super::*;
use bevy::platform::collections::{HashMap, HashSet};
use shared::components::{BuildingId, CharacterName, EmployedAt};

#[derive(Component)]
pub(super) struct WorkplacePeople(Option<BuildingId>);

#[derive(Component, PartialEq, Eq)]
pub(super) struct BoundRoster {
    building: Option<BuildingId>,
    people: Vec<(PersonId, String)>,
}

pub(in crate::ui::encyclopedia) fn spawn_site_people(
    parent: &mut ChildSpawnerCommands<'_>,
    site: BuildingId,
) {
    spawn(parent, Some(site));
}

pub(in crate::ui::encyclopedia) fn spawn_workplace_people(parent: &mut ChildSpawnerCommands<'_>) {
    spawn(parent, None);
}

fn spawn(parent: &mut ChildSpawnerCommands<'_>, site: Option<BuildingId>) {
    parent.spawn((
        WorkplacePeople(site),
        Node {
            display: Display::None,
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            row_gap: Val::Px(3.0),
            ..default()
        },
    ));
}

pub(super) fn sync_rosters(
    mut commands: Commands,
    places: Res<places::KnownPlaces>,
    selected: Res<places::SelectedPlace>,
    entry: Res<places::SelectedPlaceEntry>,
    workers: Query<(&PersonId, &CharacterName, &EmployedAt)>,
    mut hosts: Query<(Entity, &WorkplacePeople, Option<&BoundRoster>, &mut Node)>,
    people: Res<KnownPeople>,
    god: Res<crate::ui::hud::GodCapability>,
    time: Res<Time>,
    mut last_scan: Local<Option<f64>>,
) {
    if hosts.is_empty() {
        return;
    }
    let now = time.elapsed_secs_f64();
    if last_scan.is_some_and(|last| now - last < 0.5)
        && !selected.is_changed()
        && !entry.is_changed()
        && !people.is_changed()
        && !god.is_changed()
        && hosts.iter().all(|(_, _, bound, _)| bound.is_some())
    {
        return;
    }
    *last_scan = Some(now);
    let selected_building = selected
        .0
        .and_then(|id| places.records.iter().find(|place| place.id == id))
        .and_then(|place| match *entry {
            places::SelectedPlaceEntry::Building(index) => {
                place.buildings.get(index).and_then(|building| building.id)
            }
            _ => None,
        });
    let mut rosters: HashMap<BuildingId, Vec<(PersonId, String)>> = HashMap::new();
    for (_, host, _, _) in &hosts {
        if let Some(building) = host.0.or(selected_building) {
            rosters.entry(building).or_default();
        }
    }
    let known: HashSet<_> = people
        .records
        .iter()
        .filter(|person| person.id.is_assigned() && (person.known || person.is_self || god.0))
        .map(|person| person.id)
        .collect();
    for (person, name, employer) in &workers {
        if known.contains(person) {
            if let Some(roster) = rosters.get_mut(&employer.0) {
                roster.push((*person, name.0.clone()));
            }
        }
    }
    for roster in rosters.values_mut() {
        // During replication handover a durable person may briefly have two
        // bodies/names. Deduplicate identity before sorting display labels.
        roster.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        roster.dedup_by_key(|person| person.0);
        roster.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    }
    for (entity, host, previous, mut node) in &mut hosts {
        let building = host.0.or(selected_building);
        let people = building
            .and_then(|id| rosters.get(&id))
            .cloned()
            .unwrap_or_default();
        let next = BoundRoster { building, people };
        if previous == Some(&next) {
            continue;
        }
        node.display = if next.people.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
        commands
            .entity(entity)
            .despawn_children()
            .with_children(|rows| {
                if !next.people.is_empty() {
                    rows.spawn(ledger::body_strong("WORKERS", 13.0));
                    for (person, name) in &next.people {
                        spawn_person_link(rows, *person, name, "View person");
                    }
                }
            })
            .insert(next);
    }
}
