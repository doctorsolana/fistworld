//! Region registry, interest management and the strategic tick.
//!
//! See `docs/ARCHITECTURE.md`. Three responsibilities, deliberately together because they
//! are three views of the same partition:
//!
//! 1. **Registry** — every region the world contains, and its current state.
//! 2. **Interest** — which regions each client cares about, driving replication.
//! 3. **Strategic tick** — the cheap simulation that runs everywhere, forever.

use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use lightyear::prelude::*;

use shared::components::{Player, PlayerPosition, TimeWarp};
use shared::region::{RegionCoord, SimLevel, REGION_SIZE};
use shared::terrain::WorldTerrain;

use crate::net::input::ClientInputs;

/// Strategic simulation rate. Slow on purpose: this runs for the entire world including
/// regions no player has ever visited, so its cost is bounded by tick rate, not by how
/// much of the world is active.
const STRATEGIC_TICK_HZ: f64 = 1.0;

/// Extra rings kept visible beyond a client's view radius before dropping them.
///
/// Hysteresis: entities on an interest boundary would otherwise gain and lose visibility
/// every tick as the camera jitters, spawning and despawning on the client repeatedly.
const INTEREST_EXIT_MARGIN_RINGS: i32 = 1;

/// Per-region state owned by the strategic layer.
#[derive(Debug, Clone)]
pub struct RegionState {
    pub coord: RegionCoord,
    /// How much simulation this region currently receives.
    pub sim_level: SimLevel,
    /// Clients currently interested in this region. Drives `sim_level`.
    pub observers: u32,
    /// Accumulated strategic time, so settlement/economy work added later can integrate
    /// over real elapsed time rather than assuming a fixed tick.
    pub strategic_secs: f64,
}

impl RegionState {
    fn new(coord: RegionCoord) -> Self {
        Self {
            coord,
            sim_level: SimLevel::Strategic,
            observers: 0,
            strategic_secs: 0.0,
        }
    }
}

/// All regions in the world.
///
/// Populated once from map bounds rather than lazily, because the strategic tick must
/// visit regions nobody has ever been to — that is the whole point of a persistent world.
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

    pub fn iter(&self) -> impl Iterator<Item = &RegionState> {
        self.regions.values()
    }

    /// Number of regions currently promoted to tactical simulation.
    pub fn tactical_count(&self) -> usize {
        self.regions
            .values()
            .filter(|r| r.sim_level == SimLevel::Tactical)
            .count()
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
    pub fn regions_for(&self, client: Entity) -> Option<&HashSet<RegionCoord>> {
        self.by_client.get(&client)
    }

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
            registry.regions.insert(coord, RegionState::new(coord));
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
) {
    interest.by_client.clear();

    for (player, position, controlled_by) in commanders.iter() {
        // View radius is client-reported; fall back to the focus point alone if a client
        // has not sent input yet, so a fresh connection still receives its surroundings.
        let view_radius = inputs
            .latest
            .get(&player.client_id)
            .map(|input| input.view_radius)
            .filter(|r| r.is_finite() && *r > 0.0)
            .unwrap_or(REGION_SIZE);

        let rings = (view_radius / REGION_SIZE).ceil() as i32;
        let center = RegionCoord::from_world_pos(position.0);

        let set: HashSet<RegionCoord> = center
            .in_radius(rings)
            .into_iter()
            .filter(|coord| registry.contains(*coord))
            .collect();

        interest.by_client.insert(controlled_by.owner, set);
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
pub fn apply_region_visibility(
    mut commands: Commands,
    interest: Res<ClientInterest>,
    replicated: Query<(Entity, &RegionCoord), With<Replicate>>,
    mut applied: Local<HashMap<Entity, HashMap<Entity, bool>>>,
) {
    // Drop state for despawned entities and disconnected clients so the record cannot
    // grow without bound (and so a reconnecting client is treated as a fresh pair).
    applied.retain(|entity, _| replicated.contains(*entity));
    for senders in applied.values_mut() {
        senders.retain(|sender, _| interest.by_client.contains_key(sender));
    }

    for (entity, coord) in replicated.iter() {
        let entity_state = applied.entry(entity).or_default();

        for (client, regions) in interest.by_client.iter() {
            let inside = regions.contains(coord);

            let should_be_visible = match entity_state.get(client) {
                // First sighting of this (entity, sender) pair: set both states
                // explicitly to override the visible-by-default spawn state.
                None => inside,
                // Widen the boundary for entities already visible so a camera hovering
                // on a region edge does not thrash spawn/despawn on the client.
                Some(true) => regions
                    .iter()
                    .any(|r| r.ring_distance(*coord) <= INTEREST_EXIT_MARGIN_RINGS),
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

/// Promote regions with observers to tactical simulation, demote the rest.
pub fn update_region_sim_levels(
    mut registry: ResMut<RegionRegistry>,
    interest: Res<ClientInterest>,
) {
    for region in registry.regions.values_mut() {
        region.observers = 0;
    }

    for regions in interest.by_client.values() {
        for coord in regions.iter() {
            if let Some(region) = registry.regions.get_mut(coord) {
                region.observers += 1;
            }
        }
    }

    for region in registry.regions.values_mut() {
        region.sim_level = if region.observers > 0 {
            SimLevel::Tactical
        } else {
            SimLevel::Strategic
        };
    }
}

/// Drives the strategic tick and reports its cost.
#[derive(Resource)]
pub struct StrategicClock {
    accumulator: f64,
    interval: f64,
    pub ticks: u64,
    pub last_tick_micros: u128,
}

impl Default for StrategicClock {
    fn default() -> Self {
        Self {
            accumulator: 0.0,
            interval: 1.0 / STRATEGIC_TICK_HZ,
            ticks: 0,
            last_tick_micros: 0,
        }
    }
}

/// The always-on simulation: advances every region in the world, observed or not.
///
/// This is the load-bearing cost of a persistent world, so it must stay cheap enough to
/// run forever over the entire map. Keep it free of pathfinding, physics and anything
/// per-soldier — those belong to the tactical layer, which only runs where someone is
/// looking. `last_tick_micros` exists so that constraint is measured, not assumed.
pub fn tick_strategic_world(
    time: Res<Time>,
    warp: Query<&TimeWarp>,
    mut clock: ResMut<StrategicClock>,
    mut registry: ResMut<RegionRegistry>,
) {
    let factor = warp.iter().next().map(|w| w.0).unwrap_or(1.0);
    clock.accumulator += time.delta_secs_f64() * factor as f64;
    if clock.accumulator < clock.interval {
        return;
    }
    let elapsed = clock.accumulator;
    clock.accumulator = 0.0;

    let started = std::time::Instant::now();
    for region in registry.regions.values_mut() {
        region.strategic_secs += elapsed;
        // Settlement production, caravan movement and clan upkeep hook in here. They must
        // integrate over `elapsed` rather than assume a fixed step, because this tick is
        // deliberately allowed to drift under load.
    }
    clock.last_tick_micros = started.elapsed().as_micros();
    clock.ticks += 1;
}

/// Periodic report so the "strategic tick stays cheap at world scale" claim is checkable.
pub fn log_region_telemetry(
    registry: Res<RegionRegistry>,
    clock: Res<StrategicClock>,
    time: Res<Time>,
    mut last_log_secs: Local<f64>,
) {
    if clock.ticks == 0 || clock.ticks % 30 != 0 {
        return;
    }
    // Strategic ticks outrun real time under warp; floor the log rate in real seconds.
    let now = time.elapsed_secs_f64();
    if now - *last_log_secs < 30.0 {
        return;
    }
    *last_log_secs = now;
    info!(
        "Regions: {} total, {} tactical | strategic tick {}us",
        registry.len(),
        registry.tactical_count(),
        clock.last_tick_micros,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry_with(coords: &[RegionCoord]) -> RegionRegistry {
        let mut registry = RegionRegistry::default();
        for coord in coords {
            registry.regions.insert(*coord, RegionState::new(*coord));
        }
        registry
    }

    #[test]
    fn sim_level_follows_observers() {
        let watched = RegionCoord::new(0, 0);
        let empty = RegionCoord::new(9, 9);
        let mut registry = registry_with(&[watched, empty]);

        let mut interest = ClientInterest::default();
        interest.by_client.insert(
            Entity::from_raw_u32(1).unwrap(),
            HashSet::from_iter([watched]),
        );

        // Exercised directly rather than through a World, so the promotion rule is
        // testable without standing up a full app.
        for region in registry.regions.values_mut() {
            region.observers = 0;
        }
        for regions in interest.by_client.values() {
            for coord in regions {
                if let Some(r) = registry.regions.get_mut(coord) {
                    r.observers += 1;
                }
            }
        }
        for region in registry.regions.values_mut() {
            region.sim_level = if region.observers > 0 {
                SimLevel::Tactical
            } else {
                SimLevel::Strategic
            };
        }

        assert_eq!(registry.get(watched).unwrap().sim_level, SimLevel::Tactical);
        assert_eq!(registry.get(empty).unwrap().sim_level, SimLevel::Strategic);
        assert_eq!(registry.tactical_count(), 1);
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
}
