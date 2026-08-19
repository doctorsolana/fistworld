//! Dev-mode gate for god commands.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::{MessageReceiver, MessageSender, RemoteId};

use shared::components::{settlement_founding_refusal, Hero, TimeWarp};
use shared::protocol::{DevCommand, DevStatus, GodAccessResult, ReliableChannel, RequestGodAccess};
use shared::spatial::SpatialObstacleGrid;
use shared::terrain::{world_pos_in_bounds, WorldTerrain};

use crate::collision::library::{DerivedColliderLibrary, StaticColliders};

/// Administrative configuration read once at startup.
///
/// `FISTWORLD_DEV=1` is the convenient local-development mode and grants every
/// connection. A hosted server leaves that off and supplies a long
/// `FISTWORLD_GOD_KEY`; individual connections must unlock through the J menu.
#[derive(Resource)]
pub struct DevMode {
    unrestricted: bool,
    access_key: Option<String>,
}

impl Default for DevMode {
    fn default() -> Self {
        let unrestricted = parse_dev_flag(std::env::var("FISTWORLD_DEV").ok());
        let access_key = std::env::var("FISTWORLD_GOD_KEY")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| value.len() >= 12);
        if unrestricted {
            info!("Dev mode: god commands enabled");
        } else if access_key.is_some() {
            info!("Hosted God Mode challenge enabled");
        } else if std::env::var("FISTWORLD_GOD_KEY").is_ok() {
            warn!("Ignoring FISTWORLD_GOD_KEY shorter than 12 characters");
        }
        Self {
            unrestricted,
            access_key,
        }
    }
}

impl DevMode {
    pub fn unrestricted(&self) -> bool {
        self.unrestricted
    }

    pub fn allows(&self, peer: lightyear::prelude::PeerId, sessions: &GodAccessSessions) -> bool {
        self.unrestricted || sessions.granted.contains(&peer)
    }

    fn key_matches(&self, candidate: &str) -> bool {
        let Some(expected) = self.access_key.as_deref() else {
            return false;
        };
        // Equal-length constant-work comparison avoids turning the challenge
        // endpoint into a useful byte-by-byte timing oracle.
        if expected.len() != candidate.len() {
            return false;
        }
        expected
            .as_bytes()
            .iter()
            .zip(candidate.as_bytes())
            .fold(0_u8, |difference, (a, b)| difference | (a ^ b))
            == 0
    }
}

/// God grants last only for the current network connection. They deliberately
/// do not become account/world state: reconnecting requires the key again.
#[derive(Resource, Default)]
pub struct GodAccessSessions {
    granted: bevy::platform::collections::HashSet<lightyear::prelude::PeerId>,
    failures: bevy::platform::collections::HashMap<lightyear::prelude::PeerId, u8>,
}

impl GodAccessSessions {
    pub fn remove(&mut self, peer: lightyear::prelude::PeerId) {
        self.granted.remove(&peer);
        self.failures.remove(&peer);
    }
}

/// Read-only authorization bundled as one system parameter so large dev
/// command handlers do not pay an extra top-level Bevy parameter slot.
#[derive(SystemParam)]
pub struct GodAccess<'w> {
    dev: Res<'w, DevMode>,
    sessions: Res<'w, GodAccessSessions>,
}

impl GodAccess<'_> {
    pub fn allows(&self, peer: lightyear::prelude::PeerId) -> bool {
        self.dev.allows(peer, &self.sessions)
    }
}

const MAX_GOD_ACCESS_FAILURES: u8 = 5;

/// Handle the deliberate hosted-server unlock separately from ordinary dev
/// commands. Failed attempts are bounded per connection to prevent a modified
/// client from brute-forcing the secret or flooding logs.
pub fn handle_god_access_requests(
    dev: Res<DevMode>,
    mut sessions: ResMut<GodAccessSessions>,
    mut clients: Query<
        (
            &RemoteId,
            &mut MessageReceiver<RequestGodAccess>,
            &mut MessageSender<GodAccessResult>,
            &mut MessageSender<DevStatus>,
        ),
        With<ClientOf>,
    >,
) {
    for (remote, mut receiver, mut result_sender, mut status_sender) in clients.iter_mut() {
        for request in receiver.receive() {
            let failures = sessions.failures.get(&remote.0).copied().unwrap_or(0);
            if failures >= MAX_GOD_ACCESS_FAILURES {
                result_sender.send::<ReliableChannel>(GodAccessResult {
                    granted: false,
                    message: "Too many attempts; reconnect before trying again".to_string(),
                });
                continue;
            }
            if dev.unrestricted() || dev.key_matches(request.key.trim()) {
                sessions.granted.insert(remote.0);
                sessions.failures.remove(&remote.0);
                status_sender.send::<ReliableChannel>(DevStatus { god: true });
                result_sender.send::<ReliableChannel>(GodAccessResult {
                    granted: true,
                    message: "God Mode unlocked for this connection".to_string(),
                });
                info!("Hosted God Mode unlocked for {:?}", remote.0);
            } else {
                let next = failures.saturating_add(1);
                sessions.failures.insert(remote.0, next);
                result_sender.send::<ReliableChannel>(GodAccessResult {
                    granted: false,
                    message: if dev.access_key.is_some() {
                        format!("Access key rejected ({next}/{MAX_GOD_ACCESS_FAILURES})")
                    } else {
                        "This server has no hosted God Mode key configured".to_string()
                    },
                });
                warn!("Rejected hosted God Mode attempt from {:?}", remote.0);
            }
        }
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
pub(crate) fn safe_villager_spawn_position(
    requested: Vec3,
    seed: u64,
    terrain: &WorldTerrain,
    obstacles: Option<&SpatialObstacleGrid>,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
) -> Option<Vec3> {
    const RING_STEP: f32 = 0.75;
    // The first 12 metres handle ordinary trees and cabin clicks. Continue
    // out to 48 metres for burst spawns beside a shoreline business: a whole
    // row can otherwise overlap the hut, its pier, water and nearby props and
    // fall back to the known-blocked requested point.
    const RINGS: usize = 64;
    const MIN_SAMPLES_PER_RING: usize = 16;
    const MAX_SAMPLES_PER_RING: usize = 96;
    const GOLDEN_ANGLE: f32 = 2.399_963_1;

    // A burst of villagers receives a small deterministic disc distribution,
    // preventing a hundred identical starts while keeping them close to the
    // player's click.
    let scatter_index = (seed % 97) as f32;
    let scatter_radius = scatter_index.sqrt() * 0.48;
    let scatter_angle = seed as f32 * GOLDEN_ANGLE;
    let centre = Vec2::new(requested.x, requested.z)
        + Vec2::new(scatter_angle.cos(), scatter_angle.sin()) * scatter_radius;
    let mut rejected = [0usize; 5];

    let mut validate = |point: Vec2| -> Option<Vec3> {
        if !world_pos_in_bounds(point.x, point.y) {
            rejected[0] += 1;
            return None;
        }
        let height = terrain.get_height(point.x, point.y);
        if terrain
            .water_surface_height(point.x, point.y)
            .is_some_and(|water| height < water + crate::world::village::FREEBOARD)
        {
            rejected[1] += 1;
            return None;
        }
        if 1.0 - terrain.get_normal(point.x, point.y).y.clamp(0.0, 1.0) > 0.24 {
            rejected[2] += 1;
            return None;
        }
        if obstacles.is_some_and(|grid| grid.point_blocked(point)) {
            rejected[3] += 1;
            return None;
        }
        if !crate::player::hero::navigation_segment_clear(point, point, None, colliders, derived) {
            rejected[4] += 1;
            return None;
        }
        Some(Vec3::new(point.x, height, point.y))
    };

    for ring in 0..=RINGS {
        // A fixed angular count leaves wider and wider holes between samples:
        // at 48 m the old sixteen-sample ring had almost 19 m arcs and could
        // jump across the narrow dry strip behind a fishing hut. Increase
        // angular resolution with radius while keeping a hard upper bound.
        let samples = if ring == 0 {
            1
        } else {
            (ring * 4).clamp(MIN_SAMPLES_PER_RING, MAX_SAMPLES_PER_RING)
        };
        for sample in 0..samples {
            let angle = scatter_angle
                + sample as f32 * std::f32::consts::TAU / samples as f32
                + ring as f32 * 0.31;
            let radius = ring as f32 * RING_STEP;
            let point = centre + Vec2::new(angle.cos(), angle.sin()) * radius;
            if let Some(position) = validate(point) {
                return Some(position);
            }
        }
    }

    // Polar samples are cheap and near-first, but even dense angular rings can
    // phase past a thin, irregular shoreline strip. A bounded square lattice
    // covers every 75 cm cell in the same 48 m search radius. This slower
    // fallback is only reached for hostile clicks (normally water or a dense
    // harbour) and guarantees that a narrow piece of dry ground is not missed
    // merely because none of the radial angles crossed it.
    for ring in 1..=RINGS as i32 {
        for x in -ring..=ring {
            for z in [-ring, ring] {
                let point = centre + Vec2::new(x as f32, z as f32) * RING_STEP;
                if let Some(position) = validate(point) {
                    return Some(position);
                }
            }
        }
        for z in (-ring + 1)..ring {
            for x in [-ring, ring] {
                let point = centre + Vec2::new(x as f32, z as f32) * RING_STEP;
                if let Some(position) = validate(point) {
                    return Some(position);
                }
            }
        }
    }

    // Never create a person at a point already proven unreachable. The caller
    // can report/refuse an exceptionally hostile water or obstacle click;
    // manufacturing an Idle villager inside it creates an immortal route
    // retry instead of honoring the spawn request.
    warn!(
        "No safe villager spawn within 48 metres of {requested:?}: rejected [bounds={}, water={}, slope={}, building={}, prop={}]",
        rejected[0], rejected[1], rejected[2], rejected[3], rejected[4]
    );
    #[cfg(test)]
    eprintln!(
        "LAB safe-spawn rejection at {requested:?}: bounds={} water={} slope={} building={} prop={}",
        rejected[0], rejected[1], rejected[2], rejected[3], rejected[4]
    );
    None
}

pub fn handle_dev_commands(
    mut commands: Commands,
    access: GodAccess,
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
            if !access.allows(remote_id.0) {
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
                        shared::components::Health::default(),
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
                    let Some(safe_position) = safe_villager_spawn_position(
                        pos,
                        villager_seed.0,
                        terrain,
                        obstacles.as_deref(),
                        colliders.as_deref(),
                        derived.as_deref(),
                    ) else {
                        warn!(
                            "Dev: refusing villager spawn at {pos:?}: no navigable ground within 48 metres"
                        );
                        continue;
                    };
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
                            shared::economy::GoodsInventory::new_partitioned(
                                shared::components::SettlementBuildingKind::Hall
                                    .storage_bulk_capacity(),
                            ),
                            shared::economy::MootMarket::founding(),
                            shared::components::SettlementPolicies::from_foundation(
                                &name, grounded,
                            ),
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
    fn hosted_key_requires_an_exact_complete_match() {
        let dev = DevMode {
            unrestricted: false,
            access_key: Some("a-long-hosted-key".to_string()),
        };
        assert!(dev.key_matches("a-long-hosted-key"));
        assert!(!dev.key_matches("a-long-hosted-ke"));
        assert!(!dev.key_matches("a-long-hosted-key!"));
        assert!(!dev.key_matches("A-long-hosted-key"));
    }

    #[test]
    fn an_unconfigured_hosted_server_rejects_every_challenge() {
        let dev = DevMode {
            unrestricted: false,
            access_key: None,
        };
        assert!(!dev.key_matches("a-long-hosted-key"));
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
            safe_villager_spawn_position(requested, 97, &terrain, Some(&obstacles), None, None)
                .expect("the blocked click has nearby navigable ground");

        assert!(safe.is_finite());
        assert!(!obstacles.point_blocked(Vec2::new(safe.x, safe.z)));
        assert!(safe.distance(requested) > 5.0);
    }

    #[test]
    fn burst_spawn_search_reaches_clear_ground_beyond_the_old_twelve_metre_limit() {
        let terrain = WorldTerrain::default();
        let requested = Vec3::new(1_700.0, terrain.get_height(1_700.0, 0.0), 0.0);
        let mut obstacles = SpatialObstacleGrid::default();
        obstacles.insert(ObstacleEntry {
            center: Vec2::new(requested.x, requested.z),
            half_extents: Vec2::splat(14.0),
            rotation: 0.0,
            obstacle_type: 1,
        });

        let safe =
            safe_villager_spawn_position(requested, 97, &terrain, Some(&obstacles), None, None)
                .expect("the extended search must find navigable ground");

        assert!(safe.is_finite());
        assert!(!obstacles.point_blocked(Vec2::new(safe.x, safe.z)));
        assert!(safe.distance(requested) > 14.0);
    }
}
