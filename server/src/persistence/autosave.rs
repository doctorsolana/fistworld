//! Periodic autosave systems.

use bevy::prelude::*;
use shared::components::{ Health, Player, PlayerPosition, PlayerProgression, PlayerRotation,
    PlayerVelocity,
};
use shared::player_profile::{PlayerProfile, PROFILE_VERSION};

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
        &PlayerProgression,
        Option<&RespawnTimer>,
    )>,
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
        progression,
        respawn_timer,
    ) in players.iter()
    {
        let Some(name_lower) = profiles.peer_to_name.get(&player.client_id) else {
            continue;
        };

        let profile = PlayerProfile {
            version: PROFILE_VERSION,
            player_name: name_lower.clone(),
            position: [pos.0.x, pos.0.y, pos.0.z],
            rotation: rot.0,
            velocity: [vel.0.x, vel.0.y, vel.0.z],
            health_current: health.current,
            health_max: health.max,
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
