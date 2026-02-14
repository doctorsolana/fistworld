//! Connection lifecycle systems.

use bevy::prelude::*;
use lightyear::prelude::server::*;
use lightyear::prelude::*;

use shared::components::{
    EquippedWeapon, Health, Player, PlayerPosition, PlayerProgression, PlayerRotation,
    PlayerVelocity,
};
use shared::items::{
    ChestTransferRequest, CloseChestRequest, DropRequest, HotbarSelection, Inventory,
    InventoryMoveRequest, OpenChestRequest, PickupRequest, SelectHotbarSlot,
};
use shared::player_profile::{PlayerProfile, PROFILE_VERSION};
use shared::protocol::{
    tick_duration, BulletImpact, DamageReceived, HitConfirm, NameSubmissionResult, PlayerInput,
    PlayerKilled, PlayerRoster, ReloadRequest, RequestPlayerRoster, SetPlayerCharacter,
    SetTimeOfDay, ShootRequest, SubmitPlayerName, SwitchWeapon,
};
use shared::vehicle::{InVehicle, Vehicle, VehicleDriver, VehicleState};

use crate::net::input::ClientInputs;
use crate::net::peer::peer_id_to_u64;
use crate::persistence::io_queue::{ProfileIoQueue, SavePriority};
use crate::persistence::profiles::PlayerProfiles;
use crate::player::lifecycle::RespawnTimer;
use crate::player::roster_cache::PlayerRosterCache;

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

        commands.entity(client_entity).insert((
            ReplicationSender::new(tick_duration(), SendUpdatesMode::SinceLastAck, false),
            MessageReceiver::<PlayerInput>::default(),
            MessageReceiver::<ShootRequest>::default(),
            MessageReceiver::<SwitchWeapon>::default(),
            MessageReceiver::<ReloadRequest>::default(),
            MessageReceiver::<SetTimeOfDay>::default(),
            MessageReceiver::<SetPlayerCharacter>::default(),
            MessageReceiver::<SubmitPlayerName>::default(),
            MessageReceiver::<RequestPlayerRoster>::default(),
        ));

        commands.entity(client_entity).insert((
            MessageReceiver::<PickupRequest>::default(),
            MessageReceiver::<DropRequest>::default(),
            MessageReceiver::<SelectHotbarSlot>::default(),
            MessageReceiver::<InventoryMoveRequest>::default(),
            MessageReceiver::<OpenChestRequest>::default(),
            MessageReceiver::<CloseChestRequest>::default(),
            MessageReceiver::<ChestTransferRequest>::default(),
        ));

        commands.entity(client_entity).insert((
            MessageSender::<HitConfirm>::default(),
            MessageSender::<DamageReceived>::default(),
            MessageSender::<PlayerKilled>::default(),
            MessageSender::<BulletImpact>::default(),
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
        &PlayerVelocity,
        &Health,
        &EquippedWeapon,
        &Inventory,
        &HotbarSelection,
        &PlayerProgression,
        Option<&InVehicle>,
        Option<&RespawnTimer>,
    )>,
    mut vehicles: Query<(&mut VehicleDriver, &VehicleState, &Vehicle)>,
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

    info!("Saving state for player '{}'", name_lower);

    let mut found_player = None;
    for (
        player_entity,
        player,
        pos,
        rot,
        vel,
        health,
        weapon,
        inventory,
        hotbar,
        progression,
        in_vehicle,
        respawn_timer,
    ) in players.iter()
    {
        if player.client_id == peer_id {
            found_player = Some((
                player_entity,
                pos,
                rot,
                vel,
                health,
                weapon,
                inventory,
                hotbar,
                progression,
                in_vehicle,
                respawn_timer,
            ));
            break;
        }
    }

    let Some((
        _player_entity,
        pos,
        rot,
        vel,
        health,
        weapon,
        inventory,
        hotbar,
        progression,
        in_vehicle,
        respawn_timer,
    )) = found_player
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

    let (vehicle_data, in_veh) = if let Some(in_veh) = in_vehicle {
        if let Ok((mut driver, veh_state, vehicle)) = vehicles.get_mut(in_veh.vehicle_entity) {
            driver.driver_id = None;

            let veh_type = vehicle.vehicle_type;
            let veh_pos = [
                veh_state.position.x,
                veh_state.position.y,
                veh_state.position.z,
            ];
            let veh_rot = [veh_state.heading, veh_state.pitch, veh_state.roll];
            let veh_vel = [
                veh_state.velocity.x,
                veh_state.velocity.y,
                veh_state.velocity.z,
            ];
            let veh_ang_vel = [
                veh_state.angular_velocity_yaw,
                veh_state.angular_velocity_pitch,
                veh_state.angular_velocity_roll,
            ];

            (
                Some((veh_type, veh_pos, veh_rot, veh_vel, veh_ang_vel)),
                true,
            )
        } else {
            (None, false)
        }
    } else {
        (None, false)
    };

    let profile = PlayerProfile {
        version: PROFILE_VERSION,
        player_name: name_lower.clone(),
        position: [pos.0.x, pos.0.y, pos.0.z],
        rotation: rot.0,
        velocity: [vel.0.x, vel.0.y, vel.0.z],
        health_current: health.current,
        health_max: health.max,
        equipped_weapon: weapon.weapon_type,
        weapon_ammo_in_mag: weapon.ammo_in_mag,
        inventory_slots: *inventory.slots(),
        hotbar_selection: hotbar.index,
        in_vehicle: in_veh,
        vehicle_type: vehicle_data.as_ref().map(|(vt, _, _, _, _)| *vt),
        vehicle_position: vehicle_data.as_ref().map(|(_, pos, _, _, _)| *pos),
        vehicle_rotation: vehicle_data.as_ref().map(|(_, _, rot, _, _)| *rot),
        vehicle_velocity: vehicle_data.as_ref().map(|(_, _, _, vel, _)| *vel),
        vehicle_angular_velocity: vehicle_data.as_ref().map(|(_, _, _, _, ang)| *ang),
        is_dead: respawn_timer.is_some() || health.is_dead(),
        death_timestamp: if respawn_timer.is_some() || health.is_dead() {
            Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs_f64(),
            )
        } else {
            None
        },
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

    for (mut driver, _, _) in vehicles.iter_mut() {
        if driver.driver_id == Some(peer_id_to_u64(peer_id)) {
            driver.driver_id = None;
        }
    }

    profiles.peer_to_name.remove(&peer_id);
    profiles.name_to_peer.remove(&name_lower);
    info!("Freed up name '{}' for peer {:?}", name_lower, peer_id);

    inputs.latest.remove(&peer_id);
}
