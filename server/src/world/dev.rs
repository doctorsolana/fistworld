//! Dev-mode gate for god commands.

use bevy::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::{MessageReceiver, RemoteId};

use shared::components::{settlement_founding_refusal, Hero, TimeWarp};
use shared::protocol::DevCommand;
use shared::spatial::SpatialObstacleGrid;
use shared::terrain::{world_pos_in_bounds, WorldTerrain};

use crate::collision::library::{DerivedColliderLibrary, StaticColliders};

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

/// God-mode clicks can land inside a tree, a completed building, water, or on
/// a crowd's exact shared point. Starting inside a static collider makes the
/// route planner correctly reject every migration route, which looked like an
/// AI decision failure. Scatter the requested point slightly and choose the
/// nearest genuinely navigable sample before the villager exists.
fn safe_villager_spawn_position(
    requested: Vec3,
    seed: u64,
    terrain: &WorldTerrain,
    obstacles: Option<&SpatialObstacleGrid>,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
) -> Vec3 {
    const RING_STEP: f32 = 0.75;
    const RINGS: usize = 16;
    const SAMPLES_PER_RING: usize = 16;
    const GOLDEN_ANGLE: f32 = 2.399_963_1;

    // A burst of villagers receives a small deterministic disc distribution,
    // preventing a hundred identical starts while keeping them close to the
    // player's click.
    let scatter_index = (seed % 97) as f32;
    let scatter_radius = scatter_index.sqrt() * 0.48;
    let scatter_angle = seed as f32 * GOLDEN_ANGLE;
    let centre = Vec2::new(requested.x, requested.z)
        + Vec2::new(scatter_angle.cos(), scatter_angle.sin()) * scatter_radius;

    for ring in 0..=RINGS {
        let samples = if ring == 0 { 1 } else { SAMPLES_PER_RING };
        for sample in 0..samples {
            let angle = scatter_angle
                + sample as f32 * std::f32::consts::TAU / samples as f32
                + ring as f32 * 0.31;
            let radius = ring as f32 * RING_STEP;
            let point = centre + Vec2::new(angle.cos(), angle.sin()) * radius;
            if !world_pos_in_bounds(point.x, point.y) {
                continue;
            }
            let height = terrain.get_height(point.x, point.y);
            if terrain
                .water_surface_height(point.x, point.y)
                .is_some_and(|water| height < water + crate::world::village::FREEBOARD)
            {
                continue;
            }
            if 1.0 - terrain.get_normal(point.x, point.y).y.clamp(0.0, 1.0) > 0.24 {
                continue;
            }
            if !crate::player::hero::navigation_segment_clear(
                point, point, obstacles, colliders, derived,
            ) {
                continue;
            }
            return Vec3::new(point.x, height, point.y);
        }
    }

    // Preserve the old finite, terrain-grounded behavior if an exceptionally
    // hostile click has no safe sample nearby. The migration state machine
    // will report its route failure instead of silently deleting the person.
    Vec3::new(
        requested.x,
        terrain.get_height(requested.x, requested.z),
        requested.z,
    )
}

pub fn handle_dev_commands(
    mut commands: Commands,
    dev: Res<DevMode>,
    terrain: Option<Res<WorldTerrain>>,
    obstacles: Option<Res<SpatialObstacleGrid>>,
    colliders: Option<Res<StaticColliders>>,
    derived: Option<Res<DerivedColliderLibrary>>,
    profiles: Res<crate::persistence::profiles::PlayerProfiles>,
    mut hero_index: ResMut<crate::player::hero::HeroIndex>,
    heroes: Query<&Hero>,
    mut named: Query<(
        &shared::components::PersonId,
        &mut shared::components::CharacterAffiliation,
    )>,
    kinds: Query<(
        Entity,
        &shared::components::PersonId,
        &shared::components::CharacterKind,
    )>,
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
                        info!(
                            "Dev: ignoring SpawnHero from {:?}: hero exists",
                            remote_id.0
                        );
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
                        profiles
                            .profiles
                            .get(&name_lower)
                            .map(|profile| profile.character_attributes())
                            .unwrap_or_default(),
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
                    let safe_position = safe_villager_spawn_position(
                        pos,
                        villager_seed.0,
                        terrain,
                        obstacles.as_deref(),
                        colliders.as_deref(),
                        derived.as_deref(),
                    );
                    let entity = crate::player::hero::spawn_villager(
                        &mut commands,
                        terrain,
                        villager_seed.0,
                        safe_position,
                    );
                    info!(
                        "Dev: villager {entity:?} spawned at {safe_position:?} (requested {pos:?})"
                    );
                }
                DevCommand::SetAffiliation { person, banner } => {
                    // Reject out-of-range indices rather than storing one: a
                    // stored bad index renders as UNAFFILIATED and would look
                    // like the change silently failed.
                    if banner.is_some_and(|b| (b as usize) >= shared::names::BANNERS.len()) {
                        warn!("Dev: ignoring SetAffiliation with unknown banner {banner:?}");
                        continue;
                    }
                    let mut hit = false;
                    for (person_id, mut affiliation) in named.iter_mut() {
                        if *person_id != person {
                            continue;
                        }
                        hit = true;
                        let next = shared::components::CharacterAffiliation(banner);
                        // Change detection drives replication; an idle re-set
                        // must not re-send the component to every client.
                        if *affiliation != next {
                            *affiliation = next;
                        }
                        break;
                    }
                    if hit {
                        info!("Dev: person {} banner set to {banner:?}", person.0);
                    } else {
                        info!("Dev: no character with id {}", person.0);
                    }
                }
                DevCommand::FoundSettlement { pos, name } => {
                    if !pos.is_finite() {
                        continue;
                    }
                    let Some(terrain) = terrain.as_ref() else {
                        continue;
                    };
                    let ground = terrain.get_height(pos.x, pos.z);
                    let grounded = Vec3::new(pos.x, ground, pos.z);
                    let nearest = settlements
                        .iter()
                        .map(|(settlement, position)| {
                            (settlement.name.as_str(), position.0.distance(pos))
                        })
                        .min_by(|a, b| a.1.total_cmp(&b.1));
                    // The server remains authoritative. The identical shared
                    // footprint test also drives the client's placement hint,
                    // so a dry centre with a wet hall corner cannot slip
                    // through either side of the network boundary.
                    if let Some(reason) = settlement_founding_refusal(terrain, grounded, nearest) {
                        info!("Dev: cannot found a settlement here: {reason}");
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
                    let entity = commands
                        .spawn((
                            shared::components::Settlement {
                                residents: 0,
                                treasury: shared::economy::STARTING_TREASURY_MONEY,
                                name: name.clone(),
                                // Founding lands you at the bottom LIVING tier.
                                // Ruins is only ever reached by destruction.
                                tier: shared::components::SettlementTier::Hamlet,
                            },
                            // At hamlet tier the moot hall is also the common
                            // store. This is physical stock, not a claim that
                            // the hall bought the goods or minted the payment.
                            shared::economy::GoodsInventory::new(
                                shared::components::SettlementBuildingKind::Hall
                                    .storage_bulk_capacity(),
                            ),
                            shared::economy::MootMarket::founding(),
                            shared::components::SettlementPolicies::default(),
                            shared::components::PlayerPosition(grounded),
                            // The shared village schedule tags the physical hall
                            // with RegionCoord before network visibility is
                            // applied. Its tiny SettlementSummary is the global
                            // map/encyclopedia record.
                            lightyear::prelude::Replicate::to_clients(
                                lightyear::prelude::NetworkTarget::All,
                            ),
                        ))
                        .id();
                    info!("Dev: founded '{name}' ({entity:?}) at {grounded:?}");
                }
                DevCommand::SetRetinue { person, commanded } => {
                    if !person.is_assigned() {
                        continue;
                    }
                    let Some(account) = profiles.peer_to_name.get(&remote_id.0).cloned() else {
                        continue;
                    };
                    // Only VILLAGERS can be conscripted. A hero is somebody's
                    // persisted body; taking one into a retinue would let god
                    // mode hand a player's character to another player.
                    let Some((unit, _, kind)) = kinds.iter().find(|(_, id, _)| **id == person)
                    else {
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
                        commands
                            .entity(unit)
                            .remove::<crate::player::hero::MoveTarget>();
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
    use shared::spatial::ObstacleEntry;

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

    #[test]
    fn god_spawn_moves_a_villager_out_of_a_blocked_click() {
        let terrain = WorldTerrain::default();
        let requested = Vec3::new(1_700.0, terrain.get_height(1_700.0, 0.0), 0.0);
        let mut obstacles = SpatialObstacleGrid::default();
        obstacles.insert(ObstacleEntry {
            center: Vec2::new(requested.x, requested.z),
            half_extents: Vec2::splat(6.0),
            rotation: 0.0,
            obstacle_type: 1,
        });

        let safe =
            safe_villager_spawn_position(requested, 97, &terrain, Some(&obstacles), None, None);

        assert!(safe.is_finite());
        assert!(!obstacles.point_blocked(Vec2::new(safe.x, safe.z)));
        assert!(safe.distance(requested) > 5.0);
    }
}
