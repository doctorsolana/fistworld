use std::collections::BTreeMap;

use bevy::prelude::*;
use shared::components::{BuildingId, HouseAppearance, HouseholdId, PersonId, SettlementId};
use shared::economy::{CarriedLoad, Good, GoodsInventory};

pub const fn required_escrow_pennies() -> u64 {
    shared::components::HOUSE_UPGRADE_ESCROW_PENNIES
}

/// Project ownership outlives a missing/despawned visual worksite, so its
/// remaining cash and purchased materials can always be settled exactly once.
#[derive(Resource, Default)]
pub struct HouseUpgradeProjects {
    pub(super) entries: BTreeMap<BuildingId, UpgradeProject>,
    /// Rotates the one permitted roster search per tick, including under warp.
    pub(super) last_worker_reviewed: Option<BuildingId>,
}

impl HouseUpgradeProjects {
    pub fn contains_house(&self, house: BuildingId) -> bool {
        self.entries.contains_key(&house)
    }

    pub fn pending_in_settlement(&self, settlement: SettlementId) -> usize {
        self.entries
            .values()
            .filter(|project| project.settlement == settlement && project.phase != Phase::Refunding)
            .count()
    }

    pub fn carried_load_for_worker(&self, worker: Entity) -> Option<CarriedLoad> {
        self.entries
            .values()
            .find(|project| project.worker == Some(worker))
            .map(|project| CarriedLoad::from_inventory(&project.cargo))
    }

    pub fn escrow_in_settlement(&self, settlement: SettlementId) -> u64 {
        self.entries
            .values()
            .filter(|project| project.settlement == settlement)
            .map(|project| project.escrow)
            .sum()
    }

    pub fn transit_wood_in_settlement(&self, settlement: SettlementId) -> u32 {
        self.entries
            .values()
            .filter(|project| project.settlement == settlement)
            .map(|project| project.cargo.amount(Good::Wood))
            .sum()
    }

    #[cfg(test)]
    pub fn total_escrow_pennies(&self) -> u64 {
        self.entries.values().map(|project| project.escrow).sum()
    }

    /// Paid material in transit or awaiting an orphaned project's refund.
    /// Delivered piles remain in the worksite's ordinary GoodsInventory.
    #[cfg(test)]
    pub fn transit_wood(&self) -> u32 {
        self.entries
            .values()
            .map(|project| project.cargo.amount(Good::Wood))
            .sum()
    }
}

/// A temporary private construction contract, never municipal employment or
/// a change to the worker's household/personal inventory ownership.
#[derive(Component, Clone, Copy, Debug)]
pub struct HouseUpgradeBuilderRoutine {
    pub project: Entity,
    pub carrying: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Waiting,
    ToMarket,
    ToSite,
    Working,
    Refunding,
}

#[derive(Default)]
pub(super) struct DemandClaim {
    pub epoch: Option<u64>,
    pub unavailable: u32,
    pub unaffordable: u32,
    pub funded: u32,
}

pub(super) struct UpgradeProject {
    pub house: BuildingId,
    pub home: Entity,
    pub owner: PersonId,
    pub owner_entity: Entity,
    pub estate_household: Option<HouseholdId>,
    pub settlement: SettlementId,
    pub hall: Entity,
    pub worksite: Entity,
    pub original: HouseAppearance,
    pub target: HouseAppearance,
    pub position: Vec3,
    pub rotation: f32,
    pub stand: Vec3,
    pub market_stand: Vec3,
    pub escrow: u64,
    pub paid_labor: u64,
    pub cargo: GoodsInventory,
    /// A recovery mirror of the project-owned physical pile. Only this
    /// lifecycle mutates it; an externally despawned site cannot erase title.
    pub delivered: u32,
    pub worker: Option<Entity>,
    pub phase: Phase,
    pub work_done: f32,
    pub travel_left: f32,
    pub last_time: f64,
    pub next_attempt: f64,
    pub claim: DemandClaim,
}
