//! Periodic autosave systems.

use bevy::prelude::*;
use shared::components::{
    CharacterAttributes, Health, Hero, HeroOutfit, Player, PlayerPosition, PlayerProgression,
    PlayerRotation,
};
use shared::player_profile::PlayerProfile;
use shared::protocol::DEFAULT_COMMANDER_ZOOM;

use crate::net::input::ClientInputs;
use crate::persistence::profiles::PlayerProfiles;

/// How often to refresh connection-independent session profiles (seconds).
const AUTO_SAVE_INTERVAL: f32 = 30.0;

/// Periodically refresh all connected players' reconnect snapshots.
pub fn update_periodic_player_save(
    mut profiles: ResMut<PlayerProfiles>,
    inputs: Res<ClientInputs>,
    players: Query<(
        &Player,
        &PlayerPosition,
        &PlayerRotation,
        &PlayerProgression,
    )>,
    heroes: Query<(
        &Hero,
        &PlayerPosition,
        &PlayerRotation,
        &HeroOutfit,
        &CharacterAttributes,
        &Health,
    )>,
    time: Res<Time>,
    mut last_save_time: Local<f32>,
) {
    let now = time.elapsed_secs();
    if now - *last_save_time < AUTO_SAVE_INTERVAL {
        return;
    }

    *last_save_time = now;

    let mut snapshots = Vec::new();
    for (player, pos, rot, progression) in players.iter() {
        let Some(name_lower) = profiles.peer_to_name.get(&player.client_id) else {
            continue;
        };

        // Heroes outlive connections. The compact snapshot keeps the commander
        // profile current; the live body retains exact inventory and wallet.
        let hero_snapshot = heroes
            .iter()
            .find(|(hero, ..)| hero.owner == player.client_id);
        let hero_state = hero_snapshot.map(|(_, position, rotation, outfit, _, health)| {
            crate::player::hero::hero_save(position, rotation, outfit, health)
        });
        let attributes = hero_snapshot.map(|(_, _, _, _, attributes, _)| *attributes);

        let previous = profiles.profiles.get(name_lower);
        let latest_view = inputs
            .latest
            .get(&player.client_id)
            .and_then(|input| input.commander_view());
        let profile = PlayerProfile {
            hero: hero_state,
            player_name: profiles
                .profiles
                .get(name_lower)
                .map(|p| p.player_name.clone())
                .unwrap_or_else(|| name_lower.clone()),
            position: latest_view
                .map(|view| view.focus.to_array())
                .unwrap_or([pos.0.x, pos.0.y, pos.0.z]),
            rotation: latest_view.map_or(rot.0, |view| view.yaw),
            zoom: latest_view
                .map(|view| view.zoom)
                .or_else(|| previous.map(|profile| profile.zoom))
                .unwrap_or(DEFAULT_COMMANDER_ZOOM),
            level: progression.level,
            prestige: progression.prestige,
            reputation: progression.reputation,
            stamina: attributes
                .map(|attributes| u32::from(attributes.physique()))
                .unwrap_or(progression.stamina),
            intelligence: attributes
                .map(|attributes| u32::from(attributes.intelligence()))
                .unwrap_or(progression.intelligence),
            charm: attributes
                .map(|attributes| u32::from(attributes.charm()))
                .unwrap_or(progression.charm),
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

        snapshots.push((name_lower.clone(), profile));
    }

    for (name_lower, profile) in snapshots {
        profiles.profiles.insert(name_lower, profile);
    }
}
