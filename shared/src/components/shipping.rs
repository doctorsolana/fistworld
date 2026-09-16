//! Public harbour geometry and durable company-owned ships. Navigation,
//! procurement and cargo custody remain authoritative server responsibilities.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::{
    BuildingId, CompanyId, SettlementId, SettlementTier, ShipId, ShipOrderId, TradeRouteId,
};
use crate::economy::Good;

mod geometry;
pub use geometry::*;
pub const MAX_COMPANY_SHIPS: usize = 8;

#[derive(
    Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord,
)]
pub enum ShipKind {
    #[default]
    Coaster,
    Cog,
}

impl ShipKind {
    pub const ALL: [Self; 2] = [Self::Coaster, Self::Cog];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Coaster => "Coaster",
            Self::Cog => "Cargo cog",
        }
    }

    /// Bulk, not individual items. Iron, stone and bread occupy different space.
    pub const fn capacity(self) -> u32 {
        match self {
            Self::Coaster => 480,
            Self::Cog => 1_200,
        }
    }

    /// The order is also the meaning of ShipConstructionOrder::delivered.
    pub const fn materials(self) -> [(Good, u32); 3] {
        match self {
            Self::Coaster => [(Good::Wood, 48), (Good::Iron, 8), (Good::Wool, 12)],
            Self::Cog => [(Good::Wood, 96), (Good::Iron, 20), (Good::Wool, 24)],
        }
    }

    pub const fn build_seconds(self) -> f32 {
        match self {
            Self::Coaster => 180.0,
            Self::Cog => 360.0,
        }
    }

    pub const fn length(self) -> f32 {
        match self {
            Self::Coaster => 6.0,
            Self::Cog => 9.0,
        }
    }

    pub const fn beam(self) -> f32 {
        match self {
            Self::Coaster => 2.2,
            Self::Cog => 3.2,
        }
    }

    pub const fn draft(self) -> f32 {
        match self {
            Self::Coaster => 0.65,
            Self::Cog => 1.0,
        }
    }

    /// Model height above flat water. Navigation additionally reserves swell.
    pub const fn mast_height(self) -> f32 {
        match self {
            Self::Coaster => 3.6,
            Self::Cog => 5.2,
        }
    }
}

/// All points are authored by the server survey. The shore point is dry land;
/// berth/departure are hull-centre positions at the local water surface.
/// A single reserved berth prevents two hulls being loaded in the same spot.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct PortGeometry {
    pub shore: Vec3,
    pub pier_end: Vec3,
    pub berth: Vec3,
    pub departure: Vec3,
    pub yaw: f32,
    pub maximum_ship: ShipKind,
}

#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct SettlementPort {
    pub settlement: SettlementId,
    pub geometry: PortGeometry,
    pub built: bool,
}

impl SettlementPort {
    pub const fn tier_allowed(tier: SettlementTier) -> bool {
        matches!(tier, SettlementTier::Town | SettlementTier::City)
    }

    pub fn accepts(self, kind: ShipKind) -> bool {
        self.built && self.geometry.valid() && kind <= self.geometry.maximum_ship
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShipStatus {
    #[default]
    Moored,
    Loading,
    Sailing,
    Unloading,
    WaitingForCrew,
    WaitingForCargo,
    WaitingForBerth,
    Blocked,
}

impl ShipStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Moored => "Moored",
            Self::Loading => "Loading",
            Self::Sailing => "Under sail",
            Self::Unloading => "Unloading",
            Self::WaitingForCrew => "Waiting for crew",
            Self::WaitingForCargo => "Waiting for cargo",
            Self::WaitingForBerth => "Waiting for a berth",
            Self::Blocked => "Passage unavailable",
        }
    }
}

#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompanyShip {
    pub company: CompanyId,
    pub kind: ShipKind,
    pub home_port: BuildingId,
    pub assigned_route: Option<TradeRouteId>,
    pub status: ShipStatus,
}

/// Transport assignment on the ordinary CompanyTradeRoute entity. Its absence
/// means a land caravan; route purpose, timetable and ledger are still shared.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaritimeTradeRoute {
    pub ship: ShipId,
}

/// Physical attachment of an employed sailor; independent of opening voyages.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct AboardShip {
    pub ship: ShipId,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShipOrderStatus {
    #[default]
    AwaitingMaterials,
    Hauling,
    Building,
    Completed,
    Cancelled,
}

impl ShipOrderStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::AwaitingMaterials => "Awaiting wood, iron and wool",
            Self::Hauling => "Delivering shipbuilding supplies",
            Self::Building => "Building hull",
            Self::Completed => "Launched",
            Self::Cancelled => "Cancelled",
        }
    }
}

#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShipConstructionOrder {
    pub company: CompanyId,
    pub port: BuildingId,
    pub kind: ShipKind,
    pub status: ShipOrderStatus,
    pub delivered: [u32; 3],
    /// Completed labour in thousandths; materials alone do not launch a ship.
    pub progress: u16,
}

/// Small company directory record. Physical hulls remain region-scoped while
/// the master can inspect and assign the fleet without chasing its camera.
#[derive(Component, Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct CompanyFleet {
    pub ships: Vec<(ShipId, CompanyShip)>,
    /// Actual ship holds, available to company UI outside the hull camera region.
    pub cargo: Vec<(ShipId, crate::economy::Good, u32)>,
    pub orders: Vec<(ShipOrderId, ShipConstructionOrder)>,
}

/// Public port availability accompanies the existing global town directory.
/// Pier geometry and detailed inventories remain on region-scoped entities.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettlementPortSummary {
    pub port: BuildingId,
    pub maximum_ship: ShipKind,
    pub built: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PortCargoOwner {
    Company(CompanyId),
    Treasury(SettlementId),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hull_classes_have_real_distinct_cost_and_capacity() {
        assert!(ShipKind::Cog.capacity() > ShipKind::Coaster.capacity());
        for kind in ShipKind::ALL {
            assert!(kind.materials().iter().all(|(_, amount)| *amount > 0));
            assert_eq!(
                kind.materials().map(|(good, _)| good),
                [Good::Wood, Good::Iron, Good::Wool]
            );
        }
        assert!(!SettlementPort::tier_allowed(SettlementTier::Village));
        assert!(SettlementPort::tier_allowed(SettlementTier::Town));
    }
    #[test]
    fn fleet_directory_roundtrip_retains_stable_assets_orders_and_partial_materials() {
        let fleet = CompanyFleet {
            cargo: vec![(ShipId(41), Good::Wood, 12)],
            ships: vec![(
                ShipId(71),
                CompanyShip {
                    company: CompanyId(3),
                    kind: ShipKind::Cog,
                    home_port: BuildingId(8),
                    assigned_route: Some(TradeRouteId(29)),
                    status: ShipStatus::WaitingForBerth,
                },
            )],
            orders: vec![(
                ShipOrderId(93),
                ShipConstructionOrder {
                    company: CompanyId(3),
                    port: BuildingId(10),
                    kind: ShipKind::Coaster,
                    status: ShipOrderStatus::Hauling,
                    delivered: [17, 3, 5],
                    progress: 315,
                },
            )],
        };
        let bytes = bincode::serialize(&fleet).unwrap();
        assert_eq!(bincode::deserialize::<CompanyFleet>(&bytes).unwrap(), fleet);
        let port = SettlementPort {
            settlement: SettlementId(4),
            built: true,
            geometry: PortGeometry {
                shore: Vec3::new(10., 2., 3.),
                pier_end: Vec3::new(10., 1., 23.),
                berth: Vec3::new(10., 0., 27.),
                departure: Vec3::new(22., 0., 27.),
                yaw: 0.3,
                maximum_ship: ShipKind::Coaster,
            },
        };
        let wire = bincode::serialize(&port).unwrap();
        let received: SettlementPort = bincode::deserialize(&wire).unwrap();
        assert_eq!(received, port);
        assert!(received.accepts(ShipKind::Coaster));
        assert!(!received.accepts(ShipKind::Cog));
        let mut unfinished = received;
        unfinished.built = false;
        assert!(!unfinished.accepts(ShipKind::Coaster));
    }
}
