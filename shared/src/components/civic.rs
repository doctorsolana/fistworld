//! Civic administration, payroll rosters and bounded policy contracts.

use super::SettlementTier;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// The small public office operated from a settlement's Moot Hall.
///
/// This is separate from [`Settlement`] because administration is optional
/// state that can grow without making every old settlement constructor and
/// save record know about future civic jobs. The founding roster has a Reeve
/// and up to two combined Moot Stewards; later tier and policy targets can
/// advertise Guards. It is replicated so clicking the hall exposes who
/// holds each job and what the public purse owes them.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct MootAdministration {
    /// Senior civic clerk. The Reeve remains accountable for permits and the
    /// public purse while the other two founding positions have physical jobs.
    #[serde(default)]
    pub reeve: Option<String>,
    /// Readable alias for the first combined Moot Steward. Durable employment
    /// remains authoritative and supports more than one steward.
    pub lead_steward: Option<String>,
    /// Public safety positions. Guards are real named jobs even before guard
    /// patrol/combat behaviour is implemented.
    #[serde(default)]
    pub guards: Vec<String>,
    /// Public works positions. Founding worker slots are combined Moot
    /// Stewards: both haul goods and either can accept road repairs. The
    /// lead alias above remains the readable primary steward.
    #[serde(default)]
    pub city_workers: Vec<String>,
    /// One payable per present or former public employee. Inactive entries
    /// remain until their arrears are actually paid, so changing jobs cannot
    /// erase a municipal debt.
    #[serde(default)]
    pub payroll: Vec<CivicPayrollEntry>,
    pub steward_daily_salary: u64,
    pub wage_arrears: u64,
    pub roadless_buildings: u16,
    pub disconnected_buildings: u16,
    /// Completed buildings whose live builder still owns an unfinished
    /// connector. Kept separate so the UI never calls pending work healthy.
    pub pending_road_buildings: u16,
    pub last_road_audit_day: u32,
}

impl Default for MootAdministration {
    fn default() -> Self {
        Self {
            reeve: None,
            lead_steward: None,
            guards: Vec::new(),
            city_workers: Vec::new(),
            payroll: Vec::new(),
            steward_daily_salary: crate::economy::MOOT_STEWARD_DAILY_SALARY,
            wage_arrears: 0,
            roadless_buildings: 0,
            disconnected_buildings: 0,
            pending_road_buildings: 0,
            last_road_audit_day: 0,
        }
    }
}

/// A durable public wage claim. Civic positions use the same explicit cash
/// and arrears rules as private businesses rather than silently volunteering
/// whenever the treasury is empty.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CivicPayrollEntry {
    pub person_id: super::PersonId,
    pub name: String,
    pub role: super::CivicRole,
    pub daily_wage: u64,
    pub arrears: u64,
    pub last_accrual_day: u32,
    pub active: bool,
}

/// Stable civic temperament used by the automatic Reeve. These are priorities,
/// not separate economies: every current strategy still trades through private
/// seller-owned offers.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CivicStrategy {
    #[default]
    Balanced,
    Frugal,
    Mercantile,
    MutualAid,
    Growth,
}

impl CivicStrategy {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Balanced => "Balanced",
            Self::Frugal => "Frugal",
            Self::Mercantile => "Mercantile",
            Self::MutualAid => "Mutual aid",
            Self::Growth => "Growth",
        }
    }
}

/// Whether the treasury may buy food for residents who cannot afford a meal.
/// `SurplusOnly` never creates stock or ignores scarcity: recent production
/// must cover the population and the enacted reserve floor must remain after
/// the purchase.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PoorReliefMode {
    #[default]
    Off,
    SurplusOnly,
}

impl PoorReliefMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::SurplusOnly => "Surplus only",
        }
    }

    pub const fn allows_purchase(self) -> bool {
        matches!(self, Self::SurplusOnly)
    }
}

/// How many of the tier's available public positions the settlement attempts
/// to fill. Treasury runway remains a hard constraint under every posture.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CivicStaffingPosture {
    Essential,
    #[default]
    Balanced,
    Full,
}

impl CivicStaffingPosture {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Essential => "Essential",
            Self::Balanced => "Balanced",
            Self::Full => "Full",
        }
    }

    pub const fn level(self) -> u8 {
        match self {
            Self::Essential => 0,
            Self::Balanced => 1,
            Self::Full => 2,
        }
    }

    pub fn targets(self, tier: SettlementTier) -> (u8, u8) {
        let workers = tier.public_worker_positions();
        let guards = tier.public_guard_positions();
        match self {
            Self::Essential => (workers.min(1), 0),
            Self::Balanced => (workers, guards.min(1)),
            Self::Full => (workers, guards),
        }
    }

    /// Advertised capacity; actual public hires still require treasury funding
    /// and available workers. Food circulation must scale before tier promotion.
    pub fn targets_for_population(self, tier: SettlementTier, residents: u32) -> (u8, u8) {
        let (workers, guards) = self.targets(tier);
        let workers = if workers == 0 || self == Self::Essential {
            workers
        } else {
            workers.max(residents.div_ceil(24).min(24) as u8)
        };
        (workers, guards)
    }
}

/// Why the automatic Reeve last changed an enacted policy.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CivicPolicyReason {
    #[default]
    None,
    PayrollArrears,
    TreasuryStress,
    SustainableRelief,
    FoodStress,
    HealthySurplus,
}

impl CivicPolicyReason {
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "No adjustment",
            Self::PayrollArrears => "Civic payroll arrears",
            Self::TreasuryStress => "Low treasury runway",
            Self::SustainableRelief => "Sustainable food surplus",
            Self::FoodStress => "Food reserve stress",
            Self::HealthySurplus => "Healthy civic surplus",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CivicPolicyAdjustment {
    #[default]
    None,
    RaisedMarketFee,
    LoweredMarketFee,
    RaisedProfitTax,
    LoweredProfitTax,
    EnabledPoorRelief,
    DisabledPoorRelief,
    RaisedGrowthSubsidy,
    LoweredGrowthSubsidy,
    ExpandedStaffing,
    ReducedStaffing,
}

impl CivicPolicyAdjustment {
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "No policy change",
            Self::RaisedMarketFee => "Raised market fee",
            Self::LoweredMarketFee => "Lowered market fee",
            Self::RaisedProfitTax => "Raised profit levy",
            Self::LoweredProfitTax => "Lowered profit levy",
            Self::EnabledPoorRelief => "Enabled Poor Relief",
            Self::DisabledPoorRelief => "Disabled Poor Relief",
            Self::RaisedGrowthSubsidy => "Raised growth subsidy",
            Self::LoweredGrowthSubsidy => "Lowered growth subsidy",
            Self::ExpandedStaffing => "Expanded civic staffing",
            Self::ReducedStaffing => "Reduced civic staffing",
        }
    }
}

/// Public rules chosen by a settlement rather than hidden simulation switches.
///
/// The first policy is deliberately narrow: it does not make food free. When
/// enabled, the settlement treasury may buy one market ration for a resident
/// whose personal wallet cannot, but only from sustainable surplus above the
/// configured emergency reserve. Public money, production and physical stock
/// can all constrain relief, so it softens unemployment without deleting
/// scarcity.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettlementPolicies {
    #[serde(default = "default_poor_relief")]
    pub poor_relief: PoorReliefMode,
    #[serde(default = "default_food_reserve_target_days")]
    pub food_reserve_target_days: u8,
    #[serde(default)]
    pub strategy: CivicStrategy,
    #[serde(default = "default_true")]
    pub autopilot: bool,
    #[serde(default = "default_market_fee_bps")]
    pub market_fee_bps: u16,
    #[serde(default = "default_business_profit_tax_bps")]
    pub business_profit_tax_bps: u16,
    #[serde(default = "default_civic_payroll_reserve_days")]
    pub civic_payroll_reserve_days: u8,
    #[serde(default)]
    pub staffing_posture: CivicStaffingPosture,
    /// Discount on settlement-requested private business permits. This is
    /// foregone permit revenue, not a treasury payment or newly created coin.
    #[serde(default = "default_business_permit_subsidy_bps")]
    pub business_permit_subsidy_bps: u16,
    #[serde(default = "unreviewed_day")]
    pub last_review_day: u32,
    #[serde(default = "unreviewed_day")]
    pub last_change_day: u32,
    #[serde(default)]
    pub last_adjustment: CivicPolicyAdjustment,
    #[serde(default)]
    pub last_reason: CivicPolicyReason,
}

const fn default_poor_relief() -> PoorReliefMode {
    PoorReliefMode::SurplusOnly
}

const fn default_food_reserve_target_days() -> u8 {
    3
}

const fn default_true() -> bool {
    true
}

const fn default_market_fee_bps() -> u16 {
    crate::economy::DEFAULT_MARKET_FEE_BPS
}

const fn default_business_profit_tax_bps() -> u16 {
    crate::economy::DEFAULT_BUSINESS_PROFIT_TAX_BPS
}

const fn default_civic_payroll_reserve_days() -> u8 {
    crate::economy::DEFAULT_CIVIC_PAYROLL_RESERVE_DAYS
}

const fn default_business_permit_subsidy_bps() -> u16 {
    crate::economy::DEFAULT_BUSINESS_PERMIT_SUBSIDY_BPS
}

const fn unreviewed_day() -> u32 {
    u32::MAX
}

impl Default for SettlementPolicies {
    fn default() -> Self {
        Self {
            poor_relief: default_poor_relief(),
            food_reserve_target_days: default_food_reserve_target_days(),
            strategy: CivicStrategy::Balanced,
            autopilot: true,
            market_fee_bps: default_market_fee_bps(),
            business_profit_tax_bps: default_business_profit_tax_bps(),
            civic_payroll_reserve_days: default_civic_payroll_reserve_days(),
            staffing_posture: CivicStaffingPosture::Balanced,
            business_permit_subsidy_bps: default_business_permit_subsidy_bps(),
            last_review_day: u32::MAX,
            last_change_day: u32::MAX,
            last_adjustment: CivicPolicyAdjustment::None,
            last_reason: CivicPolicyReason::None,
        }
    }
}

impl SettlementPolicies {
    /// Visual settlement seeds choose streets and centre form, not politics.
    /// Every foundation begins from the agreed Balanced charter; later Reeve
    /// reviews or player control may enact different values.
    pub fn from_foundation(_name: &str, _position: Vec3) -> Self {
        Self::default()
    }

    pub const fn poor_relief() -> Self {
        Self {
            poor_relief: PoorReliefMode::SurplusOnly,
            food_reserve_target_days: default_food_reserve_target_days(),
            strategy: CivicStrategy::Balanced,
            autopilot: true,
            market_fee_bps: default_market_fee_bps(),
            business_profit_tax_bps: default_business_profit_tax_bps(),
            civic_payroll_reserve_days: default_civic_payroll_reserve_days(),
            staffing_posture: CivicStaffingPosture::Balanced,
            business_permit_subsidy_bps: default_business_permit_subsidy_bps(),
            last_review_day: u32::MAX,
            last_change_day: u32::MAX,
            last_adjustment: CivicPolicyAdjustment::None,
            last_reason: CivicPolicyReason::None,
        }
    }
}
