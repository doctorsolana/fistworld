//! Region registry and per-client network interest.
//!
//! Observation determines replication only. The authoritative world schedule
//! advances the same actors and activities everywhere, including with no clients.

use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use lightyear::prelude::*;

use shared::components::{
    CommandedBy, Hero, ImmigrantArrivalBoat, Player, PlayerBoat, PlayerPosition,
};
use shared::region::{REGION_SIZE, RegionCoord, view_radius_to_rings};
use shared::terrain::WorldTerrain;

use crate::net::input::ClientInputs;

/// Extra rings kept visible beyond a client's view radius before dropping them.
///
/// Hysteresis: entities on an interest boundary would otherwise gain and lose visibility
/// every tick as the camera jitters, spawning and despawning on the client repeatedly.
const INTEREST_EXIT_MARGIN_RINGS: i32 = 1;

/// Network-interest evidence; never a gameplay admission or speed control.
#[derive(Debug, Clone, Default)]
pub struct RegionState {
    pub observers: u32,
}

impl RegionState {
    fn new() -> Self {
        Self::default()
    }
}

/// All regions in the world.
///
/// Populated once from map bounds so network interest can address places nobody
/// has visited. Gameplay does not depend on this registry or its observer counts.
#[derive(Resource, Default)]
pub struct RegionRegistry {
    regions: HashMap<RegionCoord, RegionState>,
}

impl RegionRegistry {
    pub fn len(&self) -> usize {
        self.regions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }

    pub fn get(&self, coord: RegionCoord) -> Option<&RegionState> {
        self.regions.get(&coord)
    }

    pub fn contains(&self, coord: RegionCoord) -> bool {
        self.regions.contains_key(&coord)
    }

    /// Number of regions currently included in at least one client's interest.
    pub fn observed_count(&self) -> usize {
        self.regions
            .values()
            .filter(|region| region.observers > 0)
            .count()
    }

    #[cfg(test)]
    pub(crate) fn set_observers_for_test(&mut self, coord: RegionCoord, observers: u32) {
        self.regions.insert(coord, RegionState { observers });
    }
}

/// Regions each connected client is interested in.
///
/// Keyed by the client's link entity (the one carrying `ReplicationSender`), which is
/// what lightyear's visibility API expects — not the peer id and not the commander
/// entity.
#[derive(Resource, Default)]
pub struct ClientInterest {
    by_client: HashMap<Entity, HashSet<RegionCoord>>,
}

impl ClientInterest {
    #[cfg(test)]
    pub fn is_interested(&self, client: Entity, coord: RegionCoord) -> bool {
        self.by_client
            .get(&client)
            .is_some_and(|set| set.contains(&coord))
    }
}

/// Build the region grid covering the loaded map.
pub fn build_region_registry(mut registry: ResMut<RegionRegistry>, terrain: Res<WorldTerrain>) {
    if !registry.is_empty() {
        return;
    }

    let bounds = terrain.generator.active_map_bounds();
    let min = RegionCoord::from_world_pos(Vec3::new(bounds.min[0], 0.0, bounds.min[1]));
    let max = RegionCoord::from_world_pos(Vec3::new(bounds.max[0], 0.0, bounds.max[1]));

    for z in min.z..=max.z {
        for x in min.x..=max.x {
            let coord = RegionCoord::new(x, z);
            registry.regions.insert(coord, RegionState::new());
        }
    }

    info!(
        "Regions: {} ({}x{} at {}m) covering {:.0}x{:.0}m",
        registry.len(),
        max.x - min.x + 1,
        max.z - min.z + 1,
        REGION_SIZE,
        bounds.max[0] - bounds.min[0],
        bounds.max[1] - bounds.min[1],
    );
}

/// Recompute which regions each client cares about, from its commander view.
///
/// The radius comes from the client's reported view radius (how far the camera can see at
/// its current zoom), so zooming out widens interest instead of leaving the world empty.
pub fn update_client_interest(
    mut interest: ResMut<ClientInterest>,
    registry: Res<RegionRegistry>,
    inputs: Res<ClientInputs>,
    commanders: Query<(&Player, &PlayerPosition, &ControlledBy)>,
    links: Query<Entity, With<lightyear::prelude::server::ClientOf>>,
    mut cached_keys: Local<Vec<(Entity, RegionCoord, i32)>>,
    mut cached_links: Local<Vec<Entity>>,
) {
    // Interest only changes when a commander's center region or ring count
    // does (or clients come/go, or the registry changes). Rebuilding a
    // ~4k-entry set per client at 60Hz for a static camera was pure waste —
    // and the unconditional rebuild also dirtied the resource every tick,
    // defeating any change-gating downstream.
    //
    // The key is computed ONCE and reused to build the sets below. It used to be
    // recomputed in a second pass, which meant the cache key and the thing it
    // claimed to describe could silently drift apart.
    let mut keys: Vec<(Entity, RegionCoord, i32)> = Vec::with_capacity(commanders.iter().len());
    for (player, position, controlled_by) in commanders.iter() {
        let rings =
            view_radius_to_rings(inputs.latest.get(&player.client_id).map(|i| i.view_radius));
        keys.push((
            controlled_by.owner,
            RegionCoord::from_world_pos(position.0),
            rings,
        ));
    }
    keys.sort_unstable_by_key(|(entity, _, _)| *entity);

    // Connected-but-unnamed clients must be represented too (see below), so a
    // client connecting or disconnecting has to invalidate the cache even when
    // no commander moved.
    let mut link_entities: Vec<Entity> = links.iter().collect();
    link_entities.sort_unstable();

    let dirty = *cached_keys != keys || registry.is_changed() || *cached_links != link_entities;
    if !dirty {
        return;
    }
    *cached_keys = keys.clone();
    *cached_links = link_entities.clone();

    interest.by_client.clear();

    // Seed EVERY connected link with an empty set before filling in commanders.
    // A client gets its `ReplicationSender` at connect but its commander only
    // after it submits a name, so between those two events it had no entry at
    // all — and `apply_region_visibility` only iterates entries, so lightyear's
    // visible-by-default left it receiving every region-tagged entity in the
    // world. An empty set means "interested in nothing", which is correct.
    for link in link_entities {
        interest.by_client.insert(link, HashSet::new());
    }

    for (client, center, rings) in keys {
        let set: HashSet<RegionCoord> = center
            .in_radius(rings)
            .into_iter()
            .filter(|coord| registry.contains(*coord))
            .collect();
        interest.by_client.insert(client, set);
    }
}

/// Apply per-client visibility to region-tagged replicated entities.
///
/// Replaces blanket `NetworkTarget::All` replication: an entity is only sent to clients
/// whose interest covers its region. Entities without a `RegionCoord` are unaffected and
/// keep replicating globally, which is what world-level state (time, map state) wants.
///
/// lightyear 0.28 inverted the defaults: a replicated entity is visible to every client
/// until `lose_visibility` is called, and per-pair visibility can no longer be read back
/// from `ReplicationState`. So this system keeps its own record of the last state it
/// applied per (entity, sender) pair. The first time a pair is seen — entity just
/// spawned, or client just connected — visibility is set explicitly in both directions,
/// otherwise an uninterested client would receive the entity until it first crossed an
/// interest boundary.
fn entity_is_owned_by_peer(
    peer: PeerId,
    account: Option<&str>,
    hero: Option<&Hero>,
    commanded_by: Option<&CommandedBy>,
) -> bool {
    // Anything sworn to this account - the hero itself, the starter boat,
    // and every retinue villager - stays replicated to its commander no
    // matter where the camera looks: your own people must never wink out of
    // your clan roster, and LOCATE must always know where they are.
    hero.is_some_and(|hero| hero.owner == peer)
        || account
            .zip(commanded_by)
            .is_some_and(|(account, owner)| account == owner.0.as_str())
}

fn enters_region_interest(
    immigrant_arrival: bool,
    owned_by_client: bool,
    regions: &HashSet<RegionCoord>,
    coord: RegionCoord,
) -> bool {
    immigrant_arrival || owned_by_client || regions.contains(&coord)
}

pub fn apply_region_visibility(
    mut commands: Commands,
    interest: Res<ClientInterest>,
    profiles: Res<crate::persistence::profiles::PlayerProfiles>,
    links: Query<&RemoteId, With<lightyear::prelude::server::ClientOf>>,
    replicated: Query<
        (
            Entity,
            &RegionCoord,
            Option<&Hero>,
            Option<&CommandedBy>,
            Has<PlayerBoat>,
            Has<ImmigrantArrivalBoat>,
        ),
        With<Replicate>,
    >,
    mut applied: Local<HashMap<Entity, HashMap<Entity, bool>>>,
) {
    // Drop state for despawned entities and disconnected clients so the record cannot
    // grow without bound (and so a reconnecting client is treated as a fresh pair).
    applied.retain(|entity, _| replicated.contains(*entity));
    for senders in applied.values_mut() {
        senders.retain(|sender, _| interest.by_client.contains_key(sender));
    }

    for (entity, coord, hero, commanded_by, _player_boat, immigrant_arrival) in replicated.iter() {
        let entity_state = applied.entry(entity).or_default();

        for (client, regions) in interest.by_client.iter() {
            // Camera interest must never hide a player's own embodied character
            // or starter vessel from them. In particular, a new voyage begins
            // at the map edge while the pre-cinematic camera is still at the
            // map spawn. Requiring the camera to move before replicating the
            // entities that move it creates a permanent bootstrap deadlock.
            let owned_by_client = links.get(*client).is_ok_and(|remote| {
                entity_is_owned_by_peer(
                    remote.0,
                    profiles.peer_to_name.get(&remote.0).map(String::as_str),
                    hero,
                    commanded_by,
                )
            });
            // A lab observer cannot move its camera to a randomized map-edge
            // arrival until it knows where that boat is. Natural dinghies are
            // globally visible for this bootstrap; their hard server cap of
            // eight keeps this trivial, while passengers and all ordinary
            // region entities retain normal interest filtering.
            let inside =
                enters_region_interest(immigrant_arrival, owned_by_client, regions, *coord);

            let should_be_visible = match entity_state.get(client) {
                // First sighting of this (entity, sender) pair: set both states
                // explicitly to override the visible-by-default spawn state.
                None => inside,
                // Widen the boundary for entities already visible so a camera hovering
                // on a region edge does not thrash spawn/despawn on the client.
                Some(true) => {
                    immigrant_arrival
                        || owned_by_client
                        || regions
                            .iter()
                            .any(|r| r.ring_distance(*coord) <= INTEREST_EXIT_MARGIN_RINGS)
                }
                Some(false) => inside,
            };

            let changed = entity_state.get(client) != Some(&should_be_visible);
            if changed {
                if should_be_visible {
                    commands.gain_visibility(entity, *client);
                } else {
                    commands.lose_visibility(entity, *client);
                }
                entity_state.insert(*client, should_be_visible);
            }
        }
    }
}

/// Update diagnostics about network interest without changing simulation.
pub fn update_region_observers(
    mut registry: ResMut<RegionRegistry>,
    interest: Res<ClientInterest>,
) {
    if !interest.is_changed() {
        return;
    }
    // Interest counts cannot dirty the registry's structural cache and cause
    // update_client_interest to rebuild all sets on the next fixed tick.
    let registry = registry.bypass_change_detection();
    for region in registry.regions.values_mut() {
        region.observers = 0;
    }
    for regions in interest.by_client.values() {
        for coord in regions {
            if let Some(region) = registry.regions.get_mut(coord) {
                region.observers += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry_with(coords: &[RegionCoord]) -> RegionRegistry {
        let mut registry = RegionRegistry::default();
        for coord in coords {
            registry.regions.insert(*coord, RegionState::new());
        }
        registry
    }

    #[test]
    fn observation_counts_follow_interest_and_clear_after_disconnect() {
        let watched = RegionCoord::new(0, 0);
        let empty = RegionCoord::new(9, 9);
        let mut app = App::new();
        app.insert_resource(registry_with(&[watched, empty]))
            .init_resource::<ClientInterest>()
            .add_systems(Update, update_region_observers);
        app.world_mut()
            .resource_mut::<ClientInterest>()
            .by_client
            .insert(
                Entity::from_raw_u32(1).unwrap(),
                HashSet::from_iter([watched]),
            );
        app.update();
        let registry = app.world().resource::<RegionRegistry>();
        assert_eq!(registry.get(watched).unwrap().observers, 1);
        assert_eq!(registry.get(empty).unwrap().observers, 0);
        assert_eq!(registry.observed_count(), 1);
        app.world_mut()
            .resource_mut::<ClientInterest>()
            .by_client
            .clear();
        app.update();
        assert_eq!(app.world().resource::<RegionRegistry>().observed_count(), 0);
    }

    #[test]
    fn unchanged_commander_views_do_not_rebuild_interest_without_transport_links() {
        #[derive(Resource, Default)]
        struct Writes(u32);
        fn count(interest: Res<ClientInterest>, mut writes: ResMut<Writes>) {
            if interest.is_changed() {
                writes.0 += 1;
            }
        }
        let mut app = App::new();
        app.insert_resource(registry_with(&[RegionCoord::new(0, 0)]))
            .init_resource::<ClientInputs>()
            .init_resource::<ClientInterest>()
            .init_resource::<Writes>()
            .add_systems(Update, (update_client_interest, count).chain());
        let owner = app.world_mut().spawn_empty().id();
        let commander = app
            .world_mut()
            .spawn((
                Player {
                    client_id: PeerId::Local(17),
                },
                PlayerPosition(Vec3::ZERO),
                ControlledBy {
                    owner,
                    lifetime: Lifetime::default(),
                },
            ))
            .id();
        app.update();
        app.update();
        assert_eq!(app.world().resource::<Writes>().0, 1);
        app.world_mut()
            .get_mut::<PlayerPosition>(commander)
            .unwrap()
            .0
            .x += REGION_SIZE;
        app.update();
        assert_eq!(app.world().resource::<Writes>().0, 2);
        app.world_mut().despawn(commander);
        app.update();
        assert!(
            app.world()
                .resource::<ClientInterest>()
                .by_client
                .is_empty()
        );
    }

    #[test]
    fn interest_lookup_is_per_client() {
        let mut interest = ClientInterest::default();
        let a = Entity::from_raw_u32(1).unwrap();
        let b = Entity::from_raw_u32(2).unwrap();
        interest
            .by_client
            .insert(a, HashSet::from_iter([RegionCoord::new(0, 0)]));
        interest
            .by_client
            .insert(b, HashSet::from_iter([RegionCoord::new(5, 5)]));

        assert!(interest.is_interested(a, RegionCoord::new(0, 0)));
        assert!(!interest.is_interested(a, RegionCoord::new(5, 5)));
        assert!(interest.is_interested(b, RegionCoord::new(5, 5)));
    }

    #[test]
    fn hysteresis_margin_keeps_edge_entities_visible() {
        // An entity one ring outside the interest set stays visible once it already is,
        // which is what stops spawn/despawn thrash on a region boundary.
        let inside = RegionCoord::new(0, 0);
        let just_outside = RegionCoord::new(1, 0);
        let far = RegionCoord::new(4, 0);
        let regions: HashSet<RegionCoord> = HashSet::from_iter([inside]);

        let within_margin = |coord: RegionCoord| {
            regions
                .iter()
                .any(|r: &RegionCoord| r.ring_distance(coord) <= INTEREST_EXIT_MARGIN_RINGS)
        };

        assert!(within_margin(just_outside));
        assert!(!within_margin(far));
    }

    #[test]
    fn owned_hero_boat_and_retinue_bypass_camera_region_interest() {
        let peer = PeerId::Netcode(77);
        let other = PeerId::Netcode(88);
        let hero = Hero { owner: peer };
        let owner = CommandedBy("hilda".to_string());

        assert!(entity_is_owned_by_peer(
            peer,
            Some("hilda"),
            Some(&hero),
            None
        ));
        assert!(!entity_is_owned_by_peer(
            other,
            Some("alwin"),
            Some(&hero),
            None,
        ));
        // Any commanded character - boat or retinue villager - belongs to its
        // commander's interest, and to nobody else's.
        assert!(entity_is_owned_by_peer(
            peer,
            Some("hilda"),
            None,
            Some(&owner),
        ));
        assert!(!entity_is_owned_by_peer(
            peer,
            Some("alwin"),
            None,
            Some(&owner),
        ));
    }

    #[test]
    fn immigrant_dinghy_bootstraps_visibility_outside_the_camera_region() {
        let watched = HashSet::from_iter([RegionCoord::new(0, 0)]);
        let map_edge = RegionCoord::new(20, -20);

        assert!(enters_region_interest(true, false, &watched, map_edge));
        assert!(!enters_region_interest(false, false, &watched, map_edge));
    }
}
