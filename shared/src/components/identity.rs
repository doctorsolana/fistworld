use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::SettlementTier;

macro_rules! stable_id {
    ($name:ident) => {
        #[derive(
            Component,
            Serialize,
            Deserialize,
            Clone,
            Copy,
            Debug,
            Default,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
        )]
        pub struct $name(pub u64);

        impl $name {
            pub const UNASSIGNED: Self = Self(0);

            pub const fn is_assigned(self) -> bool {
                self.0 != 0
            }
        }
    };
}

stable_id!(PersonId);
stable_id!(SettlementId);
stable_id!(BuildingId);
stable_id!(PermitId);

/// Lightweight world-directory record. This is the only settlement data that
/// must be globally visible; markets, inventories, buildings and residents are
/// region-scoped detail.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SettlementSummary {
    pub id: SettlementId,
    pub name: String,
    pub tier: SettlementTier,
    pub residents: u32,
    pub treasury: u64,
    pub prosperity: f32,
    pub reserve_days: f32,
    pub houses: u16,
    pub farmsteads: u16,
    pub fishing_huts: u16,
    pub lumber_huts: u16,
    #[serde(default)]
    pub windmills: u16,
    #[serde(default)]
    pub bakeries: u16,
}

/// Durable settlement membership. Runtime AI may still hold a session-local
/// Entity for fast ECS access, but saves, histories and cross-region systems
/// join through this identifier.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ResidentOf(pub SettlementId);

/// Durable building-to-settlement relationship.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BuildingOf(pub SettlementId);

/// Durable road-to-settlement relationship. `VillageRoad::settlement` remains
/// a readable label for panels and logs only.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RoadOf(pub SettlementId);

/// Durable parent link for a building's authored adjuncts such as wheat fields
/// and fishing piers.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AttachedTo(pub BuildingId);

/// Durable private ownership. Display names remain on SettlementBuilding for
/// UI compatibility; this component is authoritative when money or rights move.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OwnedBy(pub PersonId);

/// Durable workplace assignment stored on the person rather than inferred
/// from a display-name roster.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EmployedAt(pub BuildingId);

/// Founding and municipal jobs live at the settlement rather than in a
/// private building roster, but still need the same durable identity as a
/// business assignment.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CivicRole {
    Reeve,
    /// Legacy pre-v1. The live staffing system migrates this separate job into
    /// [`Self::MootSteward`], but retaining the variant keeps old records
    /// readable.
    MarketPorter,
    /// Legacy pre-v1 road-only office. See [`Self::MootSteward`].
    RoadSteward,
    CityWorker,
    Guard,
    /// A founding public-works job which operates a Moot goods cart, audits
    /// the road network and builds or adopts missing connectors. A solvent
    /// Hamlet may staff two people in this same role.
    MootSteward,
}

impl CivicRole {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Reeve => "Reeve",
            Self::MarketPorter => "Market Porter (legacy)",
            Self::RoadSteward => "Road Steward (legacy)",
            Self::CityWorker => "City Worker",
            Self::Guard => "Guard",
            Self::MootSteward => "Moot Steward",
        }
    }
}

#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CivicEmployment {
    pub settlement: SettlementId,
    pub role: CivicRole,
}

/// Durable home assignment. The live server also keeps the house Entity for
/// fast door access, while this survives serialization and entity remapping.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LivesAt(pub BuildingId);
