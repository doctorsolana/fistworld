//! Settlement identity, development layouts and progression state.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// A settlement: the economic atom, and a PLACE rather than a set of buildings.
///
/// Buildings are how its plan gets expressed; this is what constitutes it. See
/// WORLD-DESIGN section 1.
///
/// The hall entity is region-scoped detail. A separate [`super::SettlementSummary`]
/// carries the small globally visible map record, joined through [`super::SettlementId`].
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Settlement {
    /// Chosen by whoever founded it. Naming a place is the first act of
    /// ownership the game offers, so a generator only ever SUGGESTS.
    pub name: String,
    pub tier: SettlementTier,
    /// How many people live here.
    ///
    /// A COUNT on the wire, not the roster itself: the roster is server truth
    /// and can be long, while every client needs the number for the map screen.
    /// Nobody sets this by hand -- residents arrive on their own feet and the
    /// server counts them.
    pub residents: u32,
    /// Local coin. Permit fees land here, including for independent
    /// settlements: a place's income is its own, and belongs to nobody else.
    /// Market fees, positive-profit levies and public-sale receipts join that
    /// income; wages, relief and public procurement spend the same real cash.
    pub treasury: u64,
}

/// Seed-selected settlement morphology. It biases future candidate plots but
/// never relocates a structure that is already built.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SettlementLayoutStyle {
    #[default]
    Organic,
    Radial,
    Grid,
    Avenue,
    Polycentric,
}

impl SettlementLayoutStyle {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Organic => "ORGANIC LANES",
            Self::Radial => "RADIAL COMMONS",
            Self::Grid => "ORDERED GRID",
            Self::Avenue => "GREAT AVENUE",
            Self::Polycentric => "NEIGHBOURHOOD CLUSTERS",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SettlementCenterStyle {
    #[default]
    Green,
    Square,
    Avenue,
    Courtyard,
}

impl SettlementCenterStyle {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Green => "VILLAGE GREEN",
            Self::Square => "MARKET SQUARE",
            Self::Avenue => "CIVIC AVENUE",
            Self::Courtyard => "CIVIC COURTYARD",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SettlementWallStyle {
    #[default]
    Organic,
    Round,
    Square,
    DistrictFitted,
}

impl SettlementWallStyle {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Organic => "ORGANIC",
            Self::Round => "ROUND",
            Self::Square => "SQUARE",
            Self::DistrictFitted => "DISTRICT-FITTED",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SettlementProgressGate {
    #[default]
    FoodSecurity,
    Population,
    Marketplace,
    Tavern,
    Trade,
    Prosperity,
    Church,
    Sustaining,
    CivicHallMaterials,
    CivicHallConstruction,
    Complete,
}

impl SettlementProgressGate {
    pub const fn label(self) -> &'static str {
        match self {
            Self::FoodSecurity => "secure food supply",
            Self::Population => "more permanent residents",
            Self::Marketplace => "build a marketplace",
            Self::Tavern => "build a tavern",
            Self::Trade => "operate the local market",
            Self::Prosperity => "raise prosperity",
            Self::Church => "build a church",
            Self::Sustaining => "sustain all requirements",
            Self::CivicHallMaterials => "buy and stage materials for the Hall upgrade",
            Self::CivicHallConstruction => "construct the Hall upgrade",
            Self::Complete => "highest settlement tier reached",
        }
    }
}

/// Replicated, inspectable settlement plan and promotion ledger.
///
/// The seed fixes a settlement's planning temperament. Demand and permits still
/// decide how many farms, houses and businesses exist, so the seed is a set of
/// preferences rather than a pre-baked city.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SettlementDevelopment {
    pub plan_seed: u64,
    pub layout: SettlementLayoutStyle,
    pub center: SettlementCenterStyle,
    pub inner_wall: SettlementWallStyle,
    pub outer_wall: SettlementWallStyle,
    pub next_gate: SettlementProgressGate,
    pub progress_days: u16,
    pub required_days: u16,
    pub last_progress_day: u32,
    pub last_road_work_day: u32,
    pub dirt_roads: u16,
    pub stone_roads: u16,
    pub stone_committed: u32,
    pub stone_needed: u32,
}

impl SettlementDevelopment {
    pub fn from_foundation(name: &str, position: Vec3, day: u32) -> Self {
        let mut seed = 0xcbf2_9ce4_8422_2325_u64;
        for byte in name.bytes() {
            seed ^= u64::from(byte);
            seed = seed.wrapping_mul(0x0000_0100_0000_01b3);
        }
        seed ^= u64::from(position.x.to_bits()).rotate_left(17);
        seed ^= u64::from(position.z.to_bits()).rotate_left(41);
        Self::from_seed(seed, day)
    }

    /// Reproduce a charter directly in diagnostics without changing its rules.
    pub fn from_seed(seed: u64, day: u32) -> Self {
        // Small hand-entered seeds deserve the same trait variation as hashed
        // foundation names. Preserve the original seed as the replay identity.
        let traits = crate::worldgen::splitmix64(seed);
        let choose = |shift: u32, count: u64| (traits.rotate_right(shift) % count) as u8;
        let layout = match choose(0, 5) {
            0 => SettlementLayoutStyle::Organic,
            1 => SettlementLayoutStyle::Radial,
            2 => SettlementLayoutStyle::Grid,
            3 => SettlementLayoutStyle::Avenue,
            _ => SettlementLayoutStyle::Polycentric,
        };
        let center = match choose(11, 4) {
            0 => SettlementCenterStyle::Green,
            1 => SettlementCenterStyle::Square,
            2 => SettlementCenterStyle::Avenue,
            _ => SettlementCenterStyle::Courtyard,
        };
        let wall = |value| match value {
            0 => SettlementWallStyle::Organic,
            1 => SettlementWallStyle::Round,
            2 => SettlementWallStyle::Square,
            _ => SettlementWallStyle::DistrictFitted,
        };
        let inner_wall = wall(choose(23, 4));
        let mut outer_wall = wall(choose(37, 4));
        if outer_wall == inner_wall {
            outer_wall = wall((choose(37, 4) + 1) % 4);
        }
        Self {
            plan_seed: seed,
            layout,
            center,
            inner_wall,
            outer_wall,
            next_gate: SettlementProgressGate::FoodSecurity,
            progress_days: 0,
            required_days: crate::economy::VILLAGE_REQUIRED_SECURE_DAYS,
            last_progress_day: day,
            last_road_work_day: day,
            dirt_roads: 0,
            stone_roads: 0,
            stone_committed: 0,
            stone_needed: 0,
        }
    }
}

/// The rungs a settlement climbs. Every step asks for something the step below
/// did not -- see the tier table in WORLD-DESIGN section 1.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum SettlementTier {
    /// Reached by DESTRUCTION, deliberate razing, or long physical decay after
    /// abandonment -- never by economic decline alone.
    Ruins,
    /// A city hall and not much else. Where founding lands you.
    #[default]
    Hamlet,
    /// Feeds itself.
    Village,
    /// Trade and defence.
    Town,
    /// Reserved for future progression; retained for existing wire/save values.
    City,
}

impl SettlementTier {
    pub fn label(self) -> &'static str {
        match self {
            SettlementTier::Ruins => "RUINS",
            SettlementTier::Hamlet => "HAMLET",
            SettlementTier::Village => "VILLAGE",
            SettlementTier::Town => "TOWN",
            SettlementTier::City => "CITY",
        }
    }

    /// What this rung must acquire to reach the next one. `None` at the top.
    pub fn next_requirement(self) -> Option<&'static str> {
        match self {
            SettlementTier::Ruins => Some("refounding"),
            SettlementTier::Hamlet => Some("food security: fed, grown here or bought in"),
            SettlementTier::Village => Some("external trade and administration: a working market"),
            SettlementTier::Town | SettlementTier::City => None,
        }
    }

    pub const fn public_guard_positions(self) -> u8 {
        match self {
            SettlementTier::Ruins => 0,
            SettlementTier::Hamlet => 0,
            SettlementTier::Village | SettlementTier::Town | SettlementTier::City => 2,
        }
    }

    pub const fn public_worker_positions(self) -> u8 {
        match self {
            SettlementTier::Ruins => 0,
            SettlementTier::Hamlet
            | SettlementTier::Village
            | SettlementTier::Town
            | SettlementTier::City => 2,
        }
    }
}
