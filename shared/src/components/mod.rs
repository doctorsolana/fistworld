//! Shared ECS components used by both server and client.

mod actors;
mod archery;
mod army;
pub use archery::*;
mod building_kinds;
mod buildings;
mod civic;
mod fortifications;
pub use fortifications::*;
mod civic_square;
pub use civic_square::*;
mod health;
mod horse;
pub use horse::*;
mod identity;
mod permits;
mod placement;
mod settlements;
mod trade;
mod village_life;
mod village_roads;
mod world;

pub use actors::{
    AboardBoat, CharacterActivity, CharacterAffiliation, CharacterAttributes, CharacterKind,
    CharacterMotion, CharacterName, CharacterNavigationStatus, CharacterObjective, CommandedBy,
    Ground, Hero, HeroOutfit, ImmigrantArrivalBoat, LocalPlayer, Player, PlayerBoat,
    PlayerPosition, PlayerProgression, PlayerRotation, Vessel, WreckedVessel, HERO_SLOT_MAX,
};
pub use army::*;
pub use building_kinds::SettlementBuildingKind;
pub use buildings::{
    builder_stand_position, BuildingDoorDemand, BuildingDoorUse, CivicHallLevel,
    CivicHallUpgradeWorksite, ConstructionSite, FarmField, FishingPier, HouseAppearance,
    HouseLevel, HouseLine, LivestockPasture, MarketLevel, SettlementBuilding, WorkplaceOperation,
    FARM_FIELDS_PER_FARMSTEAD, FARM_FIELD_EDGE_CLEARANCE, FARM_FIELD_LATERAL_OFFSET,
    FARM_FIELD_TERRACE_MARGIN, SETTLEMENT_RAISE_SECONDS,
};
pub use civic::{
    CivicPayrollEntry, CivicPolicyAdjustment, CivicPolicyReason, CivicStaffingPosture,
    CivicStrategy, MootAdministration, PoorReliefMode, SettlementPolicies,
};
pub use health::*;
pub use identity::*;
pub use permits::{
    PermitMarketOpportunity, PlayerPermit, PlayerPermitLedger, PropertyListingStage,
    PropertyMarketListing, SettlementOpportunityBoard, SettlementPropertyBoard,
};
pub use placement::{
    founding_refusal, minimum_building_water_clearance,
    minimum_civic_hall_reservation_water_clearance, minimum_rotated_rect_water_clearance,
    settlement_founding_refusal, MIN_SETTLEMENT_SPACING, SETTLEMENT_FREEBOARD,
};
pub use settlements::{
    Settlement, SettlementCenterStyle, SettlementDevelopment, SettlementLayoutStyle,
    SettlementProgressGate, SettlementTier, SettlementWallStyle,
};
pub use trade::*;
pub use village_life::{
    CharacterDayPlan, Household, Nutrition, NutritionCondition, Occupation, PlannedLeisure,
    PlannedLeisureStatus, Residence, WorkStatus,
};
pub use village_roads::*;
pub use world::*;

mod siege;
pub use siege::*;
