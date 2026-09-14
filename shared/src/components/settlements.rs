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
    Housing,
    OccupiedHomes,
    BusinessActivity,
}

impl SettlementProgressGate {
    pub const fn label(self) -> &'static str {
        match self {
            Self::FoodSecurity => "secure food supply",
            Self::Population => "more permanent residents",
            Self::Marketplace => "complete an accessible marketplace",
            Self::Tavern => "build a tavern",
            Self::Trade => "complete recent paid trade",
            Self::Prosperity => "raise prosperity",
            Self::Church => "build a church",
            Self::Sustaining => "qualify on two of the last three days",
            Self::CivicHallMaterials => "buy and stage materials for the Hall upgrade",
            Self::CivicHallConstruction => "construct the Hall upgrade",
            Self::Complete => "highest settlement tier reached",
            Self::Housing => "house more permanent residents",
            Self::OccupiedHomes => "establish occupied homes",
            Self::BusinessActivity => "operate different business types",
        }
    }
}

/// Compact structural evidence, aggregated by the authoritative daily economy.
/// Housed residents occupy completed homes in their own settlement. Activity
/// and paid trade refer to dated observations, never lifetime account totals.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SettlementDevelopmentEvidence {
    pub residents: u32,
    pub housed_residents: u32,
    pub occupied_homes: u32,
    pub operating_business_types: u8,
    pub market_accessible: bool,
    pub paid_trade_pennies: u64,
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
    /// Qualifying completed dates in the rolling window; never material units.
    pub progress_days: u16,
    pub required_days: u16,
    /// The last observed completed date plus one, initialized to foundation day.
    pub last_progress_day: u32,
    pub last_road_work_day: u32,
    pub dirt_roads: u16,
    pub stone_roads: u16,
    pub stone_committed: u32,
    pub stone_needed: u32,
    pub evidence: SettlementDevelopmentEvidence,
    /// Low bit is the latest sampled completed day; skipped dates shift in zero.
    pub qualification_bits: u8,
    pub material_staged: u32,
    pub material_required: u32,
}

impl SettlementDevelopment {
    /// Add one honest calendar observation. Repeated or pre-foundation samples
    /// do not count; gaps expire old evidence without copying current conditions.
    pub fn record_qualification_day(&mut self, completed_day: u32, qualifies: bool) {
        let Some(stamp) = completed_day.checked_add(1) else {
            return;
        };
        if stamp <= self.last_progress_day {
            return;
        }
        let elapsed = stamp.saturating_sub(self.last_progress_day);
        let window = crate::economy::DEVELOPMENT_WINDOW_DAYS;
        self.qualification_bits = if elapsed >= u32::from(window) {
            0
        } else {
            self.qualification_bits << elapsed
        };
        self.qualification_bits =
            (self.qualification_bits | u8::from(qualifies)) & ((1 << window) - 1);
        self.progress_days = self.qualification_bits.count_ones() as u16;
        self.last_progress_day = stamp;
    }

    pub fn reset_qualification(&mut self, day: u32) {
        self.qualification_bits = 0;
        self.progress_days = 0;
        self.last_progress_day = day;
    }

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
            next_gate: SettlementProgressGate::Population,
            progress_days: 0,
            required_days: crate::economy::DEVELOPMENT_REQUIRED_DAYS,
            last_progress_day: day,
            last_road_work_day: day,
            dirt_roads: 0,
            stone_roads: 0,
            stone_committed: 0,
            stone_needed: 0,
            evidence: SettlementDevelopmentEvidence::default(),
            qualification_bits: 0,
            material_staged: 0,
            material_required: 0,
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
    /// An established residential settlement with a completed Village Hall.
    Village,
    /// Housing and active commerce supported by a completed Town Hall.
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
            SettlementTier::Hamlet => Some("established residents and occupied homes"),
            SettlementTier::Village => Some("housing, operating businesses and paid trade"),
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

#[cfg(test)]
mod development_tests {
    use super::*;

    #[test]
    fn qualification_window_keeps_one_bad_day_and_expires_calendar_gaps() {
        let mut development = SettlementDevelopment::from_seed(0, 0);
        development.record_qualification_day(0, true);
        development.record_qualification_day(1, false);
        development.record_qualification_day(2, true);
        assert_eq!(
            (development.qualification_bits, development.progress_days),
            (0b101, 2)
        );
        development.record_qualification_day(2, true);
        development.record_qualification_day(1, true);
        assert_eq!(
            (development.qualification_bits, development.progress_days),
            (0b101, 2)
        );
        development.record_qualification_day(5, true);
        assert_eq!(
            (development.qualification_bits, development.progress_days),
            (1, 1)
        );
        development.record_qualification_day(6, true);
        assert_eq!(development.progress_days, 2);
    }

    #[test]
    fn qualification_ignores_pre_foundation_dates_and_resets_at_new_tier() {
        let mut development = SettlementDevelopment::from_seed(0, 40);
        development.record_qualification_day(39, true);
        assert_eq!(development.progress_days, 0);
        development.record_qualification_day(40, true);
        assert_eq!(development.progress_days, 1);
        development.reset_qualification(42);
        development.record_qualification_day(41, true);
        assert_eq!(development.progress_days, 0);
        development.record_qualification_day(42, true);
        assert_eq!(development.progress_days, 1);
        development.record_qualification_day(u32::MAX, true);
        assert_eq!(development.progress_days, 1);
    }

    #[test]
    fn structural_evidence_and_material_progress_roundtrip_independently() {
        let mut development = SettlementDevelopment::from_seed(u64::MAX, u32::MAX - 2);
        development.evidence = SettlementDevelopmentEvidence {
            residents: u32::MAX,
            housed_residents: u32::MAX - 1,
            occupied_homes: 999,
            operating_business_types: u8::MAX,
            market_accessible: true,
            paid_trade_pennies: u64::MAX,
        };
        development.record_qualification_day(u32::MAX - 2, true);
        development.material_staged = 5;
        development.material_required = 12;
        development.next_gate = SettlementProgressGate::CivicHallMaterials;
        let bytes = bincode::serialize(&development).unwrap();
        let decoded: SettlementDevelopment = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded, development);
        assert_eq!((decoded.progress_days, decoded.material_staged), (1, 5));
        for gate in [
            SettlementProgressGate::Housing,
            SettlementProgressGate::OccupiedHomes,
            SettlementProgressGate::BusinessActivity,
        ] {
            assert_eq!(
                bincode::deserialize::<SettlementProgressGate>(&bincode::serialize(&gate).unwrap())
                    .unwrap(),
                gate
            );
        }
    }
}
