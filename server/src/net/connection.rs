//! Connection lifecycle systems.

use bevy::prelude::*;
use lightyear::prelude::server::*;
use lightyear::prelude::*;
use std::time::Duration;

use shared::components::{Player, PlayerPosition, PlayerProgression, PlayerRotation};
use shared::player_profile::{PlayerProfile, PROFILE_VERSION};

use crate::net::input::ClientInputs;
use crate::persistence::io_queue::{ProfileIoQueue, SavePriority};
use crate::persistence::profiles::PlayerProfiles;
use crate::player::roster_cache::PlayerRosterCache;

/// Replication send interval, applied app-wide via `ReplicationMetadata`.
///
/// lightyear 0.28 made the interval a global resource shared by all senders, so this can
/// no longer vary per client. The old `CITYSIM_REPLICATION_SEND_MODE` knob is gone with
/// it: the SinceLastAck/SinceLastSend distinction no longer exists.
pub fn configured_replication_send_interval() -> Duration {
    const DEFAULT_MS: u64 = 33;
    let ms = std::env::var("CITYSIM_REPLICATION_SEND_INTERVAL_MS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .unwrap_or(DEFAULT_MS)
        .clamp(5, 250);
    Duration::from_millis(ms)
}

/// Handle new client connections - enable replication to the new link.
/// Player spawning happens in `handle_player_name_submission` after name validation.
///
/// Message channels need no setup here: since lightyear 0.28 every registered message's
/// `MessageSender`/`MessageReceiver` is a required component of `ClientOf` and appears on
/// the link entity automatically (re-inserting them here would overwrite buffers and drop
/// messages received between link creation and `Connected`).
pub fn handle_connections(
    mut commands: Commands,
    new_clients: Query<(Entity, &RemoteId), Added<Connected>>,
    client_filter: Query<(), With<ClientOf>>,
) {
    for (client_entity, remote_id) in new_clients.iter() {
        if client_filter.get(client_entity).is_err() {
            continue;
        }

        info!(
            "Client connected: {:?} - awaiting player name submission",
            remote_id.0
        );

        commands.entity(client_entity).insert(ReplicationSender);
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
    heroes: Query<(
        &shared::components::Hero,
        &PlayerPosition,
        &PlayerRotation,
        &shared::components::HeroOutfit,
    )>,
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

    // The hero KEEPS STANDING in the world (it carries no ControlledBy, so no
    // lifetime despawns it) and is re-adopted when this player returns.
    //
    // Its move order is deliberately NOT cancelled. Orders now live on the unit
    // rather than in a per-peer map, and a retinue is keyed by account name, so
    // a villager mid-walk keeps walking and finishes where it was sent -- the
    // world is meant to carry on without you (WORLD-DESIGN pillar 1). The old
    // per-peer map could not express that: one removal cancelled every order the
    // player had given.

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

    let Some((_player_entity, pos, rot, progression)) = found_player else {
        warn!(
            "Player entity not found for disconnected peer {:?} - state not saved!",
            peer_id
        );
        profiles.peer_to_name.remove(&peer_id);
        profiles.name_to_peer.remove(&name_lower);
        inputs.latest.remove(&peer_id);
        return;
    };

    // Snapshot the hero so a SERVER RESTART can rebuild it; the live entity
    // itself survives an ordinary disconnect.
    let hero_state = heroes
        .iter()
        .find(|(hero, _, _, _)| hero.owner == peer_id)
        .map(|(_, position, rotation, outfit)| {
            crate::player::hero::hero_save(position, rotation, outfit)
        });

    let profile = PlayerProfile {
        version: PROFILE_VERSION,
        player_name: display_name,
        hero: hero_state,
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
