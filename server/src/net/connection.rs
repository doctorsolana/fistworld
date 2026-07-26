//! Connection lifecycle systems.

use bevy::prelude::*;
use lightyear::prelude::server::*;
use lightyear::prelude::*;
use std::time::Duration;

use shared::components::{Player, PlayerPosition, PlayerProgression, PlayerRotation};
use shared::player_profile::{PlayerProfile, PROFILE_VERSION};
use shared::protocol::{NameSubmissionResult, PlayerInput, PlayerRoster, RequestPlayerRoster, SetTimeOfDay, SubmitPlayerName};

use crate::net::input::ClientInputs;
use crate::persistence::io_queue::{ProfileIoQueue, SavePriority};
use crate::persistence::profiles::PlayerProfiles;
use crate::player::roster_cache::PlayerRosterCache;

fn configured_replication_send_interval() -> Duration {
    const DEFAULT_MS: u64 = 33;
    let ms = std::env::var("CITYSIM_REPLICATION_SEND_INTERVAL_MS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .unwrap_or(DEFAULT_MS)
        .clamp(5, 250);
    Duration::from_millis(ms)
}

fn configured_replication_send_mode() -> SendUpdatesMode {
    match std::env::var("CITYSIM_REPLICATION_SEND_MODE") {
        Ok(raw) => match raw.trim().to_ascii_lowercase().as_str() {
            "ack" | "since_last_ack" => SendUpdatesMode::SinceLastAck,
            "send" | "since_last_send" => SendUpdatesMode::SinceLastSend,
            _ => SendUpdatesMode::SinceLastAck,
        },
        Err(_) => SendUpdatesMode::SinceLastAck,
    }
}

/// Handle new client connections - setup message channels.
/// Player spawning happens in `handle_player_name_submission` after name validation.
pub fn handle_connections(
    mut commands: Commands,
    new_clients: Query<(Entity, &RemoteId), Added<Connected>>,
    client_filter: Query<(), With<ClientOf>>,
) {
    for (client_entity, remote_id) in new_clients.iter() {
        if client_filter.get(client_entity).is_err() {
            continue;
        }

        let peer_id = remote_id.0;
        info!(
            "Client connected: {:?} - awaiting player name submission",
            peer_id
        );

        let replication_interval = configured_replication_send_interval();
        let replication_mode = configured_replication_send_mode();
        info!(
            "Replication sender config for {:?}: interval={}ms mode={:?}",
            peer_id,
            replication_interval.as_millis(),
            replication_mode
        );

        commands.entity(client_entity).insert((
            ReplicationSender::new(replication_interval, replication_mode, false),
            MessageReceiver::<PlayerInput>::default(),
            MessageReceiver::<SetTimeOfDay>::default(),
            MessageReceiver::<SubmitPlayerName>::default(),
            MessageReceiver::<RequestPlayerRoster>::default(),
        ));

        commands.entity(client_entity).insert((
            MessageSender::<NameSubmissionResult>::default(),
            MessageSender::<PlayerRoster>::default(),
        ));
    }
}

/// Save player state on disconnect.
/// This is an observer that triggers when a client gets `Disconnected` added.
pub fn handle_disconnections(
    trigger: On<Add, Disconnected>,
    mut profiles: ResMut<PlayerProfiles>,
    mut roster_cache: ResMut<PlayerRosterCache>,
    mut io_queue: ResMut<ProfileIoQueue>,
    client_entities: Query<&RemoteId>,
    players: Query<(
        Entity,
        &Player,
        &PlayerPosition,
        &PlayerRotation,
        &PlayerProgression,
    )>,
    mut inputs: ResMut<ClientInputs>,
) {
    let client_entity = trigger.entity;

    let peer_id = if let Ok(remote_id) = client_entities.get(client_entity) {
        remote_id.0
    } else {
        warn!(
            "Disconnect trigger for entity {:?} but no RemoteId found",
            client_entity
        );
        return;
    };

    info!(
        "PLAYER LEFT GAME - Client {:?} disconnected: {:?}",
        client_entity, peer_id
    );

    let name_lower = if let Some(name) = profiles.peer_to_name.get(&peer_id) {
        name.clone()
    } else {
        warn!(
            "Player {:?} disconnected but no name found in tracking - cannot save",
            peer_id
        );
        return;
    };

    // Preserve the player's authored capitalisation. `name_lower` is only the lookup key;
    // writing it into `player_name` silently lowercased display names after the first
    // disconnect. The cached profile (created by spawn.rs with the original casing) is
    // the source of truth.
    let display_name = profiles
        .profiles
        .get(&name_lower)
        .map(|p| p.player_name.clone())
        .unwrap_or_else(|| name_lower.clone());

    info!("Saving state for player '{}'", display_name);

    let mut found_player = None;
    for (player_entity, player, pos, rot, progression) in players.iter() {
        if player.client_id == peer_id {
            found_player = Some((player_entity, pos, rot, progression));
            break;
        }
    }

    let Some((_player_entity, pos, rot, progression)) = found_player
    else {
        warn!(
            "Player entity not found for disconnected peer {:?} - state not saved!",
            peer_id
        );
        profiles.peer_to_name.remove(&peer_id);
        profiles.name_to_peer.remove(&name_lower);
        inputs.latest.remove(&peer_id);
        return;
    };

    let profile = PlayerProfile {
        version: PROFILE_VERSION,
        player_name: display_name,
        position: [pos.0.x, pos.0.y, pos.0.z],
        rotation: rot.0,
        level: progression.level,
        prestige: progression.prestige,
        reputation: progression.reputation,
        stamina: progression.stamina,
        intelligence: progression.intelligence,
        bank_gold: profiles
            .profiles
            .get(&name_lower)
            .map(|p| p.bank_gold)
            .unwrap_or(0),
        last_login: std::time::SystemTime::now(),
        total_playtime_secs: profiles
            .profiles
            .get(&name_lower)
            .map(|p| p.total_playtime_secs)
            .unwrap_or(0),
    };

    roster_cache.upsert_profile(&profile);
    profiles
        .profiles
        .insert(name_lower.clone(), profile.clone());
    let job_id = io_queue.enqueue_profile_save(profile, SavePriority::High);
    io_queue.track_disconnect_job(job_id, name_lower.clone());
    info!(
        "Queued disconnect save for player '{}' (job {})",
        name_lower, job_id
    );


    profiles.peer_to_name.remove(&peer_id);
    profiles.name_to_peer.remove(&name_lower);
    info!("Freed up name '{}' for peer {:?}", name_lower, peer_id);

    inputs.latest.remove(&peer_id);
}
