//! Connection systems
//!
//! Networking, connection handling, cursor management, and menu transitions.

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use lightyear::prelude::client::*;
use lightyear::prelude::*;
use shared::components::Player;
use shared::protocol::{
    NETCODE_CLIENT_TIMEOUT_SECS, NETCODE_TOKEN_EXPIRE_SECS, PRIVATE_KEY, PROTOCOL_ID,
};
use shared::vehicle::Vehicle;
use std::net::{SocketAddr, ToSocketAddrs};

use super::particles::SandParticle;
use super::world::ClientWorldRoot;
use crate::states::GameState;
use crate::terrain::LoadedChunks;
use crate::ui::ServerAddress;
use shared::components::Npc;

// =============================================================================
// CONNECTION
// =============================================================================

/// Start connection to server
/// In Lightyear 0.25, we spawn a Client entity with the appropriate networking components
/// and then trigger the Connect event to initiate the connection
pub fn handle_start_connection(
    mut commands: Commands,
    existing_clients: Query<Entity, With<crate::GameClient>>,
    server_address: Res<ServerAddress>,
) {
    info!(
        "Initiating connection to server at {}:{}...",
        server_address.ip, server_address.port
    );

    // Ensure we only ever have ONE GameClient entity.
    // If we keep spawning new ones on each connect attempt, `Query::single()` calls
    // will start failing and gameplay (inputs/weapons/etc) silently stops working.
    for e in existing_clients.iter() {
        commands.entity(e).despawn();
    }

    let server_target = format!("{}:{}", server_address.ip.trim(), server_address.port);
    let server_addr: SocketAddr = match server_target
        .to_socket_addrs()
        .ok()
        .and_then(|addrs| addrs.into_iter().next())
    {
        Some(addr) => addr,
        None => {
            error!("Invalid/unresolvable server address: {}", server_target);
            return;
        }
    };
    let local_addr: SocketAddr = "0.0.0.0:0".parse().unwrap();

    // Generate a unique client ID
    let client_id = rand::random::<u64>();

    // Build authentication (netcode connect token)
    let auth = Authentication::Manual {
        server_addr,
        protocol_id: PROTOCOL_ID,
        private_key: PRIVATE_KEY,
        client_id,
    };

    // Spawn client entity with UDP + Netcode
    let client_entity = commands
        .spawn((
            crate::GameClient,
            Client::default(),
            UdpIo::default(),
            LocalAddr(local_addr),
            PeerAddr(server_addr),
            NetcodeClient::new(
                auth,
                NetcodeConfig {
                    client_timeout_secs: NETCODE_CLIENT_TIMEOUT_SECS,
                    token_expire_secs: NETCODE_TOKEN_EXPIRE_SECS,
                    ..NetcodeConfig::default()
                },
            )
            .expect("Failed to create netcode client"),
            // IMPORTANT: enable replication receive on this client.
            // Without this, the client will never receive `WorldTime` / `Player` / `Vehicle` / etc.
            ReplicationReceiver::default(),
        ))
        .id();

    // Add Client -> Server message senders (split to avoid tuple size limit)
    commands.entity(client_entity).insert((
        MessageSender::<shared::protocol::PlayerInput>::default(),
        MessageSender::<shared::protocol::ShootRequest>::default(),
        MessageSender::<shared::protocol::SwitchWeapon>::default(),
        MessageSender::<shared::protocol::ReloadRequest>::default(),
        MessageSender::<shared::protocol::SetTimeOfDay>::default(),
        MessageSender::<shared::protocol::SpawnOilmanDebug>::default(),
        MessageSender::<shared::items::PickupRequest>::default(),
        MessageSender::<shared::items::DropRequest>::default(),
        MessageSender::<shared::items::SelectHotbarSlot>::default(),
        MessageSender::<shared::items::InventoryMoveRequest>::default(),
        // Player name submission
        MessageSender::<shared::protocol::SubmitPlayerName>::default(),
        MessageSender::<shared::protocol::RequestPlayerRoster>::default(),
    ));

    // Chest messages (split to avoid tuple size limit)
    commands.entity(client_entity).insert((
        MessageSender::<shared::items::OpenChestRequest>::default(),
        MessageSender::<shared::items::CloseChestRequest>::default(),
        MessageSender::<shared::items::ChestTransferRequest>::default(),
    ));

    // Add server -> client message receivers (split to avoid tuple size limit)
    commands.entity(client_entity).insert((
        MessageReceiver::<shared::protocol::HitConfirm>::default(),
        MessageReceiver::<shared::protocol::BulletImpact>::default(),
        MessageReceiver::<shared::protocol::DamageReceived>::default(),
        MessageReceiver::<shared::protocol::PlayerKilled>::default(),
        // Name submission response
        MessageReceiver::<shared::protocol::NameSubmissionResult>::default(),
        MessageReceiver::<shared::protocol::PlayerRoster>::default(),
    ));

    // Trigger the Connect event to actually initiate the connection
    commands.trigger(Connect {
        entity: client_entity,
    });

    info!("Client entity spawned, client_id: {}", client_id);
}

/// Check connection status
/// In Lightyear 0.25, we query for Connected/Disconnected components on the client entity
pub fn update_connection_status(
    mut next_state: ResMut<NextState<GameState>>,
    new_connections: Query<Entity, (With<crate::GameClient>, Added<Connected>)>,
    new_disconnections: Query<
        (Entity, &Disconnected),
        (With<crate::GameClient>, Added<Disconnected>),
    >,
    server_address: Res<ServerAddress>,
) {
    for _entity in new_connections.iter() {
        info!("Connected to server! Awaiting name submission...");
        next_state.set(GameState::Connected);
    }

    for (_entity, disconnected) in new_disconnections.iter() {
        let reason = disconnected.reason.as_deref().unwrap_or("unknown");
        warn!(
            "Connection failed or disconnected from {}:{} ({})",
            server_address.ip, server_address.port, reason
        );
        if (server_address.ip == "127.0.0.1" || server_address.ip.eq_ignore_ascii_case("localhost"))
            && reason.contains("ConnectionRequestTimedOut")
        {
            warn!(
                "Timed out on loopback address. If the server runs on another machine, use that machine's LAN/WAN IP instead of 127.0.0.1."
            );
        }
        next_state.set(GameState::MainMenu);
    }
}

// =============================================================================
// CURSOR
// =============================================================================

/// Grab cursor for FPS controls
pub fn apply_cursor_grab(
    windows: Query<Entity, With<PrimaryWindow>>,
    mut cursor_opts: Query<&mut CursorOptions>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    input_state: Res<crate::input::InputState>,
) {
    // Don't grab cursor when any UI is open
    if input_state.ui_blocking() {
        return;
    }

    let Ok(window_entity) = windows.single() else {
        return;
    };

    if mouse_button.just_pressed(MouseButton::Left) {
        if let Ok(mut cursor) = cursor_opts.get_mut(window_entity) {
            cursor.grab_mode = CursorGrabMode::Locked;
            cursor.visible = false;
        }
    }
}

// =============================================================================
// MENU TRANSITIONS
// =============================================================================

/// Entering the main menu: cleanup
pub fn cleanup_enter_main_menu(
    mut commands: Commands,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut cursor_opts: Query<&mut CursorOptions>,
    world_roots: Query<Entity, With<ClientWorldRoot>>,
    players: Query<Entity, With<Player>>,
    npcs: Query<Entity, With<Npc>>,
    vehicles: Query<Entity, With<Vehicle>>,
    particles: Query<Entity, With<SandParticle>>,
    mut loaded_chunks: ResMut<LoadedChunks>,
) {
    // Release cursor when entering main menu
    if let Ok(window_entity) = windows.single() {
        if let Ok(mut cursor) = cursor_opts.get_mut(window_entity) {
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
        }
    }

    for root in world_roots.iter() {
        commands.entity(root).despawn();
    }

    for entity in players.iter() {
        commands.entity(entity).despawn();
    }

    for entity in npcs.iter() {
        commands.entity(entity).despawn();
    }

    for entity in vehicles.iter() {
        commands.entity(entity).despawn();
    }

    // Clean up particles
    for entity in particles.iter() {
        commands.entity(entity).despawn();
    }

    loaded_chunks.chunks.clear();
    commands.insert_resource(ClearColor(Color::BLACK));
}
