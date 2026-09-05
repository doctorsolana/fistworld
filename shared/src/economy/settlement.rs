//! Household provisions, settlement economic state and development gates.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Shared necessities budget attached to a completed house.
///
/// Personal wallets still exist for permits and discretionary purchases. A
/// household contributes only enough for its pantry target, so one resident
/// can shop without pretending every dependant owns an equal share of cash.
#[derive(Component, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HouseholdEconomy {
    pub pennies: u64,
    pub pantry_target_days: u8,
    /// Stable identity of the resident currently responsible for provisions.
    /// Character names are presentation and are never used to find a wallet.
    #[serde(default)]
    pub shopper: Option<crate::components::PersonId>,
    pub last_budget_day: u32,
}

impl Default for HouseholdEconomy {
    fn default() -> Self {
        Self {
            pennies: 0,
            pantry_target_days: 3,
            shopper: None,
            last_budget_day: u32::MAX,
        }
    }
}

/// Replicated settlement-level reading of the physical food loop.
///
/// Stocks remain in [`GoodsInventory`]; this is the small derived summary used
/// by autonomous planning, tier advancement and the settlement panel. Recent
/// rates are rolling daily averages rather than lifetime totals.
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettlementEconomy {
    pub edible_stock: u32,
    pub reserve_days: f32,
    pub recent_food_production: f32,
    pub recent_food_consumption: f32,
    pub unmet_food: u32,
    pub observed_days: u16,
    pub food_secure_days: u16,
    pub reserve_prosperity: f32,
    pub production_prosperity: f32,
    pub housing_prosperity: f32,
    pub employment_prosperity: f32,
    pub hunger_penalty: f32,
    pub prosperity: f32,
    #[serde(default)]
    pub private_job_positions: u16,
    #[serde(default)]
    pub private_filled_jobs: u16,
    #[serde(default)]
    pub private_vacant_jobs: u16,
    #[serde(default)]
    pub civic_job_positions: u16,
    #[serde(default)]
    pub civic_filled_jobs: u16,
    #[serde(default)]
    pub civic_vacant_jobs: u16,
    #[serde(default)]
    pub job_seekers: u16,
    #[serde(default)]
    pub best_open_private_wage: u64,
    /// Completed beds currently available across the settlement.
    #[serde(default)]
    pub housing_capacity: u32,
    /// Residents without a real [`crate::components::HomeAssignment`].
    #[serde(default)]
    pub homeless_residents: u32,
    /// Current public or private workers attached to a workplace which owes
    /// wage arrears. Historical claimants are not counted here.
    #[serde(default)]
    pub unpaid_workers: u16,
    /// Slow-moving public instability. This is a settlement reading, not a
    /// new need or continuously ticking state on every character.
    #[serde(default)]
    pub unrest: f32,
    /// Today's pressure before persistence is applied.
    #[serde(default)]
    pub unrest_target: f32,
    /// Signed change made on the most recently completed world day.
    #[serde(default)]
    pub unrest_change: f32,
    #[serde(default)]
    pub unrest_hunger_pressure: f32,
    #[serde(default)]
    pub unrest_housing_pressure: f32,
    #[serde(default)]
    pub unrest_wage_pressure: f32,
}

impl Default for SettlementEconomy {
    fn default() -> Self {
        Self {
            edible_stock: 0,
            reserve_days: 0.0,
            recent_food_production: 0.0,
            recent_food_consumption: 0.0,
            unmet_food: 0,
            observed_days: 0,
            food_secure_days: 0,
            reserve_prosperity: 0.0,
            production_prosperity: 0.0,
            housing_prosperity: 0.0,
            employment_prosperity: 0.0,
            hunger_penalty: 0.0,
            prosperity: 0.0,
            private_job_positions: 0,
            private_filled_jobs: 0,
            private_vacant_jobs: 0,
            civic_job_positions: 0,
            civic_filled_jobs: 0,
            civic_vacant_jobs: 0,
            job_seekers: 0,
            best_open_private_wage: 0,
            housing_capacity: 0,
            homeless_residents: 0,
            unpaid_workers: 0,
            unrest: 0.0,
            unrest_target: 0.0,
            unrest_change: 0.0,
            unrest_hunger_pressure: 0.0,
            unrest_housing_pressure: 0.0,
            unrest_wage_pressure: 0.0,
        }
    }
}

impl SettlementEconomy {
    pub const fn unrest_label(&self) -> &'static str {
        if self.unrest < 20.0 {
            "Calm"
        } else if self.unrest < 40.0 {
            "Uneasy"
        } else if self.unrest < 60.0 {
            "Tense"
        } else if self.unrest < 80.0 {
            "Volatile"
        } else {
            "Rebellious"
        }
    }

    pub const fn unrest_trend_label(&self) -> &'static str {
        if self.unrest_change > 0.05 {
            "rising"
        } else if self.unrest_change < -0.05 {
            "falling"
        } else {
            "steady"
        }
    }
}

/// A settlement is secure once it can survive this many days without another
/// harvest. Production reliability remains a separate promotion requirement.
pub const FOOD_SECURITY_TARGET_DAYS: f32 = 3.0;

pub const VILLAGE_MIN_RESIDENTS: u32 = 12;

pub const VILLAGE_REQUIRED_SECURE_DAYS: u16 = 3;

pub const VILLAGE_MIN_PROSPERITY: f32 = 65.0;

/// Wood staged beside the founding Moot before its permanent Village Hall is
/// raised. This uses the same paid civic-worksite pipeline as later upgrades.
pub const VILLAGE_HALL_WOOD_REQUIRED: u32 = 12;

pub const TOWN_MIN_RESIDENTS: u32 = 30;

pub const TOWN_MIN_PROSPERITY: f32 = 70.0;

pub const TOWN_MIN_MARKET_VOLUME: u64 = 5_000;

pub const TOWN_REQUIRED_DAYS: u16 = 3;

/// Dressed Stone staged beside the civic centre before a Village can raise
/// its permanent Town Hall. The exchange buys this from real private offers;
/// promotion never withdraws seller-owned stock for free.
pub const TOWN_HALL_STONE_REQUIRED: u32 = 8;

/// Provisional monotonic gate while City population balance remains open.
/// Keeping it above the agreed 30-person Town gate prevents a prosperous
/// small Village from skipping through two civic identities.
pub const CITY_MIN_RESIDENTS: u32 = 75;

pub const CITY_MIN_PROSPERITY: f32 = 75.0;

pub const CITY_REQUIRED_DAYS: u16 = 5;
