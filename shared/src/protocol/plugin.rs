use bevy::prelude::*;
use lightyear::prelude::*;

use crate::components::{
    ActiveMapState, Bullet, BulletVelocity, CloudSeed, EquippedWeapon, Health, Npc, NpcFleeing,
    NpcIdentity, NpcPosition, NpcRotation, Player, PlayerCharacter, PlayerJumpState,
    PlayerPosition, PlayerProgression, PlayerRotation, PlayerWaterState, WorldTime,
};
use crate::items::{
    ChestPosition, ChestStorage, ChestTransferRequest, CloseChestRequest, DropRequest, GroundItem,
    GroundItemPosition, HotbarSelection, Inventory, InventoryMoveRequest, OpenChestRequest,
    PickupRequest, SelectHotbarSlot,
};
use crate::terrain::{TerrainDeltaChunk, TerrainPaintOp};
use crate::vehicle::{Vehicle, VehicleDriver, VehicleState};

use super::messages::*;

pub struct ProtocolPlugin;

impl Plugin for ProtocolPlugin {
    fn build(&self, app: &mut App) {
        // === PLAYER COMPONENTS ===
        app.register_component::<Player>().add_prediction();
        app.register_component::<PlayerPosition>().add_prediction();
        app.register_component::<PlayerRotation>().add_prediction();
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
        app.register_component::<NpcFleeing>().add_prediction();

        // === VEHICLE COMPONENTS ===
        app.register_component::<Vehicle>().add_prediction();
        app.register_component::<VehicleState>().add_prediction();
        app.register_component::<VehicleDriver>().add_prediction();

        // === COMBAT COMPONENTS ===
        app.register_component::<Health>().add_prediction();
        app.register_component::<EquippedWeapon>().add_prediction();

        // === BULLET COMPONENTS ===
        app.register_component::<Bullet>().add_prediction();
        app.register_component::<BulletVelocity>().add_prediction();

        // === WORLD COMPONENTS ===
        app.register_component::<WorldTime>().add_prediction();
        app.register_component::<CloudSeed>().add_prediction();
        app.register_component::<ActiveMapState>().add_prediction();
        app.register_component::<TerrainPaintOp>().add_prediction();

        // === INVENTORY COMPONENTS ===
        app.register_component::<Inventory>().add_prediction();
        app.register_component::<GroundItem>().add_prediction();
        app.register_component::<GroundItemPosition>()
            .add_prediction();

        // === NPC IDENTITY ===
        app.register_component::<NpcIdentity>().add_prediction();

        // === EQUIPMENT / HOTBAR ===
        app.register_component::<HotbarSelection>().add_prediction();

        // === CHEST / STORAGE ===
        app.register_component::<ChestStorage>().add_prediction();
        app.register_component::<ChestPosition>().add_prediction();

        // === TERRAIN DELTA CHUNKS ===
        app.register_component::<TerrainDeltaChunk>()
            .add_prediction();

        // === MESSAGES ===
        // Client -> Server
        app.register_message::<SpawnPlayer>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<PlayerInput>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<ShootRequest>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<SwitchWeapon>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<ReloadRequest>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<SetTimeOfDay>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<SetPlayerCharacter>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<SpawnOilmanDebug>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<PickupRequest>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<DropRequest>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<SelectHotbarSlot>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<InventoryMoveRequest>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<OpenChestRequest>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<CloseChestRequest>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<ChestTransferRequest>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<SubmitPlayerName>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<RequestPlayerRoster>()
            .add_direction(NetworkDirection::ClientToServer);

        // Server -> Client
        app.register_message::<NameSubmissionResult>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<HitConfirm>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<BulletImpact>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<DamageReceived>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<PlayerKilled>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<AudioEvent>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<PlayerRoster>()
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
