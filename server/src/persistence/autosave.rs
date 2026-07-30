//! Periodic autosave systems.

use bevy::prelude::*;
use shared::components::{
    Hero, HeroOutfit, Player, PlayerPosition, PlayerProgression, PlayerRotation,
};
use shared::player_profile::{PlayerProfile, PROFILE_VERSION};

use crate::persistence::io_queue::{ProfileIoQueue, SavePriority};
use crate::persistence::profiles::PlayerProfiles;
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
        &PlayerProgression,
    )>,
    heroes: Query<(&Hero, &PlayerPosition, &PlayerRotation, &HeroOutfit)>,
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
        progression,
    ) in players.iter()
    {
        let Some(name_lower) = profiles.peer_to_name.get(&player.client_id) else {
            continue;
        };

        // Heroes outlive connections, so the snapshot is only needed to
        // survive a server restart -- but it must be current when one happens.
        let hero_state = heroes
            .iter()
            .find(|(hero, _, _, _)| hero.owner == player.client_id)
            .map(|(_, position, rotation, outfit)| {
                crate::player::hero::hero_save(position, rotation, outfit)
            });

        let profile = PlayerProfile {
            version: PROFILE_VERSION,
            hero: hero_state,
            player_name: profiles
                .profiles
                .get(name_lower)
                .map(|p| p.player_name.clone())
                .unwrap_or_else(|| name_lower.clone()),
            position: [pos.0.x, pos.0.y, pos.0.z],
            rotation: rot.0,
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
