//! Player spawn/setup systems.

use bevy::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::{
    ControlledBy, Lifetime, MessageReceiver, MessageSender, NetworkTarget, RemoteId, Replicate,
    ReplicationMode,
};

use shared::components::{
    EquippedWeapon, Health, Player, PlayerCharacter, PlayerGrounded, PlayerPosition,
    PlayerProgression, PlayerRotation, PlayerVelocity,
};
use shared::items::{HotbarSelection, Inventory};
use shared::physics::ground_clearance_center;
use shared::player::SPAWN_POSITION;
use shared::player_profile::PlayerProfile;
use shared::protocol::{
    NameRejectionReason, NameSubmissionResult, ReliableChannel, SetPlayerCharacter,
    SubmitPlayerName,
};
use shared::terrain::WorldTerrain;
use shared::vehicle::{InVehicle, Vehicle, VehicleDriver, VehicleState, VehicleType};
use shared::weapons::WeaponType;

use crate::inventory::hotbar::PreviousHotbarSlot;
use crate::net::peer::peer_id_to_u64;
use crate::persistence::profiles::PlayerProfiles;
use crate::player::index::PlayerEntityIndex;
use crate::player::roster_cache::PlayerRosterCache;

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
                equipped_weapon,
                weapon_ammo,
                inventory,
                hotbar_sel,
                vehicle_spawn,
            ): (
                Vec3,
                f32,
                Vec3,
                Health,
                EquippedWeapon,
                u32,
                Inventory,
                u8,
                Option<(VehicleType, [f32; 3], [f32; 3], [f32; 3], [f32; 3])>,
            ) = if profile.is_dead {
                info!(
                    "Player '{}' was dead - spawning at spawn point with empty inventory",
                    name
                );
                let spawn_x = SPAWN_POSITION[0];
                let spawn_z = SPAWN_POSITION[2];
                let ground_y = terrain.get_height(spawn_x, spawn_z);
                let pos = Vec3::new(spawn_x, ground_y + ground_clearance_center(), spawn_z);

                (
                    pos,
                    0.0,
                    Vec3::ZERO,
                    Health::default(),
                    EquippedWeapon::new(WeaponType::AssaultRifle),
                    30,
                    Inventory::new(),
                    0,
                    None,
                )
            } else if profile.in_vehicle {
                info!("Player '{}' was in vehicle - restoring vehicle state", name);

                let veh_pos = profile.vehicle_position.unwrap_or(profile.position);
                let veh_rot = profile
                    .vehicle_rotation
                    .unwrap_or([profile.rotation, 0.0, 0.0]);
                let veh_vel = profile.vehicle_velocity.unwrap_or([0.0, 0.0, 0.0]);
                let veh_ang_vel = profile.vehicle_angular_velocity.unwrap_or([0.0, 0.0, 0.0]);
                let veh_type = profile.vehicle_type.unwrap_or(VehicleType::Motorbike);

                let mut inventory = Inventory::new();
                for (i, slot) in profile.inventory_slots.iter().enumerate() {
                    if let Some(stack) = slot {
                        let _ = inventory.set_slot(i, Some(*stack));
                    }
                }

                (
                    Vec3::from_slice(&veh_pos),
                    veh_rot[0],
                    Vec3::ZERO,
                    Health {
                        current: profile.health_current,
                        max: profile.health_max,
                    },
                    EquippedWeapon::new(profile.equipped_weapon),
                    profile.weapon_ammo_in_mag,
                    inventory,
                    profile.hotbar_selection,
                    Some((veh_type, veh_pos, veh_rot, veh_vel, veh_ang_vel)),
                )
            } else {
                info!(
                    "Player '{}' spawning at saved position {:?}",
                    name, profile.position
                );

                let mut inventory = Inventory::new();
                for (i, slot) in profile.inventory_slots.iter().enumerate() {
                    if let Some(stack) = slot {
                        let _ = inventory.set_slot(i, Some(*stack));
                    }
                }

                (
                    Vec3::from_slice(&profile.position),
                    profile.rotation,
                    Vec3::from_slice(&profile.velocity),
                    Health {
                        current: profile.health_current,
                        max: profile.health_max,
                    },
                    EquippedWeapon::new(profile.equipped_weapon),
                    profile.weapon_ammo_in_mag,
                    inventory,
                    profile.hotbar_selection,
                    None,
                )
            };

            let progression = PlayerProgression {
                level: profile.level,
                prestige: profile.prestige,
                reputation: profile.reputation,
                stamina: profile.stamina,
                intelligence: profile.intelligence,
            };

            let mut equipped_weapon_component = equipped_weapon;
            equipped_weapon_component.ammo_in_mag = weapon_ammo;

            let player_entity = commands
                .spawn((
                    Player { client_id: peer_id },
                    PlayerPosition(spawn_pos),
                    PlayerRotation(spawn_rot),
                    PlayerVelocity(spawn_vel),
                    PlayerGrounded::default(),
                    PlayerCharacter::default(),
                    health,
                    progression,
                    equipped_weapon_component,
                    inventory,
                    HotbarSelection { index: hotbar_sel },
                    PreviousHotbarSlot {
                        index: Some(hotbar_sel as usize),
                    },
                    Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
                    ControlledBy {
                        owner: client_entity,
                        lifetime: Lifetime::default(),
                    },
                ))
                .id();

            if let Some((veh_type, veh_pos, veh_rot, veh_vel, veh_ang_vel)) = vehicle_spawn {
                let vehicle_entity = commands
                    .spawn((
                        Vehicle {
                            vehicle_type: veh_type,
                        },
                        VehicleState {
                            position: Vec3::from_slice(&veh_pos),
                            heading: veh_rot[0],
                            pitch: veh_rot[1],
                            roll: veh_rot[2],
                            velocity: Vec3::from_slice(&veh_vel),
                            angular_velocity_yaw: veh_ang_vel[0],
                            angular_velocity_pitch: veh_ang_vel[1],
                            angular_velocity_roll: veh_ang_vel[2],
                            grounded: true,
                        },
                        VehicleDriver {
                            driver_id: Some(peer_id_to_u64(peer_id)),
                        },
                        Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
                    ))
                    .id();

                commands
                    .entity(player_entity)
                    .insert(InVehicle { vehicle_entity });

                info!("Spawned vehicle {:?} for player '{}'", veh_type, name);
            }

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
