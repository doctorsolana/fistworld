//! Periodic autosave systems.

use bevy::prelude::*;
use shared::components::{ Health, Player, PlayerPosition, PlayerProgression, PlayerRotation,
    PlayerVelocity,
};
use shared::items::{HotbarSelection, Inventory};
use shared::player_profile::{PlayerProfile, PROFILE_VERSION};
use shared::vehicle::{InVehicle, Vehicle, VehicleState};

use crate::persistence::io_queue::{ProfileIoQueue, SavePriority};
use crate::persistence::profiles::PlayerProfiles;
use crate::player::lifecycle::RespawnTimer;
use crate::player::roster_cache::PlayerRosterCache;

/// How often to auto-save all players (seconds).
/// This is a safety backup - primary save happens on disconnect.
const AUTO_SAVE_INTERVAL: f32 = 30.0;

/// Periodically save all connected players.
pub fn update_periodic_player_save(
    profiles: Res<PlayerProfiles>,
    mut io_queue: ResMut<ProfileIoQueue>,
    mut roster_cache: ResMut<PlayerRosterCache>,
    players: Query<(
        &Player,
        &PlayerPosition,
        &PlayerRotation,
        &PlayerVelocity,
        &Health,
        &Inventory,
        &HotbarSelection,
        &PlayerProgression,
        Option<&InVehicle>,
        Option<&RespawnTimer>,
    )>,
    vehicles: Query<(&VehicleState, &Vehicle)>,
    time: Res<Time>,
    mut last_save_time: Local<f32>,
) {
    let now = time.elapsed_secs();
    if now - *last_save_time < AUTO_SAVE_INTERVAL {
        return;
    }

    *last_save_time = now;

    let mut saved_count = 0;
    for (
        player,
        pos,
        rot,
        vel,
        health,
        inventory,
        hotbar,
        progression,
        in_vehicle,
        respawn_timer,
    ) in players.iter()
    {
        let Some(name_lower) = profiles.peer_to_name.get(&player.client_id) else {
            continue;
        };

        let (vehicle_data, in_veh) = if let Some(in_veh) = in_vehicle {
            if let Ok((veh_state, vehicle)) = vehicles.get(in_veh.vehicle_entity) {
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
                .get(name_lower)
                .map(|p| p.bank_gold)
                .unwrap_or(0),
            last_login: std::time::SystemTime::now(),
            total_playtime_secs: profiles
                .profiles
                .get(name_lower)
                .map(|p| p.total_playtime_secs)
                .unwrap_or(0),
        };

        roster_cache.upsert_profile(&profile);
        let _job_id = io_queue.enqueue_profile_save(profile, SavePriority::Normal);
        saved_count += 1;
    }

    if saved_count > 0 {
        info!("Queued auto-save for {} player profile(s)", saved_count);
    }
}
