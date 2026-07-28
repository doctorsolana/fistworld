//! Player spawn/setup systems.

use bevy::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::{
    ControlledBy, Lifetime, MessageReceiver, MessageSender, NetworkTarget, RemoteId, Replicate,
};

use shared::components::{Player, PlayerPosition, PlayerProgression, PlayerRotation};
use shared::physics::ground_clearance_center;
use shared::player::SPAWN_POSITION;
use shared::player_profile::PlayerProfile;
use shared::protocol::{
    DevStatus, NameRejectionReason, NameSubmissionResult, ReliableChannel, SubmitPlayerName,
};
use shared::terrain::WorldTerrain;

use crate::persistence::profiles::PlayerProfiles;
use crate::player::roster_cache::PlayerRosterCache;
use crate::world::dev::DevMode;

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
            &mut MessageSender<DevStatus>,
        ),
        With<ClientOf>,
    >,
    dev: Res<DevMode>,
) {
    for (client_entity, remote_id, mut receiver, mut sender, mut dev_sender) in
        client_links.iter_mut()
    {
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

            // The commander has no body: restore the saved camera focus + yaw.
            let (spawn_pos, spawn_rot): (Vec3, f32) = if profile_loaded {
                info!(
                    "Player '{}' resuming at saved view {:?}",
                    name, profile.position
                );
                (Vec3::from_slice(&profile.position), profile.rotation)
            } else {
                info!("Spawning new player '{}' at map spawn", name);
                (resolve_map_spawn_position(&terrain), profile.rotation)
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
                    // The region tag opts this entity into interest management:
                    // `apply_region_visibility` hides it from clients whose interest
                    // does not cover its region. In lightyear 0.28 replicated entities
                    // are visible to everyone by default, so the visibility pass must
                    // run before the entity leaks world-wide.
                    shared::region::RegionCoord::from_world_pos(spawn_pos),
                    PlayerPosition(spawn_pos),
                    PlayerRotation(spawn_rot),
                    progression,
                    Replicate::to_clients(NetworkTarget::All),
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
            dev_sender.send::<ReliableChannel>(DevStatus { god: dev.0 });
            info!("Player '{}' spawned successfully for {:?}", name, peer_id);
        }
    }
}
