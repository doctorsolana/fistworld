//! Physical goods, fixed-point money and the local Moot market.
//!
//! Goods remain physical and capacity-bounded. Coin is deliberately a ledger:
//! it consumes no cargo space and every transfer has two sides. The Moot is a
//! local consignment exchange: owners set offers, buyers pay those owners at
//! the moment of purchase, and the settlement receives only its market fee.
//!
//! The public economy contract is re-exported here. Implementations live in
//! focused modules: goods/inventory, money/accounts, business/company, civic,
//! market/history, settlement, permits and tavern. Keep wire field and enum
//! order unchanged when reorganizing these types.

mod accounts;
mod business;
mod civic;
mod company;
mod goods;
mod history;
mod inventory;
mod market;
mod money;
mod permits;
mod settlement;
mod tavern;

pub use accounts::{BusinessAccount, BusinessDayLedger};
pub use business::{
    business_working_capital, BusinessCondition, BusinessForSale, BusinessInputRule,
    BusinessLiquidation, BusinessManagementPolicy, BusinessPrivateInputRule,
    BusinessProcurementPolicy, BusinessSalePolicy, BusinessSaleReason, BusinessSourcingMode,
    BusinessStaffingPolicy, BusinessState, BusinessStrategy, BusinessSupplyPolicy,
    BusinessWageClaim, BusinessWagePolicy, BusinessWorkingCapital, WorkforceRequirements,
    BUSINESS_WAGE_REVIEW_STEP, DEFAULT_DAILY_PRICE_STEP_BPS, DEFAULT_INPUT_COVERAGE_DAYS,
    DEFAULT_TARGET_MARGIN_BPS, FOUNDING_DAILY_WAGE, FULLY_STAFFED_DAYS_BEFORE_REVIEW,
    MAXIMUM_BUSINESS_DAILY_WAGE, MAXIMUM_STOCK_COVERAGE_DAYS, MINIMUM_BUSINESS_DAILY_WAGE,
    PAYROLL_STRESS_DAYS_BEFORE_CUT, PROPERTY_MARKET_EXPOSURE_DAYS, VACANCY_DAYS_BEFORE_RAISE,
};
pub use civic::{
    CivicAccount, CivicDayLedger, CIVIC_POLICY_REVIEW_DAYS, DEFAULT_BUSINESS_PERMIT_SUBSIDY_BPS,
    DEFAULT_BUSINESS_PROFIT_TAX_BPS, DEFAULT_CIVIC_PAYROLL_RESERVE_DAYS, DEFAULT_MARKET_FEE_BPS,
    MARKET_FEE_REVIEW_STEP_BPS, MAXIMUM_BUSINESS_PERMIT_SUBSIDY_BPS,
    MAXIMUM_BUSINESS_PROFIT_TAX_BPS, MAXIMUM_CIVIC_PAYROLL_RESERVE_DAYS,
    MAXIMUM_FOOD_RESERVE_TARGET_DAYS, MAXIMUM_MARKET_FEE_BPS, MINIMUM_FOOD_RESERVE_TARGET_DAYS,
    MINIMUM_MARKET_FEE_BPS, MOOT_STEWARD_DAILY_SALARY, PERMIT_SUBSIDY_REVIEW_STEP_BPS,
    PROFIT_TAX_REVIEW_STEP_BPS,
};
pub use company::{
    company_expansion_cash, CompanyAccount, CompanyBranchPolicies, CompanyBranchPolicy,
    CompanyDayLedger, CompanyDecisionHistory, CompanyDecisionReason, CompanyDecisionRecord,
    CompanyManagementPolicy, CompanyResourcePolicy,
};
pub use goods::{Good, MarketTradeTier};
pub use history::{
    BusinessHistoryArchive, BusinessHistoryDay, CivicHistoryDay, CompanyHistoryArchive,
    MarketGoodHistoryDay, SettlementHistoryArchive, SettlementHistoryDay, WorldHistoryArchive,
    WorldHistoryDay, SETTLEMENT_HISTORY_DAYS,
};
pub use inventory::{capacity, CarriedAppearance, CarriedLoad, GoodsInventory, PorterCartState};
pub use market::{
    MarketDayFlow, MarketFill, MarketListing, MarketPool, MarketPurchase, MarketSeller,
    MarketTrade, MootMarket,
};
pub use money::{
    format_money, sustainable_unit_price, Wallet, BASIS_POINTS, PENNIES_PER_COIN,
    STARTING_HERO_COINS, STARTING_HERO_MONEY, STARTING_TREASURY_MONEY, STARTING_VILLAGER_COINS,
    STARTING_VILLAGER_MONEY,
};
pub use permits::{permit_price, permit_price_with_subsidy, player_permit_price_with_subsidy};
pub use settlement::{
    HouseholdEconomy, SettlementEconomy, CITY_MIN_PROSPERITY, CITY_MIN_RESIDENTS,
    CITY_REQUIRED_DAYS, FOOD_SECURITY_TARGET_DAYS, TOWN_HALL_STONE_REQUIRED,
    TOWN_MIN_MARKET_VOLUME, TOWN_MIN_PROSPERITY, TOWN_MIN_RESIDENTS, TOWN_REQUIRED_DAYS,
    VILLAGE_HALL_WOOD_REQUIRED, VILLAGE_MIN_PROSPERITY, VILLAGE_MIN_RESIDENTS,
    VILLAGE_REQUIRED_SECURE_DAYS,
};
pub use tavern::{
    TavernService, TavernServiceDay, TAVERN_GUEST_CAPACITY, TAVERN_MEALS_PER_INNKEEPER_DAY,
};

#[cfg(test)]
mod tests;
