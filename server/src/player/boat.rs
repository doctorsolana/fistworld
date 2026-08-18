//! Server-authoritative opening voyage.
//!
//! A new player creates one hero and receives one small boat at a validated
//! water point on the edge of the active map. Boats have a deliberately
//! separate navigator from land characters: a water route can never inherit a
//! village road shortcut or terrain snap and accidentally climb onto shore.

use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::{MessageReceiver, NetworkTarget, RemoteId, Replicate};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, VecDeque};

use shared::components::{
    AboardBoat, CharacterActivity, CharacterMotion, CloudSeed, CommandedBy, PlayerBoat,
    PlayerPosition, PlayerRotation, Vessel, WorldTime, WreckedVessel,
};
use shared::protocol::{CreateHero, DisembarkBoat};
use shared::region::RegionCoord;
use shared::terrain::WorldTerrain;

use crate::player::hero::{HeroIndex, OfflineHero};

const BOAT_SPEED: f32 = 7.0;
const BOAT_ARRIVE_EPSILON: f32 = 0.2;
const NAV_CELL: f32 = 6.0;
const NAV_MAX_EXPANDED: usize = 60_000;
const NAV_ROUTES_PER_TICK: usize = 4;
const COAST_SCAN_STEP: f32 = 8.0;
// Candidate corridors still begin at the map edge, but the actual opening
// shot starts within a short, readable voyage of reachable shore rather than
// either several minutes of featureless ocean or almost on the beach.
const EDGE_INSET: f32 = 64.0;
const START_OFFSHORE_DISTANCE: f32 = 56.0;
const MIN_EDGE_APPROACH: f32 = START_OFFSHORE_DISTANCE + COAST_SCAN_STEP;
const MAX_DISEMBARK_DISTANCE: f32 = 11.0;
const HELM_LOCAL: Vec3 = Vec3::new(0.0, 0.35, 1.24);

/// Retained route on one player boat. Waypoints are world XZ positions whose
/// water occupancy was certified when the order was accepted.
#[derive(Component, Debug, Clone)]
pub struct VesselRoute {
    pub(crate) waypoints: Vec<Vec2>,
    pub(crate) next: usize,
}

/// Per-hull movement parameters. Future merchant and war vessels use this
/// same navigator with their own base speeds instead of branching pathfinding.
#[derive(Component, Debug, Clone, Copy)]
pub struct VesselNavigation {
    hull_speed: f32,
}

/// Bounded planning ingress for every vessel class. Route searches are never
/// run in the network-message loop: a player ordering a fleet cannot turn one
/// server tick into hundreds of A* searches. Re-ordering the same vessel
/// replaces its stale pending destination.
#[derive(Resource, Default)]
pub struct VesselNavigationQueue {
    pending: VecDeque<(Entity, Vec2)>,
}

impl VesselNavigationQueue {
    pub(crate) fn request(&mut self, vessel: Entity, goal: Vec2) {
        self.pending.retain(|(queued, _)| *queued != vessel);
        self.pending.push_back((vessel, goal));
    }
}

impl VesselNavigation {
    const DINGHY: Self = Self {
        hull_speed: BOAT_SPEED,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct WaterCell {
    x: i32,
    z: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OpenWaterCell {
    cost: i32,
    cell: WaterCell,
}

impl Ord for OpenWaterCell {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .cmp(&self.cost)
            .then_with(|| self.cell.x.cmp(&other.cell.x))
            .then_with(|| self.cell.z.cmp(&other.cell.z))
    }
}

impl PartialOrd for OpenWaterCell {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn stable_account_seed(account: &str) -> u64 {
    // FNV-1a is sufficient here and, unlike DefaultHasher, stable across Rust
    // versions and processes. A reconnect therefore keeps the same candidate.
    account.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3)
    })
}

fn water_at(terrain: &WorldTerrain, point: Vec2) -> Option<f32> {
    let bounds = terrain.generator.active_map_bounds();
    bounds
        .contains_xz(point.x, point.y)
        .then(|| terrain.get_water_height(point.x, point.y))
        .flatten()
}

fn dry_at(terrain: &WorldTerrain, point: Vec2) -> bool {
    let bounds = terrain.generator.active_map_bounds();
    bounds.contains_xz(point.x, point.y) && water_at(terrain, point).is_none()
}

fn segment_is_water(terrain: &WorldTerrain, start: Vec2, end: Vec2) -> bool {
    let distance = start.distance(end);
    let steps = (distance / (NAV_CELL * 0.5)).ceil().max(1.0) as usize;
    (0..=steps).all(|step| {
        let point = start.lerp(end, step as f32 / steps as f32);
        water_at(terrain, point).is_some()
    })
}

fn edge_candidate(terrain: &WorldTerrain, side: usize, t: f32) -> Option<(Vec3, Vec2)> {
    let bounds = terrain.generator.active_map_bounds();
    let t = t.clamp(0.04, 0.96);
    let (start, inward, max_scan) = match side % 4 {
        0 => (
            Vec2::new(
                bounds.min[0] + EDGE_INSET,
                bounds.min[1] + bounds.depth() * t,
            ),
            Vec2::X,
            bounds.width() * 0.6,
        ),
        1 => (
            Vec2::new(
                bounds.max[0] - EDGE_INSET,
                bounds.min[1] + bounds.depth() * t,
            ),
            Vec2::NEG_X,
            bounds.width() * 0.6,
        ),
        2 => (
            Vec2::new(
                bounds.min[0] + bounds.width() * t,
                bounds.min[1] + EDGE_INSET,
            ),
            Vec2::Y,
            bounds.depth() * 0.6,
        ),
        _ => (
            Vec2::new(
                bounds.min[0] + bounds.width() * t,
                bounds.max[1] - EDGE_INSET,
            ),
            Vec2::NEG_Y,
            bounds.depth() * 0.6,
        ),
    };
    water_at(terrain, start)?;
    let mut distance = COAST_SCAN_STEP;
    let mut shore = None;
    while distance <= max_scan {
        let point = start + inward * distance;
        if dry_at(terrain, point) {
            shore = Some(point);
            break;
        }
        // If an authored map has a water gap, do not pretend the inland dry
        // point is reachable through a land bridge.
        if water_at(terrain, point).is_none() {
            break;
        }
        distance += COAST_SCAN_STEP;
    }
    let shore = shore?;
    let approach_distance = start.distance(shore);
    if approach_distance < MIN_EDGE_APPROACH {
        return None;
    }
    let inward = (shore - start).normalize_or_zero();
    let voyage_start = shore - inward * START_OFFSHORE_DISTANCE;
    let water = water_at(terrain, voyage_start)?;
    segment_is_water(terrain, start, voyage_start)
        .then_some((Vec3::new(voyage_start.x, water, voyage_start.y), inward))
}

/// Pick a deterministic-looking edge start without trusting the client. The
/// account hash changes the first side and sample, while a complete fallback
/// scan guarantees that an unlucky first choice does not reject a valid map.
fn starting_voyage(terrain: &WorldTerrain, account: &str) -> Option<(Vec3, f32)> {
    let seed = stable_account_seed(account);
    let first_side = (seed & 3) as usize;
    let first_sample = ((seed >> 8) % 48) as usize;
    for side_offset in 0..4 {
        let side = (first_side + side_offset) % 4;
        for sample_offset in 0..48 {
            let sample = (first_sample + sample_offset * 17) % 48;
            let t = (sample as f32 + 0.5) / 48.0;
            if let Some((position, inward)) = edge_candidate(terrain, side, t) {
                // Authored -Z is the bow. This yaw points -Z along `inward`.
                let yaw = f32::atan2(-inward.x, -inward.y);
                return Some((position, yaw));
            }
        }
    }
    None
}

fn to_cell(point: Vec2) -> WaterCell {
    WaterCell {
        x: (point.x / NAV_CELL).round() as i32,
        z: (point.y / NAV_CELL).round() as i32,
    }
}

fn from_cell(cell: WaterCell) -> Vec2 {
    Vec2::new(cell.x as f32 * NAV_CELL, cell.z as f32 * NAV_CELL)
}

fn nearest_connected_water_cell(terrain: &WorldTerrain, point: Vec2) -> Option<WaterCell> {
    let center = to_cell(point);
    let mut candidates = Vec::with_capacity(25);
    for dx in -2..=2 {
        for dz in -2..=2 {
            let cell = WaterCell {
                x: center.x + dx,
                z: center.z + dz,
            };
            candidates.push((point.distance_squared(from_cell(cell)), cell));
        }
    }
    candidates.sort_by(|(a, _), (b, _)| a.total_cmp(b));
    candidates.into_iter().find_map(|(_, cell)| {
        let cell_point = from_cell(cell);
        (water_at(terrain, cell_point).is_some() && segment_is_water(terrain, point, cell_point))
            .then_some(cell)
    })
}

fn water_heuristic(a: WaterCell, b: WaterCell) -> f32 {
    let delta = Vec2::new((a.x - b.x) as f32, (a.z - b.z) as f32);
    delta.length()
}

/// Water-only A* with a direct-line fast path. It is intentionally computed on
/// command, not every tick; even a long opening sail pays for one search.
pub(crate) fn water_route(terrain: &WorldTerrain, start: Vec2, goal: Vec2) -> Option<Vec<Vec2>> {
    water_at(terrain, start)?;
    water_at(terrain, goal)?;
    if segment_is_water(terrain, start, goal) {
        return Some(vec![goal]);
    }

    // A perfectly valid click can round to a grid centre just over the shore.
    // Connect both exact endpoints to their nearest visible water cell rather
    // than rejecting the order because of that discretization accident.
    let start_cell = nearest_connected_water_cell(terrain, start)?;
    let goal_cell = nearest_connected_water_cell(terrain, goal)?;
    let mut open = BinaryHeap::new();
    let mut came_from = HashMap::new();
    let mut score = HashMap::new();
    let mut closed = HashSet::new();
    score.insert(start_cell, 0.0_f32);
    open.push(OpenWaterCell {
        cost: (water_heuristic(start_cell, goal_cell) * 1_000.0) as i32,
        cell: start_cell,
    });
    let neighbours = [
        (-1, -1),
        (0, -1),
        (1, -1),
        (-1, 0),
        (1, 0),
        (-1, 1),
        (0, 1),
        (1, 1),
    ];
    let mut expanded = 0;
    while let Some(OpenWaterCell { cell, .. }) = open.pop() {
        if !closed.insert(cell) {
            continue;
        }
        expanded += 1;
        if expanded > NAV_MAX_EXPANDED {
            return None;
        }
        if cell == goal_cell {
            let mut cells = vec![cell];
            let mut cursor = cell;
            while let Some(previous) = came_from.get(&cursor).copied() {
                cells.push(previous);
                cursor = previous;
            }
            cells.reverse();
            let mut route: Vec<Vec2> = cells.into_iter().skip(1).map(from_cell).collect();
            route.push(goal);
            // Remove unnecessary stair-steps without ever cutting across land.
            let mut simplified = Vec::new();
            let mut anchor = start;
            let mut index = 0;
            while index < route.len() {
                let mut furthest = index;
                for candidate in (index..route.len()).rev() {
                    if segment_is_water(terrain, anchor, route[candidate]) {
                        furthest = candidate;
                        break;
                    }
                }
                anchor = route[furthest];
                simplified.push(anchor);
                index = furthest + 1;
            }
            return Some(simplified);
        }
        let current_score = score.get(&cell).copied().unwrap_or(f32::INFINITY);
        for (dx, dz) in neighbours {
            let next = WaterCell {
                x: cell.x + dx,
                z: cell.z + dz,
            };
            if closed.contains(&next) {
                continue;
            }
            let next_world = from_cell(next);
            if water_at(terrain, next_world).is_none()
                || !segment_is_water(terrain, from_cell(cell), next_world)
            {
                continue;
            }
            let step = if dx != 0 && dz != 0 {
                std::f32::consts::SQRT_2
            } else {
                1.0
            };
            let tentative = current_score + step;
            if tentative >= score.get(&next).copied().unwrap_or(f32::INFINITY) {
                continue;
            }
            came_from.insert(next, cell);
            score.insert(next, tentative);
            open.push(OpenWaterCell {
                cost: ((tentative + water_heuristic(next, goal_cell)) * 1_000.0) as i32,
                cell: next,
            });
        }
    }
    None
}

/// Spend a fixed amount of route-planning work per server tick. Direct open
/// water orders remain practically immediate, while complex coast searches
/// are naturally spread out under fleet-scale command bursts.
pub fn plan_vessel_routes(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    mut queue: ResMut<VesselNavigationQueue>,
    vessels: Query<&PlayerPosition, With<Vessel>>,
) {
    for _ in 0..NAV_ROUTES_PER_TICK {
        let Some((vessel, goal)) = queue.pending.pop_front() else {
            break;
        };
        let Ok(position) = vessels.get(vessel) else {
            continue;
        };
        let start = position.0.xz();
        if let Some(waypoints) = water_route(&terrain, start, goal) {
            commands
                .entity(vessel)
                .insert(VesselRoute { waypoints, next: 0 });
        }
    }
}

/// Normal (non-god) hero creation. Position is selected from the active map,
/// and the hero starts seated at the boat's authored helm.
#[allow(clippy::too_many_arguments)]
pub fn handle_create_hero_requests(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    profiles: Res<crate::persistence::profiles::PlayerProfiles>,
    mut hero_index: ResMut<HeroIndex>,
    heroes: Query<&shared::components::Hero>,
    boats: Query<&CommandedBy, With<PlayerBoat>>,
    mut clients: Query<(&RemoteId, &mut MessageReceiver<CreateHero>), With<ClientOf>>,
) {
    for (remote, mut receiver) in clients.iter_mut() {
        for request in receiver.receive() {
            let Some(account) = profiles.peer_to_name.get(&remote.0).cloned() else {
                continue;
            };
            if hero_index.by_name.contains_key(&account)
                || heroes.iter().any(|hero| hero.owner == remote.0)
                || boats.iter().any(|owner| owner.0 == account)
            {
                continue;
            }
            let Some((position, yaw)) = starting_voyage(&terrain, &account) else {
                warn!(
                    "Cannot create hero for '{account}': active map has no reachable edge voyage"
                );
                continue;
            };
            let profile = profiles.profiles.get(&account).cloned();
            let display_name = profile
                .as_ref()
                .map(|profile| profile.player_name.as_str())
                .unwrap_or(account.as_str());
            let hero = crate::player::hero::spawn_hero(
                &mut commands,
                &mut hero_index,
                &terrain,
                remote.0,
                &account,
                display_name,
                position,
                yaw,
                request.outfit,
                profile
                    .as_ref()
                    .map(|profile| profile.character_attributes())
                    .unwrap_or_default(),
                shared::components::Health::default(),
            );
            // `spawn_hero` terrain-snaps ordinary land starts. Override that
            // here with the exact waterline/helm position in the same command
            // queue; the latter insert is authoritative.
            commands.entity(hero).insert((
                AboardBoat,
                CharacterActivity::Sitting,
                PlayerPosition(position),
                RegionCoord::from_world_pos(position),
            ));
            let boat = commands
                .spawn((
                    PlayerBoat,
                    Vessel,
                    VesselNavigation::DINGHY,
                    CommandedBy(account.clone()),
                    PlayerPosition(position),
                    PlayerRotation(yaw),
                    CharacterMotion::STATIONARY,
                    RegionCoord::from_world_pos(position),
                    Replicate::to_clients(NetworkTarget::All),
                ))
                .id();
            info!("Opening voyage created for '{account}': hero={hero:?} boat={boat:?} at={position:?}");
        }
    }
}

/// Advance every active boat along its certified water route.
pub fn step_boats(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    simulation_time: crate::world::simulation_time::SimulationTime,
    world_time: Query<&WorldTime>,
    cloud_seed: Query<&CloudSeed>,
    mut boats: Query<
        (
            Entity,
            &mut VesselRoute,
            &VesselNavigation,
            &mut PlayerPosition,
            &mut PlayerRotation,
            &mut RegionCoord,
            &mut CharacterMotion,
        ),
        With<Vessel>,
    >,
) {
    let dt = simulation_time.world_seconds();
    let real_dt = simulation_time.real_seconds().max(1.0e-5);
    let absolute_seconds = world_time.iter().next().map_or(0.0, |clock| {
        clock.day as f32 * clock.cycle_duration() + clock.seconds_in_cycle
    });
    let seed_phase =
        shared::wind::wind_seed_phase(cloud_seed.iter().next().map_or(0, |seed| seed.seed));
    let downwind = shared::wind::wind_direction(absolute_seconds, seed_phase);
    let (_, wind_speed) = shared::wind::wind_state(absolute_seconds, seed_phase);
    for (entity, mut route, navigation, mut position, mut rotation, mut region, mut motion) in
        boats.iter_mut()
    {
        let before = position.0;
        let mut current = Vec2::new(before.x, before.z);
        let mut remaining_seconds = dt;
        let mut direction = Vec2::ZERO;
        while remaining_seconds > 1.0e-5 && route.next < route.waypoints.len() {
            let goal = route.waypoints[route.next];
            let delta = goal - current;
            let distance = delta.length();
            if distance <= BOAT_ARRIVE_EPSILON {
                current = goal;
                route.next += 1;
                continue;
            }
            direction = delta / distance;
            let speed = navigation.hull_speed
                * shared::wind::sailing_speed_multiplier(direction, downwind, wind_speed);
            let step = (speed * remaining_seconds).min(distance);
            let proposed = if step + BOAT_ARRIVE_EPSILON >= distance {
                goal
            } else {
                current + direction * step
            };
            // World changes can invalidate an old route. Stop at the last
            // valid water point; never let a stale plan beach the vessel.
            if water_at(&terrain, proposed).is_none()
                || !segment_is_water(&terrain, current, proposed)
            {
                commands.entity(entity).remove::<VesselRoute>();
                if motion.is_moving() {
                    *motion = CharacterMotion::STATIONARY;
                }
                break;
            }
            current = proposed;
            remaining_seconds -= step / speed;
            if step + BOAT_ARRIVE_EPSILON >= distance {
                route.next += 1;
            }
        }
        let water_y = water_at(&terrain, current).unwrap_or(before.y);
        let next = Vec3::new(current.x, water_y, current.y);
        if position.0 != next {
            position.0 = next;
        }
        if direction != Vec2::ZERO {
            let yaw = f32::atan2(-direction.x, -direction.y);
            if rotation.0 != yaw {
                rotation.0 = yaw;
            }
        }
        let next_region = RegionCoord::from_world_pos(next);
        if *region != next_region {
            *region = next_region;
        }
        let arrived = route.next >= route.waypoints.len();
        let next_motion = if arrived {
            CharacterMotion::STATIONARY
        } else {
            CharacterMotion::new((next - before) / real_dt)
        };
        if *motion != next_motion {
            *motion = next_motion;
        }
        if arrived {
            commands.entity(entity).remove::<VesselRoute>();
        }
    }
}

/// Pin seated heroes to the authored helm after boat movement and before
/// replication. Matching by stable account avoids a fragile persisted Entity
/// reference and naturally survives reconnects.
pub fn sync_aboard_heroes(
    boats: Query<
        (
            &CommandedBy,
            &PlayerPosition,
            &PlayerRotation,
            &CharacterMotion,
        ),
        With<PlayerBoat>,
    >,
    mut heroes: Query<
        (
            &CommandedBy,
            &mut PlayerPosition,
            &mut PlayerRotation,
            &mut RegionCoord,
            &mut CharacterMotion,
            &mut CharacterActivity,
        ),
        (With<AboardBoat>, Without<PlayerBoat>, Without<OfflineHero>),
    >,
) {
    for (owner, mut position, mut rotation, mut region, mut motion, mut activity) in
        heroes.iter_mut()
    {
        let Some((_, boat_position, boat_rotation, boat_motion)) = boats
            .iter()
            .find(|(boat_owner, ..)| boat_owner.0 == owner.0)
        else {
            continue;
        };
        let helm = boat_position.0 + Quat::from_rotation_y(boat_rotation.0) * HELM_LOCAL;
        if position.0 != helm {
            position.0 = helm;
        }
        if rotation.0 != boat_rotation.0 {
            rotation.0 = boat_rotation.0;
        }
        let next_region = RegionCoord::from_world_pos(helm);
        if *region != next_region {
            *region = next_region;
        }
        if *motion != *boat_motion {
            *motion = *boat_motion;
        }
        if *activity != CharacterActivity::Sitting {
            *activity = CharacterActivity::Sitting;
        }
    }
}

/// Validate nearby dry land and transition the sender's hero to ordinary
/// on-foot play. The boat remains moored and owned for later boat gameplay.
pub fn handle_disembark_requests(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    profiles: Res<crate::persistence::profiles::PlayerProfiles>,
    mut clients: Query<(&RemoteId, &mut MessageReceiver<DisembarkBoat>), With<ClientOf>>,
    // A boat and a hero both carry PlayerPosition. Tell Bevy that these two
    // queries are disjoint so the mutable hero position below is legal. This
    // filter is also a useful invariant: a vessel must never double as its
    // occupant entity.
    boats: Query<
        (&CommandedBy, &PlayerPosition),
        (With<PlayerBoat>, Without<shared::components::Hero>),
    >,
    mut heroes: Query<
        (
            Entity,
            &CommandedBy,
            &mut PlayerPosition,
            &mut PlayerRotation,
            &mut RegionCoord,
            &mut CharacterMotion,
            &mut CharacterActivity,
        ),
        (
            With<shared::components::Hero>,
            With<AboardBoat>,
            Without<OfflineHero>,
        ),
    >,
) {
    for (remote, mut receiver) in clients.iter_mut() {
        let account = profiles.peer_to_name.get(&remote.0).cloned();
        for request in receiver.receive() {
            let Some(account) = account.as_deref() else {
                continue;
            };
            if request.boat == Entity::PLACEHOLDER || !request.landing.is_finite() {
                continue;
            }
            let Ok((boat_owner, boat_position)) = boats.get(request.boat) else {
                continue;
            };
            if boat_owner.0 != account {
                continue;
            }
            let landing_xz = Vec2::new(request.landing.x, request.landing.z);
            let boat_xz = Vec2::new(boat_position.0.x, boat_position.0.z);
            if boat_xz.distance(landing_xz) > MAX_DISEMBARK_DISTANCE
                || !dry_at(&terrain, landing_xz)
            {
                continue;
            }
            let Some((
                hero_entity,
                _,
                mut position,
                mut rotation,
                mut region,
                mut motion,
                mut activity,
            )) = heroes.iter_mut().find(|(_, owner, ..)| owner.0 == account)
            else {
                continue;
            };
            let ground = Vec3::new(
                landing_xz.x,
                terrain.get_height(landing_xz.x, landing_xz.y),
                landing_xz.y,
            );
            let direction = landing_xz - boat_xz;
            position.0 = ground;
            if direction.length_squared() > 1.0e-4 {
                rotation.0 = f32::atan2(-direction.x, -direction.y);
            }
            *region = RegionCoord::from_world_pos(ground);
            *motion = CharacterMotion::STATIONARY;
            *activity = CharacterActivity::Idle;
            commands.entity(hero_entity).remove::<AboardBoat>();
            commands
                .entity(request.boat)
                .insert(WreckedVessel)
                .remove::<Vessel>()
                .remove::<VesselNavigation>()
                .remove::<VesselRoute>();
            info!("'{account}' disembarked at {ground:?}; the starter dinghy is now wrecked");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disembark_system_queries_remain_disjoint() {
        // Bevy validates query aliasing only when a schedule/system is
        // initialized, so `cargo check` cannot catch this class of startup
        // crash. Keep the exact runtime system under that validation here.
        let mut world = World::new();
        let mut system = IntoSystem::into_system(handle_disembark_requests);
        system.initialize(&mut world);
    }

    #[test]
    fn account_start_seed_is_stable_and_name_sensitive() {
        assert_eq!(stable_account_seed("hilda"), stable_account_seed("hilda"));
        assert_ne!(stable_account_seed("hilda"), stable_account_seed("alwin"));
    }

    #[test]
    fn boat_yaw_points_authored_negative_z_along_travel() {
        for direction in [Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y] {
            let yaw = f32::atan2(-direction.x, -direction.y);
            let forward = (Quat::from_rotation_y(yaw) * Vec3::NEG_Z).xz().normalize();
            assert!(forward.distance(direction) < 1.0e-5);
        }
    }

    #[test]
    fn active_world_offers_a_water_start_with_reachable_land() {
        let terrain = WorldTerrain::default();
        let (start, yaw) = starting_voyage(&terrain, "voyage-test")
            .expect("the active gameplay map must expose at least one coastal start");
        assert!(water_at(&terrain, start.xz()).is_some());

        let inward = (Quat::from_rotation_y(yaw) * Vec3::NEG_Z).xz().normalize();
        let search_steps = ((START_OFFSHORE_DISTANCE + COAST_SCAN_STEP) / 0.5).ceil() as usize;
        let dry_distance = (1..=search_steps)
            .map(|step| step as f32 * 0.5)
            .find(|distance| dry_at(&terrain, start.xz() + inward * *distance))
            .expect("the opening heading must reach its validated shore");
        assert!(
            dry_distance >= START_OFFSHORE_DISTANCE - COAST_SCAN_STEP - 0.5,
            "voyage began almost on dry land: {dry_distance:.1}m"
        );
        assert!(
            dry_distance <= START_OFFSHORE_DISTANCE + 0.5,
            "voyage still begins too far offshore: {dry_distance:.1}m"
        );
    }

    #[test]
    fn water_route_rejects_dry_destinations() {
        let terrain = WorldTerrain::default();
        let (start, _) = starting_voyage(&terrain, "route-test")
            .expect("the active gameplay map must expose at least one coastal start");
        let bounds = terrain.generator.active_map_bounds();
        let dry = (0..=32)
            .flat_map(|x| (0..=32).map(move |z| (x, z)))
            .map(|(x, z)| {
                Vec2::new(
                    bounds.min[0] + bounds.width() * x as f32 / 32.0,
                    bounds.min[1] + bounds.depth() * z as f32 / 32.0,
                )
            })
            .find(|point| dry_at(&terrain, *point))
            .expect("the active gameplay map must contain dry land");
        assert!(water_route(&terrain, start.xz(), dry).is_none());
    }

    #[test]
    fn latest_pending_order_replaces_an_older_destination() {
        let mut queue = VesselNavigationQueue::default();
        let vessel = Entity::from_bits(42);
        queue.request(vessel, Vec2::ONE);
        queue.request(vessel, Vec2::splat(2.0));
        assert_eq!(queue.pending.len(), 1);
        assert_eq!(queue.pending.front(), Some(&(vessel, Vec2::splat(2.0))));
    }
}
