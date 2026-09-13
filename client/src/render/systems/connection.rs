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
use std::net::{SocketAddr, ToSocketAddrs};

use super::world::ClientWorldRoot;
use crate::states::GameState;
use crate::terrain::LoadedChunks;
use crate::ui::ServerAddress;

/// A recoverable launcher error, distinct from the active connection stage.
#[derive(Resource, Default)]
pub struct ConnectionFeedback {
    pub error_message: Option<String>,
}

#[derive(Component)]
pub(crate) struct ConnectionCancel;

#[derive(Resource, Default)]
struct ConnectionAttempt {
    address: Option<bevy::tasks::Task<Result<SocketAddr, String>>>,
    started: Option<std::time::Instant>,
}

pub fn install_connection_flow(app: &mut App) {
    app.init_resource::<ConnectionFeedback>()
        .init_resource::<ConnectionAttempt>()
        .add_systems(
            OnEnter(GameState::Connecting),
            handle_start_connection.run_if(not(resource_exists::<crate::capture::CaptureConfig>)),
        )
        .add_systems(
            Update,
            (
                finish_address_lookup,
                update_connection_status,
                cancel_connection_attempt,
            )
                .chain(),
        )
        .add_systems(OnEnter(GameState::MainMenu), release_connection);
}

/// DNS can block for seconds. Keep it off the frame thread so the loading
/// animation and Cancel control remain responsive even for an invalid host.
fn handle_start_connection(
    mut commands: Commands,
    existing_clients: Query<Entity, With<crate::GameClient>>,
    server_address: Res<ServerAddress>,
    mut attempt: ResMut<ConnectionAttempt>,
    mut feedback: ResMut<ConnectionFeedback>,
) {
    for entity in &existing_clients {
        commands.entity(entity).despawn();
    }
    feedback.error_message = None;
    let host = server_address.ip.trim().to_string();
    let port = server_address.port;
    info!("Connecting to {host}:{port}");
    attempt.started = Some(std::time::Instant::now());
    attempt.address = Some(
        bevy::tasks::IoTaskPool::get().spawn(async move { resolve_server_address(&host, port) }),
    );
}

fn resolve_server_address(host: &str, port: u16) -> Result<SocketAddr, String> {
    let addresses: Vec<_> = (host, port)
        .to_socket_addrs()
        .map_err(|_| "Could not find that server. Check the address and try again.".to_string())?
        .collect();
    // Prefer IPv4 when both exist (notably localhost), matching the server's
    // default listener; an explicitly IPv6-only server gets a matching socket.
    addresses
        .iter()
        .find(|address| address.is_ipv4())
        .or(addresses.first())
        .copied()
        .ok_or_else(|| "Could not find that server. Check the address and try again.".into())
}

fn finish_address_lookup(
    mut commands: Commands,
    state: Res<State<GameState>>,
    mut next: ResMut<NextState<GameState>>,
    mut attempt: ResMut<ConnectionAttempt>,
    mut feedback: ResMut<ConnectionFeedback>,
) {
    if *state.get() != GameState::Connecting {
        return;
    }
    if attempt
        .started
        .is_some_and(|started| started.elapsed().as_secs() > 20)
    {
        attempt.address = None;
        attempt.started = None;
        feedback.error_message =
            Some("Connection timed out. Check the server and try again.".into());
        next.set(GameState::MainMenu);
        return;
    }
    let Some(task) = attempt.address.as_mut() else {
        return;
    };
    let Some(result) = bevy::tasks::block_on(bevy::tasks::poll_once(task)) else {
        return;
    };
    attempt.address = None;
    let server_addr = match result {
        Ok(address) => address,
        Err(message) => {
            feedback.error_message = Some(message);
            next.set(GameState::MainMenu);
            return;
        }
    };
    let client_id = rand::random::<u64>();
    let auth = Authentication::Manual {
        server_addr,
        protocol_id: PROTOCOL_ID,
        private_key: PRIVATE_KEY,
        client_id,
    };
    let netcode = match NetcodeClient::new(
        auth,
        NetcodeConfig {
            client_timeout_secs: NETCODE_CLIENT_TIMEOUT_SECS,
            token_expire_secs: NETCODE_TOKEN_EXPIRE_SECS,
            ..default()
        },
    ) {
        Ok(netcode) => netcode,
        Err(error) => {
            error!("Cannot initialize connection: {error:?}");
            feedback.error_message =
                Some("Could not start the connection. Please try again.".into());
            next.set(GameState::MainMenu);
            return;
        }
    };
    let local_addr = if server_addr.is_ipv4() {
        SocketAddr::from(([0, 0, 0, 0], 0))
    } else {
        SocketAddr::from(([0u16; 8], 0))
    };
    commands.insert_resource(crate::camera_rts::LocalPeerId(client_id));
    let entity = commands
        .spawn((
            crate::GameClient,
            Client::default(),
            UdpIo::default(),
            LocalAddr(local_addr),
            PeerAddr(server_addr),
            netcode,
            ReplicationReceiver,
        ))
        .id();
    // Registered message senders/receivers are Client required components.
    commands.trigger(Connect { entity });
}

/// Observe disconnects throughout name entry, map preparation and gameplay.
/// Restricting this to Connecting stranded the old modal after a lost server.
fn update_connection_status(
    state: Res<State<GameState>>,
    mut next: ResMut<NextState<GameState>>,
    new_connections: Query<Entity, (With<crate::GameClient>, Added<Connected>)>,
    new_disconnections: Query<&Disconnected, (With<crate::GameClient>, Added<Disconnected>)>,
    mut attempt: ResMut<ConnectionAttempt>,
    mut feedback: ResMut<ConnectionFeedback>,
) {
    if *state.get() == GameState::MainMenu {
        return;
    }
    if *state.get() == GameState::Connecting && !new_connections.is_empty() {
        attempt.started = None;
        next.set(GameState::Connected);
    }
    if let Some(disconnected) = new_disconnections.iter().next() {
        warn!(
            "Disconnected: {}",
            disconnected.reason.as_deref().unwrap_or("unknown")
        );
        if feedback.error_message.is_none() {
            feedback.error_message = Some(
                if *state.get() == GameState::Connecting {
                    "Could not connect. Check the server address and try again."
                } else {
                    "Connection lost. Reconnect to return to your game."
                }
                .into(),
            );
        }
        next.set(GameState::MainMenu);
    }
}

fn cancel_connection_attempt(
    keyboard: Res<ButtonInput<KeyCode>>,
    state: Res<State<GameState>>,
    buttons: Query<(Entity, &Interaction), With<ConnectionCancel>>,
    focus: Option<Res<bevy::input_focus::InputFocus>>,
    mut next: ResMut<NextState<GameState>>,
    mut feedback: ResMut<ConnectionFeedback>,
) {
    if *state.get() != GameState::Connecting {
        return;
    }
    let activate =
        keyboard.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space]);
    let focused = focus.as_ref().and_then(|focus| focus.get());
    if keyboard.just_pressed(KeyCode::Escape)
        || buttons.iter().any(|(entity, interaction)| {
            *interaction == Interaction::Pressed || (activate && focused == Some(entity))
        })
    {
        feedback.error_message = None;
        next.set(GameState::MainMenu);
    }
}

fn release_connection(
    mut commands: Commands,
    clients: Query<Entity, With<crate::GameClient>>,
    mut attempt: ResMut<ConnectionAttempt>,
    mut input: ResMut<crate::ui::name_entry::PlayerNameInput>,
    mut phase: ResMut<crate::ui::name_entry::NameEntryPhase>,
) {
    attempt.address = None;
    attempt.started = None;
    input.submitted = false;
    *phase = crate::ui::name_entry::NameEntryPhase::Editing;
    for entity in &clients {
        commands.trigger(Disconnect { entity });
        commands.entity(entity).despawn();
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
    // Top-level since the static-subtree fix — despawned here, not via the root.
    lights: Query<
        Entity,
        Or<(
            With<super::rendering::SunLight>,
            With<super::rendering::FillLight>,
        )>,
    >,
    players: Query<Entity, With<Player>>,
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
    for light in lights.iter() {
        commands.entity(light).despawn();
    }

    for entity in players.iter() {
        commands.entity(entity).despawn();
    }

    loaded_chunks.chunks.clear();
    commands.insert_resource(ClearColor(Color::BLACK));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disconnect_during_preparation_or_gameplay_returns_to_launcher_with_feedback() {
        for state in [
            GameState::Connected,
            GameState::Playing,
            GameState::Connecting,
        ] {
            let mut app = App::new();
            app.insert_resource(State::new(state))
                .init_resource::<NextState<GameState>>()
                .init_resource::<ConnectionAttempt>()
                .init_resource::<ConnectionFeedback>()
                .add_systems(Update, update_connection_status);
            app.world_mut()
                .spawn((crate::GameClient, Disconnected::default()));
            app.update();
            assert!(matches!(
                app.world().resource::<NextState<GameState>>(),
                NextState::Pending(GameState::MainMenu)
            ));
            assert!(app
                .world()
                .resource::<ConnectionFeedback>()
                .error_message
                .is_some());
        }
    }

    #[test]
    fn cancelled_attempt_does_not_replace_launcher_feedback_with_late_disconnect() {
        let mut app = App::new();
        app.insert_resource(State::new(GameState::MainMenu))
            .init_resource::<NextState<GameState>>()
            .init_resource::<ConnectionAttempt>()
            .init_resource::<ConnectionFeedback>()
            .add_systems(Update, update_connection_status);
        app.world_mut()
            .spawn((crate::GameClient, Disconnected::default()));
        app.update();
        assert!(app
            .world()
            .resource::<ConnectionFeedback>()
            .error_message
            .is_none());
        assert!(matches!(
            app.world().resource::<NextState<GameState>>(),
            NextState::Unchanged
        ));
    }

    #[test]
    fn address_lookup_deadline_recovers_without_waiting_for_dns_worker() {
        let mut app = App::new();
        app.insert_resource(State::new(GameState::Connecting))
            .init_resource::<NextState<GameState>>()
            .insert_resource(ConnectionAttempt {
                started: Some(std::time::Instant::now() - std::time::Duration::from_secs(21)),
                ..default()
            })
            .init_resource::<ConnectionFeedback>()
            .add_systems(Update, finish_address_lookup);
        app.update();
        assert!(matches!(
            app.world().resource::<NextState<GameState>>(),
            NextState::Pending(GameState::MainMenu)
        ));
        assert!(app
            .world()
            .resource::<ConnectionFeedback>()
            .error_message
            .as_deref()
            .unwrap()
            .contains("timed out"));
    }

    #[test]
    fn literal_server_addresses_support_both_ip_families() {
        assert_eq!(
            resolve_server_address("127.0.0.1", 5000).unwrap(),
            "127.0.0.1:5000".parse::<SocketAddr>().unwrap()
        );
        assert_eq!(
            resolve_server_address("::1", 5000).unwrap(),
            "[::1]:5000".parse::<SocketAddr>().unwrap()
        );
    }
}
