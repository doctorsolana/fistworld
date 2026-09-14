//! One daily, identity-safe view of durable settlement development.
//!
//! Population, houses, operating firms and completed roads are aggregated for
//! all settlements together. No per-person timers or navigation searches are
//! needed, and a skipped calendar day never inherits today's housing state.

use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use shared::components::{
    BuildingId, BuildingOf, CharacterKind, ConstructionSite, EmployedAt, Health, OperatedBy,
    PersonId, PlayerPosition, PlayerRotation, ResidentOf, RoadOf, Settlement, SettlementBuilding,
    SettlementBuildingKind, SettlementDevelopmentEvidence, SettlementId, VillageRoad, WorldTime,
};
use shared::economy::{BusinessAccount, BusinessCondition};

use super::{HomeAssignment, UnderConstruction, VillagerIntent};
use crate::world::village_roads::{hall_connected_road_keys, road_point_key, RoadRequest};

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SettlementDevelopmentSample {
    /// Current structure, with activity from the latest exact ledger date.
    pub(crate) current: SettlementDevelopmentEvidence,
    /// The actual completed date reviewed at an observed day boundary. Initial
    /// samples and skipped dates cannot manufacture qualifying observations.
    pub(crate) completed: Option<(u32, SettlementDevelopmentEvidence)>,
}

#[derive(Resource, Default)]
pub(crate) struct SettlementDevelopmentSamples {
    pub(crate) by_settlement: HashMap<SettlementId, SettlementDevelopmentSample>,
    last_sample_day: Option<u32>,
}

#[derive(Default)]
struct Aggregate {
    evidence: SettlementDevelopmentEvidence,
    occupied_homes: HashSet<BuildingId>,
    business_types: HashSet<SettlementBuildingKind>,
}

/// Spatially indexed starts of fully built Hall-connected roads. These use
/// exactly the Road Steward's completed graph and 0.75 m doorway tolerance,
/// so evaluating several markets does not rebuild or rescan their road graph.
#[derive(Default)]
struct ConnectedEntrances {
    buckets: HashMap<(i32, i32), Vec<Vec2>>,
}

impl ConnectedEntrances {
    fn key(point: Vec2) -> (i32, i32) {
        (point.x.floor() as i32, point.y.floor() as i32)
    }

    fn from_roads(hall_door: Vec2, roads: &[&VillageRoad]) -> Self {
        let connected = hall_connected_road_keys(hall_door, roads);
        let mut entrances = Self::default();
        for road in roads.iter().copied().filter(|road| road.is_complete()) {
            if !road
                .built_points()
                .iter()
                .any(|point| connected.contains(&road_point_key(*point)))
            {
                continue;
            }
            if let Some(start) = road.points.first().copied() {
                entrances
                    .buckets
                    .entry(Self::key(start))
                    .or_default()
                    .push(start);
            }
        }
        entrances
    }

    fn contains(&self, door: Vec2) -> bool {
        let (x, z) = Self::key(door);
        (-1..=1).any(|dx| {
            (-1..=1).any(|dz| {
                self.buckets.get(&(x + dx, z + dz)).is_some_and(|starts| {
                    starts
                        .iter()
                        .any(|start| start.distance_squared(door) <= 0.75_f32.powi(2))
                })
            })
        })
    }
}

#[allow(clippy::type_complexity)]
pub(crate) fn aggregate_settlement_development(
    clock: Query<&WorldTime>,
    mut samples: ResMut<SettlementDevelopmentSamples>,
    halls: Query<
        (
            Entity,
            &SettlementId,
            &PlayerPosition,
            Option<&PlayerRotation>,
        ),
        With<Settlement>,
    >,
    buildings: Query<
        (
            Entity,
            &BuildingId,
            &BuildingOf,
            &SettlementBuilding,
            Option<&PlayerPosition>,
            Option<&PlayerRotation>,
            Option<&BusinessAccount>,
            Option<&BusinessCondition>,
            Option<&OperatedBy>,
            Has<RoadRequest>,
        ),
        (Without<UnderConstruction>, Without<ConstructionSite>),
    >,
    people: Query<
        (
            &PersonId,
            &Health,
            &ResidentOf,
            &VillagerIntent,
            Option<&HomeAssignment>,
            Option<&EmployedAt>,
        ),
        With<CharacterKind>,
    >,
    roads: Query<(&RoadOf, &VillageRoad)>,
) {
    let Some(day) = clock.iter().next().map(|clock| clock.day) else {
        return;
    };
    if samples.last_sample_day == Some(day) {
        return;
    }
    let previous_day = samples.last_sample_day.replace(day);
    let completed_day = previous_day.filter(|previous| previous.checked_add(1) == Some(day));
    let ledger_day = day.saturating_sub(1);

    let hall_ids: HashMap<_, _> = halls.iter().map(|(entity, id, ..)| (entity, *id)).collect();
    let mut aggregates: HashMap<_, Aggregate> = halls
        .iter()
        .map(|(_, id, ..)| (*id, Aggregate::default()))
        .collect();
    let mut roads_by_settlement: HashMap<_, Vec<_>> = HashMap::new();
    for (of, road) in &roads {
        if aggregates.contains_key(&of.0) {
            roads_by_settlement.entry(of.0).or_default().push(road);
        }
    }
    let entrances: HashMap<_, _> = halls
        .iter()
        .map(|(_, id, position, rotation)| {
            let hall_door = SettlementBuildingKind::Hall
                .entrance_position(position.0, rotation.map_or(0.0, |rotation| rotation.0))
                .xz();
            let network = ConnectedEntrances::from_roads(
                hall_door,
                roads_by_settlement.get(id).map_or(&[], Vec::as_slice),
            );
            (*id, network)
        })
        .collect();

    let mut living_people = HashSet::new();
    let mut staffed_buildings = HashSet::new();
    for (person, health, resident, intent, home, employment) in &people {
        if health.is_dead()
            || !intent.counts_as_resident()
            || intent.settlement().and_then(|hall| hall_ids.get(&hall)) != Some(&resident.0)
            || !living_people.insert(*person)
        {
            continue;
        }
        let Some(aggregate) = aggregates.get_mut(&resident.0) else {
            continue;
        };
        aggregate.evidence.residents = aggregate.evidence.residents.saturating_add(1);
        if let Some(employment) = employment {
            staffed_buildings.insert(employment.0);
        }
        let Some((_, id, of, building, ..)) = home.and_then(|home| buildings.get(home.home()).ok())
        else {
            continue;
        };
        if building.kind == SettlementBuildingKind::House && of.0 == resident.0 {
            aggregate.evidence.housed_residents =
                aggregate.evidence.housed_residents.saturating_add(1);
            aggregate.occupied_homes.insert(*id);
        }
    }

    for (_, id, of, building, position, rotation, account, condition, company, road_requested) in
        &buildings
    {
        let Some(aggregate) = aggregates.get_mut(&of.0) else {
            continue;
        };
        if building.kind == SettlementBuildingKind::Market && !road_requested {
            if let Some(position) = position {
                let door = building
                    .kind
                    .entrance_position(position.0, rotation.map_or(0.0, |rotation| rotation.0))
                    .xz();
                aggregate.evidence.market_accessible |= entrances
                    .get(&of.0)
                    .is_some_and(|network| network.contains(door));
            }
        }
        if !super::is_private_business(building.kind)
            || company.is_none()
            || !staffed_buildings.contains(id)
            || condition.is_none_or(|condition| !condition.state.can_operate())
        {
            continue;
        }
        let Some(ledger) = account.and_then(|account| account.ledger_for_day(ledger_day)) else {
            continue;
        };
        // sold_units also includes transfers between a company's own sites.
        // Only real production or actual buyer payments establish activity.
        if ledger.produced_units > 0 || ledger.gross_revenue > 0 {
            aggregate.business_types.insert(building.kind);
            aggregate.evidence.paid_trade_pennies = aggregate
                .evidence
                .paid_trade_pennies
                .saturating_add(ledger.gross_revenue);
        }
    }

    let old_samples = std::mem::take(&mut samples.by_settlement);
    samples.by_settlement = aggregates
        .into_iter()
        .map(|(id, aggregate)| {
            let evidence = SettlementDevelopmentEvidence {
                occupied_homes: aggregate.occupied_homes.len().min(u32::MAX as usize) as u32,
                operating_business_types: aggregate.business_types.len().min(u8::MAX as usize)
                    as u8,
                ..aggregate.evidence
            };
            // A settlement founded since the last sample has no observed
            // completed date of its own, even when the global clock advanced.
            let completed = completed_day
                .filter(|_| old_samples.contains_key(&id))
                .map(|day| (day, evidence));
            (
                id,
                SettlementDevelopmentSample {
                    current: evidence,
                    completed,
                },
            )
        })
        .collect();
}

#[cfg(test)]
#[path = "development_evidence_tests.rs"]
mod tests;
