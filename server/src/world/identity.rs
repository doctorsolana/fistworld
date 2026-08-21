//! Stable world identity and session-local lookup indexes.
//!
//! Bevy `Entity` values are excellent live handles but are not save keys, and
//! names are presentation rather than identity. Every durable person, place
//! and building receives an opaque monotonic id once. Relationship components
//! retain those ids while runtime AI remains free to cache Entity handles.

use bevy::prelude::*;
#[cfg(test)]
use shared::components::CharacterKind;
use shared::components::{
    AttachedTo, BuildingId, BuildingOf, CharacterName, CivicEmployment, CivicRole,
    CivicTradeContract, Company, CompanyId, CompanyTradeRoute, EmployedAt, FarmField, FishingPier,
    LivesAt, LivestockPasture, MootAdministration, OwnedBy, PersonId, PlayerPosition, ResidentOf,
    RoadOf, Settlement, SettlementBuilding, SettlementBuildingKind, SettlementId, TradeContractId,
    TradeRouteId, VillageRoad,
};

use super::village::{HomeAssignment, VillagerIntent};

#[derive(Resource, Debug)]
pub struct WorldIdAllocator {
    next_person: u64,
    next_settlement: u64,
    next_building: u64,
    next_company: u64,
    next_trade_contract: u64,
    next_trade_route: u64,
}

/// Migrate field and pier parent links from their old position join. New
/// adjuncts receive AttachedTo at spawn, so this compatibility pass becomes a
/// no-op after old worlds have been observed once.
pub fn reconcile_stable_adjunct_relationships(
    mut commands: Commands,
    buildings: Query<(&BuildingId, &SettlementBuilding, &PlayerPosition)>,
    fields: Query<(Entity, &FarmField), Without<AttachedTo>>,
    piers: Query<(Entity, &FishingPier), Without<AttachedTo>>,
    pastures: Query<(Entity, &LivestockPasture), Without<AttachedTo>>,
) {
    for (entity, field) in fields.iter() {
        let mut matches = buildings.iter().filter(|(_, building, position)| {
            building.kind == SettlementBuildingKind::Farmstead
                && building.settlement == field.settlement
                && position.0 == field.farmstead
        });
        if let (Some(first), None) = (matches.next().map(|(id, ..)| *id), matches.next()) {
            commands.entity(entity).insert(AttachedTo(first));
        }
    }
    for (entity, pier) in piers.iter() {
        let mut matches = buildings.iter().filter(|(_, building, position)| {
            building.kind == SettlementBuildingKind::FishermansHut
                && building.settlement == pier.settlement
                && position.0 == pier.fishermans_hut
        });
        if let (Some(first), None) = (matches.next().map(|(id, ..)| *id), matches.next()) {
            commands.entity(entity).insert(AttachedTo(first));
        }
    }
    for (entity, pasture) in pastures.iter() {
        let mut matches = buildings.iter().filter(|(_, building, position)| {
            building.kind == SettlementBuildingKind::LivestockFarm
                && building.settlement == pasture.settlement
                && position.0 == pasture.livestock_farm
        });
        if let (Some(first), None) = (matches.next().map(|(id, ..)| *id), matches.next()) {
            commands.entity(entity).insert(AttachedTo(first));
        }
    }
}

/// Migrate legacy road labels once. Ambiguous duplicate settlement names are
/// deliberately left unresolved; newly planned roads receive `RoadOf`
/// immediately and never depend on this compatibility boundary.
pub fn reconcile_stable_road_relationships(
    mut commands: Commands,
    settlements: Query<(&Settlement, &SettlementId)>,
    roads: Query<(Entity, &VillageRoad), Without<RoadOf>>,
) {
    for (entity, road) in roads.iter() {
        let mut matches = settlements
            .iter()
            .filter(|(settlement, _)| settlement.name == road.settlement);
        if let (Some(first), None) = (
            matches.next().map(|(_, settlement_id)| *settlement_id),
            matches.next(),
        ) {
            commands.entity(entity).insert(RoadOf(first));
        }
    }
}

/// Backfill durable civic assignments from the replicated named roster. New
/// hires write this component immediately; this pass exists for old saves and
/// removes posts whose holder has left the settlement. Ambiguous duplicate
/// display names are never guessed.
pub fn reconcile_stable_civic_employment(
    mut commands: Commands,
    halls: Query<(Entity, &SettlementId, Ref<MootAdministration>)>,
    people: Query<(
        Entity,
        &CharacterName,
        &VillagerIntent,
        Option<&CivicEmployment>,
    )>,
    changed_people: Query<
        (Entity, &VillagerIntent, Option<&CivicEmployment>),
        Changed<VillagerIntent>,
    >,
) {
    // A person leaving town can invalidate a post without changing the hall's
    // roster in the same tick. This query is proportional to movers, not to
    // the entire population.
    for (entity, intent, current) in changed_people.iter() {
        let Some(current) = current else {
            continue;
        };
        let remains_in_settlement = intent.settlement().is_some_and(|hall| {
            halls
                .get(hall)
                .is_ok_and(|(_, settlement, _)| *settlement == current.settlement)
        });
        if !remains_in_settlement {
            commands.entity(entity).remove::<CivicEmployment>();
        }
    }

    for (hall, settlement_id, administration) in halls.iter() {
        // New staffing writes CivicEmployment directly. The name join is only
        // an old-save migration and therefore runs when its source changes.
        if !administration.is_changed() {
            continue;
        }
        let role_for_name = |name: &str| {
            if administration.lead_steward.as_deref() == Some(name) {
                Some(CivicRole::MootSteward)
            } else if administration.reeve.as_deref() == Some(name) {
                Some(CivicRole::Reeve)
            } else if administration.guards.iter().any(|guard| guard == name) {
                Some(CivicRole::Guard)
            } else if administration
                .city_workers
                .iter()
                .any(|worker| worker == name)
            {
                Some(CivicRole::CityWorker)
            } else {
                None
            }
        };

        for (entity, name, intent, current) in people.iter() {
            if intent.settlement() != Some(hall) {
                if current.is_some_and(|job| job.settlement == *settlement_id) {
                    commands.entity(entity).remove::<CivicEmployment>();
                }
                continue;
            }
            // Durable employment is authoritative. The readable legacy roster
            // must never erase or transfer a post merely because its holder
            // was renamed (or shares a display name with somebody else).
            if current.is_some() {
                continue;
            }
            let Some(role) = role_for_name(&name.0) else {
                continue;
            };
            let desired = CivicEmployment {
                settlement: *settlement_id,
                role,
            };
            let matches = people
                .iter()
                .filter(|(_, other_name, other_intent, _)| {
                    other_name.0 == name.0 && other_intent.settlement() == Some(hall)
                })
                .take(2)
                .count();
            if matches == 1 {
                commands.entity(entity).insert(desired);
            }
        }
    }
}

impl Default for WorldIdAllocator {
    fn default() -> Self {
        Self {
            next_person: 1,
            next_settlement: 1,
            next_building: 1,
            next_company: 1,
            next_trade_contract: 1,
            next_trade_route: 1,
        }
    }
}

impl WorldIdAllocator {
    fn observe_person(&mut self, id: PersonId) {
        self.next_person = self.next_person.max(id.0.saturating_add(1));
    }

    fn observe_settlement(&mut self, id: SettlementId) {
        self.next_settlement = self.next_settlement.max(id.0.saturating_add(1));
    }

    fn observe_building(&mut self, id: BuildingId) {
        self.next_building = self.next_building.max(id.0.saturating_add(1));
    }

    fn observe_company(&mut self, id: CompanyId) {
        self.next_company = self.next_company.max(id.0.saturating_add(1));
    }

    fn observe_trade_contract(&mut self, id: TradeContractId) {
        self.next_trade_contract = self.next_trade_contract.max(id.0.saturating_add(1));
    }

    fn observe_trade_route(&mut self, id: TradeRouteId) {
        self.next_trade_route = self.next_trade_route.max(id.0.saturating_add(1));
    }

    fn person(&mut self) -> PersonId {
        let id = PersonId(self.next_person);
        self.next_person = self.next_person.saturating_add(1);
        id
    }

    pub(crate) fn settlement(&mut self) -> SettlementId {
        let id = SettlementId(self.next_settlement);
        self.next_settlement = self.next_settlement.saturating_add(1);
        id
    }

    fn building(&mut self) -> BuildingId {
        let id = BuildingId(self.next_building);
        self.next_building = self.next_building.saturating_add(1);
        id
    }

    pub(crate) fn company(&mut self) -> CompanyId {
        let id = CompanyId(self.next_company);
        self.next_company = self.next_company.saturating_add(1);
        id
    }

    fn trade_contract(&mut self) -> TradeContractId {
        let id = TradeContractId(self.next_trade_contract);
        self.next_trade_contract = self.next_trade_contract.saturating_add(1);
        id
    }

    fn trade_route(&mut self) -> TradeRouteId {
        let id = TradeRouteId(self.next_trade_route);
        self.next_trade_route = self.next_trade_route.saturating_add(1);
        id
    }
}

#[derive(Resource, Default, Debug)]
pub struct WorldIdentityIndex {
    pub people: bevy::platform::collections::HashMap<PersonId, Entity>,
    pub settlements: bevy::platform::collections::HashMap<SettlementId, Entity>,
    pub buildings: bevy::platform::collections::HashMap<BuildingId, Entity>,
    pub companies: bevy::platform::collections::HashMap<CompanyId, Entity>,
    initialized: bool,
}

/// Assign ids to newly created durable entities. Existing ids are observed
/// first so loading a world and then creating something can never reuse an id.
pub fn assign_stable_world_ids(
    mut commands: Commands,
    mut allocator: ResMut<WorldIdAllocator>,
    existing_people: Query<&PersonId, Added<PersonId>>,
    existing_settlements: Query<&SettlementId, Added<SettlementId>>,
    existing_buildings: Query<&BuildingId, Added<BuildingId>>,
    existing_companies: Query<&CompanyId, Added<CompanyId>>,
    existing_trade_contracts: Query<&TradeContractId, Added<TradeContractId>>,
    existing_trade_routes: Query<&TradeRouteId, Added<TradeRouteId>>,
    new_people: Query<Entity, (With<CharacterName>, Without<PersonId>)>,
    new_settlements: Query<Entity, (With<Settlement>, Without<SettlementId>)>,
    new_buildings: Query<Entity, (With<SettlementBuilding>, Without<BuildingId>)>,
    new_companies: Query<Entity, (With<Company>, Without<CompanyId>)>,
    new_trade_contracts: Query<Entity, (With<CivicTradeContract>, Without<TradeContractId>)>,
    new_trade_routes: Query<Entity, (With<CompanyTradeRoute>, Without<TradeRouteId>)>,
) {
    for id in existing_people.iter() {
        allocator.observe_person(*id);
    }
    for id in existing_settlements.iter() {
        allocator.observe_settlement(*id);
    }
    for id in existing_buildings.iter() {
        allocator.observe_building(*id);
    }
    for id in existing_companies.iter() {
        allocator.observe_company(*id);
    }
    for id in existing_trade_contracts.iter() {
        allocator.observe_trade_contract(*id);
    }
    for id in existing_trade_routes.iter() {
        allocator.observe_trade_route(*id);
    }

    for entity in new_people.iter() {
        commands.entity(entity).insert(allocator.person());
    }
    for entity in new_settlements.iter() {
        commands.entity(entity).insert(allocator.settlement());
    }
    for entity in new_buildings.iter() {
        commands.entity(entity).insert(allocator.building());
    }
    for entity in new_companies.iter() {
        commands.entity(entity).insert(allocator.company());
    }
    for entity in new_trade_contracts.iter() {
        commands.entity(entity).insert(allocator.trade_contract());
    }
    for entity in new_trade_routes.iter() {
        commands.entity(entity).insert(allocator.trade_route());
    }
}

/// Rebuild the cheap Entity lookup tables only when identity components change.
pub fn rebuild_world_identity_index(
    mut index: ResMut<WorldIdentityIndex>,
    people: Query<(Entity, &PersonId), With<CharacterName>>,
    settlements: Query<(Entity, &SettlementId), With<Settlement>>,
    buildings: Query<(Entity, &BuildingId), With<SettlementBuilding>>,
    companies: Query<(Entity, &CompanyId), With<Company>>,
    changed_people: Query<(), Changed<PersonId>>,
    changed_settlements: Query<(), Changed<SettlementId>>,
    changed_buildings: Query<(), Changed<BuildingId>>,
    changed_companies: Query<(), Changed<CompanyId>>,
    removed_people: RemovedComponents<PersonId>,
    removed_settlements: RemovedComponents<SettlementId>,
    removed_buildings: RemovedComponents<BuildingId>,
    removed_companies: RemovedComponents<CompanyId>,
) {
    let dirty = !index.initialized
        || !changed_people.is_empty()
        || !changed_settlements.is_empty()
        || !changed_buildings.is_empty()
        || !changed_companies.is_empty()
        || !removed_people.is_empty()
        || !removed_settlements.is_empty()
        || !removed_buildings.is_empty()
        || !removed_companies.is_empty();
    if !dirty {
        return;
    }

    index.people.clear();
    index.settlements.clear();
    index.buildings.clear();
    index.companies.clear();
    for (entity, id) in people.iter() {
        assert!(
            index.people.insert(*id, entity).is_none(),
            "duplicate durable PersonId {}",
            id.0
        );
    }
    for (entity, id) in settlements.iter() {
        assert!(
            index.settlements.insert(*id, entity).is_none(),
            "duplicate durable SettlementId {}",
            id.0
        );
    }
    for (entity, id) in buildings.iter() {
        assert!(
            index.buildings.insert(*id, entity).is_none(),
            "duplicate durable BuildingId {}",
            id.0
        );
    }
    for (entity, id) in companies.iter() {
        assert!(
            index.companies.insert(*id, entity).is_none(),
            "duplicate durable CompanyId {}",
            id.0
        );
    }
    index.initialized = true;
}

/// Maintain the durable mirrors of the live Entity relationships. Compatibility
/// recovery from old name-only state is deliberately conservative: ambiguous
/// duplicate names are left unresolved instead of silently paying or assigning
/// the wrong person.
#[allow(clippy::type_complexity)]
pub fn reconcile_stable_world_relationships(
    mut commands: Commands,
    settlements: Query<(Entity, &Settlement, &SettlementId)>,
    buildings: Query<(
        Entity,
        Ref<SettlementBuilding>,
        &BuildingId,
        Option<&OwnedBy>,
    )>,
    people: Query<(
        Entity,
        &CharacterName,
        &PersonId,
        &VillagerIntent,
        Option<&ResidentOf>,
        Option<&EmployedAt>,
    )>,
    changed_people: Query<
        (
            Entity,
            &VillagerIntent,
            Option<&HomeAssignment>,
            Option<&ResidentOf>,
            Option<&LivesAt>,
        ),
        Or<(Changed<VillagerIntent>, Changed<HomeAssignment>)>,
    >,
    unscoped_buildings: Query<(Entity, &SettlementBuilding), Without<BuildingOf>>,
    building_ids: Query<&BuildingId>,
    mut removed_homes: RemovedComponents<HomeAssignment>,
) {
    let settlement_by_entity: bevy::platform::collections::HashMap<Entity, SettlementId> =
        settlements
            .iter()
            .map(|(entity, _, id)| (entity, *id))
            .collect();
    let settlement_by_name: bevy::platform::collections::HashMap<&str, SettlementId> = settlements
        .iter()
        .map(|(_, settlement, id)| (settlement.name.as_str(), *id))
        .collect();

    for (entity, intent, home, resident_of, lives_at) in changed_people.iter() {
        if let Some(settlement_id) = intent
            .settlement()
            .and_then(|settlement| settlement_by_entity.get(&settlement).copied())
        {
            if resident_of.copied() != Some(ResidentOf(settlement_id)) {
                commands.entity(entity).insert(ResidentOf(settlement_id));
            }
        } else if resident_of.is_some() {
            commands.entity(entity).remove::<ResidentOf>();
        }

        let home_id = home.and_then(|home| building_ids.get(home.home()).ok().copied());
        if let Some(home_id) = home_id {
            if lives_at.copied() != Some(LivesAt(home_id)) {
                commands.entity(entity).insert(LivesAt(home_id));
            }
        } else if lives_at.is_some() {
            commands.entity(entity).remove::<LivesAt>();
        }
    }
    for entity in removed_homes.read() {
        if people.get(entity).is_ok() {
            commands.entity(entity).remove::<LivesAt>();
        }
    }

    // New buildings receive BuildingOf at creation. Only unresolved legacy
    // entities enter this migration query.
    for (building_entity, building) in unscoped_buildings.iter() {
        if let Some(settlement_id) = settlement_by_name
            .get(building.settlement.as_str())
            .copied()
        {
            commands
                .entity(building_entity)
                .insert(BuildingOf(settlement_id));
        }
    }

    // New construction writes OwnedBy directly. This branch migrates legacy
    // buildings only when one and only one resident matches the display name.
    for (building_entity, building, _, owner_id) in buildings.iter() {
        if !building.is_changed() || owner_id.is_some() {
            continue;
        }
        let settlement_id = settlement_by_name
            .get(building.settlement.as_str())
            .copied();
        let Some(owner) = building.owner.as_deref() else {
            continue;
        };
        let mut matches = people
            .iter()
            .filter(|(_, name, _, intent, resident_of, _)| {
                let person_settlement = resident_of.map(|resident| resident.0).or_else(|| {
                    intent
                        .settlement()
                        .and_then(|hall| settlement_by_entity.get(&hall).copied())
                });
                name.0 == owner && person_settlement == settlement_id
            });
        if let (Some(first), None) = (matches.next().map(|(_, _, id, ..)| *id), matches.next()) {
            commands.entity(building_entity).insert(OwnedBy(first));
        }
    }

    // Migrate the legacy worker-name roster once. New hiring writes EmployedAt
    // directly, so duplicate display names never become an authority again.
    for (_, building, building_id, _) in buildings.iter() {
        if !building.is_changed() {
            continue;
        }
        let settlement_id = settlement_by_name
            .get(building.settlement.as_str())
            .copied();
        for worker_name in &building.workers {
            let mut matches =
                people
                    .iter()
                    .filter(|(_, name, _, intent, resident_of, employed_at)| {
                        let person_settlement =
                            resident_of.map(|resident| resident.0).or_else(|| {
                                intent
                                    .settlement()
                                    .and_then(|hall| settlement_by_entity.get(&hall).copied())
                            });
                        name.0 == *worker_name
                            && person_settlement == settlement_id
                            && employed_at.is_none()
                    });
            if let (Some(first), None) = (matches.next().map(|(entity, ..)| entity), matches.next())
            {
                commands.entity(first).insert(EmployedAt(*building_id));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocator_observes_loaded_ids_and_never_reassigns_existing_people() {
        let mut app = App::new();
        app.init_resource::<WorldIdAllocator>()
            .add_systems(Update, assign_stable_world_ids);
        let loaded = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                CharacterName("Loaded".into()),
                PersonId(100),
            ))
            .id();
        let first_new = app
            .world_mut()
            .spawn((CharacterKind::Villager, CharacterName("First".into())))
            .id();
        let second_new = app
            .world_mut()
            .spawn((CharacterKind::Villager, CharacterName("Second".into())))
            .id();

        app.update();
        let ids = [loaded, first_new, second_new].map(|entity| {
            *app.world()
                .get::<PersonId>(entity)
                .expect("every person receives a durable id")
        });
        assert_eq!(ids[0], PersonId(100));
        assert!(ids[1].0 > 100 && ids[2].0 > 100);
        assert_ne!(ids[1], ids[2]);

        app.update();
        assert_eq!(app.world().get::<PersonId>(first_new), Some(&ids[1]));
        assert_eq!(app.world().get::<PersonId>(second_new), Some(&ids[2]));
    }

    #[test]
    #[should_panic(expected = "duplicate durable PersonId 9")]
    fn identity_index_refuses_ambiguous_durable_ids() {
        let mut app = App::new();
        app.init_resource::<WorldIdentityIndex>()
            .add_systems(Update, rebuild_world_identity_index);
        app.world_mut().spawn((
            CharacterKind::Villager,
            CharacterName("First".into()),
            PersonId(9),
        ));
        app.world_mut().spawn((
            CharacterKind::Villager,
            CharacterName("Second".into()),
            PersonId(9),
        ));

        app.update();
    }
}
