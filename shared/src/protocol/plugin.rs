use bevy::prelude::*;
use lightyear::prelude::*;

use crate::components::{
    ActiveMapState, CharacterAffiliation, CharacterKind, CharacterName, CommandedBy,
    ConstructionSite, Residence, Settlement, SettlementBuilding, CloudSeed, Health, Hero, HeroOutfit, Player,
    PlayerPosition, PlayerProgression, PlayerRotation, TimeWarp, WorldTime,
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

        // === CHARACTERS (heroes and villagers alike) ===
        app.component::<CharacterName>().replicate();
        app.component::<CharacterKind>().replicate();
        app.component::<CharacterAffiliation>().replicate();
        app.component::<CommandedBy>().replicate();
        app.component::<Settlement>().replicate();
        app.component::<SettlementBuilding>().replicate();
        app.component::<ConstructionSite>().replicate();
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
        // `.add_map_entities()` must live HERE, in the shared plugin: it swaps
        // both the serialize and deserialize functions for the type, so if only
        // one peer registered it the two would disagree on the wire format.
        app.register_message::<DevCommand>()
            .add_map_entities()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<UnitMoveOrder>()
            .add_map_entities()
            .add_direction(NetworkDirection::ClientToServer);

        // Server -> Client
        app.register_message::<NameSubmissionResult>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<CharacterRoster>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<DevStatus>()
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
