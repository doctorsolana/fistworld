use super::unit_orders::{ArmyOrderFeedback, UnitOrder};
use crate::components::EngagedWith;
use bevy::prelude::*;
use lightyear::prelude::*;

use crate::components::{
    AboardBoat, ActiveMapState, AttachedTo, Battalion, BuildingDoorDemand, BuildingId, BuildingOf,
    CharacterActivity, CharacterAffiliation, CharacterAttributes, CharacterDayPlan, CharacterKind,
    CharacterMotion, CharacterName, CharacterNavigationStatus, CharacterObjective, CivicEmployment,
    CivicHallLevel, CivicHallUpgradeWorksite, CivicTradeContract, CloudSeed, CommandedBy, Company,
    CompanyId, CompanyLeadership, CompanyOwnership, CompanyShareMarket, CompanyTradeRoute,
    ConstructionSite, EmployedAt, FarmField, FishingPier, Health, Hero, HeroOutfit,
    HouseAppearance, Household, ImmigrantArrivalBoat, LivesAt, LivestockPasture, MarketLevel,
    MemberOfBattalion, MootAdministration, Nutrition, Occupation, OperatedBy, OwnedBy, PersonId,
    Player, PlayerBoat, PlayerPermitLedger, PlayerPosition, PlayerProgression, PlayerRotation,
    Residence, ResidentOf, Settlement, SettlementBuilding, SettlementDevelopment, SettlementId,
    SettlementOpportunityBoard, SettlementPolicies, SettlementPropertyBoard, SettlementSummary,
    StandardBearer, TimeWarp, TradeContractId, TradeRouteHistory, TradeRouteId, TradeRouteSchedule,
    Vessel, VillageRoad, WorkStatus, WorkplaceOperation, WorldTime, WreckedVessel,
};
use crate::economy::{
    BusinessAccount, BusinessCondition, BusinessForSale, BusinessLiquidation,
    BusinessManagementPolicy, BusinessProcurementPolicy, BusinessSalePolicy,
    BusinessStaffingPolicy, BusinessSupplyPolicy, BusinessWagePolicy, CarriedLoad, CivicAccount,
    CompanyAccount, CompanyBranchPolicies, CompanyDecisionHistory, CompanyManagementPolicy,
    GoodsInventory, HouseholdEconomy, MootMarket, PorterCartState, SettlementEconomy,
    TavernService, Wallet, WorkforceRequirements,
};
use crate::terrain::TerrainDeltaChunk;

use super::messages::*;

pub struct ProtocolPlugin;

impl Plugin for ProtocolPlugin {
    fn build(&self, app: &mut App) {
        // === PLAYER COMPONENTS ===
        // No .predict(): nothing spawns PredictionTarget or queries Predicted, so
        // prediction registration would be dead weight (lightyear 0.28 requires
        // PredictionTarget on entities for it to do anything at all).
        app.component::<Player>().replicate();
        app.component::<PlayerPosition>().replicate();
        app.component::<PlayerRotation>().replicate();
        app.component::<PlayerProgression>().replicate();

        // === HERO (embodied character; server-authoritative position) ===
        app.component::<Hero>().replicate();
        app.component::<PlayerBoat>().replicate();
        app.component::<ImmigrantArrivalBoat>().replicate();
        app.component::<Vessel>().replicate();
        app.component::<WreckedVessel>().replicate();
        app.component::<AboardBoat>().replicate();
        app.component::<HeroOutfit>().replicate();
        app.component::<PlayerPermitLedger>().replicate();

        // === CHARACTERS (heroes and villagers alike) ===
        app.component::<CharacterName>().replicate();
        app.component::<PersonId>().replicate();
        app.component::<SettlementId>().replicate();
        app.component::<SettlementSummary>().replicate();
        app.component::<BuildingId>().replicate();
        app.component::<CompanyId>().replicate();
        app.component::<TradeContractId>().replicate();
        app.component::<TradeRouteId>().replicate();
        app.component::<Company>().replicate();
        app.component::<CompanyLeadership>().replicate();
        app.component::<CompanyOwnership>().replicate();
        app.component::<CompanyShareMarket>().replicate();
        app.component::<CivicTradeContract>().replicate();
        app.component::<CompanyTradeRoute>().replicate();
        app.component::<TradeRouteSchedule>().replicate();
        app.component::<TradeRouteHistory>().replicate();
        app.component::<ResidentOf>().replicate();
        app.component::<BuildingOf>().replicate();
        app.component::<AttachedTo>().replicate();
        app.component::<OwnedBy>().replicate();
        app.component::<OperatedBy>().replicate();
        app.component::<EmployedAt>().replicate();
        app.component::<CivicEmployment>().replicate();
        app.component::<LivesAt>().replicate();
        app.component::<CharacterKind>().replicate();
        app.component::<CharacterAttributes>().replicate();
        app.component::<CharacterMotion>().replicate();
        app.component::<CharacterActivity>().replicate();
        app.component::<Battalion>().replicate();
        app.component::<MemberOfBattalion>().replicate();
        app.component::<StandardBearer>().replicate();
        app.component::<EngagedWith>().replicate();
        app.component::<CharacterObjective>().replicate();
        app.component::<CharacterDayPlan>().replicate();
        app.component::<CharacterNavigationStatus>().replicate();
        app.component::<Occupation>().replicate();
        app.component::<WorkStatus>().replicate();
        app.component::<Nutrition>().replicate();
        app.component::<CarriedLoad>().replicate();
        app.component::<PorterCartState>().replicate();
        app.component::<GoodsInventory>().replicate();
        app.component::<Wallet>().replicate();
        app.component::<MootMarket>().replicate();
        app.component::<SettlementEconomy>().replicate();
        app.component::<CharacterAffiliation>().replicate();
        app.component::<CommandedBy>().replicate();
        app.component::<Settlement>().replicate();
        app.component::<CivicHallLevel>().replicate();
        app.component::<CivicHallUpgradeWorksite>().replicate();
        app.component::<MarketLevel>().replicate();
        app.component::<HouseAppearance>().replicate();
        app.component::<SettlementDevelopment>().replicate();
        app.component::<SettlementOpportunityBoard>().replicate();
        app.component::<SettlementPropertyBoard>().replicate();
        app.component::<MootAdministration>().replicate();
        app.component::<SettlementPolicies>().replicate();
        app.component::<CivicAccount>().replicate();
        app.component::<SettlementBuilding>().replicate();
        app.component::<WorkplaceOperation>().replicate();
        app.component::<ConstructionSite>().replicate();
        app.component::<FarmField>().replicate();
        app.component::<FishingPier>().replicate();
        app.component::<LivestockPasture>().replicate();
        app.component::<Household>().replicate();
        app.component::<HouseholdEconomy>().replicate();
        app.component::<BusinessAccount>().replicate();
        app.component::<BusinessCondition>().replicate();
        app.component::<BusinessForSale>().replicate();
        app.component::<BusinessLiquidation>().replicate();
        app.component::<BusinessManagementPolicy>().replicate();
        app.component::<BusinessProcurementPolicy>().replicate();
        app.component::<BusinessSupplyPolicy>().replicate();
        app.component::<BusinessSalePolicy>().replicate();
        app.component::<BusinessStaffingPolicy>().replicate();
        app.component::<BusinessWagePolicy>().replicate();
        app.component::<TavernService>().replicate();
        app.component::<CompanyAccount>().replicate();
        app.component::<CompanyManagementPolicy>().replicate();
        app.component::<CompanyBranchPolicies>().replicate();
        app.component::<CompanyDecisionHistory>().replicate();
        app.component::<WorkforceRequirements>().replicate();
        app.component::<BuildingDoorDemand>().replicate();
        app.component::<VillageRoad>().replicate();
        app.component::<Residence>().replicate();

        // === HEALTH ===
        app.component::<Health>().replicate();

        // === WORLD COMPONENTS ===
        app.component::<WorldTime>().replicate();
        app.component::<TimeWarp>().replicate();
        app.component::<CloudSeed>().replicate();
        app.component::<ActiveMapState>().replicate();

        // === TERRAIN DELTA CHUNKS ===
        app.component::<TerrainDeltaChunk>().replicate();

        // === MESSAGES ===
        // Client -> Server
        app.register_message::<PlayerInput>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<SetTimeOfDay>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<SubmitPlayerName>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<CreateHero>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<DisembarkBoat>()
            .add_map_entities()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<SailToLanding>()
            .add_map_entities()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<RequestCharacterRoster>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<RequestSettlementHistory>()
            .add_map_entities()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<RequestWorldHistory>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<RequestCompanyHistory>()
            .add_direction(NetworkDirection::ClientToServer);
        // `.add_map_entities()` must live HERE, in the shared plugin: it swaps
        // both the serialize and deserialize functions for the type, so if only
        // one peer registered it the two would disagree on the wire format.
        app.register_message::<DevCommand>()
            .add_map_entities()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<RequestGodAccess>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<UnitOrder>()
            .add_map_entities()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<ArmyOrder>()
            .add_map_entities()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<ArmyOrderFeedback>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<HeroConstructionOrder>()
            .add_map_entities()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<HeroBusinessOrder>()
            .add_map_entities()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<HeroCompanyOrder>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<HeroTradeRouteOrder>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<HeroCompanyFoundingOrder>()
            .add_map_entities()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<HeroMarketOrder>()
            .add_map_entities()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<HeroPermitOrder>()
            .add_map_entities()
            .add_direction(NetworkDirection::ClientToServer);

        // Server -> Client
        app.register_message::<NameSubmissionResult>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<CharacterRoster>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<SettlementHistoryResponse>()
            .add_map_entities()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<WorldHistoryResponse>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<CompanyHistoryResponse>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<DevStatus>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<GodAccessResult>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<HeroMarketResult>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<HeroPermitResult>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<HeroConstructionResult>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<HeroBusinessResult>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<HeroCompanyResult>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<HeroTradeRouteResult>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<HeroCompanyFoundingResult>()
            .add_direction(NetworkDirection::ServerToClient);

        // === CHANNELS ===
        app.add_channel::<ReliableChannel>(ChannelSettings {
            mode: ChannelMode::OrderedReliable(ReliableSettings::default()),
            ..default()
        })
        // Used for most gameplay messages (shooting, hit confirms, etc.)
        .add_direction(NetworkDirection::Bidirectional);

        app.add_channel::<InputChannel>(ChannelSettings {
            mode: ChannelMode::UnorderedUnreliable,
            ..default()
        })
        // High-frequency input: client -> server only
        .add_direction(NetworkDirection::ClientToServer);
    }
}
