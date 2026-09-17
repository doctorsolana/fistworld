//! Shared ECS components used by both server and client.

mod actors;
mod archery;
mod army;
pub use archery::*;
mod building_kinds;
mod buildings;
mod farm_fields;
pub use farm_fields::{
    farm_field_permanent_obstacles, fit_farm_field_shapes, fit_farm_field_shapes_on_terrain,
    FarmFieldSection, FarmFieldShape, FARM_FENCE_HEIGHT, FARM_FENCE_OBSTACLE_TYPE,
    FARM_FENCE_THICKNESS,
};
mod civic;
mod fortifications;
pub use fortifications::*;
mod civic_square;
pub use civic_square::*;
mod health;
mod horse;
pub use horse::*;
mod house_upgrades;
pub use house_upgrades::*;
mod household_yards;
pub use household_yards::*;
mod identity;
mod permits;
mod placement;
mod settlements;
mod trade;
mod shipping;
pub use shipping::*;
mod village_life;
mod village_roads;
mod road_bridges;
pub use road_bridges::*;
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
    building_freeboard, founding_refusal, minimum_building_water_clearance,
    minimum_civic_hall_reservation_water_clearance, minimum_rotated_rect_water_clearance,
    settlement_founding_refusal, BUILDING_FREEBOARD, MIN_SETTLEMENT_SPACING,
    SETTLEMENT_FREEBOARD,
};
pub use placement::{
    accepted_field_claims, building_claims, corridor_blocks_claim, describe_claim,
    doorway_approach, doorway_claim, footprint_claim, hall_claims, intended_field_claims,
    land_conflict_sentence, land_owner_label, lane_conflict_sentence, legacy_field_claims,
    pasture_claim, polyline_intersects_rotated_rect, polyline_within_radius,
    proposed_plot_claims, road_conflict_sentence, worst_conflict, yard_margin, LandClaim,
    LandUse, PlotClaims, CIVIC_YARD_MARGIN, DOOR_APRON_HALF_WIDTH, DOOR_APRON_LENGTH,
    FIELD_VERGE_MARGIN, HALL_FORECOURT_HALF_EXTENTS, HOUSE_YARD_MARGIN, ROAD_VERGE,
    WORKPLACE_YARD_MARGIN,
};
pub use settlements::{
    Settlement, SettlementCenterStyle, SettlementDevelopment, SettlementDevelopmentEvidence,
    SettlementLayoutStyle, SettlementProgressGate, SettlementTier, SettlementWallStyle,
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
