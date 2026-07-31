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
/// Counter behind generated villager names, so each spawn is a distinct person.
#[derive(Resource, Default)]
pub struct VillagerSeed(pub u64);

/// How far apart settlements must be, in metres.
///
/// Two settlements closer than this would fight over the same plan footprint
/// and the same working radius of land, and it is what stops the map being
/// carpeted in halls.
pub const MIN_SETTLEMENT_SPACING: f32 = 300.0;

pub fn handle_dev_commands(
    mut commands: Commands,
    dev: Res<DevMode>,
    terrain: Option<Res<WorldTerrain>>,
    profiles: Res<crate::persistence::profiles::PlayerProfiles>,
    mut hero_index: ResMut<crate::player::hero::HeroIndex>,
    heroes: Query<&Hero>,
    mut named: Query<(
        &shared::components::CharacterName,
        &mut shared::components::CharacterAffiliation,
    )>,
    kinds: Query<&shared::components::CharacterKind>,
    settlements: Query<(
        &shared::components::Settlement,
        &shared::components::PlayerPosition,
    )>,
    mut client_links: Query<(&RemoteId, &mut MessageReceiver<DevCommand>), With<ClientOf>>,
    mut warp: Query<&mut TimeWarp>,
    mut villager_seed: ResMut<VillagerSeed>,
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
                        // The display name off the profile, so the hero is
                        // listed under the name the player typed rather than the
                        // lowercased key.
                        profiles
                            .profiles
                            .get(&name_lower)
                            .map(|p| p.player_name.as_str())
                            .unwrap_or(name_lower.as_str()),
                        pos,
                        0.0,
                        outfit,
                    );
                    spawned_this_run.insert(remote_id.0);
                    info!("Dev: hero {entity:?} spawned for '{name_lower}' at {pos:?}");
                }
                DevCommand::SpawnNpc { pos } => {
                    if !pos.is_finite() {
                        continue;
                    }
                    let Some(terrain) = terrain.as_ref() else {
                        continue;
                    };
                    // The seed is a running counter, not a hash of the position:
                    // two villagers spawned on the same spot must not be the same
                    // person, and a villager must keep its name if the world is
                    // later rebuilt around it.
                    villager_seed.0 = villager_seed.0.wrapping_add(1);
                    let entity = crate::player::hero::spawn_villager(
                        &mut commands,
                        terrain,
                        villager_seed.0,
                        pos,
                    );
                    info!("Dev: villager {entity:?} spawned at {pos:?}");
                }
                DevCommand::SetAffiliation { character, banner } => {
                    // Reject out-of-range indices rather than storing one: a
                    // stored bad index renders as UNAFFILIATED and would look
                    // like the change silently failed.
                    if banner.is_some_and(|b| (b as usize) >= shared::names::BANNERS.len()) {
                        warn!("Dev: ignoring SetAffiliation with unknown banner {banner:?}");
                        continue;
                    }
                    let wanted = character.to_lowercase();
                    let mut hit = false;
                    for (name, mut affiliation) in named.iter_mut() {
                        if name.0.to_lowercase() != wanted {
                            continue;
                        }
                        hit = true;
                        let next = shared::components::CharacterAffiliation(banner);
                        // Change detection drives replication; an idle re-set
                        // must not re-send the component to every client.
                        if *affiliation != next {
                            *affiliation = next;
                        }
                        // STOP at the first match. Generated names are not
                        // unique -- the first duplicate appears around the
                        // fifty-first villager -- so without this one command
                        // re-flags every namesake in the world.
                        break;
                    }
                    if hit {
                        info!("Dev: '{character}' banner set to {banner:?}");
                    } else {
                        info!("Dev: no character named '{character}'");
                    }
                }
                DevCommand::FoundSettlement { pos, name } => {
                    if !pos.is_finite() {
                        continue;
                    }
                    let Some(terrain) = terrain.as_ref() else {
                        continue;
                    };
                    // Spacing is enforced HERE, not on the client: it is what
                    // stops the map being carpeted, so it cannot be advisory.
                    if let Some(existing) = settlements
                        .iter()
                        .find(|(_, p)| p.0.distance(pos) < MIN_SETTLEMENT_SPACING)
                    {
                        info!(
                            "Dev: too close to '{}' to found here ({:.0}m, need {MIN_SETTLEMENT_SPACING:.0}m)",
                            existing.0.name,
                            existing.1 .0.distance(pos)
                        );
                        continue;
                    }
                    // Dry land only. A hall founded in a lake would look
                    // fine and then never build anything, because every site
                    // its residents tried would be refused as underwater -- a
                    // silent failure that reads as "the village is broken".
                    let ground = terrain.get_height(pos.x, pos.z);
                    if terrain
                        .water_level()
                        .is_some_and(|level| ground < level + crate::world::village::FREEBOARD)
                    {
                        info!("Dev: cannot found a settlement in the water here");
                        continue;
                    }
                    let name = name.trim().to_string();
                    let name = if name.is_empty() {
                        // A generator only ever SUGGESTS; this is the fallback
                        // for an empty field, not the naming policy.
                        shared::names::place_name(
                            (pos.x as i64 as u64) ^ (pos.z as i64 as u64).rotate_left(17),
                        )
                    } else {
                        name
                    };
                    let grounded = Vec3::new(pos.x, ground, pos.z);
                    let entity = commands
                        .spawn((
                            shared::components::Settlement {
                                residents: 0,
                                treasury: 0,
                                name: name.clone(),
                                // Founding lands you at the bottom LIVING tier.
                                // Ruins is only ever reached by destruction.
                                tier: shared::components::SettlementTier::Hamlet,
                            },
                            shared::components::PlayerPosition(grounded),
                            // NO RegionCoord, deliberately. Region tagging is
                            // what opts an entity into interest management, and
                            // settlement summaries are the map screen
                            // (WORLD-DESIGN section 7) -- a place you have to
                            // stand next to before it appears on your map is not
                            // a map. Entities without the tag replicate to
                            // everyone, which is exactly what a summary wants.
                            lightyear::prelude::Replicate::to_clients(
                                lightyear::prelude::NetworkTarget::All,
                            ),
                        ))
                        .id();
                    info!("Dev: founded '{name}' ({entity:?}) at {grounded:?}");
                }
                DevCommand::SetRetinue { unit, commanded } => {
                    // Entity-targeted, because command must be exact and
                    // generated names collide.
                    if unit == Entity::PLACEHOLDER {
                        continue;
                    }
                    let Some(account) = profiles.peer_to_name.get(&remote_id.0).cloned() else {
                        continue;
                    };
                    // Only VILLAGERS can be conscripted. A hero is somebody's
                    // persisted body; taking one into a retinue would let god
                    // mode hand a player's character to another player.
                    let Ok(kind) = kinds.get(unit) else {
                        continue;
                    };
                    if *kind != shared::components::CharacterKind::Villager {
                        info!("Dev: refusing to conscript a hero");
                        continue;
                    }
                    if commanded {
                        commands
                            .entity(unit)
                            .insert(shared::components::CommandedBy(account.clone()));
                        info!("Dev: {unit:?} joined '{account}'s retinue");
                    } else {
                        commands
                            .entity(unit)
                            .remove::<shared::components::CommandedBy>();
                        // Dropping the order too: a dismissed villager should
                        // stop where it stands, not finish an errand for someone
                        // who no longer commands it.
                        commands.entity(unit).remove::<crate::player::hero::MoveTarget>();
                        info!("Dev: {unit:?} dismissed from '{account}'s retinue");
                    }
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
