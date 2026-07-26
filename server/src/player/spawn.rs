//! Player spawn/setup systems.

use bevy::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::{
    ControlledBy, Lifetime, MessageReceiver, MessageSender, NetworkTarget, RemoteId, Replicate,
    ReplicationGroup, ReplicationMode,
};

use shared::components::{
    Health, Player, PlayerCharacter, PlayerGrounded, PlayerPosition,
    PlayerProgression, PlayerRotation, PlayerVelocity,
};
use shared::physics::ground_clearance_center;
use shared::player::SPAWN_POSITION;
use shared::player_profile::PlayerProfile;
use shared::protocol::{
    NameRejectionReason, NameSubmissionResult, ReliableChannel, SetPlayerCharacter,
    SubmitPlayerName,
};
use shared::terrain::WorldTerrain;

use crate::persistence::profiles::PlayerProfiles;
use crate::player::index::PlayerEntityIndex;
use crate::player::roster_cache::PlayerRosterCache;

const PLAYER_REPLICATION_PRIORITY: f32 = 20.0;
const VEHICLE_REPLICATION_PRIORITY: f32 = 5.0;

fn resolve_map_spawn_position(terrain: &WorldTerrain) -> Vec3 {
    if let Some(spawn) = terrain.generator.loaded_map().definition.player_spawn {
        let ground_y = terrain.get_height(spawn[0], spawn[2]);
        return Vec3::new(spawn[0], ground_y + ground_clearance_center(), spawn[2]);
    }

    let spawn_x = SPAWN_POSITION[0];
    let spawn_z = SPAWN_POSITION[2];
    let ground_y = terrain.get_height(spawn_x, spawn_z);
    Vec3::new(spawn_x, ground_y + ground_clearance_center(), spawn_z)
}

/// Handle player name submissions from clients.
/// Validates name, loads/creates profile, spawns player entity.
pub fn handle_player_name_submission(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    mut profiles: ResMut<PlayerProfiles>,
    mut roster_cache: ResMut<PlayerRosterCache>,
    mut client_links: Query<
        (
            Entity,
            &RemoteId,
            &mut MessageReceiver<SubmitPlayerName>,
            &mut MessageSender<NameSubmissionResult>,
        ),
        With<ClientOf>,
    >,
) {
    for (client_entity, remote_id, mut receiver, mut sender) in client_links.iter_mut() {
        let peer_id = remote_id.0;

        if profiles.peer_to_name.contains_key(&peer_id) {
            continue;
        }

        for submission in receiver.receive() {
            let name = submission.name.trim().to_string();
            info!("Received name submission from {:?}: '{}'", peer_id, name);

            if let Err(reason) = PlayerProfiles::validate_name(&name) {
                warn!("Name '{}' rejected: {:?}", name, reason);
                sender.send::<ReliableChannel>(NameSubmissionResult::Rejected { reason });
                continue;
            }

            if profiles.is_name_online(&name) {
                warn!("Name '{}' rejected: already online", name);
                sender.send::<ReliableChannel>(NameSubmissionResult::Rejected {
                    reason: NameRejectionReason::AlreadyOnline,
                });
                continue;
            }

            let name_lower = name.to_lowercase();
            let (profile, profile_loaded) = match profiles.load_profile(&name) {
                Ok(profile) => {
                    info!("Loaded existing profile for '{}'", name);
                    (profile, true)
                }
                Err(e) => {
                    info!("Creating new profile for '{}': {}", name, e);
                    (PlayerProfile::new_player(name.clone()), false)
                }
            };

            let (
                spawn_pos,
                spawn_rot,
                spawn_vel,
                health,
            ): (
                Vec3,
                f32,
                Vec3,
                Health,
            ) = if profile.is_dead {
                info!(
                    "Player '{}' was dead - spawning at spawn point with empty inventory",
                    name
                );
                let pos = resolve_map_spawn_position(&terrain);

                (
                    pos,
                    0.0,
                    Vec3::ZERO,
                    Health::default(),                )
            } else if !profile_loaded {
                info!("Spawning new player '{}' at map spawn", name);


                (
                    resolve_map_spawn_position(&terrain),
                    profile.rotation,
                    Vec3::ZERO,
                    Health {
                        current: profile.health_current,
                        max: profile.health_max,
                    },                )
            } else {
                info!(
                    "Player '{}' spawning at saved position {:?}",
                    name, profile.position
                );


                (
                    Vec3::from_slice(&profile.position),
                    profile.rotation,
                    Vec3::from_slice(&profile.velocity),
                    Health {
                        current: profile.health_current,
                        max: profile.health_max,
                    },                )
            };

            let progression = PlayerProgression {
                level: profile.level,
                prestige: profile.prestige,
                reputation: profile.reputation,
                stamina: profile.stamina,
                intelligence: profile.intelligence,
            };

            let _player_entity = commands
                .spawn((
                    Player { client_id: peer_id },
                    PlayerPosition(spawn_pos),
                    PlayerRotation(spawn_rot),
                    PlayerVelocity(spawn_vel),
                    PlayerGrounded::default(),
                    PlayerCharacter::default(),
                    health,
                    progression,
                    ReplicationGroup::new_from_entity().set_priority(PLAYER_REPLICATION_PRIORITY),
                    Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
                    ControlledBy {
                        owner: client_entity,
                        lifetime: Lifetime::default(),
                    },
                ))
                .id();


            profiles.peer_to_name.insert(peer_id, name_lower.clone());
            profiles.name_to_peer.insert(name_lower.clone(), peer_id);
            roster_cache.upsert_profile(&profile);
            profiles.profiles.insert(name_lower, profile);

            sender.send::<ReliableChannel>(NameSubmissionResult::Accepted { profile_loaded });
            info!("Player '{}' spawned successfully for {:?}", name, peer_id);
        }
    }
}

/// Handle player character selection requests from clients.
pub fn handle_set_player_character(
    mut commands: Commands,
    mut client_links: Query<(&RemoteId, &mut MessageReceiver<SetPlayerCharacter>), With<ClientOf>>,
    player_index: Res<PlayerEntityIndex>,
    players: Query<(Entity, &Player, Option<&PlayerCharacter>)>,
) {
    for (remote_id, mut receiver) in client_links.iter_mut() {
        let peer_id = remote_id.0;
        for msg in receiver.receive() {
            let player_entity = player_index.entity_for_peer(peer_id).or_else(|| {
                players
                    .iter()
                    .find_map(|(entity, player, _)| (player.client_id == peer_id).then_some(entity))
            });
            let Some(player_entity) = player_entity else {
                continue;
            };
            let Ok((_entity, _player, current)) = players.get(player_entity) else {
                continue;
            };

            if current.is_some_and(|c| *c == msg.character) {
                continue;
            }

            commands.entity(player_entity).insert(msg.character);
            info!(
                "Player {:?} selected character {:?}",
                peer_id, msg.character
            );
        }
    }
}
