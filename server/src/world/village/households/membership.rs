//! Stable domestic groups and their physical dwelling assignments.
//!
//! A group owns its purse even while homeless. A house owns its physical stock;
//! reassignment never transfers inventory. Membership is independent of family,
//! work, political affiliation and command authority.

use super::*;
use bevy::ecs::system::SystemParam;
use shared::components::{
    BuildingId, BuildingOf, HouseAppearance, HouseUpgradeWorksite, HouseholdId, HouseholdMember,
    HouseholdMembers, LivesAt, OccupiedByHousehold, OwnedBy, PersonId, ResidentOf, SettlementId,
};

/// Attach physical house state once, preserving explicitly authored upper
/// storeys. Settlement promotion alone never changes an existing home's level.
pub fn ensure_house_appearances(
    mut commands: Commands,
    houses: Query<
        (
            Entity,
            &SettlementBuilding,
            Option<&shared::building::PlacedBuilding>,
            Option<&PlayerPosition>,
        ),
        (With<Household>, Without<HouseAppearance>),
    >,
) {
    for (entity, building, placed, position) in &houses {
        if building.kind != SettlementBuildingKind::House {
            continue;
        }
        let appearance = placed
            .and_then(|placed| HouseAppearance::from_building_type(placed.building_type))
            .unwrap_or_else(|| {
                HouseAppearance::for_new_house(
                    shared::components::SettlementTier::Hamlet,
                    position.map_or(Vec3::ZERO, |position| position.0),
                )
            });
        commands.entity(entity).insert(appearance);
    }
}

/// Empty houses have a display roster, but no independent necessities account.
pub fn ensure_households(
    mut commands: Commands,
    houses: Query<(Entity, &SettlementBuilding), Without<Household>>,
) {
    for (entity, building) in &houses {
        if building.kind == SettlementBuildingKind::House {
            commands.entity(entity).insert(Household::default());
        }
    }
}

/// A homeless group waiting for construction does not rebuild the world roster
/// each fixed tick. Only membership, residence and housing events require work.
#[derive(SystemParam)]
pub struct MembershipChanges<'w, 's> {
    changed: Query<
        'w,
        's,
        (),
        Or<(
            Added<PersonId>,
            Added<BuildingId>,
            Changed<BuildingOf>,
            Changed<CharacterName>,
            Changed<VillagerIntent>,
            Changed<ResidentOf>,
            Changed<HomeAssignment>,
            Changed<HouseholdMember>,
            Changed<HouseholdMembers>,
            Changed<Household>,
            Changed<HouseAppearance>,
            Changed<HouseUpgradeWorksite>,
            Changed<OwnedBy>,
        )>,
    >,
    people: RemovedComponents<'w, 's, PersonId>,
    buildings: RemovedComponents<'w, 's, BuildingId>,
    memberships: RemovedComponents<'w, 's, HouseholdMember>,
    groups: RemovedComponents<'w, 's, HouseholdMembers>,
    residences: RemovedComponents<'w, 's, ResidentOf>,
    homes: RemovedComponents<'w, 's, HomeAssignment>,
    intents: RemovedComponents<'w, 's, VillagerIntent>,
    appearances: RemovedComponents<'w, 's, HouseAppearance>,
    upgrades: RemovedComponents<'w, 's, HouseUpgradeWorksite>,
}

impl MembershipChanges<'_, '_> {
    fn changed(&mut self) -> bool {
        // Drain every reader, even when another source already requires work.
        !self.changed.is_empty()
            | (self.people.read().count() != 0)
            | (self.buildings.read().count() != 0)
            | (self.memberships.read().count() != 0)
            | (self.groups.read().count() != 0)
            | (self.residences.read().count() != 0)
            | (self.homes.read().count() != 0)
            | (self.intents.read().count() != 0)
            | (self.appearances.read().count() != 0)
            | (self.upgrades.read().count() != 0)
    }
}

struct Resident {
    entity: Entity,
    id: PersonId,
    name: String,
    settlement: Option<SettlementId>,
    position: Vec3,
    home: Option<Entity>,
    lives_at: Option<BuildingId>,
    household: Option<HouseholdId>,
}

struct Dwelling {
    entity: Entity,
    id: BuildingId,
    settlement: SettlementId,
    position: Vec3,
    capacity: usize,
    roster: Household,
    household: Option<HouseholdId>,
    opening_account: Option<HouseholdEconomy>,
    owner: Option<PersonId>,
}

struct DomesticGroup {
    entity: Entity,
    id: HouseholdId,
    members: HouseholdMembers,
    account: HouseholdEconomy,
    region: Option<RegionCoord>,
    new: bool,
    account_changed: bool,
}

impl DomesticGroup {
    fn create(
        commands: &mut Commands,
        id: HouseholdId,
        place: SettlementId,
        residents: Vec<PersonId>,
        dwelling: Option<BuildingId>,
    ) -> Self {
        Self {
            entity: commands.spawn_empty().id(),
            id,
            members: HouseholdMembers {
                resident_ids: residents,
                settlement: place,
                dwelling,
            },
            account: HouseholdEconomy::default(),
            region: None,
            new: true,
            account_changed: false,
        }
    }

    fn position(&self, people: &HashMap<PersonId, Resident>) -> Vec3 {
        let mut sum = Vec3::ZERO;
        let mut count = 0;
        for id in &self.members.resident_ids {
            if let Some(person) = people.get(id) {
                sum += person.position;
                count += 1;
            }
        }
        if count == 0 {
            Vec3::ZERO
        } else {
            sum / count as f32
        }
    }
}

/// Existing members are never reshuffled to fill beds. Unaffiliated newcomers
/// may join a local group with capacity; a displaced group waits together for
/// an empty house. A traveller keeps membership without occupying a resident
/// bed. When all members settle elsewhere, the group follows them together.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn assign_households(
    mut commands: Commands,
    mut ids: ResMut<crate::world::identity::WorldIdAllocator>,
    mut initialized: Local<bool>,
    mut changes: MembershipChanges,
    mut settlements: Query<(&SettlementId, &mut Settlement, Option<&PlayerPosition>)>,
    existing_groups: Query<(
        Entity,
        &HouseholdId,
        &HouseholdMembers,
        &HouseholdEconomy,
        Option<&RegionCoord>,
    )>,
    houses: Query<(
        Entity,
        &BuildingId,
        &BuildingOf,
        &SettlementBuilding,
        &PlayerPosition,
        &Household,
        Option<&OccupiedByHousehold>,
        Option<&HouseholdEconomy>,
        Option<&HouseAppearance>,
        Option<&OwnedBy>,
    )>,
    upgrades: Query<&HouseUpgradeWorksite>,
    villagers: Query<
        (
            Entity,
            &PersonId,
            &CharacterName,
            Option<&VillagerIntent>,
            Option<&ResidentOf>,
            &PlayerPosition,
            Option<&HomeAssignment>,
            Option<&LivesAt>,
            Option<&HouseholdMember>,
        ),
        Without<crate::player::combat::SettledCombatDeath>,
    >,
) {
    let changed = changes.changed();
    if *initialized && !changed {
        return;
    }
    *initialized = true;

    let people: HashMap<PersonId, Resident> = villagers
        .iter()
        .map(
            |(entity, id, name, intent, residence, position, home, lives_at, household)| {
                (
                    *id,
                    Resident {
                        entity,
                        id: *id,
                        name: name.0.clone(),
                        settlement: intent
                            .is_some_and(VillagerIntent::counts_as_resident)
                            .then(|| residence.map(|s| s.0))
                            .flatten(),
                        position: position.0,
                        home: home.map(|home| home.home),
                        lives_at: lives_at.map(|home| home.0),
                        household: household.map(|household| household.0),
                    },
                )
            },
        )
        .collect();
    let mut dwellings: Vec<Dwelling> = houses
        .iter()
        .filter(|(_, _, _, building, ..)| building.kind == SettlementBuildingKind::House)
        .map(
            |(
                entity,
                id,
                place,
                building,
                position,
                roster,
                household,
                account,
                appearance,
                owner,
            )| {
                Dwelling {
                    entity,
                    id: *id,
                    settlement: place.0,
                    position: position.0,
                    capacity: building.kind.housing_capacity_with_house(appearance) as usize,
                    roster: roster.clone(),
                    household: household.map(|household| household.0),
                    opening_account: account.cloned(),
                    owner: owner.map(|owner| owner.0),
                }
            },
        )
        .collect();
    dwellings.sort_unstable_by_key(|house| house.id);
    let house_by_id: HashMap<BuildingId, usize> = dwellings
        .iter()
        .enumerate()
        .map(|(index, house)| (house.id, index))
        .collect();
    let mut groups: Vec<DomesticGroup> = existing_groups
        .iter()
        .map(|(entity, id, members, account, region)| DomesticGroup {
            entity,
            id: *id,
            members: members.clone(),
            account: account.clone(),
            region: region.copied(),
            new: false,
            account_changed: false,
        })
        .collect();
    groups.sort_unstable_by_key(|group| group.id);
    let group_ids: HashSet<HouseholdId> = groups.iter().map(|group| group.id).collect();
    let mut grouped = HashSet::<PersonId>::new();

    // An explicit new membership takes precedence over the previous roster.
    // Restore absent inverse links from complete canonical records on adoption.
    for group in &mut groups {
        group.members.resident_ids.retain(|id| {
            people.get(id).is_some_and(|person| {
                person.household.is_none_or(|household| {
                    household == group.id || !group_ids.contains(&household)
                }) && grouped.insert(*id)
            })
        });
    }
    for person in people.values() {
        if grouped.contains(&person.id) {
            continue;
        }
        if let Some(group) = person
            .household
            .and_then(|id| groups.iter_mut().find(|group| group.id == id))
        {
            group.members.resident_ids.push(person.id);
            grouped.insert(person.id);
        }
    }

    // Adopt complete starting homes from founding/authored fixtures. Only people
    // with no stable group are grouped by their existing physical assignment.
    for house in &dwellings {
        if groups
            .iter()
            .any(|group| group.members.dwelling == Some(house.id))
        {
            continue;
        }
        let mut members: Vec<PersonId> = people
            .values()
            .filter(|person| {
                !grouped.contains(&person.id)
                    && person.settlement == Some(house.settlement)
                    && (person.home == Some(house.entity)
                        || house.roster.resident_ids.contains(&person.id))
            })
            .map(|person| person.id)
            .collect();
        members.sort_unstable();
        members.truncate(house.capacity);
        if members.is_empty() {
            continue;
        }
        grouped.extend(members.iter().copied());
        groups.push(DomesticGroup::create(
            &mut commands,
            ids.household(),
            house.settlement,
            members,
            Some(house.id),
        ));
    }

    let mut occupied = HashSet::<BuildingId>::new();
    for group in &mut groups {
        group.members.resident_ids.sort_unstable();
        if group.members.resident_ids.is_empty() {
            continue;
        }
        let mut places = group
            .members
            .resident_ids
            .iter()
            .map(|id| people[id].settlement);
        if let Some(Some(place)) = places.next() {
            if places.all(|other| other == Some(place)) {
                group.members.settlement = place;
            }
        }
        let valid = group
            .members
            .dwelling
            .and_then(|id| house_by_id.get(&id).copied())
            .is_some_and(|index| {
                let house = &dwellings[index];
                house.settlement == group.members.settlement
                    && group.members.resident_ids.len() <= house.capacity
                    && occupied.insert(house.id)
            });
        if !valid {
            group.members.dwelling = None;
        }
    }
    // Private vacant homes give their unhoused owner's group first refusal.
    // An explicitly commissioned extension also reserves an undersized home
    // until that same group can fit. Other vacant homes remain shared lodging.
    let waiting_owners: HashMap<PersonId, (HouseholdId, SettlementId, usize)> = groups
        .iter()
        .filter(|group| group.members.dwelling.is_none())
        .flat_map(|group| {
            group.members.resident_ids.iter().filter_map(|id| {
                (people[id].settlement == Some(group.members.settlement)).then_some((
                    *id,
                    (
                        group.id,
                        group.members.settlement,
                        group.members.resident_ids.len(),
                    ),
                ))
            })
        })
        .collect();
    let pending_owners: HashMap<BuildingId, PersonId> = upgrades
        .iter()
        .map(|site| (site.house, site.owner))
        .collect();
    let reservations: HashMap<BuildingId, HouseholdId> = dwellings
        .iter()
        .filter_map(|house| {
            if occupied.contains(&house.id) {
                return None;
            }
            let owner = house.owner?;
            let &(group, settlement, members) = waiting_owners.get(&owner)?;
            (settlement == house.settlement
                && (members <= house.capacity
                    || (members
                        <= usize::from(
                            shared::components::HouseLevel::UpperStorey.housing_capacity(),
                        )
                        && pending_owners.get(&house.id) == Some(&owner))))
            .then_some((house.id, group))
        })
        .collect();
    // Existing displaced groups get the first chance at an unoccupied house.
    for group in &mut groups {
        if group.members.resident_ids.is_empty() || group.members.dwelling.is_some() {
            continue;
        }
        let position = group.position(&people);
        if let Some(house) = dwellings
            .iter()
            .filter(|house| {
                house.settlement == group.members.settlement
                    && !occupied.contains(&house.id)
                    && group.members.resident_ids.len() <= house.capacity
                    && reservations
                        .get(&house.id)
                        .is_none_or(|reserved| *reserved == group.id)
            })
            .min_by(|a, b| {
                a.position
                    .distance_squared(position)
                    .total_cmp(&b.position.distance_squared(position))
                    .then_with(|| a.id.cmp(&b.id))
            })
        {
            group.members.dwelling = Some(house.id);
            occupied.insert(house.id);
        }
    }

    let mut newcomers: Vec<PersonId> = people
        .values()
        .filter(|person| person.settlement.is_some() && !grouped.contains(&person.id))
        .map(|person| person.id)
        .collect();
    newcomers.sort_unstable();
    for id in &newcomers {
        let person = &people[id];
        let candidate = groups
            .iter()
            .enumerate()
            .filter_map(|(index, group)| {
                let house = &dwellings[*house_by_id.get(&group.members.dwelling?)?];
                (person.settlement == Some(group.members.settlement)
                    && !group.members.resident_ids.is_empty()
                    && group.members.resident_ids.len() < house.capacity)
                    .then_some((
                        index,
                        house.position.distance_squared(person.position),
                        group.id,
                    ))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.2.cmp(&b.2)));
        if let Some((index, ..)) = candidate {
            groups[index].members.resident_ids.push(*id);
            grouped.insert(*id);
        }
    }
    for house in &dwellings {
        if occupied.contains(&house.id) || reservations.contains_key(&house.id) {
            continue;
        }
        let mut candidates: Vec<PersonId> = newcomers
            .iter()
            .copied()
            .filter(|id| !grouped.contains(id) && people[id].settlement == Some(house.settlement))
            .collect();
        candidates.sort_by(|a, b| {
            people[a]
                .position
                .distance_squared(house.position)
                .total_cmp(&people[b].position.distance_squared(house.position))
                .then_with(|| a.cmp(b))
        });
        candidates.truncate(house.capacity);
        if candidates.is_empty() {
            continue;
        }
        grouped.extend(candidates.iter().copied());
        occupied.insert(house.id);
        groups.push(DomesticGroup::create(
            &mut commands,
            ids.household(),
            house.settlement,
            candidates,
            Some(house.id),
        ));
    }
    // Unhoused people also acquire a durable support group. Keep groups bounded
    // by ordinary capacity so they can move together into a later completed home.
    for id in newcomers {
        if grouped.contains(&id) {
            continue;
        }
        let place = people[&id]
            .settlement
            .expect("newcomers are settled residents");
        if let Some(group) = groups.iter_mut().find(|group| {
            group.members.dwelling.is_none()
                && group.members.settlement == place
                && !group.members.resident_ids.is_empty()
                && group.members.resident_ids.len()
                    < SettlementBuildingKind::House.housing_capacity() as usize
        }) {
            group.members.resident_ids.push(id);
        } else {
            groups.push(DomesticGroup::create(
                &mut commands,
                ids.household(),
                place,
                vec![id],
                None,
            ));
        }
        grouped.insert(id);
    }

    // Exact, one-time transfer of any bootstrap house account. This never reads
    // or writes GoodsInventory. A vacant house's orphan cash returns to its Hall.
    for house in &dwellings {
        let Some(account) = &house.opening_account else {
            continue;
        };
        if let Some(group) = groups.iter_mut().find(|group| {
            group.members.dwelling == Some(house.id) && !group.members.resident_ids.is_empty()
        }) {
            if group.new {
                group.account = account.clone();
            } else {
                group.account.pennies = group.account.pennies.saturating_add(account.pennies);
            }
            group.account_changed = true;
        } else if let Some((_, mut settlement, _)) = settlements
            .iter_mut()
            .find(|(id, ..)| **id == house.settlement)
        {
            settlement.treasury = settlement.treasury.saturating_add(account.pennies);
        } else if account.pennies != 0 {
            continue; // Do not destroy an orphan account with no possible recipient.
        }
        commands.entity(house.entity).remove::<HouseholdEconomy>();
    }

    let mut assigned = HashSet::<PersonId>::new();
    let mut rosters = HashMap::<BuildingId, (HouseholdId, Household)>::new();
    for mut group in groups {
        if group.members.resident_ids.is_empty() {
            if let Some((_, mut settlement, _)) = settlements
                .iter_mut()
                .find(|(id, ..)| **id == group.members.settlement)
            {
                settlement.treasury = settlement.treasury.saturating_add(group.account.pennies);
                commands.entity(group.entity).despawn();
            } else if group.account.pennies == 0 {
                commands.entity(group.entity).despawn();
            }
            continue;
        }
        group.members.resident_ids.sort_unstable();
        let dwelling = group
            .members
            .dwelling
            .and_then(|id| house_by_id.get(&id).map(|index| &dwellings[*index]));
        let position = dwelling.map_or_else(
            || {
                settlements
                    .iter()
                    .find(|(id, ..)| **id == group.members.settlement)
                    .and_then(|(_, _, position)| position.map(|position| position.0))
                    .unwrap_or_else(|| group.position(&people))
            },
            |house| house.position,
        );
        let region = RegionCoord::from_world_pos(position);
        let mut roster = Household::default();
        for id in &group.members.resident_ids {
            let person = &people[id];
            if person.household != Some(group.id) {
                commands
                    .entity(person.entity)
                    .insert(HouseholdMember(group.id));
            }
            if let Some(house) =
                dwelling.filter(|house| person.settlement == Some(house.settlement))
            {
                if person.home != Some(house.entity) {
                    commands
                        .entity(person.entity)
                        .insert(HomeAssignment::new(house.entity));
                }
                if person.lives_at != Some(house.id) {
                    commands.entity(person.entity).insert(LivesAt(house.id));
                }
                assigned.insert(*id);
                roster.resident_ids.push(*id);
                roster.residents.push(person.name.clone());
            }
        }
        if let Some(house) = dwelling {
            rosters.insert(house.id, (group.id, roster));
        }
        if group.new {
            commands.entity(group.entity).insert((
                group.id,
                group.members,
                group.account,
                region,
                Replicate::to_clients(NetworkTarget::All),
            ));
        } else {
            let (_, _, previous, _, _) = existing_groups
                .get(group.entity)
                .expect("existing household");
            if *previous != group.members {
                commands.entity(group.entity).insert(group.members);
            }
            if group.account_changed {
                commands.entity(group.entity).insert(group.account);
            }
            if group.region != Some(region) {
                commands.entity(group.entity).insert(region);
            }
        }
    }
    for person in people.values() {
        if !assigned.contains(&person.id) {
            if person.home.is_some() {
                commands.entity(person.entity).remove::<HomeAssignment>();
            }
            if person.lives_at.is_some() {
                commands.entity(person.entity).remove::<LivesAt>();
            }
        }
        if !grouped.contains(&person.id) && person.household.is_some() {
            commands.entity(person.entity).remove::<HouseholdMember>();
        }
    }
    for house in dwellings {
        let (household, roster) = rosters
            .remove(&house.id)
            .map_or((None, Household::default()), |(id, roster)| {
                (Some(id), roster)
            });
        if house.roster != roster {
            commands.entity(house.entity).insert(roster);
        }
        if house.household != household {
            if let Some(id) = household {
                commands
                    .entity(house.entity)
                    .insert(OccupiedByHousehold(id));
            } else {
                commands
                    .entity(house.entity)
                    .remove::<OccupiedByHousehold>();
            }
        }
    }
}

#[cfg(test)]
mod tests;
