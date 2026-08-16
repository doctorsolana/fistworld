//! Durable inter-settlement trade contracts and company route assets.
//!
//! A route belongs to an ordinary company. There is intentionally no special
//! "trade company" category: a quarry concern, bakery or warehouse merchant
//! may all own the same asset and use the same accounting rules.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::{BuildingId, CompanyId, PersonId, SettlementId, TradeContractId};
use crate::economy::{Good, MarketSeller};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TradeContractStatus {
    #[default]
    Open,
    Assigned,
    InTransit,
    Fulfilled,
    Cancelled,
}

impl TradeContractStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Open => "Awaiting carrier",
            Self::Assigned => "Carrier assigned",
            Self::InTransit => "Cargo in transit",
            Self::Fulfilled => "Fulfilled",
            Self::Cancelled => "Cancelled",
        }
    }

    pub const fn is_active(self) -> bool {
        matches!(self, Self::Open | Self::Assigned | Self::InTransit)
    }
}

/// A real civic buyer order backed by cash removed from the destination
/// treasury. Source sellers are paid at collection; unused escrow returns to
/// the destination when the contract closes.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CivicTradeContract {
    /// Filled once a real seller has enough listed stock to accept the order.
    /// `None` is a public, cash-backed tender visible to potential suppliers.
    pub origin: Option<SettlementId>,
    pub destination: SettlementId,
    pub good: Good,
    pub source_seller: Option<MarketSeller>,
    pub requested_units: u32,
    pub delivered_units: u32,
    pub maximum_unit_price: u64,
    pub delivery_fee_per_bulk: u64,
    pub reserved_cash: u64,
    pub escrow_cash: u64,
    pub spent_on_goods: u64,
    pub spent_on_freight: u64,
    pub created_day: u32,
    pub last_attempt_day: u32,
    pub status: TradeContractStatus,
}

impl CivicTradeContract {
    pub const fn remaining_units(self) -> u32 {
        self.requested_units.saturating_sub(self.delivered_units)
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TradeRouteStatus {
    #[default]
    WaitingForPorter,
    GoingToOrigin,
    Loading,
    InTransit,
    Returning,
    Idle,
    Mothballed,
}

impl TradeRouteStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::WaitingForPorter => "Waiting for porter",
            Self::GoingToOrigin => "Going to origin market",
            Self::Loading => "Loading cargo",
            Self::InTransit => "Travelling to destination",
            Self::Returning => "Returning to warehouse",
            Self::Idle => "Idle",
            Self::Mothballed => "Mothballed",
        }
    }
}

/// A reusable logistics asset owned by an ordinary company. The first version
/// serves buyer-funded contracts; the price fields deliberately leave the
/// seam needed by later merchant speculation without creating another route
/// type.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompanyTradeRoute {
    pub company: CompanyId,
    pub warehouse: BuildingId,
    pub origin: SettlementId,
    pub destination: SettlementId,
    pub good: Good,
    pub cargo_target: u32,
    pub maximum_purchase_price: u64,
    pub minimum_destination_price: u64,
    pub automatic: bool,
    pub active_contract: Option<TradeContractId>,
    pub assigned_caravaner: Option<PersonId>,
    pub status: TradeRouteStatus,
    pub completed_trips: u32,
    pub lifetime_units: u32,
    pub lifetime_delivery_revenue: u64,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TradeRouteTrip {
    pub departed_day: u32,
    pub completed_day: u32,
    pub units: u32,
    /// Buyer-funded source purchase, retained for route audit rather than
    /// counted as an expense of the carrier company.
    pub source_purchase_cost: u64,
    pub source_market_fees: u64,
    pub delivery_revenue: u64,
    pub travel_world_seconds: u32,
}

pub const MAX_TRADE_ROUTE_TRIPS: usize = 32;

#[derive(Component, Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct TradeRouteHistory {
    trips: Vec<TradeRouteTrip>,
}

impl TradeRouteHistory {
    pub fn trips(&self) -> &[TradeRouteTrip] {
        &self.trips
    }

    pub fn record(&mut self, trip: TradeRouteTrip) {
        if self.trips.len() == MAX_TRADE_ROUTE_TRIPS {
            self.trips.remove(0);
        }
        self.trips.push(trip);
    }
}
