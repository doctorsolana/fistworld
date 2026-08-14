use bevy::prelude::*;
use lightyear::prelude::*;

use crate::components::{
    ActiveMapState, AttachedTo, BuildingDoorDemand, BuildingId, BuildingOf, CharacterActivity,
    CharacterAffiliation, CharacterAttributes, CharacterKind, CharacterMotion, CharacterName,
    CharacterNavigationStatus, CharacterObjective, CivicEmployment, CivicHallLevel, CloudSeed,
    CommandedBy, ConstructionSite, EmployedAt, FarmField, FishingPier, Health, Hero, HeroOutfit,
    Household, LivesAt, MootAdministration, Nutrition, Occupation, OwnedBy, PersonId, Player,
    PlayerPermitLedger, PlayerPosition, PlayerProgression, PlayerRotation, Residence, ResidentOf,
    Settlement, SettlementBuilding, SettlementDevelopment, SettlementId,
    SettlementOpportunityBoard, SettlementPolicies, SettlementPropertyBoard, SettlementSummary,
    TimeWarp, VillageRoad, WorkStatus, WorkplaceOperation, WorldTime,
};
use crate::economy::{
    BusinessAccount, BusinessCondition, BusinessForSale, BusinessLiquidation,
    BusinessManagementPolicy, BusinessProcurementPolicy, BusinessSalePolicy, BusinessWagePolicy,
    CarriedLoad, CivicAccount, GoodsInventory, HouseholdEconomy, MootMarket, SettlementEconomy,
    Wallet, WorkforceRequirements,
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
        app.component::<HeroOutfit>().replicate();
        app.component::<PlayerPermitLedger>().replicate();

        // === CHARACTERS (heroes and villagers alike) ===
        app.component::<CharacterName>().replicate();
        app.component::<PersonId>().replicate();
        app.component::<SettlementId>().replicate();
        app.component::<SettlementSummary>().replicate();
        app.component::<BuildingId>().replicate();
        app.component::<ResidentOf>().replicate();
        app.component::<BuildingOf>().replicate();
        app.component::<AttachedTo>().replicate();
        app.component::<OwnedBy>().replicate();
        app.component::<EmployedAt>().replicate();
        app.component::<CivicEmployment>().replicate();
        app.component::<LivesAt>().replicate();
        app.component::<CharacterKind>().replicate();
        app.component::<CharacterAttributes>().replicate();
        app.component::<CharacterMotion>().replicate();
        app.component::<CharacterActivity>().replicate();
        app.component::<CharacterObjective>().replicate();
        app.component::<CharacterNavigationStatus>().replicate();
        app.component::<Occupation>().replicate();
        app.component::<WorkStatus>().replicate();
        app.component::<Nutrition>().replicate();
        app.component::<CarriedLoad>().replicate();
        app.component::<GoodsInventory>().replicate();
        app.component::<Wallet>().replicate();
        app.component::<MootMarket>().replicate();
        app.component::<SettlementEconomy>().replicate();
        app.component::<CharacterAffiliation>().replicate();
        app.component::<CommandedBy>().replicate();
        app.component::<Settlement>().replicate();
        app.component::<CivicHallLevel>().replicate();
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
        app.component::<Household>().replicate();
        app.component::<HouseholdEconomy>().replicate();
        app.component::<BusinessAccount>().replicate();
        app.component::<BusinessCondition>().replicate();
        app.component::<BusinessForSale>().replicate();
        app.component::<BusinessLiquidation>().replicate();
        app.component::<BusinessManagementPolicy>().replicate();
        app.component::<BusinessProcurementPolicy>().replicate();
        app.component::<BusinessSalePolicy>().replicate();
        app.component::<BusinessWagePolicy>().replicate();
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
        app.register_message::<RequestCharacterRoster>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<RequestSettlementHistory>()
            .add_map_entities()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<RequestWorldHistory>()
            .add_direction(NetworkDirection::ClientToServer);
        // `.add_map_entities()` must live HERE, in the shared plugin: it swaps
        // both the serialize and deserialize functions for the type, so if only
        // one peer registered it the two would disagree on the wire format.
        app.register_message::<DevCommand>()
            .add_map_entities()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<UnitMoveOrder>()
            .add_map_entities()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<HeroConstructionOrder>()
            .add_map_entities()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<HeroBusinessOrder>()
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
        app.register_message::<DevStatus>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<HeroMarketResult>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<HeroPermitResult>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<HeroConstructionResult>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<HeroBusinessResult>()
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
