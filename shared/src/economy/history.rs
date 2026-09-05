//! Bounded historical data contracts for on-demand economic ledgers.

use super::{BusinessState, BusinessStrategy, Good};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Maximum number of completed in-game days retained in session memory.
///
/// History is deliberately not persistent yet: settlements still use runtime
/// entities as identity and the test workflow regularly starts a fresh world.
pub const SETTLEMENT_HISTORY_DAYS: usize = 365;

/// One completed good-day sent to clients as part of an on-demand archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MarketGoodHistoryDay {
    pub opening_bid: u64,
    pub opening_ask: u64,
    pub closing_bid: u64,
    pub closing_ask: u64,
    pub high_bid: u64,
    pub low_bid: u64,
    pub high_ask: u64,
    pub low_ask: u64,
    pub producer_units: u64,
    pub producer_coin: u64,
    pub consumer_units: u64,
    pub consumer_coin: u64,
    #[serde(default)]
    pub unavailable_units: u64,
    #[serde(default)]
    pub unaffordable_units: u64,
    #[serde(default)]
    pub funded_unmet_units: u64,
    pub closing_stock: u32,
    pub target_stock: u32,
    pub listed_units: u32,
}

impl MarketGoodHistoryDay {
    pub fn average_producer_price(self) -> Option<u64> {
        (self.producer_units > 0).then(|| self.producer_coin / self.producer_units)
    }

    pub fn average_consumer_price(self) -> Option<u64> {
        (self.consumer_units > 0).then(|| self.consumer_coin / self.consumer_units)
    }

    pub const fn coin_volume(self) -> u64 {
        self.consumer_coin
    }

    pub const fn unmet_units(self) -> u64 {
        self.unavailable_units
            .saturating_add(self.unaffordable_units)
    }
}

/// One completed daily reading of a settlement's money, goods and welfare.
///
/// `total_local_coin` is the conservation-oriented measure: civic treasury +
/// personal wallets + household purses + each locally active company's treasury
/// counted once. Produced goods are tracked separately and valued at that day's
/// last completed sale price. `business_cash` retains its historical wire name but
/// contains company treasury cash, never per-building wallets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettlementHistoryDay {
    pub day: u32,
    pub market: [MarketGoodHistoryDay; Good::COUNT],
    pub civic_treasury: u64,
    pub resident_wallet_money: u64,
    pub household_cash: u64,
    pub business_cash: u64,
    #[serde(default)]
    pub business_wage_arrears: u64,
    #[serde(default)]
    pub business_tax_arrears: u64,
    pub civic_wage_arrears: u64,
    #[serde(default)]
    pub civic: CivicHistoryDay,
    pub physical_stock: [u32; Good::COUNT],
    pub stock_liquidation_value: u64,
    pub total_local_coin: u64,
    pub population: u32,
    pub employed: u32,
    pub hungry: u32,
    #[serde(default)]
    pub job_seekers: u32,
    #[serde(default)]
    pub homeless: u32,
    #[serde(default)]
    pub unpaid_workers: u32,
    #[serde(default)]
    pub unrest: f32,
    #[serde(default)]
    pub unrest_target: f32,
    pub food_reserves: u32,
    #[serde(default)]
    pub purchasable_food: u32,
    #[serde(default)]
    pub unlisted_business_food: u32,
    pub food_produced: u32,
    pub food_consumed: u32,
    pub buildings: u16,
    pub productive_buildings: u16,
    pub work_positions: u16,
    pub filled_jobs: u16,
    pub prosperity: f32,
    pub reserve_prosperity: f32,
    pub production_prosperity: f32,
    pub housing_prosperity: f32,
    pub employment_prosperity: f32,
    pub hunger_penalty: f32,
}

/// Daily municipal accounts and the enacted rules which produced them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CivicHistoryDay {
    pub observed: bool,
    pub permit_income: u64,
    pub market_fee_income: u64,
    #[serde(default)]
    pub delivery_fee_income: u64,
    pub profit_tax_income: u64,
    pub public_sale_income: u64,
    pub wage_expense: u64,
    pub poor_relief_expense: u64,
    pub material_expense: u64,
    #[serde(default)]
    pub freight_expense: u64,
    pub filled_positions: u16,
    pub vacant_positions: u16,
    pub market_fee_bps: u16,
    pub business_profit_tax_bps: u16,
    pub poor_relief: crate::components::PoorReliefMode,
    pub food_reserve_target_days: u8,
    pub civic_payroll_reserve_days: u8,
    pub staffing_posture: crate::components::CivicStaffingPosture,
    pub business_permit_subsidy_bps: u16,
    pub strategy: crate::components::CivicStrategy,
    pub autopilot: bool,
    pub adjustment: crate::components::CivicPolicyAdjustment,
    pub reason: crate::components::CivicPolicyReason,
}

/// Up to one in-game year of session history for a single settlement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SettlementHistoryArchive {
    pub settlement: String,
    /// Oldest to newest, never longer than [`SETTLEMENT_HISTORY_DAYS`].
    pub days: Vec<SettlementHistoryDay>,
    /// Individually addressable firm histories in this settlement. These are
    /// pulled with the settlement archive rather than continuously replicated.
    #[serde(default)]
    pub businesses: Vec<BusinessHistoryArchive>,
}

/// One completed daily reading of an individual private business.
///
/// Prices, wages, strategy and solvency are stored beside the P&L so a debug
/// view can explain *why* profit changed rather than showing only the result.
/// `observed` is false only when a developer jumps across multiple calendar
/// days without simulating their intervening boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct BusinessHistoryDay {
    pub day: u32,
    pub observed: bool,
    pub cash: u64,
    #[serde(default)]
    pub protected_working_capital: u64,
    #[serde(default)]
    pub withdrawable_profit: u64,
    pub wage_arrears: u64,
    pub tax_arrears: u64,
    pub gross_revenue: u64,
    #[serde(default)]
    pub internal_revenue: u64,
    pub wage_expense: u64,
    pub input_expense: u64,
    #[serde(default)]
    pub internal_input_expense: u64,
    pub market_fees: u64,
    #[serde(default)]
    pub delivery_fees: u64,
    pub profit_taxes: u64,
    pub owner_withdrawals: u64,
    #[serde(default)]
    pub capital_expenditures: u64,
    #[serde(default)]
    pub book_value: u64,
    pub profit: i64,
    pub produced_units: u32,
    pub sold_units: u32,
    pub purchased_input_units: u32,
    pub workplace_stock: [u32; Good::COUNT],
    pub listed_output_units: u32,
    pub asking_unit_price: u64,
    pub daily_wage: u64,
    pub strategy: BusinessStrategy,
    pub autopilot: bool,
    pub state: BusinessState,
}

/// Up to one in-game year for one stable business identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessHistoryArchive {
    pub id: crate::components::BuildingId,
    pub settlement: crate::components::SettlementId,
    #[serde(default)]
    pub company_id: Option<crate::components::CompanyId>,
    pub kind: crate::components::SettlementBuildingKind,
    pub owner_id: Option<crate::components::PersonId>,
    pub owner_name: Option<String>,
    pub output_good: Option<Good>,
    /// Oldest to newest, never longer than [`SETTLEMENT_HISTORY_DAYS`].
    pub days: Vec<BusinessHistoryDay>,
}

/// Pull-based, cross-settlement history for one legal company.
///
/// The archive intentionally carries the underlying site ledgers instead of a
/// second persisted accounting format. The client can therefore show both a
/// consolidated company result (with internal transfers eliminated) and the
/// exact sites which produced it. It is requested only while a player opens a
/// company ledger; ordinary replication remains current-state only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompanyHistoryArchive {
    pub company: crate::components::CompanyId,
    pub businesses: Vec<BusinessHistoryArchive>,
}

/// One completed world-wide daily rollup across every settlement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldHistoryDay {
    pub day: u32,
    pub settlements: u32,
    pub population: u32,
    pub employed: u32,
    pub hungry: u32,
    pub civic_treasury: u64,
    pub resident_wallet_money: u64,
    pub household_cash: u64,
    /// Sum of authoritative company treasuries. The field keeps its legacy wire
    /// name so existing replicated history remains compatible.
    pub business_cash: u64,
    #[serde(default)]
    pub business_wage_arrears: u64,
    #[serde(default)]
    pub business_tax_arrears: u64,
    pub civic_wage_arrears: u64,
    pub total_local_coin: u64,
    pub stock_liquidation_value: u64,
    pub physical_stock: [u32; Good::COUNT],
    pub food_reserves: u32,
    #[serde(default)]
    pub purchasable_food: u32,
    #[serde(default)]
    pub unlisted_business_food: u32,
    pub food_produced: u32,
    pub food_consumed: u32,
    pub buildings: u32,
    pub productive_buildings: u32,
    /// Population-weighted mean of settlement prosperity. Empty foundations
    /// receive equal weight only when the whole world has zero residents.
    pub prosperity: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct WorldHistoryArchive {
    /// Oldest to newest, never longer than [`SETTLEMENT_HISTORY_DAYS`].
    pub days: Vec<WorldHistoryDay>,
}
