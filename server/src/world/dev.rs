//! Dev-mode gate for god commands.

use bevy::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::{MessageReceiver, RemoteId};

use shared::components::{Hero, TimeWarp};
use shared::protocol::DevCommand;
use shared::terrain::WorldTerrain;

/// Whether this server honours god commands. Read once from `FISTWORLD_DEV` at startup;
/// production deployments simply never set the variable.
#[derive(Resource)]
pub struct DevMode(pub bool);

impl Default for DevMode {
    fn default() -> Self {
        let enabled = parse_dev_flag(std::env::var("FISTWORLD_DEV").ok());
        if enabled {
            info!("Dev mode: god commands enabled");
        }
        Self(enabled)
    }
}

fn parse_dev_flag(raw: Option<String>) -> bool {
    raw.is_some_and(|raw| matches!(raw.trim(), "1" | "true"))
}

/// Drain god commands from every client link.
///
/// Receivers are drained even with dev mode off so messages never accumulate; they are
/// just never applied.
#[allow(clippy::too_many_arguments)]
pub fn handle_dev_commands(
    mut commands: Commands,
    dev: Res<DevMode>,
    terrain: Option<Res<WorldTerrain>>,
    profiles: Res<crate::persistence::profiles::PlayerProfiles>,
    mut hero_index: ResMut<crate::player::hero::HeroIndex>,
    heroes: Query<&Hero>,
    mut client_links: Query<(&RemoteId, &mut MessageReceiver<DevCommand>), With<ClientOf>>,
    mut warp: Query<&mut TimeWarp>,
    mut warned_peers: Local<bevy::platform::collections::HashSet<lightyear::prelude::PeerId>>,
) {
    // Spawns go through deferred Commands, so the Hero query cannot see a
    // spawn from earlier in this same drain — track them here or a burst of
    // two reliable SpawnHero messages in one tick defeats one-per-player.
    let mut spawned_this_run = bevy::platform::collections::HashSet::new();
    for (remote_id, mut receiver) in client_links.iter_mut() {
        for command in receiver.receive() {
            if !dev.0 {
                // A legitimate client never sends these without the grant; log the first
                // violation per peer, never per message, so a hostile client can't flood
                // production logs.
                if warned_peers.insert(remote_id.0) {
                    warn!(
                        "Ignoring {:?} from {:?}: dev mode disabled",
                        command, remote_id.0
                    );
                }
                continue;
            }

            match command {
                DevCommand::SetTimeWarp(factor) => {
                    let Some(mut tw) = warp.iter_mut().next() else {
                        continue;
                    };
                    let next = TimeWarp::clamped(factor);
                    // Change detection drives replication, so only touch on change.
                    if *tw != next {
                        *tw = next;
                    }
                    info!("Dev: time warp set to {}x by {:?}", tw.0, remote_id.0);
                }
                DevCommand::SpawnHero { pos, outfit } => {
                    // One hero per player, enforced HERE (the client also
                    // greys its button, but the server is the authority).
                    let Some(name_lower) = profiles.peer_to_name.get(&remote_id.0).cloned() else {
                        info!("Dev: ignoring SpawnHero from {:?}: no profile", remote_id.0);
                        continue;
                    };
                    if spawned_this_run.contains(&remote_id.0)
                        || hero_index.by_name.contains_key(&name_lower)
                        || heroes.iter().any(|h| h.owner == remote_id.0)
                    {
                        info!("Dev: ignoring SpawnHero from {:?}: hero exists", remote_id.0);
                        continue;
                    }
                    if !pos.is_finite() {
                        continue;
                    }
                    let Some(terrain) = terrain.as_ref() else {
                        continue;
                    };
                    let entity = crate::player::hero::spawn_hero(
                        &mut commands,
                        &mut hero_index,
                        terrain,
                        remote_id.0,
                        &name_lower,
                        pos,
                        0.0,
                        outfit,
                    );
                    spawned_this_run.insert(remote_id.0);
                    info!("Dev: hero {entity:?} spawned for '{name_lower}' at {pos:?}");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_flag_only_accepts_explicit_enables() {
        assert!(parse_dev_flag(Some("1".to_string())));
        assert!(parse_dev_flag(Some("true".to_string())));
        assert!(parse_dev_flag(Some(" true ".to_string())));
        assert!(!parse_dev_flag(Some("0".to_string())));
        assert!(!parse_dev_flag(Some("false".to_string())));
        assert!(!parse_dev_flag(Some("yes".to_string())));
        assert!(!parse_dev_flag(Some(String::new())));
        assert!(!parse_dev_flag(None));
    }
}
