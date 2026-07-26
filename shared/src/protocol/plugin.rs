use bevy::prelude::*;
use lightyear::prelude::*;

use crate::components::{
    ActiveMapState, CloudSeed, DebugPhysicsBox, DebugPhysicsBoxPosition, DebugPhysicsBoxRotation,
    Health, Npc, NpcActivity, NpcFleeing, NpcIdentity, NpcPosition, NpcRotation, NpcVelocity,
    Player, PlayerCharacter, PlayerJumpState, PlayerPosition, PlayerProgression, PlayerRotation,
    PlayerVelocity, PlayerWaterState, WorldTime,
};
use crate::terrain::TerrainDeltaChunk;

use super::messages::*;

pub struct ProtocolPlugin;

impl Plugin for ProtocolPlugin {
    fn build(&self, app: &mut App) {
        // === PLAYER COMPONENTS ===
        app.register_component::<Player>().add_prediction();
        app.register_component::<PlayerPosition>().add_prediction();
        app.register_component::<PlayerRotation>().add_prediction();
        app.register_component::<PlayerVelocity>().add_prediction();
        app.register_component::<PlayerJumpState>().add_prediction();
        app.register_component::<PlayerWaterState>()
            .add_prediction();
        app.register_component::<PlayerProgression>()
            .add_prediction();
        app.register_component::<PlayerCharacter>().add_prediction();

        // === NPC COMPONENTS ===
        app.register_component::<Npc>().add_prediction();
        app.register_component::<NpcPosition>().add_prediction();
        app.register_component::<NpcRotation>().add_prediction();
        app.register_component::<NpcVelocity>().add_prediction();
        app.register_component::<NpcActivity>().add_prediction();
        app.register_component::<NpcFleeing>().add_prediction();
        app.register_component::<DebugPhysicsBox>().add_prediction();
        app.register_component::<DebugPhysicsBoxPosition>()
            .add_prediction();
        app.register_component::<DebugPhysicsBoxRotation>()
            .add_prediction();


        // === HEALTH ===
        app.register_component::<Health>().add_prediction();

        // === WORLD COMPONENTS ===
        app.register_component::<WorldTime>().add_prediction();
        app.register_component::<CloudSeed>().add_prediction();
        app.register_component::<ActiveMapState>().add_prediction();

        // === NPC IDENTITY ===
        app.register_component::<NpcIdentity>().add_prediction();

        // === TERRAIN DELTA CHUNKS ===
        app.register_component::<TerrainDeltaChunk>()
            .add_prediction();


        // === MESSAGES ===
        // Client -> Server
        app.register_message::<SpawnPlayer>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<PlayerInput>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<SetTimeOfDay>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<SetPlayerCharacter>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<SpawnOilmanDebug>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<SpawnPhysicsBoxDebug>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<SubmitPlayerName>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<RequestPlayerRoster>()
            .add_direction(NetworkDirection::ClientToServer);

        // Server -> Client
        app.register_message::<NameSubmissionResult>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<PlayerRoster>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<NpcRagdollStarted>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<NpcRagdollPoseBatch>()
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

        app.add_channel::<RagdollPoseChannel>(ChannelSettings {
            mode: ChannelMode::UnorderedUnreliable,
            ..default()
        })
        .add_direction(NetworkDirection::ServerToClient);
    }
}
