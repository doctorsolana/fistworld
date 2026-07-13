use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// First-pass cargo set for the rail tycoon pivot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CargoKind {
    Passengers,
    Mail,
    Wood,
    Coal,
    Grain,
    IronOre,
    Lumber,
    Goods,
}

impl CargoKind {
    pub const ALL: [CargoKind; 8] = [
        CargoKind::Passengers,
        CargoKind::Mail,
        CargoKind::Wood,
        CargoKind::Coal,
        CargoKind::Grain,
        CargoKind::IronOre,
        CargoKind::Lumber,
        CargoKind::Goods,
    ];

    pub fn display_name(self) -> &'static str {
        match self {
            CargoKind::Passengers => "Passengers",
            CargoKind::Mail => "Mail",
            CargoKind::Wood => "Wood",
            CargoKind::Coal => "Coal",
            CargoKind::Grain => "Grain",
            CargoKind::IronOre => "Iron Ore",
            CargoKind::Lumber => "Lumber",
            CargoKind::Goods => "Goods",
        }
    }
}

/// Neutral map places that produce or consume cargo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IndustryKind {
    Town,
    Forest,
    CoalMine,
    IronMine,
    Farm,
    Sawmill,
    Factory,
}

impl IndustryKind {
    pub const ALL: [IndustryKind; 7] = [
        IndustryKind::Town,
        IndustryKind::Forest,
        IndustryKind::CoalMine,
        IndustryKind::IronMine,
        IndustryKind::Farm,
        IndustryKind::Sawmill,
        IndustryKind::Factory,
    ];

    pub fn display_name(self) -> &'static str {
        match self {
            IndustryKind::Town => "Town",
            IndustryKind::Forest => "Forest",
            IndustryKind::CoalMine => "Coal Mine",
            IndustryKind::IronMine => "Iron Mine",
            IndustryKind::Farm => "Farm",
            IndustryKind::Sawmill => "Sawmill",
            IndustryKind::Factory => "Factory",
        }
    }

    pub fn primary_output(self) -> Option<CargoKind> {
        match self {
            IndustryKind::Town => Some(CargoKind::Passengers),
            IndustryKind::Forest => Some(CargoKind::Wood),
            IndustryKind::CoalMine => Some(CargoKind::Coal),
            IndustryKind::IronMine => Some(CargoKind::IronOre),
            IndustryKind::Farm => Some(CargoKind::Grain),
            IndustryKind::Sawmill => Some(CargoKind::Lumber),
            IndustryKind::Factory => Some(CargoKind::Goods),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CargoAmount {
    pub kind: CargoKind,
    pub amount: f32,
}

impl CargoAmount {
    pub fn new(kind: CargoKind, amount: f32) -> Self {
        Self { kind, amount }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CargoDemand {
    pub kind: CargoKind,
    pub desired_per_tick: f32,
}

#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EconomyInventory {
    pub cargo: Vec<CargoAmount>,
}

impl Default for EconomyInventory {
    fn default() -> Self {
        Self { cargo: Vec::new() }
    }
}

pub const STARTING_COMPANY_MONEY: i64 = 50_000;
pub const TRACK_COST_PER_METER: i64 = 12;
pub const STATION_COST: i64 = 8_000;
pub const TRAIN_COST: i64 = 14_000;
pub const BASE_DELIVERY_REVENUE: i64 = 500;
pub const REVENUE_PER_METER: f32 = 0.85;
